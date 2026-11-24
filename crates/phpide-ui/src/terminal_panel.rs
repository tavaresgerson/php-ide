//! Terminal tool window (Alt+F12): the user's shell on a real
//! pseudo-terminal, in tabs.
//!
//! While the terminal has the keyboard, every key goes to the shell (Tab,
//! arrows and Esc included) except the tool window shortcuts; Ctrl+C sends
//! an interrupt unless text is selected. Ctrl+click opens `file:line`
//! references in the editor; the wheel scrolls back.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, Color32, Pos2, Rect};
use phpide_term::{Color as TermColor, Key, Mods, Pty, Terminal};

use crate::design::components::{IconButton, Tab};
use crate::PhpIdeApp;

/// Lines per wheel notch.
const WHEEL_LINES: f32 = 3.0;

pub struct TermSession {
    pub id: u64,
    pub name: String,
    pub pty: Option<Pty>,
    pub term: Terminal,
    /// Lines scrolled up into the scrollback (0 = following the output).
    pub scroll: usize,
    /// Selection as (absolute line, column) anchor and head.
    selection: Option<((usize, usize), (usize, usize))>,
    pub exited: bool,
    cwd: PathBuf,
}

impl TermSession {
    pub fn title(&self) -> String {
        if !self.term.title.is_empty() {
            let title: String = self.term.title.chars().take(30).collect();
            return title;
        }
        self.name.clone()
    }

    /// The visible screen as text (tests).
    #[cfg(test)]
    pub fn screen_text(&self) -> String {
        self.term.screen_text()
    }

    fn selected_text(&self) -> Option<String> {
        let (a, b) = self.selection?;
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        if start == end {
            return None;
        }
        let mut out = String::new();
        for line in start.0..=end.0 {
            let row = self.term.line(line)?;
            let from = if line == start.0 { start.1 } else { 0 };
            let to = if line == end.0 {
                end.1
            } else {
                row.cells.len()
            };
            let text: String = row.cells[from.min(row.cells.len())..to.min(row.cells.len())]
                .iter()
                .filter(|c| c.ch != '\0')
                .map(|c| c.ch)
                .collect();
            out.push_str(text.trim_end());
            if line != end.0 && !row.wrapped {
                out.push('\n');
            }
        }
        Some(out)
    }

    pub fn write(&mut self, bytes: &[u8]) {
        if let Some(pty) = &self.pty {
            let _ = pty.write(bytes);
        }
        self.scroll = 0;
        self.selection = None;
    }
}

#[derive(Default)]
pub struct TerminalState {
    pub sessions: Vec<TermSession>,
    pub active: usize,
    next_id: u64,
    /// The terminal had the keyboard last frame (shortcuts stand aside).
    pub focused: bool,
    focus_request: bool,
    /// The program asked for the bell since it was last shown.
    pub error: Option<String>,
}

impl TerminalState {
    /// Opens a new shell in `cwd`.
    pub fn open(&mut self, ctx: &egui::Context, cwd: &Path) {
        self.next_id += 1;
        let id = self.next_id;
        let (cols, rows) = (100, 24);
        let command = phpide_term::Command::shell(cwd)
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .env("TERM_PROGRAM", "phpide");
        let waker_ctx = ctx.clone();
        let waker: Arc<dyn Fn() + Send + Sync> = Arc::new(move || waker_ctx.request_repaint());
        let pty = match Pty::spawn_with_waker(&command, cols, rows, Some(waker)) {
            Ok(pty) => Some(pty),
            Err(error) => {
                self.error = Some(format!("could not start {}: {error}", command.program));
                None
            }
        };
        let number = self.sessions.len() + 1;
        self.sessions.push(TermSession {
            id,
            name: if number == 1 {
                "Local".to_string()
            } else {
                format!("Local ({number})")
            },
            exited: pty.is_none(),
            pty,
            term: Terminal::new(cols as usize, rows as usize),
            scroll: 0,
            selection: None,
            cwd: cwd.to_path_buf(),
        });
        self.active = self.sessions.len() - 1;
        self.focus_request = true;
    }

