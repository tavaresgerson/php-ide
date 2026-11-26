//! The editor widget: virtualized painting, input, gutter, sticky lines,
//! scrollbars and the find bar.

use std::time::{Duration, Instant};

use eframe::egui::{
    self, epaint::PathShape, pos2, vec2, Color32, CornerRadius, Event, Key, Modifiers, Pos2, Rect,
    Response, Sense, Stroke,
};
use phpide_core::Buffer;
use ropey::Rope;

use super::folds::DisplayMap;
use super::layout::{LineInput, LineLayout};
use super::minimap::{self, MinimapGeometry, MinimapOptions, MinimapSlider, BLOCK_LINES};
use super::{DragKind, EditorView, Reveal, ViewCommand};
use crate::decorations::{BreakpointMark, CoverageMark, Decorations, DiagnosticSeverity, GitMark};
use crate::edit;
use crate::movement::{move_selection, Motion};
use crate::selection::{Range, Selection};
use crate::style::EditorStyle;
use crate::syntax::Syntax;
use crate::textutil::{line_end, line_text, word_at};

const SCROLLBAR_WIDTH: f32 = 12.0;
const TEXT_PADDING: f32 = 6.0;
const BLINK: Duration = Duration::from_millis(530);
const STICKY_KINDS: [&str; 11] = [
    "class_declaration",
    "interface_declaration",
    "trait_declaration",
    "enum_declaration",
    "method_declaration",
    "function_definition",
    "if_statement",
    "foreach_statement",
    "for_statement",
    "while_statement",
    "switch_statement",
];

#[derive(Debug, Clone, PartialEq)]
pub struct EditorOptions {
    pub line_numbers: bool,
    pub folding: bool,
    /// Show fold markers on every foldable line, not only on hover.
    pub fold_markers_always: bool,
    /// Maximum sticky lines (0 turns them off).
    pub sticky_lines: usize,
    pub indent_guides: bool,
    pub blink: bool,
    /// Show the message of the worst diagnostic at the end of its line.
    pub inline_diagnostics: bool,
    /// Tooltip with the diagnostic under the pointer (off when the app
    /// shows its own documentation popup).
    pub diagnostic_tooltips: bool,
    pub minimap: MinimapOptions,
}

impl Default for EditorOptions {
    fn default() -> Self {
        EditorOptions {
            line_numbers: true,
            folding: true,
            fold_markers_always: false,
            sticky_lines: 5,
            indent_guides: true,
            blink: true,
            inline_diagnostics: true,
            diagnostic_tooltips: true,
            minimap: MinimapOptions::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GutterClick {
    /// Click on a line number (toggle a breakpoint there).
    LineNumber(usize),
}

/// Where a right click landed, for the host's context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextTarget {
    /// In the text, at this char (the caret is already there, unless the
    /// click fell inside the selection, which is kept).
    Text(usize),
    /// On the gutter, at this line.
    Gutter(usize),
}

#[derive(Default)]
pub struct EditorOutput {
    pub response: Option<Response>,
    /// The document changed this frame (in this view).
    pub edited: bool,
    /// Text typed this frame (completion triggers).
    pub typed: String,
    pub selection_changed: bool,
    pub gutter_click: Option<GutterClick>,
    /// Ctrl+click on a char (go to declaration).
    pub ctrl_click: Option<usize>,
    /// Click on a code lens: its index in the decorations.
    pub lens_click: Option<usize>,
    /// Right click this frame: the host opens its context menu.
    pub context: Option<ContextTarget>,
    /// Char under the pointer in the text area.
    pub hover: Option<usize>,
    /// Screen rect of the primary caret (completion popup anchor).
    pub caret_rect: Option<Rect>,
    /// Line under the pointer on the vertical scrollbar.
    pub scrollbar_hover_line: Option<usize>,
    /// The user tried to edit a read-only buffer.
    pub blocked_edit: bool,
    pub focused: bool,
    pub first_visible_line: usize,
}

/// One pane of code. Build it every frame and call [`Editor::show`].
pub struct Editor<'a> {
    id: egui::Id,
    buffer: &'a mut Buffer,
    view: &'a mut EditorView,
    syntax: &'a mut Syntax,
    style: &'a EditorStyle,
    decorations: &'a Decorations<'a>,
    options: EditorOptions,
}

struct Geometry {
    rect: Rect,
    text: Rect,
    gutter: Rect,
    row_height: f32,
    char_width: f32,
    text_left: f32,
    coverage_x: f32,
    blame: Option<(f32, f32)>,
    numbers: (f32, f32),
    git_x: f32,
    fold_x: Option<f32>,
    vbar: Rect,
    /// The minimap column, between the text and the scrollbar.
    minimap: Option<Rect>,
}

/// Columns of `[start, end)` that fall on `line`, given its bounds.
fn columns_on_line(
    start: usize,
    end: usize,
    line_start: usize,
    line_end: usize,
) -> Option<(usize, usize)> {
    if end < line_start || start > line_end {
        return None;
    }
    Some((
        start.max(line_start) - line_start,
        end.min(line_end) - line_start,
    ))
}

fn wavy(painter: &egui::Painter, from: f32, to: f32, y: f32, color: Color32) {
    let mut points = Vec::new();
    let mut x = from;
    let mut up = true;
    while x <= to.max(from + 4.0) {
        points.push(pos2(x, if up { y - 1.5 } else { y + 1.0 }));
        x += 2.5;
        up = !up;
    }
    if points.len() > 1 {
        painter.add(PathShape::line(points, Stroke::new(1.0, color)));
    }
}

fn chevron(painter: &egui::Painter, center: Pos2, size: f32, open: bool, color: Color32) {
    let s = size * 0.5;
    let points = if open {
        vec![
            pos2(center.x - s, center.y - s * 0.45),
            pos2(center.x, center.y + s * 0.55),
            pos2(center.x + s, center.y - s * 0.45),
        ]
    } else {
        vec![
            pos2(center.x - s * 0.45, center.y - s),
            pos2(center.x + s * 0.55, center.y),
            pos2(center.x - s * 0.45, center.y + s),
        ]
    };
    painter.add(PathShape::line(points, Stroke::new(1.4, color)));
}

impl<'a> Editor<'a> {
    pub fn new(
        id_salt: impl std::hash::Hash + std::fmt::Debug,
        buffer: &'a mut Buffer,
        view: &'a mut EditorView,
        syntax: &'a mut Syntax,
        style: &'a EditorStyle,
        decorations: &'a Decorations<'a>,
    ) -> Self {
        Editor {
            id: egui::Id::new(("phpide-editor", id_salt)),
            buffer,
            view,
            syntax,
            style,
            decorations,
            options: EditorOptions::default(),
        }
    }

    pub fn options(mut self, options: EditorOptions) -> Self {
        self.options = options;
        self
    }

    /// The id the widget registers (focus requests).
    pub fn widget_id(id_salt: impl std::hash::Hash + std::fmt::Debug) -> egui::Id {
        egui::Id::new(("phpide-editor", id_salt))
    }

    fn layout_line(&mut self, ctx: &egui::Context, style_key: u64, line: usize) -> LineLayout {
        let rope = self.buffer.document.rope();
        let text = line_text(rope, line);
        let spans = self.syntax.line_spans(&self.buffer.document, line, line);
        let line_start = rope.line_to_char(line);
        let line_end = line_start + text.chars().count();
        let deco = self.decorations;
        let first = deco.semantic.partition_point(|t| t.1 <= line_start);
        let semantic: Vec<(usize, usize, phpide_php::SyntaxRole)> = deco.semantic[first..]
            .iter()
            .take_while(|t| t.0 < line_end)
            .map(|(s, e, r)| {
                (
                    (*s).max(line_start) - line_start,
                    (*e).min(line_end) - line_start,
                    *r,
                )
            })
            .collect();
        let hints: Vec<(usize, &str)> = deco
            .inlay_hints
            .iter()
            .filter(|h| h.position >= line_start && h.position <= line_end)
            .map(|h| (h.position - line_start, h.label.as_str()))
            .collect();
        let mut hints = hints;
        hints.sort_by_key(|h| h.0);
        let input = LineInput {
            text: &text,
            spans: spans.first().map(|s| s.as_slice()).unwrap_or(&[]),
            semantic: &semantic,
            hints: &hints,
            tab_size: self.view.config.tab_size,
        };
        self.view.layouts.layout(ctx, self.style, style_key, &input)
    }

