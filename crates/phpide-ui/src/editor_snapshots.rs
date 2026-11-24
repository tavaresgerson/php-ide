//! Snapshots of the editor with the real themes: syntax colors, folds,
//! multi-cursor, find bar and every gutter layer, so a visual regression
//! shows up as a failing test (review images under `target/snapshots/`).

use std::path::Path;

use eframe::egui;
use phpide_core::Buffer;
use phpide_editor::decorations::{
    BreakpointMark, CoverageMark, Decorations, Diagnostic, DiagnosticSeverity, GitMark, InlayHint,
};
use phpide_editor::{Editor, EditorOptions, EditorView, Range, Selection, Syntax, ViewCommand};

use crate::design::snapshot;
use crate::design::{install, Density, Theme, ThemeId};

const SOURCE: &str = include_str!("../tests/fixtures/OrderService.php");

fn char_of(buffer: &Buffer, needle: &str, nth: usize) -> usize {
    let text = buffer.document.text();
    let byte = text
        .match_indices(needle)
        .nth(nth)
        .unwrap_or_else(|| panic!("{needle} #{nth} not in the fixture"))
        .0;
    text[..byte].chars().count()
}

fn render(theme: ThemeId) -> snapshot::Image {
    let ctx = egui::Context::default();
    install(&ctx, Theme::new(theme, Density::Comfortable));
    let style = crate::design::current(&ctx).editor_style();

    let mut buffer = Buffer::untitled();
    buffer.replace_all(SOURCE);
    let mut syntax = Syntax::for_path(Some(Path::new("OrderService.php")));
    syntax.wait_idle(&buffer.document);
    let _ = syntax.structure(&buffer.document);
    syntax.wait_idle(&buffer.document);
    let mut view = EditorView::new();

    // Fold the constructor (header on line 20).
    let header = buffer.document.rope().line_to_char(19) + 8;
    view.set_caret(&buffer.document, header, false);
    assert!(view.run(ViewCommand::Fold, &mut buffer, &mut syntax));

    // Find bar with a query.
    assert!(view.run(ViewCommand::Find, &mut buffer, &mut syntax));
    view.find.query = "order".into();

    // Three carets on `$order` and a selection on `MAX_ITEMS`.
    let carets = vec![
        Range::point(char_of(&buffer, "$order =", 0) + 6),
        Range::point(char_of(&buffer, "$order->add", 0) + 6),
        Range::point(char_of(&buffer, "save($order)", 0) + 11),
        Range::new(
            char_of(&buffer, "MAX_ITEMS", 1),
            char_of(&buffer, "MAX_ITEMS", 1) + 9,
        ),
    ];
    view.set_selection(&mut buffer, Selection::new(carets, 0));

    let line = |l: usize| buffer.document.rope().line_to_char(l);
    let throw = char_of(&buffer, "\\InvalidArgumentException", 0);
    let decorations = Decorations {
        breakpoints: vec![
            (31, BreakpointMark::Verified),
            (36, BreakpointMark::Disabled),
        ],
        bookmarks: vec![40],
        git: vec![
            (36, GitMark::Added),
            (37, GitMark::Added),
            (41, GitMark::Modified),
            (46, GitMark::Deleted),
        ],
        coverage: vec![
            (30, CoverageMark::Full),
            (31, CoverageMark::Partial),
            (32, CoverageMark::None),
        ],
        diagnostics: vec![Diagnostic {
            start: throw,
            end: throw + 25,
            severity: DiagnosticSeverity::Warning,
            message: "Unhandled exception".into(),
        }],
        inlay_hints: vec![InlayHint {
            position: char_of(&buffer, "$item['sku']", 0),
            label: "sku:".into(),
        }],
        occurrences: vec![],
        execution_line: Some(41),
        ..Decorations::default()
    };
    let _ = line;
    let options = EditorOptions {
        blink: false,
        ..EditorOptions::default()
    };
    let size = egui::vec2(980.0, 760.0);
    let id = Editor::widget_id("snapshot");
    snapshot::render(&ctx, size, 4, |ui| {
        ui.memory_mut(|m| m.request_focus(id));
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ui, |ui| {
                Editor::new(
                    "snapshot",
                    &mut buffer,
                    &mut view,
                    &mut syntax,
                    &style,
                    &decorations,
                )
                .options(options.clone())
                .show(ui);
            });
    })
}

#[test]
fn editor_snapshots() {
    for theme in [ThemeId::Dark, ThemeId::Light] {
        let image = render(theme);
        snapshot::assert_snapshot(&format!("editor-{}", theme.name()), &image);
    }
}
