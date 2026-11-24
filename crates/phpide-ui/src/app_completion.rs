//! Completion popup and snippet tab stops (Phase 5).
//!
//! The editor keeps the keyboard focus: typing goes on editing the
//! document and the popup filters by the word in front of the caret. While
//! the popup is open it consumes ↑/↓, Page Up/Down, Enter, Tab and Esc
//! before the editor sees them.

use super::*;
use crate::app_lsp::{lsp_client_for_origin, lsp_client_for_path};
use crate::completion::{self, SnippetSession};
use phpide_lsp::CompletionItem;

/// Rows shown before the list scrolls.
const VISIBLE_ROWS: f32 = 10.0;
const LIST_WIDTH: f32 = 420.0;
const DOC_WIDTH: f32 = 340.0;

impl PhpIdeApp {
    /// Ctrl+Space.
    pub(crate) fn trigger_completion(&mut self) {
        self.start_completion(None, true);
    }

    /// Asks the language server for completions at the caret. `explicit`
    /// reports missing servers in the status bar.
    pub(crate) fn start_completion(&mut self, trigger: Option<char>, explicit: bool) {
        self.sync_documents();
        let Some(buf) = self.ws.active_buffer() else {
            return;
        };
        let Some(path) = buf.path.clone() else { return };
        let Some((lsp, origin)) = lsp_client_for_path(&self.lsp, &self.fe, &path) else {
            if explicit {
                self.ws.message = format!(
                    "no language server for {:?}",
                    path.extension().and_then(|e| e.to_str()).unwrap_or("?")
                );
            }
            return;
        };
        if !matches!(lsp.status(), LspStatus::Running(_)) {
            if explicit {
                self.ws.message = "the language server is not ready yet".to_string();
            }
            return;
        }
        if !lsp.capabilities().completion_provider {
            return;
        }
        let cursor = self.cursor_char.min(buf.document.len_chars());
        let (query, word_start) = completion::word_before(&buf.document, cursor);
        let (line, col) = buf
            .document
            .line_col_encoded(cursor, phpide_core::PositionEncoding::Utf16);
        self.completion
            .begin(lsp, &path, origin, line, col, word_start, query, trigger);
    }

    pub(crate) fn close_completion(&mut self) {
        let origin = self.completion.origin;
        let client = lsp_client_for_origin(&self.lsp, &self.fe, origin);
        self.completion.close(client);
    }

    /// Follows the caret and the typed word; reads answers; resolves the
    /// documentation of the selected item.
    pub(crate) fn poll_completion(&mut self) {
        if !self.completion.open {
            return;
        }
        let origin = self.completion.origin;
        let Some(buf) = self.ws.active_buffer() else {
            self.close_completion();
            return;
        };
        if buf.path.as_deref() != self.completion.request_path.as_deref() {
            self.close_completion();
            return;
        }
        let cursor = self.cursor_char.min(buf.document.len_chars());
        let start = self.completion.word_start;
        let rope = buf.document.rope();
        let same_line = start <= cursor
            && rope.char_to_line(start.min(rope.len_chars())) == rope.char_to_line(cursor);
        let typed: String = if same_line {
            rope.slice(start..cursor).to_string()
        } else {
            String::new()
        };
        let word_ok = typed
            .chars()
            .enumerate()
            .all(|(i, c)| c.is_alphanumeric() || c == '_' || c == '\\' || (i == 0 && c == '$'));
        if !same_line || !word_ok {
            self.close_completion();
            return;
        }
        let changed = typed != self.completion.query;
        self.completion.set_query(typed);
        let Some(client) = lsp_client_for_origin(&self.lsp, &self.fe, origin) else {
            self.completion.close(None);
            return;
        };
        if changed && self.completion.incomplete() && !self.completion.loading() {
            // The server limited its answer: ask again for what is typed now.
            self.start_completion(None, false);
            return;
        }
        if !self.completion.poll(client) {
            self.close_completion();
            return;
        }
        let resolve = client.capabilities().completion_resolve;
        self.completion.resolve_selected(client, resolve);
    }

