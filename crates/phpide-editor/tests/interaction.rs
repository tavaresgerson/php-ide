//! Interaction tests of the editor widget: real egui frames fed with input
//! events (typing, keys, clipboard), checked against the buffer and view.

use std::path::Path;

use eframe::egui::{self, Event, Key, Modifiers, RawInput};
use phpide_core::Buffer;
use phpide_editor::decorations::Decorations;
use phpide_editor::{
    Editor, EditorOutput, EditorStyle, EditorView, Range, Selection, Syntax, ViewCommand,
};

const SALT: &str = "pane";

struct Harness {
    ctx: egui::Context,
    style: EditorStyle,
    buffer: Buffer,
    view: EditorView,
    syntax: Syntax,
    time: f64,
}

impl Harness {
    fn new(text: &str) -> Self {
        let mut buffer = Buffer::untitled();
        buffer.replace_all(text);
        let ctx = egui::Context::default();
        let style = EditorStyle::from_visuals(&egui::Visuals::dark(), 14.0);
        let mut syntax = Syntax::for_path(Some(Path::new("test.php")));
        syntax.wait_idle(&buffer.document);
        let mut harness = Harness {
            ctx,
            style,
            buffer,
            view: EditorView::new(),
            syntax,
            time: 0.0,
        };
        harness
            .ctx
            .memory_mut(|m| m.request_focus(Editor::widget_id(SALT)));
        harness.frame(Vec::new());
        harness
    }

    fn text(&self) -> String {
        self.buffer.document.text()
    }

    fn frame_with(&mut self, events: Vec<Event>, _modifiers: Modifiers) -> EditorOutput {
        self.frame_full(events).0
    }

    fn frame_full(&mut self, events: Vec<Event>) -> (EditorOutput, egui::FullOutput) {
        self.time += 1.0 / 60.0;
        let raw = RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 600.0),
            )),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        let mut output = None;
        let decorations = Decorations::default();
        let Harness {
            ctx,
            style,
            buffer,
            view,
            syntax,
            ..
        } = self;
        let mut full = ctx.run_ui(raw, |ui| {
            output = Some(Editor::new(SALT, buffer, view, syntax, style, &decorations).show(ui));
        });
        full.textures_delta.clear();
        (output.expect("editor shown"), full)
    }

    fn frame(&mut self, events: Vec<Event>) -> EditorOutput {
        self.frame_with(events, Modifiers::NONE)
    }

    fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.frame(vec![Event::Text(c.to_string())]);
        }
    }

    fn key(&mut self, key: Key, modifiers: Modifiers) -> EditorOutput {
        let event = |pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        };
        self.frame_with(vec![event(true), event(false)], modifiers)
    }

    fn caret(&mut self, pos: usize) {
        self.view.set_caret(&self.buffer.document, pos, false);
    }

    fn run(&mut self, command: ViewCommand) -> bool {
        self.view.run(command, &mut self.buffer, &mut self.syntax)
    }
}

#[test]
fn typing_inserts_at_the_caret_and_auto_indents() {
    let mut h = Harness::new("<?php\n");
    h.caret(6);
    h.type_text("function f() {");
    let out = h.frame(Vec::new());
    assert!(out.focused, "the editor keeps keyboard focus");
    assert_eq!(h.text(), "<?php\nfunction f() {}", "the brace is paired");
    h.key(Key::Enter, Modifiers::NONE);
    assert_eq!(h.text(), "<?php\nfunction f() {\n    \n}");
    h.type_text("return 1;");
    assert_eq!(h.text(), "<?php\nfunction f() {\n    return 1;\n}");
    h.key(Key::Backspace, Modifiers::NONE);
    assert_eq!(h.text(), "<?php\nfunction f() {\n    return 1\n}");
}

#[test]
fn quick_typing_undoes_as_one_group_and_redoes() {
    let mut h = Harness::new("<?php\n");
    h.caret(6);
    h.type_text("$name");
    h.key(Key::Enter, Modifiers::NONE);
    h.type_text("$age");
    assert_eq!(h.text(), "<?php\n$name\n$age");

    assert!(h.run(ViewCommand::Undo));
    assert_eq!(
        h.text(),
        "<?php\n$name\n",
        "the word after Enter is its own group"
    );
    assert!(h.run(ViewCommand::Undo));
    assert_eq!(h.text(), "<?php\n", "the first word undoes at once");
    assert_eq!(
        h.view.primary_caret(),
        6,
        "the caret returns to where typing began"
    );

    assert!(h.run(ViewCommand::Redo));
    assert_eq!(h.text(), "<?php\n$name\n");
    assert_eq!(h.view.primary_caret(), 12);
}