    fn geometry(&self, ui: &egui::Ui, rect: Rect) -> Geometry {
        let font = self.style.font().clone();
        let (row, char_width, gutter_char) = ui.ctx().fonts_mut(|f| {
            (
                f.row_height(&font),
                f.glyph_width(&font, 'M'),
                f.glyph_width(&self.style.gutter_font, '0'),
            )
        });
        let row_height = (row * self.style.line_spacing).round().max(row);
        let lines = self.buffer.document.line_count().max(1);
        let digits = (lines as f32).log10().floor() as usize + 1;
        let mut x = rect.left();
        let coverage_x = x;
        if !self.decorations.coverage.is_empty() {
            x += 4.0;
        }
        let blame = self.decorations.blame.map(|_| {
            let w = gutter_char * 22.0;
            let span = (x + 4.0, x + 4.0 + w);
            x += w + 8.0;
            span
        });
        let numbers_w = if self.options.line_numbers {
            digits.max(2) as f32 * gutter_char + 22.0
        } else {
            18.0
        };
        let numbers = (x, x + numbers_w);
        x += numbers_w;
        let git_x = x;
        x += 6.0;
        let fold_x = self.options.folding.then(|| {
            let center = x + 7.0;
            x += 14.0;
            center
        });
        x += 2.0;
        let vbar = Rect::from_min_max(pos2(rect.right() - SCROLLBAR_WIDTH, rect.top()), rect.max);
        // The minimap gives way when the pane is too narrow for text.
        let minimap_width = self.options.minimap.width.round().max(8.0);
        let minimap = (self.options.minimap.enabled && vbar.left() - x > minimap_width + 160.0)
            .then(|| {
                Rect::from_min_max(
                    pos2(vbar.left() - minimap_width, rect.top()),
                    pos2(vbar.left(), rect.bottom()),
                )
            });
        let text_right = minimap.map_or(vbar.left(), |m| m.left());
        let text = Rect::from_min_max(pos2(x, rect.top()), pos2(text_right, rect.bottom()));
        Geometry {
            rect,
            gutter: Rect::from_min_max(rect.min, pos2(x, rect.bottom())),
            text,
            row_height,
            char_width,
            text_left: text.left() + TEXT_PADDING,
            coverage_x,
            blame,
            numbers,
            git_x,
            fold_x,
            vbar,
            minimap,
        }
    }

    /// The minimap column and what it shows this frame.
    fn minimap_geometry(&self, g: &Geometry, map: &DisplayMap) -> Option<(Rect, MinimapGeometry)> {
        let rect = g.minimap?;
        let rows = map.rows().max(1);
        let first_row = ((self.view.scroll.y / g.row_height).floor() as usize).min(rows - 1);
        let visible = (g.text.height() / g.row_height).floor() as usize;
        let first_line = map.line_of_row(first_row);
        let last_line = map.line_of_row((first_row + visible).min(rows - 1));
        let max_y = self.max_scroll_y(g, map);
        Some((
            rect,
            MinimapGeometry::new(
                rect.height(),
                self.buffer.document.line_count().max(1),
                self.view.scroll.y,
                max_y,
                first_line,
                last_line + 1 - first_line,
            ),
        ))
    }

