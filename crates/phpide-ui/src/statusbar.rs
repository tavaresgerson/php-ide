//! Status bar e Navigation Bar (REQUISITOS 1.1/1.3): eventos, Processes,
//! widgets do editor/PHP/VCS/debug/deployment e caminho do arquivo na New UI.

use eframe::egui;

use crate::design::components::{
    menu_item, menu_radio_item, menu_separator, menu_submenu, StatusItem, Tone,
};
use crate::settings_dialog::SettingsPage;
use crate::toolwindows::ToolWindow;
use crate::PhpIdeApp;
use phpide_icons::IconId;

const PHP_LEVELS: [&str; 8] = ["5.3", "7.0", "7.4", "8.0", "8.1", "8.2", "8.3", "8.4"];
const INDENTS: [&str; 4] = ["2 spaces", "4 spaces", "8 spaces", "Tab"];

pub fn status_bar(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let navigation_bottom = app.settings.navigation_bar == "Bottom";
    egui::Panel::bottom("status").show(ui, |ui| {
        crate::design::components::ghost_controls(ui);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if app.files.indexing() {
                crate::design::components::Spinner::show(ui, 14.0);
                StatusItem::new("Indexing…").show(ui);
            }
            let msg = app.ws.message.clone();
            let message = if msg.is_empty() {
                "Ready".to_string()
            } else {
                msg
            };
            let tone = match crate::notifications::classify(&message) {
                Tone::Info => Tone::Neutral,
                tone => tone,
            };
            let icon = if tone == Tone::Neutral {
                IconId::Notifications
            } else {
                tone.icon()
            };
            if StatusItem::new(message)
                .icon(icon)
                .max_width((ui.available_width() * 0.45).max(120.0))
                .tone(tone)
                .tooltip("Event Log — Notifications")
                .show(ui)
                .clicked()
            {
                app.tw.toggle(ToolWindow::Notifications);
            }
            // Widget de inspeções do arquivo ativo (UI-ED-006-03): abre o
            // Problems no escopo do arquivo.
            let insp = app.inspections.label();
            if !insp.is_empty()
                && StatusItem::new(insp)
                    .icon(IconId::Problems)
                    .tooltip("Inspections — open Problems for this file")
                    .show(ui)
                    .clicked()
            {
                app.problems_scope = 1;
                app.tw.toggle(ToolWindow::Problems);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                memory_widget(app, ui);
                language_server_widget(app, ui);
                if crate::scope::EXPERIMENTAL {
                    deployment_widget(app, ui);
                }
                debug_listen_widget(app, ui);
                readonly_widget(app, ui);
                branch_widget(app, ui);
                indent_widget(app, ui);
                encoding_widget(app, ui);
                line_ending_widget(app, ui);
                caret_widget(app, ui);
                file_type_widget(app, ui);
                php_level_widget(app, ui);
            });
        });
        if navigation_bottom {
            ui.separator();
            navigation_bar_contents(app, ui);
        }
    });
}

pub fn navigation_bar_top(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    if app.settings.navigation_bar != "Top" {
        return;
    }
    egui::Panel::top("navigation-bar-top").show(ui, |ui| {
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
            // Each segment lists its folder: pick a file to open (RF-SEL-11).
            let relative = path.strip_prefix(&root).unwrap_or(&path).to_path_buf();
            let mut dir = root.clone();
            for component in relative.components() {
                ui.label(egui::RichText::new("›").weak());
                let name = component.as_os_str().to_string_lossy().into_owned();
                let parent = dir.clone();
                dir = dir.join(&name);
                let is_last = dir == path;
                let response = ui.add(
                    egui::Button::new(egui::RichText::new(&name).color(if is_last {
                        ui.visuals().text_color()
                    } else {
                        ui.visuals().weak_text_color()
                    }))
                    .frame(false),
                );
                egui::Popup::menu(&response).show(|ui| {
                    folder_menu(app, ui, &parent, &dir);
                });
            }
        }
    });
}

/// File type of the active file, with a menu to force another one
/// (RF-HL-04) or associate the extension with it (RF-HL-02).
fn file_type_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let Some(buffer) = app.ws.active_buffer() else {
        return;
    };
    let Some(path) = buffer.path.clone() else {
        return;
    };
    let detected = app.editors.detect(buffer);
    let overridden = app.file_type_override(&path).is_some();
    let current = detected.name().to_string();
    let response = StatusItem::new(current.clone())
        .tooltip("File type: highlighting and language support")
        .show(ui);
    egui::Popup::menu(&response).show(|ui| {
        ui.set_min_width(180.0);
        ui.label(egui::RichText::new("File type of this file").weak().small());
        if menu_radio_item(ui, "Auto-detect", !overridden, true).clicked() {
            app.set_file_type_override(&path, None);
            ui.close();
        }
        menu_separator(ui);
        egui::ScrollArea::vertical()
            .max_height(320.0)
            .show(ui, |ui| {
                for file_type in phpide_php::filetype::FILE_TYPES {
                    let selected = overridden && current == file_type.name;
                    if menu_radio_item(ui, file_type.name, selected, true).clicked() {
                        app.set_file_type_override(&path, Some(file_type.id));
                        ui.close();
                    }
                }
            });
        if let Some(pattern) = crate::app_editor::extension_pattern(&path) {
            if let phpide_php::filetype::Detected::Known(file_type) = &detected {
                menu_separator(ui);
                let label = format!("Use {} for all {pattern} files", file_type.name);
                if menu_item(ui, Some(IconId::FileConfig), &label, "", true).clicked() {
                    app.associate_file_type(&pattern, file_type.id);
                    app.set_file_type_override(&path, None);
                    ui.close();
                }
            }
        }
    });
}

