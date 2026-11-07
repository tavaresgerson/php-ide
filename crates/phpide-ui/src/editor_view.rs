//! Estágio 6.2 — superfície do editor (REQUISITOS 1.4): gutter com números de
//! linha, breakpoints **sobre** os números (New UI), marcadores de bookmark,
//! destaque da linha do cursor; sticky lines (máx. 5 declarações estruturais);
//! faixas de inspeção na scrollbar (erros/avisos do arquivo ativo).
//!
//! Tudo é pintado sobre o viewport usando a geometria exata da galley do
//! `TextEdit` (`TextEditOutput.galley_pos` + `Row.rect`) compensada pelo offset
//! do `ScrollArea` — sem widgets irmãos, sem drift de rolagem.

use std::collections::BTreeSet;

use eframe::egui;

use phpide_core::Severity;

pub use crate::geometry::{GUTTER_WIDTH, STICKY_MAX};

/// Geometria do viewport do editor em espaço de TELA. O `galley_pos` do egui
/// já inclui a trava de rolagem fisicamente aplicada: a linha 0 ocupa
/// `[galley_pos.y, galley_pos.y + row_height)`, mesmo que fora da janela
/// (confirmado empiricamente — ver `geometry_line_mapping`).
///
/// Wrapper fino sobre [`crate::geometry::Geometry`] (a fonte única das
/// fórmulas, T-UI-ED-000-02): o modelo do egui equivale ao mapa puro com
/// `origin_y = galley_pos.y` e `scroll = 0`.
pub struct EditorGeometry {
    /// Posição da galley na tela: linha 0 começa em `galley_pos.y`.
    pub galley_pos: egui::Pos2,
    /// Retângulo visível do editor (clip do painel central, espaço de tela).
    pub viewport: egui::Rect,
    /// Altura uniforme de uma linha (rows da galley são monoespaçadas).
    pub row_height: f32,
}

impl EditorGeometry {
    /// Scroll lógico implícito: quanto o conteúdo subiu em relação ao topo
    /// do viewport.
    pub fn scroll_y(&self) -> f32 {
        (self.viewport.top() - self.galley_pos.y).max(0.0)
    }

    /// O mapa puro equivalente para `total_lines` linhas.
    pub fn map(&self, total_lines: usize) -> crate::geometry::Geometry {
        crate::geometry::Geometry::new(self.row_height, self.viewport.height(), total_lines)
    }

    /// Y de tela do topo da linha `line0` (0-based).
    pub fn row_y(&self, line0: usize) -> f32 {
        self.galley_pos.y + line0 as f32 * self.row_height
    }

    /// Linha (0-based) cuja banda vertical contém `screen_y`, se houver.
    pub fn line_at_screen_y(&self, screen_y: f32) -> Option<usize> {
        self.map(usize::MAX)
            .screen_y_to_line(screen_y, 0.0, self.galley_pos.y)
    }

    /// Primeira linha visível no topo do viewport.
    pub fn first_visible_line(&self) -> usize {
        self.visible_line_range(usize::MAX).0
    }

    /// Intervalo inclusivo de linhas visíveis (clampado a `total_lines`).
    pub fn visible_line_range(&self, total_lines: usize) -> (usize, usize) {
        self.map(total_lines).visible_lines(self.scroll_y())
    }
}

/// Ações que o gutter pode solicitar (aplicadas pelo dono do estado).
#[derive(Default)]
pub struct GutterAction {
    pub toggle_breakpoint: Option<usize>,
}

/// Trilha de sticky lines: índices em `blocks` das declarações que contêm
/// `line0`, da mais externa à mais interna, apenas blocos que começam ANTES da
/// linha visível (os já visíveis não precisam ficar fixados). Cobertura de
/// testes: `sticky_trail_skips_blocks_starting_at_visible_line`.
pub fn sticky_trail(blocks: &[phpide_syntax::Block], line0: usize, max: usize) -> Vec<usize> {
    let mut chain: Vec<(usize, &phpide_syntax::Block)> = blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| {
            let end = if b.kind == "namespace_definition" {
                usize::MAX
            } else {
                b.end_line0
            };
            b.start_line0 < line0 && end >= line0
        })
        .collect();
    chain.sort_by_key(|(_, b)| b.start_line0);
    chain.into_iter().take(max).map(|(i, _)| i).collect()
}

