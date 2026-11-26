//! The Markdown editor: a toolbar of formatting actions above the text and
//! a live preview beside it (or instead of it), as in PhpStorm.
//!
//! - Layouts: Editor, Editor and Preview (the default), Preview.
//! - The preview follows the editor's scroll; a double click on a block
//!   puts the caret on its line; task boxes tick in the text; links open
//!   (other Markdown files and local files in the IDE, `#anchors` in the
//!   preview, web addresses in the browser); local PNG images are shown.
//! - Enter continues lists and quotes; Ctrl+B and Ctrl+I make bold and
//!   italic text; the Structure tool window lists the headings.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, text::LayoutJob, Color32, FontFamily, FontId, TextFormat};
use phpide_editor::MarkdownEdit;
use phpide_icons::IconId;

use crate::design::components::IconButton;
use crate::editor_area::Pane;
use crate::markdown_doc::{self as md, Align, Block, BlockKind, Inline};
use crate::PhpIdeApp;

/// How a Markdown file is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Editor,
    Split,
    Preview,
}

impl Layout {
    pub fn id(self) -> &'static str {
        match self {
            Layout::Editor => "editor",
            Layout::Split => "split",
            Layout::Preview => "preview",
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "editor" => Layout::Editor,
            "preview" => Layout::Preview,
            _ => Layout::Split,
        }
    }
}

/// What the preview asks the app to do.
#[derive(Debug, Clone, PartialEq)]
pub enum PreviewAction {
    OpenLink(String),
    ToggleTask(usize),
    GotoLine(usize),
}

#[derive(Default)]
pub struct MarkdownState {
    /// Parsed documents by buffer id: (revision, blocks).
    docs: HashMap<u64, (u64, Arc<Vec<Block>>)>,
    /// Local images by path (`None`: could not be read).
    images: HashMap<PathBuf, Option<egui::TextureHandle>>,
    /// First line the editor showed, by buffer id.
    pub editor_line: HashMap<u64, usize>,
    /// The editor line the preview last followed, by buffer id.
    followed: HashMap<u64, usize>,
    /// Where each block landed in the preview last frame: (line, y).
    tops: HashMap<u64, Vec<(usize, f32)>>,
    /// A heading anchor to scroll the preview to.
    jump_anchor: Option<String>,
    /// Anchors of the headings and their y, by buffer id.
    anchors: HashMap<u64, Vec<(String, f32)>>,
    /// The preview's scroll offset last frame, by buffer id.
    offsets: HashMap<u64, f32>,
}

impl MarkdownState {
    /// The preview's scroll offset (tests).
    #[cfg(test)]
    pub fn preview_offset(&self, buffer: u64) -> f32 {
        self.offsets.get(&buffer).copied().unwrap_or_default()
    }

    /// The parsed document of a buffer, parsed again when it changed.
    pub fn doc(&mut self, buffer: &phpide_core::Buffer) -> Arc<Vec<Block>> {
        let revision = buffer.document.revision();
        match self.docs.get(&buffer.id) {
            Some((rev, doc)) if *rev == revision => doc.clone(),
            _ => {
                let doc = Arc::new(md::parse(&buffer.document.text()));
                self.docs.insert(buffer.id, (revision, doc.clone()));
                doc
            }
        }
    }
}

pub fn is_markdown(detected: &phpide_php::filetype::Detected) -> bool {
    matches!(detected, phpide_php::filetype::Detected::Known(t) if t.id == "markdown")
}

