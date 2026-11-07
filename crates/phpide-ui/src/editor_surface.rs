//! Superfície avançada do editor — pintura das camadas novas da Fase 5
//! (UI-ED-001..007) sobre o viewport: chevrons de folding com hit areas,
//! estados reais de breakpoint, gutter de blame, hints/inlay, Code Vision,
//! lens preview e markers agregados da scrollbar.
//!
//! Funções puras de decisão (posições, visibilidade) ficam nos módulos
//! [`crate::folding`]/[`crate::lens`]/[`crate::hints`] e são testadas lá;
//! aqui só há pintura egui a partir dos modelos.

use eframe::egui;
use phpide_dbgp::breakpoint::BreakpointStatus;

use crate::editor_view::EditorGeometry;
use crate::folding::FoldVisuals;
use crate::icons_ui;
use crate::lens::ScrollMarker;

/// Estado visual de breakpoint por linha (UI-ED-001-04): derivado do
/// `BreakpointStatus` real do backend — nada de confirmação inventada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BpVisual {
    Enabled,
    Disabled,
    Verified,
    Invalid,
    Muted,
}

pub fn bp_visual(status: &BreakpointStatus, store_muted: bool) -> BpVisual {
    if store_muted {
        return BpVisual::Muted;
    }
    match status {
        BreakpointStatus::Pending => BpVisual::Enabled,
        BreakpointStatus::Verified => BpVisual::Verified,
        BreakpointStatus::Disabled => BpVisual::Disabled,
        BreakpointStatus::Invalid(_) => BpVisual::Invalid,
    }
}

/// Pinta o disco do breakpoint conforme o estado (sobre o número, New UI).
pub fn paint_breakpoint_disc(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    visual: BpVisual,
) {
    let (fill, ring) = match visual {
        BpVisual::Enabled => (
            egui::Color32::from_rgb(0xD9, 0x4A, 0x4A),
            egui::Color32::from_rgb(0xFF, 0x80, 0x80),
        ),
        BpVisual::Verified => (
            egui::Color32::from_rgb(0x2E, 0x7D, 0x32),
            egui::Color32::from_rgb(0x7B, 0xC6, 0x7F),
        ),
        BpVisual::Disabled => (
            egui::Color32::from_rgb(0x6E, 0x6E, 0x6E),
            egui::Color32::from_rgb(0x9E, 0x9E, 0x9E),
        ),
        BpVisual::Invalid => (
            egui::Color32::TRANSPARENT,
            egui::Color32::from_rgb(0xD9, 0x4A, 0x4A),
        ),
        BpVisual::Muted => (
            egui::Color32::from_rgb(0x9E, 0x9E, 0x9E).gamma_multiply(0.45_f32),
            egui::Color32::from_rgb(0x9E, 0x9E, 0x9E).gamma_multiply(0.45_f32),
        ),
    };
    painter.circle_filled(center, radius, fill);
    painter.circle_stroke(center, radius, egui::Stroke::new(1.2_f32, ring));
    if visual == BpVisual::Invalid {
        // X no centro (invalidado pelo backend).
        let r = radius * 0.55_f32;
        painter.line_segment(
            [center + egui::vec2(-r, -r), center + egui::vec2(r, r)],
            egui::Stroke::new(1.4_f32, ring),
        );
        painter.line_segment(
            [center + egui::vec2(r, -r), center + egui::vec2(-r, r)],
            egui::Stroke::new(1.4_f32, ring),
        );
    }
}

/// Ações devolvidas pela faixa de folding no gutter.
#[derive(Default)]
pub struct FoldAction {
    /// range idx a alternar (fold/unfold)
    pub toggle: Option<usize>,
}

