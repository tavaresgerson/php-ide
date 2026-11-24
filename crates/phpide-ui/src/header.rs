//! Window header no estilo New UI (REQUISITOS 1.1): main menu (hamburger),
//! widget de Projeto, widget VCS (branch) e widget Run — tudo customizável
//! por menus. Substitui a barra superior do Estágio 2.

use eframe::egui;

use crate::commands::{self, is_available, Category, Command, REGISTRY};
use crate::toolwindows::ToolWindow;
use crate::PhpIdeApp;

/// Desenha o header unificado. Deve ser chamado antes dos painéis laterais.
pub fn header(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    egui::Panel::top("header").show(ui, |ui| {
        crate::design::components::ghost_controls(ui);
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
        let mut chosen = None;
        for category in Category::ALL {
            ui.menu_button(category.label(), |ui| {
                ui.set_min_width(260.0);
                for spec in REGISTRY {
                    if spec.category != category || !spec.in_menu || !is_available(spec.id) {
                        continue;
                    }
                    let mut button = egui::Button::new(spec.label)
                        .shortcut_text(commands::shortcut_text(spec.id));
                    if let Command::ToggleToolWindow(tw) = spec.id {
                        button = button.selected(app.tw.is_visible(tw));
                    }
                    if ui.add(button).clicked() {
                        chosen = Some(spec.id);
                        ui.close();
                    }
                    if spec.id == Command::OpenProject {
                        recent_projects_menu(app, ui);
                    }
                }
            });
        }
        if let Some(command) = chosen {
            let ctx = ui.ctx().clone();
            app.dispatch_command(command, &ctx);
        }
    })
    .response
    .on_hover_text("Main menu");
}

/// File › Recent Projects (RF-PRJ-03).
fn recent_projects_menu(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let recents: Vec<String> = app
        .settings
        .recent_projects
        .iter()
        .filter(|p| std::path::Path::new(p.as_str()) != app.ws.root || !app.project_open)
        .cloned()
        .collect();
    ui.add_enabled_ui(!recents.is_empty(), |ui| {
        ui.menu_button("Recent Projects", |ui| {
            ui.set_min_width(320.0);
            for path in recents {
                let name = std::path::Path::new(&path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone());
                let button = egui::Button::new(name).shortcut_text(path.clone());
                if ui.add(button).clicked() {
                    let ctx = ui.ctx().clone();
                    app.open_project_at(std::path::PathBuf::from(path), &ctx);
                    ui.close();
                }
            }
        });
    });
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
        recent_projects_menu(app, ui);
        if ui.button("Open Project…").clicked() {
            app.open_project_dialog(ui.ctx());
            ui.close();
        }
        if ui.button("Close Project").clicked() {
            app.request_close_project(ui.ctx());
            ui.close();
        }
        ui.separator();
        if ui.button("Reload Project Tree").clicked() {
            app.files.refresh(&app.jobs, ui.ctx());
            ui.close();
        }
        if ui.button("Reveal Active File").clicked() {
            app.tw.set_visible(ToolWindow::Project, true);
            crate::project_tree::follow_active_file(app, ui.ctx(), true);
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
        if ui.button("Run Current File\tShift+F10").clicked() {
            app.run_current_php();
            ui.close();
        }
        if ui.button("Debug Current File\tShift+F9").clicked() {
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