#[test]
fn multi_cursor_typing_and_deleting_edits_every_caret() {
    let mut h = Harness::new("<?php\n$a = 1;\n$a = 2;\n$a = 3;\n");
    h.caret(7); // on `a` of the first `$a`
    assert!(h.run(ViewCommand::SelectAllOccurrences));
    assert_eq!(h.view.selection.len(), 3);
    h.type_text("b");
    assert_eq!(h.text(), "<?php\n$b = 1;\n$b = 2;\n$b = 3;\n");
    h.key(Key::End, Modifiers::NONE);
    h.key(Key::Backspace, Modifiers::NONE);
    assert_eq!(h.text(), "<?php\n$b = 1\n$b = 2\n$b = 3\n");
    h.key(Key::Escape, Modifiers::NONE);
    assert_eq!(
        h.view.selection.len(),
        1,
        "Escape keeps only the primary caret"
    );

    // Column selection: three carets in a column, typed at once.
    let rope = h.buffer.document.rope().clone();
    let carets: Vec<Range> = (1..4)
        .map(|line| Range::point(rope.line_to_char(line)))
        .collect();
    h.view
        .set_selection(&mut h.buffer, Selection::new(carets, 0));
    h.type_text("// ");
    assert_eq!(h.text(), "<?php\n// $b = 1\n// $b = 2\n// $b = 3\n");
    assert!(h.run(ViewCommand::Undo));
    assert_eq!(h.text(), "<?php\n$b = 1\n$b = 2\n$b = 3\n");
}

#[test]
fn clipboard_copies_lines_and_pastes_them_above() {
    let mut h = Harness::new("<?php\none\ntwo\n");
    h.caret(7);
    let (_, full) = h.frame_full(vec![Event::Copy]);
    let copied = full
        .platform_output
        .commands
        .iter()
        .find_map(|c| match c {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_default();
    assert_eq!(copied, "one\n", "copy without a selection takes the line");
    h.caret(11);
    h.frame(vec![Event::Paste("one\n".into())]);
    assert_eq!(
        h.text(),
        "<?php\none\none\ntwo\n",
        "a line paste lands above"
    );
}

#[test]
fn folding_hides_lines_and_arrows_skip_the_fold() {
    let source = "<?php\nfunction a() {\n    $x = 1;\n    $y = 2;\n}\n$z = 3;\n";
    let mut h = Harness::new(source);
    h.caret(7); // header line of `a`
    assert!(h.run(ViewCommand::Fold));
    assert!(h.view.folds.is_folded(h.buffer.document.rope(), 1));
    h.frame(Vec::new());
    let map = h.view.display_map(&h.buffer.document);
    assert_eq!(map.rows(), 4, "lines 2-4 are hidden (rows: 0, 1, 5, 6)");

    h.key(Key::ArrowDown, Modifiers::NONE);
    let line = h
        .buffer
        .document
        .rope()
        .char_to_line(h.view.primary_caret());
    assert_eq!(line, 5, "Down jumps over the folded body");

    // Editing inside the fold's header keeps it; Unfold All opens it.
    h.caret(7);
    assert!(h.run(ViewCommand::UnfoldAll));
    assert_eq!(h.view.display_map(&h.buffer.document).rows(), 7);
}

#[test]
fn split_views_share_the_buffer_and_keep_their_own_carets() {
    let mut h = Harness::new("<?php\nfirst\nsecond\n");
    let mut other = EditorView::new();
    other.set_caret(&h.buffer.document, 13, false); // start of `second`
    h.caret(6);
    h.type_text("X");
    assert_eq!(h.text(), "<?php\nXfirst\nsecond\n");

    // The second pane renders the same buffer in its own frame.
    let raw = RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 600.0),
        )),
        ..Default::default()
    };
    let decorations = Decorations::default();
    let mut full = h.ctx.run_ui(raw, |ui| {
        Editor::new(
            "other",
            &mut h.buffer,
            &mut other,
            &mut h.syntax,
            &h.style,
            &decorations,
        )
        .show(ui);
    });
    full.textures_delta.clear();
    assert_eq!(
        other.primary_caret(),
        14,
        "the other caret moved with the text"
    );
    assert_eq!(h.view.primary_caret(), 7);

    // Undo from the second pane reverts the shared buffer.
    assert!(other.run(ViewCommand::Undo, &mut h.buffer, &mut h.syntax));
    assert_eq!(h.text(), "<?php\nfirst\nsecond\n");
    h.frame(Vec::new());
    assert_eq!(h.view.primary_caret(), 6);
}

#[test]
fn find_and_replace_through_the_view() {
    let mut h = Harness::new("<?php\n$foo = foo();\n$bar = foo();\n");
    h.view.find.query = "foo".into();
    h.view.find.options.whole_word = true;
    assert!(h.run(ViewCommand::FindNext));
    assert_eq!(h.view.find.matches.len(), 3);
    h.view.find.replacement = "baz".into();
    let replaced = h.view.replace(&mut h.buffer, true);
    assert_eq!(replaced, 3);
    assert_eq!(h.text(), "<?php\n$baz = baz();\n$bar = baz();\n");
    assert!(h.run(ViewCommand::Undo));
    assert_eq!(h.text(), "<?php\n$foo = foo();\n$bar = foo();\n");
}

#[test]
fn read_only_buffers_refuse_typing() {
    let mut h = Harness::new("<?php\n");
    h.buffer.readonly = true;
    h.caret(6);
    let out = h.frame(vec![Event::Text("x".into())]);
    assert!(out.blocked_edit);
    assert!(!out.edited);
    assert_eq!(h.text(), "<?php\n");
}