    /// Draws the minimap: the blocks of text, the marks and the slider.
    fn paint_minimap(&mut self, ui: &egui::Ui, g: &Geometry, map: &DisplayMap) {
        let Some((rect, mg)) = self.minimap_geometry(g, map) else {
            return;
        };
        let ctx = ui.ctx().clone();
        let doc = &self.buffer.document;
        let first_block = mg.line_at(0.0) / BLOCK_LINES;
        let last_block = mg.line_at(rect.height()) / BLOCK_LINES;
        let source = self.syntax.highlight_source(doc);
        self.view.minimap.update(
            &ctx,
            doc,
            first_block..last_block + 1,
            minimap::BlockInputs {
                source,
                cols: rect.width() as usize,
                tab_size: self.view.config.tab_size,
                colors: &self.style.syntax.colors,
                render: self.options.minimap.render,
            },
        );
        let c = &self.style.colors;
        let painter = ui.painter_at(rect);
        let rope = doc.rope();
        let band = |line: usize, color: Color32| {
            let top = rect.top() + mg.line_top(line);
            painter.rect_filled(
                Rect::from_min_size(
                    pos2(rect.left(), top),
                    vec2(rect.width(), minimap::LINE_PITCH),
                ),
                CornerRadius::ZERO,
                color,
            );
        };
        let shown = |line: usize| {
            let top = mg.line_top(line);
            top + minimap::LINE_PITCH >= 0.0 && top <= rect.height()
        };
        // Search results and the selection under the text.
        if self.view.find.open {
            for (start, _) in self.view.find.matches.iter().take(5000) {
                let line = rope.char_to_line((*start).min(rope.len_chars()));
                if shown(line) {
                    band(line, c.search_match);
                }
            }
        }
        for range in self
            .view
            .selection
            .ranges()
            .iter()
            .filter(|r| !r.is_empty())
        {
            let a = rope.char_to_line(range.start().min(rope.len_chars()));
            let b = rope.char_to_line(range.end().min(rope.len_chars()));
            for line in a..=b.min(a + 5000) {
                if shown(line) {
                    band(line, c.selection);
                }
            }
        }
        for index in first_block..=last_block {
            if let Some(texture) = self.view.minimap.texture(index) {
                let size = texture.size_vec2();
                let top = rect.top() + mg.line_top(index * BLOCK_LINES);
                painter.image(
                    texture.id(),
                    Rect::from_min_size(pos2(rect.left(), top), size),
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        }
        // Git changes on the left edge, problems on the right one.
        for (line, git) in &self.decorations.git {
            if shown(*line) {
                let color = match git {
                    GitMark::Added => c.git_added,
                    GitMark::Modified => c.git_modified,
                    GitMark::Deleted => c.git_deleted,
                };
                painter.rect_filled(
                    Rect::from_min_size(
                        pos2(rect.left(), rect.top() + mg.line_top(*line)),
                        vec2(2.0, minimap::LINE_PITCH),
                    ),
                    CornerRadius::ZERO,
                    color,
                );
            }
        }
        for d in &self.decorations.diagnostics {
            let color = match d.severity {
                DiagnosticSeverity::Error => c.error,
                DiagnosticSeverity::Warning => c.warning,
                _ => continue,
            };
            let line = rope.char_to_line(d.start.min(rope.len_chars()));
            if shown(line) {
                painter.rect_filled(
                    Rect::from_min_size(
                        pos2(rect.right() - 3.0, rect.top() + mg.line_top(line) - 0.5),
                        vec2(3.0, minimap::LINE_PITCH + 1.0),
                    ),
                    CornerRadius::ZERO,
                    color,
                );
            }
        }
        let hovered = ui
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|p| rect.contains(p));
        let dragging = matches!(self.view.drag, Some(DragKind::Minimap { .. }));
        if self.options.minimap.slider == MinimapSlider::Always || hovered || dragging {
            let alpha = if hovered || dragging { 0.75 } else { 0.45 };
            painter.rect_filled(
                Rect::from_min_size(
                    pos2(rect.left(), rect.top() + mg.slider_top),
                    vec2(rect.width(), mg.slider_height),
                ),
                CornerRadius::ZERO,
                c.scrollbar_thumb.gamma_multiply(alpha),
            );
        }
    }

    fn max_scroll_y(&self, g: &Geometry, map: &DisplayMap) -> f32 {
        let content = map.rows() as f32 * g.row_height;
        (content - g.text.height() * 0.5)
            .max(0.0)
            .min((content - g.row_height).max(0.0))
    }

    fn pos_at(
        &mut self,
        ctx: &egui::Context,
        g: &Geometry,
        map: &DisplayMap,
        style_key: u64,
        point: Pos2,
    ) -> usize {
        let rows = map.rows().max(1);
        let row = ((point.y - g.text.top() + self.view.scroll.y) / g.row_height)
            .floor()
            .clamp(0.0, (rows - 1) as f32) as usize;
        let line = map.line_of_row(row);
        let layout = self.layout_line(ctx, style_key, line);
        let column = layout.column_at(point.x - g.text_left + self.view.scroll.x);
        let rope = self.buffer.document.rope();
        (rope.line_to_char(line) + column).min(line_end(rope, line))
    }

    fn line_cell_at(&self, g: &Geometry, map: &DisplayMap, point: Pos2) -> (usize, usize) {
        let rows = map.rows().max(1);
        let row = ((point.y - g.text.top() + self.view.scroll.y) / g.row_height)
            .floor()
            .clamp(0.0, (rows - 1) as f32) as usize;
        let cell = ((point.x - g.text_left + self.view.scroll.x) / g.char_width)
            .round()
            .max(0.0) as usize;
        (map.line_of_row(row), cell)
    }

    fn apply(&mut self, edit: edit::Edit, output: &mut EditorOutput) {
        if self.buffer.readonly {
            output.blocked_edit = true;
            return;
        }
        let changed = !edit.changes.is_empty();
        self.view.apply(self.buffer, edit);
        output.edited |= changed;
        output.selection_changed = true;
    }

    fn select(&mut self, selection: Selection, output: &mut EditorOutput) {
        self.view.set_selection(self.buffer, selection);
        output.selection_changed = true;
    }

    fn handle_key(
        &mut self,
        key: Key,
        mods: Modifiers,
        map: &DisplayMap,
        output: &mut EditorOutput,
    ) -> bool {
        let rope = self.buffer.document.rope().clone();
        let config = self.view.config.clone();
        let tab = config.tab_size;
        let sel = self.view.selection.clone();
        let ctrl = mods.command || mods.ctrl;
        let motion = |m: Motion| move_selection(&rope, &sel, m, mods.shift, tab, map);
        match key {
            Key::ArrowLeft | Key::ArrowRight if !mods.alt => {
                let m = match (key, ctrl) {
                    (Key::ArrowLeft, false) => Motion::Left,
                    (Key::ArrowLeft, true) => Motion::WordLeft,
                    (_, false) => Motion::Right,
                    _ => Motion::WordRight,
                };
                self.select(motion(m), output);
            }
            Key::ArrowUp | Key::ArrowDown if !mods.alt => {
                if ctrl && !mods.shift {
                    let delta = if key == Key::ArrowUp { -1.0 } else { 1.0 };
                    self.view.scroll.y += delta * self.view_row_height_hint();
                } else if !ctrl {
                    let m = if key == Key::ArrowUp {
                        Motion::Up
                    } else {
                        Motion::Down
                    };
                    self.select(motion(m), output);
                } else {
                    return false;
                }
            }
            Key::Home | Key::End if !mods.alt => {
                let m = match (key, ctrl) {
                    (Key::Home, false) => Motion::LineStart,
                    (Key::Home, true) => Motion::DocStart,
                    (_, false) => Motion::LineEnd,
                    _ => Motion::DocEnd,
                };
                self.select(motion(m), output);
            }
            Key::PageUp | Key::PageDown if !ctrl && !mods.alt => {
                let rows = self.view.page_rows.max(1);
                let up = key == Key::PageUp;
                self.select(
                    motion(if up {
                        Motion::PageUp(rows)
                    } else {
                        Motion::PageDown(rows)
                    }),
                    output,
                );
                let delta = rows as f32 * self.view_row_height_hint();
                self.view.scroll.y += if up { -delta } else { delta };
            }
            Key::Backspace if !mods.alt => {
                let e = if ctrl {
                    edit::delete_word(&rope, &sel, false)
                } else {
                    edit::delete_backward(&rope, &sel, &config)
                };
                self.apply(e, output);
            }
            Key::Delete if !mods.alt && !mods.shift => {
                let e = if ctrl {
                    edit::delete_word(&rope, &sel, true)
                } else {
                    edit::delete_forward(&rope, &sel)
                };
                self.apply(e, output);
            }
            Key::Enter if !ctrl && !mods.alt => {
                let sel = if mods.shift {
                    move_selection(&rope, &sel, Motion::LineEnd, false, tab, map)
                } else {
                    sel
                };
                let e = edit::newline(&rope, &sel, &config);
                self.apply(e, output);
                output.typed.push('\n');
            }
            Key::Tab if !ctrl && !mods.alt => {
                let e = if mods.shift {
                    edit::outdent_lines(&rope, &sel, &config)
                } else {
                    edit::tab(&rope, &sel, &config)
                };
                self.apply(e, output);
            }
            Key::Escape if !mods.shift && !ctrl && !mods.alt => {
                if sel.len() > 1 {
                    self.select(sel.into_primary(), output);
                } else if self.view.find.open {
                    self.view.find.open = false;
                } else if !sel.is_empty() {
                    let head = sel.primary().head;
                    self.select(Selection::point(head), output);
                } else {
                    return false;
                }
            }
            Key::A if ctrl && !mods.shift && !mods.alt => {
                self.select(edit::select_all(&rope), output);
                self.view.reveal = None;
            }
            _ => return false,
        }
        true
    }

    fn view_row_height_hint(&self) -> f32 {
        (self.style.font().size * self.style.line_spacing * 1.2).round()
    }

    fn handle_events(&mut self, ui: &egui::Ui, map: &DisplayMap, output: &mut EditorOutput) {
        let events = ui.input(|i| i.events.clone());
        for event in events {
            let rope = self.buffer.document.rope().clone();
            let config = self.view.config.clone();
            match event {
                Event::Text(text) => {
                    if text.chars().all(|c| c.is_control()) {
                        continue;
                    }
                    let e = edit::insert_text(&rope, &self.view.selection, &text, &config);
                    self.apply(e, output);
                    output.typed.push_str(&text);
                }
                Event::Copy => {
                    let (text, line_mode) = edit::copy_text(&rope, &self.view.selection);
                    self.view.line_clipboard = line_mode.then(|| text.clone());
                    ui.ctx().copy_text(text);
                }
                Event::Cut => {
                    let (text, line_mode, e) = edit::cut(&rope, &self.view.selection);
                    self.view.line_clipboard = line_mode.then(|| text.clone());
                    ui.ctx().copy_text(text);
                    self.apply(e, output);
                }
                Event::Paste(text) => {
                    let normalized = text.replace("\r\n", "\n");
                    let line_mode =
                        self.view.line_clipboard.as_deref() == Some(normalized.as_str());
                    let e = edit::paste(&rope, &self.view.selection, &normalized, line_mode);
                    self.apply(e, output);
                }
                Event::Ime(egui::ImeEvent::Preedit { text, .. }) => {
                    self.view.preedit = (!text.is_empty()).then_some(text);
                }
                Event::Ime(egui::ImeEvent::Commit(text)) => {
                    self.view.preedit = None;
                    if !text.is_empty() {
                        let e = edit::insert_text(&rope, &self.view.selection, &text, &config);
                        self.apply(e, output);
                        output.typed.push_str(&text);
                    }
                }
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    let map = if output.edited {
                        self.view.display_map(&self.buffer.document)
                    } else {
                        map.clone()
                    };
                    self.handle_key(key, modifiers, &map, output);
                }
                _ => {}
            }
        }
    }

    fn reveal(&mut self, ctx: &egui::Context, g: &Geometry, map: &DisplayMap, style_key: u64) {
        let Some(reveal) = self.view.reveal.take() else {
            return;
        };
        let rope = self.buffer.document.rope().clone();
        let head = self.view.selection.primary().head.min(rope.len_chars());
        let line = rope.char_to_line(head);
        let row = map.row_of_line(line);
        let y = row as f32 * g.row_height;
        let h = g.text.height();
        let sticky = self.options.sticky_lines.min(3) as f32 * g.row_height;
        match reveal {
            Reveal::Center => self.view.scroll.y = y - h * 0.4,
            Reveal::Caret => {
                let margin = g.row_height * 2.0;
                if y < self.view.scroll.y + margin + sticky {
                    self.view.scroll.y = y - margin - sticky;
                } else if y + g.row_height > self.view.scroll.y + h - margin {
                    self.view.scroll.y = y + g.row_height - h + margin;
                }
            }
        }
        let layout = self.layout_line(ctx, style_key, line);
        let x = layout.x_of(head - rope.line_to_char(line));
        let w = g.text.width() - TEXT_PADDING * 2.0;
        if x < self.view.scroll.x + g.char_width * 2.0 {
            self.view.scroll.x = (x - g.char_width * 4.0).max(0.0);
        } else if x > self.view.scroll.x + w - g.char_width * 2.0 {
            self.view.scroll.x = x - w + g.char_width * 6.0;
        }
    }

