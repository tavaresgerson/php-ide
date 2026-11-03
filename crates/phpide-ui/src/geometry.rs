//! Mapa centralizado de geometria do editor — T-UI-ED-000-02/000-03 (UI-ED-000).
//!
//! Única fonte das fórmulas entre **documento → linha visual → coordenada
//! de tela**, incluindo gutter e viewport. Puro (sem egui) para ser testado
//! de forma determinística; `editor_view::EditorGeometry` é um wrapper fino
//! sobre este mapa no espaço de tela do egui.
//!
//! Modelo: espaço de CONTEÚDO (y cresce com as linhas, sem scroll) +
//! `scroll_y` (pixels rolados para baixo). Conversão para tela:
//! `screen_y = origin_y - scroll_y + content_y`, onde `origin_y` é a posição
//! de tela do topo do conteúdo sem scroll (equivale ao modelo do egui, onde
//! o scroll já está fisicamente aplicado em `galley_pos`).

/// Largura fixa do gutter em px (escala 1.0). Recuo do texto = esta largura.
pub const GUTTER_WIDTH: f32 = 46.0;
/// Altura máxima da pilha de sticky lines (REQUISITOS UI-ED-004: máx. 5).
pub const STICKY_MAX: usize = 5;

/// Parâmetros puros da geometria do editor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    /// Altura de uma linha em px (já com zoom aplicado — a galley do egui
    /// é monoespaçada em altura).
    pub row_height: f32,
    /// Largura do gutter em px.
    pub gutter_width: f32,
    /// Altura visível do viewport em px.
    pub viewport_height: f32,
    /// Número de linhas do documento (rope: arquivo vazio = 1).
    pub total_lines: usize,
}

impl Geometry {
    pub fn new(row_height: f32, viewport_height: f32, total_lines: usize) -> Self {
        Geometry {
            row_height,
            gutter_width: GUTTER_WIDTH,
            viewport_height,
            total_lines: total_lines.max(1),
        }
    }

    /// Altura total do conteúdo em px.
    pub fn content_height(&self) -> f32 {
        self.total_lines as f32 * self.row_height
    }

    /// Máximo de scroll válido (nunca negativo; 0 quando o conteúdo cabe).
    pub fn max_scroll(&self) -> f32 {
        (self.content_height() - self.viewport_height).max(0.0)
    }

    /// Clampa `scroll_y` em `[0, max_scroll]` (scroll elástico da UI).
    pub fn clamp_scroll(&self, scroll_y: f32) -> f32 {
        scroll_y.clamp(0.0, self.max_scroll())
    }

    /// Y em espaço de conteúdo do topo da linha `line0`.
    pub fn line_to_content_y(&self, line0: usize) -> f32 {
        line0 as f32 * self.row_height
    }

    /// Linha (0-based) que contém o y de conteúdo (floor do índice).
    pub fn content_y_to_line(&self, content_y: f32) -> usize {
        if content_y <= 0.0 {
            return 0;
        }
        let l = (content_y / self.row_height) as usize;
        l.min(self.total_lines.saturating_sub(1))
    }

    /// Y de tela do topo da linha `line0` para um dado scroll.
    // API do mapa centralizado: consumida pelos benches e pelas próximas
    // camadas (folding UI-ED-002); coberta por UT.
    #[allow(dead_code)]
    pub fn line_to_screen_y(&self, line0: usize, scroll_y: f32, origin_y: f32) -> f32 {
        origin_y - scroll_y + self.line_to_content_y(line0)
    }

    /// Linha cuja banda vertical contém `screen_y`, se dentro do conteúdo.
    pub fn screen_y_to_line(&self, screen_y: f32, scroll_y: f32, origin_y: f32) -> Option<usize> {
        let content_y = screen_y - origin_y + scroll_y;
        if content_y < 0.0 || self.total_lines == 0 {
            return None;
        }
        Some(self.content_y_to_line(content_y))
    }

    /// Intervalo inclusivo de linhas visíveis no viewport para um scroll.
    /// Arquivo vazio (1 linha) ou viewport maior que o conteúdo: linha 0.
    pub fn visible_lines(&self, scroll_y: f32) -> (usize, usize) {
        let scroll = self.clamp_scroll(scroll_y);
        let first = (scroll / self.row_height) as usize;
        let count = (self.viewport_height / self.row_height).ceil() as usize;
        let last = (first + count).min(self.total_lines.saturating_sub(1));
        (first.min(last), last)
    }

    /// `true` se a linha estiver (pelo menos parcialmente) visível.
    // API do mapa centralizado: consumida pelos benches e pelas próximas
    // camadas (revelação automática de caret com folding); coberta por UT.
    #[allow(dead_code)]
    pub fn line_visible(&self, line0: usize, scroll_y: f32) -> bool {
        let (first, last) = self.visible_lines(scroll_y);
        line0 >= first && line0 <= last
    }

