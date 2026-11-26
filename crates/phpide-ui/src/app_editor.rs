//! The editor surface: the text area and its decorations. (The structure
//! under the caret is in the navigation bar; the breadcrumb strip above the
//! editor was removed: it appeared and vanished with the caret and resized
//! the editor under it.)

use super::*;
use crate::editor_area::{Pane, SplitAxis};

impl PhpIdeApp {
    /// The declarations around the caret in a PHP file (namespace, class,
    /// method…), with the line each starts on: the end of the navigation
    /// bar.
    pub(crate) fn structure_trail(&self) -> Vec<(String, usize)> {
        let Some(buf) = self.ws.active_buffer() else {
            return Vec::new();
        };
        let Some(path) = buf.path.as_ref() else {
            return Vec::new();
        };
        if !phpide_php::Php::is_php_file(path) {
            return Vec::new();
        }
        let line0 = buf
            .document
            .line_col(self.cursor_char.min(buf.document.len_chars()))
            .0
            .saturating_sub(1);
        let Some((sp, cache)) = &self.syntax else {
            return Vec::new();
        };
        if sp != path {
            return Vec::new();
        }
        // Blocks of the last analyzed revision (they lag a few lines
        // behind while typing in large files, which is fine here).
        cache
            .raw()
            .iter()
            .filter(|b| {
                let end = if b.kind == "namespace_definition" {
                    usize::MAX
                } else {
                    b.end_line0
                };
                b.start_line0 <= line0 && end >= line0
            })
            .map(|b| {
                let label = if b.name.is_empty() {
                    b.kind.clone()
                } else {
                    format!("{} {}", short_kind(&b.kind), b.name)
                };
                (label, b.start_line0)
            })
            .collect()
    }

