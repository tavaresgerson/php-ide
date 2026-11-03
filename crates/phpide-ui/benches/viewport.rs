//! Baseline de scroll e viewport no arquivo de 10 mil linhas —
//! T-UI-ED-000-04 (UI-ED-000), antes das novas camadas do editor.
//!
//! Simula o trabalho por frame do mapa de geometria: intervalo de linhas
//! visíveis, hit tests de gutter e saltos de "go to line" ao longo de um
//! sweep de scroll. O pipeline de pixels (galley/egui) não é mensurável
//! sem GPU; aqui mede-se a parte determinística do núcleo.

#![allow(dead_code)]

#[path = "../../phpide-core/benches/harness.rs"]
mod harness;

#[path = "../src/geometry.rs"]
mod geometry;

use harness::bench;
use phpide_core::fixtures::large_php_document;

fn main() {
    let text = large_php_document(10_000);
    let total_lines = text.lines().count().max(1);
    // Viewport típico de editor: 800×600, linha 18px → ~33 linhas visíveis.
    let g = geometry::Geometry::new(18.0, 600.0, total_lines);
    let max_scroll = g.max_scroll();
    assert!(max_scroll > 0.0);

    // Sweep de 600 frames cobrindo o documento inteiro (ida e volta).
    bench("scroll_sweep_600_frames_10k", 20, 3, || {
        for frame in 0..600 {
            let t = frame as f32 / 600.0;
            let scroll = max_scroll * if t < 0.5 { t * 2.0 } else { (1.0 - t) * 2.0 };
            let scroll = std::hint::black_box(g.clamp_scroll(std::hint::black_box(scroll)));
            let (first, last) = g.visible_lines(scroll);
            // Hit test de clique no gutter a cada frame (centro do viewport).
            let mid_y = 300.0;
            let hit = g.screen_y_to_line(mid_y, scroll, 0.0);
            std::hint::black_box((first, last, hit));
        }
    });

    // "Go to line": saltos para linhas aleatórias determinísticas (F2, busca,
    // palette) — deve achar o scroll alvo sem caminhar frame a frame.
    let targets: Vec<usize> = (0..50).map(|i| (i * 13_337) % total_lines).collect();
    bench("goto_line_50_jumps_10k", 20, 3, || {
        let mut scroll = 0.0f32;
        for &t in &targets {
            scroll = g.scroll_to_line(std::hint::black_box(t), 2, scroll);
            assert!(g.line_visible(t, scroll));
        }
        std::hint::black_box(scroll);
    });

    // Per-frame de renderização realista: visible_lines + 33 hit tests
    // (uma por linha visível, como o gutter pinta números e recebe hover).
    bench("frame_geometry_33_hits_10k", 50, 5, || {
        let scroll = max_scroll * 0.37;
        let (first, last) = g.visible_lines(scroll);
        for line0 in first..=last {
            let y = g.line_to_screen_y(std::hint::black_box(line0), scroll, 0.0);
            let back = g.screen_y_to_line(y, scroll, 0.0);
            std::hint::black_box(back);
        }
    });
}