/// Toolbar buttons: (icon, tooltip, action).
const ACTIONS: &[(IconId, &str, MarkdownEdit)] = &[
    (IconId::Bold, "Bold (Ctrl+B)", MarkdownEdit::Bold),
    (IconId::Italic, "Italic (Ctrl+I)", MarkdownEdit::Italic),
    (
        IconId::Strikethrough,
        "Strikethrough",
        MarkdownEdit::Strikethrough,
    ),
    (IconId::Code, "Inline Code", MarkdownEdit::Code),
    (IconId::FileCode, "Code Block", MarkdownEdit::CodeBlock),
    (IconId::Link, "Link", MarkdownEdit::Link),
    (IconId::FileImage, "Image", MarkdownEdit::Image),
    (
        IconId::ListBullet,
        "Bulleted List",
        MarkdownEdit::BulletList,
    ),
    (
        IconId::ListNumbers,
        "Numbered List",
        MarkdownEdit::NumberedList,
    ),
    (IconId::ListCheck, "Task List", MarkdownEdit::TaskList),
    (IconId::Quote, "Quote", MarkdownEdit::Quote),
    (IconId::Table, "Table", MarkdownEdit::Table),
    (IconId::Rule, "Horizontal Rule", MarkdownEdit::Rule),
];

pub const TOOLBAR_HEIGHT: f32 = 30.0;

