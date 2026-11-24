//! Editor tabs (paridade REQUISITOS 1.4): preview tab em itálico, pin,
//! context menu (Close / Close Others / Pin), Closing Policy de 10 abas no
//! core. Navegação por atalhos fica no `handle_shortcuts` de main.

use eframe::egui;

use phpide_icons::file_icon;

use crate::design::components::Tab;
use crate::PhpIdeApp;

pub fn tab_strip(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let palette = crate::design::current(ui.ctx()).palette;
    egui::Panel::top("tabs")
        .frame(egui::Frame::new().fill(palette.bg_canvas))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                let count = app.ws.buffers.len();
                let active = app.ws.active;
                let mut focus: Option<usize> = None;
                let mut close: Option<usize> = None;
                let mut close_others: Option<usize> = None;
                let mut pin_toggle: Option<usize> = None;

                for i in 0..count {
                    let (name, dirty, preview, pinned) = {
                        let buf = &app.ws.buffers[i];
                        (buf.name(), buf.dirty, buf.preview, buf.pinned)
                    };
                    let (icon, kind) = file_icon(&name);
                    let shown = Tab::new(name)
                        .icon(icon, palette.file_color(kind))
                        .active(i == active)
                        .modified(dirty)
                        .preview(preview)
                        .pinned(pinned)
                        .show(ui);
                    if shown.close_clicked {
                        close = Some(i);
                    } else if shown.response.clicked() {
                        focus = Some(i);
                    }
                    shown.response.context_menu(|ui| {
                        if ui.button("Close\tCtrl+F4").clicked() {
                            close = Some(i);
                            ui.close();
                        }
                        if ui.button("Close Others").clicked() {
                            close_others = Some(i);
                            ui.close();
                        }
                        if ui
                            .button(if pinned { "Unpin Tab" } else { "Pin Tab" })
                            .clicked()
                        {
                            pin_toggle = Some(i);
                            ui.close();
                        }
                    });
                }

                if let Some(i) = focus {
                    app.ws.active = i;
                }
                if let Some(i) = close {
                    app.request_close_tab(i);
                }
                if let Some(i) = close_others {
                    app.request_close_others(i);
                }
                if let Some(i) = pin_toggle {
                    if let Some(buf) = app.ws.buffers.get_mut(i) {
                        buf.pinned = !buf.pinned;
                    }
                }
            });
        });
}
