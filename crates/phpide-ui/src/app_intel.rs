//! Language features on top of the editor (Phase 5): Quick Documentation
//! (hover, Ctrl+Q), Parameter Info (Ctrl+P), Rename (Shift+F6), context
//! actions (Alt+Enter), Reformat Code (Ctrl+Alt+L), File Structure
//! (Ctrl+F12), edits and messages the server sends, and the server log.

use std::time::{Duration, Instant};

use super::*;
use crate::design::components::{Dialog, ListRow, TextInput};

/// How long the pointer rests on a word before its documentation shows.
const HOVER_DELAY: Duration = Duration::from_millis(500);
const POPUP_WIDTH: f32 = 460.0;

#[derive(Default)]
pub struct IntelState {
    pub signature: SignatureState,
    pub hover: HoverState,
    pub rename: Option<RenameState>,
    pub actions: Option<ActionsState>,
    /// Reformat Code in flight: (path, revision, answer).
    pub formatting: Option<(PathBuf, u64, mpsc::Receiver<Value>)>,
    /// Code action being resolved before it is applied.
    pub resolving_action: Option<mpsc::Receiver<Value>>,
    pub structure: Option<StructureState>,
    pub log_open: bool,
}

#[derive(Default)]
pub struct SignatureState {
    pub open: bool,
    path: Option<PathBuf>,
    /// (revision, caret) the shown help was asked for.
    asked: Option<(u64, usize)>,
    rx: Option<mpsc::Receiver<Value>>,
    pub help: Option<Value>,
}

#[derive(Default)]
pub struct HoverState {
    /// Char under the pointer and since when.
    resting: Option<(usize, Instant, egui::Pos2)>,
    pending: Option<(PathBuf, usize, mpsc::Receiver<Value>)>,
    pub shown: Option<HoverPopup>,
    /// Screen rect of the popup last frame (the pointer may move into it).
    rect: Option<egui::Rect>,
}

pub struct HoverPopup {
    path: PathBuf,
    /// Char the documentation is about.
    at: usize,
    revision: u64,
    anchor: egui::Pos2,
    pub text: Option<String>,
    pub diagnostics: Vec<(phpide_core::Severity, String)>,
    /// Opened with Ctrl+Q: stays until Esc or the caret moves.
    keyboard: bool,
}

pub struct RenameState {
    path: PathBuf,
    at: usize,
    revision: u64,
    pub name: String,
    original: String,
    prepare: Option<mpsc::Receiver<Value>>,
    request: Option<mpsc::Receiver<Value>>,
    focus: bool,
}

pub struct ActionsState {
    path: PathBuf,
    revision: u64,
    rx: Option<mpsc::Receiver<Value>>,
    pub items: Vec<Value>,
    selected: usize,
}

pub struct StructureState {
    query: String,
    selected: usize,
}

impl PhpIdeApp {
    /// The PHP language server when it is running.
    fn running_lsp(&self) -> Option<&LspClient> {
        self.lsp
            .as_ref()
            .filter(|lsp| matches!(lsp.status(), LspStatus::Running(_)))
    }

    /// Active file, its revision and the caret as an LSP position.
    fn caret_position(&self) -> Option<(PathBuf, u64, usize, usize, usize)> {
        let buf = self.ws.active_buffer()?;
        let path = buf.path.clone()?;
        let caret = self.cursor_char.min(buf.document.len_chars());
        let (line, col) = buf
            .document
            .line_col_encoded(caret, phpide_core::PositionEncoding::Utf16);
        Some((path, buf.document.revision(), caret, line, col))
    }

    /// Reads every pending answer of the features in this module.
    pub(crate) fn poll_intel(&mut self) {
        self.poll_server_edits();
        self.poll_signature();
        self.poll_hover();
        self.poll_rename();
        self.poll_actions();
        self.poll_formatting();
    }

    // ----------------------------------------------------------------- server

    /// `workspace/applyEdit` and `window/showMessage` from the server.
    fn poll_server_edits(&mut self) {
        let Some(lsp) = &self.lsp else { return };
        let edits = lsp.take_apply_edits();
        let messages = lsp.take_messages();
        for edit in edits {
            self.apply_workspace_edit(&edit, "server edit");
        }
        if let Some((_, text)) = messages.last() {
            self.ws.message = format!("{}: {text}", self.lsp_name);
        }
    }

    /// Applies a `WorkspaceEdit` as one undo step. Returns (files, edits).
    pub(crate) fn apply_workspace_edit(
        &mut self,
        edit: &Value,
        label: &str,
    ) -> Option<(usize, usize)> {
        let Some(parsed) = phpide_lsp::edits::parse_workspace_edit(edit) else {
            self.ws.message = format!("{label}: the server sent an edit that cannot be applied");
            return None;
        };
        if parsed.is_empty() {
            return Some((0, 0));
        }
        let mut store = WsStore(&mut self.ws);
        match workspace_edit::apply(&mut store, &parsed, label, &mut self.refactor_undo) {
            Ok((files, edits, _)) => Some((files, edits)),
            Err(error) => {
                self.ws.message = format!("{label}: {error}");
                None
            }
        }
    }

