//! User actions: the command dispatcher, shortcuts, closing and saving
//! buffers, running and debugging.

use super::*;

impl PhpIdeApp {
    /// F2 / Shift+F2: salta para o próximo/anterior problema vigente
    /// (revisão atual; diagnóstico obsoleto não entra na ordem; wrap-around
    /// nas extremidades — T-PHP-002-03).
    pub fn goto_problem(&mut self, dir: i32) {
        let diags: Vec<phpide_core::Diagnostic> = self.ws.current_diagnostics().cloned().collect();
        if diags.is_empty() {
            self.ws.message = "no problems found".to_string();
            return;
        }
        let cur_line = self
            .ws
            .active_buffer()
            .map(|b| {
                b.document
                    .line_col(self.cursor_char.min(b.document.len_chars()))
                    .0
            })
            .unwrap_or(0);
        let Some(idx) = intelligence::next_problem_index(diags.len(), cur_line, dir) else {
            return;
        };
        let d = diags[idx].clone();
        self.ws
            .open_file_at(&d.path, d.line.saturating_sub(1), d.col.saturating_sub(1));
    }

    /// Stop global (Ctrl+F2): encerra execução e debug ativos (RUN-001,
    /// RUN-DBG-001-04 — Stop confiável em processos e socket).
    pub fn stop_execution(&mut self) {
        if self.exec.running {
            self.exec.stop();
        }
        self.rd.stop();
    }