    pub fn close(&mut self, index: usize) {
        if index < self.sessions.len() {
            self.sessions.remove(index);
            self.active = self.active.min(self.sessions.len().saturating_sub(1));
        }
    }

    pub fn request_focus(&mut self) {
        self.focus_request = true;
    }

    /// Reads every session's output (once per frame).
    pub fn poll(&mut self) {
        for session in &mut self.sessions {
            let Some(pty) = session.pty.as_mut() else {
                continue;
            };
            let (bytes, exited) = pty.read();
            if !bytes.is_empty() {
                let before = session.term.total_lines();
                session.term.feed(&bytes);
                let replies = session.term.take_replies();
                if !replies.is_empty() {
                    let _ = pty.write(&replies);
                }
                // Keep the scrolled-back view still while output arrives.
                if session.scroll > 0 {
                    session.scroll += session.term.total_lines().saturating_sub(before);
                }
            }
            if exited && !session.exited {
                session.exited = true;
                session
                    .term
                    .feed(b"\r\n\x1b[2m[Process completed]\x1b[0m\r\n");
            }
        }
    }
}

/// The Terminal tool window body.
pub fn show(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    if app.terminal.sessions.is_empty() {
        let root = app.ws.root.clone();
        app.terminal.open(&ctx, &root);
    }
    // Tabs.
    let mut close = None;
    let mut new_session = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (index, session) in app.terminal.sessions.iter().enumerate() {
            let response = Tab::new(session.title())
                .active(index == app.terminal.active)
                .closable(true)
                .show(ui);
            if response.response.clicked() {
                app.terminal.active = index;
                app.terminal.focus_request = true;
            }
            if response.close_clicked {
                close = Some(index);
            }
        }
        if IconButton::new(phpide_icons::IconId::Plus, "New Session")
            .show(ui)
            .clicked()
        {
            new_session = true;
        }
    });
    if let Some(index) = close {
        app.terminal.close(index);
    }
    if new_session {
        let cwd = app
            .terminal
            .sessions
            .get(app.terminal.active)
            .and_then(|s| s.pty.as_ref().and_then(|p| p.cwd()))
            .unwrap_or_else(|| app.ws.root.clone());
        app.terminal.open(&ctx, &cwd);
    }
    if let Some(error) = app.terminal.error.take() {
        app.ws.message = error;
    }
    if app.terminal.sessions.is_empty() {
        return;
    }
    terminal_view(app, ui);
}

