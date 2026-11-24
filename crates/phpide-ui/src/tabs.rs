//! Editor tabs (RF-TAB-01..19, 23).
//!
//! One row that scrolls sideways, with a list of every tab at the end
//! for the ones out of view. Preview tabs are italic and become permanent
//! on edit or double click; double click on a permanent tab maximizes the
//! editor. The context menu closes (this, others, all, unpinned), pins,
//! reopens closed tabs, copies the path, reveals the file and splits.

use std::collections::HashMap;
use std::path::Path;

use eframe::egui;
use phpide_icons::{file_icon, IconId};

use crate::design::components::{menu_item, menu_separator, IconButton, Tab};
use crate::editor_area::SplitAxis;
use crate::PhpIdeApp;

enum TabAction {
    Focus(usize),
    Close(usize),
    CloseOthers(usize),
    CloseAll { keep_pinned: bool },
    TogglePin(usize),
    Promote(usize),
    Maximize,
    Reopen,
    CopyPath(usize, bool),
    CopyUrl(usize),
    Reveal(usize),
    Split(usize, SplitAxis),
}

/// Tab labels: the file name, with the parent folder in front when two
/// open files share a name (RF-TAB-08).
pub fn tab_labels(buffers: &[phpide_core::Buffer]) -> Vec<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for buffer in buffers {
        *counts.entry(buffer.name()).or_default() += 1;
    }
    buffers
        .iter()
        .map(|buffer| {
            let name = buffer.name();
            let parent = buffer
                .path
                .as_deref()
                .and_then(Path::parent)
                .and_then(Path::file_name)
                .map(|p| p.to_string_lossy().into_owned());
            match parent {
                Some(parent) if counts.get(&name).copied().unwrap_or(0) > 1 => {
                    format!("{parent}/{name}")
                }
                _ => name,
            }
        })
        .collect()
}

pub fn tab_strip(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let palette = crate::design::current(ui.ctx()).palette;
    let mut actions = Vec::new();
    egui::Panel::top("tabs")
        .frame(egui::Frame::new().fill(palette.bg_canvas))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                let list_width = crate::design::current(ui.ctx()).metrics.icon_button + 8.0;
                let labels = tab_labels(&app.ws.buffers);
                let active = app.ws.active;
                let active_id = app.ws.active_buffer().map(|b| b.id);
                let memory_key = egui::Id::new("tab-strip-active");
                let last_active: Option<u64> = ui.data(|d| d.get_temp(memory_key));
                let reveal = last_active != active_id;
                ui.data_mut(|d| d.insert_temp(memory_key, active_id));
                if app.settings.tabs_multiple_rows {
                    // Several rows: every tab stays visible (RF-TAB-06).
                    ui.allocate_ui_with_layout(
                        egui::vec2((ui.available_width() - list_width).max(0.0), 0.0),
                        egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true),
                        |ui| {
                            for (i, label) in labels.iter().enumerate() {
                                tab(app, ui, i, label, false, &mut actions);
                            }
                        },
                    );
                } else {
                    egui::ScrollArea::horizontal()
                        .id_salt("tab-scroll")
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .max_width((ui.available_width() - list_width).max(0.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (i, label) in labels.iter().enumerate() {
                                    tab(app, ui, i, label, i == active && reveal, &mut actions);
                                }
                            });
                        });
                }
                tab_list(app, ui, &labels, &mut actions);
            });
        });
    for action in actions {
        run(app, ui.ctx(), action);
    }
}

fn tab(
    app: &PhpIdeApp,
    ui: &mut egui::Ui,
    i: usize,
    label: &str,
    reveal: bool,
    actions: &mut Vec<TabAction>,
) {
    let palette = crate::design::current(ui.ctx()).palette;
    let buffer = &app.ws.buffers[i];
    let (icon, kind) = file_icon(&buffer.name());
    let shown = Tab::new(label)
        .icon(icon, palette.file_color(kind))
        .active(i == app.ws.active)
        .modified(buffer.dirty)
        .preview(buffer.preview)
        .pinned(buffer.pinned)
        .show(ui);
    if reveal {
        shown.response.scroll_to_me(None);
    }
    let response = shown.response.on_hover_text(
        buffer
            .path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "Not saved yet".to_string()),
    );
    let alt = ui.input(|i| i.modifiers.alt);
    if shown.close_clicked {
        // Alt+click on the close button closes the other tabs (RF-TAB-14).
        actions.push(if alt {
            TabAction::CloseOthers(i)
        } else {
            TabAction::Close(i)
        });
    } else if response.double_clicked() {
        actions.push(if buffer.preview {
            TabAction::Promote(i)
        } else {
            TabAction::Maximize
        });
    } else if response.clicked() {
        actions.push(TabAction::Focus(i));
    }
    let pinned = buffer.pinned;
    let has_path = buffer.path.is_some();
    response.context_menu(|ui| {
        ui.set_min_width(260.0);
        let mut item = |ui: &mut egui::Ui, label: &str, shortcut: &str, enabled: bool, action| {
            if menu_item(ui, None, label, shortcut, enabled).clicked() {
                actions.push(action);
                ui.close();
            }
        };
        item(ui, "Close", "Ctrl+F4", true, TabAction::Close(i));
        item(ui, "Close Other Tabs", "", true, TabAction::CloseOthers(i));
        item(
            ui,
            "Close All Tabs",
            "",
            true,
            TabAction::CloseAll { keep_pinned: false },
        );
        item(
            ui,
            "Close Unpinned Tabs",
            "",
            true,
            TabAction::CloseAll { keep_pinned: true },
        );
        menu_separator(ui);
        item(
            ui,
            if pinned { "Unpin Tab" } else { "Pin Tab" },
            "",
            true,
            TabAction::TogglePin(i),
        );
        item(
            ui,
            "Reopen Closed Tab",
            "Ctrl+Shift+T",
            !app.ws.closed_tabs.is_empty(),
            TabAction::Reopen,
        );
        menu_separator(ui);
        item(ui, "Copy Path", "", has_path, TabAction::CopyPath(i, true));
        item(
            ui,
            "Copy File Name",
            "",
            has_path,
            TabAction::CopyPath(i, false),
        );
        item(
            ui,
            "Copy Repository URL",
            "",
            has_path && app.git.remote().is_some(),
            TabAction::CopyUrl(i),
        );
        item(
            ui,
            "Select in Project View",
            "Alt+F1",
            has_path,
            TabAction::Reveal(i),
        );
        menu_separator(ui);
        item(
            ui,
            "Split Right",
            "",
            true,
            TabAction::Split(i, SplitAxis::Right),
        );
        item(
            ui,
            "Split Down",
            "",
            true,
            TabAction::Split(i, SplitAxis::Down),
        );
    });
}

