//! The minimap through real egui frames: blocks drawn on other threads, a
//! click centers the line, dragging scrolls, and the marks land on their
//! lines.

use std::path::Path;
use std::time::{Duration, Instant};

use eframe::egui::{self, epaint, Event, PointerButton, Pos2, RawInput};
use phpide_core::Buffer;
use phpide_editor::decorations::{Decorations, Diagnostic, DiagnosticSeverity, GitMark};
use phpide_editor::{
    Editor, EditorOptions, EditorOutput, EditorStyle, EditorView, MinimapOptions, Syntax,
};

const SIZE: egui::Vec2 = egui::vec2(900.0, 600.0);
/// The minimap column: 100 points left of the 12-point scrollbar.
const MINIMAP_LEFT: f32 = 900.0 - 12.0 - 100.0;

struct Harness {
    ctx: egui::Context,
    style: EditorStyle,
    buffer: Buffer,
    view: EditorView,
    syntax: Syntax,
    time: f64,
    shapes: Vec<epaint::ClippedShape>,
}

impl Harness {
    fn new(lines: usize) -> Self {
        let text: String = (0..lines)
            .map(|i| format!("    $value{i} = compute({i}) + $other;\n"))
            .collect();
        let mut buffer = Buffer::untitled();
        buffer.replace_all(&format!("<?php\n{text}"));
        let mut syntax = Syntax::for_path(Some(Path::new("big.php")));
        syntax.wait_idle(&buffer.document);
        Harness {
            ctx: egui::Context::default(),
            style: EditorStyle::from_visuals(&egui::Visuals::dark(), 14.0),
            buffer,
            view: EditorView::new(),
            syntax,
            time: 0.0,
            shapes: Vec::new(),
        }
    }

    fn frame(&mut self, events: Vec<Event>, decorations: &Decorations) -> EditorOutput {
        self.time += 1.0 / 60.0;
        let raw = RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, SIZE)),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        let options = EditorOptions {
            minimap: MinimapOptions {
                enabled: true,
                ..MinimapOptions::default()
            },
            blink: false,
            ..EditorOptions::default()
        };
        let mut output = None;
        let Harness {
            ctx,
            style,
            buffer,
            view,
            syntax,
            ..
        } = self;
        let mut full = ctx.run_ui(raw, |ui| {
            output = Some(
                Editor::new("minimap", buffer, view, syntax, style, decorations)
                    .options(options.clone())
                    .show(ui),
            );
        });
        full.textures_delta.clear();
        self.shapes = full.shapes;
        output.unwrap()
    }

    /// Frames until the minimap blocks on screen are drawn and shown.
    fn settle(&mut self, decorations: &Decorations) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.frame(Vec::new(), decorations);
            if self.view.minimap_idle() && self.images() > 0 {
                self.frame(Vec::new(), decorations);
                return;
            }
            assert!(Instant::now() < deadline, "the minimap was not drawn");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Textured meshes painted: the minimap blocks.
    fn images(&self) -> usize {
        self.shapes
            .iter()
            .filter(|s| matches!(&s.shape, epaint::Shape::Mesh(m) if m.texture_id != egui::TextureId::default()))
            .count()
    }

    fn rects_of(&self, color: egui::Color32) -> Vec<egui::Rect> {
        self.shapes
            .iter()
            .filter_map(|s| match &s.shape {
                epaint::Shape::Rect(r) if r.fill == color => Some(r.rect),
                _ => None,
            })
            .collect()
    }

    fn press(&mut self, at: Pos2, decorations: &Decorations) {
        self.frame(
            vec![
                Event::PointerMoved(at),
                Event::PointerButton {
                    pos: at,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
            decorations,
        );
    }

    fn release(&mut self, at: Pos2, decorations: &Decorations) {
        self.frame(
            vec![Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            decorations,
        );
    }
}

#[test]
fn the_minimap_draws_the_file_and_its_marks() {
    let mut h = Harness::new(2000);
    let error_at = h.buffer.document.rope().line_to_char(40);
    let decorations = Decorations {
        diagnostics: vec![Diagnostic {
            start: error_at + 4,
            end: error_at + 10,
            severity: DiagnosticSeverity::Error,
            message: "broken".into(),
        }],
        git: vec![(60, GitMark::Added)],
        ..Decorations::default()
    };
    h.settle(&decorations);
    assert!(h.images() >= 1, "block images are painted");

    // The error on line 40 sits on the minimap's right edge at 2 points
    // per line; the Git mark of line 60 on its left edge.
    let c = h.style.colors.clone();
    let error = h
        .rects_of(c.error)
        .into_iter()
        .find(|r| r.left() > MINIMAP_LEFT)
        .expect("the error mark");
    assert!((error.top() - (40.0 * 2.0 - 0.5)).abs() < 0.01, "{error:?}");
    assert!((error.right() - (MINIMAP_LEFT + 100.0)).abs() < 0.01);
    let git = h
        .rects_of(c.git_added)
        .into_iter()
        .find(|r| r.left() >= MINIMAP_LEFT)
        .expect("the Git mark");
    assert_eq!((git.left(), git.top()), (MINIMAP_LEFT, 120.0));
    // With the minimap on, the scrollbar has no marks.
    assert!(h
        .rects_of(c.error)
        .iter()
        .all(|r| r.left() > MINIMAP_LEFT && r.right() <= MINIMAP_LEFT + 100.0 + 0.01));
}

#[test]
fn a_click_centers_the_line_and_dragging_scrolls() {
    let mut h = Harness::new(2000);
    let none = Decorations::default();
    let out = h.frame(Vec::new(), &none);
    let row = out.caret_rect.map(|r| r.height() + 4.0).expect("caret");
    h.settle(&none);
    assert_eq!(h.view.scroll.y, 0.0);

    // Dragging the slider (at the top) down by 45 points moves far: the
    // slider travels the panel while the editor travels the whole file.
    let grab = Pos2::new(MINIMAP_LEFT + 40.0, 5.0);
    h.press(grab, &none);
    let to = Pos2::new(grab.x, grab.y + 45.0);
    h.frame(vec![Event::PointerMoved(to)], &none);
    h.release(to, &none);
    assert!(h.view.scroll.y > 45.0 * row, "{}", h.view.scroll.y);

    // Back at the top, the minimap shows line 250 at y = 500: a click
    // there centers it.
    h.view.scroll.y = 0.0;
    h.frame(Vec::new(), &none);
    let at = Pos2::new(MINIMAP_LEFT + 40.0, 500.0);
    h.press(at, &none);
    h.release(at, &none);
    let centre = (h.view.scroll.y + SIZE.y * 0.5) / row;
    assert!((centre - 250.0).abs() < 2.0, "line {centre} in the middle");

    // An edit redraws the block and the minimap settles again.
    let at = h.buffer.document.rope().line_to_char(10);
    h.buffer.insert(at, "x");
    h.view.scroll.y = 0.0;
    h.settle(&none);
}