    /// The completion popup; runs before the editor so its keys win.
    pub(crate) fn completion_popup(&mut self, ctx: &egui::Context) {
        if !self.completion.open {
            return;
        }
        let items: Vec<CompletionItem> = self.completion.visible().into_iter().cloned().collect();
        if items.is_empty() && !self.completion.loading() {
            return;
        }
        let (mut down, mut up, mut page_down, mut page_up, mut accept, mut close) =
            (false, false, false, false, false, false);
        ctx.input_mut(|i| {
            down = i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown);
            up = i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp);
            page_down = i.consume_key(egui::Modifiers::NONE, egui::Key::PageDown);
            page_up = i.consume_key(egui::Modifiers::NONE, egui::Key::PageUp);
            if !items.is_empty() {
                accept = i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                    || i.consume_key(egui::Modifiers::NONE, egui::Key::Tab);
            }
            close = i.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
        });
        let moved = down || up || page_down || page_up;
        if down {
            self.completion.move_selection(1);
        }
        if up {
            self.completion.move_selection(-1);
        }
        if page_down {
            self.completion.move_selection(VISIBLE_ROWS as isize);
        }
        if page_up {
            self.completion.move_selection(-(VISIBLE_ROWS as isize));
        }
        if close {
            self.close_completion();
            return;
        }
        if accept {
            self.accept_completion();
            return;
        }

        let theme = crate::design::theme::current(ctx);
        let p = theme.palette;
        let row = theme.metrics.row;
        let list_height = row * VISIBLE_ROWS.min(items.len().max(1) as f32) + 8.0;
        let caret = self.editors.caret_rect.unwrap_or(egui::Rect::from_min_size(
            egui::pos2(200.0, 200.0),
            egui::Vec2::ZERO,
        ));
        let screen = ctx.content_rect();
        let below = caret.bottom() + 4.0 + list_height + 16.0 < screen.bottom();
        let (pivot, at) = if below {
            (
                egui::Align2::LEFT_TOP,
                egui::pos2(caret.left() - 24.0, caret.bottom() + 4.0),
            )
        } else {
            (
                egui::Align2::LEFT_BOTTOM,
                egui::pos2(caret.left() - 24.0, caret.top() - 4.0),
            )
        };
        let selected = self.completion.selected;
        let mut clicked: Option<usize> = None;
        egui::Area::new(egui::Id::new("completion-popup"))
            .order(egui::Order::Foreground)
            .pivot(pivot)
            .fixed_pos(at)
            .constrain(true)
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    crate::design::components::Dialog::frame(ui)
                        .inner_margin(egui::Margin::same(4))
                        .show(ui, |ui| {
                            ui.set_width(LIST_WIDTH);
                            if items.is_empty() {
                                ui.horizontal(|ui| {
                                    crate::design::components::Spinner::show(ui, 14.0);
                                    ui.weak("Loading…");
                                });
                                return;
                            }
                            egui::ScrollArea::vertical()
                                .id_salt("completion-list")
                                .max_height(row * VISIBLE_ROWS)
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    for (index, item) in items.iter().enumerate() {
                                        let response =
                                            completion_row(ui, item, index == selected, &p);
                                        if index == selected && moved {
                                            response.scroll_to_me(None);
                                        }
                                        if response.clicked() {
                                            clicked = Some(index);
                                        }
                                    }
                                });
                            if self.completion.loading() {
                                ui.horizontal(|ui| {
                                    crate::design::components::Spinner::show(ui, 12.0);
                                    ui.weak("Updating…");
                                });
                            }
                        });
                    if let Some(item) = items.get(selected) {
                        let doc = item.documentation.clone();
                        if doc.is_some() || !item.detail.is_empty() {
                            crate::design::components::Dialog::frame(ui)
                                .inner_margin(egui::Margin::same(10))
                                .show(ui, |ui| {
                                    ui.vertical(|ui| {
                                        ui.set_width(DOC_WIDTH);
                                        // The documentation usually repeats the signature.
                                        let repeated =
                                            doc.as_ref().is_some_and(|d| d.contains(&item.detail));
                                        if !item.detail.is_empty() && !repeated {
                                            ui.add(
                                                egui::Label::new(
                                                    egui::RichText::new(&item.detail)
                                                        .monospace()
                                                        .color(p.text_primary),
                                                )
                                                .wrap(),
                                            );
                                        }
                                        if let Some(doc) = doc {
                                            egui::ScrollArea::vertical()
                                                .id_salt("completion-documentation")
                                                .max_height(row * VISIBLE_ROWS)
                                                .show(ui, |ui| {
                                                    crate::markdown::show(ui, &doc, DOC_WIDTH);
                                                });
                                        }
                                    })
                                });
                        }
                    }
                });
            });
        if let Some(index) = clicked {
            self.completion.selected = index;
            self.accept_completion();
        }
    }

    /// Applies the selected item: its `textEdit` or `insertText` (snippets
    /// become tab stops) and its `additionalTextEdits` (imports).
    pub(crate) fn accept_completion(&mut self) {
        let Some(item) = self.completion.selected_item().cloned() else {
            self.close_completion();
            return;
        };
        let word_start = self.completion.word_start;
        self.close_completion();
        let raw = item.raw.clone();
        let cursor = self.cursor_char;
        let Some(buf) = self.ws.active_buffer_mut() else {
            return;
        };
        if buf.readonly {
            self.ws.message = "the file is read-only (unlock it in the status bar)".to_string();
            return;
        }
        let cursor = cursor.min(buf.document.len_chars());
        let edit = completion::accept_edit(&buf.document, &raw, word_start, cursor).unwrap_or(
            completion::AcceptEdit {
                start_char: word_start.min(cursor),
                end_char: cursor,
                new_text: item.label.clone(),
            },
        );
        let snippet = raw["insertTextFormat"].as_u64() == Some(2);
        let (text, stops) = if snippet {
            let expanded = phpide_lsp::snippet::expand(&edit.new_text);
            (expanded.text, expanded.stops)
        } else {
            (edit.new_text.clone(), Vec::new())
        };
        let start = edit.start_char.min(edit.end_char);
        buf.edit(
            phpide_core::ChangeSet::single(start, edit.end_char.max(start), text.clone()),
            phpide_core::EditKind::Other,
            Vec::new(),
            Vec::new(),
        );
        buf.break_undo_group();
        let revision = buf.document.revision();
        let path = buf.path.clone();
        let end = start + text.chars().count();
        // The stops and the end of the insertion move with the import
        // edits below: track them as one session.
        let mut tracked = SnippetSession {
            path: path.clone().unwrap_or_default(),
            revision,
            stops: stops
                .into_iter()
                .map(|(a, b)| (start + a, start + b))
                .chain([(end, end)])
                .collect(),
            next: 0,
        };
        // Imports and other edits the item carries, as one undo step.
        if let Some(path) = path.clone() {
            let version = Some(revision as i64);
            if let Some(extra) =
                phpide_lsp::edits::parse_additional_edits_at_version(&raw, &path, version)
            {
                let label = format!("import for {}", item.label);
                let mut store = WsStore(&mut self.ws);
                if let Err(error) =
                    workspace_edit::apply(&mut store, &extra, &label, &mut self.refactor_undo)
                {
                    self.ws.message = error.to_string();
                }
            }
        }
        if let Some(b) = self.ws.active_buffer() {
            tracked.follow(&b.document);
        }
        let end = tracked.stops.pop().map(|s| s.1).unwrap_or(end);
        if let Some(&(a, b)) = tracked.stops.first() {
            tracked.next = 1;
            self.editors.select(&mut self.ws, a, b);
            self.cursor_char = b;
            // A lone `$0` needs no session.
            self.snippet = (tracked.stops.len() > 1 && path.is_some()).then_some(tracked);
        } else {
            self.editors.set_caret(&mut self.ws, end);
            self.cursor_char = end;
            self.snippet = None;
        }
        self.editors.request_focus();
        if text.contains('(') {
            self.start_signature_help(None, false);
        }
    }

    /// Tab / Shift+Tab walk the stops of the last snippet; Esc ends it.
    pub(crate) fn snippet_keys(&mut self, ctx: &egui::Context) {
        let Some(session) = self.snippet.as_mut() else {
            return;
        };
        let Some(buf) = self.ws.active_buffer() else {
            self.snippet = None;
            return;
        };
        if buf.path.as_ref() != Some(&session.path) || !session.follow(&buf.document) {
            self.snippet = None;
            return;
        }
        let (forward, back, escape) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::Tab),
                i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if escape {
            self.snippet = None;
            return;
        }
        let target = if forward {
            Some(session.next)
        } else if back {
            Some(session.next.saturating_sub(2))
        } else {
            None
        };
        if let Some(index) = target {
            if let Some(&(a, b)) = session.stops.get(index) {
                session.next = index + 1;
                let done = session.next >= session.stops.len();
                self.editors.select(&mut self.ws, a, b);
                self.cursor_char = b;
                if done {
                    self.snippet = None;
                }
            } else {
                self.snippet = None;
            }
        }
    }

    /// Typing in the editor: completion and parameter info triggers.
    pub(crate) fn on_typed(&mut self, typed: &str) {
        let Some(last) = typed.chars().last() else {
            return;
        };
        let Some(buf) = self.ws.active_buffer() else {
            return;
        };
        let Some(path) = buf.path.clone() else { return };
        let Some((lsp, _)) = lsp_client_for_path(&self.lsp, &self.fe, &path) else {
            return;
        };
        let caps = lsp.capabilities();
        let cursor = self.cursor_char.min(buf.document.len_chars());
        let before = |n: usize| -> Option<char> {
            (cursor >= n).then(|| buf.document.rope().char(cursor - n))
        };
        let member =
            (last == '>' && before(2) == Some('-')) || (last == ':' && before(2) == Some(':'));
        let (word, _) = completion::word_before(&buf.document, cursor);
        // Parameter info.
        let sig_trigger = caps.signature_triggers.iter().any(|t| t.starts_with(last));
        let sig_retrigger = caps
            .signature_retriggers
            .iter()
            .any(|t| t.starts_with(last));
        if caps.signature_help_provider {
            if sig_trigger {
                self.start_signature_help(Some(last), self.intel.signature.open);
            } else if self.intel.signature.open {
                self.start_signature_help(if sig_retrigger { Some(last) } else { None }, true);
            }
        }
        if self.completion.open {
            return;
        }
        let server_trigger = caps.completion_triggers.iter().any(|t| t.ends_with(last));
        if member || (server_trigger && !matches!(last, '>' | ':')) {
            self.start_completion(Some(last), false);
        } else if self.settings.completion_auto_popup && (last.is_alphabetic() || last == '_') {
            // Not after a digit-only word or inside a long identifier typed fast.
            if word.chars().count() == 1 || (word.starts_with('$') && word.chars().count() == 2) {
                self.start_completion(None, false);
            }
        }
    }
}

