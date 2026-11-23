//! Window header no estilo New UI (REQUISITOS 1.1): main menu (hamburger),
//! widget de Projeto, widget VCS (branch) e widget Run — tudo customizável
//! por menus. Substitui a barra superior do Estágio 2.

use eframe::egui;

use crate::toolwindows::ToolWindow;
use crate::{PaletteMode, PhpIdeApp};

/// Desenha o header unificado. Deve ser chamado antes dos painéis laterais.
pub fn header(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    egui::Panel::top("header").show(ui, |ui| {
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
                    "0 problems".to_string()
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
        ui.label(egui::RichText::new("File").weak().small());
        if ui.button("New File\tCtrl+N").clicked() {
            app.ws.new_file();
            ui.close();
        }
        if ui.button("Save\tCtrl+S").clicked() {
            app.save_active();
            ui.close();
        }
        if ui.button("Close Tab\tCtrl+F4").clicked() {
            // Same path as Ctrl+F4: a dirty buffer asks before closing.
            app.request_close_active();
            ui.close();
        }
        ui.separator();
        ui.label(egui::RichText::new("Edit").weak().small());
        if ui.button("Undo\tCtrl+Z").clicked() {
            if let Some(buf) = app.ws.active_buffer_mut() {
                buf.undo();
            }
            ui.close();
        }
        if ui.button("Redo\tCtrl+Shift+Z").clicked() {
            if let Some(buf) = app.ws.active_buffer_mut() {
                buf.redo();
            }
            ui.close();
        }
        ui.separator();
        ui.label(egui::RichText::new("View").weak().small());
        for tw in ToolWindow::ALL {
            if !tw.is_available() {
                continue;
            }
            let mut vis = app.tw.is_visible(tw);
            let text = format!(
                "{} Tool Window: {}\t{}",
                tw.icon(),
                tw.title(),
                tw.shortcut_label()
            );
            if ui.checkbox(&mut vis, text).clicked() {
                app.tw.set_visible(tw, vis);
                ui.close();
            }
        }
        if ui.button("Hide All Tool Windows\tCtrl+Shift+F12").clicked() {
            app.tw.toggle_hide_all();
            ui.close();
        }
        ui.separator();
        ui.label(egui::RichText::new("Navigate").weak().small());
        if ui.button("Go to Definition\tF12").clicked() {
            app.request_definition();
            ui.close();
        }
        if ui.button("Next Problem\tF2").clicked() {
            app.goto_problem(1);
            ui.close();
        }
        if ui.button("Search Everywhere\tShift×2").clicked() {
            app.open_palette(PaletteMode::All);
            ui.close();
        }
        if ui.button("Find Action\tCtrl+Shift+A").clicked() {
            app.open_palette(PaletteMode::Actions);
            ui.close();
        }
        ui.separator();
        ui.label(egui::RichText::new("Run").weak().small());
        if ui.button("Run Current File\tCtrl+R").clicked() {
            app.run_current_php();
            ui.close();
        }
        if ui.button("Debug Current File\tCtrl+D").clicked() {
            app.debug_run();
            ui.close();
        }
        ui.separator();
        if ui.button("Quick Switch Scheme\tCtrl+`").clicked() {
            app.scheme_open = true;
            ui.close();
        }
        if ui.button("Settings\tCtrl+Alt+S").clicked() {
            app.settings_dialog
                .open(crate::settings_dialog::SettingsPage::Appearance);
            ui.close();
        }
        if ui.button("Welcome Screen").clicked() {
            app.welcome_open = true;
            ui.close();
        }
        if ui.button("Theme (Quick Switch)").clicked() {
            app.scheme_open = true;
            ui.close();
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
        if ui.button("Reload Project Tree").clicked() {
            app.files.refresh(&app.jobs, ui.ctx());
            ui.close();
        }
        if ui.button("Reveal Active File").clicked() {
            if let Some(path) = app.ws.active_buffer().and_then(|b| b.path.clone()) {
                app.files.reveal(&app.jobs, ui.ctx(), &path);
                app.selected = Some(path);
                app.tw.set_visible(ToolWindow::Project, true);
            }
            ui.close();
        }
        if ui.button("Back to Welcome Screen").clicked() {
            app.welcome_open = true;
            ui.close();
        }
    })
    .response
    .on_hover_text("Project widget");
}

/// VCS widget: current branch with a menu of Git actions.
fn vcs_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    if !app.git.is_repo() {
        return;
    }
    let branch = app.git_branch();
    ui.menu_button(format!("Git: {branch}"), |ui| {
        ui.label(egui::RichText::new("Git").weak().small());
        if ui.button("Refresh Branch").clicked() {
            app.git_refresh();
            ui.close();
        }
        if ui.button("Branches / Log...\tAlt+9").clicked() {
            app.tw.set_visible(ToolWindow::VersionControl, true);
            ui.close();
        }
        if ui.button("Commit...\tAlt+0").clicked() {
            app.tw.set_visible(ToolWindow::Commit, true);
            ui.close();
        }
    })
    .response
    .on_hover_text("VCS widget (Git)");
}

/// Widget Run: ações de execução/depuração do arquivo atual.
fn run_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    ui.menu_button("▶ Run", |ui| {
        if ui.button("Run Current File\tCtrl+R").clicked() {
            app.run_current_php();
            ui.close();
        }
        if ui.button("Debug Current File\tCtrl+D").clicked() {
            app.debug_run();
            ui.close();
        }
        if ui.button("Stop\tCtrl+F2").clicked() {
            app.stop_execution();
            ui.close();
        }
    })
    .response
    .on_hover_text("Run widget");
}