fn terminal_view(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let theme = crate::design::current(ui.ctx());
    let p = theme.palette;
    let font = theme.editor_style().font().clone();
    let (char_width, row_height) = ui
        .ctx()
        .fonts_mut(|f| (f.glyph_width(&font, 'M'), f.row_height(&font)));
    let rect = ui.available_rect_before_wrap();
    let cols = ((rect.width() - 8.0) / char_width).floor().max(10.0) as usize;
    let rows = ((rect.height() - 4.0) / row_height).floor().max(2.0) as usize;
    let index = app.terminal.active;
    let id = egui::Id::new("terminal-view").with(app.terminal.sessions[index].id);
    let response = ui.interact(rect, id, egui::Sense::click_and_drag());
    ui.allocate_rect(rect, egui::Sense::hover());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Terminal"));
    if app.terminal.focus_request || response.clicked() || response.drag_started() {
        app.terminal.focus_request = false;
        response.request_focus();
    }
    let focused = response.has_focus();
    app.terminal.focused = focused;
    if focused {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            )
        });
    }

    let session = &mut app.terminal.sessions[index];
    if session.term.cols() != cols || session.term.rows() != rows {
        session.term.resize(cols, rows);
        if let Some(pty) = &session.pty {
            pty.resize(cols as u16, rows as u16);
        }
    }
    let origin = rect.min + egui::vec2(4.0, 2.0);
    let total = session.term.total_lines();
    let first_line = total.saturating_sub(rows + session.scroll);
    let cell_at = |pos: Pos2| -> (usize, usize) {
        let col = ((pos.x - origin.x) / char_width).floor().max(0.0) as usize;
        let row = ((pos.y - origin.y) / row_height).floor().max(0.0) as usize;
        (first_line + row.min(rows - 1), col.min(cols))
    };

    // Mouse: wheel, selection, links.
    let mut open_link: Option<(PathBuf, usize, usize)> = None;
    let modifiers = ui.input(|i| i.modifiers);
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 && !session.term.alternate_screen() {
            let lines = (scroll / row_height * WHEEL_LINES / 3.0).round() as isize;
            let max = session.term.scrollback.len();
            session.scroll = (session.scroll as isize + lines).clamp(0, max as isize) as usize;
        }
    }
    let hover_link = response.hover_pos().and_then(|pos| {
        let (line, col) = cell_at(pos);
        let row = session.term.line(line)?;
        let text: String = row
            .cells
            .iter()
            .map(|c| if c.ch == '\0' { ' ' } else { c.ch })
            .collect();
        phpide_term::find_links(&text)
            .into_iter()
            .find(|l| l.start <= col && col < l.end)
            .map(|l| (line, l))
    });
    if response.drag_started() && !modifiers.ctrl {
        if let Some(pos) = response.interact_pointer_pos() {
            let at = cell_at(pos);
            session.selection = Some((at, at));
        }
    } else if response.dragged() {
        if let (Some(pos), Some((anchor, _))) = (response.interact_pointer_pos(), session.selection)
        {
            session.selection = Some((anchor, cell_at(pos)));
        }
    } else if response.clicked() {
        if let (true, Some((_, link))) = (modifiers.ctrl, &hover_link) {
            let base = session
                .pty
                .as_ref()
                .and_then(|p| p.cwd())
                .unwrap_or_else(|| session.cwd.clone());
            let path = if Path::new(&link.path).is_absolute() {
                PathBuf::from(&link.path)
            } else {
                base.join(&link.path)
            };
            open_link = Some((
                path,
                link.line.unwrap_or(1).saturating_sub(1),
                link.column.unwrap_or(1).saturating_sub(1),
            ));
        } else {
            session.selection = None;
        }
    }
    if hover_link.is_some() && modifiers.ctrl {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
    }

    // Keyboard.
    if focused {
        let events = ui.input(|i| i.events.clone());
        let app_cursor = session.term.modes.app_cursor;
        let bracketed = session.term.modes.bracketed_paste;
        let mut out: Vec<u8> = Vec::new();
        let mut copy = None;
        for event in events {
            match event {
                egui::Event::Text(text) => out.extend(text.as_bytes()),
                egui::Event::Paste(text) => out.extend(phpide_term::encode_paste(&text, bracketed)),
                egui::Event::Copy => match session.selected_text() {
                    Some(text) => copy = Some(text),
                    None => out.push(0x03),
                },
                egui::Event::Cut => out.push(0x18),
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    // Shift+PageUp/Down scroll the view, not the program.
                    if modifiers.shift && matches!(key, egui::Key::PageUp | egui::Key::PageDown) {
                        let max = session.term.scrollback.len() as isize;
                        let delta = if key == egui::Key::PageUp {
                            rows as isize
                        } else {
                            -(rows as isize)
                        };
                        session.scroll = (session.scroll as isize + delta).clamp(0, max) as usize;
                        continue;
                    }
                    // Ctrl+Shift+C copies (Ctrl+C is the interrupt).
                    if modifiers.ctrl && modifiers.shift && key == egui::Key::C {
                        copy = session.selected_text();
                        continue;
                    }
                    let mods = Mods {
                        shift: modifiers.shift,
                        ctrl: modifiers.ctrl,
                        alt: modifiers.alt,
                    };
                    if let Some(key) = map_key(key, modifiers) {
                        if let Some(bytes) = phpide_term::encode_key(key, mods, app_cursor) {
                            out.extend(bytes);
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(text) = copy {
            ui.ctx().copy_text(text);
        }
        if !out.is_empty() {
            session.write(&out);
        }
    }

    // Paint.
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, p.bg_editor);
    let selection = session
        .selection
        .map(|(a, b)| if a <= b { (a, b) } else { (b, a) });
    for screen_row in 0..rows {
        let line = first_line + screen_row;
        let Some(row) = session.term.line(line) else {
            break;
        };
        let y = origin.y + screen_row as f32 * row_height;
        // Backgrounds and selection.
        let mut col = 0;
        while col < row.cells.len() {
            let (fg, bg) = colors(&row.cells[col].style, &p);
            let _ = fg;
            let selected = selection.is_some_and(|(s, e)| (line, col) >= s && (line, col) < e);
            let fill = if selected {
                Some(p.editor_selection_bg)
            } else {
                bg
            };
            if let Some(fill) = fill {
                let r = Rect::from_min_size(
                    egui::pos2(origin.x + col as f32 * char_width, y),
                    egui::vec2(char_width + 0.5, row_height),
                );
                painter.rect_filled(r, 0.0, fill);
            }
            col += 1;
        }
        // Text in runs of one style.
        let mut start = 0;
        while start < row.cells.len() {
            let style = row.cells[start].style;
            let mut end = start + 1;
            while end < row.cells.len()
                && row.cells[end].style == style
                && !phpide_term::term::is_wide(row.cells[end].ch)
                && !phpide_term::term::is_wide(row.cells[start].ch)
            {
                end += 1;
            }
            let text: String = row.cells[start..end]
                .iter()
                .filter(|c| c.ch != '\0')
                .map(|c| c.ch)
                .collect();
            if !text.trim().is_empty() && !style.hidden {
                let (fg, _) = colors(&style, &p);
                let mut format = egui::TextFormat {
                    font_id: font.clone(),
                    color: fg,
                    italics: style.italic,
                    ..Default::default()
                };
                if style.underline {
                    format.underline = egui::Stroke::new(1.0, fg);
                }
                if style.strike {
                    format.strikethrough = egui::Stroke::new(1.0, fg);
                }
                let mut job = egui::text::LayoutJob::default();
                job.append(&text, 0.0, format);
                let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
                let pos = egui::pos2(origin.x + start as f32 * char_width, y);
                painter.galley(pos, galley, fg);
                if style.bold {
                    // Faux bold: the monospace font has no bold cell metrics.
                    let mut job = egui::text::LayoutJob::default();
                    job.append(
                        &text,
                        0.0,
                        egui::TextFormat {
                            font_id: font.clone(),
                            color: fg,
                            ..Default::default()
                        },
                    );
                    let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
                    painter.galley(pos + egui::vec2(0.6, 0.0), galley, fg);
                }
            }
            start = end;
        }
        // Link under the pointer with Ctrl.
        if let (true, Some((link_line, link))) = (modifiers.ctrl, &hover_link) {
            if *link_line == line {
                painter.hline(
                    origin.x + link.start as f32 * char_width
                        ..=origin.x + link.end as f32 * char_width,
                    y + row_height - 2.0,
                    egui::Stroke::new(1.0, p.accent),
                );
            }
        }
    }
    // Cursor.
    if session.scroll == 0 && session.term.modes.cursor_visible && !session.exited {
        let (row, col) = session.term.cursor();
        let r = Rect::from_min_size(
            egui::pos2(
                origin.x + col as f32 * char_width,
                origin.y + row as f32 * row_height,
            ),
            egui::vec2(char_width, row_height),
        );
        if focused {
            painter.rect_filled(r, 0.0, p.editor_caret.gamma_multiply(0.75));
        } else {
            painter.rect_stroke(
                r,
                0.0,
                egui::Stroke::new(1.0, p.editor_caret),
                egui::StrokeKind::Inside,
            );
        }
    }
    // Context menu.
    let mut paste = false;
    let mut clear = false;
    response.context_menu(|ui| {
        let session = &app.terminal.sessions[index];
        if ui
            .add_enabled(
                session.selection.is_some(),
                egui::Button::new("Copy").shortcut_text("Ctrl+Shift+C"),
            )
            .clicked()
        {
            if let Some(text) = session.selected_text() {
                ui.ctx().copy_text(text);
            }
            ui.close();
        }
        if ui
            .add(egui::Button::new("Paste").shortcut_text("Ctrl+Shift+V"))
            .clicked()
        {
            paste = true;
            ui.close();
        }
        if ui.button("Clear Buffer").clicked() {
            clear = true;
            ui.close();
        }
    });
    if paste {
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
        app.terminal.focus_request = true;
    }
    if clear {
        let session = &mut app.terminal.sessions[index];
        session.term.clear_scrollback();
        session.write(b"\x0c");
    }
    if let Some((path, line, col)) = open_link {
        if path.is_file() {
            app.ws.open_file_at(&path, line, col);
            app.editors.request_focus();
        } else {
            app.ws.message = format!("{} does not exist", path.display());
        }
    }
}

fn map_key(key: egui::Key, modifiers: egui::Modifiers) -> Option<Key> {
    use egui::Key as K;
    Some(match key {
        K::Enter => Key::Enter,
        K::Backspace => Key::Backspace,
        K::Tab => Key::Tab,
        K::Escape => Key::Escape,
        K::ArrowUp => Key::Up,
        K::ArrowDown => Key::Down,
        K::ArrowLeft => Key::Left,
        K::ArrowRight => Key::Right,
        K::Home => Key::Home,
        K::End => Key::End,
        K::PageUp => Key::PageUp,
        K::PageDown => Key::PageDown,
        K::Insert => Key::Insert,
        K::Delete => Key::Delete,
        K::F1 => Key::F(1),
        K::F2 => Key::F(2),
        K::F3 => Key::F(3),
        K::F4 => Key::F(4),
        K::F5 => Key::F(5),
        K::F6 => Key::F(6),
        K::F7 => Key::F(7),
        K::F8 => Key::F(8),
        K::F9 => Key::F(9),
        K::F10 => Key::F(10),
        K::F11 => Key::F(11),
        K::F12 => Key::F(12),
        // Letters and symbols only with Ctrl or Alt; plain typing comes as
        // text events.
        _ if modifiers.ctrl || modifiers.alt => {
            let name = key.name();
            let c = if name.chars().count() == 1 {
                name.chars().next()?.to_ascii_lowercase()
            } else {
                match key {
                    K::Space => ' ',
                    K::OpenBracket => '[',
                    K::CloseBracket => ']',
                    K::Backslash => '\\',
                    K::Minus => '-',
                    K::Slash => '/',
                    _ => return None,
                }
            };
            Key::Char(c)
        }
        _ => return None,
    })
}

/// Foreground and (non-default) background of a cell style.
fn colors(
    style: &phpide_term::Style,
    p: &crate::design::tokens::Palette,
) -> (Color32, Option<Color32>) {
    let resolve = |color: TermColor, bold: bool| -> Option<Color32> {
        match color {
            TermColor::Default => None,
            TermColor::Indexed(i) if i < 16 => {
                let i = if bold && i < 8 { i + 8 } else { i };
                Some(p.terminal_ansi[i as usize])
            }
            TermColor::Indexed(i) => {
                let (r, g, b) = phpide_term::term::xterm_rgb(i);
                Some(crate::design::tokens::program_color(r, g, b))
            }
            TermColor::Rgb(r, g, b) => Some(crate::design::tokens::program_color(r, g, b)),
        }
    };
    let mut fg = resolve(style.fg, style.bold).unwrap_or(p.text_primary);
    let mut bg = resolve(style.bg, false);
    if style.inverse {
        let new_fg = bg.unwrap_or(p.bg_editor);
        bg = Some(fg);
        fg = new_fg;
    }
    if style.dim {
        fg = fg.gamma_multiply(0.6);
    }
    (fg, bg)
}
