//! Window header no estilo New UI (REQUISITOS 1.1): main menu (hamburger),
//! widget de Projeto, widget VCS (branch) e widget Run — tudo customizável
//! por menus. Substitui a barra superior do Estágio 2.

use eframe::egui;

use crate::commands::{self, is_available, Category, Command, REGISTRY};
use crate::design::components::{IconButton, ToolbarWidget};
use crate::toolwindows::ToolWindow;
use crate::PhpIdeApp;
use phpide_icons::IconId;

/// Draws the window header. Must run before the side panels.
pub fn header(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let height = crate::design::theme::current(ui.ctx())
        .metrics
        .window_header;
    egui::Panel::top("header")
        .exact_size(height)
        .show(ui, |ui| {
            crate::design::components::ghost_controls(ui);
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = crate::design::tokens::space::S2;
                main_menu(app, ui);
                project_widget(app, ui);
                vcs_widget(app, ui);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    right_actions(app, ui);
                    run_widget(app, ui);
                });
            });
        });
}

/// Search, Settings, Notifications and the problems summary.
fn right_actions(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    if IconButton::new(IconId::Settings, "Settings")
        .shortcut(commands::shortcut_text(Command::OpenSettings))
        .show(ui)
        .clicked()
    {
        let ctx = ui.ctx().clone();
        app.dispatch_command(Command::OpenSettings, &ctx);
    }
    if IconButton::new(IconId::Search, "Search Everywhere")
        .shortcut("Double Shift")
        .show(ui)
        .clicked()
    {
        let ctx = ui.ctx().clone();
        app.dispatch_command(Command::SearchEverywhere, &ctx);
    }
    let unread = app.event_log.unread();
    let palette = crate::design::theme::current(ui.ctx()).palette;
    let mut bell = IconButton::new(
        IconId::Notifications,
        if unread == 0 {
            "Notifications".to_string()
        } else {
            format!("Notifications ({unread} new)")
        },
    )
    .selected(app.tw.is_visible(ToolWindow::Notifications));
    if unread > 0 {
        bell = bell.tint(palette.warning);
    }
    if bell.show(ui).clicked() {
        app.tw.toggle(ToolWindow::Notifications);
    }
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
    if errors + warnings > 0 {
        let (icon, tint) = if errors > 0 {
            (IconId::Error, palette.error)
        } else {
            (IconId::Warning, palette.warning)
        };
        if ToolbarWidget::new(format!("{}", errors + warnings))
            .icon(icon)
            .icon_tint(tint)
            .chevron(false)
            .tooltip(format!(
                "{errors} errors, {warnings} warnings — Problems (Alt+6)"
            ))
            .show(ui)
            .clicked()
        {
            app.tw.toggle(ToolWindow::Problems);
        }
    }
}

/// Main menu — hamburger mode (REQUISITOS 1.1).
fn main_menu(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let response = IconButton::new(IconId::Menu, "Main menu").show(ui);
    egui::Popup::menu(&response).show(|ui| {
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
                if category == Category::Window {
                    windows_menu(app, ui);
                }
            });
        }
        if let Some(command) = chosen {
            let ctx = ui.ctx().clone();
            app.dispatch_command(command, &ctx);
        }
    });
}

/// Window menu: every open project window; this one is checked (RF-PRJ-10).
fn windows_menu(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let windows = app.windows.all_windows();
    if windows.is_empty() {
        return;
    }
    ui.separator();
    let me = std::process::id();
    for window in windows {
        let name = window
            .root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| window.root.display().to_string());
        let button = egui::Button::new(name)
            .selected(window.pid == me)
            .shortcut_text(window.root.display().to_string());
        if ui.add(button).clicked() {
            if window.pid != me {
                app.windows
                    .send(window.pid, crate::services::windows::WindowCommand::Focus);
            }
            ui.close();
        }
    }
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
    let response = ToolbarWidget::new(name)
        .icon(IconId::Project)
        .tooltip(app.ws.root.display().to_string())
        .show(ui);
    egui::Popup::menu(&response).show(|ui| {
        ui.set_min_width(240.0);
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
    });
}

/// Id of the branch popup (Git › Branches… opens it too).
pub(crate) fn branch_popup() -> egui::Id {
    egui::Id::new("git-branches-popup")
}