/// Pinta chevrons de fold no gutter esquerdo (UI-ED-002-03): um por range
/// cujo início está visível. `icons_always` exibe sem hover; senão só no
/// hover da linha. Hit area = a linha inteira do gutter.
#[allow(clippy::too_many_arguments)]
pub fn paint_fold_chevrons(
    ui: &egui::Ui,
    geom: &EditorGeometry,
    gutter_left: f32,
    gutter_width: f32,
    ranges: &[phpide_syntax::folding::FoldRange],
    #[allow(unused_variables)] visuals: &FoldVisuals,
    folded: &std::collections::BTreeSet<usize>,
    doc_to_visual: Option<&[usize]>,
    total_visual_lines: usize,
    icons_always: bool,
) -> FoldAction {
    let mut action = FoldAction::default();
    let painter = ui.painter();
    let color = ui
        .visuals()
        .widgets
        .noninteractive
        .fg_stroke
        .color
        .gamma_multiply(0.8);
    let (first, last) = geom.visible_line_range(total_visual_lines);

    for (idx, r) in ranges.iter().enumerate() {
        let Some(vline) = (match doc_to_visual {
            Some(map) => map.get(r.start_line0).copied(),
            None => Some(r.start_line0),
        }) else {
            continue;
        };
        if vline < first || vline > last {
            continue; // sem desenho fora da viewport (aceite UI-ED-001-05)
        }
        let y = geom.row_y(vline);
        let row = egui::Rect::from_min_size(
            egui::pos2(gutter_left, y),
            egui::vec2(gutter_width, geom.row_height),
        );
        let is_folded = folded.contains(&idx);
        let hovered = ui.rect_contains_pointer(row);
        if icons_always || hovered {
            let icon = if is_folded {
                phpide_icons::IconId::ChevronRight
            } else {
                phpide_icons::IconId::ChevronDown
            };
            icons_ui::paint_icon(painter, row.shrink(geom.row_height * 0.22), icon, color);
        }
        let resp = ui.interact(row, egui::Id::new(("fold", idx)), egui::Sense::click());
        if resp.clicked() {
            action.toggle = Some(idx);
        }
        resp.on_hover_text(format!(
            "{} {} — clique para {}",
            r.kind,
            r.start_line0 + 1,
            if is_folded { "expandir" } else { "dobrar" }
        ));
    }
    action
}

/// Pinta anotações de blame na faixa esquerda do gutter (UI-ED-007-01).
/// Linha sem autoria: "·" (desconhecido ≠ sucesso).
pub fn paint_blame_strip(
    ui: &egui::Ui,
    geom: &EditorGeometry,
    strip: egui::Rect,
    annotations: &dyn Fn(usize) -> crate::blame_gutter::BlameAnnotation,
    total_lines: usize,
    stale: bool,
) {
    let painter = ui.painter();
    painter.rect_filled(
        strip,
        egui::CornerRadius::ZERO,
        ui.visuals().panel_fill.gamma_multiply(0.9),
    );
    let (first, last) = geom.visible_line_range(total_lines);
    let font = egui::FontId::proportional(10.5);
    let color = ui
        .visuals()
        .widgets
        .noninteractive
        .fg_stroke
        .color
        .gamma_multiply(if stale { 0.35 } else { 0.6 });
    for line0 in first..=last.min(total_lines.saturating_sub(1)) {
        let a = annotations(line0 + 1);
        let text = if a.uncommitted {
            "local".to_string()
        } else {
            format!(
                "{} {}",
                a.author_initials.as_deref().unwrap_or("·"),
                a.date.as_deref().unwrap_or("·")
            )
        };
        painter.text(
            egui::pos2(
                strip.left() + 3.0,
                geom.row_y(line0) + geom.row_height * 0.5,
            ),
            egui::Align2::LEFT_CENTER,
            text,
            font.clone(),
            color,
        );
    }
}

/// Pinta hints inlay no fim das linhas (UI-ED-005-03): texto fora do
/// documento, à direita do último glyph; não entra em save/busca/colar.
pub fn paint_inlay_hints(
    ui: &egui::Ui,
    geom: &EditorGeometry,
    galley: &egui::Galley,
    galley_pos: egui::Pos2,
    hints: &[crate::hints::InlayHint],
    doc_to_visual: Option<&[usize]>,
    total_lines: usize,
) {
    if hints.is_empty() {
        return;
    }
    let painter = ui.painter();
    let color = ui
        .visuals()
        .widgets
        .noninteractive
        .fg_stroke
        .color
        .gamma_multiply(0.55);
    let font = egui::FontId::proportional(11.5);
    let (first, last) = geom.visible_line_range(total_lines);
    for hint in hints {
        let vline = match doc_to_visual {
            Some(map) => match map.get(hint.line0).copied() {
                Some(v) => v,
                None => continue,
            },
            None => hint.line0,
        };
        if vline < first || vline > last {
            continue;
        }
        let Some(row) = galley.rows.get(vline) else {
            continue;
        };
        let x = galley_pos.x
            + row
                .glyphs
                .last()
                .map(|g| g.pos.x + g.advance_width)
                .unwrap_or(0.0)
            + 10.0;
        painter.text(
            egui::pos2(x, galley_pos.y + row.rect.center().y),
            egui::Align2::LEFT_CENTER,
            hint.label.clone(),
            font.clone(),
            color,
        );
    }
}

