//! Status bar e Navigation Bar (REQUISITOS 1.1/1.3): eventos, Processes,
//! widgets do editor/PHP/VCS/debug/deployment e caminho do arquivo na New UI.

use eframe::egui;

use crate::settings_dialog::SettingsPage;
use crate::toolwindows::ToolWindow;
use crate::PhpIdeApp;

const PHP_LEVELS: [&str; 8] = ["5.3", "7.0", "7.4", "8.0", "8.1", "8.2", "8.3", "8.4"];
const INDENTS: [&str; 4] = ["2 spaces", "4 spaces", "8 spaces", "Tab"];

pub fn status_bar(app: &mut PhpIdeApp, ctx: &egui::Context) {
    let navigation_bottom = app.settings.navigation_bar == "Bottom";
    egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
        ui.horizontal(|ui| {
            let msg = app.ws.message.clone();
            if ui
                .button("Processes")
                .on_hover_text("Processes — Services (Alt+8)")
                .clicked()
            {
                app.tw.toggle(ToolWindow::Services);
            }
            let message = if msg.is_empty() {
                "Pronto".to_string()
            } else {
                msg
            };
            if ui.button(message).on_hover_text("Notifications").clicked() {
                app.tw.toggle(ToolWindow::Notifications);
            }
            // Widget de inspeções do arquivo ativo (UI-ED-006-03): abre o
            // Problems no escopo do arquivo.
            let insp = app.inspections.label();
            if !insp.is_empty()
                && ui
                    .button(insp)
                    .on_hover_text("Inspections — abrir Problems neste arquivo")
                    .clicked()
            {
                app.problems_scope = 1;
                app.tw.toggle(ToolWindow::Problems);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                memory_widget(app, ui);
                deployment_widget(app, ui);
                debug_listen_widget(app, ui);
                readonly_widget(app, ui);
                branch_widget(app, ui);
                indent_widget(app, ui);
                column_selection_widget(app, ui);
                encoding_widget(app, ui);
                line_ending_widget(app, ui);
                caret_widget(app, ui);
                php_level_widget(app, ui);
            });
        });
        if navigation_bottom {
            ui.separator();
            navigation_bar_contents(app, ui);
        }
    });
}

pub fn navigation_bar_top(app: &mut PhpIdeApp, ctx: &egui::Context) {
    if app.settings.navigation_bar != "Top" {
        return;
    }
    egui::TopBottomPanel::top("navigation-bar-top").show(ctx, |ui| {
        navigation_bar_contents(app, ui);
    });
}

fn navigation_bar_contents(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let root = app.ws.root.clone();
    let path = app
        .ws
        .active_buffer()
        .and_then(|buffer| buffer.path.clone());
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("Project:").weak());
        if ui
            .link(
                root.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| root.display().to_string()),
            )
            .clicked()
        {
            app.tw.set_visible(ToolWindow::Project, true);
        }
        if let Some(path) = path {
            let relative = path.strip_prefix(&root).unwrap_or(&path).to_path_buf();
            for component in relative.components() {
                ui.label(egui::RichText::new(">").weak());
                ui.label(component.as_os_str().to_string_lossy());
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new("Navigation Bar · Alt+Home")
                    .weak()
                    .small(),
            );
        });
    });
}

fn php_level_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let current = app.settings.php_level.clone();
    ui.menu_button(format!("PHP {current}"), |ui| {
        ui.set_min_width(120.0);
        for level in PHP_LEVELS {
            if ui
                .selectable_label(level == current, format!("PHP {level}"))
                .clicked()
            {
                app.settings.php_level = level.to_string();
                app.persist_settings();
                ui.close_menu();
            }
        }
        ui.separator();
        ui.label(
            egui::RichText::new("Sincronização com composer.json: módulo Composer")
                .weak()
                .small(),
        );
    })
    .response
    .on_hover_text("PHP Language Level");
}

fn caret_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let text = if let Some(buf) = app.ws.active_buffer() {
        let (line, col) = buf
            .document
            .line_col(app.cursor_char.min(buf.document.len_chars()));
        format!("{line}:{col}")
    } else {
        "—:—".to_string()
    };
    if ui
        .button(text)
        .on_hover_text("Line:Column — Go to Line")
        .clicked()
    {
        app.goto_line_open = true;
    }
}

fn line_ending_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let current = app.settings.line_ending.clone();
    if ui
        .button(&current)
        .on_hover_text("Line endings — alternar LF/CRLF")
        .clicked()
    {
        app.settings.line_ending = if current == "LF" { "CRLF" } else { "LF" }.to_string();
        app.persist_settings();
    }
}

fn encoding_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    if ui.button("UTF-8").on_hover_text("Encoding").clicked() {
        app.ws.message = "Encoding do buffer: UTF-8".to_string();
    }
}

fn indent_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let current = app.settings.indent.clone();
    ui.menu_button(current.clone(), |ui| {
        for option in INDENTS {
            if ui.selectable_label(option == current, option).clicked() {
                app.settings.indent = option.to_string();
                app.persist_settings();
                ui.close_menu();
            }
        }
    })
    .response
    .on_hover_text("Indent style");
}

fn branch_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    if !crate::vcs::is_repo(&app.ws.root) {
        return;
    }
    let branch = app.git_branch();
    if ui
        .button(format!("Git {branch}"))
        .on_hover_text("VCS branch — Version Control (Alt+9)")
        .clicked()
    {
        app.git_refresh();
        app.tw.toggle(ToolWindow::VersionControl);
    }
}

fn readonly_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let Some(buf) = app.ws.active_buffer() else {
        return;
    };
    let readonly = buf.readonly;
    let label = if readonly { "RO" } else { "RW" };
    if ui
        .button(label)
        .on_hover_text("Read-only / Read-write")
        .clicked()
    {
        if let Some(buf) = app.ws.active_buffer_mut() {
            buf.readonly = !buf.readonly;
        }
    }
}

fn column_selection_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let selected = app.settings.column_selection;
    if ui
        .selectable_label(selected, "Column")
        .on_hover_text("Column Selection Mode (Alt+Shift+Insert)")
        .clicked()
    {
        app.settings.column_selection = !selected;
        app.persist_settings();
    }
}

fn debug_listen_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let on = app.debug_listen;
    let label = if on { "Listen on" } else { "Listen off" };
    if ui
        .button(egui::RichText::new(label).color(if on {
            egui::Color32::LIGHT_GREEN
        } else {
            ui.style().visuals.text_color()
        }))
        .on_hover_text("Listen for PHP Debug Connections")
        .clicked()
    {
        app.debug_listen = !on;
        app.ws.message = if app.debug_listen {
            "escuta Xdebug ativada"
        } else {
            "escuta Xdebug desativada"
        }
        .to_string();
    }
}

fn deployment_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let server = app.settings.deployment_server.clone();
    ui.menu_button(format!("Deploy: {server}"), |ui| {
        if ui.selectable_label(server == "None", "None").clicked() {
            app.settings.deployment_server = "None".to_string();
            app.persist_settings();
            ui.close_menu();
        }
        if ui.button("Deployment...").clicked() {
            app.settings_dialog.open(SettingsPage::Named("Deployment"));
            ui.close_menu();
        }
    })
    .response
    .on_hover_text("Default deployment server");
}

fn memory_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    if ui
        .button(format!("{} MB", app.rss_mb()))
        .on_hover_text("Memory indicator — RSS")
        .clicked()
    {
        app.tw.set_visible(ToolWindow::Services, true);
    }
}
