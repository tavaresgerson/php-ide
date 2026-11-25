//! Components of the design system. Every component reads colors and sizes
//! from the active [`Theme`](crate::design::Theme); none takes a color
//! literal. Builders accept `force(State)` so the gallery can show every
//! interaction state side by side.

mod buttons;
mod inputs;
mod navigation;
mod surfaces;
mod toast;

pub use buttons::{Button, ButtonKind, IconButton, ToolbarWidget};
pub use inputs::{Checkbox, Select, TextInput};
pub use navigation::{
    menu_frame, menu_item, menu_separator, menu_submenu, ListRow, Tab, TabResponse, TreeResponse,
    TreeRow,
};
pub use surfaces::{
    editor_notice, panel_frame, splitter, tooltip, tooltip_contents, Dialog, DialogResponse,
    EmptyState, PanelHeader, ProgressBar, Spinner, SplitAxis, StatusItem,
};
pub use toast::{toast_card, Toast, Toasts};

use eframe::egui::{self, epaint::PathShape, Color32, FontId, Rect, Response, Stroke};
use phpide_icons::{paths, IconId};

use super::fonts;
use super::theme::{current, Theme};
use super::tokens::{typography, Palette};

/// Interaction state used to pick a component's colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    Rest,
    Hovered,
    Pressed,
    Focused,
    Disabled,
}

impl State {
    pub const ALL: [State; 5] = [
        State::Rest,
        State::Hovered,
        State::Pressed,
        State::Focused,
        State::Disabled,
    ];

    pub fn label(self) -> &'static str {
        match self {
            State::Rest => "Rest",
            State::Hovered => "Hover",
            State::Pressed => "Pressed",
            State::Focused => "Focus",
            State::Disabled => "Disabled",
        }
    }
}

pub(crate) fn state_of(response: &Response, enabled: bool, forced: Option<State>) -> State {
    if let Some(state) = forced {
        return state;
    }
    if !enabled {
        State::Disabled
    } else if response.is_pointer_button_down_on() {
        State::Pressed
    } else if response.has_focus() {
        State::Focused
    } else if response.hovered() {
        State::Hovered
    } else {
        State::Rest
    }
}

/// Meaning of a status color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tone {
    Neutral,
    Accent,
    Success,
    Warning,
    Error,
    Info,
}

impl Tone {
    pub fn color(self, palette: &Palette) -> Color32 {
        match self {
            Tone::Neutral => palette.text_secondary,
            Tone::Accent => palette.accent,
            Tone::Success => palette.success,
            Tone::Warning => palette.warning,
            Tone::Error => palette.error,
            Tone::Info => palette.info,
        }
    }

    pub fn icon(self) -> IconId {
        match self {
            Tone::Neutral | Tone::Info | Tone::Accent => IconId::Info,
            Tone::Success => IconId::Success,
            Tone::Warning => IconId::Warning,
            Tone::Error => IconId::Error,
        }
    }
}

pub(crate) fn theme(ui: &egui::Ui) -> std::sync::Arc<Theme> {
    current(ui.ctx())
}

pub(crate) fn body_font() -> FontId {
    FontId::proportional(typography::BODY)
}

pub(crate) fn label_font() -> FontId {
    FontId::new(typography::LABEL, fonts::family(fonts::UI_MEDIUM))
}

pub(crate) fn small_font() -> FontId {
    FontId::proportional(typography::SMALL)
}

pub(crate) fn title_font() -> FontId {
    FontId::new(typography::TITLE, fonts::family(fonts::UI_SEMIBOLD))
}

/// Paints a Tabler icon inside `rect` with the stroke weight of the set
/// (2 units on the 24-unit grid).
pub fn paint_icon(painter: &egui::Painter, rect: Rect, id: IconId, color: Color32) {
    let scale = rect.width().min(rect.height()) / 24.0;
    let origin = rect.center() - egui::vec2(12.0, 12.0) * scale;
    let width = (2.0 * scale).max(1.0);
    let stroke = Stroke::new(width, color);
    for path in paths(id) {
        let points: Vec<egui::Pos2> = path
            .points
            .iter()
            .map(|p| origin + egui::vec2(p[0], p[1]) * scale)
            .collect();
        if path.is_dot() {
            painter.circle_filled(points[0], width * 0.75, color);
        } else if path.closed {
            painter.add(PathShape::closed_line(points, stroke));
        } else {
            painter.add(PathShape::line(points, stroke));
        }
    }
}

/// An icon as a non-interactive widget of `size` points.
pub fn icon(ui: &mut egui::Ui, id: IconId, size: f32, color: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    paint_icon(ui.painter(), rect, id, color);
    response
}

/// Makes plain egui buttons in `ui` quiet (no fill or border until
/// hovered), for toolbars and status bars built from stock widgets.
pub fn ghost_controls(ui: &mut egui::Ui) {
    let widgets = &mut ui.visuals_mut().widgets;
    widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    widgets.inactive.bg_stroke = Stroke::NONE;
    widgets.hovered.bg_stroke = Stroke::NONE;
    widgets.active.bg_stroke = Stroke::NONE;
}

/// Two-pixel focus ring drawn outside `rect`.
pub(crate) fn focus_ring(painter: &egui::Painter, rect: Rect, radius: u8, palette: &Palette) {
    painter.rect_stroke(
        rect.expand(1.0),
        egui::CornerRadius::same(radius.saturating_add(1)),
        Stroke::new(2.0, palette.focus_ring),
        egui::StrokeKind::Outside,
    );
}

pub(crate) fn galley(
    ui: &egui::Ui,
    text: impl Into<String>,
    font: FontId,
    color: Color32,
) -> std::sync::Arc<egui::Galley> {
    ui.painter().layout_no_wrap(text.into(), font, color)
}

/// Galley truncated with an ellipsis to `max_width`.
pub(crate) fn galley_truncated(
    ui: &egui::Ui,
    text: impl Into<String>,
    font: FontId,
    color: Color32,
    max_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.into(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_width.max(0.0));
    ui.ctx().fonts_mut(|fonts| fonts.layout_job(job))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_points_map_inside_their_rect() {
        let rect = Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(16.0, 16.0));
        // Geometry: every icon point maps inside the rect (+ stroke).
        for id in IconId::catalog() {
            for path in paths(*id) {
                for p in &path.points {
                    let point = rect.left_top() + egui::vec2(p[0], p[1]) * (16.0 / 24.0);
                    assert!(rect.expand(1.0).contains(point), "{}", id.slug());
                }
            }
        }
    }
}
