//! Panels, splitters, status items, dialogs, empty states, progress and
//! tooltips.

use eframe::egui::{self, CornerRadius, Response, Sense, Stroke};
use phpide_icons::IconId;

use super::{
    body_font, galley, label_font, paint_icon, small_font, theme, title_font, Button, IconButton,
    State, Tone,
};
use crate::design::tokens::{radius, space};

/// Frame of a tool window body.
pub fn panel_frame(ui: &egui::Ui) -> egui::Frame {
    let theme = theme(ui);
    egui::Frame::new()
        .fill(theme.palette.bg_panel)
        .inner_margin(egui::Margin::same(theme.metrics.panel_padding as i8))
}

/// Tool window header: title on the left, actions on the right, subtle
/// bottom divider.
pub struct PanelHeader {
    title: String,
}

impl PanelHeader {
    pub fn new(title: impl Into<String>) -> Self {
        PanelHeader {
            title: title.into(),
        }
    }

    pub fn show<R>(self, ui: &mut egui::Ui, actions: impl FnOnce(&mut egui::Ui) -> R) -> R {
        let theme = theme(ui);
        let p = theme.palette;
        let height = theme.metrics.tool_window_header;
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
        let title = galley(ui, self.title, label_font(), p.text_secondary);
        ui.painter().galley(
            egui::pos2(
                rect.left() + space::S4,
                rect.center().y - title.size().y / 2.0,
            ),
            title,
            p.text_secondary,
        );
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(1.0, p.border_subtle),
        );
        let actions_rect = rect.shrink2(egui::vec2(space::S2, 0.0));
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(actions_rect)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = space::S1;
                actions(ui)
            },
        )
        .inner
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitAxis {
    /// A vertical line between left and right areas (drag along x).
    Vertical,
    /// A horizontal line between top and bottom areas (drag along y).
    Horizontal,
}

/// Splitter handle: a one-pixel line with a 6-point hit area and a resize
/// cursor. Use `response.drag_delta()` to resize.
pub fn splitter(
    ui: &mut egui::Ui,
    axis: SplitAxis,
    length: f32,
    forced: Option<State>,
) -> Response {
    let p = theme(ui).palette;
    let hit = 6.0;
    let size = match axis {
        SplitAxis::Vertical => egui::vec2(hit, length),
        SplitAxis::Horizontal => egui::vec2(length, hit),
    };
    let (rect, response) = ui.allocate_exact_size(size, Sense::drag());
    let active = forced.map_or(response.dragged(), |s| s == State::Pressed);
    let hovered = forced.map_or(response.hovered(), |s| s == State::Hovered);
    if hovered || active {
        ui.ctx().set_cursor_icon(match axis {
            SplitAxis::Vertical => egui::CursorIcon::ResizeHorizontal,
            SplitAxis::Horizontal => egui::CursorIcon::ResizeVertical,
        });
    }
    let (color, width) = if active {
        (p.accent, 2.0)
    } else if hovered {
        (p.border_strong, 2.0)
    } else {
        (p.border_subtle, 1.0)
    };
    let stroke = Stroke::new(width, color);
    match axis {
        SplitAxis::Vertical => {
            ui.painter().vline(rect.center().x, rect.y_range(), stroke);
        }
        SplitAxis::Horizontal => {
            ui.painter().hline(rect.x_range(), rect.center().y, stroke);
        }
    }
    response
}

/// Quiet status bar entry: optional icon and text, hover fill only.
pub struct StatusItem {
    text: String,
    icon: Option<IconId>,
    tone: Tone,
    tooltip: Option<String>,
    max_width: Option<f32>,
    forced: Option<State>,
}

impl StatusItem {
    pub fn new(text: impl Into<String>) -> Self {
        StatusItem {
            text: text.into(),
            icon: None,
            tone: Tone::Neutral,
            tooltip: None,
            max_width: None,
            forced: None,
        }
    }