    /// Gutter, inline and scrollbar marks of buffer `index` for the editor.
    fn decorations(&self, index: usize) -> phpide_editor::decorations::Decorations<'static> {
        use phpide_editor::decorations::{
            BreakpointMark, CoverageMark, Decorations, Diagnostic, DiagnosticSeverity, GitMark,
            InlayHint,
        };
        let mut deco = Decorations::default();
        let buffer = &self.ws.buffers[index];
        let Some(path) = buffer.path.clone() else {
            return deco;
        };
        let doc = &buffer.document;
        let rope = doc.rope();
        let line_count = rope.len_lines();
        let is_active = index == self.ws.active;
        let muted = self.rd.bp_muted();
        for line1 in self.rd.bp_lines_for(&path) {
            let mark = match self.rd.bp_status_for(&path, line1) {
                _ if muted => BreakpointMark::Muted,
                Some(phpide_dbgp::BreakpointStatus::Verified) => BreakpointMark::Verified,
                Some(phpide_dbgp::BreakpointStatus::Disabled) => BreakpointMark::Disabled,
                Some(phpide_dbgp::BreakpointStatus::Invalid(_)) => BreakpointMark::Invalid,
                _ => BreakpointMark::Enabled,
            };
            deco.breakpoints.push((line1.saturating_sub(1), mark));
        }
        if let Some(lines) = self.bookmarks.get(&path) {
            deco.bookmarks = lines.iter().map(|l| l.saturating_sub(1)).collect();
        }
        if let Some((p, _, markers)) = &self.git_markers {
            if p == &path && !buffer.dirty {
                deco.git = markers
                    .iter()
                    .map(|(line1, added)| {
                        let mark = if *added {
                            GitMark::Added
                        } else {
                            GitMark::Modified
                        };
                        ((*line1 as usize).saturating_sub(1), mark)
                    })
                    .collect();
            }
        }
        if let Some(coverage) = self.rd.coverage.as_ref().and_then(|r| r.file(&path)) {
            deco.coverage = coverage
                .lines
                .iter()
                .map(|(line1, hits)| {
                    let mark = if *hits > 0 {
                        CoverageMark::Full
                    } else {
                        CoverageMark::None
                    };
                    ((*line1 as usize).saturating_sub(1), mark)
                })
                .collect();
        }
        for d in self.ws.diagnostics_for(&path) {
            let line0 = d.line.saturating_sub(1);
            if line0 >= line_count {
                continue;
            }
            let line_start = rope.line_to_char(line0);
            let line_end = phpide_editor::textutil::line_end(rope, line0);
            let start = (line_start + d.col.saturating_sub(1)).min(line_end);
            let end = phpide_editor::textutil::word_at(rope, start)
                .filter(|(s, _)| *s == start)
                .map(|(_, e)| e)
                .unwrap_or((start + 1).min(line_end.max(start + 1)));
            let severity = match d.severity {
                phpide_core::Severity::Error => DiagnosticSeverity::Error,
                phpide_core::Severity::Warning => DiagnosticSeverity::Warning,
                phpide_core::Severity::Information => DiagnosticSeverity::Info,
                phpide_core::Severity::Hint => DiagnosticSeverity::Hint,
            };
            deco.diagnostics.push(Diagnostic {
                start,
                end,
                severity,
                message: format!("{} ({})", d.message, d.source),
            });
        }
        if let Some((file, line)) = &self.rd.last_stop {
            if self.rd.status == run_debug::DebugStatus::Paused && file == &path {
                deco.execution_line = Some((*line as usize).saturating_sub(1));
            }
        }
        if !is_active {
            return deco;
        }
        deco.code_lenses = self.lens_decorations(&path);
        deco.links = self.link_decorations(&path, doc.revision());
        // End-of-line hints and Code Vision, for the active file.
        let mut line_hints: Vec<(usize, String)> = Vec::new();
        if self.settings.inlay_hints {
            let settings = hints::HintSettings {
                enabled: true,
                all_arguments: self.settings.hint_all_arguments,
                pass_by_reference: self.settings.hint_pass_by_reference,
                exclude: self.settings.hint_exclude.clone(),
            };
            for hint in settings.filter(self.hints.hints()) {
                line_hints.push((hint.line0, hint.label));
            }
        }
        for (line0, labels) in &self.code_vision.lenses {
            line_hints.push((*line0, labels.join(" · ")));
        }
        for (line0, label) in line_hints {
            if line0 < line_count {
                let at = phpide_editor::textutil::line_end(rope, line0);
                deco.inlay_hints.push(InlayHint {
                    position: at,
                    label: format!("  {label}"),
                });
            }
        }
        if let Some((p, revision, _, spans)) = &self.semantic {
            if p == &path && *revision == doc.revision() {
                let len = rope.len_bytes();
                deco.semantic = spans
                    .iter()
                    .filter(|(s, e, _)| s < e && *e <= len)
                    .filter_map(|(s, e, ty)| {
                        let role = phpide_php::SyntaxRole::from_semantic_token(ty)?;
                        Some((rope.byte_to_char(*s), rope.byte_to_char(*e), role))
                    })
                    .collect();
                deco.semantic.sort_by_key(|t| t.0);
            }
        }
        if let Some((p, revision, _, ranges)) = &self.occurrences {
            if p == &path && *revision == doc.revision() {
                deco.occurrences = ranges.clone();
                deco.occurrences.sort();
            }
        }
        deco
    }

    /// Requests blame and Git line markers for the active file when stale.
    fn refresh_vcs_marks(&mut self, ctx: &egui::Context) {
        let Some(buffer) = self.ws.active_buffer() else {
            return;
        };
        let Some(path) = buffer.path.clone() else {
            return;
        };
        let text = buffer.document.text();
        let saved_hash = blame_gutter::text_hash(&text);
        let dirty = buffer.dirty;
        let rev = buffer.document.revision();
        if self.settings.blame_gutter
            && !dirty
            && self.blame.is_stale(&path.to_string_lossy(), saved_hash)
        {
            self.git
                .request_blame(&self.jobs, ctx, &path, saved_hash, rev);
        }
        if self
            .git_markers
            .as_ref()
            .map(|(p, _, _)| p != &path)
            .unwrap_or(true)
        {
            self.git_markers = None;
        }
        let fresh = matches!(&self.git_markers, Some((p, _, _)) if p == &path)
            && lens::markers_are_current(
                self.git_markers
                    .as_ref()
                    .map(|(_, revision, _)| *revision)
                    .unwrap_or(u64::MAX),
                rev,
                dirty,
            );
        if !fresh && !dirty {
            self.git.request_markers(&self.jobs, ctx, &path, rev);
        }
    }

    fn editor_options(&self) -> phpide_editor::EditorOptions {
        phpide_editor::EditorOptions {
            line_numbers: true,
            folding: self.settings.folding,
            fold_markers_always: self.settings.fold_icons_always,
            sticky_lines: if self.settings.sticky_lines {
                self.settings.sticky_lines_max as usize
            } else {
                0
            },
            indent_guides: true,
            blink: !self.settings.reduced_motion,
            inline_diagnostics: self.settings.inline_diagnostics,
            // The documentation popup shows the diagnostics under the pointer.
            diagnostic_tooltips: false,
            minimap: self.settings.minimap_options(),
        }
    }

    /// One editor pane; returns whether it holds the keyboard focus.
    fn editor_pane(&mut self, ui: &mut egui::Ui, pane: Pane, style: &phpide_editor::EditorStyle) {
        let Some(index) = self.editors.pane_buffer(pane, &self.ws) else {
            return;
        };
        self.file_type_notice(ui, index);
        if crate::markdown_view::is_markdown(&self.editors.detect(&self.ws.buffers[index])) {
            self.markdown_pane(ui, pane, index, style);
        } else {
            self.editor_pane_inner(ui, pane, index, style);
        }
    }

    /// The text editor of buffer `index` in a pane.
    pub(crate) fn editor_pane_inner(
        &mut self,
        ui: &mut egui::Ui,
        pane: Pane,
        index: usize,
        style: &phpide_editor::EditorStyle,
    ) {
        // The user's color scheme may change a language's colors.
        let detected = self.editors.detect(&self.ws.buffers[index]);
        let is_markdown = crate::markdown_view::is_markdown(&detected);
        let language = language_id(&detected);
        let mut language_style = style.clone();
        language_style.syntax = crate::design::current(ui.ctx()).syntax_style_for(Some(&language));
        let style = &language_style;
        let deco_owned = self.decorations(index);
        let blame_on = self.settings.blame_gutter && index == self.ws.active;
        let blame_stale = self.ws.buffers[index].dirty;
        let blame_cache = &self.blame;
        let blame_fn = move |line0: usize| -> Option<phpide_editor::decorations::BlameLine> {
            let a = blame_cache.annotate(line0 + 1);
            if a.uncommitted {
                return Some(phpide_editor::decorations::BlameLine {
                    text: "Not committed".into(),
                    tooltip: "Not committed yet".into(),
                });
            }
            let author = a.author_initials?;
            Some(phpide_editor::decorations::BlameLine {
                text: format!("{author}  {}", a.date.clone().unwrap_or_default()),
                tooltip: format!(
                    "{author} {} {}",
                    a.date.unwrap_or_default(),
                    a.sha_short.unwrap_or_default()
                ),
            })
        };
        let deco = phpide_editor::decorations::Decorations {
            blame: blame_on.then_some(
                &blame_fn as &dyn Fn(usize) -> Option<phpide_editor::decorations::BlameLine>,
            ),
            blame_stale,
            ..deco_owned
        };
        let options = self.editor_options();
        let buffer_id = self.ws.buffers[index].id;
        // Popups own the keyboard while open; the request waits for them.
        let popup_open = self.completion.open
            || self.intel.rename.is_some()
            || self.intel.structure.is_some()
            || self.intel.log_open
            || self.palette.open
            || self.goto_line_open
            || self.select_in.is_some()
            || self.settings_dialog.open
            || self.tree.dialog.is_some();
        if !popup_open && self.editors.take_focus_request(pane) {
            let id = phpide_editor::Editor::widget_id((buffer_id, pane == Pane::Secondary));
            ui.memory_mut(|m| m.request_focus(id));
        }
        let buffer = &mut self.ws.buffers[index];
        let (view, syntax) = self.editors.pane_parts(pane, buffer);
        view.config.markdown = is_markdown;
        let salt = (buffer_id, pane == Pane::Secondary);
        let output = phpide_editor::Editor::new(salt, buffer, view, syntax, style, &deco)
            .options(options)
            .show(ui);
        let caret = view.primary_caret();
        let path = buffer.path.clone();
        let has_selection = view.selection.ranges().iter().any(|r| !r.is_empty());
        let readonly = buffer.readonly;
        if output.focused {
            self.editors.focus = pane;
            self.editors.editor_focused = true;
        }
        if is_markdown {
            self.markdown
                .editor_line
                .insert(buffer_id, output.first_visible_line);
        }
        let focused_pane = self.editors.split.is_none() || self.editors.focus == pane;
        if focused_pane {
            self.cursor_char = caret;
            self.editors.caret_rect = output.caret_rect;
        }
        if output.blocked_edit {
            self.ws.message = "the file is read-only (unlock it in the status bar)".to_string();
        }
        if output.edited {
            if let Some(path) = path.clone() {
                let buffer = &self.ws.buffers[index];
                let (line, col) = buffer
                    .document
                    .line_col(caret.min(buffer.document.len_chars()));
                self.ws
                    .record_edit(&path, line.saturating_sub(1), col.saturating_sub(1));
            }
        }
        if let Some(phpide_editor::GutterClick::LineNumber(line0)) = output.gutter_click {
            if let Some(path) = path.clone() {
                let added = self.rd.toggle_breakpoint(path, line0 + 1);
                self.ws.message = format!(
                    "breakpoint {} (Ln {})",
                    if added { "added" } else { "removed" },
                    line0 + 1
                );
            }
        }
        if let Some(pos) = output.ctrl_click {
            self.cursor_char = pos;
            if !self.follow_link_at(pos, ui.ctx()) {
                self.dispatch_command(Command::GotoDefinition, ui.ctx());
            }
        }
        if let Some(index) = output.lens_click {
            self.click_lens(index);
        }
        self.editor_context_menu(ui, &output, pane, path.clone(), has_selection, readonly);
        if focused_pane && !output.typed.is_empty() {
            self.on_typed(&output.typed);
        }
        if focused_pane {
            let pointer = ui.ctx().pointer_hover_pos();
            self.note_hover(output.hover, pointer);
        }
        if let (Some(line0), true) = (output.scrollbar_hover_line, self.settings.lens_mode) {
            let pointer = ui
                .ctx()
                .input(|input| input.pointer.hover_pos())
                .unwrap_or_default();
            self.lens_hover = Some((line0, pointer, std::time::Instant::now()));
        }
    }

    /// Right click in this pane: the editor context menu, kept open by egui
    /// until an item is chosen or the user clicks elsewhere.
    fn editor_context_menu(
        &mut self,
        ui: &mut egui::Ui,
        output: &phpide_editor::EditorOutput,
        pane: Pane,
        path: Option<std::path::PathBuf>,
        selection: bool,
        readonly: bool,
    ) {
        let Some(response) = output.response.clone() else {
            return;
        };
        let key = response.id.with("context-target");
        if let Some(target) = output.context {
            ui.data_mut(|d| d.insert_temp(key, target));
            self.editors.focus = pane;
        }
        let Some(target) = ui.data(|d| d.get_temp::<phpide_editor::ContextTarget>(key)) else {
            return;
        };
        let php = path.as_deref().is_some_and(phpide_php::Php::is_php_file);
        let context = crate::editor_menu::Context {
            target,
            path: path.clone(),
            php,
            selection,
            readonly,
        };
        let mut chosen = None;
        // A right click while the menu is open would only close it in egui
        // (a click outside); like any desktop menu it opens again where the
        // new click landed, on the next frame.
        let reopen_key = key.with("reopen");
        // Only once the click is over: egui would take its release for a
        // click outside the menu it just opened and close it again.
        let clicking = ui.input(|i| i.pointer.any_down() || i.pointer.any_released());
        let reopen = !clicking
            && ui
                .data_mut(|d| d.remove_temp::<bool>(reopen_key))
                .unwrap_or(false);
        if clicking && ui.data(|d| d.get_temp::<bool>(reopen_key)).is_some() {
            ui.ctx().request_repaint();
        }
        let mut popup = egui::Popup::context_menu(&response);
        if reopen {
            popup = popup.open_memory(egui::SetOpenCommand::Bool(true));
        }
        popup.show(|ui| {
            if let Some(act) = crate::editor_menu::show(self, ui, &context) {
                chosen = Some(act);
                ui.close();
            }
        });
        if output.context.is_some() {
            // Every right click (re)opens the menu at the pointer on the
            // next frame, whether it was closed or open somewhere else.
            egui::Popup::close_id(ui.ctx(), egui::Popup::default_response_id(&response));
            ui.data_mut(|d| d.insert_temp(reopen_key, true));
            ui.ctx().request_repaint();
        }
        if let Some(act) = chosen {
            let ctx = ui.ctx().clone();
            self.run_editor_act(act, path.as_deref(), &ctx);
        }
    }

    pub(crate) fn editor(&mut self, ui: &mut egui::Ui) {
        let ctx = &ui.ctx().clone();
        self.editors.retain_open(&self.ws);
        self.editors.editor_focused = false;
        self.editors.set_file_types(
            &self.settings.file_type_patterns,
            &self.settings.file_type_overrides,
        );
        self.editors.note_active(&self.ws);
        self.refresh_vcs_marks(ctx);
        let theme = crate::design::current(ctx);
        let style = theme.editor_style();
        let editor_bg = theme.palette.bg_editor;
        if let Some(line) = self.scroll_hook.take() {
            if let Some(buffer) = self.ws.active_buffer_mut() {
                buffer.request_cursor(line, 0);
            }
        }
        // A diff tab shows in place of the editor.
        self.diffs
            .follow_editor(self.ws.active_buffer().map(|b| b.id));
        if self.diffs.active.is_some() {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(editor_bg))
                .show(ui, |ui| crate::diff_view::show(self, ui));
            return;
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(editor_bg))
            .show(ui, |ui| {
                if self.ws.active_buffer().is_none() {
                    let p = theme.palette;
                    ui.centered_and_justified(|ui| {
                        ui.label(egui::RichText::new("No file open").color(p.text_muted));
                    });
                    return;
                }
                if let Some(path) = self.ws.active_buffer().and_then(|b| b.path.clone()) {
                    if !self.switcher.is_cycling() {
                        self.switcher.touch(&path.to_string_lossy());
                    }
                }
                let area = ui.available_rect_before_wrap();
                let Some((axis, fraction)) =
                    self.editors.split.as_ref().map(|s| (s.axis, s.fraction))
                else {
                    ui.scope_builder(egui::UiBuilder::new().max_rect(area), |ui| {
                        self.editor_pane(ui, Pane::Primary, &style)
                    });
                    return;
                };
                let (first, handle, second) = match axis {
                    SplitAxis::Right => {
                        let x = area.left() + area.width() * fraction;
                        (
                            egui::Rect::from_min_max(area.min, egui::pos2(x - 3.0, area.bottom())),
                            egui::Rect::from_min_max(
                                egui::pos2(x - 3.0, area.top()),
                                egui::pos2(x + 3.0, area.bottom()),
                            ),
                            egui::Rect::from_min_max(egui::pos2(x + 3.0, area.top()), area.max),
                        )
                    }
                    SplitAxis::Down => {
                        let y = area.top() + area.height() * fraction;
                        (
                            egui::Rect::from_min_max(area.min, egui::pos2(area.right(), y - 3.0)),
                            egui::Rect::from_min_max(
                                egui::pos2(area.left(), y - 3.0),
                                egui::pos2(area.right(), y + 3.0),
                            ),
                            egui::Rect::from_min_max(egui::pos2(area.left(), y + 3.0), area.max),
                        )
                    }
                };
                ui.scope_builder(egui::UiBuilder::new().max_rect(first), |ui| {
                    self.editor_pane(ui, Pane::Primary, &style)
                });
                ui.scope_builder(egui::UiBuilder::new().max_rect(handle), |ui| {
                    use crate::design::components::{splitter, SplitAxis as Axis};
                    let (axis_kind, length) = match axis {
                        SplitAxis::Right => (Axis::Vertical, handle.height()),
                        SplitAxis::Down => (Axis::Horizontal, handle.width()),
                    };
                    let response = splitter(ui, axis_kind, length, None);
                    if response.dragged() {
                        let delta = response.drag_delta();
                        if let Some(split) = self.editors.split.as_mut() {
                            let (d, total) = match axis {
                                SplitAxis::Right => (delta.x, area.width()),
                                SplitAxis::Down => (delta.y, area.height()),
                            };
                            split.fraction =
                                (split.fraction + d / total.max(1.0)).clamp(0.15, 0.85);
                        }
                    }
                });
                ui.scope_builder(egui::UiBuilder::new().max_rect(second), |ui| {
                    let p = theme.palette;
                    let name = self
                        .editors
                        .pane_buffer(Pane::Secondary, &self.ws)
                        .map(|i| self.ws.buffers[i].name())
                        .unwrap_or_default();
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(name).color(p.text_secondary));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if crate::design::components::IconButton::new(
                                phpide_icons::IconId::Close,
                                "Close Split",
                            )
                            .show(ui)
                            .clicked()
                            {
                                self.editors.unsplit();
                            }
                        });
                    });
                    if self.editors.split.is_some() {
                        self.editor_pane(ui, Pane::Secondary, &style);
                    }
                });
            });
        self.lens_preview(ctx);
        // Popup do switcher MRU (UI-ED-003-02): desenha por cima de tudo.
        let mut labels = std::collections::HashMap::new();
        for b in &self.ws.buffers {
            if let Some(p) = &b.path {
                let mut title = b.name();
                if b.dirty {
                    title.push_str(" *");
                }
                labels.insert(p.to_string_lossy().into_owned(), title);
            }
        }
        switcher::popup(ctx, &self.switcher, &labels, 0);
    }

    /// Preview of the lines under the pointer on the scrollbar.
    fn lens_preview(&mut self, ctx: &egui::Context) {
        let Some((line0, pointer, at)) = self.lens_hover else {
            return;
        };
        if at.elapsed() > std::time::Duration::from_millis(400) {
            self.lens_hover = None;
            return;
        }
        let Some(buffer) = self.ws.active_buffer() else {
            return;
        };
        let rope = buffer.document.rope();
        let first = line0.saturating_sub(3);
        let last = (line0 + 3).min(rope.len_lines().saturating_sub(1));
        let lines: Vec<String> = (first..=last)
            .map(|l| phpide_editor::textutil::line_text(rope, l))
            .collect();
        let viewport = ctx.content_rect();
        egui::Area::new(egui::Id::new("lens-preview"))
            .order(egui::Order::Foreground)
            .fixed_pos(lens::popup_position(
                pointer,
                viewport,
                egui::vec2(360.0, 168.0),
            ))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    for l in &lines {
                        ui.label(egui::RichText::new(l).monospace().size(11.0));
                    }
                });
            });
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