    // ---------------------------------------------------------- parameter info

    /// Ctrl+P, or `(` / `,` typed: asks for the signature at the caret.
    pub(crate) fn start_signature_help(&mut self, trigger: Option<char>, retrigger: bool) {
        self.sync_documents();
        let Some((path, revision, caret, line, col)) = self.caret_position() else {
            return;
        };
        let Some(lsp) = self.running_lsp() else {
            if trigger.is_none() && !retrigger {
                self.ws.message = "Parameter Info needs a language server".to_string();
            }
            return;
        };
        if !lsp.capabilities().signature_help_provider {
            return;
        }
        let rx = lsp.signature_help(&path, line, col, trigger, retrigger);
        let state = &mut self.intel.signature;
        state.path = Some(path);
        state.asked = Some((revision, caret));
        state.rx = Some(rx);
        if !retrigger && trigger.is_none() {
            state.open = true;
        }
    }

    fn poll_signature(&mut self) {
        let state = &mut self.intel.signature;
        if let Some(rx) = &state.rx {
            match rx.try_recv() {
                Ok(value) => {
                    state.rx = None;
                    let has = value["signatures"]
                        .as_array()
                        .is_some_and(|s| !s.is_empty());
                    state.open = has;
                    state.help = has.then_some(value);
                }
                Err(mpsc::TryRecvError::Disconnected) => state.rx = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if !state.open {
            return;
        }
        // Follow the caret: ask again when it moves or the text changes.
        let (path, asked, waiting) = (state.path.clone(), state.asked, state.rx.is_some());
        match self.caret_position() {
            Some((current, revision, caret, _, _)) if Some(&current) == path.as_ref() => {
                if !waiting && asked != Some((revision, caret)) {
                    self.start_signature_help(None, true);
                }
            }
            _ => {
                self.intel.signature.open = false;
                self.intel.signature.help = None;
            }
        }
    }

    pub(crate) fn signature_popup(&mut self, ctx: &egui::Context) {
        if !self.intel.signature.open {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !self.completion.open {
            self.intel.signature = SignatureState::default();
            return;
        }
        let Some(help) = self.intel.signature.help.clone() else {
            return;
        };
        let Some(caret) = self.editors.caret_rect else {
            return;
        };
        let active_signature = help["activeSignature"].as_u64().unwrap_or(0) as usize;
        let signatures = help["signatures"].as_array().cloned().unwrap_or_default();
        let Some(signature) = signatures.get(active_signature).or(signatures.first()) else {
            return;
        };
        let active = signature["activeParameter"]
            .as_u64()
            .or(help["activeParameter"].as_u64())
            .unwrap_or(0) as usize;
        let label = signature["label"].as_str().unwrap_or("").to_string();
        let ranges = parameter_ranges(&label, signature["parameters"].as_array());
        let p = crate::design::theme::current(ctx).palette;
        let font = egui::FontId::monospace(crate::design::tokens::typography::BODY);
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = POPUP_WIDTH;
        let mut cursor = 0;
        let chars: Vec<char> = label.chars().collect();
        let push = |job: &mut egui::text::LayoutJob, from: usize, to: usize, strong: bool| {
            if to <= from {
                return;
            }
            let text: String = chars[from..to].iter().collect();
            job.append(
                &text,
                0.0,
                egui::TextFormat {
                    font_id: font.clone(),
                    color: if strong {
                        p.text_primary
                    } else {
                        p.text_secondary
                    },
                    background: if strong {
                        p.bg_selected
                    } else {
                        Default::default()
                    },
                    ..Default::default()
                },
            );
        };
        if let Some(&(a, b)) = ranges.get(active) {
            push(&mut job, cursor, a, false);
            push(&mut job, a, b, true);
            cursor = b;
        }
        push(&mut job, cursor, chars.len(), false);
        let parameter_doc = signature["parameters"]
            .get(active)
            .and_then(|param| phpide_lsp::documentation_text(&param["documentation"]));
        let doc = phpide_lsp::documentation_text(&signature["documentation"]);
        let count = signatures.len();
        egui::Area::new(egui::Id::new("parameter-info"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::LEFT_BOTTOM)
            .fixed_pos(egui::pos2(caret.left() - 8.0, caret.top() - 4.0))
            .constrain(true)
            .interactable(false)
            .show(ctx, |ui| {
                Dialog::frame(ui)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_max_width(POPUP_WIDTH);
                        ui.horizontal(|ui| {
                            if count > 1 {
                                ui.weak(format!("{}/{count}", active_signature + 1));
                            }
                            ui.add(egui::Label::new(job).wrap());
                        });
                        if let Some(text) = parameter_doc.or(doc) {
                            ui.separator();
                            crate::markdown::show(ui, &text, POPUP_WIDTH);
                        }
                    });
            });
    }

    // -------------------------------------------------------- documentation

    /// The pointer rests on `pos` (from the editor) at `pointer`.
    pub(crate) fn note_hover(&mut self, pos: Option<usize>, pointer: Option<egui::Pos2>) {
        let state = &mut self.intel.hover;
        let over_popup =
            matches!((state.rect, pointer), (Some(r), Some(p)) if r.expand(8.0).contains(p));
        match (pos, pointer) {
            (Some(pos), Some(pointer)) => {
                let same_word = state.resting.is_some_and(|(at, _, _)| {
                    self.ws.active_buffer().is_some_and(|b| {
                        completion::word_around(&b.document, at).1
                            == completion::word_around(&b.document, pos).1
                    })
                });
                if !same_word {
                    state.resting = Some((pos, Instant::now(), pointer));
                    if state.shown.as_ref().is_some_and(|s| !s.keyboard) && !over_popup {
                        state.shown = None;
                        state.rect = None;
                    }
                }
            }
            _ => {
                state.resting = None;
                if !over_popup && state.shown.as_ref().is_some_and(|s| !s.keyboard) {
                    state.shown = None;
                    state.rect = None;
                }
            }
        }
    }

    /// Ctrl+Q: documentation of the element at the caret.
    pub(crate) fn quick_documentation(&mut self) {
        self.sync_documents();
        let Some((path, revision, caret, line, col)) = self.caret_position() else {
            return;
        };
        let anchor = self
            .editors
            .caret_rect
            .map(|r| r.left_bottom() + egui::vec2(0.0, 4.0))
            .unwrap_or(egui::pos2(200.0, 200.0));
        let diagnostics = self.diagnostics_at(&path, caret);
        let rx = self
            .running_lsp()
            .filter(|lsp| lsp.capabilities().hover_provider)
            .map(|lsp| lsp.hover(&path, line, col));
        if rx.is_none() && diagnostics.is_empty() {
            self.ws.message = "No documentation found".to_string();
            return;
        }
        self.intel.hover.shown = Some(HoverPopup {
            path: path.clone(),
            at: caret,
            revision,
            anchor,
            text: None,
            diagnostics,
            keyboard: true,
        });
        self.intel.hover.pending = rx.map(|rx| (path, caret, rx));
    }

    fn diagnostics_at(&self, path: &Path, pos: usize) -> Vec<(phpide_core::Severity, String)> {
        let Some(buf) = self.ws.active_buffer() else {
            return Vec::new();
        };
        let doc = &buf.document;
        let line = doc.rope().char_to_line(pos.min(doc.len_chars()));
        let (_, word_start) = completion::word_around(doc, pos);
        self.ws
            .diagnostics
            .iter()
            .filter(|d| d.path == path && d.line == line + 1)
            .filter(|d| {
                let start = doc.line_start(line) + d.col.saturating_sub(1);
                let (word, from) = completion::word_around(doc, start);
                let end = (from + word.chars().count()).max(start + 1);
                (start <= pos && pos <= end) || from == word_start
            })
            .map(|d| (d.severity, d.message.clone()))
            .collect()
    }

    fn poll_hover(&mut self) {
        // Answers.
        if let Some((path, at, rx)) = &self.intel.hover.pending {
            match rx.try_recv() {
                Ok(value) => {
                    let (path, at) = (path.clone(), *at);
                    self.intel.hover.pending = None;
                    let text = phpide_lsp::documentation_text(&value["contents"]);
                    if let Some(shown) = self.intel.hover.shown.as_mut() {
                        if shown.path == path && shown.at == at {
                            shown.text = text;
                            if shown.text.is_none() && shown.diagnostics.is_empty() {
                                if shown.keyboard {
                                    self.ws.message = "No documentation found".to_string();
                                }
                                self.intel.hover.shown = None;
                            }
                        }
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => self.intel.hover.pending = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        // A keyboard popup closes when the caret or the text moves.
        if let Some(shown) = &self.intel.hover.shown {
            let current = self
                .ws
                .active_buffer()
                .map(|b| (b.path.clone(), b.document.revision()));
            let moved = current != Some((Some(shown.path.clone()), shown.revision))
                || (shown.keyboard && self.cursor_char != shown.at);
            if moved {
                self.intel.hover.shown = None;
                self.intel.hover.rect = None;
            }
        }
        // The pointer rested long enough: ask.
        let Some((pos, since, pointer)) = self.intel.hover.resting else {
            return;
        };
        if since.elapsed() < HOVER_DELAY {
            return;
        }
        if self
            .intel
            .hover
            .shown
            .as_ref()
            .is_some_and(|s| s.at == pos || s.keyboard)
        {
            return;
        }
        let Some(buf) = self.ws.active_buffer() else {
            return;
        };
        let Some(path) = buf.path.clone() else { return };
        let revision = buf.document.revision();
        let (line, col) = buf
            .document
            .line_col_encoded(pos, phpide_core::PositionEncoding::Utf16);
        let diagnostics = self.diagnostics_at(&path, pos);
        self.sync_documents();
        let rx = self
            .running_lsp()
            .filter(|lsp| lsp.capabilities().hover_provider)
            .map(|lsp| lsp.hover(&path, line, col));
        if rx.is_none() && diagnostics.is_empty() {
            self.intel.hover.resting = None;
            return;
        }
        self.intel.hover.shown = Some(HoverPopup {
            path: path.clone(),
            at: pos,
            revision,
            anchor: pointer + egui::vec2(0.0, 18.0),
            text: None,
            diagnostics,
            keyboard: false,
        });
        self.intel.hover.pending = rx.map(|rx| (path, pos, rx));
    }

    pub(crate) fn hover_popup(&mut self, ctx: &egui::Context) {
        if let Some((_, since, _)) = self.intel.hover.resting {
            if since.elapsed() < HOVER_DELAY {
                ctx.request_repaint_after(HOVER_DELAY - since.elapsed());
            }
        }
        let Some(shown) = &self.intel.hover.shown else {
            return;
        };
        if shown.keyboard && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.intel.hover.shown = None;
            return;
        }
        if shown.text.is_none() && shown.diagnostics.is_empty() {
            return;
        }
        let p = crate::design::theme::current(ctx).palette;
        let response = egui::Area::new(egui::Id::new("quick-documentation"))
            .order(egui::Order::Tooltip)
            .fixed_pos(shown.anchor)
            .constrain(true)
            .show(ctx, |ui| {
                Dialog::frame(ui)
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_max_width(POPUP_WIDTH);
                        for (severity, message) in &shown.diagnostics {
                            let (icon, color) = match severity {
                                phpide_core::Severity::Error => {
                                    (phpide_icons::IconId::Error, p.error)
                                }
                                phpide_core::Severity::Warning => {
                                    (phpide_icons::IconId::Warning, p.warning)
                                }
                                _ => (phpide_icons::IconId::Info, p.info),
                            };
                            ui.horizontal_top(|ui| {
                                crate::design::components::icon(ui, icon, 14.0, color);
                                ui.add(egui::Label::new(message).wrap());
                            });
                        }
                        if let Some(text) = &shown.text {
                            if !shown.diagnostics.is_empty() {
                                ui.separator();
                            }
                            egui::ScrollArea::vertical()
                                .max_height(320.0)
                                .show(ui, |ui| crate::markdown::show(ui, text, POPUP_WIDTH));
                        }
                    });
            })
            .response;
        self.intel.hover.rect = Some(response.rect);
    }

    // ----------------------------------------------------------------- rename

    /// Shift+F6 in the editor.
    pub(crate) fn rename_symbol(&mut self) {
        self.sync_documents();
        let Some((path, revision, caret, line, col)) = self.caret_position() else {
            return;
        };
        let Some(lsp) = self.running_lsp() else {
            self.ws.message = "Rename needs a language server".to_string();
            return;
        };
        let caps = lsp.capabilities();
        if !caps.rename_provider {
            self.ws.message = format!("{} cannot rename symbols", self.lsp_name);
            return;
        }
        let word = self
            .ws
            .active_buffer()
            .map(|b| completion::word_around(&b.document, caret).0)
            .unwrap_or_default();
        let prepare = caps
            .prepare_rename
            .then(|| lsp.prepare_rename(&path, line, col));
        if prepare.is_none() && word.is_empty() {
            self.ws.message = "Nothing to rename at the caret".to_string();
            return;
        }
        self.intel.rename = Some(RenameState {
            path,
            at: caret,
            revision,
            name: word.clone(),
            original: word,
            prepare,
            request: None,
            focus: true,
        });
    }

    fn poll_rename(&mut self) {
        let Some(state) = self.intel.rename.as_mut() else {
            return;
        };
        if let Some(rx) = &state.prepare {
            match rx.try_recv() {
                Ok(value) => {
                    state.prepare = None;
                    if value.is_null() || value.get("code").is_some() {
                        let reason = value["message"]
                            .as_str()
                            .unwrap_or("This element cannot be renamed")
                            .to_string();
                        self.intel.rename = None;
                        self.ws.message = reason;
                        return;
                    }
                    let placeholder =
                        value["placeholder"].as_str().map(String::from).or_else(|| {
                            // A bare range: the text it covers.
                            let range = if value.get("range").is_some() {
                                &value["range"]
                            } else {
                                &value
                            };
                            let buf = self.ws.active_buffer()?;
                            let start = phpide_lsp::sync::char_at(&buf.document, &range["start"]);
                            let end = phpide_lsp::sync::char_at(&buf.document, &range["end"]);
                            Some(buf.document.rope().slice(start..end.max(start)).to_string())
                        });
                    if let Some(name) = placeholder.filter(|n| !n.is_empty()) {
                        state.original = name.clone();
                        state.name = name;
                        state.focus = true;
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => state.prepare = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let Some(state) = self.intel.rename.as_mut() else {
            return;
        };
        let Some(rx) = &state.request else { return };
        match rx.try_recv() {
            Ok(value) => {
                let (old, new) = (state.original.clone(), state.name.clone());
                self.intel.rename = None;
                if value.is_null() || value.get("code").is_some() {
                    self.ws.message = value["message"]
                        .as_str()
                        .unwrap_or("The server could not rename this element")
                        .to_string();
                    return;
                }
                let label = format!("Rename '{old}' to '{new}'");
                if let Some((files, edits)) = self.apply_workspace_edit(&value, &label) {
                    self.ws.message = format!(
                        "Renamed '{old}' to '{new}': {edits} occurrence{} in {files} file{}",
                        if edits == 1 { "" } else { "s" },
                        if files == 1 { "" } else { "s" }
                    );
                }
                self.editors.request_focus();
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.intel.rename = None;
                self.ws.message = "The language server stopped during the rename".to_string();
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }

    pub(crate) fn rename_dialog(&mut self, ctx: &egui::Context) {
        let Some(state) = self.intel.rename.as_mut() else {
            return;
        };
        let mut submit = false;
        let mut cancel = false;
        let busy = state.prepare.is_some() || state.request.is_some();
        let title = if state.original.is_empty() {
            "Rename".to_string()
        } else {
            format!("Rename '{}'", state.original)
        };
        let p = crate::design::theme::current(ctx).palette;
        egui::Modal::new(egui::Id::new("rename-symbol"))
            .backdrop_color(p.shadow)
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                Dialog::frame(ui)
                    .inner_margin(egui::Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(360.0);
                        ui.label(egui::RichText::new(title).strong());
                        ui.add_space(6.0);
                        let response = TextInput::new(&mut state.name)
                            .id_salt("rename-symbol-name")
                            .width(340.0)
                            .enabled(state.prepare.is_none())
                            .show(ui);
                        if state.focus && state.prepare.is_none() {
                            response.request_focus();
                            state.focus = false;
                        }
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if busy {
                                crate::design::components::Spinner::show(ui, 14.0);
                            }
                            ui.weak("Updates every usage in the project (Edit › Undo Last Refactoring reverts it).");
                        });
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            let ready = !busy
                                && !state.name.trim().is_empty()
                                && state.name != state.original;
                            if crate::design::components::Button::primary("Rename")
                                .enabled(ready)
                                .show(ui)
                                .clicked()
                            {
                                submit = true;
                            }
                            if crate::design::components::Button::new("Cancel")
                                .show(ui)
                                .clicked()
                            {
                                cancel = true;
                            }
                        });
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            cancel = true;
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::Enter))
                            && !busy
                            && !state.name.trim().is_empty()
                            && state.name != state.original
                        {
                            submit = true;
                        }
                    })
            });
        if cancel {
            self.intel.rename = None;
            self.editors.request_focus();
            return;
        }
        if submit {
            let (path, revision, at, name) = {
                let s = self.intel.rename.as_ref().unwrap();
                (s.path.clone(), s.revision, s.at, s.name.trim().to_string())
            };
            let current = self.ws.active_buffer().map(|b| b.document.revision());
            if current != Some(revision) {
                self.intel.rename = None;
                self.ws.message = "The file changed; rename again".to_string();
                return;
            }
            let Some(buf) = self.ws.active_buffer() else {
                return;
            };
            let (line, col) = buf
                .document
                .line_col_encoded(at, phpide_core::PositionEncoding::Utf16);
            self.sync_documents();
            let rx = self
                .running_lsp()
                .map(|lsp| lsp.rename(&path, line, col, &name));
            match rx {
                Some(rx) => {
                    let state = self.intel.rename.as_mut().unwrap();
                    state.name = name;
                    state.request = Some(rx);
                }
                None => self.intel.rename = None,
            }
        }
    }

