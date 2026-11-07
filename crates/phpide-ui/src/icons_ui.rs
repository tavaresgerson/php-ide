//! Pintura dos ícones semânticos em egui — `T-UI-ICON-001-05` (aplicar o
//! catálogo aos controles novos do editor: toolbar Run/Debug, gutter,
//! switcher, tool windows).
//!
//! egui não rasteriza SVG; cada ícone do catálogo tem aqui um pintor
//! vetorial que **espelha** o SVG canônico de `phpide-icons` (mesmos
//! traços no espaço 24×24, `currentColor` → cor do tema). O SVG continua
//! sendo a fonte de verdade para licença/manifesto e para qualquer
//! renderizador futuro; UT garante que todo `IconId` tem pintor.

use eframe::egui;
use phpide_icons::IconId;

/// Primitiva de desenho no espaço 24×24.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Prim {
    Circle(f32, f32, f32),
    /// Linha única.
    Line(f32, f32, f32, f32),
    /// Polilinha (traçado fechado ou aberto).
    Poly(&'static [(f32, f32)]),
    /// Retângulo (x, y, w, h).
    Rect(f32, f32, f32, f32),
    /// Ponto grosso (círculo preenchido pequeno, ex.: o "i" do info).
    Dot(f32, f32),
}

/// Primitivas de cada ícone (espelho do SVG homônimo).
pub fn prims(id: IconId) -> &'static [Prim] {
    use Prim::*;
    match id {
        IconId::Run => &[Poly(&[(7.0, 5.0), (19.0, 12.0), (7.0, 19.0)])],
        IconId::Debug => &[
            Circle(12.0, 12.0, 5.0),
            Line(12.0, 4.0, 12.0, 7.0),
            Line(9.5, 4.8, 8.0, 3.0),
            Line(14.5, 4.8, 16.0, 3.0),
            Line(7.0, 12.0, 4.5, 12.0),
            Line(19.5, 12.0, 17.0, 12.0),
            Line(8.5, 16.0, 7.0, 19.0),
            Line(15.5, 16.0, 17.0, 19.0),
        ],
        IconId::Stop => &[Rect(6.5, 6.5, 11.0, 11.0)],
        IconId::Pause => &[Line(9.0, 6.0, 9.0, 18.0), Line(15.0, 6.0, 15.0, 18.0)],
        IconId::Settings => &[
            Circle(12.0, 12.0, 3.2),
            Line(12.0, 2.8, 12.0, 5.4),
            Line(12.0, 18.6, 12.0, 21.2),
            Line(2.8, 12.0, 5.4, 12.0),
            Line(18.6, 12.0, 21.2, 12.0),
            Line(5.5, 5.5, 7.3, 7.3),
            Line(16.7, 16.7, 18.5, 18.5),
            Line(18.5, 5.5, 16.7, 7.3),
            Line(7.3, 16.7, 5.5, 18.5),
        ],
        IconId::Close => &[Line(6.0, 6.0, 18.0, 18.0), Line(18.0, 6.0, 6.0, 18.0)],
        IconId::Warning => &[
            Poly(&[(12.0, 4.0), (21.0, 20.0), (3.0, 20.0)]),
            Line(12.0, 10.0, 12.0, 14.0),
            Dot(12.0, 17.2),
        ],
        IconId::Error => &[
            Circle(12.0, 12.0, 8.0),
            Line(9.0, 9.0, 15.0, 15.0),
            Line(15.0, 9.0, 9.0, 15.0),
        ],
        IconId::Info => &[
            Circle(12.0, 12.0, 8.0),
            Line(12.0, 11.0, 12.0, 16.0),
            Dot(12.0, 8.0),
        ],
        IconId::Project => &[
            Poly(&[
                (3.0, 6.0),
                (9.0, 6.0),
                (11.0, 8.0),
                (21.0, 8.0),
                (21.0, 18.0),
                (3.0, 18.0),
            ]),
            Circle(16.5, 13.5, 1.6),
        ],
        IconId::Folder => &[Poly(&[
            (3.0, 6.0),
            (9.0, 6.0),
            (11.0, 8.0),
            (21.0, 8.0),
            (21.0, 18.0),
            (3.0, 18.0),
        ])],
        IconId::File => &[
            Poly(&[
                (6.0, 3.0),
                (14.0, 3.0),
                (18.0, 7.0),
                (18.0, 21.0),
                (6.0, 21.0),
            ]),
            Poly(&[(14.0, 3.0), (14.0, 7.0), (18.0, 7.0)]),
        ],
        IconId::GitBranch => &[
            Circle(7.0, 6.0, 2.5),
            Circle(7.0, 18.0, 2.5),
            Circle(17.0, 12.0, 2.5),
            Line(7.0, 8.5, 7.0, 15.5),
            Line(7.0, 12.0, 14.5, 12.0),
        ],
        IconId::Search => &[Circle(11.0, 11.0, 6.0), Line(15.5, 15.5, 20.0, 20.0)],
        IconId::Bookmark => &[Poly(&[
            (7.0, 4.0),
            (17.0, 4.0),
            (17.0, 20.0),
            (12.0, 16.0),
            (7.0, 20.0),
        ])],
        IconId::Breakpoint => &[Circle(12.0, 12.0, 6.0)],
        IconId::ChevronRight => &[Poly(&[(9.0, 6.0), (15.0, 12.0), (9.0, 18.0)])],
        IconId::ChevronDown => &[Poly(&[(6.0, 9.0), (12.0, 15.0), (18.0, 9.0)])],
        IconId::SplitRight => &[Rect(4.0, 5.0, 16.0, 14.0), Line(12.0, 5.0, 12.0, 19.0)],
        IconId::SplitDown => &[Rect(4.0, 5.0, 16.0, 14.0), Line(4.0, 12.0, 20.0, 12.0)],
        IconId::Switcher => &[Rect(3.5, 4.0, 13.0, 7.0), Rect(7.5, 13.0, 13.0, 7.0)],
        IconId::Eye => &[
            Poly(&[
                (3.0, 12.0),
                (6.0, 7.5),
                (18.0, 7.5),
                (21.0, 12.0),
                (18.0, 16.5),
                (6.0, 16.5),
            ]),
            Circle(12.0, 12.0, 2.5),
        ],
        IconId::Blame => &[
            Circle(12.0, 8.0, 3.5),
            Poly(&[(5.5, 20.0), (12.0, 14.5), (18.5, 20.0)]),
        ],
        IconId::Coverage => &[
            Poly(&[
                (12.0, 3.0),
                (20.0, 6.0),
                (20.0, 12.0),
                (18.0, 17.0),
                (12.0, 21.0),
                (6.0, 17.0),
                (4.0, 12.0),
                (4.0, 6.0),
            ]),
            Poly(&[(8.5, 12.0), (11.0, 14.5), (15.5, 9.5)]),
        ],
        IconId::Hint => &[
            Circle(12.0, 10.0, 6.0),
            Line(9.5, 15.2, 9.0, 18.0),
            Line(9.0, 18.0, 15.0, 18.0),
            Line(15.0, 18.0, 14.5, 15.2),
            Line(10.0, 21.0, 14.0, 21.0),
        ],
        IconId::Unknown => &[Circle(12.0, 12.0, 7.0), Circle(12.0, 12.0, 1.6)],
    }
}