    pub fn show(mut self, ui: &mut egui::Ui) -> EditorOutput {
        let mut output = EditorOutput::default();
        let ctx = ui.ctx().clone();
        self.view.sync(&self.buffer.document);
        if let Some((line, column)) = self.buffer.take_pending_cursor() {
            self.view.goto_line(&self.buffer.document, line, column);
            output.selection_changed = true;
        }
        let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
        let response = ui.interact(rect, self.id, Sense::click_and_drag());
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Editor"));
        let g = self.geometry(ui, rect);
        self.view.page_rows = ((g.text.height() / g.row_height).floor() as usize)
            .saturating_sub(1)
            .max(1);
        let style_key = self.style.layout_key();
        self.view.layouts.begin_frame();

        if response.clicked() || response.drag_started() || response.middle_clicked() {
            response.request_focus();
        }
        let focused = response.has_focus();
        output.focused = focused;
        if focused {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    self.id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                )
            });
        }

        // ---- Input ----
        let mut map = self.view.display_map(&self.buffer.document);
        if focused {
            self.handle_events(ui, &map, &mut output);
            map = self.view.display_map(&self.buffer.document);
        }
        self.handle_pointer(ui, &response, &g, &map, style_key, &mut output);
        map = self.view.display_map(&self.buffer.document);
        if response.hovered() {
            let delta = ui.input(|i| i.smooth_scroll_delta);
            if delta != egui::Vec2::ZERO {
                self.view.scroll -= delta;
            }
        }
        self.reveal(&ctx, &g, &map, style_key);
        let max_y = self.max_scroll_y(&g, &map);
        self.view.scroll.y = self.view.scroll.y.clamp(0.0, max_y);

        // ---- Visible lines ----
        let rope = self.buffer.document.rope().clone();
        let total_rows = map.rows().max(1);
        let first_row = (self.view.scroll.y / g.row_height).floor() as usize;
        let last_row = (((self.view.scroll.y + g.text.height()) / g.row_height).ceil() as usize)
            .min(total_rows - 1);
        let mut visible: Vec<(usize, usize, LineLayout)> = Vec::new();
        for row in first_row..=last_row.max(first_row) {
            if row >= total_rows {
                break;
            }
            let line = map.line_of_row(row);
            visible.push((row, line, self.layout_line(&ctx, style_key, line)));
        }
        output.first_visible_line = visible.first().map(|v| v.1).unwrap_or(0);
        let max_width = visible.iter().map(|v| v.2.width()).fold(0.0, f32::max);
        let max_x = (max_width + g.char_width * 8.0 - (g.text.width() - TEXT_PADDING)).max(0.0);
        self.view.scroll.x = self.view.scroll.x.clamp(0.0, max_x);
        let scroll = self.view.scroll;

        // ---- Paint ----
        let c = &self.style.colors;
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, c.background);
        let text_painter = ui.painter_at(g.text);
        let row_y = |row: usize| g.text.top() + row as f32 * g.row_height - scroll.y;
        let x0 = g.text_left - scroll.x;
        let caret_lines: Vec<usize> = self
            .view
            .selection
            .ranges()
            .iter()
            .map(|r| rope.char_to_line(r.head.min(rope.len_chars())))
            .collect();
        if self.view.find.open {
            self.view
                .find
                .refresh(&rope, self.buffer.document.revision());
        }
        let blink_on = !self.options.blink
            || (Instant::now().duration_since(self.view.blink).as_millis() / BLINK.as_millis())
                .is_multiple_of(2);
        let mut fold_chips = Vec::new();
        let mut lens_chips = Vec::new();
        // Ctrl held over a document link underlines it.
        let mods_command = ui.input(|i| i.modifiers.command);
        let hover_char = response
            .hover_pos()
            .filter(|p| g.text.contains(*p))
            .map(|p| self.pos_at(&ctx, &g, &map, style_key, p));
        for (row, line, layout) in &visible {
            let y = row_y(*row);
            let line_start = rope.line_to_char(*line);
            let line_stop = line_end(&rope, *line);
            let band =
                Rect::from_min_size(pos2(g.text.left(), y), vec2(g.text.width(), g.row_height));
            if self.decorations.execution_line == Some(*line) {
                text_painter.rect_filled(band, CornerRadius::ZERO, c.execution_line);
            } else if caret_lines.contains(line)
                && self.view.selection.ranges().iter().all(|r| r.is_empty())
            {
                text_painter.rect_filled(band, CornerRadius::ZERO, c.current_line);
            }
            let x_of = |col: usize| x0 + layout.x_of(col);
            let highlight = |ranges: &[(usize, usize)], color: Color32| {
                let from = ranges.partition_point(|r| r.1 < line_start);
                for &(s, e) in ranges[from..].iter().take_while(|r| r.0 <= line_stop) {
                    if let Some((a, b)) = columns_on_line(s, e, line_start, line_stop) {
                        if a < b {
                            let r = Rect::from_min_max(
                                pos2(x_of(a), y),
                                pos2(x_of(b), y + g.row_height),
                            );
                            text_painter.rect_filled(r, CornerRadius::same(2), color);
                        }
                    }
                }
            };
            highlight(&self.decorations.occurrences, c.occurrence);
            if self.view.find.open {
                highlight(&self.view.find.matches, c.search_match);
            }
            for range in self
                .view
                .selection
                .ranges()
                .iter()
                .filter(|r| !r.is_empty())
            {
                if let Some((a, b)) =
                    columns_on_line(range.start(), range.end(), line_start, line_stop)
                {
                    let extra = if range.end() > line_stop {
                        g.char_width * 0.6
                    } else {
                        0.0
                    };
                    let r = Rect::from_min_max(
                        pos2(x_of(a), y),
                        pos2(x_of(b) + extra, y + g.row_height),
                    );
                    text_painter.rect_filled(r, CornerRadius::same(2), c.selection);
                }
            }
            if self.options.indent_guides {
                let text = line_text(&rope, *line);
                let indent = if text.trim().is_empty() {
                    (*line + 1..(*line + 12).min(rope.len_lines()))
                        .map(|l| line_text(&rope, l))
                        .find(|t| !t.trim().is_empty())
                        .map(|t| {
                            crate::textutil::cell_of(
                                &Rope::from_str(&t),
                                t.chars().take_while(|c| c.is_whitespace()).count(),
                                self.view.config.tab_size,
                            )
                        })
                        .unwrap_or(0)
                } else {
                    let ws = text.chars().take_while(|c| *c == ' ' || *c == '\t').count();
                    crate::textutil::cell_of(&rope, line_start + ws, self.view.config.tab_size)
                };
                let tab = self.view.config.tab_size.max(1);
                let mut level = tab;
                while level < indent + usize::from(text.trim().is_empty()) && level <= indent {
                    let x = x0 + level as f32 * g.char_width - g.char_width * (tab as f32) + 0.5;
                    text_painter.vline(x, y..=y + g.row_height, Stroke::new(1.0, c.indent_guide));
                    level += tab;
                }
            }
            let galley_y = y + (g.row_height - layout.galley.size().y) * 0.5;
            text_painter.galley(pos2(x0, galley_y), layout.galley.clone(), c.text);
            // Text written after the line: fold chip, lenses, diagnostic.
            let mut tail_x = x0 + layout.width() + g.char_width * 2.0;
            if map.folds_after(*line) {
                let x = x0 + layout.width() + g.char_width;
                let chip = Rect::from_min_size(
                    pos2(x, y + 3.0),
                    vec2(g.char_width * 3.0, g.row_height - 6.0),
                );
                text_painter.rect_filled(chip, CornerRadius::same(3), c.fold_placeholder);
                text_painter.text(
                    chip.center(),
                    egui::Align2::CENTER_CENTER,
                    "…",
                    self.style.font().clone(),
                    c.fold_placeholder_text,
                );
                fold_chips.push((chip, *line));
                tail_x = chip.right() + g.char_width;
            }
            for (index, lens) in self
                .decorations
                .code_lenses
                .iter()
                .enumerate()
                .filter(|(_, l)| l.line == *line && !l.label.is_empty())
            {
                let font = egui::FontId::new(
                    self.style.font().size * 0.86,
                    egui::FontFamily::Proportional,
                );
                let galley = ui
                    .painter()
                    .layout_no_wrap(lens.label.clone(), font, c.inlay_text);
                let at = pos2(tail_x, y + (g.row_height - galley.size().y) * 0.5);
                let chip = Rect::from_min_size(at, galley.size()).expand2(vec2(3.0, 1.0));
                let hovered = response
                    .hover_pos()
                    .is_some_and(|p| chip.contains(p) && g.text.contains(p));
                if hovered {
                    text_painter.rect_filled(chip, CornerRadius::same(3), c.inlay_background);
                }
                text_painter.galley(at, galley.clone(), c.inlay_text);
                if hovered {
                    text_painter.hline(
                        at.x..=at.x + galley.size().x,
                        at.y + galley.size().y,
                        Stroke::new(1.0, c.inlay_text),
                    );
                }
                lens_chips.push((chip, index));
                tail_x = chip.right() + g.char_width;
            }
            if mods_command && !self.decorations.links.is_empty() {
                if let Some(hover) = hover_char {
                    for &(s, e) in self
                        .decorations
                        .links
                        .iter()
                        .filter(|(s, e)| *s <= hover && hover < *e)
                    {
                        if let Some((a, b)) = columns_on_line(s, e, line_start, line_stop) {
                            text_painter.hline(
                                x_of(a)..=x_of(b),
                                y + g.row_height - 2.0,
                                Stroke::new(1.0, c.info),
                            );
                        }
                    }
                }
            }
            for diagnostic in &self.decorations.diagnostics {
                if let Some((a, b)) = columns_on_line(
                    diagnostic.start,
                    diagnostic.end.max(diagnostic.start + 1),
                    line_start,
                    line_stop,
                ) {
                    let color = match diagnostic.severity {
                        DiagnosticSeverity::Error => c.error,
                        DiagnosticSeverity::Warning => c.warning,
                        DiagnosticSeverity::Info => c.info,
                        DiagnosticSeverity::Hint => c.hint,
                    };
                    let to = if b > a {
                        x_of(b)
                    } else {
                        x_of(a) + g.char_width
                    };
                    wavy(&text_painter, x_of(a), to, y + g.row_height - 3.0, color);
                }
            }
            if self.options.inline_diagnostics {
                let worst = self
                    .decorations
                    .diagnostics
                    .iter()
                    .filter(|d| {
                        d.severity >= DiagnosticSeverity::Warning
                            && d.start >= line_start
                            && d.start <= line_stop
                    })
                    .max_by_key(|d| d.severity);
                if let Some(d) = worst {
                    let color = match d.severity {
                        DiagnosticSeverity::Error => c.error,
                        _ => c.warning,
                    };
                    let message = d.message.lines().next().unwrap_or("").to_string();
                    let x = tail_x + g.char_width;
                    let font = egui::FontId::new(
                        self.style.font().size * 0.92,
                        egui::FontFamily::Proportional,
                    );
                    let galley =
                        ui.painter()
                            .layout_no_wrap(message, font, color.gamma_multiply(0.85));
                    let at = pos2(x, y + (g.row_height - galley.size().y) * 0.5);
                    text_painter.galley(at, galley, color);
                }
            }
            for range in self.view.selection.ranges() {
                let head = range.head.min(rope.len_chars());
                if rope.char_to_line(head) != *line {
                    continue;
                }
                let x = x_of(head - line_start);
                let caret =
                    Rect::from_min_size(pos2(x - 1.0, y + 2.0), vec2(2.0, g.row_height - 4.0));
                if *range == self.view.selection.primary() {
                    output.caret_rect = Some(caret);
                    if let Some(preedit) = &self.view.preedit {
                        let galley = ui.painter().layout_no_wrap(
                            preedit.clone(),
                            self.style.font().clone(),
                            c.text,
                        );
                        let at = pos2(x, galley_y);
                        let r = Rect::from_min_size(at, galley.size());
                        text_painter.rect_filled(r, CornerRadius::ZERO, c.background);
                        text_painter.galley(at, galley, c.text);
                        text_painter.hline(r.x_range(), r.bottom(), Stroke::new(1.0, c.text));
                    }
                }
                if focused && blink_on {
                    text_painter.rect_filled(caret, CornerRadius::ZERO, c.caret);
                }
            }
        }
        self.view.fold_chips = fold_chips;
        self.view.lens_chips = lens_chips;

        self.paint_gutter(ui, &g, &map, &visible, &caret_lines, &response);
        self.paint_sticky(ui, &g, &map, &rope, style_key, first_row);
        self.paint_minimap(ui, &g, &map);
        self.paint_scrollbars(ui, &g, &map, max_y, max_x, &mut output);
        if self.view.find.open {
            self.find_bar(ui, &g, &mut output);
        }

        if let Some(pointer) = response.hover_pos() {
            let over_link = mods_command
                && hover_char.is_some_and(|h| {
                    self.decorations
                        .links
                        .iter()
                        .any(|(s, e)| *s <= h && h < *e)
                });
            if self
                .view
                .lens_chips
                .iter()
                .any(|(r, _)| r.contains(pointer))
                || over_link
            {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            } else if g.text.contains(pointer) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
            }
            if g.text.contains(pointer) {
                let pos = self.pos_at(&ctx, &g, &map, style_key, pointer);
                output.hover = Some(pos);
                if let Some(d) = self
                    .decorations
                    .diagnostics
                    .iter()
                    .filter(|_| self.options.diagnostic_tooltips)
                    .filter(|d| d.start <= pos && pos <= d.end.max(d.start + 1))
                    .max_by_key(|d| d.severity)
                {
                    let message = d.message.clone();
                    egui::Tooltip::always_open(
                        ctx.clone(),
                        ui.layer_id(),
                        self.id.with("diagnostic"),
                        egui::PopupAnchor::Pointer,
                    )
                    .show(|ui| {
                        ui.set_max_width(480.0);
                        ui.label(message);
                    });
                }
            }
        }

        if focused {
            if let Some(caret) = output.caret_rect {
                ui.ctx().output_mut(|o| {
                    o.ime = Some(egui::output::IMEOutput {
                        purpose: egui::IMEPurpose::Normal,
                        rect: g.text,
                        cursor_rect: caret,
                        should_interrupt_composition: false,
                    })
                });
            }
            if self.options.blink {
                let elapsed = Instant::now().duration_since(self.view.blink).as_millis() as u64;
                let next = BLINK.as_millis() as u64 - elapsed % BLINK.as_millis() as u64;
                ctx.request_repaint_after(Duration::from_millis(next));
            }
        }
        if self.syntax.is_pending() {
            ctx.request_repaint_after(Duration::from_millis(60));
        }
        output.response = Some(response);
        output
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_pointer(
        &mut self,
        ui: &egui::Ui,
        response: &Response,
        g: &Geometry,
        map: &DisplayMap,
        style_key: u64,
        output: &mut EditorOutput,
    ) {
        let ctx = ui.ctx().clone();
        let (pointer, primary_pressed, middle_pressed, any_down, mods, released) = ui.input(|i| {
            (
                i.pointer.interact_pos(),
                i.pointer.primary_pressed(),
                i.pointer.button_pressed(egui::PointerButton::Middle),
                i.pointer.any_down(),
                i.modifiers,
                i.pointer.any_released(),
            )
        });
        let secondary_pressed = ui.input(|i| i.pointer.secondary_pressed());
        let Some(pointer) = pointer else {
            return;
        };
        let rope = self.buffer.document.rope().clone();
        let tab = self.view.config.tab_size;
        // By position and top layer rather than `contains_pointer`: egui
        // does not hover widgets under an open popup, and a right click here
        // while a menu is open must still say where it landed.
        // (Panels are not areas: over the editor, no area means its layer.)
        let over_editor = response.rect.contains(pointer)
            && ctx
                .layer_id_at(pointer)
                .is_none_or(|layer| layer == response.layer_id);
        if secondary_pressed && over_editor && !g.vbar.contains(pointer) {
            if g.gutter.contains(pointer) {
                let (line, _) = self.line_cell_at(g, map, pointer);
                output.context = Some(ContextTarget::Gutter(line));
            } else if g.text.contains(pointer) {
                let pos = self.pos_at(&ctx, g, map, style_key, pointer);
                // As in IntelliJ: a right click outside the selection moves
                // the caret there first, so the menu acts on that place.
                let inside = self
                    .view
                    .selection
                    .ranges()
                    .iter()
                    .any(|r| !r.is_empty() && r.start() <= pos && pos <= r.end());
                if !inside {
                    self.view.reveal = None;
                    self.view.set_selection(self.buffer, Selection::point(pos));
                    output.selection_changed = true;
                }
                output.context = Some(ContextTarget::Text(pos));
            }
            return;
        }
        let pressed_here = response.contains_pointer() && (primary_pressed || middle_pressed);
        if pressed_here && primary_pressed {
            // Minimap: a click outside the slider centers that line; then
            // (or on the slider) dragging scrolls.
            if let Some((rect, mg)) = self
                .minimap_geometry(g, map)
                .filter(|(rect, _)| rect.contains(pointer))
            {
                let y = pointer.y - rect.top();
                if y < mg.slider_top || y > mg.slider_top + mg.slider_height {
                    let row = map.row_of_line(mg.line_at(y));
                    let max_y = self.max_scroll_y(g, map);
                    self.view.scroll.y =
                        (row as f32 * g.row_height - g.text.height() * 0.5).clamp(0.0, max_y);
                }
                self.view.drag = Some(DragKind::Minimap {
                    scroll: self.view.scroll.y,
                    pointer_y: pointer.y,
                });
                return;
            }
        }
        if pressed_here {
            // Scrollbar.
            if g.vbar.contains(pointer) {
                let max_y = self.max_scroll_y(g, map);
                let (thumb, _) = self.thumb(g, max_y);
                if thumb.contains(pointer) {
                    self.view.drag = Some(DragKind::Scrollbar {
                        grab: pointer.y - thumb.top(),
                    });
                } else {
                    let frac = ((pointer.y - g.vbar.top()) / g.vbar.height()).clamp(0.0, 1.0);
                    self.view.scroll.y = frac * max_y;
                    self.view.drag = Some(DragKind::Scrollbar { grab: 12.0 });
                }
                return;
            }
            // Code lenses, fold placeholder and sticky lines (rects from
            // the last frame).
            if primary_pressed {
                if let Some((_, index)) = self
                    .view
                    .lens_chips
                    .iter()
                    .find(|(r, _)| r.contains(pointer))
                    .copied()
                {
                    output.lens_click = Some(index);
                    return;
                }
            }
            if let Some((_, header)) = self
                .view
                .fold_chips
                .iter()
                .find(|(r, _)| r.contains(pointer))
                .copied()
            {
                self.view.folds.unfold_header(&rope, header);
                return;
            }
            if let Some((_, line)) = self
                .view
                .sticky_rows
                .iter()
                .find(|(r, _)| r.contains(pointer))
                .copied()
            {
                let indent = crate::textutil::indentation(&rope, line).chars().count();
                self.view.set_caret(
                    &self.buffer.document,
                    rope.line_to_char(line) + indent,
                    false,
                );
                let row = map.row_of_line(line);
                self.view.scroll.y = row as f32 * g.row_height;
                self.view.reveal = None;
                output.selection_changed = true;
                return;
            }
            // Gutter.
            if g.gutter.contains(pointer) && primary_pressed {
                let (line, _) = self.line_cell_at(g, map, pointer);
                if let Some(fold_x) = g.fold_x {
                    if pointer.x >= fold_x - 8.0 {
                        if !self.view.folds.unfold_header(&rope, line) {
                            if let Some(structure) = self.syntax.structure(&self.buffer.document) {
                                if let Some(f) =
                                    structure.folds.iter().find(|f| f.start_line0 == line)
                                {
                                    self.view.folds.fold(&rope, f.start_line0, f.end_line0);
                                    let caret_line = rope.char_to_line(self.view.primary_caret());
                                    if caret_line > f.start_line0 && caret_line <= f.end_line0 {
                                        self.view.selection =
                                            Selection::point(rope.line_to_char(f.start_line0));
                                    }
                                }
                            }
                        }
                        return;
                    }
                }
                if pointer.x >= g.numbers.0 && pointer.x < g.numbers.1 {
                    output.gutter_click = Some(GutterClick::LineNumber(line));
                    return;
                }
                return;
            }
            if !g.text.contains(pointer) {
                return;
            }
            let pos = self.pos_at(&ctx, g, map, style_key, pointer);
            let now = Instant::now();
            let count = match self.view.last_press {
                Some((at, p, n))
                    if now.duration_since(at) < Duration::from_millis(450)
                        && p.distance(pointer) < 5.0 =>
                {
                    (n % 3) + 1
                }
                _ => 1,
            };
            self.view.last_press = Some((now, pointer, count));
            if middle_pressed || (mods.alt && mods.shift) {
                let (line, cell) = self.line_cell_at(g, map, pointer);
                self.view.drag = Some(DragKind::Column { line, cell });
                let sel = edit::column_selection(&rope, (line, cell), (line, cell), tab, map);
                self.select(sel, output);
                return;
            }
            if mods.alt {
                let sel = self.view.selection.clone().push(Range::point(pos));
                self.select(sel, output);
                return;
            }
            let ctrl = mods.command || mods.ctrl;
            let range = match count {
                2 => word_at(&rope, pos)
                    .map(|(s, e)| Range::new(s, e))
                    .unwrap_or(Range::point(pos)),
                3 => {
                    let line = rope.char_to_line(pos);
                    let end = if line + 1 < rope.len_lines() {
                        rope.line_to_char(line + 1)
                    } else {
                        rope.len_chars()
                    };
                    Range::new(rope.line_to_char(line), end)
                }
                _ if mods.shift => Range::new(self.view.selection.primary().anchor, pos),
                _ => Range::point(pos),
            };
            self.view.drag = Some(DragKind::Text {
                granularity: if mods.shift { 0 } else { count - 1 },
                origin: range,
            });
            self.view.reveal = None;
            self.view
                .set_selection(self.buffer, Selection::new(vec![range], 0));
            self.view.reveal = None;
            output.selection_changed = true;
            if ctrl && count == 1 {
                output.ctrl_click = Some(pos);
            }
            return;
        }
        match self.view.drag {
            Some(_) if !any_down || released && !any_down => {
                self.view.drag = None;
                self.buffer.break_undo_group();
            }
            Some(DragKind::Minimap { scroll, pointer_y }) => {
                if let Some((_, mg)) = self.minimap_geometry(g, map) {
                    let max_y = self.max_scroll_y(g, map);
                    let ratio = mg.drag_ratio(max_y);
                    self.view.scroll.y =
                        (scroll + (pointer.y - pointer_y) * ratio).clamp(0.0, max_y);
                }
            }
            Some(DragKind::Scrollbar { grab }) => {
                let max_y = self.max_scroll_y(g, map);
                let (thumb, track) = self.thumb(g, max_y);
                let travel = (track.height() - thumb.height()).max(1.0);
                let frac = ((pointer.y - grab - track.top()) / travel).clamp(0.0, 1.0);
                self.view.scroll.y = frac * max_y;
            }
            Some(DragKind::Text {
                granularity,
                origin,
            }) => {
                // Auto-scroll when dragging past the text edges.
                if pointer.y < g.text.top() {
                    self.view.scroll.y -= (g.text.top() - pointer.y).min(g.row_height * 3.0);
                    ctx.request_repaint();
                } else if pointer.y > g.text.bottom() {
                    self.view.scroll.y += (pointer.y - g.text.bottom()).min(g.row_height * 3.0);
                    ctx.request_repaint();
                }
                let clamped = pos2(
                    pointer.x.max(g.text.left()),
                    pointer.y.clamp(g.text.top(), g.text.bottom() - 1.0),
                );
                let pos = self.pos_at(&ctx, g, map, style_key, clamped);
                let range = match granularity {
                    1 => {
                        let (s, e) = word_at(&rope, pos).unwrap_or((pos, pos));
                        if pos < origin.start() {
                            Range::new(origin.end(), s)
                        } else {
                            Range::new(origin.start(), e.max(origin.end()))
                        }
                    }
                    2 => {
                        let line = rope.char_to_line(pos);
                        if pos < origin.start() {
                            Range::new(origin.end(), rope.line_to_char(line))
                        } else {
                            let end = if line + 1 < rope.len_lines() {
                                rope.line_to_char(line + 1)
                            } else {
                                rope.len_chars()
                            };
                            Range::new(origin.start(), end)
                        }
                    }
                    _ => Range::new(origin.anchor, pos),
                };
                if self.view.selection.primary() != range {
                    self.view.selection = self.view.selection.clone().with_primary(range);
                    output.selection_changed = true;
                }
            }
            Some(DragKind::Column { line, cell }) => {
                let (to_line, to_cell) = self.line_cell_at(g, map, pointer);
                let sel = edit::column_selection(&rope, (line, cell), (to_line, to_cell), tab, map);
                if sel != self.view.selection {
                    self.view.selection = sel;
                    output.selection_changed = true;
                }
            }
            None => {}
        }
    }

    fn thumb(&self, g: &Geometry, max_y: f32) -> (Rect, Rect) {
        let track = g.vbar.shrink2(vec2(2.0, 2.0));
        let h = track.height();
        let thumb_h = (h * h / (h + max_y.max(0.0))).clamp(24.0_f32.min(h), h);
        let frac = if max_y > 0.0 {
            self.view.scroll.y / max_y
        } else {
            0.0
        };
        let top = track.top() + (h - thumb_h) * frac;
        (
            Rect::from_min_size(pos2(track.left(), top), vec2(track.width(), thumb_h)),
            track,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_gutter(
        &mut self,
        ui: &egui::Ui,
        g: &Geometry,
        map: &DisplayMap,
        visible: &[(usize, usize, LineLayout)],
        caret_lines: &[usize],
        response: &Response,
    ) {
        let c = &self.style.colors;
        let painter = ui.painter_at(g.gutter);
        painter.rect_filled(g.gutter, CornerRadius::ZERO, c.gutter_background);
        let scroll = self.view.scroll;
        let deco = self.decorations;
        let folds = if self.options.folding {
            self.syntax.structure(&self.buffer.document)
        } else {
            None
        };
        let hovered = response.hover_pos().is_some_and(|p| g.gutter.contains(p));
        let font = self.style.gutter_font.clone();
        for (row, line, _) in visible {
            let y = g.rect.top() + *row as f32 * g.row_height - scroll.y;
            let mid = y + g.row_height * 0.5;
            let row_rect =
                Rect::from_min_size(pos2(g.rect.left(), y), vec2(g.gutter.width(), g.row_height));
            if deco.execution_line == Some(*line) {
                painter.rect_filled(row_rect, CornerRadius::ZERO, c.execution_line);
            }
            if let Some((_, mark)) = deco.coverage.iter().find(|(l, _)| l == line) {
                let color = match mark {
                    CoverageMark::Full => c.coverage_full,
                    CoverageMark::Partial => c.coverage_partial,
                    CoverageMark::None => c.coverage_none,
                };
                painter.rect_filled(
                    Rect::from_min_size(pos2(g.coverage_x, y), vec2(3.0, g.row_height)),
                    CornerRadius::ZERO,
                    color,
                );
            }
            if let (Some((from, to)), Some(blame)) = (g.blame, deco.blame) {
                if let Some(entry) = blame(*line) {
                    let color = if deco.blame_stale {
                        c.text_muted.gamma_multiply(0.6)
                    } else {
                        c.blame_text
                    };
                    let galley = ui.painter().layout_no_wrap(entry.text, font.clone(), color);
                    let clip = painter.with_clip_rect(Rect::from_min_max(
                        pos2(from, y),
                        pos2(to, y + g.row_height),
                    ));
                    clip.galley(pos2(from, mid - galley.size().y * 0.5), galley, color);
                }
            }
            let breakpoint = deco
                .breakpoints
                .iter()
                .find(|(l, _)| l == line)
                .map(|(_, m)| *m);
            if self.options.line_numbers && breakpoint.is_none() {
                let current = caret_lines.contains(line);
                let color = if current {
                    c.gutter_text_current
                } else {
                    c.gutter_text
                };
                painter.text(
                    pos2(g.numbers.1 - 8.0, mid),
                    egui::Align2::RIGHT_CENTER,
                    (line + 1).to_string(),
                    font.clone(),
                    color,
                );
            }
            if let Some(mark) = breakpoint {
                let center = pos2(g.numbers.1 - 8.0 - g.row_height * 0.3, mid);
                let radius = g.row_height * 0.3;
                match mark {
                    BreakpointMark::Enabled | BreakpointMark::Verified => {
                        painter.circle_filled(center, radius, c.breakpoint);
                        if mark == BreakpointMark::Verified {
                            let s = radius * 0.5;
                            painter.add(PathShape::line(
                                vec![
                                    center + vec2(-s, 0.0),
                                    center + vec2(-s * 0.2, s * 0.7),
                                    center + vec2(s, -s * 0.6),
                                ],
                                Stroke::new(1.4, c.breakpoint_check),
                            ));
                        }
                    }
                    BreakpointMark::Disabled => {
                        painter.circle_stroke(center, radius - 0.5, Stroke::new(1.4, c.breakpoint));
                    }
                    BreakpointMark::Invalid => {
                        painter.circle_stroke(center, radius - 0.5, Stroke::new(1.4, c.text_muted));
                        let s = radius * 0.5;
                        painter.line_segment(
                            [center + vec2(-s, -s), center + vec2(s, s)],
                            Stroke::new(1.4, c.text_muted),
                        );
                    }
                    BreakpointMark::Muted => {
                        painter.circle_filled(center, radius, c.breakpoint.gamma_multiply(0.35));
                    }
                }
            }
            if deco.bookmarks.contains(line) {
                painter.rect_filled(
                    Rect::from_min_size(
                        pos2(g.numbers.0 + 2.0, y + 3.0),
                        vec2(3.0, g.row_height - 6.0),
                    ),
                    CornerRadius::same(1),
                    c.bookmark,
                );
            }
            if let Some((_, mark)) = deco.git.iter().find(|(l, _)| l == line) {
                match mark {
                    GitMark::Added | GitMark::Modified => {
                        let color = if *mark == GitMark::Added {
                            c.git_added
                        } else {
                            c.git_modified
                        };
                        painter.rect_filled(
                            Rect::from_min_size(pos2(g.git_x, y), vec2(3.0, g.row_height)),
                            CornerRadius::ZERO,
                            color,
                        );
                    }
                    GitMark::Deleted => {
                        let base = y + g.row_height;
                        painter.add(egui::Shape::convex_polygon(
                            vec![
                                pos2(g.git_x, base - 4.0),
                                pos2(g.git_x + 5.0, base),
                                pos2(g.git_x, base + 4.0),
                            ],
                            c.git_deleted,
                            Stroke::NONE,
                        ));
                    }
                }
            }
            if let (Some(fold_x), Some(structure)) = (g.fold_x, &folds) {
                let folded = map.folds_after(*line);
                let foldable = folded || structure.folds.iter().any(|f| f.start_line0 == *line);
                if foldable && (folded || hovered || self.options.fold_markers_always) {
                    chevron(&painter, pos2(fold_x, mid), 8.0, !folded, c.fold_marker);
                }
            }
        }
    }

    fn paint_sticky(
        &mut self,
        ui: &egui::Ui,
        g: &Geometry,
        map: &DisplayMap,
        rope: &Rope,
        style_key: u64,
        first_row: usize,
    ) {
        self.view.sticky_rows.clear();
        let max = self.options.sticky_lines;
        if max == 0 || self.view.scroll.y <= 0.0 {
            return;
        }
        let Some(structure) = self.syntax.structure(&self.buffer.document) else {
            return;
        };
        let mut stack: Vec<usize> = Vec::new();
        // The header rows cover the first rows: a block stays while its
        // body is under the stack.
        for _ in 0..max {
            let probe_row = first_row + stack.len();
            let probe = map.line_of_row(probe_row);
            let next = structure
                .report
                .blocks
                .iter()
                .filter(|b| STICKY_KINDS.contains(&b.kind.as_str()))
                .filter(|b| b.start_line0 < probe && probe <= b.end_line0)
                .filter(|b| !stack.contains(&b.start_line0))
                .filter(|b| stack.last().is_none_or(|top| b.start_line0 > *top))
                .min_by_key(|b| b.start_line0)
                .map(|b| b.start_line0);
            match next {
                Some(line) if line < rope.len_lines() => stack.push(line),
                _ => break,
            }
        }
        if stack.is_empty() {
            return;
        }
        let c = self.style.colors.clone();
        let painter = ui.painter_at(Rect::from_min_max(
            g.rect.min,
            pos2(g.text.right(), g.rect.bottom()),
        ));
        let ctx = ui.ctx().clone();
        let x0 = g.text_left - self.view.scroll.x;
        for (i, line) in stack.iter().enumerate() {
            let y = g.rect.top() + i as f32 * g.row_height;
            let row = Rect::from_min_size(
                pos2(g.rect.left(), y),
                vec2(g.text.right() - g.rect.left(), g.row_height),
            );
            painter.rect_filled(row, CornerRadius::ZERO, c.sticky_background);
            if self.options.line_numbers {
                painter.text(
                    pos2(g.numbers.1 - 8.0, y + g.row_height * 0.5),
                    egui::Align2::RIGHT_CENTER,
                    (line + 1).to_string(),
                    self.style.gutter_font.clone(),
                    c.gutter_text,
                );
            }
            let layout = self.layout_line(&ctx, style_key, *line);
            let text_clip = painter.with_clip_rect(Rect::from_min_max(
                pos2(g.text.left(), y),
                pos2(g.text.right(), y + g.row_height),
            ));
            text_clip.galley(
                pos2(x0, y + (g.row_height - layout.galley.size().y) * 0.5),
                layout.galley.clone(),
                c.text,
            );
            self.view.sticky_rows.push((row, *line));
        }
        let bottom = g.rect.top() + stack.len() as f32 * g.row_height;
        painter.hline(
            g.rect.left()..=g.text.right(),
            bottom,
            Stroke::new(1.0, c.sticky_border),
        );
    }

    fn paint_scrollbars(
        &mut self,
        ui: &egui::Ui,
        g: &Geometry,
        map: &DisplayMap,
        max_y: f32,
        max_x: f32,
        output: &mut EditorOutput,
    ) {
        let c = &self.style.colors;
        let painter = ui.painter_at(g.rect);
        let rows = map.rows().max(1) as f32;
        let rope = self.buffer.document.rope();
        let marker =
            |line: usize| g.vbar.top() + map.row_of_line(line) as f32 / rows * g.vbar.height();
        let marker_x = g.vbar.right() - 5.0;
        // With the minimap on, the marks are drawn there instead.
        let marks = g.minimap.is_none();
        let mark = |y: f32, color: Color32| {
            if marks {
                painter.rect_filled(
                    Rect::from_min_size(pos2(marker_x, y), vec2(4.0, 2.5)),
                    CornerRadius::ZERO,
                    color,
                );
            }
        };
        for (line, git) in &self.decorations.git {
            let color = match git {
                GitMark::Added => c.git_added,
                GitMark::Modified => c.git_modified,
                GitMark::Deleted => c.git_deleted,
            };
            mark(marker(*line), color);
        }
        if self.view.find.open {
            for (start, _) in self.view.find.matches.iter().take(2000) {
                mark(marker(rope.char_to_line(*start)), c.search_current);
            }
        }
        for d in &self.decorations.diagnostics {
            let color = match d.severity {
                DiagnosticSeverity::Error => c.error,
                DiagnosticSeverity::Warning => c.warning,
                _ => continue,
            };
            mark(
                marker(rope.char_to_line(d.start.min(rope.len_chars()))),
                color,
            );
        }
        let (thumb, _) = self.thumb(g, max_y);
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let hovered = pointer.is_some_and(|p| g.vbar.contains(p));
        if max_y > 0.0 {
            let color = if hovered || matches!(self.view.drag, Some(DragKind::Scrollbar { .. })) {
                c.scrollbar_thumb_hover
            } else {
                c.scrollbar_thumb
            };
            painter.rect_filled(thumb.shrink2(vec2(2.0, 0.0)), CornerRadius::same(3), color);
        }
        if let Some(p) = pointer.filter(|_| hovered) {
            let frac = ((p.y - g.vbar.top()) / g.vbar.height()).clamp(0.0, 1.0);
            output.scrollbar_hover_line = Some(map.line_of_row((frac * rows) as usize));
        }
        if max_x > 0.0 {
            let track = Rect::from_min_max(
                pos2(g.text.left(), g.rect.bottom() - 6.0),
                pos2(g.text.right(), g.rect.bottom() - 2.0),
            );
            let visible = g.text.width() / (g.text.width() + max_x);
            let w = (track.width() * visible).max(24.0);
            let x = track.left() + (track.width() - w) * (self.view.scroll.x / max_x);
            painter.rect_filled(
                Rect::from_min_size(pos2(x, track.top()), vec2(w, track.height())),
                CornerRadius::same(2),
                c.scrollbar_thumb,
            );
        }
    }

    fn find_bar(&mut self, ui: &egui::Ui, g: &Geometry, output: &mut EditorOutput) {
        let c = self.style.colors.clone();
        let width = 420.0_f32.min(g.text.width() - 16.0);
        let pos = pos2(g.text.right() - width - 8.0, g.text.top() + 6.0);
        let id = self.id.with("find");
        let mut command = None;
        let mut replace: Option<bool> = None;
        let mut close = false;
        let font = self.style.ui_font.clone();
        egui::Area::new(id)
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(c.panel_background)
                    .stroke(Stroke::new(1.0, c.panel_border))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        ui.set_width(width - 12.0);
                        let find = &mut self.view.find;
                        ui.horizontal(|ui| {
                            let edit = egui::TextEdit::singleline(&mut find.query)
                                .id(id.with("query"))
                                .hint_text("Find")
                                .font(font.clone())
                                .desired_width(width - 220.0);
                            let response = ui.add(edit);
                            if find.focus_query {
                                response.request_focus();
                                find.focus_query = false;
                            }
                            if response.has_focus() || response.lost_focus() {
                                let (enter, shift, escape) = ui.input(|i| {
                                    (
                                        i.key_pressed(Key::Enter),
                                        i.modifiers.shift,
                                        i.key_pressed(Key::Escape),
                                    )
                                });
                                if enter {
                                    command = Some(if shift {
                                        ViewCommand::FindPrevious
                                    } else {
                                        ViewCommand::FindNext
                                    });
                                    response.request_focus();
                                }
                                if escape {
                                    close = true;
                                }
                            }
                            ui.toggle_value(&mut find.options.case_sensitive, "Aa")
                                .on_hover_text("Match case");
                            ui.toggle_value(&mut find.options.whole_word, "W")
                                .on_hover_text("Whole words");
                            ui.toggle_value(&mut find.options.regex, ".*")
                                .on_hover_text("Regex");
                            if ui
                                .small_button("↑")
                                .on_hover_text("Previous (Shift+Enter)")
                                .clicked()
                            {
                                command = Some(ViewCommand::FindPrevious);
                            }
                            if ui.small_button("↓").on_hover_text("Next (Enter)").clicked() {
                                command = Some(ViewCommand::FindNext);
                            }
                            if ui
                                .small_button("✕")
                                .on_hover_text("Close (Escape)")
                                .clicked()
                            {
                                close = true;
                            }
                        });
                        let status = match (&find.error, find.matches.len(), find.current) {
                            (Some(error), _, _) => (error.clone(), c.error),
                            (None, 0, _) if !find.query.is_empty() => {
                                ("No results".to_string(), c.text_muted)
                            }
                            (None, 0, _) => (String::new(), c.text_muted),
                            (None, n, Some(i)) => (format!("{} of {n}", i + 1), c.text_muted),
                            (None, n, None) => (format!("{n} results"), c.text_muted),
                        };
                        if find.replace_open {
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut find.replacement)
                                        .id(id.with("replacement"))
                                        .hint_text("Replace")
                                        .font(font.clone())
                                        .desired_width(width - 220.0),
                                );
                                if ui.small_button("Replace").clicked() {
                                    replace = Some(false);
                                }
                                if ui.small_button("All").clicked() {
                                    replace = Some(true);
                                }
                            });
                        }
                        if !status.0.is_empty() {
                            ui.label(
                                egui::RichText::new(status.0)
                                    .font(font.clone())
                                    .color(status.1),
                            );
                        }
                    });
            });
        if close {
            self.view.find.open = false;
            ui.memory_mut(|m| m.request_focus(self.id));
        }
        if let Some(all) = replace {
            if self.buffer.readonly {
                output.blocked_edit = true;
            } else if self.view.replace(self.buffer, all) > 0 {
                output.edited = true;
            }
        }
        if let Some(command) = command {
            self.view
                .find_step(self.buffer, command == ViewCommand::FindNext);
            output.selection_changed = true;
        }
    }
}
