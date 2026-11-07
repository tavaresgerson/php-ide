//! Renderização dos painéis Run / Debug / Test (Estágio 8).
//!
//! Consome o estado [`RunDebugState`]: process list com Rerun/Stop/Pause
//! Output/Pin, output por sessão, árvore de testes com duração e falhas
//! navegáveis, toolbar de stepping, frames, variáveis, watches, evaluate e
//! lista de breakpoints com estados do backend.

use eframe::egui;
use phpide_dbgp::session::{ContinueCommand, Watch};
use phpide_run::session::{SessionId, SessionState};
use phpide_run::{RunKind, TestNode, TestStatus};

use crate::run_debug::DebugStatus;
use crate::PhpIdeApp;

/// Snapshot das sessões para iteração sem borrow do manager.
fn session_snapshots(
    manager: &phpide_run::RunManager,
) -> Vec<(SessionId, String, SessionState, bool, bool)> {
    manager
        .sessions()
        .iter()
        .map(|s| {
            (
                s.id,
                s.title(),
                s.state,
                s.pinned,
                s.state == SessionState::PausedOutput,
            )
        })
        .collect()
}

/// Seletor de Run Configuration + botão Run (RUN-001/002).
fn config_selector(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    if app.rd.configs.is_empty() {
        return;
    }
    let names: Vec<String> = app.rd.configs.iter().map(|c| c.name.clone()).collect();
    let mut sel = app.rd.selected_config.min(names.len() - 1);
    egui::ComboBox::from_id_salt("run-config-selector")
        .selected_text(names.get(sel).cloned().unwrap_or_default())
        .show_ui(ui, |ui| {
            for (i, name) in names.iter().enumerate() {
                ui.selectable_value(&mut sel, i, name);
            }
        });
    app.rd.selected_config = sel;
    if ui
        .button("▶")
        .on_hover_text("Run selected configuration")
        .clicked()
    {
        let php = crate::exec::resolve_php(&app.settings)
            .map_err(|e| format!("{} — {}", e.message, e.action));
        match php {
            Ok(php) => {
                let ctx = crate::exec::build_context(&app.settings, &php);
                if let Err(e) = app.rd.start_config(&app.ws.root, &ctx) {
                    app.ws.message = e;
                }
            }
            Err(e) => app.ws.message = e,
        }
    }
}

fn state_icon(state: SessionState) -> &'static str {
    match state {
        SessionState::Starting | SessionState::Running => "▶",
        SessionState::PausedOutput => "Ⅱ",
        SessionState::Finished => "✓",
        SessionState::Stopped => "■",
        SessionState::Failed => "✗",
    }
}

/// Tool window Run: lista de processos (sessões) + conteúdo da sessão.
pub fn run_panel(app: &mut PhpIdeApp, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("run-panel")
        .resizable(true)
        .default_height(230.0)
        .show(ctx, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Run").strong());
                config_selector(app, ui);
                let live = app
                    .rd
                    .manager
                    .sessions()
                    .iter()
                    .filter(|s| s.state.is_live())
                    .count();
                if live > 0 {
                    ui.label(
                        egui::RichText::new(format!("{live} em execução"))
                            .weak()
                            .small(),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("X").on_hover_text("Hide (Shift+Esc)").clicked() {
                        app.tw
                            .set_visible(crate::toolwindows::ToolWindow::Run, false);
                    }
                    if ui
                        .button("Clear Finished")
                        .on_hover_text("Reciclar abas encerradas não pinadas")
                        .clicked()
                    {
                        app.rd.manager.reclaim_finished(5);
                    }
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(250.0);
                    session_list(app, ui);
                });
                ui.separator();
                ui.vertical(|ui| {
                    session_content(app, ui);
                });
            });
        });
}

