//! The per-frame cost of the editor, measured on real egui frames.
//!
//! Phase 3 acceptance: a 50k-line file opens in under 300 ms, scrolling
//! stays inside the 60 fps budget (16.6 ms per frame) and a keystroke,
//! edit + incremental reparse + repaint, costs under 16 ms. Frames run the
//! whole widget (layout, highlight, painting) and egui's tessellation; only
//! the GPU upload is missing.
//!
//! The largest file runs twice: without and with the minimap (Phase 12:
//! the minimap must keep the Phase 3 numbers).
//!
//! Run: `cargo bench -p phpide-editor --bench editor_pipeline`

use std::path::Path;
use std::time::{Duration, Instant};

use eframe::egui::{self, Event, RawInput};
use phpide_core::fixtures::large_php_document;
use phpide_core::Buffer;
use phpide_editor::decorations::Decorations;
use phpide_editor::{Editor, EditorOptions, EditorStyle, EditorView, MinimapOptions, Syntax};

/// File sizes, and whether the minimap is on.
const RUNS: [(usize, bool); 4] = [
    (2_000, false),
    (10_000, false),
    (50_000, false),
    (50_000, true),
];
const FRAME_BUDGET_MS: f64 = 16.6;
const OPEN_BUDGET_MS: f64 = 300.0;

struct Pane {
    ctx: egui::Context,
    style: EditorStyle,
    buffer: Buffer,
    view: EditorView,
    syntax: Syntax,
    options: EditorOptions,
    time: f64,
}

impl Pane {
    /// Renders one frame and tessellates it, like `eframe` does before the
    /// GPU upload.
    fn frame(&mut self, events: Vec<Event>) -> Duration {
        self.time += 1.0 / 60.0;
        let raw = RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        let decorations = Decorations::default();
        let t0 = Instant::now();
        let Pane {
            ctx,
            style,
            buffer,
            view,
            syntax,
            options,
            ..
        } = self;
        let mut full = ctx.run_ui(raw, |ui| {
            Editor::new("bench", buffer, view, syntax, style, &decorations)
                .options(options.clone())
                .show(ui);
        });
        let shapes = std::mem::take(&mut full.shapes);
        let primitives = ctx.tessellate(shapes, full.pixels_per_point);
        std::hint::black_box(primitives.len());
        full.textures_delta.clear();
        t0.elapsed()
    }
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

fn max(samples: &[Duration]) -> Duration {
    samples.iter().copied().max().unwrap_or_default()
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn row(name: &str, lines: usize, value: Duration, budget: f64) {
    let v = ms(value);
    let flag = if v > budget {
        format!("  OVER {budget} ms")
    } else {
        String::new()
    };
    println!("{name:<36} {lines:>6} lines  {v:>9.2} ms{flag}");
}

fn main() {
    println!("editor pipeline (release build recommended)");
    println!("{}", "-".repeat(72));
    Syntax::warm_up();
    // Font atlas and query compilation happen once per process.
    let ctx = egui::Context::default();
    let style = EditorStyle::from_visuals(&egui::Visuals::dark(), 14.0);

    for (lines, minimap) in RUNS {
        let text = large_php_document(lines);
        let total = text.lines().count();
        if minimap {
            println!("with the minimap");
        }

        // Open: buffer + syntax + first painted frame.
        let t0 = Instant::now();
        let mut buffer = Buffer::untitled();
        buffer.replace_all(&text);
        let syntax = Syntax::for_path(Some(Path::new("big.php")));
        let mut pane = Pane {
            ctx: ctx.clone(),
            style: style.clone(),
            buffer,
            view: EditorView::new(),
            syntax,
            options: EditorOptions {
                minimap: MinimapOptions {
                    enabled: minimap,
                    ..MinimapOptions::default()
                },
                ..EditorOptions::default()
            },
            time: 0.0,
        };
        pane.ctx
            .memory_mut(|m| m.request_focus(Editor::widget_id("bench")));
        pane.frame(Vec::new());
        let open = t0.elapsed();
        row("open + first frame", total, open, OPEN_BUDGET_MS);

        // Background parse finishes; it never blocks a frame.
        let t0 = Instant::now();
        pane.syntax.wait_idle(&pane.buffer.document);
        let _ = pane.syntax.structure(&pane.buffer.document);
        pane.syntax.wait_idle(&pane.buffer.document);
        row(
            "  background parse + structure",
            total,
            t0.elapsed(),
            f64::INFINITY,
        );

        // Scroll: 3 rows per frame through the whole file.
        let row_height = 14.0 * 1.4;
        let step = row_height * 3.0;
        let frames = 300usize;
        let stride = (total as f32 * row_height / frames as f32).max(step);
        let mut scroll_samples = Vec::with_capacity(frames);
        for i in 0..frames {
            pane.view.scroll.y = i as f32 * stride;
            scroll_samples.push(pane.frame(Vec::new()));
        }
        row(
            "scroll frame (median)",
            total,
            median(scroll_samples.clone()),
            FRAME_BUDGET_MS,
        );
        row(
            "scroll frame (worst)",
            total,
            max(&scroll_samples),
            FRAME_BUDGET_MS * 2.0,
        );

        // Idle frames on the same screen (layout cache hits).
        let idle: Vec<Duration> = (0..60).map(|_| pane.frame(Vec::new())).collect();
        row("idle frame (median)", total, median(idle), FRAME_BUDGET_MS);

        // Keystrokes in the middle of the file: edit, incremental reparse
        // and highlight of the screen, then paint.
        let middle = pane.buffer.document.rope().line_to_char(total / 2);
        pane.view.set_caret(&pane.buffer.document, middle, true);
        pane.frame(Vec::new());
        let keys: Vec<Duration> = (0..60)
            .map(|i| {
                let c = if i % 8 == 7 { " " } else { "x" };
                pane.frame(vec![Event::Text(c.to_string())])
            })
            .collect();
        row(
            "keystroke frame (median)",
            total,
            median(keys.clone()),
            FRAME_BUDGET_MS,
        );
        row(
            "keystroke frame (worst)",
            total,
            max(&keys),
            FRAME_BUDGET_MS * 2.0,
        );
        println!("{}", "-".repeat(72));
    }
}
