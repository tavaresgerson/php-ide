//! Baseline of the real per-frame editor pipeline (Phase 0).
//!
//! The current editor is an `egui::TextEdit` fed by a `String`. On every
//! frame `PhpIdeApp::editor` copies the rope into a `String`, clones it for
//! the widget, re-highlights the whole file with syntect, lays it out and
//! compares the edited text with the original. On every keystroke it also
//! recomputes folding ranges with tree-sitter. This benchmark measures those
//! steps on 2k, 10k and 50k-line PHP files so the new editor (Phase 3) has a
//! number to beat. Budget for one frame at 60 fps: 16.6 ms.
//!
//! Run: `cargo bench -p phpide-ui --bench editor_pipeline`

use std::time::{Duration, Instant};

use eframe::egui;
use phpide_core::fixtures::large_php_document;
use phpide_core::Document;

const SIZES: [usize; 3] = [2_000, 10_000, 50_000];

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

fn measure(iters: usize, mut f: impl FnMut()) -> Duration {
    f(); // warm-up
    let samples = (0..iters)
        .map(|_| {
            let t0 = Instant::now();
            f();
            t0.elapsed()
        })
        .collect();
    median(samples)
}

fn row(name: &str, lines: usize, value: Duration) {
    let ms = value.as_secs_f64() * 1000.0;
    let flag = if ms > 16.6 {
        "  > 16.6 ms frame budget"
    } else {
        ""
    };
    println!("{name:<34} {lines:>6} lines  {ms:>10.2} ms{flag}");
}

/// Lays out `job` inside its own egui pass, like one rendered frame.
/// `warm` repeats the same job in the next pass, which hits egui's galley
/// cache (what happens on frames without edits).
fn layout_in_pass(ctx: &egui::Context, job: &egui::text::LayoutJob, warm: bool) -> Duration {
    let mut elapsed = Duration::ZERO;
    let passes = if warm { 2 } else { 1 };
    for pass in 0..passes {
        ctx.run_ui(egui::RawInput::default(), |ui| {
            let t0 = Instant::now();
            let galley = ui.ctx().fonts_mut(|fonts| fonts.layout_job(job.clone()));
            std::hint::black_box(galley.rows.len());
            if pass + 1 == passes {
                elapsed = t0.elapsed();
            }
        })
        .drop_without_applying_deltas();
    }
    elapsed
}

fn main() {
    let style = egui::Style::default();
    let ctx = egui::Context::default();
    println!("editor pipeline baseline (median)");
    println!("{}", "-".repeat(72));

    for lines in SIZES {
        let text = large_php_document(lines);
        let total = text.lines().count();
        let doc = Document::new(&text);
        let iters = if lines >= 50_000 { 5 } else { 15 };

        let rope_to_string = measure(iters, || {
            std::hint::black_box(doc.text());
        });
        row("rope -> String (Document::text)", total, rope_to_string);

        let clone_and_compare = measure(iters, || {
            let edited = text.clone();
            std::hint::black_box(edited != text);
        });
        row("clone + compare (diff check)", total, clone_and_compare);

        let highlight = measure(iters, || {
            std::hint::black_box(phpide_php::highlight(&text, &style));
        });
        row("syntect highlight, whole file", total, highlight);

        let job = phpide_php::highlight(&text, &style);
        let layout_cold = median(
            (0..iters.min(5))
                .map(|i| {
                    // A different job each time forces a cache miss, as after a keystroke.
                    let mut job = job.clone();
                    job.append(&format!("// {i}\n"), 0.0, Default::default());
                    layout_in_pass(&ctx, &job, false)
                })
                .collect(),
        );
        row("layout, cache miss (after edit)", total, layout_cold);

        let layout_warm = median(
            (0..iters.min(5))
                .map(|_| layout_in_pass(&ctx, &job, true))
                .collect(),
        );
        row("layout, cache hit (idle frame)", total, layout_warm);

        let folding = measure(iters, || {
            std::hint::black_box(phpide_syntax::folding::folding_ranges(&text, total));
        });
        row("tree-sitter folding (per edit)", total, folding);

        let idle_frame = rope_to_string + clone_and_compare + highlight + layout_warm;
        let edit_frame = rope_to_string + clone_and_compare + highlight + layout_cold + folding;
        row("= estimated idle frame", total, idle_frame);
        row("= estimated frame after a keystroke", total, edit_frame);
        println!("{}", "-".repeat(72));
    }
}