fn session_list(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let sessions = session_snapshots(&app.rd.manager);
    egui::ScrollArea::vertical().show(ui, |ui| {
        for (id, title, state, pinned, paused) in sessions {
            let selected = app.rd.selected == Some(id);
            ui.horizontal(|ui| {
                let icon = state_icon(state);
                if ui
                    .selectable_label(selected, icon)
                    .on_hover_text(state.as_str())
                    .clicked()
                {
                    app.rd.selected = Some(id);
                }
                let label = if pinned {
                    format!("📌 {title}")
                } else {
                    title
                };
                if ui.selectable_label(selected, label).clicked() {
                    app.rd.selected = Some(id);
                }
                // Ações por sessão (Rerun/Stop/Pause Output/Pin/Close).
                if ui.small_button("R").on_hover_text("Rerun").clicked() {
                    app.rd.manager.rerun(id);
                }
                if state.is_live()
                    && ui
                        .small_button("■")
                        .on_hover_text("Stop (Ctrl+F2)")
                        .clicked()
                {
                    app.rd.manager.stop(id);
                }
                if ui
                    .small_button(if paused { "▶" } else { "Ⅱ" })
                    .on_hover_text("Pause Output")
                    .clicked()
                {
                    if let Some(s) = app.rd.manager.session_mut(id) {
                        s.set_output_paused(!paused);
                    }
                }
                if ui
                    .small_button(if pinned { "📍" } else { "📌" })
                    .on_hover_text("Pin Tab")
                    .clicked()
                {
                    if let Some(s) = app.rd.manager.session_mut(id) {
                        s.pinned = !pinned;
                    }
                }
                if ui.small_button("✕").on_hover_text("Fechar aba").clicked() {
                    app.rd.manager.stop(id);
                    if app.rd.selected == Some(id) {
                        app.rd.selected = None;
                    }
                }
            });
        }
        if app.rd.manager.sessions().is_empty() {
            ui.label(egui::RichText::new("Nenhuma execução. Ctrl+R roda o arquivo atual.").weak());
        }
    });
}

fn session_content(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let Some(id) = app.rd.selected else {
        ui.label(egui::RichText::new("Selecione uma sessão na lista.").weak());
        return;
    };
    let Some(session) = app.rd.manager.session(id) else {
        return;
    };
    let title = session.title();
    let state = session.state;
    let exit_code = session.exit_code;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).strong());
        ui.label(egui::RichText::new(state.as_str()).weak());
        if state.is_live() {
            ui.spinner();
        }
        if let Some(code) = exit_code {
            ui.label(egui::RichText::new(format!("exit {code}")).weak().small());
        }
    });
    ui.separator();
    let is_test = matches!(session.config.kind, RunKind::PhpUnit { .. });
    if is_test && app.rd.test_trees.contains_key(&id) {
        test_tree_view(app, ui, id);
    } else {
        output_view(app, ui, id);
    }
}

fn output_view(app: &mut PhpIdeApp, ui: &mut egui::Ui, id: SessionId) {
    let Some(session) = app.rd.manager.session(id) else {
        return;
    };
    let text = session.output().to_string();
    egui::ScrollArea::vertical()
        .stick_to_bottom(true)
        .show(ui, |ui| {
            ui.add(egui::Label::new(egui::RichText::new(text).monospace()).selectable(true));
        });
}

fn test_tree_view(app: &mut PhpIdeApp, ui: &mut egui::Ui, id: SessionId) {
    let Some(tree) = app.rd.test_trees.get(&id).cloned() else {
        return;
    };
    let stats = tree.stats();
    ui.horizontal(|ui| {
        ui.label(format!(
            "total {} · passed {} · failed {} · skipped {}",
            stats.total, stats.passed, stats.failed, stats.skipped
        ));
        ui.label(
            egui::RichText::new(format!("duração {:?}", stats.duration))
                .weak()
                .small(),
        );
        if let Some(coverage) = &app.rd.coverage {
            ui.label(
                egui::RichText::new(format!(
                    "coverage {:.1}%",
                    coverage.percent_tenths() as f32 / 10.0
                ))
                .weak()
                .small(),
            );
        }
    });
    ui.separator();
    egui::ScrollArea::vertical().show(ui, |ui| {
        for root in &tree.roots {
            render_test_node(app, ui, root);
        }
    });
}

fn render_test_node(app: &mut PhpIdeApp, ui: &mut egui::Ui, node: &TestNode) {
    if node.is_suite {
        ui.label(egui::RichText::new(&node.name).strong().small());
        ui.indent(node.name.clone(), |ui| {
            for child in &node.children {
                render_test_node(app, ui, child);
            }
        });
        return;
    }
    let (icon, color) = match node.status {
        Some(TestStatus::Passed) => ("✓", egui::Color32::from_rgb(0x6A, 0xC1, 0x6A)),
        Some(TestStatus::Failed) => ("✗", egui::Color32::from_rgb(0xD9, 0x4A, 0x4A)),
        Some(TestStatus::Skipped) => ("–", egui::Color32::GRAY),
        Some(TestStatus::Incomplete) => ("○", egui::Color32::YELLOW),
        _ => ("…", egui::Color32::GRAY),
    };
    let duration = node
        .duration
        .map(|d| format!(" {:.0}ms", d.as_secs_f64() * 1000.0))
        .unwrap_or_default();
    let label = format!("{icon} {}{duration}", node.name);
    let response = ui
        .selectable_label(false, egui::RichText::new(label).color(color).small())
        .on_hover_text(node.message.as_deref().unwrap_or(""));
    // Clique na falha navega até arquivo:linha (RUN-TST-001-02).
    if response.clicked() {
        if let Some((file, line)) = &node.location {
            app.ws.open_file_at(file, line.saturating_sub(1), 0);
        }
    }
}

