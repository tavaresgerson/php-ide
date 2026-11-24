//! Button and IconButton.

use eframe::egui::{self, Color32, CornerRadius, Response, Sense, Stroke, StrokeKind};
use phpide_icons::IconId;

use super::{body_font, focus_ring, galley, paint_icon, state_of, theme, tooltip, State};
use crate::design::tokens::{radius, space};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ButtonKind {
    /// The main action of a dialog or page (accent fill).
    Primary,
    /// Regular action with a quiet fill and subtle border.
    Secondary,
    /// No fill until hovered; for toolbars and dense rows.
    Ghost,
    /// Destructive action.
    Danger,
}

impl ButtonKind {
    pub const ALL: [ButtonKind; 4] = [
        ButtonKind::Primary,
        ButtonKind::Secondary,
        ButtonKind::Ghost,
        ButtonKind::Danger,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ButtonKind::Primary => "Primary",
            ButtonKind::Secondary => "Secondary",
            ButtonKind::Ghost => "Ghost",
            ButtonKind::Danger => "Danger",
        }
    }
}

pub struct Button {
    label: String,
    kind: ButtonKind,
    icon: Option<IconId>,
    enabled: bool,
    min_width: f32,
    forced: Option<State>,
}

impl Button {
    pub fn new(label: impl Into<String>) -> Self {
        Button {
            label: label.into(),
            kind: ButtonKind::Secondary,
            icon: None,
            enabled: true,
            min_width: 0.0,
            forced: None,
        }
    }

    pub fn primary(label: impl Into<String>) -> Self {
        Button::new(label).kind(ButtonKind::Primary)
    }

    pub fn kind(mut self, kind: ButtonKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn icon(mut self, icon: IconId) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn min_width(mut self, width: f32) -> Self {
        self.min_width = width;
        self
    }

    pub fn force(mut self, state: State) -> Self {
        self.forced = Some(state);
        self
    }

    pub fn show(self, ui: &mut egui::Ui) -> Response {
        let theme = theme(ui);
        let p = &theme.palette;
        let m = &theme.metrics;
        let enabled = self.enabled && self.forced != Some(State::Disabled);
        // Colored at paint time: the state is only known after allocation.
        let text = galley(ui, self.label.clone(), body_font(), Color32::PLACEHOLDER);
        let icon_size = m.icon;
        let icon_space = if self.icon.is_some() {
            icon_size + space::S3
        } else {
            0.0
        };
        let width = (text.size().x + icon_space + 2.0 * space::S5).max(self.min_width);
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, m.control), sense);
        let label = self.label.clone();
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &label));
        let state = state_of(&response, enabled, self.forced);
        if ui.is_rect_visible(rect) {
            let (fill, stroke, fg) = colors(self.kind, state, p);
            let corner = CornerRadius::same(radius::CONTROL);
            ui.painter()
                .rect(rect, corner, fill, stroke, StrokeKind::Inside);
            let content_width = text.size().x + icon_space;
            let mut x = rect.center().x - content_width / 2.0;
            if let Some(icon) = self.icon {
                let icon_rect = egui::Rect::from_center_size(
                    egui::pos2(x + icon_size / 2.0, rect.center().y),
                    egui::vec2(icon_size, icon_size),
                );
                paint_icon(ui.painter(), icon_rect, icon, fg);
                x += icon_space;
            }
            ui.painter().galley(
                egui::pos2(x, rect.center().y - text.size().y / 2.0),
                text,
                fg,
            );
            if state == State::Focused {
                focus_ring(ui.painter(), rect, radius::CONTROL, p);
            }
        }
        response
    }
}

fn colors(
    kind: ButtonKind,
    state: State,
    p: &crate::design::tokens::Palette,
) -> (Color32, Stroke, Color32) {
    let none = Stroke::NONE;
    if state == State::Disabled {
        let fill = match kind {
            ButtonKind::Ghost => Color32::TRANSPARENT,
            _ => p.bg_hover,
        };
        return (fill, none, p.text_disabled);
    }
    let hovered = matches!(state, State::Hovered);
    let pressed = matches!(state, State::Pressed);
    match kind {
        ButtonKind::Primary => {
            let fill = if hovered || pressed {
                p.accent_fill_hover
            } else {
                p.accent_fill
            };
            (fill, none, p.text_on_accent)
        }
        ButtonKind::Secondary => {
            let fill = if pressed {
                p.bg_pressed
            } else if hovered {
                p.bg_hover
            } else {
                p.bg_elevated
            };
            let border = if hovered || pressed {
                p.border_strong
            } else {
                p.border_subtle
            };
            (fill, Stroke::new(1.0, border), p.text_primary)
        }
        ButtonKind::Ghost => {
            let fill = if pressed {
                p.bg_pressed
            } else if hovered {
                p.bg_hover
            } else {
                Color32::TRANSPARENT
            };
            (fill, none, p.text_primary)
        }
        ButtonKind::Danger => {
            let fill = if pressed {
                p.bg_pressed
            } else if hovered {
                p.bg_hover
            } else {
                p.bg_elevated
            };
            (fill, Stroke::new(1.0, p.error), p.error)
        }
    }
}

/// Square icon-only button with a tooltip. Ghost by default; `selected`
/// shows the toggled state used by tool window rails.
pub struct IconButton {
    icon: IconId,
    tooltip: String,
    shortcut: Option<String>,
    selected: bool,
    enabled: bool,
    tint: Option<Color32>,
    size: Option<f32>,
    forced: Option<State>,
}

impl IconButton {
    pub fn new(icon: IconId, tooltip: impl Into<String>) -> Self {
        IconButton {
            icon,
            tooltip: tooltip.into(),
            shortcut: None,
            selected: false,
            enabled: true,
            tint: None,
            size: None,
            forced: None,
        }
    }