    /// Runs a command from the registry (menu, Find Action or shortcut).
    pub(crate) fn dispatch_command(&mut self, cmd: Command, ctx: &egui::Context) {
        match cmd {
            Command::Undo => {
                self.editors
                    .run(phpide_editor::ViewCommand::Undo, &mut self.ws);
            }
            Command::Redo => {
                self.editors
                    .run(phpide_editor::ViewCommand::Redo, &mut self.ws);
            }
            Command::Complete => self.trigger_completion(),
            Command::RenameSymbol => self.rename_symbol(),
            Command::ShowContextActions => self.show_context_actions(),
            Command::ReformatCode => self.reformat_code(),
            Command::QuickDocumentation => self.quick_documentation(),
            Command::ParameterInfo => self.start_signature_help(None, false),
            Command::FileStructure => self.file_structure(),
            Command::LanguageServerLog => self.intel.log_open = true,
            Command::ToggleToolWindow(tw) => {
                self.tw.toggle(tw);
                if tw == ToolWindow::Terminal {
                    if self.tw.is_visible(tw) {
                        self.terminal.request_focus();
                    } else {
                        self.editors.request_focus();
                    }
                }
            }
            Command::HideActiveToolWindow => {
                self.tw.hide_active();
            }
            Command::SearchEverywhere => self.open_palette(PaletteMode::All),
            Command::GotoFile => self.open_palette(PaletteMode::Files),
            Command::GotoClass => self.open_palette(PaletteMode::Classes),
            Command::RecentFiles => self.open_palette(PaletteMode::Recent),
            Command::RecentLocations => self.open_recent_locations(),
            Command::FindAction => self.open_palette(PaletteMode::Actions),
            Command::BlockStart => self.jump_block(true),
            Command::BlockEnd => self.jump_block(false),
            Command::ToggleBookmark => self.toggle_bookmark(),
            Command::Stop => self.stop_execution(),
            Command::ToggleBreakpoint => {
                // While debugging, F9 resumes; otherwise it toggles a
                // breakpoint on the caret line.
                if self.rd.status.is_active() {
                    if self.rd.status == run_debug::DebugStatus::Paused {
                        self.rd.continue_debug(ContinueCommand::Resume);
                    }
                } else {
                    self.toggle_breakpoint();
                }
            }
            Command::Save => self.save_active(),
            Command::NewFile => {
                self.ws.new_file();
                // New files use the separator chosen in the settings.
                let ending = phpide_core::LineEnding::from_label(&self.settings.line_ending);
                if let Some(buffer) = self.ws.buffers.last_mut() {
                    buffer.line_ending = ending;
                }
            }
            Command::CloseTab => {
                // A diff tab in front closes first.
                if let Some(index) = self.diffs.active {
                    self.diffs.close(index);
                } else {
                    self.request_close_active();
                }
            }
            Command::CloseOthers => {
                let active = self.ws.active;
                self.request_close_others(active);
            }
            Command::PinTab => {
                if let Some(buf) = self.ws.active_buffer_mut() {
                    buf.pinned = !buf.pinned;
                }
            }
            Command::RefreshTree => self.files.refresh(&self.jobs, ctx),
            Command::ToggleTheme => {
                let next = if self.theme.is_dark() {
                    ThemeId::Light
                } else {
                    ThemeId::Dark
                };
                self.apply_theme(next, ctx);
            }
            Command::ZoomIn => self.set_zoom(self.zoom * 1.1),
            Command::ZoomOut => self.set_zoom(self.zoom / 1.1),
            Command::ZoomReset => self.set_zoom(1.0),
            Command::HideAllToolWindows => self.tw.toggle_hide_all(),
            Command::LastToolWindow => {
                if self.tw.show_last() == ToolWindow::Project {
                    ctx.memory_mut(|m| m.request_focus(crate::project_tree::tree_id()));
                }
            }
            Command::OpenProject => self.open_project_dialog(ctx),
            Command::CopyPath | Command::CopyReference | Command::CopyRepositoryUrl => {
                let Some(buffer) = self.ws.active_buffer() else {
                    return;
                };
                let Some(path) = buffer.path.clone() else {
                    return;
                };
                let line = buffer
                    .document
                    .line_col(self.cursor_char.min(buffer.document.len_chars()))
                    .0;
                let text = match cmd {
                    Command::CopyPath => Some(path.display().to_string()),
                    Command::CopyReference => {
                        let relative = path.strip_prefix(&self.ws.root).unwrap_or(&path);
                        Some(format!("{}:{line}", relative.display()))
                    }
                    _ => self
                        .git
                        .remote()
                        .and_then(|r| r.file_url(&path, Some(line))),
                };
                match text {
                    Some(text) => {
                        self.ws.message = format!("Copied {text}");
                        ctx.copy_text(text);
                    }
                    None => self.ws.message = "No remote repository (origin) for this file".into(),
                }
            }
            Command::NextProjectWindow | Command::PreviousProjectWindow => {
                let forward = cmd == Command::NextProjectWindow;
                match self.windows.neighbour(forward) {
                    Some(window) => self
                        .windows
                        .send(window.pid, services::windows::WindowCommand::Focus),
                    None => self.ws.message = "No other project window".into(),
                }
            }
            Command::CloseOtherProjects => {
                let others: Vec<u32> = self.windows.others().iter().map(|w| w.pid).collect();
                for pid in &others {
                    self.windows
                        .send(*pid, services::windows::WindowCommand::Close);
                }
                self.ws.message = format!("Closing {} other project window(s)", others.len());
            }
            Command::CloseProject => self.request_close_project(ctx),
            Command::CloseAllTabs => self.request_close_all(false),
            Command::ReopenClosedTab => {
                if !self.ws.reopen_closed_tab() {
                    self.ws.message = "No closed tab to reopen".into();
                }
            }
            Command::NextTab | Command::PreviousTab => {
                let count = self.ws.buffers.len();
                if count > 1 {
                    self.ws.active = if cmd == Command::NextTab {
                        (self.ws.active + 1) % count
                    } else {
                        (self.ws.active + count - 1) % count
                    };
                }
            }
            Command::LastEditLocation => {
                match self.ws.edit_locations.pop() {
                    Some((path, line0, col0)) => {
                        let here = self.ws.active_buffer().and_then(|b| b.path.clone());
                        // Already there: go one further back.
                        let target = if here.as_deref() == Some(path.as_path())
                            && self.ws.active_buffer().is_some_and(|b| {
                                b.document.line_col(self.cursor_char).0 == line0 + 1
                            }) {
                            self.ws.edit_locations.pop().unwrap_or((path, line0, col0))
                        } else {
                            (path, line0, col0)
                        };
                        self.ws.open_file_at(&target.0, target.1, target.2);
                        self.editors.request_focus();
                    }
                    None => self.ws.message = "No edit location".into(),
                }
            }
            Command::SelectIn => {
                if self
                    .ws
                    .active_buffer()
                    .and_then(|b| b.path.as_ref())
                    .is_some()
                {
                    self.select_in = Some(0);
                }
            }
            Command::NavigateBack => {
                if !self.ws.navigate_back() {
                    self.ws.message = "no navigation history".to_string();
                }
            }
            Command::NavigateForward => {
                if !self.ws.navigate_forward() {
                    self.ws.message = "no navigation history".to_string();
                }
            }
            Command::NextProblem => self.goto_problem(1),
            Command::PrevProblem => self.goto_problem(-1),
            Command::OpenWelcome => self.welcome_open = true,
            Command::OpenScheme => self.scheme_open = true,
            Command::OpenSettings => {
                self.settings_dialog
                    .open(settings_dialog::SettingsPage::Appearance);
            }
            Command::GotoDefinition => self.request_definition(),
            Command::GotoImplementation => self.request_implementation(),
            Command::GotoTypeDeclaration => self.request_type_definition(),
            Command::GotoSuperMethod => self.request_super_method(),
            Command::TypeHierarchy => self.request_hierarchy(hierarchy::HierarchyKind::Type),
            Command::CallHierarchy => self.request_hierarchy(hierarchy::HierarchyKind::Call),
            Command::ExtendSelection => self.select_range(true),
            Command::ShrinkSelection => self.select_range(false),
            Command::GotoSymbol => {
                self.symbol_results.clear();
                self.last_symbol_query = String::new();
                self.open_palette(PaletteMode::Symbols);
            }
            Command::FindUsages => self.request_usages(),
            Command::FindInFiles => self.find_in_files(None, false),
            Command::ReplaceInFiles => self.find_in_files(None, true),
            Command::SplitRight => self.editors.split(&self.ws, editor_area::SplitAxis::Right),
            Command::SplitDown => self.editors.split(&self.ws, editor_area::SplitAxis::Down),
            Command::Unsplit => {
                self.editors.unsplit();
            }
            Command::Editor(command) => {
                if !self.editors.run(command, &mut self.ws) {
                    if let Some(buf) = self.ws.active_buffer() {
                        if buf.readonly && command.edits() {
                            self.ws.message =
                                "the file is read-only (unlock it in the status bar)".to_string();
                        }
                    }
                }
            }
            Command::ToggleMinimap => {
                self.settings.minimap = !self.settings.minimap;
                self.persist_settings();
            }
            Command::ToggleBlame => {
                self.settings.blame_gutter = !self.settings.blame_gutter;
                self.ws.message = format!(
                    "blame gutter: {}",
                    if self.settings.blame_gutter {
                        "on"
                    } else {
                        "off"
                    }
                );
            }
            Command::UndoRefactor => {
                let mut store = WsStore(&mut self.ws);
                match workspace_edit::undo_last(&mut store, &mut self.refactor_undo) {
                    Ok(Some(label)) => self.ws.message = format!("undone: {label}"),
                    Ok(None) => {
                        self.ws.message = match self.refactor_undo.top_label() {
                            Some(next) => format!("no refactoring undone (next: {next})"),
                            None => "no refactoring to undo".to_string(),
                        };
                    }
                    Err(error) => self.ws.message = error.to_string(),
                }
            }
            Command::ToggleFolding => {
                self.settings.folding = !self.settings.folding;
                if !self.settings.folding {
                    self.editors
                        .run(phpide_editor::ViewCommand::UnfoldAll, &mut self.ws);
                }
            }
            Command::RestartLsp => self.restart_lsp(),
            Command::RunCurrent => self.run_selected(false),
            Command::DebugCurrent => self.run_selected(true),
            Command::RunContext => self.run_context(false),
            Command::DebugContext => self.run_context(true),
            Command::RunWithCoverage => self.run_with_coverage(),
            Command::GitCommit => {
                self.commit_ui.focus_message = true;
                self.tw.set_visible(ToolWindow::Commit, true);
            }
            Command::GitPush => self.git_push(),
            Command::ShowLog => self.show_log(),
            Command::ShowLogFolder => self.show_log_folder(),
            Command::About => self.recovery.about_open = true,
            Command::GitUpdate => self.git_update(),
            Command::GitFetch => self.git_fetch(),
            Command::GitBranches => egui::Popup::open_id(ctx, crate::header::branch_popup()),
            Command::GitNewBranch => {
                self.git_dialog = Some(crate::app_vcs::GitDialog::new(
                    crate::app_vcs::GitDialogKind::NewBranch { start: None },
                ))
            }
            Command::GitCompareWithHead => self.compare_active_with_head(ctx),
            Command::GitShowHistory => self.show_file_history(),
            Command::GitRollback => {
                let selected: Vec<_> = self.commit_ui.selected.iter().cloned().collect();
                if self.tw.is_visible(ToolWindow::Commit) && !selected.is_empty() {
                    let mut tracked = Vec::new();
                    let mut untracked = Vec::new();
                    for (group, path) in selected {
                        if group == crate::commit_panel::Group::Unversioned {
                            untracked.push(path);
                        } else if !tracked.contains(&path) {
                            tracked.push(path);
                        }
                    }
                    self.git_dialog = Some(crate::app_vcs::GitDialog::new(
                        crate::app_vcs::GitDialogKind::Rollback { tracked, untracked },
                    ));
                } else {
                    self.rollback_active();
                }
            }
            Command::EditConfigurations => self.open_edit_configurations(),
            Command::EvaluateExpression => {
                if self.rd.status == run_debug::DebugStatus::Paused {
                    self.rd.evaluate_open = true;
                } else {
                    self.ws.message = "Evaluate needs a paused debug session".to_string();
                }
            }
            Command::ViewBreakpoints => {
                // The breakpoint on the caret's line, when there is one.
                let at = self.ws.active_buffer().and_then(|b| {
                    let path = b.path.clone()?;
                    let line = b
                        .document
                        .line_col(self.cursor_char.min(b.document.len_chars()))
                        .0 as u32;
                    let store = self.rd.bp_store.lock().ok()?;
                    store.line_at(&path, line).map(|bp| bp.id)
                });
                self.rd.breakpoints_dialog = Some(at);
            }
            Command::RerunLast => {
                let last = self
                    .rd
                    .selected
                    .or_else(|| self.rd.manager.sessions().last().map(|s| s.id));
                if let Some(id) = last {
                    if let Some(new_id) = self.rd.manager.rerun(id) {
                        self.rd.selected = Some(new_id);
                        self.tw.set_visible(ToolWindow::Run, true);
                    }
                }
            }
            Command::GotoLine => self.goto_line_open = true,
        }
        let _ = ctx;
    }