/// Pinta o gutter (números, breakpoint sobre o número, bookmark, linha atual
/// e faixa de cobertura) e devolve ações de clique/hover. `breakpoints`/
/// `bookmarks` são 1-based. `coverage` vem do relatório importado (Clover):
/// linha executável coberta (verde) ou não coberta (vermelho) — linhas fora
/// do mapa não são pintadas (ausência ≠ sucesso, RUN-TST-003).
#[allow(clippy::too_many_arguments)]
pub fn paint_gutter(
    ui: &egui::Ui,
    geom: &EditorGeometry,
    total_lines: usize,
    cursor_line1: usize,
    breakpoints: &BTreeSet<usize>,
    bookmarks: &BTreeSet<usize>,
    galley_rows: usize,
    coverage: Option<&phpide_run::coverage::FileCoverage>,
) -> GutterAction {
    let mut action = GutterAction::default();
    let vp = geom.viewport;
    let painter = ui.painter();
    let visuals = ui.visuals();

    // Fundo do gutter: faixa à esquerda da área de texto, dentro do painel
    // (o texto do editor é recuado em GUTTER_WIDTH para não ficar sob ela).
    let gutter_rect = egui::Rect::from_min_size(
        egui::pos2(vp.left(), vp.top()),
        egui::vec2(GUTTER_WIDTH, vp.height()),
    );
    painter.rect_filled(gutter_rect, egui::CornerRadius::ZERO, visuals.panel_fill);
    painter.line_segment(
        [
            egui::pos2(gutter_rect.right(), gutter_rect.top()),
            egui::pos2(gutter_rect.right(), gutter_rect.bottom()),
        ],
        egui::Stroke::new(1.0_f32, visuals.widgets.noninteractive.bg_stroke.color),
    );

    let lines_to_paint = total_lines.min(galley_rows);
    let (first, last) = geom.visible_line_range(lines_to_paint);
    let last = last.min(lines_to_paint.saturating_sub(1));

    let num_font = egui::FontId::monospace(12.0);
    let num_x = gutter_rect.right() - 7.0;

    // Destaque da linha do cursor (banda em todo o gutter).
    if cursor_line1 >= 1 && cursor_line1 <= lines_to_paint {
        let y = geom.row_y(cursor_line1 - 1);
        if y + geom.row_height > vp.top() && y < vp.bottom() {
            painter.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(gutter_rect.left(), y),
                    egui::vec2(GUTTER_WIDTH, geom.row_height),
                ),
                egui::CornerRadius::ZERO,
                visuals.selection.bg_fill.gamma_multiply(0.35),
            );
        }
    }

    for line0 in first..=last {
        let line1 = line0 + 1;
        let y = geom.row_y(line0);
        let row_center = y + geom.row_height * 0.5;
        let center = egui::pos2(num_x, row_center);

        // Faixa de cobertura na borda esquerda do gutter (T-RUN-TST-003-02):
        // só linhas executáveis do relatório são pintadas.
        if let Some(cov) = coverage {
            if let Some(hits) = cov.line_hits(line1 as u32) {
                let color = if hits > 0 {
                    egui::Color32::from_rgb(0x2E, 0x7D, 0x32)
                } else {
                    egui::Color32::from_rgb(0xC6, 0x28, 0x28)
                };
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(gutter_rect.left(), y),
                        egui::vec2(3.0, geom.row_height),
                    ),
                    egui::CornerRadius::ZERO,
                    color.gamma_multiply(0.85),
                );
            }
        }

        if breakpoints.contains(&line1) {
            // Breakpoint SOBRE o número (New UI): círculo vermelho na posição
            // do dígito; o número em si some atrás dele.
            painter.circle_filled(
                center,
                geom.row_height * 0.32,
                egui::Color32::from_rgb(0xD9, 0x4A, 0x4A),
            );
            painter.circle_stroke(
                center,
                geom.row_height * 0.32,
                egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0xFF, 0x80, 0x80)),
            );
        } else {
            let color = if line1 == cursor_line1 {
                visuals.strong_text_color()
            } else {
                visuals
                    .widgets
                    .noninteractive
                    .fg_stroke
                    .color
                    .gamma_multiply(0.55)
            };
            painter.text(
                center,
                egui::Align2::RIGHT_CENTER,
                line1.to_string(),
                num_font.clone(),
                color,
            );
        }

        if bookmarks.contains(&line1) {
            // Marcador de bookmark: barra vertical amarela na borda esquerda.
            painter.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(gutter_rect.left() + 1.0, y + 2.0),
                    egui::vec2(3.0, geom.row_height - 4.0),
                ),
                egui::CornerRadius::ZERO,
                egui::Color32::from_rgb(0xE8, 0xC5, 0x4A),
            );
        }
    }

    // Interação: clique no gutter alterna breakpoint na linha clicada.
    let resp = ui.interact(
        gutter_rect,
        egui::Id::new("editor-gutter"),
        egui::Sense::click(),
    );
    if resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            if let Some(line0) = geom.line_at_screen_y(pos.y) {
                if line0 < lines_to_paint {
                    action.toggle_breakpoint = Some(line0 + 1);
                }
            }
        }
    }
    if let Some(pos) = resp.hover_pos() {
        if let Some(line0) = geom.line_at_screen_y(pos.y) {
            if line0 < lines_to_paint {
                let y = geom.row_y(line0);
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(gutter_rect.left(), y),
                        egui::vec2(GUTTER_WIDTH, geom.row_height),
                    ),
                    egui::CornerRadius::ZERO,
                    visuals.widgets.hovered.weak_bg_fill.gamma_multiply(0.5),
                );
                resp.on_hover_text(format!(
                    "Ln {line1} — clique: breakpoint (F9)",
                    line1 = line0 + 1
                ));
            }
        }
    }
    action
}