/// Contents of `dir` in a navigation bar popup; folders open submenus.
fn folder_menu(
    app: &mut PhpIdeApp,
    ui: &mut egui::Ui,
    dir: &std::path::Path,
    current: &std::path::Path,
) {
    ui.set_min_width(220.0);
    let ctx = ui.ctx().clone();
    let Some(mut entries) = app.files.children(&app.jobs, &ctx, dir) else {
        ui.spinner();
        return;
    };
    let options = crate::project_tree::tree_options(app);
    entries.retain(|e| options.show_excluded || !e.excluded);
    crate::services::files::sort_entries(&mut entries, &options);
    egui::ScrollArea::vertical()
        .max_height(420.0)
        .show(ui, |ui| {
            for entry in entries {
                let (icon, color) = if entry.is_dir {
                    (
                        phpide_icons::IconId::Folder,
                        crate::design::current(&ctx).palette.folder,
                    )
                } else {
                    let (icon, kind) = phpide_icons::file_icon(&entry.name);
                    (icon, crate::design::current(&ctx).palette.file_color(kind))
                };
                if entry.is_dir {
                    let label = format!("{}/", entry.name);
                    menu_submenu(ui, Some(IconId::Folder), &label, true, |ui| {
                        folder_menu(app, ui, &entry.path, current);
                    });
                } else {
                    let row = crate::design::components::ListRow::new(entry.name.clone())
                        .icon(icon, color)
                        .selected(entry.path == current)
                        .show(ui);
                    if row.clicked() {
                        app.open_file_tab(&entry.path, false);
                        app.editors.request_focus();
                        ui.close();
                    }
                }
            }
        });
}

fn php_level_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let current = app.settings.php_level.clone();
    let response = StatusItem::new(format!("PHP {current}"))
        .tooltip("PHP Language Level")
        .show(ui);
    egui::Popup::menu(&response).show(|ui| {
        ui.set_min_width(120.0);
        for level in PHP_LEVELS {
            if menu_radio_item(ui, &format!("PHP {level}"), level == current, true).clicked() {
                app.settings.php_level = level.to_string();
                app.persist_settings();
                ui.close();
            }
        }
        menu_separator(ui);
        ui.label(
            egui::RichText::new("Detected from composer.json when available")
                .weak()
                .small(),
        );
    });
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
    if StatusItem::new(text)
        .tooltip("Line:Column — Go to Line (Ctrl+G)")
        .show(ui)
        .clicked()
    {
        app.goto_line_open = true;
    }
}

/// The active file's line separator; choosing another converts the file
/// (RF: one undo step, the file becomes modified).
fn line_ending_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    use phpide_core::LineEnding;
    let Some(current) = app.ws.active_buffer().map(|b| b.line_ending) else {
        return;
    };
    let response = StatusItem::new(current.label())
        .tooltip("Line Separator")
        .show(ui);
    egui::Popup::menu(&response).show(|ui| {
        ui.set_min_width(220.0);
        for (ending, label) in [
            (LineEnding::Lf, "LF - Unix and macOS (\\n)"),
            (LineEnding::CrLf, "CRLF - Windows (\\r\\n)"),
        ] {
            if menu_radio_item(ui, label, current == ending, true).clicked() {
                if let Some(buffer) = app.ws.active_buffer_mut() {
                    if buffer.readonly {
                        app.ws.message =
                            "the file is read-only (unlock it in the status bar)".into();
                    } else {
                        buffer.set_line_ending(ending);
                    }
                }
                ui.close();
            }
        }
    });
}

fn encoding_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    if StatusItem::new("UTF-8")
        .tooltip("File encoding")
        .show(ui)
        .clicked()
    {
        app.ws.message = "File encoding: UTF-8".to_string();
    }
}

fn indent_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let current = app.settings.indent.clone();
    let response = StatusItem::new(current.clone())
        .tooltip("Indent style")
        .show(ui);
    egui::Popup::menu(&response).show(|ui| {
        ui.set_min_width(160.0);
        for option in INDENTS {
            if menu_radio_item(ui, option, option == current, true).clicked() {
                app.settings.indent = option.to_string();
                app.persist_settings();
                ui.close();
            }
        }
    });
}

