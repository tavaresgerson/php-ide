//! Window header no estilo New UI (REQUISITOS 1.1): main menu (hamburger),
//! widget de Projeto, widget VCS (branch) e widget Run — tudo customizável
//! por menus. Substitui a barra superior do Estágio 2.

use eframe::egui;

use crate::toolwindows::ToolWindow;
use crate::vcs;
use crate::{PaletteMode, PhpIdeApp};

/// Desenha o header unificado. Deve ser chamado antes dos painéis laterais.
pub fn header(app: &mut PhpIdeApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("header").show(ctx, |ui| {
        ui.horizontal(|ui| {
            main_menu(app, ui);
            ui.separator();
            project_widget(app, ui);
            vcs_widget(app, ui);
            run_widget(app, ui);

            // À direita: estado do LSP + contagem de problemas (visão global).
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let errors = app
                    .ws
                    .diagnostics
                    .iter()
                    .filter(|d| d.severity == phpide_core::Severity::Error)
                    .count();
                let warnings = app
                    .ws
                    .diagnostics
                    .iter()
                    .filter(|d| d.severity == phpide_core::Severity::Warning)
                    .count();
                let diag_label = if errors + warnings == 0 {
                    "0 problemas".to_string()
                } else {
                    format!("E {errors} · W {warnings}")
                };
                if ui
                    .selectable_label(app.tw.is_visible(ToolWindow::Problems), diag_label)
                    .on_hover_text("Problems (Alt+6)")
                    .clicked()
                {
                    app.tw.toggle(ToolWindow::Problems);
                }
                ui.label(egui::RichText::new(app.lsp_label()).weak().small());
            });
        });
    });
}

/// Main menu — modo hamburger (REQUISITOS 1.1: *Hamburger* `Alt+\``).
fn main_menu(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    ui.menu_button("Menu", |ui| {
        ui.set_min_width(220.0);
        ui.label(egui::RichText::new("Arquivo").weak().small());
        if ui.button("Novo arquivo\tCtrl+N").clicked() {
            app.ws.new_file();
            ui.close_menu();
        }
        if ui.button("Salvar\tCtrl+S").clicked() {
            app.save_active();
            ui.close_menu();
        }
        if ui.button("Fechar aba\tCtrl+F4").clicked() {
            app.ws.close_active();
            ui.close_menu();
        }
        ui.separator();
        ui.label(egui::RichText::new("Editar").weak().small());
        if ui.button("Desfazer\tCtrl+Z").clicked() {
            if let Some(buf) = app.ws.active_buffer_mut() {
                buf.undo();
            }
            ui.close_menu();
        }
        if ui.button("Refazer\tCtrl+Shift+Z").clicked() {
            if let Some(buf) = app.ws.active_buffer_mut() {
                buf.redo();
            }
            ui.close_menu();
        }
        ui.separator();
        ui.label(egui::RichText::new("Ver").weak().small());
        for tw in ToolWindow::ALL {
            let mut vis = app.tw.is_visible(tw);
            let text = format!(
                "{} Tool Window: {}\t{}",
                tw.icon(),
                tw.title(),
                tw.shortcut_label()
            );
            if ui.checkbox(&mut vis, text).clicked() {
                app.tw.set_visible(tw, vis);
                ui.close_menu();
            }
        }
        if ui.button("Hide All Tool Windows\tCtrl+Shift+F12").clicked() {
            app.tw.toggle_hide_all();
            ui.close_menu();
        }
        ui.separator();
        ui.label(egui::RichText::new("Navegar").weak().small());
        if ui.button("Ir para definição\tF12").clicked() {
            app.request_definition();
            ui.close_menu();
        }
        if ui.button("Próximo problema\tF2").clicked() {
            app.goto_problem(1);
            ui.close_menu();
        }
        if ui.button("Search Everywhere\tShift×2").clicked() {
            app.open_palette(PaletteMode::All);
            ui.close_menu();
        }
        if ui.button("Find Action\tCtrl+Shift+A").clicked() {
            app.open_palette(PaletteMode::Actions);
            ui.close_menu();
        }
        ui.separator();
        ui.label(egui::RichText::new("Executar").weak().small());
        if ui.button("Rodar arquivo atual\tCtrl+R").clicked() {
            app.run_current_php();
            ui.close_menu();
        }
        if ui.button("Debug arquivo atual\tCtrl+D").clicked() {
            app.debug_run();
            ui.close_menu();
        }
        ui.separator();
        if ui.button("Quick Switch Scheme\tCtrl+`").clicked() {
            app.scheme_open = true;
            ui.close_menu();
        }
        if ui.button("Settings\tCtrl+Alt+S").clicked() {
            app.settings_dialog
                .open(crate::settings_dialog::SettingsPage::Appearance);
            ui.close_menu();
        }
        if ui.button("Welcome Screen").clicked() {
            app.welcome_open = true;
            ui.close_menu();
        }
        if ui.button("Tema (Quick Switch)").clicked() {
            app.scheme_open = true;
            ui.close_menu();
        }
    })
    .response
    .on_hover_text("Main menu (Alt+\\)");
}

/// Widget de projeto: nome da raiz + ações de projeto.
fn project_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let name = app
        .ws
        .root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| app.ws.root.display().to_string());
    ui.menu_button(format!("Project: {name}"), |ui| {
        ui.label(
            egui::RichText::new(app.ws.root.display().to_string())
                .weak()
                .small(),
        );
        ui.separator();
        if ui.button("Recarregar árvore de arquivos").clicked() {
            app.ws.refresh_files();
            ui.close_menu();
        }
        if ui.button("Voltar à Welcome Screen").clicked() {
            app.welcome_open = true;
            ui.close_menu();
        }
    })
    .response
    .on_hover_text("Project widget");
}

/// Widget VCS: branch atual com popup de operações (as operações em si vêm no
/// estágio do Git tool window; aqui são placeholders visuais + refresh).
fn vcs_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    if !vcs::is_repo(&app.ws.root) {
        return;
    }
    let branch = app.git_branch();
    ui.menu_button(format!("Git: {branch}"), |ui| {
        ui.label(egui::RichText::new("Git").weak().small());
        if ui.button("Atualizar branch").clicked() {
            app.git_refresh();
            ui.close_menu();
        }
        if ui.button("Branches / Log...\tAlt+9").clicked() {
            app.tw.set_visible(ToolWindow::VersionControl, true);
            ui.close_menu();
        }
        if ui.button("Commit...\tAlt+0").clicked() {
            app.tw.set_visible(ToolWindow::Commit, true);
            ui.close_menu();
        }
        ui.add_enabled_ui(false, |ui| {
            ui.button("Push...\tCtrl+Shift+K")
                .on_hover_text("Integração Git do roadmap");
        });
    })
    .response
    .on_hover_text("VCS widget (Git)");
}

/// Widget Run: ações de execução/depuração do arquivo atual.
fn run_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    ui.menu_button("▶ Run", |ui| {
        if ui.button("Rodar arquivo atual\tCtrl+R").clicked() {
            app.run_current_php();
            ui.close_menu();
        }
        if ui.button("Debug arquivo atual\tCtrl+D").clicked() {
            app.debug_run();
            ui.close_menu();
        }
        if ui.button("Parar\tCtrl+F2").clicked() {
            app.stop_execution();
            ui.close_menu();
        }
        ui.separator();
        ui.add_enabled_ui(false, |ui| {
            ui.button("Run Configurations...")
                .on_hover_text("Estágio 6 (REQUISITOS 3.1)");
        });
    })
    .response
    .on_hover_text("Run widget");
}