impl PhpIdeApp {
    /// A Markdown file in a pane: toolbar, then editor and/or preview.
    pub(crate) fn markdown_pane(
        &mut self,
        ui: &mut egui::Ui,
        pane: Pane,
        index: usize,
        style: &phpide_editor::EditorStyle,
    ) {
        let area = ui.available_rect_before_wrap();
        let toolbar = egui::Rect::from_min_size(area.min, egui::vec2(area.width(), TOOLBAR_HEIGHT));
        let body = egui::Rect::from_min_max(egui::pos2(area.left(), toolbar.bottom()), area.max);
        let layout = Layout::from_id(&self.settings.markdown_layout);
        let mut chosen: Option<MarkdownEdit> = None;
        let mut new_layout = layout;
        ui.scope_builder(egui::UiBuilder::new().max_rect(toolbar), |ui| {
            let p = crate::design::current(ui.ctx()).palette;
            ui.painter().rect_filled(toolbar, 0.0, p.bg_panel);
            ui.painter().hline(
                toolbar.x_range(),
                toolbar.bottom() - 0.5,
                egui::Stroke::new(1.0, p.border_subtle),
            );
            ui.horizontal_centered(|ui| {
                ui.add_space(6.0);
                ui.spacing_mut().item_spacing.x = 2.0;
                let editing = layout != Layout::Preview;
                let heading = IconButton::new(IconId::Heading, "Heading").show(ui);
                egui::Popup::menu(&heading).show(|ui| {
                    use crate::design::components::menu_item;
                    ui.set_min_width(160.0);
                    for level in 1..=6u8 {
                        let label = format!("Heading {level}");
                        if menu_item(ui, None, &label, "", editing).clicked() {
                            chosen = Some(MarkdownEdit::Heading(level));
                            ui.close();
                        }
                    }
                    if menu_item(ui, None, "Paragraph", "", editing).clicked() {
                        chosen = Some(MarkdownEdit::Heading(0));
                        ui.close();
                    }
                });
                for (icon, tip, action) in ACTIONS {
                    if editing && IconButton::new(*icon, *tip).show(ui).clicked() {
                        chosen = Some(*action);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(6.0);
                    for (l, icon, tip) in [
                        (Layout::Preview, IconId::Eye, "Preview Only"),
                        (Layout::Split, IconId::SplitRight, "Editor and Preview"),
                        (Layout::Editor, IconId::Edit, "Editor Only"),
                    ] {
                        if IconButton::new(icon, tip)
                            .selected(layout == l)
                            .show(ui)
                            .clicked()
                        {
                            new_layout = l;
                        }
                    }
                });
            });
        });
        if new_layout != layout {
            self.settings.markdown_layout = new_layout.id().to_string();
            self.persist_settings();
            if new_layout != Layout::Preview {
                self.editors.focus = pane;
                self.editors.request_focus();
            }
        }
        if let Some(action) = chosen {
            self.editors.focus = pane;
            self.editors
                .run(phpide_editor::ViewCommand::Markdown(action), &mut self.ws);
            self.editors.request_focus();
        }
        match new_layout {
            Layout::Editor => {
                ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
                    self.editor_pane_inner(ui, pane, index, style)
                });
            }
            Layout::Preview => {
                ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
                    self.markdown_preview(ui, index, false)
                });
            }
            Layout::Split => {
                let middle = body.center().x;
                let left =
                    egui::Rect::from_min_max(body.min, egui::pos2(middle - 1.0, body.bottom()));
                let right =
                    egui::Rect::from_min_max(egui::pos2(middle + 1.0, body.top()), body.max);
                ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
                    self.editor_pane_inner(ui, pane, index, style)
                });
                let p = crate::design::current(ui.ctx()).palette;
                ui.painter().vline(
                    middle,
                    body.y_range(),
                    egui::Stroke::new(1.0, p.border_subtle),
                );
                ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| {
                    self.markdown_preview(ui, index, true)
                });
            }
        }
    }

    /// With a Markdown file active, its headings make the structure (the
    /// Structure tool window and File Structure); returns whether it was one.
    pub(crate) fn markdown_outline(&mut self) -> bool {
        let Some(buffer) = self.ws.active_buffer() else {
            return false;
        };
        if !is_markdown(&self.editors.detect(buffer)) {
            return false;
        }
        let doc = self.markdown.doc(buffer);
        let path = buffer.path.clone();
        let outline: Vec<phpide_core::OutlineEntry> = md::outline(&doc)
            .into_iter()
            .map(|(level, name, line0)| phpide_core::OutlineEntry {
                name,
                kind: "heading".to_string(),
                line0,
                depth: usize::from(level.saturating_sub(1)),
            })
            .collect();
        if self.ws.outline != outline {
            self.ws.outline = outline;
        }
        self.outline_path = path;
        self.outline_from_server = false;
        self.outline_local_revision = None;
        true
    }

    /// The rendered document of buffer `index`; `follow` keeps it level
    /// with the editor's scroll.
    pub(crate) fn markdown_preview(&mut self, ui: &mut egui::Ui, index: usize, follow: bool) {
        let buffer = &self.ws.buffers[index];
        let id = buffer.id;
        let dir = buffer
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.ws.root.clone());
        let doc = self.markdown.doc(buffer);
        let theme = crate::design::current(ui.ctx());
        let p = theme.palette;
        let mono = theme.editor_style().font().clone();
        ui.painter().rect_filled(ui.max_rect(), 0.0, p.bg_editor);
        // Scroll the preview to the block the editor shows, when the editor
        // moved (so the preview can also be scrolled on its own).
        let mut offset = None;
        let editor_line = self.markdown.editor_line.get(&id).copied();
        if follow {
            if let Some(line) = editor_line {
                if self.markdown.followed.get(&id) != Some(&line) {
                    self.markdown.followed.insert(id, line);
                    offset = self.markdown.tops.get(&id).and_then(|tops| {
                        let at = tops.partition_point(|(l, _)| *l <= line);
                        at.checked_sub(1).map(|i| {
                            let (l0, y0) = tops[i];
                            // Between two blocks, in proportion to the lines.
                            match tops.get(i + 1) {
                                Some((l1, y1)) if *l1 > l0 => {
                                    y0 + (y1 - y0) * (line - l0) as f32 / (*l1 - l0) as f32
                                }
                                _ => y0,
                            }
                        })
                    });
                }
            }
        }
        if let Some(anchor) = self.markdown.jump_anchor.take() {
            offset = self
                .markdown
                .anchors
                .get(&id)
                .and_then(|list| list.iter().find(|(a, _)| *a == anchor).map(|(_, y)| *y));
        }
        let mut actions = Vec::new();
        let mut tops = Vec::new();
        let mut anchors = Vec::new();
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt(("markdown-preview", id))
            .auto_shrink([false, false]);
        if let Some(y) = offset {
            scroll = scroll.vertical_scroll_offset(y.max(0.0));
        }
        let images = &mut self.markdown.images;
        let output = scroll.show(ui, |ui| {
            ui.add_space(12.0);
            let content_top = ui.min_rect().top();
            let width = (ui.available_width() - 32.0).clamp(120.0, 900.0);
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.set_width(width);
                    ui.response().widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Markdown Preview")
                    });
                    let mut r = Renderer {
                        p: &p,
                        mono: &mono,
                        dir: &dir,
                        images,
                        actions: &mut actions,
                        tops: &mut tops,
                        anchors: &mut anchors,
                        content_top,
                        width,
                        wrap: true,
                    };
                    r.blocks(ui, &doc, 0);
                });
            });
            ui.add_space(24.0);
        });
        self.markdown.offsets.insert(id, output.state.offset.y);
        self.markdown.tops.insert(id, tops);
        self.markdown.anchors.insert(id, anchors);
        for action in actions {
            self.markdown_action(index, &dir, action);
        }
    }

    fn markdown_action(&mut self, index: usize, dir: &Path, action: PreviewAction) {
        match action {
            PreviewAction::ToggleTask(line) => {
                let buffer = &mut self.ws.buffers[index];
                if buffer.readonly {
                    self.ws.message = "the file is read-only (unlock it in the status bar)".into();
                    return;
                }
                let text = buffer.document.line(line);
                let indent = text.len() - text.trim_start().len();
                if let Some((_, len, true)) = phpide_editor::markdown::list_marker(&text) {
                    let box_at = indent + len - 3;
                    let mark = &text[box_at..box_at + 1];
                    let new = if mark == " " { "x" } else { " " };
                    let start = buffer.document.line_start(line) + text[..box_at].chars().count();
                    buffer.delete(start, start + 1);
                    buffer.insert(start, new);
                }
            }
            PreviewAction::GotoLine(line) => {
                if let Some(buffer) = self.ws.buffers.get_mut(index) {
                    buffer.request_cursor(line, 0);
                }
                self.editors.request_focus();
            }
            PreviewAction::OpenLink(url) => self.markdown_open_link(dir, url),
        }
    }

    /// A link of the preview of buffer `index`: an `#anchor` scrolls the
    /// preview, a web address opens in the browser, a file in the IDE.
    pub(crate) fn markdown_open_link(&mut self, dir: &Path, url: String) {
        if let Some(anchor) = url.strip_prefix('#') {
            self.markdown.jump_anchor = Some(anchor.to_string());
            return;
        }
        if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:") {
            if let Err(e) = crate::services::desktop::open_url(&url) {
                self.ws.message = e;
            }
            return;
        }
        let (file, anchor) = match url.split_once('#') {
            Some((f, a)) => (f, Some(a.to_string())),
            None => (url.as_str(), None),
        };
        let decoded = file.replace("%20", " ");
        let path = if Path::new(&decoded).is_absolute() {
            PathBuf::from(&decoded)
        } else {
            dir.join(&decoded)
        };
        if path.is_file() {
            self.open_file_tab(&path, false);
            self.markdown.jump_anchor = anchor;
        } else {
            self.ws.message = format!("{} does not exist", path.display());
        }
    }
}

