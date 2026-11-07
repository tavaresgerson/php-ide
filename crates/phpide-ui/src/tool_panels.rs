//! Conteúdo das tool windows auxiliares da shell. Project/Find/Run/Debug/
//! Problems/Structure possuem painéis especializados em `main.rs`.

use std::path::PathBuf;

use eframe::egui;

use crate::settings_dialog::SettingsPage;
use crate::tool_window_header;
use crate::toolwindows::{Anchor, ToolWindow};
use crate::PhpIdeApp;

pub fn show(app: &mut PhpIdeApp, ctx: &egui::Context, tw: ToolWindow) {
    match tw.anchor() {
        Anchor::Left => {
            egui::SidePanel::left(format!("tool-window-{}", tw.title()))
                .resizable(true)
                .default_width(240.0)
                .show(ctx, |ui| panel(app, ui, tw));
        }
        Anchor::Right => {
            egui::SidePanel::right(format!("tool-window-{}", tw.title()))
                .resizable(true)
                .default_width(240.0)
                .show(ctx, |ui| panel(app, ui, tw));
        }
        Anchor::Bottom => {
            egui::TopBottomPanel::bottom(format!("tool-window-{}", tw.title()))
                .resizable(true)
                .default_height(190.0)
                .show(ctx, |ui| panel(app, ui, tw));
        }
    }
}

fn panel(app: &mut PhpIdeApp, ui: &mut egui::Ui, tw: ToolWindow) {
    tool_window_header(app, ui, tw);
    ui.separator();
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
        ui.label(egui::RichText::new("Nenhum bookmark. Use F11 no editor.").weak());
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
                "Listening on port 9003"
            } else {
                "Stopped"
            }
            .to_string(),
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
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("Git: {}", app.git_branch())).strong());
        if ui.button("Refresh").on_hover_text("Refresh").clicked() {
            app.git_refresh();
        }
    });
    ui.label(app.ws.root.display().to_string());
    ui.separator();
    ui.label("Log · Branches · Incoming/Outgoing");
    ui.label(
        egui::RichText::new(
            "O histórico e as operações de merge/rebase pertencem ao incremento Git do roadmap.",
        )
        .weak(),
    );
}

fn commit(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    ui.label("Commit das mudanças já adicionadas ao índice do Git:");
    ui.text_edit_singleline(&mut app.commit_message);
    if ui
        .add_enabled(
            !app.commit_message.trim().is_empty(),
            egui::Button::new("Commit"),
        )
        .clicked()
    {
        let message = app.commit_message.trim().to_string();
        app.exec.run(
            "git commit".to_string(),
            "git".to_string(),
            vec!["commit".to_string(), "-m".to_string(), message],
            app.ws.root.clone(),
        );
        app.commit_message.clear();
    }
    if ui.button("Check Git Status").clicked() {
        app.exec.run(
            "git status".to_string(),
            "git".to_string(),
            vec!["status".to_string(), "--short".to_string()],
            app.ws.root.clone(),
        );
    }
    output(app, ui);
}

fn terminal(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.monospace(format!("{} $", app.ws.root.display()));
        let response = egui::TextEdit::singleline(&mut app.terminal_input)
            .hint_text("comando do shell")
            .desired_width(420.0)
            .show(ui)
            .response;
        let submit = response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        if (submit || ui.button("Run").clicked()) && !app.terminal_input.trim().is_empty() {
            let command = app.terminal_input.trim().to_string();
            app.exec.run(
                format!("$ {command}"),
                "sh".to_string(),
                vec!["-lc".to_string(), command],
                app.ws.root.clone(),
            );
            app.terminal_input.clear();
        }
    });
    output(app, ui);
}

fn output(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    if app.exec.running {
        ui.spinner();
    }
    egui::ScrollArea::vertical()
        .stick_to_bottom(true)
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(&app.exec.output).monospace())
                    .selectable(true),
            );
        });
}

fn database(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Data Sources and Drivers").strong());
        if ui.button("Database Settings").clicked() {
            app.settings_dialog.open(SettingsPage::Named("Database"));
        }
    });
    ui.separator();
    ui.label("Nenhum data source configurado.");
    ui.label(
        egui::RichText::new(
            "O console SQL, introspection e a grade de dados serão implementados no módulo RF-DB.",
        )
        .weak(),
    );
}

fn notifications(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("Event Log").strong());
    ui.label(if app.ws.message.is_empty() {
        "Nenhuma notificação nova."
    } else {
        &app.ws.message
    });
    ui.separator();
    ui.label(format!("Language Server: {}", app.lsp_label()));
    ui.label(format!("Diagnostics: {}", app.ws.diagnostics.len()));
}

fn remote_host(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("Deployment Servers").strong());
    ui.label(format!("Default: {}", app.settings.deployment_server));
    if ui.button("Configure Deployment...").clicked() {
        app.settings_dialog.open(SettingsPage::Named("Deployment"));
    }
    ui.label(egui::RichText::new("FTP · FTPS · SFTP · WebDAV · local/mounted").weak());
}