fn branch_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    if !app.git.is_repo() {
        return;
    }
    let branch = app.git_branch();
    if StatusItem::new(branch)
        .icon(IconId::GitBranch)
        .tooltip("Git branch — Version Control (Alt+9)")
        .show(ui)
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
    let item = if readonly {
        StatusItem::new("")
            .icon(IconId::FileLock)
            .tone(Tone::Warning)
            .tooltip("Read-only — click to make writable")
    } else {
        StatusItem::new("")
            .icon(IconId::Edit)
            .tooltip("Writable — click to make read-only")
    };
    if item.show(ui).clicked() {
        if let Some(buf) = app.ws.active_buffer_mut() {
            buf.readonly = !buf.readonly;
        }
    }
}

fn debug_listen_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let on = app.debug_listen;
    let item = StatusItem::new("")
        .icon(IconId::Debug)
        .tone(if on { Tone::Success } else { Tone::Neutral })
        .tooltip(if on {
            "Listening for PHP debug connections — click to stop"
        } else {
            "Listen for PHP debug connections"
        });
    if item.show(ui).clicked() {
        app.debug_listen = !on;
        app.ws.message = if app.debug_listen {
            "listening for Xdebug connections"
        } else {
            "stopped listening for Xdebug connections"
        }
        .to_string();
    }
}

fn deployment_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let server = app.settings.deployment_server.clone();
    let response = StatusItem::new(format!("Deploy: {server}"))
        .icon(IconId::RemoteHost)
        .tooltip("Default deployment server")
        .show(ui);
    egui::Popup::menu(&response).show(|ui| {
        if menu_radio_item(ui, "None", server == "None", true).clicked() {
            app.settings.deployment_server = "None".to_string();
            app.persist_settings();
            ui.close();
        }
        if menu_item(ui, Some(IconId::Settings), "Deployment…", "", true).clicked() {
            app.settings_dialog.open(SettingsPage::Named("Deployment"));
            ui.close();
        }
    });
}

/// State of the PHP language server, with Restart and the server log.
fn language_server_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    use phpide_lsp::LspStatus;
    let status = app.lsp.as_ref().map(|l| l.status());
    let progress = app.lsp.as_ref().and_then(|l| l.progress().last().cloned());
    let (text, icon, tone, tip) = match &status {
        None => (
            "No PHP server".to_string(),
            IconId::Info,
            Tone::Neutral,
            app.lsp_start_error
                .clone()
                .unwrap_or_else(|| "No language server is running".to_string()),
        ),
        Some(LspStatus::Starting) => (
            app.lsp_name.clone(),
            IconId::Loader,
            Tone::Neutral,
            format!("{} is starting", app.lsp_name),
        ),
        Some(LspStatus::Running(_)) => match &progress {
            Some(work) => (
                work.text(),
                IconId::Loader,
                Tone::Neutral,
                if work.message.is_empty() {
                    format!("{}: {}", app.lsp_name, work.text())
                } else {
                    format!("{}: {} — {}", app.lsp_name, work.text(), work.message)
                },
            ),
            None => (
                app.lsp_name.clone(),
                IconId::Success,
                Tone::Success,
                format!("{} is running", app.lsp_name),
            ),
        },
        Some(LspStatus::Failed(reason)) if app.lsp_supervisor.waiting() => (
            format!("{} (restarting)", app.lsp_name),
            IconId::Loader,
            Tone::Warning,
            format!("{} stopped: {reason}; restarting it", app.lsp_name),
        ),
        Some(LspStatus::Failed(reason)) => (
            app.lsp_name.clone(),
            IconId::Error,
            Tone::Error,
            if app.lsp_supervisor.gave_up() {
                format!(
                    "{} stopped: {reason}; it crashed too often to restart by itself",
                    app.lsp_name
                )
            } else {
                format!("{} stopped: {reason}", app.lsp_name)
            },
        ),
    };
    let response = StatusItem::new(text)
        .icon(icon)
        .tone(tone)
        .tooltip(format!("Language server — {tip}"))
        .show(ui);
    egui::Popup::menu(&response).show(|ui| {
        ui.set_min_width(240.0);
        ui.label(egui::RichText::new(tip).weak().small());
        menu_separator(ui);
        if menu_item(
            ui,
            Some(IconId::Refresh),
            "Restart Language Server",
            "",
            true,
        )
        .clicked()
        {
            app.restart_lsp();
            ui.close();
        }
        if menu_item(ui, Some(IconId::FileText), "Language Server Log", "", true).clicked() {
            app.intel.log_open = true;
            ui.close();
        }
        if menu_item(
            ui,
            Some(IconId::Settings),
            "Language Server Settings…",
            "",
            true,
        )
        .clicked()
        {
            app.settings_dialog
                .open(crate::settings_dialog::SettingsPage::PhpLanguageServer);
            ui.close();
        }
    });
}

fn memory_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    StatusItem::new(format!("{} MB", app.rss_mb()))
        .tooltip("Memory used by PHP IDE (RSS)")
        .show(ui);
}
