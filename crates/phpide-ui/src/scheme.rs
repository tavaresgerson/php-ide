//! Quick Switch Scheme (REQUISITOS 1.8 — `` Ctrl+` ``): troca rápida de tema
//! de UI/color scheme (e futuramente keymap). Popup central estilo JetBrains.

use eframe::egui;

use crate::design::ThemeId;
use crate::PhpIdeApp;
use phpide_icons::IconId;

pub fn scheme_popup(app: &mut PhpIdeApp, ctx: &egui::Context) {
    if !app.scheme_open {
        return;
    }
    let mut close = false;
    egui::Area::new(egui::Id::new("quick-switch-scheme"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            crate::design::components::menu_frame(ui).show(ui, |ui| {
                use crate::design::components::{menu_item, menu_radio_item, menu_separator};
                ui.set_min_width(260.0);
                ui.label(egui::RichText::new("Quick Switch Scheme").strong().small());
                menu_separator(ui);
                let current = ThemeId::from_name(&app.settings.theme);
                for theme in ThemeId::ALL {
                    if menu_radio_item(ui, theme.label(), theme == current, true).clicked() {
                        app.apply_theme(theme, ctx);
                        close = true;
                    }
                }
                menu_separator(ui);
                let keymap = format!("Keymap: {}", app.settings.keymap);
                if menu_item(ui, Some(IconId::Keyboard), &keymap, "", true).clicked() {
                    app.settings_dialog
                        .open(crate::settings_dialog::SettingsPage::Keymap);
                    close = true;
                }
                if menu_item(ui, Some(IconId::Palette), "Color Scheme…", "", true).clicked() {
                    app.settings_dialog
                        .open(crate::settings_dialog::SettingsPage::ColorScheme);
                    close = true;
                }
            });
        });
    ctx.input(|i| {
        if i.key_pressed(egui::Key::Escape) {
            close = true;
        }
    });
    if close {
        app.scheme_open = false;
    }
}