/// Pinta as sticky lines (declarações estruturais fixadas no topo do editor
/// ao rolar) e devolve linha para salto, se alguma entrada foi clicada.
pub fn paint_sticky_lines(
    ui: &egui::Ui,
    geom: &EditorGeometry,
    doc_lines: &[String],
    blocks: &[phpide_syntax::Block],
) -> Option<usize> {
    let first_visible = geom.first_visible_line();
    let trail = sticky_trail(blocks, first_visible, STICKY_MAX);
    if trail.is_empty() {
        return None;
    }
    let vp = geom.viewport;
    let painter = ui.painter();
    let visuals = ui.visuals();
    let mut jump: Option<usize> = None;

    let font = egui::FontId::monospace(13.0);
    for (depth, block_idx) in trail.iter().enumerate() {
        let block = &blocks[*block_idx];
        let y = vp.top() + depth as f32 * geom.row_height;
        let row = egui::Rect::from_min_size(
            egui::pos2(vp.left() + GUTTER_WIDTH, y),
            egui::vec2(vp.width() - GUTTER_WIDTH, geom.row_height),
        );
        painter.rect_filled(row, egui::CornerRadius::ZERO, visuals.panel_fill);
        painter.line_segment(
            [row.left_bottom(), row.right_bottom()],
            egui::Stroke::new(1.0_f32, visuals.widgets.noninteractive.bg_stroke.color),
        );
        // Linha de origem (cabeçalho da declaração), recuada por profundidade.
        let source = doc_lines
            .get(block.start_line0)
            .map(|l| l.trim().chars().take(120).collect::<String>())
            .unwrap_or_default();
        let text_x = row.left() + 8.0 + depth as f32 * 10.0;
        let interact = ui.interact(
            row,
            egui::Id::new(("sticky", block.start_line0, depth)),
            egui::Sense::click(),
        );
        painter.text(
            egui::pos2(text_x, row.center().y),
            egui::Align2::LEFT_CENTER,
            source,
            font.clone(),
            visuals.widgets.noninteractive.fg_stroke.color,
        );
        if interact.clicked() {
            jump = Some(block.start_line0);
        }
        interact.on_hover_text("Sticky line — clique para ir à declaração");
    }
    // Linha separadora inferior da pilha (só quando há sticky ativo).
    let bottom = vp.top() + trail.len() as f32 * geom.row_height;
    painter.line_segment(
        [
            egui::pos2(vp.left(), bottom),
            egui::pos2(vp.right(), bottom),
        ],
        egui::Stroke::new(1.5_f32, visuals.selection.bg_fill),
    );
    jump
}

/// Faixas de inspeção na scrollbar (REQUISITOS 1.4): marcas coloridas na borda
/// direita do viewport na altura de cada diagnóstico do arquivo ativo.
pub fn paint_scrollbar_stripes(
    ui: &egui::Ui,
    geom: &EditorGeometry,
    diagnostics: &[(u32, Severity)],
    total_lines: usize,
) {
    if diagnostics.is_empty() || total_lines == 0 {
        return;
    }
    let vp = geom.viewport;
    let painter = ui.painter();
    let stripe_w = 3.0;
    let x = vp.right() - stripe_w;
    for (line1, severity) in diagnostics {
        if *line1 == 0 {
            continue;
        }
        let frac = (*line1 as f32 - 0.5) / total_lines.max(1) as f32;
        if !(0.0..=1.0).contains(&frac) {
            continue;
        }
        let y = vp.top() + frac * vp.height();
        let color = match severity {
            Severity::Error => egui::Color32::from_rgb(0xE5, 0x53, 0x4B),
            Severity::Warning => egui::Color32::from_rgb(0xDB, 0xA9, 0x3C),
            Severity::Information => egui::Color32::from_rgb(0x35, 0x74, 0xF0),
            Severity::Hint => egui::Color32::from_rgb(0x6E, 0x6E, 0x6E),
        }
        .gamma_multiply(0.85);
        painter.rect_filled(
            egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(stripe_w, 4.0)),
            egui::CornerRadius::ZERO,
            color,
        );
    }
}

