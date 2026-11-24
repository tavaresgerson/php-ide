//! An editor view: selection, scroll, folds and find state of one pane
//! showing a document. Two views of the same buffer (split) each keep their
//! own; edits from one reach the other through the document change log.

pub mod find;
pub mod folds;
pub mod layout;
mod widget;

use std::time::Instant;

use eframe::egui;
use phpide_core::{Buffer, Document, Revision};

use crate::edit::{self, Edit, EditConfig};
use crate::movement::LineVisibility;
use crate::selection::{Range, Selection};
use crate::syntax::Syntax;
use find::FindState;
use folds::{DisplayMap, Folds};
use layout::LayoutCache;

pub use widget::{Editor, EditorOptions, EditorOutput, GutterClick};

/// Commands the application routes to the focused view (menus, shortcuts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewCommand {
    Undo,
    Redo,
    SelectAll,
    ToggleLineComment,
    ToggleBlockComment,
    Duplicate,
    DeleteLines,
    MoveLinesUp,
    MoveLinesDown,
    Indent,
    Outdent,
    SelectNextOccurrence,
    UnselectOccurrence,
    SelectAllOccurrences,
    CaretAbove,
    CaretBelow,
    Fold,
    Unfold,
    FoldAll,
    UnfoldAll,
    Find,
    Replace,
    FindNext,
    FindPrevious,
}