/// Every open tab, for the ones scrolled out of view (RF-TAB-06).
fn tab_list(app: &PhpIdeApp, ui: &mut egui::Ui, labels: &[String], actions: &mut Vec<TabAction>) {
    if app.ws.buffers.is_empty() {
        return;
    }
    let palette = crate::design::current(ui.ctx()).palette;
    ui.add_space(4.0);
    let button = IconButton::new(IconId::ChevronDown, "Show All Tabs").show(ui);
    egui::Popup::menu(&button).show(|ui| {
        ui.set_min_width(260.0);
        egui::ScrollArea::vertical()
            .max_height(420.0)
            .show(ui, |ui| {
                for (i, label) in labels.iter().enumerate() {
                    let buffer = &app.ws.buffers[i];
                    let (icon, kind) = file_icon(&buffer.name());
                    let text = if buffer.dirty {
                        format!("*{label}")
                    } else {
                        label.clone()
                    };
                    let row = crate::design::components::ListRow::new(text)
                        .icon(icon, palette.file_color(kind))
                        .selected(i == app.ws.active)
                        .show(ui);
                    if row.clicked() {
                        actions.push(TabAction::Focus(i));
                        ui.close();
                    }
                }
            });
    });
}

fn run(app: &mut PhpIdeApp, ctx: &egui::Context, action: TabAction) {
    match action {
        TabAction::Focus(i) => {
            if i < app.ws.buffers.len() {
                app.ws.active = i;
            }
        }
        TabAction::Close(i) => app.request_close_tab(i),
        TabAction::CloseOthers(i) => app.request_close_others(i),
        TabAction::CloseAll { keep_pinned } => app.request_close_all(keep_pinned),
        TabAction::TogglePin(i) => {
            if let Some(buffer) = app.ws.buffers.get_mut(i) {
                buffer.pinned = !buffer.pinned;
                buffer.preview = false;
            }
            if app.settings.tabs_pinned_left {
                app.ws.keep_pinned_first();
            }
        }
        TabAction::Promote(i) => {
            if let Some(buffer) = app.ws.buffers.get_mut(i) {
                buffer.preview = false;
            }
            app.ws.active = i;
        }
        TabAction::Maximize => app.tw.toggle_hide_all(),
        TabAction::Reopen => {
            if !app.ws.reopen_closed_tab() {
                app.ws.message = "No closed tab to reopen".into();
            }
        }
        TabAction::CopyPath(i, full) => {
            if let Some(path) = app.ws.buffers.get(i).and_then(|b| b.path.clone()) {
                let text = if full {
                    path.display().to_string()
                } else {
                    path.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                };
                app.ws.message = format!("Copied {text}");
                ctx.copy_text(text);
            }
        }
        TabAction::CopyUrl(i) => {
            let url = app
                .ws
                .buffers
                .get(i)
                .and_then(|b| b.path.clone())
                .and_then(|p| app.git.remote().and_then(|r| r.file_url(&p, None)));
            if let Some(url) = url {
                app.ws.message = format!("Copied {url}");
                ctx.copy_text(url);
            }
        }
        TabAction::Reveal(i) => {
            app.ws.active = i;
            app.tw
                .set_visible(crate::toolwindows::ToolWindow::Project, true);
            crate::project_tree::follow_active_file(app, ctx, true);
        }
        TabAction::Split(i, axis) => {
            app.ws.active = i;
            app.editors.split(&app.ws, axis);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_names_show_their_folder() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::create_dir_all(root.join("b")).unwrap();
        std::fs::write(root.join("a/index.php"), "").unwrap();
        std::fs::write(root.join("b/index.php"), "").unwrap();
        std::fs::write(root.join("c.php"), "").unwrap();
        let mut ws = phpide_core::Workspace::unscanned(root.to_path_buf());
        ws.open_file_ex(&root.join("a/index.php"), false);
        ws.open_file_ex(&root.join("b/index.php"), false);
        ws.open_file_ex(&root.join("c.php"), false);
        assert_eq!(
            tab_labels(&ws.buffers),
            vec!["a/index.php", "b/index.php", "c.php"]
        );
    }
}
