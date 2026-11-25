//! Conteúdo das tool windows auxiliares da shell. Project/Find/Run/Debug/
//! Problems/Structure possuem painéis especializados em `main.rs`.

use std::path::PathBuf;

use eframe::egui;

use crate::settings_dialog::SettingsPage;
use crate::tool_window_header;
use crate::toolwindows::ToolWindow;
use crate::PhpIdeApp;

pub fn show(app: &mut PhpIdeApp, ui: &mut egui::Ui, tw: ToolWindow) {
    let size = match tw {
        ToolWindow::Database => 360.0,
        ToolWindow::Commit => 330.0,
        ToolWindow::VersionControl => 300.0,
        _ => 240.0,
    };
    let placement = app.tw.placement(tw);
    crate::toolwindows::dock(ui, tw, placement, size, |ui| panel(app, ui, tw));
}

fn panel(app: &mut PhpIdeApp, ui: &mut egui::Ui, tw: ToolWindow) {
    tool_window_header(app, ui, tw);
    match tw {
        ToolWindow::Bookmarks => bookmarks(app, ui),
        ToolWindow::Services => services(app, ui),
        ToolWindow::VersionControl => version_control(app, ui),
        ToolWindow::Commit => commit(app, ui),
        ToolWindow::Terminal => terminal(app, ui),
        ToolWindow::Database => database(app, ui),
        ToolWindow::Notifications => notifications(app, ui),
        ToolWindow::RemoteHost => remote_host(app, ui),
        _ => {}
    }
}

fn bookmarks(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    if ui.button("Toggle Bookmark\tF11").clicked() {
        app.toggle_bookmark();
    }
    ui.separator();
    let mut rows: Vec<(PathBuf, usize)> = app
        .bookmarks
        .iter()
        .flat_map(|(path, lines)| lines.iter().map(move |line| (path.clone(), *line)))
        .collect();
    rows.sort();
    if rows.is_empty() {
        ui.label(egui::RichText::new("No bookmarks. Press F11 in the editor.").weak());
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        for (path, line) in rows {
            let label = format!(
                "{}:{line}",
                path.strip_prefix(&app.ws.root).unwrap_or(&path).display()
            );
            if ui.selectable_label(false, label).clicked() {
                app.ws.open_file_at(&path, line.saturating_sub(1), 0);
            }
        }
    });
}

fn services(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("Running Services").strong());
    for (name, status) in [
        ("PHP Language Server", app.lsp_label()),
        (
            "PHP Run",
            if app.exec.running {
                "Running"
            } else {
                "Stopped"
            }
            .to_string(),
        ),
        (
            "Xdebug Listener",
            if app.debug_listen {
                format!(
                    "Listening on port {}",
                    crate::run_debug::RunDebugState::debug_port()
                )
            } else {
                "Stopped".to_string()
            },
        ),
        (
            "File Watcher",
            if app.watcher.is_some() {
                "Watching project"
            } else {
                "Stopped"
            }
            .to_string(),
        ),
    ] {
        ui.horizontal(|ui| {
            ui.label(if status.contains("Stopped") || status.contains("off") {
                "[off]"
            } else {
                "[on]"
            });
            ui.label(name);
            ui.label(egui::RichText::new(status).weak());
        });
    }
}

fn version_control(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    crate::vcs_panel::show(app, ui);
}

fn commit(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    crate::commit_panel::show(app, ui);
}

fn terminal(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    crate::terminal_panel::show(app, ui);
}

fn database(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    crate::database::show(app, ui);
}

fn notifications(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    use crate::design::components::{paint_icon, EmptyState};
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!(
                "{} events · Language Server: {}",
                app.event_log.len(),
                app.lsp_label()
            ))
            .weak()
            .small(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(!app.event_log.is_empty(), egui::Button::new("Clear All"))
                .clicked()
            {
                app.event_log.clear();
            }
        });
    });
    ui.separator();
    if app.event_log.is_empty() {
        EmptyState::new(phpide_icons::IconId::Notifications, "No notifications")
            .description("Messages, warnings and errors appear here.")
            .show(ui);
        return;
    }
    let palette = crate::design::theme::current(ui.ctx()).palette;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for event in app.event_log.entries() {
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                    paint_icon(
                        ui.painter(),
                        rect,
                        event.tone.icon(),
                        event.tone.color(&palette),
                    );
                    ui.label(egui::RichText::new(&event.time).weak().small().monospace());
                    ui.add(egui::Label::new(&event.text).wrap());
                });
            }
        });
}

fn remote_host(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("Deployment Servers").strong());
    ui.label(format!("Default: {}", app.settings.deployment_server));
    if ui.button("Configure Deployment...").clicked() {
        app.settings_dialog.open(SettingsPage::Named("Deployment"));
    }
    ui.label(egui::RichText::new("FTP · FTPS · SFTP · WebDAV · local/mounted").weak());
}