/// One row: kind badge, label, signature or type on the right.
fn completion_row(
    ui: &mut egui::Ui,
    item: &CompletionItem,
    selected: bool,
    p: &crate::design::tokens::Palette,
) -> egui::Response {
    let row = crate::design::theme::current(ui.ctx()).metrics.row;
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, row), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            true,
            selected,
            &item.label,
        )
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 3.0, p.bg_selected_focused);
    } else if response.hovered() {
        painter.rect_filled(rect, 3.0, p.bg_hover);
    }
    let (letter, color) = kind_badge(item.kind, p);
    let badge = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 13.0, rect.center().y),
        egui::vec2(15.0, 15.0),
    );
    painter.rect_filled(badge, 3.0, color.gamma_multiply(0.22));
    painter.text(
        badge.center(),
        egui::Align2::CENTER_CENTER,
        letter,
        egui::FontId::monospace(10.0),
        color,
    );
    let label_color = if item.deprecated {
        p.text_muted
    } else {
        p.text_primary
    };
    let label = painter.layout_no_wrap(
        item.label.clone(),
        egui::FontId::monospace(crate::design::tokens::typography::BODY),
        label_color,
    );
    let label_width = label.size().x;
    let label_pos = egui::pos2(rect.left() + 28.0, rect.center().y - label.size().y / 2.0);
    painter.galley(label_pos, label, label_color);
    if item.deprecated {
        painter.hline(
            label_pos.x..=label_pos.x + label_width,
            rect.center().y,
            egui::Stroke::new(1.0, p.text_muted),
        );
    }
    let detail = item
        .detail
        .rsplit_once(':')
        .filter(|_| matches!(item.kind, 2..=4))
        .map(|(_, ret)| ret.trim().to_string())
        .unwrap_or_else(|| item.detail.clone());
    let room = rect.right() - 8.0 - (label_pos.x + label_width + 16.0);
    if room > 30.0 && !detail.is_empty() {
        let mut job = egui::text::LayoutJob::simple_singleline(
            detail,
            egui::FontId::proportional(crate::design::tokens::typography::SMALL),
            p.text_secondary,
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(room);
        let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
        painter.galley(
            egui::pos2(
                rect.right() - 8.0 - galley.size().x,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            p.text_secondary,
        );
    }
    response
}

/// IntelliJ-style letter badge for a `CompletionItemKind`.
fn kind_badge(kind: u64, p: &crate::design::tokens::Palette) -> (&'static str, egui::Color32) {
    match kind {
        2 | 4 => ("m", p.accent),
        3 => ("f", p.accent),
        5 | 10 => ("p", p.warning),
        6 => ("v", p.info),
        7 | 22 => ("c", p.success),
        8 => ("I", p.success),
        9 => ("n", p.text_secondary),
        13 | 20 => ("e", p.success),
        14 => ("k", p.text_secondary),
        15 => ("s", p.text_secondary),
        21 => ("C", p.warning),
        _ => ("·", p.text_secondary),
    }
}