    /// Fecha a aba ativa; com edição pendente, pede confirmação (NFR-007-02).
    pub fn request_close_active(&mut self) {
        let active = self.ws.active;
        self.request_close_tab(active);
    }

    /// Fecha a aba `i`; buffer sujo exige confirmação (salvar/descartar/cancelar).
    pub fn request_close_tab(&mut self, i: usize) {
        let dirty = self.ws.buffers.get(i).map(|b| b.dirty).unwrap_or(false);
        if dirty {
            self.pending_close = Some(PendingClose::Tab(i));
        } else {
            self.ws.close_tab(i);
        }
    }

    /// Close Others: confirma se alguma vítima estiver suja.
    pub fn request_close_others(&mut self, keep: usize) {
        let any_dirty = self
            .ws
            .buffers
            .iter()
            .enumerate()
            .any(|(i, b)| i != keep && b.dirty);
        if any_dirty {
            self.pending_close = Some(PendingClose::Others(keep));
        } else {
            self.ws.close_other_tabs(keep);
        }
    }

    /// Close All / Close Unpinned: confirms when a closing tab is modified.
    pub fn request_close_all(&mut self, keep_pinned: bool) {
        let any_dirty = self
            .ws
            .buffers
            .iter()
            .any(|b| b.dirty && !(keep_pinned && b.pinned));
        if any_dirty {
            self.pending_close = Some(PendingClose::All { keep_pinned });
        } else {
            self.ws.close_all_tabs(keep_pinned);
        }
    }

