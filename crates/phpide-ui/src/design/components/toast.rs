//! Toast notifications: short messages in the bottom-right corner that
//! expire on their own (errors stay until dismissed).

use eframe::egui::{self, CornerRadius, Stroke};
use phpide_icons::IconId;

use super::{body_font, paint_icon, small_font, theme, IconButton, Tone};
use crate::design::tokens::{radius, space};

const LIFETIME_SECONDS: f64 = 6.0;
const WIDTH: f32 = 340.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub tone: Tone,
    pub title: String,
    pub body: Option<String>,
    /// `ctx` time when it was shown; set by [`Toasts::push`].
    created: f64,
}

impl Toast {
    pub fn new(tone: Tone, title: impl Into<String>) -> Self {
        Toast {
            tone,
            title: title.into(),
            body: None,
            created: f64::NAN,
        }
    }

    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    fn expires(&self) -> bool {
        self.tone != Tone::Error
    }
}

#[derive(Debug, Default)]
pub struct Toasts {
    items: Vec<Toast>,
}

impl Toasts {
    pub fn push(&mut self, mut toast: Toast) {
        toast.created = f64::NAN;
        self.items.push(toast);
        if self.items.len() > 4 {
            self.items.remove(0);
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if self.items.is_empty() {
            return;
        }
        let now = ctx.input(|i| i.time);
        for toast in &mut self.items {
            if toast.created.is_nan() {
                toast.created = now;
            }
        }
        self.items
            .retain(|t| !t.expires() || now - t.created < LIFETIME_SECONDS);
        if self.items.iter().any(Toast::expires) {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        let mut dismissed = None;
        egui::Area::new(egui::Id::new("phpide-toasts"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-space::S6, -40.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = space::S4;
                for (index, toast) in self.items.iter().enumerate() {
                    if toast_card(ui, toast) {
                        dismissed = Some(index);
                    }
                }
            });
        if let Some(index) = dismissed {
            self.items.remove(index);
        }
    }
}

/// Draws one toast; returns true when its close button was clicked.
pub fn toast_card(ui: &mut egui::Ui, toast: &Toast) -> bool {
    let theme = theme(ui);
    let p = theme.palette;
    let mut closed = false;
    egui::Frame::new()
        .fill(p.bg_elevated)
        .stroke(Stroke::new(1.0, p.border_subtle))
        .corner_radius(CornerRadius::same(radius::CARD))
        .inner_margin(egui::Margin::same(space::S5 as i8))
        .shadow(ui.visuals().popup_shadow)
        .show(ui, |ui| {
            ui.set_width(WIDTH);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = space::S4;
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(16.0, 18.0), egui::Sense::hover());
                paint_icon(
                    ui.painter(),
                    egui::Rect::from_min_size(
                        rect.min + egui::vec2(0.0, 1.0),
                        egui::vec2(16.0, 16.0),
                    ),
                    toast.tone.icon(),
                    toast.tone.color(&p),
                );
                ui.vertical(|ui| {
                    ui.set_width(WIDTH - 16.0 - 2.0 * space::S4 - theme.metrics.icon_button);
                    ui.spacing_mut().item_spacing.y = space::S1;
                    ui.label(
                        egui::RichText::new(&toast.title)
                            .font(body_font())
                            .color(p.text_primary),
                    );
                    if let Some(body) = &toast.body {
                        ui.label(
                            egui::RichText::new(body)
                                .font(small_font())
                                .color(p.text_secondary),
                        );
                    }
                });
                if IconButton::new(IconId::Close, "Dismiss").show(ui).clicked() {
                    closed = true;
                }
            });
        });
    closed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toasts_expire_except_errors() {
        let ctx = egui::Context::default();
        let mut toasts = Toasts::default();
        toasts.push(Toast::new(Tone::Success, "Saved"));
        toasts.push(Toast::new(Tone::Error, "Build failed"));
        let mut input = egui::RawInput {
            time: Some(0.0),
            ..Default::default()
        };
        ctx.run_ui(input.clone(), |ui| toasts.show(ui.ctx()))
            .drop_without_applying_deltas();
        assert_eq!(toasts.len(), 2);
        input.time = Some(LIFETIME_SECONDS + 1.0);
        ctx.run_ui(input, |ui| toasts.show(ui.ctx()))
            .drop_without_applying_deltas();
        assert_eq!(toasts.len(), 1);
        assert_eq!(toasts.items[0].tone, Tone::Error);
    }
}
