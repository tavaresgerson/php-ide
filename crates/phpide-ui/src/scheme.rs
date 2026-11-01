//! Quick Switch Scheme (REQUISITOS 1.8 — `` Ctrl+` ``): troca rápida de tema
//! de UI/color scheme (e futuramente keymap). Popup central estilo JetBrains.

use eframe::egui;

use crate::themes::ThemeId;
use crate::PhpIdeApp;

pub fn scheme_popup(app: &mut PhpIdeApp, ctx: &egui::Context) {
    if !app.scheme_open {
        return;
    }
    let mut close = false;
    egui::Window::new("quick-switch-scheme")
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .movable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .frame(
            egui::Frame::window(&ctx.style())
                .shadow(egui::epaint::Shadow::default())
                .inner_margin(egui::Margin::same(10)),
        )
        .show(ctx, |ui| {
            ui.label(egui::RichText::new("Quick Switch Scheme").strong().small());
            ui.separator();
            let current = ThemeId::from_name(&app.settings.theme);
            for theme in ThemeId::ALL {
                let sel = theme == current;
                let text = format!("{}   {}", if sel { ">" } else { " " }, theme.label());
                if ui.selectable_label(sel, text).clicked() {
                    app.apply_theme(theme, ctx);
                    close = true;
                }
            }
            ui.separator();
            if ui
                .button(format!("   Keymap: {}", app.settings.keymap))
                .clicked()
            {
                app.settings_dialog
                    .open(crate::settings_dialog::SettingsPage::Keymap);
                close = true;
            }
            ui.label(egui::RichText::new(format!("   Color Scheme: {}", current.label())).weak());
            ctx.input(|i| {
                if i.key_pressed(egui::Key::Escape) {
                    close = true;
                }
            });
        });
    if close {
        app.scheme_open = false;
    }
}
