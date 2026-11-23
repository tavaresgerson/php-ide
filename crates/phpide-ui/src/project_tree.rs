//! Project tool window: the lazily loaded folder tree.
//!
//! Rows come from [`crate::services::files::ProjectFiles`]; only the visible
//! rows are laid out, so large expanded folders stay cheap to draw.

use eframe::egui;

use crate::services::files::TreeRow;
use crate::toolwindows::ToolWindow;
use crate::{tool_window_header, PhpIdeApp};

const INDENT: f32 = 14.0;

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
            ui.separator();
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
            let row_height = ui.spacing().interact_size.y;
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

/// Folder arrow: pointing right when collapsed, down when expanded. Files
/// get the same empty space so names line up.
fn disclosure_arrow(ui: &mut egui::Ui, expanded: Option<bool>) {
    let size = egui::vec2(12.0, ui.spacing().interact_size.y);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let Some(expanded) = expanded else {
        return;
    };
    let c = rect.center();
    let r = 3.5;
    let points = if expanded {
        vec![
            egui::pos2(c.x - r, c.y - r * 0.5),
            egui::pos2(c.x + r, c.y - r * 0.5),
            egui::pos2(c.x, c.y + r * 0.7),
        ]
    } else {
        vec![
            egui::pos2(c.x - r * 0.5, c.y - r),
            egui::pos2(c.x + r * 0.7, c.y),
            egui::pos2(c.x - r * 0.5, c.y + r),
        ]
    };
    let color = ui.visuals().weak_text_color();
    ui.painter().add(egui::Shape::convex_polygon(
        points,
        color,
        egui::Stroke::NONE,
    ));
}

fn draw_row(app: &PhpIdeApp, ui: &mut egui::Ui, row: &TreeRow) -> Option<Action> {
    let mut action = None;
    ui.horizontal(|ui| {
        ui.add_space(INDENT * row.depth as f32);
        disclosure_arrow(ui, row.entry.is_dir.then_some(row.expanded));
        let selected = app.selected.as_deref() == Some(row.entry.path.as_path());
        let response = ui.selectable_label(selected, row.entry.name.as_str());
        if row.loading {
            ui.spinner();
        }
        if let Some(error) = &row.error {
            ui.colored_label(ui.visuals().error_fg_color, "!")
                .on_hover_text(error);
        }
        if response.clicked() {
            action = Some(if row.entry.is_dir {
                Action::Toggle(row.entry.path.clone())
            } else {
                Action::Open(row.entry.path.clone())
            });
        }
    });
    action
}