// ---------------------------------------------------------------------------
// Painel Debug
// ---------------------------------------------------------------------------

/// Tool window Debug: toolbar de stepping + inspeção da sessão DBGp.
pub fn debug_panel(app: &mut PhpIdeApp, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("debug-panel")
        .resizable(true)
        .default_height(260.0)
        .show(ctx, |ui| {
            debug_toolbar(app, ui);
            ui.separator();
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(320.0);
                    frames_and_variables(app, ui);
                });
                ui.separator();
                ui.vertical(|ui| {
                    watches_and_eval(app, ui);
                });
                ui.separator();
                ui.vertical(|ui| {
                    breakpoints_list(app, ui);
                });
            });
        });
}

fn debug_toolbar(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let paused = app.rd.status == DebugStatus::Paused;
    let active = app.rd.status.is_active();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Debug").strong());
        ui.label(egui::RichText::new(app.rd.status.as_str()).weak().small());
        if let Some((file, line)) = &app.rd.last_stop {
            ui.label(
                egui::RichText::new(format!("{}:{}", file.display(), line))
                    .weak()
                    .small(),
            );
        }
        if ui
            .add_enabled(paused, egui::Button::new("Resume (F9)"))
            .clicked()
        {
            app.rd.continue_debug(ContinueCommand::Resume);
        }
        if ui
            .add_enabled(paused, egui::Button::new("Step Over (F8)"))
            .clicked()
        {
            app.rd.continue_debug(ContinueCommand::StepOver);
        }
        if ui
            .add_enabled(paused, egui::Button::new("Step Into (F7)"))
            .clicked()
        {
            app.rd.continue_debug(ContinueCommand::StepInto);
        }
        if ui
            .add_enabled(paused, egui::Button::new("Step Out (Shift+F8)"))
            .clicked()
        {
            app.rd.continue_debug(ContinueCommand::StepOut);
        }
        if ui
            .add_enabled(
                app.rd.last_stop.is_some(),
                egui::Button::new("Show Execution Point (Alt+F10)"),
            )
            .on_hover_text("Navega ao ponto de execução parado")
            .clicked()
        {
            show_execution_point(app);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("X").on_hover_text("Hide (Shift+Esc)").clicked() {
                app.tw
                    .set_visible(crate::toolwindows::ToolWindow::Debug, false);
            }
            if ui.add_enabled(active, egui::Button::new("Stop")).clicked() {
                app.rd.stop();
            }
            if ui
                .add_enabled(active, egui::Button::new("Detach"))
                .clicked()
            {
                app.rd.detach();
            }
        });
    });
}

/// Alt+F10 — mostra o ponto de execução atual no editor.
pub fn show_execution_point(app: &mut PhpIdeApp) {
    if let Some((file, line)) = app.rd.last_stop.clone() {
        app.ws
            .open_file_at(&file, line.saturating_sub(1) as usize, 0);
        app.ws.message = format!("execução parada em {}:{line}", file.display());
    }
}

fn frames_and_variables(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("Frames").strong().small());
    egui::ScrollArea::vertical()
        .max_height(110.0)
        .show(ui, |ui| {
            let frames = app.rd.frames.clone();
            for (i, frame) in frames.iter().enumerate() {
                let selected = app.rd.selected_frame == i;
                let label = format!(
                    "{}  {}:{}",
                    frame.name,
                    frame
                        .file
                        .as_ref()
                        .map(|f| f.display().to_string())
                        .unwrap_or_else(|| "?".into()),
                    frame.line
                );
                if ui
                    .selectable_label(selected, egui::RichText::new(label).small())
                    .clicked()
                {
                    app.rd.select_frame(i);
                }
            }
            if frames.is_empty() {
                ui.label(egui::RichText::new("Sem sessão parada.").weak().small());
            }
        });
    ui.separator();
    ui.label(egui::RichText::new("Variables").strong().small());
    egui::ScrollArea::vertical().show(ui, |ui| {
        let variables = app.rd.variables.clone();
        for var in &variables {
            ui.label(
                egui::RichText::new(format!(
                    "{} {} = {}",
                    var.type_name,
                    var.fullname,
                    var.value.as_deref().unwrap_or("∅")
                ))
                .small()
                .monospace(),
            );
        }
    });
}

