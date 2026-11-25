//! Recent Locations (Ctrl+Shift+E, RF-SEL-08): the places the caret
//! visited or edited lately, each with the code around it. Pressing
//! Ctrl+Shift+E again shows only the changed places; typing filters by
//! file name or code.

use std::path::{Path, PathBuf};

use eframe::egui;

use crate::PhpIdeApp;

/// One place: file, 0-based line and column, and the lines around it.
pub struct Location {
    pub path: PathBuf,
    pub line0: usize,
    pub col0: usize,
    pub changed: bool,
    /// `(1-based line number, text)` from one line above to one below.
    pub snippet: Vec<(usize, String)>,
}

#[derive(Default)]
pub struct RecentLocations {
    pub open: bool,
    pub changed_only: bool,
    pub query: String,
    pub selected: usize,
    pub items: Vec<Location>,
    focus: bool,
}

/// Lines `line0 - 1 ..= line0 + 1` of `text`.
fn snippet_of(text: &str, line0: usize) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .skip(line0.saturating_sub(1))
        .take(if line0 == 0 { 2 } else { 3 })
        .map(|(i, line)| (i + 1, line.trim_end().chars().take(160).collect()))
        .collect()
}

impl PhpIdeApp {
    /// Records where the caret is when it leaves a place: another file or
    /// a jump of ten lines or more.
    pub(crate) fn track_location(&mut self) {
        let here = self.ws.active_buffer().and_then(|b| {
            let path = b.path.clone()?;
            let (line, col) = b
                .document
                .line_col(self.cursor_char.min(b.document.len_chars()));
            Some((path, line - 1, col - 1))
        });
        if let (Some((last, line, col)), Some((path, now, _))) = (&self.last_place, &here) {
            if last != path || line.abs_diff(*now) >= 10 {
                self.ws.record_visit(&last.clone(), *line, *col);
            }
        }
        if here.is_some() {
            self.last_place = here;
        }
    }

    /// Ctrl+Shift+E: opens the popup, or toggles "changed only" when it is
    /// already open.
    pub(crate) fn open_recent_locations(&mut self) {
        if self.recent_locations.open {
            self.recent_locations.changed_only = !self.recent_locations.changed_only;
            self.recent_locations.selected = 0;
            return;
        }
        let mut places: Vec<(PathBuf, usize, usize, bool)> = Vec::new();
        if let Some((path, line, col)) = &self.last_place {
            places.push((path.clone(), *line, *col, false));
        }
        let visited = self
            .ws
            .visited_locations
            .iter()
            .rev()
            .map(|(p, l, c)| (p, l, c, false));
        let edited = self
            .ws
            .edit_locations
            .iter()
            .rev()
            .map(|(p, l, c)| (p, l, c, true));
        for (path, line, col, changed) in edited.chain(visited) {
            let near = places
                .iter_mut()
                .find(|(p, l, _, _)| p == path && l.abs_diff(*line) <= 3);
            match near {
                Some(place) => place.3 |= changed,
                None => places.push((path.clone(), *line, *col, changed)),
            }
        }
        let mut texts: Vec<(PathBuf, Option<String>)> = Vec::new();
        let mut items = Vec::new();
        for (path, line0, col0, changed) in places.into_iter().take(50) {
            let text = match texts.iter().find(|(p, _)| p == &path) {
                Some((_, text)) => text.clone(),
                None => {
                    let text = match self
                        .ws
                        .buffers
                        .iter()
                        .find(|b| b.path.as_ref() == Some(&path))
                    {
                        Some(buffer) => Some(buffer.document.text()),
                        None => std::fs::read_to_string(&path).ok(),
                    };
                    texts.push((path.clone(), text.clone()));
                    text
                }
            };
            let Some(text) = text else { continue };
            items.push(Location {
                snippet: snippet_of(&text, line0),
                path,
                line0,
                col0,
                changed,
            });
        }
        self.recent_locations = RecentLocations {
            open: true,
            changed_only: false,
            query: String::new(),
            selected: 0,
            items,
            focus: true,
        };
    }
}

fn matches(item: &Location, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let name = item
        .path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    name.contains(query)
        || item
            .snippet
            .iter()
            .any(|(_, line)| line.to_lowercase().contains(query))
}