    /// Scroll que torna a linha `line0` visível, com `margin` linhas de
    /// contexto (clampado). Acima: ancora no topo com margem; abaixo:
    /// ancora no fim do viewport. Se já visível, mantém o scroll atual —
    /// a UI não deve saltar desnecessariamente.
    // API do mapa centralizado: consumida pelos benches e pelas próximas
    // camadas (go to line com folding); coberta por UT.
    #[allow(dead_code)]
    pub fn scroll_to_line(&self, line0: usize, margin: usize, scroll_y: f32) -> f32 {
        if self.line_visible(line0, scroll_y) {
            return self.clamp_scroll(scroll_y);
        }
        let (first, _) = self.visible_lines(scroll_y);
        if line0 < first {
            let target = line0.saturating_sub(margin);
            self.clamp_scroll(self.line_to_content_y(target))
        } else {
            let anchor = (line0 + margin + 1).min(self.total_lines.saturating_sub(1));
            let y = self.line_to_content_y(anchor) + self.row_height - self.viewport_height;
            self.clamp_scroll(y.max(0.0))
        }
    }
}

/// Largura necessária do gutter para `digits` dígitos de número de linha.
/// `base` cobre padding + marcadores; cada dígito soma `per_digit`.
// API do mapa centralizado: a pintura usa largura fixa (New UI); a fórmula
// valida a capacidade por dígitos e servirá ao re-layout automático.
#[allow(dead_code)]
pub fn gutter_width_for_digits(digits: usize, base: f32, per_digit: f32) -> f32 {
    base + digits.max(1) as f32 * per_digit
}

/// Coluna visual após avançar `col` chars de `line` (0..=upto_chars),
/// expandindo tabs para múltiplos de `tab_width` (display, não edição).
// API do mapa centralizado: tabs são renderizados pela galley do egui;
// a expansão lógica entra com colunas visuais/indent guides (UI-ED-008).
#[allow(dead_code)]
pub fn visual_column(line: &str, upto_chars: usize, tab_width: usize) -> usize {
    let tab = tab_width.max(1);
    let mut col = 0usize;
    for c in line.chars().take(upto_chars) {
        if c == '\t' {
            col += tab - (col % tab);
        } else {
            col += 1;
        }
    }
    col
}

/// X aproximado (monoespaçado) da coluna `col` na área de texto.
// API do mapa centralizado: o x real vem da galley; usado pelos benches.
#[allow(dead_code)]
pub fn column_to_x(gutter_width: f32, col: usize, char_width: f32) -> f32 {
    gutter_width + col as f32 * char_width
}

