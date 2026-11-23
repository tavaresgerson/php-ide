//! Pintura egui dos mesmos SVGs versionados em `phpide-icons`, sem uma
//! segunda definição manual dos traços.

use eframe::egui;
use phpide_icons::{prims, IconId, Prim};

pub fn paint_icon(painter: &egui::Painter, rect: egui::Rect, id: IconId, color: egui::Color32) {
    let stroke = egui::Stroke::new((rect.width() / 24.0 * 1.8).clamp(1.0, 2.5), color);
    let map = |x: f32, y: f32| {
        egui::pos2(
            rect.left() + x / 24.0 * rect.width(),
            rect.top() + y / 24.0 * rect.height(),
        )
    };
    for prim in prims(id) {
        match prim {
            Prim::Circle(cx, cy, radius) => {
                painter.circle_stroke(map(*cx, *cy), radius / 24.0 * rect.width(), stroke);
            }
            Prim::Line(x1, y1, x2, y2) => {
                painter.line_segment([map(*x1, *y1), map(*x2, *y2)], stroke);
            }
            Prim::Poly(points) => {
                if points.len() >= 2 {
                    painter.add(egui::Shape::line(
                        points.iter().map(|&(x, y)| map(x, y)).collect(),
                        stroke,
                    ));
                }
            }
            Prim::Rect(x, y, width, height) => {
                painter.rect_stroke(
                    egui::Rect::from_min_size(
                        map(*x, *y),
                        egui::vec2(width / 24.0 * rect.width(), height / 24.0 * rect.height()),
                    ),
                    egui::CornerRadius::ZERO,
                    stroke,
                    egui::StrokeKind::Inside,
                );
            }
            Prim::Dot(x, y) => {
                painter.circle_filled(map(*x, *y), stroke.width * 0.9, color);
            }
        }
    }
}

pub fn icon_button(
    ui: &mut egui::Ui,
    id: IconId,
    size: f32,
    tint: egui::Color32,
    tooltip: &str,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    let color = if response.hovered() {
        tint.gamma_multiply(1.25)
    } else {
        tint
    };
    paint_icon(ui.painter(), rect.shrink(size * 0.08), id, color);
    response.on_hover_text(format!("{tooltip} ({})", id.accessible_name()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_id_has_prims() {
        for id in IconId::catalog() {
            assert!(!prims(*id).is_empty(), "{} sem pintor", id.slug());
        }
        assert!(!prims(IconId::Unknown).is_empty(), "fallback pintável");
    }
}
