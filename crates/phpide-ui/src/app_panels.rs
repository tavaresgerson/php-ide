//! Tool window panels and modal dialogs drawn around the editor.

use super::*;

impl PhpIdeApp {
    pub(crate) fn file_tree(&mut self, ui: &mut egui::Ui) {
        project_tree::show(self, ui);
    }

    pub(crate) fn search_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("search")
            .resizable(true)
            .show(ui, |ui| {
                tool_window_header(self, ui, ToolWindow::Find);
                ui.horizontal(|ui| {
                    let edit = egui::TextEdit::singleline(&mut self.search.query)
                        .hint_text("text (Enter to search)")
                        .desired_width(280.0)
                        .show(ui);
                    if edit.response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let files: Vec<PathBuf> = self
                            .ws
                            .files
                            .iter()
                            .filter(|e| !e.is_dir && search::is_searchable(&e.path))
                            .map(|e| e.path.clone())
                            .collect();
                        self.search.start(files, &self.jobs);
                    }
                    ui.checkbox(&mut self.search.case_sensitive, "Aa");
                    if ui.button("Search").clicked() {
                        let files: Vec<PathBuf> = self
                            .ws
                            .files
                            .iter()
                            .filter(|e| !e.is_dir && search::is_searchable(&e.path))
                            .map(|e| e.path.clone())
                            .collect();
                        self.search.start(files, &self.jobs);
                    }
                    if self.search.busy() {
                        ui.spinner();
                    }
                    ui.label(&self.search.status);
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(200.0)
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
        egui::Panel::bottom("problems")
            .resizable(true)
            .show(ui, |ui| {
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
                                    .selectable_label(
                                        false,
                                        egui::RichText::new(label).color(color),
                                    )
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
        egui::Panel::right("outline")
            .resizable(true)
            .default_size(210.0)
            .show(ui, |ui| {
                tool_window_header(self, ui, ToolWindow::Structure);
                if self.ws.outline.is_empty() {
                    ui.label("—");
                } else {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let entries = self.ws.outline.clone();
                        for e in entries {
                            let jump = ui
                                .horizontal(|ui| {
                                    ui.add_space(12.0 * e.depth as f32);
                                    ui.selectable_label(false, format!("{}  {}", e.kind, e.name))
                                        .clicked()
                                })
                                .inner;
                            if jump {
                                let path = self.ws.active_buffer().and_then(|b| b.path.clone());
                                if let Some(path) = path {
                                    self.ws.open_file_at(&path, e.line0, 0);
                                }
                            }
                        }
                    });
                }
            });
    }
}

/// Header padrão das tool windows: título + Options (...) com Hide.
pub(crate) fn tool_window_header(app: &mut PhpIdeApp, ui: &mut egui::Ui, tw: ToolWindow) {
    use crate::design::components::{menu_item, menu_separator, IconButton, PanelHeader};
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
            if menu_item(ui, None, "Hide", "Shift+Esc", true).clicked() {
                app.tw.set_visible(tw, false);
                ui.close();
            }
            menu_separator(ui);
            if menu_item(ui, None, "Hide All Tool Windows", "Ctrl+Shift+F12", true).clicked() {
                app.tw.toggle_hide_all();
                ui.close();
            }
        });
    });
}