    pub fn shortcut(mut self, shortcut: impl Into<String>) -> Self {
        let shortcut = shortcut.into();
        self.shortcut = (!shortcut.is_empty()).then_some(shortcut);
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn tint(mut self, color: Color32) -> Self {
        self.tint = Some(color);
        self
    }

    /// Overrides the density's icon button size.
    pub fn size(mut self, size: f32) -> Self {
        self.size = Some(size);
        self
    }

    pub fn force(mut self, state: State) -> Self {
        self.forced = Some(state);
        self
    }

    pub fn show(self, ui: &mut egui::Ui) -> Response {
        let theme = theme(ui);
        let p = &theme.palette;
        let side = self.size.unwrap_or(theme.metrics.icon_button);
        let enabled = self.enabled && self.forced != Some(State::Disabled);
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), sense);
        let label = self.tooltip.clone();
        let selected = self.selected;
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, selected, &label)
        });
        let state = state_of(&response, enabled, self.forced);
        if ui.is_rect_visible(rect) {
            let fill = match state {
                State::Pressed => p.bg_pressed,
                State::Hovered if self.selected => p.bg_selected_focused,
                State::Hovered => p.bg_hover,
                _ if self.selected => p.bg_selected,
                _ => Color32::TRANSPARENT,
            };
            ui.painter()
                .rect_filled(rect, CornerRadius::same(radius::CONTROL), fill);
            let color = match state {
                State::Disabled => p.text_disabled,
                _ if self.selected => p.text_primary,
                _ => self.tint.unwrap_or(p.text_secondary),
            };
            let icon_side = theme.metrics.icon.min(side - 4.0);
            let icon_rect =
                egui::Rect::from_center_size(rect.center(), egui::vec2(icon_side, icon_side));
            paint_icon(ui.painter(), icon_rect, self.icon, color);
            if state == State::Focused {
                focus_ring(ui.painter(), rect, radius::CONTROL, p);
            }
        }
        tooltip(response, &self.tooltip, self.shortcut.as_deref())
    }
}

/// Header widget (New UI): icon, label and a dropdown chevron, quiet until
/// hovered. Used by the Project, VCS and Run widgets of the window header.
pub struct ToolbarWidget {
    icon: Option<IconId>,
    icon_tint: Option<Color32>,
    text: String,
    detail: Option<String>,
    chevron: bool,
    tooltip: Option<String>,
    forced: Option<State>,
}

impl ToolbarWidget {
    pub fn new(text: impl Into<String>) -> Self {
        ToolbarWidget {
            icon: None,
            icon_tint: None,
            text: text.into(),
            detail: None,
            chevron: true,
            tooltip: None,
            forced: None,
        }
    }

    pub fn icon(mut self, icon: IconId) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn icon_tint(mut self, color: Color32) -> Self {
        self.icon_tint = Some(color);
        self
    }

    /// Secondary text after the label (for example the run target).
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn chevron(mut self, chevron: bool) -> Self {
        self.chevron = chevron;
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
        let p = &theme.palette;
        let height = theme.metrics.icon_button;
        let font = super::label_font();
        let text = galley(ui, self.text.clone(), font, p.text_primary);
        let detail = self
            .detail
            .as_ref()
            .map(|d| galley(ui, d.clone(), body_font(), p.text_secondary));
        let icon_side = theme.metrics.icon;
        let mut width = 2.0 * space::S3 + text.size().x;
        if self.icon.is_some() {
            width += icon_side + space::S2;
        }
        if let Some(detail) = &detail {
            width += space::S2 + detail.size().x;
        }
        if self.chevron {
            width += space::S1 + 12.0;
        }
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), Sense::click());
        let label = match &self.detail {
            Some(detail) => format!("{} {detail}", self.text),
            None => self.text.clone(),
        };
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        let state = state_of(&response, true, self.forced);
        if ui.is_rect_visible(rect) {
            let fill = match state {
                State::Pressed => p.bg_pressed,
                State::Hovered => p.bg_hover,
                _ => Color32::TRANSPARENT,
            };
            ui.painter()
                .rect_filled(rect, CornerRadius::same(radius::CONTROL), fill);
            let mut x = rect.left() + space::S3;
            let cy = rect.center().y;
            if let Some(icon) = self.icon {
                let icon_rect = egui::Rect::from_center_size(
                    egui::pos2(x + icon_side / 2.0, cy),
                    egui::vec2(icon_side, icon_side),
                );
                paint_icon(
                    ui.painter(),
                    icon_rect,
                    icon,
                    self.icon_tint.unwrap_or(p.text_secondary),
                );
                x += icon_side + space::S2;
            }
            let text_width = text.size().x;
            ui.painter().galley(
                egui::pos2(x, cy - text.size().y / 2.0),
                text,
                p.text_primary,
            );
            x += text_width;
            if let Some(detail) = detail {
                x += space::S2;
                let detail_width = detail.size().x;
                ui.painter().galley(
                    egui::pos2(x, cy - detail.size().y / 2.0),
                    detail,
                    p.text_secondary,
                );
                x += detail_width;
            }
            if self.chevron {
                let chevron_rect = egui::Rect::from_center_size(
                    egui::pos2(x + space::S1 + 6.0, cy),
                    egui::vec2(12.0, 12.0),
                );
                paint_icon(
                    ui.painter(),
                    chevron_rect,
                    IconId::ChevronDown,
                    p.text_secondary,
                );
            }
            if state == State::Focused {
                focus_ring(ui.painter(), rect, radius::CONTROL, p);
            }
        }
        match self.tooltip {
            Some(tip) => tooltip(response, &tip, None),
            None => response,
        }
    }
}