/// Pinta o ícone centrado em `rect` com a cor (tint do tema).
pub fn paint_icon(painter: &egui::Painter, rect: egui::Rect, id: IconId, color: egui::Color32) {
    let list = prims(id);
    let stroke = egui::Stroke::new((rect.width() / 24.0 * 1.8).clamp(1.0, 2.5), color);
    let map = |x: f32, y: f32| {
        egui::pos2(
            rect.left() + x / 24.0 * rect.width(),
            rect.top() + y / 24.0 * rect.height(),
        )
    };
    for prim in list {
        match *prim {
            Prim::Circle(cx, cy, r) => {
                let center = map(cx, cy);
                painter.circle_stroke(center, r / 24.0 * rect.width(), stroke);
            }
            Prim::Line(x1, y1, x2, y2) => {
                painter.line_segment([map(x1, y1), map(x2, y2)], stroke);
            }
            Prim::Poly(points) => {
                if points.len() >= 2 {
                    painter.add(egui::Shape::line(
                        points.iter().map(|&(x, y)| map(x, y)).collect(),
                        stroke,
                    ));
                }
            }
            Prim::Rect(x, y, w, h) => {
                painter.rect_stroke(
                    egui::Rect::from_min_size(
                        map(x, y),
                        egui::vec2(w / 24.0 * rect.width(), h / 24.0 * rect.height()),
                    ),
                    egui::CornerRadius::ZERO,
                    stroke,
                    egui::StrokeKind::Inside,
                );
            }
            Prim::Dot(x, y) => {
                painter.circle_filled(map(x, y), stroke.width * 0.9, color);
            }
        }
    }
}

/// Botão de ícone com tooltip acessível (nome do ícone + uso).
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