/// `true` se o buffer ativo é `path` na revisão `revision` (semantic
/// tokens e ocorrências só valem para o estado em que foram calculados).
/// DocStore da aplicação: buffers abertos (sujeitos a undo de buffer) têm
/// prioridade; arquivos fechados são lidos/escritos em disco (escrita
/// atômica). Versões LSP locais não são rastreadas por arquivo aqui
/// (conflitos de versão são reportados pelo servidor quando aplicável).
pub(crate) struct WsStore<'a>(pub(crate) &'a mut phpide_core::Workspace);

impl workspace_edit::DocStore for WsStore<'_> {
    fn text(&self, path: &std::path::Path) -> Result<String, phpide_core::IdeError> {
        if let Some(buffer) = self
            .0
            .buffers
            .iter()
            .find(|buffer| buffer.path.as_deref() == Some(path))
        {
            return Ok(buffer.document.text());
        }
        std::fs::read_to_string(path).map_err(|error| {
            phpide_core::IdeError::from_io(
                Component::Lsp,
                "workspace_edit_read",
                &path.display().to_string(),
                &error,
            )
        })
    }

    fn set_text(
        &mut self,
        path: &std::path::Path,
        text: &str,
    ) -> Result<(), phpide_core::IdeError> {
        if let Some(buffer) = self
            .0
            .buffers
            .iter_mut()
            .find(|buffer| buffer.path.as_deref() == Some(path))
        {
            buffer.replace_all(text);
            return Ok(());
        }
        phpide_core::fsutil::write_atomic(path, text.as_bytes()).map_err(|error| {
            phpide_core::IdeError::from_io(
                Component::Lsp,
                "workspace_edit_write",
                &path.display().to_string(),
                &error,
            )
        })
    }

    fn lsp_version(&self, path: &std::path::Path) -> Option<i64> {
        self.0
            .buffers
            .iter()
            .find(|buffer| buffer.path.as_deref() == Some(path))
            .map(|buffer| buffer.document.revision() as i64)
    }
}