/// Pinta lenses de Code Vision acima do início do bloco (UI-ED-005-05/06).
/// Clique abre resultados reais (Usages) — a ação é devolvida.
pub fn paint_code_vision(
    ui: &egui::Ui,
    geom: &EditorGeometry,
    lenses: &std::collections::BTreeMap<usize, Vec<String>>,
    doc_to_visual: Option<&[usize]>,
    total_lines: usize,
    gutter_offset_x: f32,
) -> Option<usize> {
    let mut clicked: Option<usize> = None;
    let painter = ui.painter();
    let font = egui::FontId::proportional(11.0);
    let (first, last) = geom.visible_line_range(total_lines);
    for (block_line0, labels) in lenses {
        let vline = match doc_to_visual {
            Some(map) => match map.get(*block_line0).copied() {
                Some(v) => v,
                None => continue,
            },
            None => *block_line0,
        };
        if vline < first || vline > last {
            continue;
        }
        // Acima da linha do bloco (como a New UI): não cobre o código.
        let y = geom.row_y(vline) - geom.row_height;
        if y < geom.viewport.top() {
            continue;
        }
        let label = labels.join("   ");
        let w = 7.0 * label.chars().count() as f32;
        let rect = egui::Rect::from_min_size(
            egui::pos2(geom.viewport.right() - w - 12.0, y),
            egui::vec2(w, geom.row_height),
        );
        let resp = ui.interact(
            rect,
            egui::Id::new(("vision", *block_line0)),
            egui::Sense::click(),
        );
        painter.text(
            rect.left_center(),
            egui::Align2::LEFT_CENTER,
            label,
            font.clone(),
            ui.visuals().hyperlink_color,
        );
        if resp.clicked() {
            clicked = Some(*block_line0);
        }
        let _ = gutter_offset_x;
    }
    clicked
}

/// Pinta markers agregados (diagnósticos + Git) na scrollbar (UI-ED-006-01).
pub fn paint_scroll_markers(ui: &egui::Ui, geom: &EditorGeometry, markers: &[ScrollMarker]) {
    let vp = geom.viewport;
    let painter = ui.painter();
    let stripe_w = 4.0;
    let x = vp.right() - stripe_w;
    for m in markers {
        let y = vp.top() + m.frac * vp.height();
        painter.rect_filled(
            egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(stripe_w, 4.0)),
            egui::CornerRadius::ZERO,
            egui::Color32::from_rgb(m.color.0, m.color.1, m.color.2).gamma_multiply(0.9),
        );
    }
}

/// Hover na scrollbar → lens preview ancorado à região (UI-ED-006-02).
/// Devolve a linha sob o cursor para a UI posicionar o popup.
pub fn scrollbar_hover_line(
    ui: &egui::Ui,
    geom: &EditorGeometry,
    total_lines: usize,
) -> Option<usize> {
    let vp = geom.viewport;
    let stripe = egui::Rect::from_min_size(
        egui::pos2(vp.right() - 6.0, vp.top()),
        egui::vec2(6.0, vp.height()),
    );
    let pos = ui
        .rect_contains_pointer(stripe)
        .then(|| ui.ctx().pointer_hover_pos())??;
    let frac = ((pos.y - vp.top()) / vp.height().max(1.0)).clamp(0.0, 1.0);
    Some(crate::lens::line_at_scrollbar_frac(frac, total_lines))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bp_visual_maps_real_status() {
        use phpide_dbgp::breakpoint::BreakpointStatus as S;
        assert_eq!(bp_visual(&S::Pending, false), BpVisual::Enabled);
        assert_eq!(bp_visual(&S::Verified, false), BpVisual::Verified);
        assert_eq!(bp_visual(&S::Disabled, false), BpVisual::Disabled);
        assert_eq!(
            bp_visual(&S::Invalid("motivo".into()), false),
            BpVisual::Invalid
        );
        // Mute global acinzenta qualquer estado.
        assert_eq!(bp_visual(&S::Verified, true), BpVisual::Muted);
    }
}
