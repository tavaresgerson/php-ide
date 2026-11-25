//! Window header no estilo New UI (REQUISITOS 1.1): main menu (hamburger),
//! widget de Projeto, widget VCS (branch) e widget Run — tudo customizável
//! por menus. Substitui a barra superior do Estágio 2.

use eframe::egui;

use crate::commands::{self, is_available, Category, Command, REGISTRY};
use crate::design::components::{
    menu_check_item, menu_item, menu_radio_item, menu_separator, menu_submenu, IconButton,
    ToolbarWidget,
};
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
            menu_submenu(ui, None, category.label(), true, |ui| {
                ui.set_min_width(280.0);
                for spec in REGISTRY {
                    if spec.category != category || !spec.in_menu || !is_available(spec.id) {
                        continue;
                    }
                    let shortcut = commands::shortcut_text(spec.id);
                    let response = match spec.id {
                        Command::ToggleToolWindow(tw) => menu_check_item(
                            ui,
                            spec.icon(),
                            spec.label,
                            &shortcut,
                            app.tw.is_visible(tw),
                        ),
                        _ => menu_item(ui, spec.icon(), spec.label, &shortcut, true),
                    };
                    if response.clicked() {
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
    menu_separator(ui);
    let me = std::process::id();
    for window in windows {
        let name = window
            .root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| window.root.display().to_string());
        let response = menu_radio_item(ui, &name, window.pid == me, true)
            .on_hover_text(window.root.display().to_string());
        if response.clicked() {
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
    let enabled = !recents.is_empty();
    menu_submenu(ui, Some(IconId::Clock), "Recent Projects", enabled, |ui| {
        ui.set_min_width(320.0);
        for path in recents {
            let name = std::path::Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone());
            if menu_item(ui, None, &name, &path, true).clicked() {
                let ctx = ui.ctx().clone();
                app.open_project_at(std::path::PathBuf::from(path), &ctx);
                ui.close();
            }
        }
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
        menu_separator(ui);
        recent_projects_menu(app, ui);
        if menu_item(ui, Some(IconId::FolderOpen), "Open Project…", "", true).clicked() {
            app.open_project_dialog(ui.ctx());
            ui.close();
        }
        if menu_item(ui, None, "Close Project", "", true).clicked() {
            app.request_close_project(ui.ctx());
            ui.close();
        }
        menu_separator(ui);
        if menu_item(ui, Some(IconId::Refresh), "Reload Project Tree", "", true).clicked() {
            app.files.refresh(&app.jobs, ui.ctx());
            ui.close();
        }
        if menu_item(ui, Some(IconId::Locate), "Reveal Active File", "", true).clicked() {
            app.tw.set_visible(ToolWindow::Project, true);
            crate::project_tree::follow_active_file(app, ui.ctx(), true);
            ui.close();
        }
        if menu_item(ui, Some(IconId::Home), "Back to Welcome Screen", "", true).clicked() {
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
        // Each configuration with the icon of its kind; the selected one
        // has its icon boxed, as a checked item.
        if menu_check_item(
            ui,
            Some(IconId::FilePhp),
            "Current File",
            "",
            selected.is_none(),
        )
        .clicked()
        {
            choose = Some(None);
        }
        let listed = |configs: &[phpide_run::RunConfig]| -> Vec<(String, String, IconId)> {
            configs
                .iter()
                .map(|c| {
                    (
                        c.id.clone(),
                        c.name.clone(),
                        crate::run_configs::kind_icon(&c.kind),
                    )
                })
                .collect()
        };
        let saved = listed(&app.rd.configs);
        if !saved.is_empty() {
            menu_separator(ui);
            for (id, name, icon) in saved {
                if menu_check_item(ui, Some(icon), &name, "", selected.as_ref() == Some(&id))
                    .clicked()
                {
                    choose = Some(Some(id));
                }
            }
        }
        let temporary = listed(&app.rd.temporary);
        if !temporary.is_empty() {
            menu_separator(ui);
            ui.label(egui::RichText::new("Temporary").weak().small());
            for (id, name, icon) in &temporary {
                if menu_check_item(ui, Some(*icon), name, "", selected.as_ref() == Some(id))
                    .clicked()
                {
                    choose = Some(Some(id.clone()));
                }
            }
        }
        menu_separator(ui);
        if let Some(id) = selected.filter(|id| temporary.iter().any(|(t, _, _)| t == id)) {
            if menu_item(ui, Some(IconId::Save), "Save Configuration", "", true).clicked() {
                app.save_temporary(&id);
                ui.close();
            }
        }
        if menu_item(ui, Some(IconId::Edit), "Edit Configurations…", "", true).clicked() {
            app.open_edit_configurations();
            ui.close();
        }
        if let Some(choice) = choose {
            app.rd.selected_config_id = choice;
            ui.close();
        }
    });
}