/// Paints blocks and collects what the user clicked.
struct Renderer<'a> {
    p: &'a crate::design::tokens::Palette,
    mono: &'a FontId,
    dir: &'a Path,
    images: &'a mut HashMap<PathBuf, Option<egui::TextureHandle>>,
    actions: &'a mut Vec<PreviewAction>,
    tops: &'a mut Vec<(usize, f32)>,
    anchors: &'a mut Vec<(String, f32)>,
    content_top: f32,
    width: f32,
    /// Paragraphs wrap at `width` (not in table cells).
    wrap: bool,
}

/// Text styles of inline runs.
#[derive(Clone, Copy, Default)]
struct Style {
    strong: bool,
    emphasis: bool,
    strike: bool,
    link: bool,
}

impl Renderer<'_> {
    fn blocks(&mut self, ui: &mut egui::Ui, blocks: &[Block], depth: usize) {
        for (n, block) in blocks.iter().enumerate() {
            if n > 0 {
                ui.add_space(8.0);
            }
            if depth == 0 {
                self.tops
                    .push((block.line, ui.cursor().top() - self.content_top));
            }
            let response = self.block(ui, block, depth);
            if response.double_clicked() {
                self.actions.push(PreviewAction::GotoLine(block.line));
            }
        }
    }

    fn block(&mut self, ui: &mut egui::Ui, block: &Block, depth: usize) -> egui::Response {
        let p = self.p;
        match &block.kind {
            BlockKind::Heading {
                level,
                text,
                anchor,
            } => {
                self.anchors
                    .push((anchor.clone(), ui.cursor().top() - self.content_top));
                let size = match level {
                    1 => 26.0,
                    2 => 21.0,
                    3 => 17.5,
                    4 => 15.5,
                    _ => 14.0,
                };
                ui.add_space(if *level <= 2 { 6.0 } else { 2.0 });
                let response = self.paragraph(ui, text, size, true, p.text_primary);
                if *level <= 2 {
                    ui.add_space(4.0);
                    let y = ui.cursor().top();
                    ui.painter().hline(
                        ui.min_rect().left()..=ui.min_rect().left() + self.width,
                        y,
                        egui::Stroke::new(1.0, p.border_subtle),
                    );
                    ui.add_space(2.0);
                }
                response
            }
            BlockKind::Paragraph(text) => self.paragraph(ui, text, 14.0, false, p.text_primary),
            BlockKind::Code { lang, text } => self.code(ui, lang, text),
            BlockKind::Html(text) => self.code(ui, "html", text),
            BlockKind::Rule => {
                ui.add_space(4.0);
                let r = ui.allocate_response(egui::vec2(self.width, 2.0), egui::Sense::click());
                ui.painter().rect_filled(r.rect, 1.0, p.border_subtle);
                ui.add_space(4.0);
                r
            }
            BlockKind::Quote(inner) => {
                let start = ui.cursor().top();
                let left = ui.min_rect().left();
                let r = ui
                    .horizontal(|ui| {
                        ui.add_space(14.0);
                        ui.vertical(|ui| {
                            let saved = self.width;
                            self.width -= 14.0;
                            self.blocks(ui, inner, depth + 1);
                            self.width = saved;
                        });
                    })
                    .response;
                let bar = egui::Rect::from_min_max(
                    egui::pos2(left, start),
                    egui::pos2(left + 3.0, ui.cursor().top()),
                );
                ui.painter()
                    .rect_filled(bar, 1.0, p.accent.gamma_multiply(0.6));
                r
            }
            BlockKind::List {
                ordered,
                start,
                items,
            } => {
                let r = ui.vertical(|ui| {
                    for (n, item) in items.iter().enumerate() {
                        ui.horizontal_top(|ui| {
                            let slot = egui::vec2(22.0, 18.0);
                            if let Some((done, line)) = item.task {
                                let mut checked = done;
                                if ui
                                    .add_sized(slot, egui::Checkbox::without_text(&mut checked))
                                    .changed()
                                {
                                    self.actions.push(PreviewAction::ToggleTask(line));
                                }
                            } else {
                                let marker = if *ordered {
                                    format!("{}.", start + n as u64)
                                } else {
                                    ["•", "◦", "▪"][depth.min(2)].to_string()
                                };
                                ui.add_sized(
                                    slot,
                                    egui::Label::new(
                                        egui::RichText::new(marker).color(p.text_secondary),
                                    ),
                                );
                            }
                            ui.vertical(|ui| {
                                let saved = self.width;
                                self.width -= 30.0;
                                for (k, inner) in item.blocks.iter().enumerate() {
                                    if k > 0 {
                                        ui.add_space(4.0);
                                    }
                                    let nested = matches!(inner.kind, BlockKind::List { .. });
                                    self.block(ui, inner, depth + usize::from(nested));
                                }
                                self.width = saved;
                            });
                        });
                    }
                });
                r.response
            }
            BlockKind::Table {
                aligns,
                header,
                rows,
            } => {
                let r = egui::Frame::new()
                    .stroke(egui::Stroke::new(1.0, p.border_subtle))
                    .corner_radius(3.0)
                    .inner_margin(egui::Margin::symmetric(8, 6))
                    .show(ui, |ui| {
                        egui::Grid::new(("md-table", block.line))
                            .striped(true)
                            .spacing(egui::vec2(18.0, 6.0))
                            .show(ui, |ui| {
                                for (cell, align) in header.iter().zip(aligns) {
                                    self.cell(ui, cell, *align, true);
                                }
                                ui.end_row();
                                for row in rows {
                                    for (cell, align) in row.iter().zip(aligns) {
                                        self.cell(ui, cell, *align, false);
                                    }
                                    ui.end_row();
                                }
                            });
                    });
                r.response
            }
        }
    }

    fn cell(&mut self, ui: &mut egui::Ui, inlines: &[Inline], align: Align, header: bool) {
        let layout = match align {
            Align::Center => egui::Layout::top_down(egui::Align::Center),
            Align::Right => egui::Layout::top_down(egui::Align::Max),
            _ => egui::Layout::top_down(egui::Align::Min),
        };
        ui.with_layout(layout, |ui| {
            self.wrap = false;
            self.paragraph(ui, inlines, 14.0, header, self.p.text_primary);
            self.wrap = true;
        });
    }

    fn code(&mut self, ui: &mut egui::Ui, lang: &str, text: &str) -> egui::Response {
        let p = self.p;
        let mut job = LayoutJob::default();
        let colors = crate::design::current(ui.ctx()).syntax_style_for(Some(&language_of(lang)));
        let font = FontId::new(13.0, self.mono.family.clone());
        let highlighter = grammar_of(lang).and_then(phpide_php::LineHighlighter::for_syntax_name);
        let lines: Vec<&str> = text.lines().collect();
        let spans = highlighter.map(|mut h| {
            h.highlight_lines(0, lines.len().saturating_sub(1), |n| {
                lines.get(n).map(|l| format!("{l}\n")).unwrap_or_default()
            })
        });
        for (n, line) in lines.iter().enumerate() {
            let chars: Vec<char> = line.chars().collect();
            let mut at = 0;
            let line_spans = spans.as_ref().and_then(|s| s.get(n));
            for &(start, end, role) in line_spans.into_iter().flatten() {
                let (start, end) = (start.min(chars.len()), end.min(chars.len()));
                if start > at {
                    let t: String = chars[at..start].iter().collect();
                    job.append(
                        &t,
                        0.0,
                        TextFormat::simple(
                            font.clone(),
                            colors.color(phpide_php::SyntaxRole::Plain),
                        ),
                    );
                }
                if end > start {
                    let t: String = chars[start..end].iter().collect();
                    job.append(
                        &t,
                        0.0,
                        TextFormat::simple(font.clone(), colors.color(role)),
                    );
                }
                at = at.max(end);
            }
            if at < chars.len() {
                let t: String = chars[at..].iter().collect();
                job.append(
                    &t,
                    0.0,
                    TextFormat::simple(font.clone(), colors.color(phpide_php::SyntaxRole::Plain)),
                );
            }
            if n + 1 < lines.len() {
                job.append("\n", 0.0, TextFormat::simple(font.clone(), p.text_primary));
            }
        }
        let r = egui::Frame::new()
            .fill(p.bg_panel)
            .stroke(egui::Stroke::new(1.0, p.border_subtle))
            .corner_radius(4.0)
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(self.width - 22.0);
                egui::ScrollArea::horizontal()
                    .id_salt(("md-code", text.len(), lang))
                    .show(ui, |ui| {
                        ui.add(egui::Label::new(job).extend());
                    });
            });
        r.response
    }

    /// Inline runs as one wrapped text; links are clickable.
    fn paragraph(
        &mut self,
        ui: &mut egui::Ui,
        inlines: &[Inline],
        size: f32,
        strong: bool,
        color: Color32,
    ) -> egui::Response {
        let mut job = LayoutJob::default();
        job.wrap.max_width = if self.wrap { self.width } else { f32::INFINITY };
        let mut links: Vec<(std::ops::Range<usize>, String)> = Vec::new();
        let mut images: Vec<(String, String)> = Vec::new();
        self.runs(
            &mut job,
            inlines,
            Style {
                strong,
                ..Style::default()
            },
            size,
            color,
            &mut links,
            &mut images,
        );
        let response = if job.text.is_empty() && !images.is_empty() {
            ui.allocate_response(egui::vec2(0.0, 0.0), egui::Sense::hover())
        } else {
            let wrap = if self.wrap {
                egui::TextWrapMode::Wrap
            } else {
                egui::TextWrapMode::Extend
            };
            let (pos, galley, response) = egui::Label::new(job)
                .wrap_mode(wrap)
                .sense(egui::Sense::click())
                .layout_in_ui(ui);
            ui.painter().galley(pos, galley.clone(), color);
            let text = galley.text().to_string();
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &text));
            // The link under the pointer.
            let link_at = |point: egui::Pos2| -> Option<String> {
                let cursor = galley.cursor_from_pos(point - pos);
                let index = cursor.index.0;
                links
                    .iter()
                    .find(|(range, _)| range.contains(&index))
                    .map(|(_, url)| url.clone())
            };
            if let Some(hover) = response.hover_pos() {
                if let Some(url) = link_at(hover) {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    response.clone().on_hover_text_at_pointer(url);
                }
            }
            if response.clicked() {
                if let Some(url) = response.interact_pointer_pos().and_then(link_at) {
                    self.actions.push(PreviewAction::OpenLink(url));
                }
            }
            response
        };
        for (alt, url) in images {
            self.image(ui, &alt, &url);
        }
        response
    }

    #[allow(clippy::too_many_arguments)]
    fn runs(
        &self,
        job: &mut LayoutJob,
        inlines: &[Inline],
        style: Style,
        size: f32,
        color: Color32,
        links: &mut Vec<(std::ops::Range<usize>, String)>,
        images: &mut Vec<(String, String)>,
    ) {
        let p = self.p;
        let format = |style: Style, code: bool| {
            let family = if code {
                self.mono.family.clone()
            } else if style.strong {
                crate::design::fonts::family(crate::design::fonts::UI_SEMIBOLD)
            } else {
                FontFamily::Proportional
            };
            let mut f = TextFormat::simple(
                FontId::new(if code { size - 1.0 } else { size }, family),
                if style.link { p.accent } else { color },
            );
            f.italics = style.emphasis;
            if style.strike {
                f.strikethrough = egui::Stroke::new(1.0, color);
            }
            if style.link {
                f.underline = egui::Stroke::new(1.0, p.accent.gamma_multiply(0.6));
            }
            if code {
                f.background = p.bg_panel;
                f.color = if style.link { p.accent } else { p.text_primary };
            }
            f
        };
        for inline in inlines {
            match inline {
                Inline::Text(t) => job.append(t, 0.0, format(style, false)),
                Inline::Code(t) => {
                    job.append(&format!("\u{2009}{t}\u{2009}"), 0.0, format(style, true))
                }
                Inline::LineBreak => job.append("\n", 0.0, format(style, false)),
                Inline::Strong(inner) => self.runs(
                    job,
                    inner,
                    Style {
                        strong: true,
                        ..style
                    },
                    size,
                    color,
                    links,
                    images,
                ),
                Inline::Emphasis(inner) => self.runs(
                    job,
                    inner,
                    Style {
                        emphasis: true,
                        ..style
                    },
                    size,
                    color,
                    links,
                    images,
                ),
                Inline::Strike(inner) => self.runs(
                    job,
                    inner,
                    Style {
                        strike: true,
                        ..style
                    },
                    size,
                    color,
                    links,
                    images,
                ),
                Inline::Link { label, url } => {
                    let start = job.text.chars().count();
                    self.runs(
                        job,
                        label,
                        Style {
                            link: true,
                            ..style
                        },
                        size,
                        color,
                        links,
                        images,
                    );
                    let end = job.text.chars().count();
                    links.push((start..end, url.clone()));
                }
                Inline::Image { alt, url } => images.push((alt.clone(), url.clone())),
            }
        }
    }

    /// A local image under the paragraph (PNG); otherwise its text.
    fn image(&mut self, ui: &mut egui::Ui, alt: &str, url: &str) {
        let local = !url.contains("://");
        let path = if Path::new(url).is_absolute() {
            PathBuf::from(url)
        } else {
            self.dir.join(url.replace("%20", " "))
        };
        let texture = if local {
            self.images
                .entry(path.clone())
                .or_insert_with(|| load_image(ui.ctx(), &path))
                .clone()
        } else {
            None
        };
        match texture {
            Some(texture) => {
                let size = texture.size_vec2();
                let scale = (self.width / size.x).min(1.0);
                ui.add(egui::Image::new(&texture).fit_to_exact_size(size * scale))
                    .on_hover_text(alt);
            }
            None => {
                let label = if alt.is_empty() { url } else { alt };
                ui.label(
                    egui::RichText::new(format!("[image: {label}]"))
                        .color(self.p.text_muted)
                        .italics(),
                )
                .on_hover_text(url);
            }
        }
    }
}

