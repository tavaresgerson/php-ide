//! Project tool window: the lazily loaded folder tree.
//!
//! Rows come from [`crate::services::files::ProjectFiles`]; only the visible
//! rows are laid out, so large expanded folders stay cheap to draw.

use eframe::egui;

use phpide_icons::{file_icon, IconId};

use crate::design::components::TreeRow as RowWidget;
use crate::services::files::TreeRow;
use crate::toolwindows::ToolWindow;
use crate::{tool_window_header, PhpIdeApp};

enum Action {
    Toggle(std::path::PathBuf),
    Open(std::path::PathBuf),
}

pub fn show(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    egui::Panel::left("tree")
        .resizable(true)
        .default_size(240.0)
        .show(ui, |ui| {
            tool_window_header(app, ui, ToolWindow::Project);
            if let Some(error) = app.files.root_error() {
                ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                return;
            }
            if app.files.root_loading() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak("Loading…");
                });
                return;
            }
            let rows = app.files.visible_rows();
            if rows.is_empty() {
                ui.weak("This folder is empty.");
                return;
            }
            let row_height = crate::design::current(ui.ctx()).metrics.row;
            ui.spacing_mut().item_spacing.y = 0.0;
            let mut action = None;
            egui::ScrollArea::both().auto_shrink([false; 2]).show_rows(
                ui,
                row_height,
                rows.len(),
                |ui, range| {
                    for row in &rows[range] {
                        if let Some(clicked) = draw_row(app, ui, row) {
                            action = Some(clicked);
                        }
                    }
                },
            );
            match action {
                Some(Action::Toggle(dir)) => {
                    let ctx = ui.ctx().clone();
                    app.selected = Some(dir.clone());
                    app.files.toggle(&app.jobs, &ctx, &dir);
                }
                Some(Action::Open(path)) => {
                    app.selected = Some(path.clone());
                    app.ws.open_file(&path);
                }
                None => {}
            }
        });
}

fn draw_row(app: &PhpIdeApp, ui: &mut egui::Ui, row: &TreeRow) -> Option<Action> {
    let p = crate::design::current(ui.ctx()).palette;
    let entry = &row.entry;
    let (icon, color) = if entry.is_dir {
        let icon = if row.expanded {
            IconId::FolderOpen
        } else {
            IconId::Folder
        };
        (icon, p.folder)
    } else {
        let (icon, kind) = file_icon(&entry.name);
        (icon, p.file_color(kind))
    };
    let mut widget = RowWidget::new(entry.name.as_str())
        .depth(row.depth)
        .expanded(entry.is_dir.then_some(row.expanded))
        .icon(icon, color)
        .selected(app.selected.as_deref() == Some(entry.path.as_path()));
    if row.loading {
        widget = widget.detail("Loading…");
    }
    if let Some(error) = &row.error {
        widget = widget.detail(error.as_str()).label_color(p.error);
    }
    let shown = widget.show(ui);
    (shown.response.clicked() || shown.toggled).then(|| {
        if entry.is_dir {
            Action::Toggle(entry.path.clone())
        } else {
            Action::Open(entry.path.clone())
        }
    })
}