/// Pinta retângulos de fundo sob as ocorrências do símbolo (T-PHP-005-02),
/// derivados dos glyphs da galley — o mesmo espaço de coordenadas do texto,
/// sem drift de rolagem. Ranges em caracteres absolutos do documento.
/// (Sem wrap de linha, `galley.rows` corresponde 1:1 às linhas do documento.)
pub fn paint_occurrences(
    ui: &egui::Ui,
    galley: &egui::Galley,
    galley_pos: egui::Pos2,
    ranges: &[(usize, usize)],
) {
    if ranges.is_empty() {
        return;
    }
    let painter = ui.painter();
    let color = ui.visuals().selection.bg_fill.gamma_multiply(0.22);
    // Base de char absoluta de cada linha: um glyph por char; o '\n' não é
    // glyph (flag `ends_with_newline`), mas conta no texto.
    let mut bases = Vec::with_capacity(galley.rows.len());
    let mut acc = 0usize;
    for row in &galley.rows {
        bases.push(acc);
        acc += row.glyphs.len() + usize::from(row.ends_with_newline);
    }
    for &(start, end) in ranges {
        if end <= start {
            continue;
        }
        // Linha do range pela base mais próxima (última base ≤ start).
        let line0 = match bases.binary_search(&start) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let Some(row) = galley.rows.get(line0) else {
            continue;
        };
        let base = bases[line0];
        if start < base || start >= base + row.glyphs.len() + usize::from(row.ends_with_newline) {
            continue;
        }
        let mut x_min = f32::INFINITY;
        let mut x_max = f32::NEG_INFINITY;
        let mut found = false;
        for (i, glyph) in row.glyphs.iter().enumerate() {
            let g_start = base + i;
            let g_end = g_start + 1;
            if g_end > start && g_start < end {
                let x = galley_pos.x + glyph.pos.x;
                x_min = x_min.min(x);
                x_max = x_max.max(x + glyph.advance_width);
                found = true;
            }
            if g_start >= end {
                break;
            }
        }
        if found {
            painter.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(x_min, galley_pos.y + row.rect.top()),
                    egui::vec2((x_max - x_min).max(2.0), row.rect.height()),
                ),
                egui::CornerRadius::ZERO,
                color,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocks() -> Vec<phpide_syntax::Block> {
        phpide_syntax::analyze(
            "<?php\nnamespace Demo;\nclass Product {\n    public function price(): float {\n        return 1.0;\n    }\n}\n",
        )
        .blocks
    }

    #[test]
    fn geometry_line_mapping() {
        // Sem scroll: linha 0 em y=100.
        let geom = EditorGeometry {
            galley_pos: egui::pos2(0.0, 100.0),
            viewport: egui::Rect::from_min_size(egui::pos2(0.0, 100.0), egui::vec2(800.0, 300.0)),
            row_height: 18.0,
        };
        assert_eq!(geom.line_at_screen_y(100.0), Some(0));
        assert_eq!(geom.line_at_screen_y(100.0 + 3.0 * 18.0 + 2.0), Some(3));
        assert_eq!(geom.line_at_screen_y(99.0), None);
        // Rolado 40px para baixo: a galley sobe (linha 0 em y=60) e a linha 2
        // aparece no topo do viewport (y=100) — modelo de tela do egui, onde o
        // scroll já está fisicamente aplicado em galley_pos.
        let scrolled = EditorGeometry {
            galley_pos: egui::pos2(0.0, 60.0),
            viewport: geom.viewport,
            row_height: 18.0,
        };
        assert_eq!(scrolled.first_visible_line(), 2);
        assert_eq!(scrolled.row_y(2), 96.0);
    }

    #[test]
    fn sticky_trail_skips_blocks_starting_at_visible_line() {
        let b = blocks();
        // Linha 4 (0-based) está dentro de namespace(1)+class(2)+method(3).
        let trail = sticky_trail(&b, 4, STICKY_MAX);
        assert_eq!(trail.len(), 3, "ns+class+method coladas");
        // Na linha onde a class COMEÇA (2), ela não gruda: a declaração já é a
        // própria linha visível. Só o namespace (que começa antes) fica colado.
        let at_class_start = sticky_trail(&b, 2, STICKY_MAX);
        let kinds: Vec<_> = at_class_start.iter().map(|&i| b[i].kind.as_str()).collect();
        assert_eq!(kinds, vec!["namespace_definition"]);
        // Cap de 5.
        let capped = sticky_trail(&b, 4, 2);
        assert_eq!(capped.len(), 2);
    }
}