pub fn show(app: &mut PhpIdeApp, ctx: &egui::Context) {
    if !app.recent_locations.open {
        return;
    }
    let p = crate::design::current(ctx).palette;
    let root = app.ws.root.clone();
    let state = &mut app.recent_locations;
    let query = state.query.trim().to_lowercase();
    let shown: Vec<usize> = (0..state.items.len())
        .filter(|&i| !state.changed_only || state.items[i].changed)
        .filter(|&i| matches(&state.items[i], &query))
        .collect();
    let (down, up, enter, escape) = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
        )
    });
    if down && !shown.is_empty() {
        state.selected = (state.selected + 1).min(shown.len() - 1);
    }
    if up {
        state.selected = state.selected.saturating_sub(1);
    }
    state.selected = state.selected.min(shown.len().saturating_sub(1));
    let mut chosen: Option<usize> = (enter && !shown.is_empty()).then(|| shown[state.selected]);
    let close = escape;
    let area = egui::Area::new(egui::Id::new("recent-locations"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 56.0))
        .show(ctx, |ui| {
            crate::design::components::Dialog::frame(ui).show(ui, |ui| {
                ui.set_width(640.0);
                ui.horizontal(|ui| {
                    let title = if state.changed_only {
                        "Recently Changed Locations"
                    } else {
                        "Recent Locations"
                    };
                    ui.label(egui::RichText::new(title).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.checkbox(&mut state.changed_only, "Show changed only")
                            .on_hover_text("Ctrl+Shift+E");
                    });
                });
                let field = crate::design::components::TextInput::search(&mut state.query)
                    .hint("Filter by file name or code")
                    .id_salt("recent-locations-query")
                    .show(ui);
                if std::mem::take(&mut state.focus) {
                    field.request_focus();
                }
                if field.has_focus() {
                    ui.memory_mut(|m| {
                        m.set_focus_lock_filter(
                            field.id,
                            egui::EventFilter {
                                vertical_arrows: true,
                                ..Default::default()
                            },
                        )
                    });
                }
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .id_salt("recent-locations-list")
                    .max_height(420.0)
                    .show(ui, |ui| {
                        if shown.is_empty() {
                            ui.label(egui::RichText::new("No locations yet").color(p.text_muted));
                        }
                        for (row, &i) in shown.iter().enumerate() {
                            let item = &state.items[i];
                            let selected = row == state.selected;
                            let name = item
                                .path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            let dir = item
                                .path
                                .parent()
                                .and_then(|d| d.strip_prefix(&root).ok())
                                .map(Path::display)
                                .map(|d| d.to_string())
                                .unwrap_or_default();
                            let frame = egui::Frame::new()
                                .inner_margin(egui::Margin::symmetric(8, 4))
                                .corner_radius(4.0)
                                .fill(if selected {
                                    p.bg_selected
                                } else {
                                    egui::Color32::TRANSPARENT
                                });
                            let inner = frame.show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(&name).strong());
                                    ui.label(
                                        egui::RichText::new(format!("{dir}  :{}", item.line0 + 1))
                                            .small()
                                            .color(p.text_muted),
                                    );
                                    if item.changed {
                                        ui.label(
                                            egui::RichText::new("changed").small().color(p.accent),
                                        );
                                    }
                                });
                                for (number, text) in &item.snippet {
                                    let color = if *number == item.line0 + 1 {
                                        p.text_primary
                                    } else {
                                        p.text_secondary
                                    };
                                    ui.label(
                                        egui::RichText::new(format!("{number:>5}  {text}"))
                                            .monospace()
                                            .color(color),
                                    );
                                }
                            });
                            let response = ui.interact(
                                inner.response.rect,
                                ui.id().with(("location", i)),
                                egui::Sense::click(),
                            );
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    true,
                                    format!("{name}:{}", item.line0 + 1),
                                )
                            });
                            if response.clicked() {
                                chosen = Some(i);
                            }
                            if selected && (up || down) {
                                response.scroll_to_me(None);
                            }
                        }
                    });
            });
        });
    let outside = ctx.input(|i| i.pointer.any_pressed()) && !area.response.contains_pointer();
    if let Some(i) = chosen {
        let item = &app.recent_locations.items[i];
        let (path, line0, col0) = (item.path.clone(), item.line0, item.col0);
        app.recent_locations.open = false;
        app.ws.open_file_at(&path, line0, col0);
        app.editors.request_focus();
        ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    } else if close || outside {
        app.recent_locations.open = false;
    }
}