/// Coluna visual do x na área de texto (monoespaçado).
#[allow(dead_code)]
pub fn x_to_column(gutter_width: f32, x: f32, char_width: f32) -> usize {
    if char_width <= 0.0 {
        return 0;
    }
    ((x - gutter_width).max(0.0) / char_width) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geom(lines: usize) -> Geometry {
        Geometry::new(18.0, 300.0, lines)
    }

    #[test]
    fn doc_to_line_to_coordinate_basics() {
        let g = geom(100);
        assert_eq!(g.line_to_content_y(0), 0.0);
        assert_eq!(g.line_to_content_y(3), 54.0);
        assert_eq!(g.content_y_to_line(0.0), 0);
        assert_eq!(g.content_y_to_line(54.0), 3);
        assert_eq!(g.content_y_to_line(54.0 + 17.9), 3);
        // Tela: origem 100, sem scroll → linha 3 em y=154.
        assert_eq!(g.line_to_screen_y(3, 0.0, 100.0), 154.0);
        // Rolado 40px: conteúdo sobe 40px.
        assert_eq!(g.line_to_screen_y(3, 40.0, 100.0), 114.0);
        assert_eq!(g.screen_y_to_line(114.0, 40.0, 100.0), Some(3));
        // Acima do conteúdo rolado (y < origem - scroll): nenhuma linha.
        assert_eq!(g.screen_y_to_line(59.0, 40.0, 100.0), None);
        assert_eq!(g.screen_y_to_line(60.0, 40.0, 100.0), Some(0));
    }

    #[test]
    fn scroll_clamps_at_content_bounds() {
        let g = geom(10); // conteúdo 180px < viewport 300px: sem scroll.
        assert_eq!(g.max_scroll(), 0.0);
        assert_eq!(g.clamp_scroll(50.0), 0.0);
        let big = geom(10_000); // 180_000px.
        assert_eq!(big.max_scroll(), 180_000.0 - 300.0);
        assert_eq!(big.clamp_scroll(-10.0), 0.0);
        assert_eq!(big.clamp_scroll(999_999.0), big.max_scroll());
    }

    #[test]
    fn visible_line_range_tracks_scroll_and_viewport() {
        let g = geom(10_000);
        let (first, last) = g.visible_lines(0.0);
        assert_eq!(first, 0);
        // 300px / 18px = 16.7 → 17 bandas; última = 0+17 limitada ao doc.
        assert_eq!(last, 17);
        let (first, last) = g.visible_lines(180.0); // linha 10 no topo.
        assert_eq!(first, 10);
        assert_eq!(last, 27);
        // No fim do conteúdo: última linha = total-1 (clamp).
        let (first, last) = g.visible_lines(g.max_scroll());
        assert_eq!(last, 9_999);
        assert!(first <= last);
    }

    #[test]
    fn scroll_to_line_brings_target_into_view() {
        let g = geom(10_000);
        // Linha abaixo do viewport: scroll aumenta até ela aparecer no fim.
        let s = g.scroll_to_line(100, 0, 0.0);
        assert!(g.line_visible(100, s), "linha 100 visível após scroll {s}");
        // Linha acima: sobe com margem de contexto.
        let s2 = g.scroll_to_line(10, 2, s);
        assert!(g.line_visible(10, s2));
        assert!(s2 < s);
        let (first, _) = g.visible_lines(s2);
        assert_eq!(first, 8, "margem de 2 linhas no topo");
        // Já visível: scroll não muda.
        let (first, last) = g.visible_lines(s2);
        let inside = (first + last) / 2;
        assert_eq!(g.scroll_to_line(inside, 0, s2), s2);
    }

    #[test]
    fn empty_file_keeps_single_line_geometry() {
        let g = Geometry::new(18.0, 300.0, 0);
        assert_eq!(g.total_lines, 1, "arquivo vazio = 1 linha (rope)");
        assert_eq!(g.max_scroll(), 0.0);
        assert_eq!(g.visible_lines(0.0), (0, 0));
        assert_eq!(g.screen_y_to_line(10.0, 0.0, 0.0), Some(0));
    }

    #[test]
    fn zoom_scales_row_height_and_visible_count() {
        let base = Geometry::new(18.0, 300.0, 1_000);
        let zoomed = Geometry::new(18.0 * 1.5, 300.0, 1_000);
        assert_eq!(base.visible_lines(0.0).1, 17);
        // Zoom 1.5×: linhas 27px; 300/27 = 11.1 → 12 bandas cobrem o viewport.
        assert_eq!(zoomed.visible_lines(0.0).1, 12);
        assert_eq!(
            zoomed.line_to_content_y(10),
            base.line_to_content_y(10) * 1.5
        );
        // O recuo do gutter não muda com zoom (largura fixa da New UI);
        // números continuam alinhados à direita.
        assert_eq!(base.gutter_width, zoomed.gutter_width);
    }

    #[test]
    fn gutter_width_grows_with_digit_count() {
        // Medidas reais da pintura: fonte 12px monoespaçada ≈ 6.6px/dígito
        // + 12px de padding/marcadores.
        let one = gutter_width_for_digits(1, 12.0, 6.6);
        let five = gutter_width_for_digits(5, 12.0, 6.6);
        assert!(five > one);
        // Gutter atual comporta 5 dígitos (arquivo de 10 mil linhas).
        assert!(
            gutter_width_for_digits(5, 12.0, 6.6) <= GUTTER_WIDTH,
            "gutter fixo deve caber números de 10k linhas"
        );
        // 6 dígitos (>99.999 linhas) já excede: comportamento documentado
        // (alinhamento à direita, clipa) — não aumenta sem re-layout.
        assert!(gutter_width_for_digits(6, 12.0, 6.6) > GUTTER_WIDTH);
    }

    #[test]
    fn long_lines_do_not_change_vertical_geometry() {
        let g = geom(3);
        // Altura é fixa por linha: linha longa não empurra as seguintes.
        assert_eq!(g.line_to_content_y(2), 36.0);
        assert_eq!(g.content_y_to_line(36.0), 2);
    }

    #[test]
    fn tabs_advance_to_next_multiple() {
        assert_eq!(visual_column("a\tb", 2, 4), 4, "'a' + tab(4) = col 4");
        assert_eq!(visual_column("abc\tx", 4, 4), 4);
        assert_eq!(visual_column("abcd\tx", 5, 4), 8);
        assert_eq!(visual_column("sem tabs", 3, 4), 3);
        // Unicode conta como 1 coluna visual (largura de glifo é da fonte).
        assert_eq!(visual_column("héllo\t😀", 6, 4), 8);
    }

    #[test]
    fn column_x_roundtrip_monospace() {
        let x = column_to_x(GUTTER_WIDTH, 10, 8.0);
        assert_eq!(x, GUTTER_WIDTH + 80.0);
        assert_eq!(x_to_column(GUTTER_WIDTH, x, 8.0), 10);
        assert_eq!(x_to_column(GUTTER_WIDTH, GUTTER_WIDTH - 5.0, 8.0), 0);
    }
}