    // --------------------------------------------------------- context actions

    /// Alt+Enter: quick fixes and refactorings at the caret.
    pub(crate) fn show_context_actions(&mut self) {
        self.sync_documents();
        let Some((path, revision, caret, _, _)) = self.caret_position() else {
            return;
        };
        let (start, end) = self.editors.selection(&self.ws).unwrap_or((caret, caret));
        let Some(lsp) = self.running_lsp() else {
            self.ws.message = "No context actions without a language server".to_string();
            return;
        };
        if !lsp.capabilities().code_action_provider {
            self.ws.message = "No context actions available here".to_string();
            return;
        }
        let Some(buf) = self.ws.active_buffer() else {
            return;
        };
        let enc = phpide_core::PositionEncoding::Utf16;
        let range = phpide_lsp::lsp_range(
            buf.document.line_col_encoded(start, enc),
            buf.document.line_col_encoded(end, enc),
        );
        let line = buf.document.rope().char_to_line(caret);
        let diagnostics: Vec<Value> = self
            .lsp_raw_diagnostics
            .get(&path)
            .map(|all| {
                all.iter()
                    .filter(|(first, last, _)| *first <= line && line <= *last)
                    .map(|(_, _, raw)| raw.clone())
                    .collect()
            })
            .unwrap_or_default();
        let rx = lsp.code_action(&path, range, diagnostics);
        self.intel.actions = Some(ActionsState {
            path,
            revision,
            rx: Some(rx),
            items: Vec::new(),
            selected: 0,
        });
    }

