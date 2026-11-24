//! Text input (and its search variant), Select and Checkbox.

use eframe::egui::{self, CornerRadius, Response, Sense, Stroke, StrokeKind};
use phpide_icons::IconId;

use super::{
    body_font, focus_ring, galley, galley_truncated, menu_item, paint_icon, state_of, theme,
    IconButton, State,
};
use crate::design::tokens::{radius, space};

pub struct TextInput<'a> {
    text: &'a mut String,
    hint: String,
    icon: Option<IconId>,
    clearable: bool,
    width: Option<f32>,
    id_salt: Option<egui::Id>,
    enabled: bool,
    invalid: bool,
    forced: Option<State>,
}

impl<'a> TextInput<'a> {
    pub fn new(text: &'a mut String) -> Self {
        TextInput {
            text,
            hint: String::new(),
            icon: None,
            clearable: false,
            width: None,
            id_salt: None,
            enabled: true,
            invalid: false,
            forced: None,
        }
    }

    /// Search field: magnifier icon and a clear button.
    pub fn search(text: &'a mut String) -> Self {
        let mut input = TextInput::new(text);
        input.icon = Some(IconId::Search);
        input.clearable = true;
        input.hint = "Search".into();
        input
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
        self
    }

    pub fn icon(mut self, icon: IconId) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn id_salt(mut self, salt: impl std::hash::Hash + std::fmt::Debug) -> Self {
        self.id_salt = Some(egui::Id::new(salt));
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Marks the value as invalid (error border).
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
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
        let enabled = self.enabled && self.forced != Some(State::Disabled);
        let width = self.width.unwrap_or_else(|| ui.available_width());
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, m.control), Sense::hover());
        let frame_index = ui.painter().add(egui::Shape::Noop);
        let mut inner = rect.shrink2(egui::vec2(space::S4, 0.0));
        if let Some(icon) = self.icon {
            let icon_rect = egui::Rect::from_center_size(
                egui::pos2(inner.left() + m.icon / 2.0, rect.center().y),
                egui::vec2(m.icon, m.icon),
            );
            paint_icon(ui.painter(), icon_rect, icon, p.text_muted);
            inner.min.x += m.icon + space::S3;
        }
        let show_clear = self.clearable && !self.text.is_empty() && enabled;
        if show_clear {
            inner.max.x -= m.icon_button - space::S2;
        }
        let mut edit = egui::TextEdit::singleline(self.text)
            .frame(egui::Frame::NONE)
            .hint_text(egui::RichText::new(self.hint.clone()).color(p.text_muted))
            .font(body_font())
            .text_color(if enabled {
                p.text_primary
            } else {
                p.text_disabled
            })
            .vertical_align(egui::Align::Center)
            .desired_width(inner.width())
            .interactive(enabled);
        if let Some(id) = self.id_salt {
            edit = edit.id_salt(id);
        }
        let mut response = ui.put(inner, edit);
        if show_clear {
            let clear_rect = egui::Rect::from_min_max(
                egui::pos2(inner.right(), rect.top()),
                egui::pos2(rect.right() - space::S1, rect.bottom()),
            );
            let clear = ui
                .scope_builder(egui::UiBuilder::new().max_rect(clear_rect), |ui| {
                    ui.centered_and_justified(|ui| {
                        IconButton::new(IconId::Close, "Clear")
                            .size(m.icon_button - space::S2)
                            .show(ui)
                    })
                    .inner
                })
                .inner;
            if clear.clicked() {
                self.text.clear();
                response.mark_changed();
            }
        }
        let state = state_of(&response, enabled, self.forced);
        let border = if self.invalid {
            p.error
        } else {
            match state {
                State::Focused => p.accent,
                State::Hovered | State::Pressed => p.border_strong,
                State::Disabled => p.border_subtle,
                State::Rest => p.border_strong,
            }
        };
        let fill = if state == State::Disabled {
            p.bg_panel
        } else {
            p.bg_inset
        };
        ui.painter().set(
            frame_index,
            egui::Shape::rect_filled(rect, CornerRadius::same(radius::CONTROL), fill),
        );
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(radius::CONTROL),
            Stroke::new(1.0, border),
            StrokeKind::Inside,
        );
        if state == State::Focused {
            focus_ring(ui.painter(), rect, radius::CONTROL, &p);
        }
        response
    }
}

pub struct Select<'a> {
    id_salt: egui::Id,
    selected: &'a mut usize,
    options: &'a [&'a str],
    width: Option<f32>,
    enabled: bool,
    forced: Option<State>,
}