    /// Close Project: back to the Home screen, asking about unsaved files.
    pub fn request_close_project(&mut self, ctx: &egui::Context) {
        if self.ws.buffers.iter().any(|b| b.dirty) {
            self.pending_close = Some(PendingClose::Home);
        } else {
            self.close_project(ctx);
        }
    }

    pub(crate) fn save_active(&mut self) {
        let active = self.ws.active;
        self.save_buffer_at(active);
    }

    /// Salva um buffer específico: escrita atômica (NFR-007-01), erro com
    /// componente/operação/ação (NFR-005), didSave no LSP.
    pub(crate) fn save_buffer_at(&mut self, i: usize) -> bool {
        let Some(buf) = self.ws.buffers.get_mut(i) else {
            return false;
        };
        if buf.readonly {
            self.ws.message = "the file is read-only (unlock it in the status bar)".to_string();
            return false;
        }
        let path = buf.path.clone();
        // The file keeps its own line separator (Buffer::save normalizes).
        let corr = Correlation::session(self.session).with_document(
            path.clone().unwrap_or_else(|| PathBuf::from("<untitled>")),
            buf.document.revision(),
        );
        match buf.try_save() {
            Ok(()) => {
                self.ws.message = "Saved".to_string();
                self.logger
                    .info(Component::Core, &corr, "buffer_saved", &[]);
            }
            Err(e) => {
                // Buffer permanece sujo e intacto; mensagem curta + ação.
                self.ws.message = e.to_string();
                self.logger.error(
                    Component::Core,
                    &corr,
                    "buffer_save_failed",
                    &[("error", &e.log_line())],
                );
                return false;
            }
        }
        self.git_markers = None;
        if let (Some(lsp), Some(path)) = (&self.lsp, path) {
            if phpide_php::Php::is_php_file(&path) {
                let text = self
                    .ws
                    .buffers
                    .get(i)
                    .map(|b| b.document.text())
                    .unwrap_or_default();
                lsp.did_save(&path, &text);
            }
        }
        true
    }