    fn poll_actions(&mut self) {
        if let Some(rx) = &self.intel.resolving_action {
            if let Ok(action) = rx.try_recv() {
                self.intel.resolving_action = None;
                self.run_code_action(action, false);
            }
        }
        let Some(state) = self.intel.actions.as_mut() else {
            return;
        };
        let Some(rx) = &state.rx else { return };
        match rx.try_recv() {
            Ok(value) => {
                state.rx = None;
                let mut items: Vec<Value> = value
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|a| a["disabled"].is_null())
                    .collect();
                // Preferred quick fixes first, as in the IDE's bulb menu.
                items.sort_by_key(|a| {
                    (
                        !a["isPreferred"].as_bool().unwrap_or(false),
                        a["kind"].as_str() != Some("quickfix"),
                    )
                });
                if items.is_empty() {
                    self.intel.actions = None;
                    self.ws.message = "No context actions available here".to_string();
                } else {
                    state.items = items;
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => self.intel.actions = None,
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }

    /// Applies a code action: its edit, then its command.
    fn run_code_action(&mut self, action: Value, may_resolve: bool) {
        let title = action["title"]
            .as_str()
            .unwrap_or("Context action")
            .to_string();
        // A bare Command.
        if action["command"].is_string() {
            if let Some(lsp) = self.running_lsp() {
                let _ = lsp.execute_command(&action);
                self.ws.message = title;
            }
            return;
        }
        let has_edit = action.get("edit").is_some_and(|e| !e.is_null());
        let has_command = action["command"].is_object();
        if !has_edit && !has_command && may_resolve {
            if let Some(lsp) = self.running_lsp() {
                self.intel.resolving_action = Some(lsp.code_action_resolve(&action));
            }
            return;
        }
        if has_edit {
            if let Some((files, edits)) = self.apply_workspace_edit(&action["edit"], &title) {
                self.ws.message = format!(
                    "{title} ({edits} edit{} in {files} file{})",
                    if edits == 1 { "" } else { "s" },
                    if files == 1 { "" } else { "s" }
                );
            }
        }
        if has_command {
            if let Some(lsp) = self.running_lsp() {
                let _ = lsp.execute_command(&action["command"]);
            }
        }
        self.editors.request_focus();
    }

    pub(crate) fn context_actions_popup(&mut self, ctx: &egui::Context) {
        let Some(state) = self.intel.actions.as_mut() else {
            return;
        };
        let stale = self
            .ws
            .active_buffer()
            .map(|b| {
                (b.path.as_ref() != Some(&state.path)) || b.document.revision() != state.revision
            })
            .unwrap_or(true);
        if stale {
            self.intel.actions = None;
            return;
        }
        if state.items.is_empty() {
            return;
        }
        let (mut down, mut up, mut enter, mut escape) = (false, false, false, false);
        ctx.input_mut(|i| {
            down = i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown);
            up = i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp);
            enter = i.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
            escape = i.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
        });
        let len = state.items.len();
        if down {
            state.selected = (state.selected + 1) % len;
        }
        if up {
            state.selected = (state.selected + len - 1) % len;
        }
        if escape {
            self.intel.actions = None;
            return;
        }
        let mut chosen = enter.then_some(state.selected);
        let anchor = self
            .editors
            .caret_rect
            .map(|r| r.left_bottom() + egui::vec2(-8.0, 4.0))
            .unwrap_or(egui::pos2(200.0, 200.0));
        let p = crate::design::theme::current(ctx).palette;
        let selected = state.selected;
        let items = state.items.clone();
        egui::Area::new(egui::Id::new("context-actions"))
            .order(egui::Order::Foreground)
            .fixed_pos(anchor)
            .constrain(true)
            .show(ctx, |ui| {
                Dialog::frame(ui)
                    .inner_margin(egui::Margin::same(4))
                    .show(ui, |ui| {
                        ui.set_width(380.0);
                        for (index, action) in items.iter().enumerate() {
                            let quickfix = action["kind"]
                                .as_str()
                                .unwrap_or("")
                                .starts_with("quickfix");
                            let (icon, color) = if quickfix {
                                (phpide_icons::IconId::Hint, p.warning)
                            } else {
                                (phpide_icons::IconId::Edit, p.text_secondary)
                            };
                            let row = ListRow::new(action["title"].as_str().unwrap_or("Action"))
                                .icon(icon, color)
                                .selected(index == selected)
                                .show(ui);
                            if row.clicked() {
                                chosen = Some(index);
                            }
                        }
                    });
            });
        if let Some(index) = chosen {
            let action = items[index].clone();
            self.intel.actions = None;
            self.run_code_action(action, true);
        }
    }