pub(crate) fn active_path_matches(ws: &Workspace, path: &Path, revision: u64) -> bool {
    ws.active_buffer()
        .map(|b| b.path.as_deref() == Some(path) && b.document.revision() == revision)
        .unwrap_or(false)
}

/// `*.ext` for a file with an extension (`None` for `Makefile`, `.env`).
pub(crate) fn extension_pattern(path: &std::path::Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy();
    let (stem, extension) = name.rsplit_once('.')?;
    (!stem.is_empty() && !extension.is_empty()).then(|| format!("*.{extension}"))
}

impl PhpIdeApp {
    /// Offers a file type for files no pattern recognizes (RF-HL-02).
    fn file_type_notice(&mut self, ui: &mut egui::Ui, index: usize) {
        let buffer = &self.ws.buffers[index];
        let Some(path) = buffer.path.clone() else {
            return;
        };
        if self.editors.detect(buffer) != phpide_php::filetype::Detected::Unknown {
            return;
        }
        let pattern = extension_pattern(&path);
        let message = match &pattern {
            Some(p) => format!("No file type is associated with {p} files."),
            None => "No file type is associated with this file.".to_string(),
        };
        let mut choice: Option<&'static str> = None;
        crate::design::components::editor_notice(
            ui,
            crate::design::components::Tone::Info,
            &message,
            |ui| {
                ui.menu_button("Associate with file type…", |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(320.0)
                        .show(ui, |ui| {
                            for file_type in phpide_php::filetype::FILE_TYPES {
                                if ui.button(file_type.name).clicked() {
                                    choice = Some(file_type.id);
                                    ui.close();
                                }
                            }
                        });
                });
                if ui.link("Open as plain text").clicked() {
                    choice = Some("text");
                }
            },
        );
        if let Some(id) = choice {
            match pattern {
                Some(pattern) => self.associate_file_type(&pattern, id),
                None => self.set_file_type_override(&path, Some(id)),
            }
        }
    }

    /// The type id forced on `path`, if any.
    pub(crate) fn file_type_override(&self, path: &std::path::Path) -> Option<&str> {
        let key = path.to_string_lossy();
        self.settings
            .file_type_overrides
            .iter()
            .find(|(p, _)| *p == key)
            .map(|(_, id)| id.as_str())
    }

    /// Forces a type on one file (`None` goes back to detection).
    pub(crate) fn set_file_type_override(&mut self, path: &std::path::Path, id: Option<&str>) {
        let key = path.to_string_lossy().into_owned();
        self.settings.file_type_overrides.retain(|(p, _)| *p != key);
        if let Some(id) = id {
            self.settings
                .file_type_overrides
                .push((key, id.to_string()));
        }
        self.persist_settings();
    }

    /// Associates a name pattern (`*.tpl`) with a type for every file.
    pub(crate) fn associate_file_type(&mut self, pattern: &str, id: &str) {
        self.settings
            .file_type_patterns
            .retain(|(p, _)| !p.eq_ignore_ascii_case(pattern));
        self.settings
            .file_type_patterns
            .push((pattern.to_string(), id.to_string()));
        self.persist_settings();
        self.ws.message = format!("{pattern} files now open as {id}");
    }
}

impl PhpIdeApp {
    /// Opens `path` in a tab: a preview tab (replaced by the next preview)
    /// or a permanent one, next to the active tab unless the settings say
    /// "at the end" (RF-TAB-01, RF-TAB-02).
    pub(crate) fn open_file_tab(&mut self, path: &std::path::Path, preview: bool) {
        self.ws.open_tabs_at_end = self.settings.tabs_open_at_end;
        self.ws.open_file_ex(path, preview);
        if !preview {
            if let Some(buffer) = self.ws.active_buffer_mut() {
                buffer.preview = false;
            }
        }
    }
}

/// Color-scheme section of a detected file type (its id, or the grammar name).
pub(crate) fn language_id(detected: &phpide_php::filetype::Detected) -> String {
    match detected {
        phpide_php::filetype::Detected::Known(file_type) => file_type.id.to_string(),
        phpide_php::filetype::Detected::Grammar(name) => name.to_ascii_lowercase(),
        phpide_php::filetype::Detected::Unknown => "text".to_string(),
    }
}