    pub(crate) fn save_all_dirty(&mut self) -> close::SaveReport {
        let indices: Vec<usize> = (0..self.ws.buffers.len()).collect();
        self.save_dirty_indices(&indices)
    }

    pub(crate) fn save_dirty_indices(&mut self, indices: &[usize]) -> close::SaveReport {
        let descriptors: Vec<_> = indices
            .iter()
            .map(|index| {
                (
                    *index,
                    self.ws
                        .buffers
                        .get(*index)
                        .and_then(|buffer| buffer.path.clone()),
                    self.ws.buffers.get(*index).map(|b| b.dirty).unwrap_or(true),
                )
            })
            .collect();
        close::save_dirty_indices(&descriptors, |index| self.save_buffer_at(index))
    }

    pub(crate) fn any_dirty(&self) -> bool {
        self.ws.buffers.iter().any(|b| b.dirty)
    }
    /// Shift×2 abre o Search Everywhere (edge da tecla Shift, janela de 400ms).
    /// Search Everywhere on Shift tapped twice. A tap is Shift pressed and
    /// released alone: Shift+letters, Ctrl+Shift+F10 and the like do not count.
    pub(crate) fn detect_double_shift(&mut self, ctx: &egui::Context) {
        let (down, other_modifier, key_event) = ctx.input(|i| {
            (
                i.modifiers.shift,
                i.modifiers.ctrl || i.modifiers.alt || i.modifiers.command,
                i.events.iter().any(|e| {
                    matches!(
                        e,
                        egui::Event::Key { pressed: true, .. } | egui::Event::Text(_)
                    )
                }),
            )
        });
        if self.palette.open {
            self.shift_was_down = down;
            return;
        }
        if down && !self.shift_was_down {
            self.shift_alone = !other_modifier && !key_event;
        } else if down && (other_modifier || key_event) {
            self.shift_alone = false;
        }
        if !down && self.shift_was_down && self.shift_alone {
            let now = Instant::now();
            match self.last_shift_press {
                Some(last) if now.duration_since(last) < Duration::from_millis(400) => {
                    self.open_palette(PaletteMode::All);
                    self.last_shift_press = None;
                }
                _ => self.last_shift_press = Some(now),
            }
        }
        if !down {
            self.shift_alone = false;
        }
        self.shift_was_down = down;
    }