impl<'a> Select<'a> {
    pub fn new(
        id_salt: impl std::hash::Hash + std::fmt::Debug,
        selected: &'a mut usize,
        options: &'a [&'a str],
    ) -> Self {
        Select {
            id_salt: egui::Id::new(id_salt),
            selected,
            options,
            width: None,
            enabled: true,
            forced: None,
        }
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
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
        let enabled = self.enabled && self.forced != Some(State::Disabled);
        let current = self.options.get(*self.selected).copied().unwrap_or("");
        let width = self.width.unwrap_or(180.0);
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, mut response) = ui.allocate_exact_size(egui::vec2(width, m.control), sense);
        let label = current.to_owned();
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, enabled, &label));
        let popup_id = self.id_salt.with("popup");
        let popup_open = egui::Popup::is_id_open(ui.ctx(), popup_id);
        let state = state_of(&response, enabled, self.forced);
        if ui.is_rect_visible(rect) {
            let fill = match state {
                State::Disabled => p.bg_panel,
                State::Pressed => p.bg_pressed,
                State::Hovered => p.bg_hover,
                _ => p.bg_elevated,
            };
            let border = if popup_open || matches!(state, State::Hovered | State::Pressed) {
                p.border_strong
            } else {
                p.border_subtle
            };
            ui.painter().rect(
                rect,
                CornerRadius::same(radius::CONTROL),
                fill,
                Stroke::new(1.0, border),
                StrokeKind::Inside,
            );
            let fg = if enabled {
                p.text_primary
            } else {
                p.text_disabled
            };
            let text_width = rect.width() - 2.0 * space::S4 - m.icon - space::S2;
            let text = galley_truncated(ui, current, body_font(), fg, text_width);
            ui.painter().galley(
                egui::pos2(
                    rect.left() + space::S4,
                    rect.center().y - text.size().y / 2.0,
                ),
                text,
                fg,
            );
            let icon_rect = egui::Rect::from_center_size(
                egui::pos2(rect.right() - space::S4 - m.icon / 2.0, rect.center().y),
                egui::vec2(m.icon, m.icon),
            );
            let icon_color = if enabled {
                p.text_secondary
            } else {
                p.text_disabled
            };
            paint_icon(ui.painter(), icon_rect, IconId::Selector, icon_color);
            if state == State::Focused {
                focus_ring(ui.painter(), rect, radius::CONTROL, &p);
            }
        }
        let mut changed = false;
        if enabled {
            egui::Popup::from_toggle_button_response(&response)
                .id(popup_id)
                .width(rect.width())
                .show(|ui| {
                    for (index, option) in self.options.iter().enumerate() {
                        let icon = (index == *self.selected).then_some(IconId::Check);
                        if menu_item(ui, icon, option, "", true).clicked() {
                            *self.selected = index;
                            changed = true;
                            ui.close();
                        }
                    }
                });
        }
        if changed {
            response.mark_changed();
        }
        response
    }
}

pub struct Checkbox<'a> {
    checked: &'a mut bool,
    label: String,
    enabled: bool,
    forced: Option<State>,
}

impl<'a> Checkbox<'a> {
    pub fn new(checked: &'a mut bool, label: impl Into<String>) -> Self {
        Checkbox {
            checked,
            label: label.into(),
            enabled: true,
            forced: None,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
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
        let enabled = self.enabled && self.forced != Some(State::Disabled);
        let fg = if enabled {
            p.text_primary
        } else {
            p.text_disabled
        };
        let text = galley(ui, self.label.clone(), body_font(), fg);
        let box_side = 16.0;
        let width = box_side + space::S3 + text.size().x;
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, mut response) = ui.allocate_exact_size(egui::vec2(width, m.min_target), sense);
        if response.clicked() {
            *self.checked = !*self.checked;
            response.mark_changed();
        }
        let (checked, label) = (*self.checked, self.label.clone());
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, checked, &label)
        });
        let state = state_of(&response, enabled, self.forced);
        if ui.is_rect_visible(rect) {
            let box_rect = egui::Rect::from_center_size(
                egui::pos2(rect.left() + box_side / 2.0, rect.center().y),
                egui::vec2(box_side, box_side),
            );
            let (fill, border) = match (checked, state) {
                (_, State::Disabled) => (p.bg_hover, p.border_subtle),
                (true, State::Hovered | State::Pressed) => {
                    (p.accent_fill_hover, p.accent_fill_hover)
                }
                (true, _) => (p.accent_fill, p.accent_fill),
                (false, State::Hovered | State::Pressed) => (p.bg_hover, p.text_secondary),
                (false, _) => (p.bg_inset, p.border_strong),
            };
            ui.painter().rect(
                box_rect,
                CornerRadius::same(3),
                fill,
                Stroke::new(1.0, border),
                StrokeKind::Inside,
            );
            if checked {
                let mark = if enabled {
                    p.text_on_accent
                } else {
                    p.text_disabled
                };
                paint_icon(ui.painter(), box_rect.shrink(1.0), IconId::Check, mark);
            }
            if state == State::Focused {
                focus_ring(ui.painter(), box_rect, 3, &p);
            }
            ui.painter().galley(
                egui::pos2(
                    box_rect.right() + space::S3,
                    rect.center().y - text.size().y / 2.0,
                ),
                text,
                fg,
            );
        }
        response
    }
}