    // -------------------------------------------------------------- formatting

    /// Ctrl+Alt+L.
    pub(crate) fn reformat_code(&mut self) {
        self.sync_documents();
        let Some((path, revision, _, _, _)) = self.caret_position() else {
            return;
        };
        let Some(lsp) = self.running_lsp() else {
            self.ws.message = "Reformat Code needs a language server".to_string();
            return;
        };
        if !lsp.capabilities().formatting_provider {
            self.ws.message = format!("{} cannot format files", self.lsp_name);
            return;
        }
        let (tab_size, spaces) = match self.settings.indent.as_str() {
            "Tab" => (4, false),
            other => (
                other
                    .split_whitespace()
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(4),
                true,
            ),
        };
        let rx = lsp.formatting(&path, tab_size, spaces);
        self.intel.formatting = Some((path, revision, rx));
        self.ws.message = "Reformatting…".to_string();
    }

    fn poll_formatting(&mut self) {
        let Some((path, revision, rx)) = &self.intel.formatting else {
            return;
        };
        match rx.try_recv() {
            Ok(value) => {
                let (path, revision) = (path.clone(), *revision);
                self.intel.formatting = None;
                let current = self
                    .ws
                    .buffers
                    .iter()
                    .find(|b| b.path.as_ref() == Some(&path))
                    .map(|b| b.document.revision());
                if current != Some(revision) {
                    self.ws.message =
                        "The file changed while it was being formatted; try again".into();
                    return;
                }
                let edits = value.as_array().cloned().unwrap_or_default();
                if edits.is_empty() {
                    self.ws.message = "The code is already formatted".to_string();
                    return;
                }
                let count = edits.len();
                let edit = serde_json::json!({
                    "documentChanges": [{
                        "textDocument": { "uri": phpide_lsp::path_to_uri(&path), "version": revision },
                        "edits": edits
                    }]
                });
                if self.apply_workspace_edit(&edit, "Reformat Code").is_some() {
                    self.ws.message = format!(
                        "Reformatted {}: {count} change{}",
                        path.file_name()
                            .map(|n| n.to_string_lossy())
                            .unwrap_or_default(),
                        if count == 1 { "" } else { "s" }
                    );
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.intel.formatting = None;
                self.ws.message = "The language server did not format the file".to_string();
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }

    // ---------------------------------------------------------- file structure

    /// Ctrl+F12.
    pub(crate) fn file_structure(&mut self) {
        if self.ws.outline.is_empty() {
            self.ws.message = "No structure for this file yet".to_string();
            return;
        }
        self.intel.structure = Some(StructureState {
            query: String::new(),
            selected: 0,
        });
    }

    pub(crate) fn file_structure_popup(&mut self, ctx: &egui::Context) {
        let Some(state) = self.intel.structure.as_mut() else {
            return;
        };
        let query = state.query.to_lowercase();
        let entries: Vec<phpide_core::OutlineEntry> = self
            .ws
            .outline
            .iter()
            .filter(|e| query.is_empty() || phpide_lsp::match_score(&e.name, &query).is_some())
            .cloned()
            .collect();
        let (mut down, mut up, mut enter, mut escape) = (false, false, false, false);
        ctx.input_mut(|i| {
            down = i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown);
            up = i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp);
            enter = i.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
            escape = i.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
        });
        if escape {
            self.intel.structure = None;
            self.editors.request_focus();
            return;
        }
        if !entries.is_empty() {
            if down {
                state.selected = (state.selected + 1).min(entries.len() - 1);
            }
            if up {
                state.selected = state.selected.saturating_sub(1);
            }
            state.selected = state.selected.min(entries.len() - 1);
        }
        let mut chosen = enter
            .then_some(state.selected)
            .filter(|_| !entries.is_empty());
        let p = crate::design::theme::current(ctx).palette;
        let name = self
            .ws
            .active_buffer()
            .map(|b| b.name())
            .unwrap_or_default();
        egui::Area::new(egui::Id::new("file-structure"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 90.0))
            .show(ctx, |ui| {
                Dialog::frame(ui)
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_width(460.0);
                        ui.label(egui::RichText::new(format!("Structure of {name}")).strong());
                        ui.add_space(4.0);
                        let response = TextInput::search(&mut state.query)
                            .hint("Type to filter")
                            .id_salt("file-structure-filter")
                            .width(444.0)
                            .show(ui);
                        response.request_focus();
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical()
                            .max_height(360.0)
                            .show(ui, |ui| {
                                for (index, entry) in entries.iter().enumerate() {
                                    let row = ui
                                        .horizontal(|ui| {
                                            ui.add_space(14.0 * entry.depth as f32);
                                            ListRow::new(&entry.name)
                                                .icon(symbol_icon(&entry.kind), p.accent)
                                                .detail(entry.kind.clone())
                                                .trailing(format!(":{}", entry.line0 + 1))
                                                .selected(index == state.selected)
                                                .show(ui)
                                        })
                                        .inner;
                                    if row.clicked() {
                                        chosen = Some(index);
                                    }
                                }
                            });
                    });
            });
        if let Some(index) = chosen {
            let entry = entries[index].clone();
            self.intel.structure = None;
            if let Some(path) = self.ws.active_buffer().and_then(|b| b.path.clone()) {
                self.ws.open_file_at(&path, entry.line0, 0);
            }
            self.editors.request_focus();
        }
    }

    // --------------------------------------------------------------- server log

    pub(crate) fn language_server_log(&mut self, ctx: &egui::Context) {
        if !self.intel.log_open {
            return;
        }
        let lines = self.lsp.as_ref().map(|l| l.log()).unwrap_or_default();
        let status = self.lsp_label();
        let mut close = false;
        let p = crate::design::theme::current(ctx).palette;
        egui::Modal::new(egui::Id::new("language-server-log"))
            .backdrop_color(p.shadow)
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                Dialog::frame(ui)
                    .inner_margin(egui::Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(720.0);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Language Server Log").strong());
                            ui.weak(&status);
                        });
                        ui.separator();
                        egui::ScrollArea::vertical()
                            .max_height(420.0)
                            .stick_to_bottom(true)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                if lines.is_empty() {
                                    ui.weak("The server has not written anything yet.");
                                }
                                for line in &lines {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(line).monospace().small(),
                                        )
                                        .wrap(),
                                    );
                                }
                            });
                        ui.separator();
                        ui.horizontal(|ui| {
                            if crate::design::components::Button::new("Copy")
                                .show(ui)
                                .clicked()
                            {
                                ui.ctx().copy_text(lines.join("\n"));
                            }
                            if crate::design::components::Button::new("Restart Server")
                                .show(ui)
                                .clicked()
                            {
                                close = true;
                                self.restart_lsp();
                            }
                            if crate::design::components::Button::primary("Close")
                                .show(ui)
                                .clicked()
                                || ui.input(|i| i.key_pressed(egui::Key::Escape))
                            {
                                close = true;
                            }
                        });
                    })
            });
        if close {
            self.intel.log_open = false;
        }
    }
}