impl ViewCommand {
    /// Whether the command changes the text (refused on read-only buffers).
    pub fn edits(self) -> bool {
        matches!(
            self,
            ViewCommand::Undo
                | ViewCommand::Redo
                | ViewCommand::ToggleLineComment
                | ViewCommand::ToggleBlockComment
                | ViewCommand::Duplicate
                | ViewCommand::DeleteLines
                | ViewCommand::MoveLinesUp
                | ViewCommand::MoveLinesDown
                | ViewCommand::Indent
                | ViewCommand::Outdent
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reveal {
    /// Scroll just enough to show the primary caret.
    Caret,
    /// Put the primary caret in the middle (jumps from elsewhere).
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum DragKind {
    /// Extending by chars, words (double click) or lines (triple click).
    Text {
        granularity: u8,
        origin: Range,
    },
    /// Alt/middle drag: box selection from (line, cell).
    Column {
        line: usize,
        cell: usize,
    },
    Scrollbar {
        grab: f32,
    },
}

pub struct EditorView {
    pub selection: Selection,
    seen_revision: Option<Revision>,
    /// Scroll offset in points (x: horizontal, y: rows × row height).
    pub scroll: egui::Vec2,
    pub folds: Folds,
    pub config: EditConfig,
    pub find: FindState,
    pub(crate) layouts: LayoutCache,
    pub(crate) reveal: Option<Reveal>,
    pub(crate) blink: Instant,
    pub(crate) drag: Option<DragKind>,
    /// Alt+J matches whole words (it started from a caret).
    pub(crate) whole_word: bool,
    /// Text copied without a selection (whole lines): pasting it goes
    /// above the caret line.
    pub(crate) line_clipboard: Option<String>,
    pub(crate) preedit: Option<String>,
    pub(crate) page_rows: usize,
    /// Hit areas painted last frame: fold placeholders and sticky lines.
    pub(crate) fold_chips: Vec<(egui::Rect, usize)>,
    pub(crate) sticky_rows: Vec<(egui::Rect, usize)>,
    pub(crate) last_press: Option<(Instant, egui::Pos2, u8)>,
}

impl Default for EditorView {
    fn default() -> Self {
        EditorView::new()
    }
}

impl EditorView {
    pub fn new() -> Self {
        EditorView {
            selection: Selection::point(0),
            seen_revision: None,
            scroll: egui::Vec2::ZERO,
            folds: Folds::default(),
            config: EditConfig::default(),
            find: FindState::default(),
            layouts: LayoutCache::default(),
            reveal: None,
            blink: Instant::now(),
            drag: None,
            whole_word: false,
            line_clipboard: None,
            preedit: None,
            page_rows: 30,
            fold_chips: Vec::new(),
            sticky_rows: Vec::new(),
            last_press: None,
        }
    }

    /// Follows edits made outside this view (other split, undo, LSP edits,
    /// reload from disk).
    pub fn sync(&mut self, doc: &Document) {
        let revision = doc.revision();
        match self.seen_revision {
            Some(seen) if seen == revision => return,
            Some(seen) => match doc.changes_since(seen) {
                Some(changes) => {
                    for change in changes {
                        self.selection = self.selection.map(&change.changes);
                        self.folds.map(&change.changes);
                    }
                }
                None => {
                    self.folds.clear();
                }
            },
            None => {}
        }
        self.selection = self.selection.clamp(doc.len_chars());
        self.seen_revision = Some(revision);
    }

    pub fn primary_caret(&self) -> usize {
        self.selection.primary().head
    }

    pub fn display_map(&self, doc: &Document) -> DisplayMap {
        DisplayMap::new(doc.rope(), &self.folds)
    }

    /// Moves the caret, unfolding what hides it, and scrolls to it.
    pub fn set_caret(&mut self, doc: &Document, pos: usize, center: bool) {
        self.sync(doc);
        let pos = pos.min(doc.len_chars());
        self.selection = Selection::point(pos);
        self.folds.reveal(doc.rope(), pos);
        self.reveal = Some(if center {
            Reveal::Center
        } else {
            Reveal::Caret
        });
        self.blink = Instant::now();
    }

    /// Caret at a 0-based line and column (clamped to the line).
    pub fn goto_line(&mut self, doc: &Document, line: usize, column: usize) {
        let rope = doc.rope();
        let line = line.min(rope.len_lines().saturating_sub(1));
        let end = crate::textutil::line_end(rope, line);
        let pos = (rope.line_to_char(line) + column).min(end);
        self.set_caret(doc, pos, true);
    }

    /// Replaces the selection (breaks the undo group when it moves).
    pub fn set_selection(&mut self, buffer: &mut Buffer, selection: Selection) {
        if selection != self.selection {
            buffer.break_undo_group();
        }
        self.selection = selection.clamp(buffer.document.len_chars());
        self.reveal = Some(Reveal::Caret);
        self.blink = Instant::now();
    }

    /// Applies an edit made in this view. Returns false on read-only buffers.
    pub fn apply(&mut self, buffer: &mut Buffer, edit: Edit) -> bool {
        if buffer.readonly {
            return false;
        }
        self.sync(&buffer.document);
        let before = self.selection.to_carets();
        if !edit.changes.is_empty() {
            self.folds.map(&edit.changes);
            buffer.edit(edit.changes, edit.kind, before, edit.selection.to_carets());
            self.seen_revision = Some(buffer.document.revision());
        }
        self.selection = edit.selection.clamp(buffer.document.len_chars());
        let caret = self.selection.primary().head;
        self.folds.reveal(buffer.document.rope(), caret);
        self.reveal = Some(Reveal::Caret);
        self.blink = Instant::now();
        true
    }

    fn restore(&mut self, buffer: &mut Buffer, carets: Option<phpide_core::Carets>) -> bool {
        let Some(carets) = carets else {
            return false;
        };
        self.sync(&buffer.document);
        if let Some(selection) = Selection::from_carets(&carets) {
            self.selection = selection.clamp(buffer.document.len_chars());
        }
        let caret = self.selection.primary().head;
        self.folds.reveal(buffer.document.rope(), caret);
        self.reveal = Some(Reveal::Caret);
        true
    }

    /// Runs a command; returns whether it did something.
    pub fn run(&mut self, command: ViewCommand, buffer: &mut Buffer, syntax: &mut Syntax) -> bool {
        self.sync(&buffer.document);
        let rope = buffer.document.rope().clone();
        let config = self.config.clone();
        let edit = |view: &mut EditorView, buffer: &mut Buffer, edit: Option<Edit>| match edit {
            Some(edit) => view.apply(buffer, edit),
            None => false,
        };
        match command {
            ViewCommand::Undo => {
                if buffer.readonly {
                    return false;
                }
                let carets = buffer.undo();
                self.restore(buffer, carets)
            }
            ViewCommand::Redo => {
                if buffer.readonly {
                    return false;
                }
                let carets = buffer.redo();
                self.restore(buffer, carets)
            }
            ViewCommand::SelectAll => {
                self.set_selection(buffer, edit::select_all(&rope));
                self.reveal = None;
                true
            }
            ViewCommand::ToggleLineComment => {
                let e = edit::toggle_line_comment(&rope, &self.selection, &config);
                edit(self, buffer, e)
            }
            ViewCommand::ToggleBlockComment => {
                let e = edit::toggle_block_comment(&rope, &self.selection, &config);
                edit(self, buffer, e)
            }
            ViewCommand::Duplicate => {
                let e = edit::duplicate(&rope, &self.selection);
                edit(self, buffer, Some(e))
            }
            ViewCommand::DeleteLines => {
                let e = edit::delete_lines(&rope, &self.selection);
                edit(self, buffer, Some(e))
            }
            ViewCommand::MoveLinesUp | ViewCommand::MoveLinesDown => {
                let e =
                    edit::move_lines(&rope, &self.selection, command == ViewCommand::MoveLinesUp);
                edit(self, buffer, e)
            }
            ViewCommand::Indent => {
                let e = edit::indent_lines(&rope, &self.selection, &config);
                edit(self, buffer, Some(e))
            }
            ViewCommand::Outdent => {
                let e = edit::outdent_lines(&rope, &self.selection, &config);
                edit(self, buffer, Some(e))
            }
            ViewCommand::SelectNextOccurrence => {
                let whole = self.whole_word || self.selection.primary().is_empty();
                match edit::add_next_occurrence(&rope, &self.selection, whole) {
                    Some((selection, whole_word)) => {
                        self.whole_word = whole_word;
                        self.set_selection(buffer, selection);
                        true
                    }
                    None => false,
                }
            }
            ViewCommand::UnselectOccurrence => {
                if self.selection.len() < 2 {
                    return false;
                }
                let selection = self.selection.clone().remove_primary();
                self.set_selection(buffer, selection);
                true
            }
            ViewCommand::SelectAllOccurrences => {
                match edit::select_all_occurrences(&rope, &self.selection) {
                    Some(selection) => {
                        self.set_selection(buffer, selection);
                        true
                    }
                    None => false,
                }
            }
            ViewCommand::CaretAbove | ViewCommand::CaretBelow => {
                let map = self.display_map(&buffer.document);
                let selection = edit::add_caret_vertical(
                    &rope,
                    &self.selection,
                    command == ViewCommand::CaretAbove,
                    config.tab_size,
                    &map,
                );
                self.set_selection(buffer, selection);
                true
            }
            ViewCommand::Fold => {
                let Some(structure) = syntax.structure(&buffer.document) else {
                    return false;
                };
                let line = rope.char_to_line(self.primary_caret());
                let map = self.display_map(&buffer.document);
                let target = structure
                    .folds
                    .iter()
                    .filter(|f| f.start_line0 <= line && line <= f.end_line0)
                    .filter(|f| !self.folds.is_folded(&rope, f.start_line0))
                    .filter(|f| map.is_visible(f.start_line0))
                    .min_by_key(|f| f.end_line0 - f.start_line0);
                match target {
                    Some(f) => {
                        self.folds.fold(&rope, f.start_line0, f.end_line0);
                        let start = rope.line_to_char(f.start_line0);
                        let caret = self.primary_caret();
                        if rope.char_to_line(caret) != f.start_line0 {
                            self.selection = Selection::point(
                                start
                                    + crate::textutil::indentation(&rope, f.start_line0)
                                        .chars()
                                        .count(),
                            );
                        }
                        true
                    }
                    None => false,
                }
            }
            ViewCommand::Unfold => {
                let line = rope.char_to_line(self.primary_caret());
                self.folds.unfold_header(&rope, line)
            }
            ViewCommand::FoldAll => {
                let Some(structure) = syntax.structure(&buffer.document) else {
                    return false;
                };
                for f in structure.folds.iter().filter(|f| f.kind != "namespace") {
                    self.folds.fold(&rope, f.start_line0, f.end_line0);
                }
                let caret = self.primary_caret();
                let line = rope.char_to_line(caret);
                let map = self.display_map(&buffer.document);
                if !map.is_visible(line) {
                    let row = map.row_of_line(line);
                    let header = map.line_of_row(row);
                    self.selection = Selection::point(rope.line_to_char(header));
                }
                self.reveal = Some(Reveal::Caret);
                true
            }
            ViewCommand::UnfoldAll => {
                let had = !self.folds.is_empty();
                self.folds.clear();
                had
            }
            ViewCommand::Find | ViewCommand::Replace => {
                self.find.open = true;
                self.find.replace_open = command == ViewCommand::Replace;
                self.find.focus_query = true;
                let primary = self.selection.primary();
                if !primary.is_empty()
                    && rope.char_to_line(primary.start()) == rope.char_to_line(primary.end())
                {
                    self.find.query = rope.slice(primary.start()..primary.end()).to_string();
                }
                true
            }
            ViewCommand::FindNext | ViewCommand::FindPrevious => {
                self.find_step(buffer, command == ViewCommand::FindNext)
            }
        }
    }

    /// Selects the next (or previous) find match.
    pub fn find_step(&mut self, buffer: &mut Buffer, forward: bool) -> bool {
        if self.find.query.is_empty() {
            self.find.open = true;
            self.find.focus_query = true;
            return true;
        }
        self.sync(&buffer.document);
        let rope = buffer.document.rope().clone();
        self.find.refresh(&rope, buffer.document.revision());
        let primary = self.selection.primary();
        let from = if forward {
            primary.end()
        } else {
            primary.start()
        };
        match self.find.step(from, forward) {
            Some((start, end)) => {
                self.set_selection(buffer, Selection::single(start, end));
                self.folds.reveal(&rope, start);
                self.reveal = Some(Reveal::Center);
                true
            }
            None => false,
        }
    }

    /// Replaces the current find match (or all of them).
    pub fn replace(&mut self, buffer: &mut Buffer, all: bool) -> usize {
        if buffer.readonly {
            return 0;
        }
        self.sync(&buffer.document);
        let rope = buffer.document.rope().clone();
        self.find.refresh(&rope, buffer.document.revision());
        let changes = self.find.replacement_changes(&rope, all);
        let count = changes.changes().len();
        if count == 0 {
            return 0;
        }
        let caret = changes.map_pos(self.selection.primary().end(), phpide_core::Assoc::After);
        let edit = Edit {
            selection: Selection::point(caret),
            changes,
            kind: phpide_core::EditKind::Other,
        };
        self.apply(buffer, edit);
        if !all {
            self.find
                .refresh(buffer.document.rope(), buffer.document.revision());
            let from = self.primary_caret();
            if let Some((start, end)) = self.find.step(from, true) {
                self.selection = Selection::single(start, end);
                self.reveal = Some(Reveal::Center);
            }
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(text: &str) -> Buffer {
        let mut buffer = Buffer::untitled();
        buffer.replace_all(text);
        buffer
    }

    #[test]
    fn two_views_follow_each_others_edits() {
        let mut buffer = buffer("one\ntwo\nthree");
        let mut left = EditorView::new();
        let mut right = EditorView::new();
        left.set_caret(&buffer.document, 4, false);
        right.set_caret(&buffer.document, 8, false);
        let rope = buffer.document.rope().clone();
        let e = edit::insert_text(&rope, &left.selection, "X", &left.config);
        left.apply(&mut buffer, e);
        right.sync(&buffer.document);
        assert_eq!(buffer.document.text(), "one\nXtwo\nthree");
        assert_eq!(left.primary_caret(), 5);
        assert_eq!(
            right.primary_caret(),
            9,
            "the other view's caret moved with the text"
        );
    }

    #[test]
    fn undo_restores_carets_and_redo_reapplies() {
        let mut buffer = buffer("abc");
        let mut syntax = Syntax::for_path(None);
        let mut view = EditorView::new();
        view.set_caret(&buffer.document, 3, false);
        for c in ["d", "e"] {
            let rope = buffer.document.rope().clone();
            let e = edit::insert_text(&rope, &view.selection, c, &view.config);
            view.apply(&mut buffer, e);
        }
        assert_eq!(buffer.document.text(), "abcde");
        assert!(view.run(ViewCommand::Undo, &mut buffer, &mut syntax));
        assert_eq!(buffer.document.text(), "abc", "typing is one undo step");
        assert_eq!(view.primary_caret(), 3);
        assert!(view.run(ViewCommand::Redo, &mut buffer, &mut syntax));
        assert_eq!(view.primary_caret(), 5);
        buffer.readonly = true;
        assert!(!view.run(ViewCommand::Undo, &mut buffer, &mut syntax));
    }

    #[test]
    fn fold_command_uses_the_syntax_structure() {
        let text =
            "<?php\nclass A\n{\n    public function f()\n    {\n        return 1;\n    }\n}\n";
        let mut buffer = buffer(text);
        let mut syntax = Syntax::for_path(None);
        syntax.wait_idle(&buffer.document);
        let mut view = EditorView::new();
        view.goto_line(&buffer.document, 5, 8);
        assert!(view.run(ViewCommand::Fold, &mut buffer, &mut syntax));
        let map = view.display_map(&buffer.document);
        assert!(!map.is_visible(5));
        assert!(map.is_visible(3), "the method header stays");
        assert_eq!(buffer.document.rope().char_to_line(view.primary_caret()), 3);
        assert!(view.run(ViewCommand::FoldAll, &mut buffer, &mut syntax));
        assert!(!view.display_map(&buffer.document).is_visible(3));
        assert!(view.run(ViewCommand::UnfoldAll, &mut buffer, &mut syntax));
        assert!(view.display_map(&buffer.document).is_visible(5));
    }

    #[test]
    fn find_next_and_replace() {
        let mut buffer = buffer("foo bar foo");
        let mut syntax = Syntax::for_path(None);
        let mut view = EditorView::new();
        view.find.query = "foo".into();
        assert!(view.run(ViewCommand::FindNext, &mut buffer, &mut syntax));
        assert_eq!(view.selection.primary(), Range::new(0, 3));
        assert!(view.run(ViewCommand::FindNext, &mut buffer, &mut syntax));
        assert_eq!(view.selection.primary(), Range::new(8, 11));
        view.find.replacement = "baz".into();
        assert_eq!(view.replace(&mut buffer, true), 2);
        assert_eq!(buffer.document.text(), "baz bar baz");
        assert!(view.run(ViewCommand::Undo, &mut buffer, &mut syntax));
        assert_eq!(
            buffer.document.text(),
            "foo bar foo",
            "replace all is one undo step"
        );
    }
}
