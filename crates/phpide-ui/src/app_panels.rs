//! Tool window panels and modal dialogs drawn around the editor.

use super::*;

impl PhpIdeApp {
    pub(crate) fn file_tree(&mut self, ui: &mut egui::Ui) {
        project_tree::show(self, ui);
    }

    /// Opens Find in Files (or Replace in Files) on a folder, or on the
    /// whole project (RF-NAV-21).
    pub(crate) fn find_in_files(&mut self, folder: Option<PathBuf>, replace: bool) {
        self.search.replace_mode = replace;
        self.search.directory = folder.map(|f| f.display().to_string()).unwrap_or_default();
        self.search.focus_query = true;
        self.tw.set_visible(ToolWindow::Find, true);
    }

    /// Files Find in Files reads: the project's (or a folder's) searchable
    /// files that pass the mask.
    pub(crate) fn search_files(&self) -> Vec<PathBuf> {
        let folder = self.search.directory.trim();
        let folder = (!folder.is_empty()).then(|| PathBuf::from(folder));
        let mask = self.search.mask.clone();
        let keep = |path: &Path| {
            search::is_searchable(path)
                && path
                    .file_name()
                    .is_some_and(|n| search::mask_matches(&n.to_string_lossy(), &mask))
        };
        match folder {
            Some(dir) if !dir.starts_with(&self.ws.root) => {
                // A folder outside the project: walk it (bounded).
                let mut out = Vec::new();
                let mut stack = vec![dir];
                while let Some(dir) = stack.pop() {
                    let Ok(entries) = std::fs::read_dir(&dir) else {
                        continue;
                    };
                    for entry in entries.flatten() {
                        let path = entry.path();
                        let name = entry.file_name();
                        if entry.file_type().is_ok_and(|t| t.is_dir()) {
                            if name != ".git" && name != "node_modules" {
                                stack.push(path);
                            }
                        } else if keep(&path) {
                            out.push(path);
                        }
                        if out.len() >= 20_000 {
                            return out;
                        }
                    }
                }
                out.sort();
                out
            }
            folder => self
                .ws
                .files
                .iter()
                .filter(|e| !e.is_dir && keep(&e.path))
                .filter(|e| folder.as_ref().is_none_or(|dir| e.path.starts_with(dir)))
                .map(|e| e.path.clone())
                .collect(),
        }
    }

    fn start_search(&mut self) {
        let files = self.search_files();
        self.search.start(files, &self.jobs);
    }

    /// Replaces every match in the files of the results: open files get an
    /// undoable edit, the others are written (RF-NAV-21).
    pub(crate) fn replace_in_files(&mut self) {
        let mut files: Vec<PathBuf> = self.search.results.iter().map(|h| h.path.clone()).collect();
        files.dedup();
        let (needle, with, case) = (
            self.search.query.clone(),
            self.search.replacement.clone(),
            self.search.case_sensitive,
        );
        let mut count = 0;
        let mut changed = 0;
        let mut errors = Vec::new();
        for path in &files {
            if let Some(buffer) = self
                .ws
                .buffers
                .iter_mut()
                .find(|b| b.path.as_ref() == Some(path))
            {
                if buffer.readonly {
                    errors.push(format!("{} is read-only", path.display()));
                    continue;
                }
                let (text, n) =
                    search::replace_matches(&buffer.document.text(), &needle, &with, case);
                if n > 0 {
                    buffer.replace_all(&text);
                    count += n;
                    changed += 1;
                }
                continue;
            }
            let result = std::fs::read_to_string(path).and_then(|old| {
                let (text, n) = search::replace_matches(&old, &needle, &with, case);
                if n > 0 {
                    std::fs::write(path, text)?;
                }
                Ok(n)
            });
            match result {
                Ok(0) => {}
                Ok(n) => {
                    count += n;
                    changed += 1;
                }
                Err(e) => errors.push(format!("{}: {e}", path.display())),
            }
        }
        self.ws.message = format!(
            "Replaced {count} occurrence{} in {changed} file{}",
            if count == 1 { "" } else { "s" },
            if changed == 1 { "" } else { "s" },
        );
        if !errors.is_empty() {
            self.ws.message.push_str(&format!(
                " ({} failed: {})",
                errors.len(),
                errors.join("; ")
            ));
        }
        self.start_search();
    }

