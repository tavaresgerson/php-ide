//! Editor tabs (paridade REQUISITOS 1.4): preview tab em itálico, pin,
//! context menu (Close / Close Others / Pin), Closing Policy de 10 abas no
//! core. Navegação por atalhos fica no `handle_shortcuts` de main.

use eframe::egui;

use crate::PhpIdeApp;

pub fn tab_strip(app: &mut PhpIdeApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
        ui.horizontal_wrapped(|ui| {
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
                let mut title = name.clone();
                if pinned {
                    title = format!("[P] {title}");
                }
                if dirty {
                    title.push_str(" *");
                }
                let text = if preview {
                    egui::RichText::new(title).italics()
                } else {
                    egui::RichText::new(title)
                };
                let selected = i == active;
                let resp = ui.selectable_label(selected, text);
                if resp.clicked() {
                    focus = Some(i);
                }
                resp.context_menu(|ui| {
                    if ui.button("Close\tCtrl+F4").clicked() {
                        close = Some(i);
                        ui.close_menu();
                    }
                    if ui.button("Close Others").clicked() {
                        close_others = Some(i);
                        ui.close_menu();
                    }
                    if ui
                        .button(if pinned { "Unpin Tab" } else { "Pin Tab" })
                        .clicked()
                    {
                        pin_toggle = Some(i);
                        ui.close_menu();
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