/// VCS widget: current branch, incoming/outgoing commits and the branch
/// popup with the Git actions.
fn vcs_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    if !app.git.is_repo() {
        return;
    }
    let mut label = app.git_branch();
    if let Some(current) = app.vcs.snapshot().and_then(|s| s.current_branch()) {
        if current.behind > 0 {
            label.push_str(&format!(" ↓{}", current.behind));
        }
        if current.ahead > 0 {
            label.push_str(&format!(" ↑{}", current.ahead));
        }
    }
    if let Some(op) = app.vcs.snapshot().and_then(|s| s.operation) {
        label.push_str(&format!(" ({})", op.label()));
    }
    let response = ToolbarWidget::new(label)
        .icon(IconId::GitBranch)
        .tooltip("Git branch")
        .show(ui);
    egui::Popup::menu(&response)
        .id(branch_popup())
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| crate::vcs_panel::branches_popup(app, ui));
}

/// Run widget: configuration selector plus Run, Debug and Stop buttons.
fn run_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let palette = crate::design::theme::current(ui.ctx()).palette;
    let running = app.exec.running
        || app.rd.debug_session.is_some()
        || app.rd.manager.sessions().iter().any(|s| s.is_running());
    // Right-to-left: the last button drawn is the leftmost.
    if IconButton::new(IconId::Stop, "Stop")
        .shortcut("Ctrl+F2")
        .enabled(running)
        .tint(palette.error)
        .show(ui)
        .clicked()
    {
        app.stop_execution();
    }
    let target = app
        .selected_run_config()
        .map(|c| c.name.clone())
        .unwrap_or_else(|| "Current File".to_string());
    if IconButton::new(IconId::Debug, format!("Debug '{target}'"))
        .shortcut("Shift+F9")
        .tint(palette.success)
        .show(ui)
        .clicked()
    {
        app.run_selected(true);
    }
    if IconButton::new(IconId::Run, format!("Run '{target}'"))
        .shortcut("Shift+F10")
        .tint(palette.success)
        .show(ui)
        .clicked()
    {
        app.run_selected(false);
    }
    let icon = app
        .selected_run_config()
        .map(|c| crate::run_configs::kind_icon(&c.kind))
        .unwrap_or(IconId::FilePhp);
    let response = ToolbarWidget::new(target)
        .icon(icon)
        .tooltip("Run/Debug Configurations")
        .show(ui);
    egui::Popup::menu(&response).show(|ui| {
        ui.set_min_width(260.0);
        let selected = app.rd.selected_config_id.clone();
        let mut choose: Option<Option<String>> = None;
        if ui
            .add(egui::Button::new("Current File").selected(selected.is_none()))
            .clicked()
        {
            choose = Some(None);
        }
        let saved: Vec<(String, String)> = app
            .rd
            .configs
            .iter()
            .map(|c| (c.id.clone(), c.name.clone()))
            .collect();
        if !saved.is_empty() {
            ui.separator();
            for (id, name) in saved {
                if ui
                    .add(egui::Button::new(name).selected(selected.as_ref() == Some(&id)))
                    .clicked()
                {
                    choose = Some(Some(id));
                }
            }
        }
        let temporary: Vec<(String, String)> = app
            .rd
            .temporary
            .iter()
            .map(|c| (c.id.clone(), c.name.clone()))
            .collect();
        if !temporary.is_empty() {
            ui.separator();
            ui.label(egui::RichText::new("Temporary").weak().small());
            for (id, name) in &temporary {
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new(name).italics())
                            .selected(selected.as_ref() == Some(id)),
                    )
                    .clicked()
                {
                    choose = Some(Some(id.clone()));
                }
            }
        }
        ui.separator();
        if let Some(id) = selected.filter(|id| temporary.iter().any(|(t, _)| t == id)) {
            if ui.button("Save Configuration").clicked() {
                app.save_temporary(&id);
                ui.close();
            }
        }
        if ui.button("Edit Configurations…").clicked() {
            app.open_edit_configurations();
            ui.close();
        }
        if let Some(choice) = choose {
            app.rd.selected_config_id = choice;
            ui.close();
        }
    });
}