    pub(crate) fn search_panel(&mut self, ui: &mut egui::Ui) {
        let placement = self.tw.placement(ToolWindow::Find);
        crate::toolwindows::dock(ui, ToolWindow::Find, placement, 260.0, |ui| {
            tool_window_header(self, ui, ToolWindow::Find);
            let mut search = false;
            ui.horizontal(|ui| {
                let mode = if self.search.replace_mode {
                    "Replace in Files"
                } else {
                    "Find in Files"
                };
                ui.label(egui::RichText::new(mode).strong());
                ui.checkbox(&mut self.search.replace_mode, "Replace")
                    .on_hover_text("Ctrl+Shift+R");
            });
            ui.horizontal(|ui| {
                let label = ui.label("Find:");
                let edit_response = ui
                    .add(
                        egui::TextEdit::singleline(&mut self.search.query)
                            .id_salt("find-in-files-query")
                            .hint_text("text (Enter to search)")
                            .desired_width(280.0),
                    )
                    .labelled_by(label.id);
                if std::mem::take(&mut self.search.focus_query) {
                    edit_response.request_focus();
                }
                if edit_response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    search = true;
                }
                ui.checkbox(&mut self.search.case_sensitive, "Match case");
                if ui.button("Search").clicked() {
                    search = true;
                }
                if self.search.busy() {
                    ui.spinner();
                }
                ui.label(&self.search.status);
            });
            if self.search.replace_mode {
                ui.horizontal(|ui| {
                    let label = ui.label("Replace:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.search.replacement)
                            .id_salt("find-in-files-replacement")
                            .desired_width(280.0),
                    )
                    .labelled_by(label.id);
                    let ready = !self.search.results.is_empty() && !self.search.busy();
                    if ui
                        .add_enabled(ready, egui::Button::new("Replace All"))
                        .clicked()
                    {
                        self.search.confirm_replace = true;
                    }
                });
            }
            ui.horizontal(|ui| {
                let label = ui.label("In:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.search.directory)
                        .id_salt("find-in-files-directory")
                        .hint_text("Whole project")
                        .desired_width(280.0),
                )
                .labelled_by(label.id)
                .on_hover_text("Folder to search in; empty for the whole project");
                if !self.search.directory.is_empty() && ui.small_button("Project").clicked() {
                    self.search.directory.clear();
                }
                let label = ui.label("File mask:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.search.mask)
                        .id_salt("find-in-files-mask")
                        .hint_text("*.php, *.twig")
                        .desired_width(140.0),
                )
                .labelled_by(label.id);
            });
            if search {
                self.start_search();
            }
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("find-in-files-results")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let hits: Vec<(PathBuf, usize, String)> = self
                        .search
                        .results
                        .iter()
                        .map(|h| (h.path.clone(), h.line, h.preview.clone()))
                        .collect();
                    for (path, line, preview) in hits {
                        let rel = path
                            .strip_prefix(&self.ws.root)
                            .unwrap_or(&path)
                            .display()
                            .to_string();
                        let label = format!("{rel}:{line}:  {preview}");
                        if ui.selectable_label(false, label).clicked() {
                            self.ws.open_file_at(&path, line.saturating_sub(1), 0);
                        }
                    }
                });
        });
        self.replace_confirmation(ui.ctx());
    }

    fn replace_confirmation(&mut self, ctx: &egui::Context) {
        if !self.search.confirm_replace {
            return;
        }
        let mut files: Vec<&PathBuf> = self.search.results.iter().map(|h| &h.path).collect();
        files.dedup();
        let text = format!(
            "Replace \u{201c}{}\u{201d} with \u{201c}{}\u{201d} in {} file{}? Files open in the editor can be undone; the others are written now.",
            self.search.query,
            self.search.replacement,
            files.len(),
            if files.len() == 1 { "" } else { "s" },
        );
        let mut replace = false;
        let mut cancel = false;
        let shown = crate::design::components::Dialog::new("replace-in-files", "Replace in Files")
            .width(420.0)
            .show(
                ctx,
                |ui| {
                    ui.label(text);
                },
                |ui| {
                    if crate::design::components::Button::primary("Replace")
                        .show(ui)
                        .clicked()
                    {
                        replace = true;
                    }
                    if crate::design::components::Button::new("Cancel")
                        .show(ui)
                        .clicked()
                    {
                        cancel = true;
                    }
                },
            );
        if replace {
            self.search.confirm_replace = false;
            self.replace_in_files();
        } else if cancel || shown.dismissed {
            self.search.confirm_replace = false;
        }
    }

    pub(crate) fn problems_panel(&mut self, ui: &mut egui::Ui) {
        // Escopo (T-PHP-002-02): projeto inteiro ou só o arquivo ativo.
        let active_path = self.ws.active_buffer().and_then(|b| b.path.clone());
        let mut diags: Vec<phpide_core::Diagnostic> =
            self.ws.current_diagnostics().cloned().collect();
        if self.problems_scope == 1 {
            if let Some(path) = &active_path {
                diags.retain(|d| &d.path == path);
            }
        }
        // Resumo por ferramenta de origem (LSP/tree-sitter/...).
        let sources = {
            let refs: Vec<&phpide_core::Diagnostic> = diags.iter().collect();
            intelligence::problems_by_source(&refs)
        };
        let placement = self.tw.placement(ToolWindow::Problems);
        crate::toolwindows::dock(ui, ToolWindow::Problems, placement, 190.0, |ui| {
            tool_window_header(self, ui, ToolWindow::Problems);
            ui.horizontal(|ui| {
                ui.label(format!("{} problems", diags.len()));
                ui.separator();
                let scope_label = if self.problems_scope == 0 {
                    "Scope: Project"
                } else {
                    "Scope: File"
                };
                if ui
                    .button(scope_label)
                    .on_hover_text("Toggle project/file")
                    .clicked()
                {
                    self.problems_scope = 1 - self.problems_scope;
                }
                for (source, critical, total) in &sources {
                    ui.label(
                        egui::RichText::new(format!("{source}: {critical}/{total}"))
                            .small()
                            .weak(),
                    );
                }
            });
            ui.separator();
            if diags.is_empty() {
                ui.label("No problems.");
            } else {
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        for d in &diags {
                            let p = crate::design::current(ui.ctx()).palette;
                            let color = match d.severity {
                                phpide_core::Severity::Error => p.error,
                                phpide_core::Severity::Warning => p.warning,
                                _ => p.info,
                            };
                            let rel = d
                                .path
                                .strip_prefix(&self.ws.root)
                                .unwrap_or(&d.path)
                                .display()
                                .to_string();
                            let label = format!(
                                "[{}] {rel}:{}:{}  {}",
                                d.severity.marker(),
                                d.line,
                                d.col,
                                d.message.lines().next().unwrap_or("")
                            );
                            if ui
                                .selectable_label(false, egui::RichText::new(label).color(color))
                                .clicked()
                            {
                                self.ws.open_file_at(
                                    &d.path,
                                    d.line.saturating_sub(1),
                                    d.col.saturating_sub(1),
                                );
                            }
                        }
                    });
            }
        });
    }

    /// Painéis Run e Debug (Estágio 8): a janela Run mostra as sessões
    /// (process list) e a Debug mostra a toolbar/inspeção DBGp. As duas são
    /// independentes — podem estar abertas ao mesmo tempo.
    pub(crate) fn exec_panel(&mut self, ui: &mut egui::Ui) {
        if self.tw.is_visible(ToolWindow::Run) {
            run_panels::run_panel(self, ui);
        }
        if self.tw.is_visible(ToolWindow::Debug) {
            run_panels::debug_panel(self, ui);
        }
    }

    pub(crate) fn confirm_save_close(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_close.clone() else {
            return;
        };
        let indices = close::targets(&pending, self.ws.buffers.len());
        let report = if matches!(
            &pending,
            PendingClose::Project(_) | PendingClose::Exit | PendingClose::Home
        ) {
            self.save_all_dirty()
        } else {
            self.save_dirty_indices(&indices)
        };
        match close::after_successful_save(&mut self.ws, &pending, &report) {
            close::CloseAction::Closed => self.pending_close = None,
            close::CloseAction::Home => {
                self.pending_close = None;
                self.close_project(ctx);
            }
            close::CloseAction::LoadProject(path) => {
                self.pending_close = None;
                self.load_project(path, ctx);
            }
            close::CloseAction::Exit => {
                self.pending_close = None;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            close::CloseAction::Blocked => {
                self.ws.message = format!("{} ({})", report.summary(), self.ws.message);
            }
        }
    }

    /// Modal de confirmação para buffers sujos (NFR-007-02): aba, Close
    /// Others, troca de projeto e fechamento da IDE. Salvar grava tudo o que
    /// for necessário; Descartar fecha sem salvar; Cancelar volta ao fluxo.
    pub(crate) fn dirty_close_modal(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_close.clone() else {
            return;
        };
        let (title, body) = match &pending {
            PendingClose::Tab(i) => {
                let name = self
                    .ws
                    .buffers
                    .get(*i)
                    .map(|b| b.name())
                    .unwrap_or_else(|| "aba".to_string());
                (
                    "Close Tab".to_string(),
                    format!("\"{name}\" has unsaved changes."),
                )
            }
            PendingClose::Others(_) => (
                "Close Other Tabs".to_string(),
                "Other tabs have unsaved changes.".to_string(),
            ),
            PendingClose::All { .. } => (
                "Close All Tabs".to_string(),
                "Some tabs have unsaved changes.".to_string(),
            ),
            PendingClose::Home => (
                "Close Project".to_string(),
                "There are unsaved changes. Close the project anyway?".to_string(),
            ),
            PendingClose::Project(path) => (
                "Switch Project".to_string(),
                format!(
                    "There are unsaved changes.\nOpen \"{}\" anyway?",
                    path.display()
                ),
            ),
            PendingClose::Exit => (
                "Quit PHP IDE".to_string(),
                "There are unsaved changes. Quit anyway?".to_string(),
            ),
        };
        let mut action: Option<&'static str> = None;
        egui::Modal::new(egui::Id::new("dirty-close"))
            .area(
                egui::Modal::default_area(egui::Id::new("dirty-close"))
                    .fixed_pos(ctx.content_rect().center() - egui::vec2(220.0, 60.0)),
            )
            .frame(
                egui::Frame::window(&ctx.global_style())
                    .shadow(egui::epaint::Shadow::default())
                    .inner_margin(egui::Margin::same(12)),
            )
            .show(ctx, |ui| {
                ui.label(egui::RichText::new(title).strong());
                ui.add_space(4.0);
                ui.label(body);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let save_response = ui.button("Save");
                    let discard_response = ui.button("Don't Save");
                    let cancel_response = ui.button("Cancel");
                    #[cfg(test)]
                    DIRTY_BUTTON_RECTS.with(|rects| {
                        rects.set(Some([
                            save_response.rect,
                            discard_response.rect,
                            cancel_response.rect,
                        ]));
                    });
                    if save_response.clicked() {
                        action = Some("save");
                    }
                    if discard_response.clicked() {
                        action = Some("discard");
                    }
                    if cancel_response.clicked() {
                        action = Some("cancel");
                    }
                });
            });
        match action {
            Some("save") => self.confirm_save_close(ctx),
            Some("discard") => {
                match pending {
                    PendingClose::Tab(i) => {
                        self.ws.close_tab(i);
                    }
                    PendingClose::Others(keep) => {
                        self.ws.close_other_tabs(keep);
                    }
                    PendingClose::All { keep_pinned } => {
                        self.ws.close_all_tabs(keep_pinned);
                    }
                    PendingClose::Home => {
                        self.pending_close = None;
                        self.close_project(ctx);
                        return;
                    }
                    PendingClose::Project(path) => {
                        self.pending_close = None;
                        self.load_project(path, ctx);
                        return;
                    }
                    PendingClose::Exit => {
                        // Descartar: fecha marcando os buffers como limpos
                        // (a confirmação já cobriu a perda explícita).
                        for buf in &mut self.ws.buffers {
                            buf.dirty = false;
                        }
                        self.pending_close = None;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        return;
                    }
                }
                self.pending_close = None;
            }
            Some("cancel") => close::cancel(&mut self.pending_close),
            _ => {}
        }
    }

    pub(crate) fn goto_line_popup(&mut self, ctx: &egui::Context) {
        if !self.goto_line_open {
            return;
        }
        let mut close = false;
        egui::Window::new("goto-line")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .frame(
                egui::Frame::window(&ctx.global_style())
                    .shadow(egui::epaint::Shadow::default())
                    .inner_margin(egui::Margin::same(10)),
            )
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("Go to Line").strong().small());
                ui.add_space(4.0);
                let edit = egui::TextEdit::singleline(&mut self.goto_line_input)
                    .hint_text("line number")
                    .desired_width(180.0)
                    .show(ui);
                edit.response.request_focus();
                ui.ctx().input(|i| {
                    if i.key_pressed(egui::Key::Enter) {
                        if let Ok(line) = self.goto_line_input.trim().parse::<usize>() {
                            if let Some(buf) = self.ws.active_buffer_mut() {
                                let line0 = line
                                    .saturating_sub(1)
                                    .min(buf.document.line_count().saturating_sub(1));
                                buf.request_cursor(line0, 0);
                            }
                        }
                        close = true;
                    }
                    if i.key_pressed(egui::Key::Escape) {
                        close = true;
                    }
                });
            });
        if close {
            self.goto_line_open = false;
            self.goto_line_input.clear();
            self.editors.request_focus();
        }
    }

    // -----------------------------------------------------------------
    // Estágio 3: integração LSP (poll por frame, nunca bloqueia a UI)
    // -----------------------------------------------------------------

    pub(crate) fn outline_panel(&mut self, ui: &mut egui::Ui) {
        use crate::design::components::{EmptyState, ListRow};
        let placement = self.tw.placement(ToolWindow::Structure);
        crate::toolwindows::dock(ui, ToolWindow::Structure, placement, 210.0, |ui| {
            tool_window_header(self, ui, ToolWindow::Structure);
            if self.ws.outline.is_empty() {
                let php_open = self
                    .ws
                    .active_buffer()
                    .and_then(|b| b.path.as_deref())
                    .is_some_and(phpide_php::Php::is_php_file);
                let (title, description) = if php_open {
                    (
                        "No declarations",
                        "Classes, functions, methods, properties and constants of this file appear here.",
                    )
                } else {
                    (
                        "No structure",
                        "Open a PHP file to see its classes, functions and members.",
                    )
                };
                EmptyState::new(phpide_icons::IconId::Structure, title)
                    .description(description)
                    .show(ui);
                return;
            }
            let p = crate::design::current(ui.ctx()).palette;
            let path = self.ws.active_buffer().and_then(|b| b.path.clone());
            let mut jump = None;
            egui::ScrollArea::vertical()
                .id_salt("structure-list")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for entry in &self.ws.outline {
                        let row = ui
                            .horizontal(|ui| {
                                ui.add_space(14.0 * entry.depth as f32);
                                ListRow::new(&entry.name)
                                    .icon(crate::app_intel::symbol_icon(&entry.kind), p.accent)
                                    .detail(entry.kind.clone())
                                    .trailing(format!(":{}", entry.line0 + 1))
                                    .show(ui)
                            })
                            .inner;
                        if row.clicked() {
                            jump = Some(entry.line0);
                        }
                    }
                });
            if let (Some(line0), Some(path)) = (jump, path) {
                self.ws.open_file_at(&path, line0, 0);
            }
        });
    }
}