    pub fn icon(mut self, icon: IconId) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Truncates the text with an ellipsis beyond this width.
    pub fn max_width(mut self, width: f32) -> Self {
        self.max_width = Some(width);
        self
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn force(mut self, state: State) -> Self {
        self.forced = Some(state);
        self
    }

    pub fn show(self, ui: &mut egui::Ui) -> Response {
        let theme = theme(ui);
        let p = theme.palette;
        let m = theme.metrics;
        let text_color = p.text_secondary;
        let icon_color = self.tone.color(&p);
        let text = match self.max_width {
            Some(max) => {
                super::galley_truncated(ui, self.text.clone(), body_font(), text_color, max)
            }
            None => galley(ui, self.text.clone(), body_font(), text_color),
        };
        let icon_space = match (self.icon.is_some(), self.text.is_empty()) {
            (false, _) => 0.0,
            (true, true) => 14.0,
            (true, false) => 14.0 + space::S2,
        };
        let width = icon_space + text.size().x + 2.0 * space::S3;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, m.status_bar - 4.0), Sense::click());
        // Icon-only items are announced by their tooltip.
        let label = if self.text.is_empty() {
            self.tooltip.clone().unwrap_or_default()
        } else {
            self.text.clone()
        };
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        let state = super::state_of(&response, true, self.forced);
        if ui.is_rect_visible(rect) {
            if matches!(state, State::Hovered | State::Pressed) {
                let fill = if state == State::Pressed {
                    p.bg_pressed
                } else {
                    p.bg_hover
                };
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(radius::CONTROL), fill);
            }
            let mut x = rect.left() + space::S3;
            if let Some(icon) = self.icon {
                let icon_rect = egui::Rect::from_center_size(
                    egui::pos2(x + 7.0, rect.center().y),
                    egui::vec2(14.0, 14.0),
                );
                paint_icon(ui.painter(), icon_rect, icon, icon_color);
                x += icon_space;
            }
            ui.painter().galley(
                egui::pos2(x, rect.center().y - text.size().y / 2.0),
                text,
                text_color,
            );
        }
        match self.tooltip {
            Some(tip) => tooltip(response, &tip, None),
            None => response,
        }
    }
}

pub struct DialogResponse<R> {
    pub inner: R,
    /// The close button, Escape or a click outside closed the dialog.
    pub dismissed: bool,
}

/// Modal dialog with title, close button, body and right-aligned footer.
pub struct Dialog {
    id: egui::Id,
    title: String,
    width: f32,
}

impl Dialog {
    pub fn new(id_salt: impl std::hash::Hash + std::fmt::Debug, title: impl Into<String>) -> Self {
        Dialog {
            id: egui::Id::new(id_salt),
            title: title.into(),
            width: 440.0,
        }
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn frame(ui: &egui::Ui) -> egui::Frame {
        let theme = theme(ui);
        let p = theme.palette;
        egui::Frame::new()
            .fill(p.bg_elevated)
            .stroke(Stroke::new(1.0, p.border_subtle))
            .corner_radius(CornerRadius::same(radius::DIALOG))
            .inner_margin(egui::Margin::same(theme.metrics.dialog_padding as i8))
            .shadow(ui.visuals().window_shadow)
    }

    /// Shows the dialog as a modal over the whole window.
    pub fn show<R>(
        self,
        ctx: &egui::Context,
        body: impl FnOnce(&mut egui::Ui) -> R,
        footer: impl FnOnce(&mut egui::Ui),
    ) -> DialogResponse<R> {
        let theme = crate::design::current(ctx);
        let backdrop = theme.palette.shadow;
        let mut close = false;
        let modal = egui::Modal::new(self.id)
            .backdrop_color(backdrop)
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                Dialog::frame(ui)
                    .show(ui, |ui| self.contents(ui, &mut close, body, footer))
                    .inner
            });
        let dismissed = close || modal.should_close();
        DialogResponse {
            inner: modal.inner,
            dismissed,
        }
    }

    /// Draws the dialog inside `ui` (gallery and tests).
    pub fn show_inline<R>(
        self,
        ui: &mut egui::Ui,
        body: impl FnOnce(&mut egui::Ui) -> R,
        footer: impl FnOnce(&mut egui::Ui),
    ) -> DialogResponse<R> {
        let mut close = false;
        let inner = Dialog::frame(ui)
            .show(ui, |ui| self.contents(ui, &mut close, body, footer))
            .inner;
        DialogResponse {
            inner,
            dismissed: close,
        }
    }

    fn contents<R>(
        self,
        ui: &mut egui::Ui,
        close: &mut bool,
        body: impl FnOnce(&mut egui::Ui) -> R,
        footer: impl FnOnce(&mut egui::Ui),
    ) -> R {
        let p = theme(ui).palette;
        ui.set_width(self.width);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(&self.title)
                    .font(title_font())
                    .color(p.text_primary),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if IconButton::new(IconId::Close, "Close").show(ui).clicked() {
                    *close = true;
                }
            });
        });
        ui.add_space(space::S5);
        let inner = body(ui);
        ui.add_space(space::S7);
        let footer_size = egui::vec2(ui.available_width(), theme(ui).metrics.control);
        ui.allocate_ui_with_layout(
            footer_size,
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = space::S4;
                footer(ui);
            },
        );
        inner
    }
}

/// Centered placeholder for an empty view, with an optional action.
pub struct EmptyState {
    icon: IconId,
    title: String,
    description: Option<String>,
    action: Option<String>,
}

