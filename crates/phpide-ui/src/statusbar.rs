//! Status bar (paridade REQUISITOS 1.3): mensagens à esquerda; à direita os
//! widgets — PHP Language Level, Ln:Col, line endings, encoding, indent,
//! branch VCS, Listen for PHP Debug Connections (☎), read-only e RSS.

use eframe::egui;

use crate::toolwindows::ToolWindow;
use crate::PhpIdeApp;

const PHP_LEVELS: [&str; 8] = ["5.3", "7.0", "7.4", "8.0", "8.1", "8.2", "8.3", "8.4"];
const INDENTS: [&str; 4] = ["2 spaces", "4 spaces", "8 spaces", "Tab"];

pub fn status_bar(app: &mut PhpIdeApp, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
        ui.horizontal(|ui| {
            // ---- Esquerda: mensagens de evento (clique → Notifications futuro).
            let msg = app.ws.message.clone();
            if !msg.is_empty() {
                ui.label(egui::RichText::new(msg).color(egui::Color32::LIGHT_YELLOW));
            } else {
                ui.label(egui::RichText::new("Pronto").weak());
            }

            // ---- Direita: widgets configuráveis (ordem espelhando a New UI).
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                memory_widget(app, ui);
                debug_listen_widget(app, ui);
                readonly_widget(app, ui);
                branch_widget(app, ui);
                indent_widget(app, ui);
                encoding_widget(app, ui);
                line_ending_widget(app, ui);
                caret_widget(app, ui);
                php_level_widget(app, ui);
            });
        });
    });
}

fn php_level_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let current = app.settings.php_level.clone();
    let label = format!("PHP {current}");
    ui.menu_button(label, |ui| {
        ui.set_min_width(120.0);
        for level in PHP_LEVELS {
            let sel = level == current;
            if ui.selectable_label(sel, format!("PHP {level}")).clicked() {
                app.settings.php_level = level.to_string();
                app.persist_settings();
                ui.close_menu();
            }
        }
        ui.separator();
        ui.label(
            egui::RichText::new("Sincronização com composer.json: Estágio 6 (REQUISITOS 2.7)")
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
        format!("Ln {line}, Col {col}")
    } else {
        "Ln —, Col —".to_string()
    };
    if ui.button(text).on_hover_text("Line:Column (clique para ir para a linha)").clicked() {
        app.goto_line_open = true;
    }
}

fn line_ending_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let le = app.settings.line_ending.clone();
    if ui
        .button(le.clone())
        .on_hover_text("Line endings (clique para alternar LF/CRLF)")
        .clicked()
    {
        app.settings.line_ending = if le == "LF" { "CRLF" } else { "LF" }.to_string();
        app.persist_settings();
    }
}

fn encoding_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    if ui
        .button("UTF-8")
        .on_hover_text("Encoding (UTF-8; outros encodings no Estágio 6)")
        .clicked()
    {
        app.ws.message = "Encoding fixo em UTF-8 neste estágio".to_string();
    }
}

fn indent_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let current = app.settings.indent.clone();
    ui.menu_button(current.clone(), |ui| {
        for opt in INDENTS {
            if ui.selectable_label(opt == current, opt).clicked() {
                app.settings.indent = opt.to_string();
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
        .button(format!("🌿 {branch}"))
        .on_hover_text("VCS branch (gerenciamento no Git tool window, Estágio 7)")
        .clicked()
    {
        app.git_refresh();
    }
}

fn readonly_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let Some(buf) = app.ws.active_buffer() else { return };
    let ro = buf.readonly;
    let label = if ro { "🔒" } else { "🔓" };
    if ui
        .button(label)
        .on_hover_text("Read-only / Read-write (clique para alternar)")
        .clicked()
    {
        if let Some(buf) = app.ws.active_buffer_mut() {
            buf.readonly = !buf.readonly;
        }
    }
}

fn debug_listen_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let on = app.debug_listen;
    let label = if on { "☎ on" } else { "☎ off" };
    if ui
        .button(egui::RichText::new(label).color(if on {
            egui::Color32::LIGHT_GREEN
        } else {
            ui.style().visuals.text_color()
        }))
        .on_hover_text("Listen for PHP Debug Connections (Xdebug)")
        .clicked()
    {
        app.debug_listen = !app.debug_listen;
        app.ws.message = if app.debug_listen {
            "escuta Xdebug ativada — Ctrl+D dispara o PHP com XDEBUG_CONFIG".to_string()
        } else {
            "escuta Xdebug desativada".to_string()
        };
    }
}

fn memory_widget(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let rss = app.rss_mb();
    if ui
        .button(format!("{rss} MB"))
        .on_hover_text("Memória RSS do processo da IDE")
        .clicked()
    {
        app.tw.set_visible(ToolWindow::Problems, true);
    }
}