/// Header padrão das tool windows: título + Options (...) com Hide.
pub(crate) fn tool_window_header(app: &mut PhpIdeApp, ui: &mut egui::Ui, tw: ToolWindow) {
    use crate::design::components::{menu_item, menu_separator, IconButton, PanelHeader};
    use phpide_icons::IconId;
    PanelHeader::new(tw.title()).show(ui, |ui| {
        if IconButton::new(phpide_icons::IconId::Minus, "Hide")
            .shortcut("Shift+Esc")
            .show(ui)
            .clicked()
        {
            app.tw.set_visible(tw, false);
        }
        let more = IconButton::new(phpide_icons::IconId::More, "Options").show(ui);
        egui::Popup::menu(&more).show(|ui| {
            ui.set_min_width(220.0);
            if menu_item(ui, Some(IconId::Minus), "Hide", "Shift+Esc", true).clicked() {
                app.tw.set_visible(tw, false);
                ui.close();
            }
            move_to_menu(app, ui, tw);
            menu_separator(ui);
            if menu_item(ui, None, "Hide All Tool Windows", "Ctrl+Shift+F12", true).clicked() {
                app.tw.toggle_hide_all();
                ui.close();
            }
        });
    });
}

/// "Move To" submenu of a tool window header (RF-UI-02).
pub(crate) fn move_to_menu(app: &mut PhpIdeApp, ui: &mut egui::Ui, tw: ToolWindow) {
    use crate::design::components::{menu_radio_item, menu_submenu};
    menu_submenu(ui, None, "Move To", true, |ui| {
        ui.set_min_width(180.0);
        let current = app.tw.placement(tw);
        for placement in crate::toolwindows::Placement::ALL {
            if menu_radio_item(ui, placement.label(), current == placement, true).clicked() {
                app.tw.set_placement(tw, placement);
                app.settings.tool_window_placements = app.tw.placements();
                app.persist_settings();
                ui.close();
            }
        }
    });
}