fn load_image(ctx: &egui::Context, path: &Path) -> Option<egui::TextureHandle> {
    let bytes = std::fs::read(path).ok()?;
    let image = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    Some(ctx.load_texture(
        format!("md-image-{}", path.display()),
        color,
        egui::TextureOptions::LINEAR,
    ))
}

/// The syntect grammar of a code block's language.
fn grammar_of(lang: &str) -> Option<&'static str> {
    Some(match lang.to_lowercase().as_str() {
        "php" => "PHP",
        "js" | "javascript" | "jsx" | "mjs" => "JavaScript",
        "ts" | "typescript" | "tsx" => "JavaScript",
        "json" => "JSON",
        "html" | "xml" | "vue" | "blade" => "HTML",
        "css" | "scss" => "CSS",
        "sql" => "SQL",
        "sh" | "bash" | "shell" | "zsh" | "console" => "Bourne Again Shell (bash)",
        "yaml" | "yml" => "YAML",
        "py" | "python" => "Python",
        "rs" | "rust" => "Rust",
        "md" | "markdown" => "Markdown",
        "diff" | "patch" => "Diff",
        _ => return None,
    })
}

/// The file type id whose colors a code block uses.
fn language_of(lang: &str) -> String {
    match lang.to_lowercase().as_str() {
        "js" | "javascript" => "javascript".into(),
        "" => "text".into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_round_trip() {
        for l in [Layout::Editor, Layout::Split, Layout::Preview] {
            assert_eq!(Layout::from_id(l.id()), l);
        }
        assert_eq!(Layout::from_id("?"), Layout::Split);
        assert_eq!(grammar_of("PHP"), Some("PHP"));
        assert_eq!(grammar_of("brainfuck"), None);
    }
}