impl EmptyState {
    pub fn new(icon: IconId, title: impl Into<String>) -> Self {
        EmptyState {
            icon,
            title: title.into(),
            description: None,
            action: None,
        }
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn action(mut self, label: impl Into<String>) -> Self {
        self.action = Some(label.into());
        self
    }

    /// Returns true when the action button was clicked.
    pub fn show(self, ui: &mut egui::Ui) -> bool {
        let p = theme(ui).palette;
        let mut clicked = false;
        ui.vertical_centered(|ui| {
            ui.add_space(space::S7);
            super::icon(ui, self.icon, 32.0, p.text_muted);
            ui.add_space(space::S4);
            ui.label(
                egui::RichText::new(&self.title)
                    .font(body_font())
                    .color(p.text_primary),
            );
            if let Some(description) = &self.description {
                ui.add_space(space::S2);
                ui.label(
                    egui::RichText::new(description)
                        .font(body_font())
                        .color(p.text_muted),
                );
            }
            if let Some(action) = self.action {
                ui.add_space(space::S5);
                clicked = Button::new(action).show(ui).clicked();
            }
            ui.add_space(space::S7);
        });
        clicked
    }
}

/// Thin progress bar. `None` means indeterminate (animated unless
/// Reduced Motion is on).
pub struct ProgressBar {
    fraction: Option<f32>,
    label: Option<String>,
}

impl ProgressBar {
    pub fn new(fraction: Option<f32>) -> Self {
        ProgressBar {
            fraction: fraction.map(|f| f.clamp(0.0, 1.0)),
            label: None,
        }
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn show(self, ui: &mut egui::Ui, width: f32) -> Response {
        let theme = theme(ui);
        let p = theme.palette;
        let response = ui
            .vertical(|ui| {
                ui.spacing_mut().item_spacing.y = space::S2;
                if let Some(label) = &self.label {
                    ui.label(
                        egui::RichText::new(label)
                            .font(small_font())
                            .color(p.text_secondary),
                    );
                }
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(width, 4.0), Sense::hover());
                let corner = CornerRadius::same(2);
                ui.painter().rect_filled(rect, corner, p.bg_hover);
                let bar = match self.fraction {
                    Some(fraction) => egui::Rect::from_min_size(
                        rect.min,
                        egui::vec2(rect.width() * fraction, 4.0),
                    ),
                    None => {
                        let phase = if theme.reduced_motion {
                            0.35
                        } else {
                            ui.ctx().request_repaint();
                            (ui.input(|i| i.time) as f32 * 0.8).fract()
                        };
                        let segment = rect.width() * 0.3;
                        let x = rect.left() + (rect.width() + segment) * phase - segment;
                        egui::Rect::from_min_max(
                            egui::pos2(x.max(rect.left()), rect.top()),
                            egui::pos2((x + segment).min(rect.right()), rect.bottom()),
                        )
                    }
                };
                ui.painter().rect_filled(bar, corner, p.accent);
                response
            })
            .inner;
        let fraction = self.fraction;
        response.widget_info(|| {
            let mut info = egui::WidgetInfo::new(egui::WidgetType::ProgressIndicator);
            info.value = fraction.map(f64::from);
            info
        });
        response
    }
}

/// Spinner shown only while real work runs.
pub struct Spinner;

impl Spinner {
    pub fn show(ui: &mut egui::Ui, size: f32) -> Response {
        let theme = theme(ui);
        if theme.reduced_motion {
            return super::icon(ui, IconId::Loader, size, theme.palette.text_secondary);
        }
        ui.add(
            egui::Spinner::new()
                .size(size)
                .color(theme.palette.text_secondary),
        )
    }
}

/// Tooltip with an optional shortcut in muted text.
pub fn tooltip(response: Response, text: &str, shortcut: Option<&str>) -> Response {
    if text.is_empty() {
        return response;
    }
    response.on_hover_ui(|ui| tooltip_contents(ui, text, shortcut))
}

pub fn tooltip_contents(ui: &mut egui::Ui, text: &str, shortcut: Option<&str>) {
    let p = theme(ui).palette;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S4;
        ui.label(
            egui::RichText::new(text)
                .font(body_font())
                .color(p.text_primary),
        );
        if let Some(keys) = shortcut.filter(|k| !k.is_empty()) {
            ui.label(
                egui::RichText::new(keys)
                    .font(small_font())
                    .color(p.text_muted),
            );
        }
    });
}

/// Strip at the top of an editor pane (IntelliJ "editor notification"):
/// a tone icon, a message and link-style actions added by `actions`.
pub fn editor_notice(
    ui: &mut egui::Ui,
    tone: Tone,
    message: &str,
    actions: impl FnOnce(&mut egui::Ui),
) {
    let theme = theme(ui);
    let p = &theme.palette;
    egui::Frame::new()
        .fill(p.bg_panel)
        .stroke(Stroke::new(1.0, p.border_subtle))
        .inner_margin(egui::Margin::symmetric(space::S5 as i8, space::S3 as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(theme.metrics.icon, theme.metrics.icon),
                    Sense::hover(),
                );
                paint_icon(ui.painter(), rect, tone.icon(), tone.color(p));
                ui.add_space(space::S3);
                ui.label(
                    egui::RichText::new(message)
                        .font(body_font())
                        .color(p.text_primary),
                );
                ui.add_space(space::S5);
                ui.style_mut().visuals.hyperlink_color = p.accent;
                actions(ui);
            });
        });
}