fn watches_and_eval(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("Watches").strong().small());
    ui.horizontal(|ui| {
        let next_id = app.rd.watches.iter().map(|w| w.id).max().unwrap_or(0) + 1;
        if ui
            .small_button("+")
            .on_hover_text("Adicionar watch")
            .clicked()
        {
            app.rd.watches.push(Watch {
                id: next_id,
                expression: "$this".to_string(),
            });
            app.rd.refresh_watches();
        }
    });
    egui::ScrollArea::vertical()
        .max_height(90.0)
        .show(ui, |ui| {
            let watches = app.rd.watches.clone();
            for (i, watch) in watches.iter().enumerate() {
                ui.horizontal(|ui| {
                    let mut expr = watch.expression.clone();
                    if ui
                        .add(egui::TextEdit::singleline(&mut expr).desired_width(140.0))
                        .changed()
                    {
                        app.rd.watches[i].expression = expr;
                        app.rd.refresh_watches();
                    }
                    let value = app
                        .rd
                        .watch_values
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| "—".to_string());
                    ui.label(egui::RichText::new(value).small().monospace());
                    if ui.small_button("✕").clicked() {
                        app.rd.watches.remove(i);
                        app.rd.watch_values.clear();
                    }
                });
            }
            if watches.is_empty() {
                ui.label(egui::RichText::new("Sem watches.").weak().small());
            }
        });
    ui.separator();
    ui.label(egui::RichText::new("Evaluate (Alt+F8)").strong().small());
    ui.horizontal(|ui| {
        let mut input = app.rd.eval_input.clone();
        let response = ui.add(
            egui::TextEdit::singleline(&mut input)
                .hint_text("expressão no frame selecionado")
                .desired_width(240.0),
        );
        let submit = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if input != app.rd.eval_input {
            app.rd.eval_input = input;
        }
        if (submit || ui.button("Evaluate").clicked()) && !app.rd.eval_input.trim().is_empty() {
            let expr = app.rd.eval_input.clone();
            app.rd.evaluate(&expr);
        }
    });
    if !app.rd.eval_output.is_empty() {
        ui.label(egui::RichText::new(&app.rd.eval_output).small().monospace());
    }
}

fn breakpoints_list(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("Breakpoints").strong().small());
    egui::ScrollArea::vertical().show(ui, |ui| {
        let bps = app.rd.bp_store.lock().unwrap().breakpoints.clone();
        for bp in &bps {
            let status = bp.status.as_str();
            let label = format!(
                "{}:{} ({status})",
                bp.file
                    .as_ref()
                    .map(|f| f.display().to_string())
                    .unwrap_or_else(|| "?".into()),
                bp.line
            );
            ui.horizontal(|ui| {
                let color = match &bp.status {
                    phpide_dbgp::BreakpointStatus::Verified => {
                        egui::Color32::from_rgb(0x6A, 0xC1, 0x6A)
                    }
                    phpide_dbgp::BreakpointStatus::Invalid(_) => {
                        egui::Color32::from_rgb(0xD9, 0x4A, 0x4A)
                    }
                    _ => egui::Color32::GRAY,
                };
                if ui
                    .selectable_label(false, egui::RichText::new(label).color(color).small())
                    .clicked()
                {
                    if let Some(file) = &bp.file {
                        app.ws
                            .open_file_at(file, bp.line.saturating_sub(1) as usize, 0);
                    }
                }
                if ui.small_button("✕").clicked() {
                    app.rd.bp_store.lock().unwrap().remove(bp.id);
                    app.rd.breakpoints_dirty = true;
                }
            });
            if let phpide_dbgp::BreakpointStatus::Invalid(msg) = &bp.status {
                ui.label(egui::RichText::new(msg).weak().small());
            }
        }
        if bps.is_empty() {
            ui.label(
                egui::RichText::new("F9 no editor ou clique no gutter.")
                    .weak()
                    .small(),
            );
        }
    });
}