    pub(crate) fn ctrl_wheel_zoom(&mut self, ctx: &egui::Context) {
        if !self.zoom_on_ctrl_wheel {
            return;
        }
        // egui turns Ctrl+wheel into a zoom delta (1.0 = no change).
        let delta = ctx.input(|i| i.zoom_delta());
        if (delta - 1.0).abs() > f32::EPSILON {
            self.set_zoom(self.zoom * delta);
        }
    }

    /// Keyboard shortcuts from the command registry, plus the two gestures
    /// that are not plain shortcuts: double Shift (handled elsewhere) and the
    /// Ctrl+Tab switcher.
    pub(crate) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let pairs = commands::shortcuts_by_specificity();
        let mut fired = Vec::new();
        // Text commands belong to the focused widget: with the Project tree
        // or a text field focused, Ctrl+D or Ctrl+Z must not edit the code.
        let other_focus = !self.editors.editor_focused && ctx.memory(|m| m.focused().is_some());
        let tree_focused = ctx.memory(|m| m.focused() == Some(crate::project_tree::tree_id()));
        // Esc in a tool window returns to the editor (RF-UI-02); the tree
        // handles its own Esc (speed search first).
        let popup = self.palette.open
            || self.recent_locations.open
            || self.settings_dialog.open
            || self.completion.open
            || self.intel.rename.is_some()
            || self.intel.actions.is_some()
            || self.intel.structure.is_some()
            || self.intel.log_open
            || self.intel.hover.shown.is_some()
            || self.intel.signature.open
            || self.goto_line_open
            || self.select_in.is_some()
            || self.tree.dialog.is_some()
            || self.open_prompt.is_some()
            || self.pending_close.is_some()
            || self.welcome_open;
        if !popup
            && !self.terminal.focused
            && !tree_focused
            && !self.editors.editor_focused
            && !self.ws.buffers.is_empty()
            && ctx.input(|i| i.key_pressed(egui::Key::Escape) && i.modifiers.is_none())
        {
            if let Some(id) = ctx.memory(|m| m.focused()) {
                ctx.memory_mut(|m| m.surrender_focus(id));
            }
            self.editors.request_focus();
        }
        // In a Markdown editor Ctrl+B and Ctrl+I make bold and italic text
        // (Go to Declaration keeps Ctrl+click there, as in PhpStorm).
        if !popup && self.editors.editor_focused && !self.terminal.focused {
            let markdown = self
                .editors
                .pane_buffer(self.editors.focus, &self.ws)
                .is_some_and(|i| {
                    crate::markdown_view::is_markdown(&self.editors.detect(&self.ws.buffers[i]))
                });
            if markdown {
                let action = ctx.input_mut(|i| {
                    if i.consume_key(egui::Modifiers::COMMAND, egui::Key::B) {
                        Some(phpide_editor::MarkdownEdit::Bold)
                    } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::I) {
                        Some(phpide_editor::MarkdownEdit::Italic)
                    } else {
                        None
                    }
                });
                if let Some(action) = action {
                    self.editors
                        .run(phpide_editor::ViewCommand::Markdown(action), &mut self.ws);
                }
            }
        }
        // The terminal keeps every key but the tool window shortcuts.
        let terminal_focused = self.terminal.focused;
        ctx.input_mut(|i| {
            for (shortcut, command) in &pairs {
                if other_focus && command.is_text_command() {
                    continue;
                }
                if terminal_focused
                    && !matches!(
                        command,
                        Command::ToggleToolWindow(_)
                            | Command::HideActiveToolWindow
                            | Command::HideAllToolWindows
                            | Command::LastToolWindow
                            | Command::OpenSettings
                    )
                {
                    continue;
                }
                // Ctrl+NumPad +/− expand and collapse the focused tree.
                if tree_focused
                    && shortcut.modifiers.ctrl
                    && matches!(command, Command::ZoomIn | Command::ZoomOut)
                {
                    continue;
                }
                if i.consume_shortcut(shortcut) {
                    fired.push(*command);
                }
            }
            if i.consume_key(egui::Modifiers::CTRL, egui::Key::Tab) {
                // MRU switcher: the first Ctrl+Tab starts the cycle on the
                // most recent tab, more Tabs walk it, releasing Ctrl confirms.
                if !self.switcher.is_cycling() {
                    self.switcher.begin_cycle();
                } else {
                    self.switcher.cycle_next();
                }
            }
            if !i.modifiers.ctrl && self.switcher.is_cycling() {
                if let Some(key) = self.switcher.confirm() {
                    if let Some(idx) = self.ws.buffers.iter().position(|b| {
                        b.path
                            .as_ref()
                            .map(|p| p.to_string_lossy().into_owned())
                            .as_deref()
                            == Some(key.as_str())
                    }) {
                        self.ws.active = idx;
                    }
                }
            }
        });
        for command in fired {
            self.dispatch_command(command, ctx);
        }
    }

    /// Roda o arquivo atual como PHP Script (Shift+F10) — Run Configuration
    /// temporária sobre o RunManager (T-RUN-002-01): validação de campos,
    /// cwd, env e sessão com output/estado no painel Run.
    pub(crate) fn run_current_php(&mut self) {
        self.save_active();
        let Some(path) = self.ws.active_buffer().and_then(|b| b.path.clone()) else {
            self.ws.message = "save the file before running it".to_string();
            return;
        };
        // NFR-003-01: PHP por configuração (env/settings) antes do PATH;
        // ausência vira mensagem acionável (NFR-005), nunca panic.
        let php = match exec::resolve_php(&self.settings) {
            Ok(program) => program,
            Err(e) => {
                self.ws.message = e.to_string();
                return;
            }
        };
        let rel = path
            .strip_prefix(&self.ws.root)
            .unwrap_or(&path)
            .to_path_buf();
        let config = phpide_run::RunConfig::new(
            format!("temp-run-{}", path.display()),
            format!("php {}", path.file_name().unwrap().to_string_lossy()),
            phpide_run::RunKind::PhpScript {
                file: rel,
                args: Vec::new(),
                interpreter_options: Vec::new(),
                break_at_first_line: false,
            },
        );
        match phpide_run::build_command(
            &config,
            &self.ws.root,
            &exec::build_context(&self.settings, &php),
        ) {
            Ok(built) => {
                self.tw.set_visible(ToolWindow::Run, true);
                let id = self.rd.manager.start(config, built.spec, built.plan);
                self.rd.selected = Some(id);
            }
            Err(e) => self.ws.message = format!("{} — {}", e.message, e.action),
        }
    }

    /// Liga/desliga breakpoint na linha do cursor (F9).
    pub(crate) fn toggle_breakpoint(&mut self) {
        let Some(buf) = self.ws.active_buffer() else {
            return;
        };
        let Some(path) = buf.path.clone() else { return };
        if !phpide_php::Php::is_php_file(&path) {
            return;
        }
        let line = buf
            .document
            .line_col(self.cursor_char.min(buf.document.len_chars()))
            .0;
        self.toggle_breakpoint_at(line);
    }

    /// Alterna breakpoint na linha informada (1-based) — usado por F9 e pelo
    /// clique no gutter. O ponto entra no store compartilhado com a engine
    /// DBGp e é sincronizado com o backend (T-RUN-DBG-003-01).
    pub(crate) fn toggle_breakpoint_at(&mut self, line: usize) {
        let Some(path) = self.ws.active_buffer().and_then(|b| b.path.clone()) else {
            return;
        };
        let added = self.rd.toggle_breakpoint(path, line);
        let action = if added { "added" } else { "removed" };
        let total = self.rd.bp_store.lock().unwrap().breakpoints.len();
        self.ws.message = format!("breakpoint {action} (Ln {line}) — total {total}");
    }

    /// Adiciona/remove um bookmark na linha atual (F11 / Bookmarks tool window).
    pub fn toggle_bookmark(&mut self) {
        let Some(buf) = self.ws.active_buffer() else {
            return;
        };
        let Some(path) = buf.path.clone() else { return };
        let line = buf
            .document
            .line_col(self.cursor_char.min(buf.document.len_chars()))
            .0;
        let entries = self.bookmarks.entry(path.clone()).or_default();
        let action = if entries.remove(&line) {
            "removed"
        } else {
            entries.insert(line);
            "added"
        };
        if entries.is_empty() {
            self.bookmarks.remove(&path);
        }
        self.ws.message = format!("bookmark {action} (Ln {line})");
    }

    /// Debug do arquivo atual (Shift+F9): engine DBGp + PHP com XDEBUG_CONFIG
    /// zero-config (mescla o env do usuário, T-RUN-DBG-001-01) e Stop/
    /// Detach confiáveis (T-RUN-DBG-001-04 — processo via RunManager,
    /// socket via engine).
    pub(crate) fn debug_run(&mut self) {
        if self.rd.status.is_active() {
            self.ws.message = "a debug session is already active".to_string();
            return;
        }
        self.save_active();
        let Some(path) = self.ws.active_buffer().and_then(|b| b.path.clone()) else {
            self.ws.message = "save the file before debugging it".to_string();
            return;
        };
        let php = match exec::resolve_php(&self.settings) {
            Ok(program) => program,
            Err(e) => {
                self.ws.message = e.to_string();
                return;
            }
        };
        match self.rd.start_debug(&self.ws.root, &php, path, Vec::new()) {
            Ok(()) => {
                self.tw.set_visible(ToolWindow::Debug, true);
                self.ws.message = format!(
                    "Debug: waiting for Xdebug on 127.0.0.1:{} (IDE key phpide)",
                    run_debug::RunDebugState::debug_port()
                );
            }
            Err(e) => self.ws.message = e,
        }
    }

    /// Atalhos de debug com sessão ativa (roteamento contextual — ver README):
    /// F9 Resume, F7/F8/Shift+F8 stepping, Alt+F10 Show Execution Point.
    pub(crate) fn debug_keys(&mut self, ctx: &egui::Context) {
        if !self.rd.status.is_active() {
            return;
        }
        let paused = self.rd.status == run_debug::DebugStatus::Paused;
        ctx.input_mut(|i| {
            if paused && i.consume_key(egui::Modifiers::NONE, egui::Key::F9) {
                self.rd.continue_debug(ContinueCommand::Resume);
            }
            if paused && i.consume_key(egui::Modifiers::NONE, egui::Key::F7) {
                self.rd.continue_debug(ContinueCommand::StepInto);
            }
            // Shift+F8 first: egui would also match it as plain F8.
            if paused && i.consume_key(egui::Modifiers::SHIFT, egui::Key::F8) {
                self.rd.continue_debug(ContinueCommand::StepOut);
            }
            if paused && i.consume_key(egui::Modifiers::NONE, egui::Key::F8) {
                self.rd.continue_debug(ContinueCommand::StepOver);
            }
            if paused && i.consume_key(egui::Modifiers::ALT, egui::Key::F10) {
                run_panels::show_execution_point(self);
            }
        });
    }
}

impl PhpIdeApp {
    /// Opens a `path:line` reported by a tool (output, stack traces):
    /// absolute, or relative to the project root.
    pub(crate) fn open_reported_location(&mut self, path: &str, line: usize) {
        let candidate = std::path::Path::new(path);
        let path = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            self.ws.root.join(candidate)
        };
        if path.is_file() {
            self.ws.open_file_at(&path, line.saturating_sub(1), 0);
            self.editors.request_focus();
        } else {
            self.ws.message = format!("{} does not exist", path.display());
        }
    }
}