/// Parameter label ranges in a signature label (char offsets): from
/// `[start, end]` UTF-16 offsets or by finding the label string.
fn parameter_ranges(label: &str, parameters: Option<&Vec<Value>>) -> Vec<(usize, usize)> {
    let Some(parameters) = parameters else {
        return Vec::new();
    };
    let utf16_to_char = |units: usize| -> usize {
        let mut count = 0;
        for (index, c) in label.chars().enumerate() {
            if count >= units {
                return index;
            }
            count += c.len_utf16();
        }
        label.chars().count()
    };
    let mut from = 0;
    parameters
        .iter()
        .map(|param| match &param["label"] {
            Value::Array(range) => {
                let a = range.first().and_then(Value::as_u64).unwrap_or(0) as usize;
                let b = range.get(1).and_then(Value::as_u64).unwrap_or(0) as usize;
                (utf16_to_char(a), utf16_to_char(b))
            }
            Value::String(text) => {
                let chars: Vec<char> = label.chars().collect();
                let needle: Vec<char> = text.chars().collect();
                let found = (from..=chars.len().saturating_sub(needle.len()))
                    .find(|&i| chars[i..].starts_with(&needle));
                match found {
                    Some(i) => {
                        from = i + needle.len();
                        (i, i + needle.len())
                    }
                    None => (0, 0),
                }
            }
            _ => (0, 0),
        })
        .collect()
}

pub(crate) fn symbol_icon(kind: &str) -> phpide_icons::IconId {
    use phpide_icons::IconId;
    match kind {
        "class" | "interface" | "enum" | "struct" | "trait" => IconId::FileCode,
        "method" | "function" | "constructor" => IconId::Command,
        "property" | "field" | "constant" | "variable" => IconId::Structure,
        _ => IconId::Structure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_ranges_from_offsets_and_labels() {
        let label = "function greet(string $name, int $times): string";
        let offsets = vec![
            serde_json::json!({ "label": [15, 27] }),
            serde_json::json!({ "label": [29, 39] }),
        ];
        let ranges = parameter_ranges(label, Some(&offsets));
        let chars: Vec<char> = label.chars().collect();
        let text = |r: (usize, usize)| chars[r.0..r.1].iter().collect::<String>();
        assert_eq!(text(ranges[0]), "string $name");
        assert_eq!(text(ranges[1]), "int $times");
        let strings = vec![
            serde_json::json!({ "label": "string $name" }),
            serde_json::json!({ "label": "int $times" }),
        ];
        assert_eq!(parameter_ranges(label, Some(&strings)), ranges);
    }
}
