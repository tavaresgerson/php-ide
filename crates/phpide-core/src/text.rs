//! Text changes: change sets in char offsets, their inverse, position
//! mapping, and the byte/point form tree-sitter and LSP consumers need.
//!
//! A [`ChangeSet`] is a list of non-overlapping replacements expressed in
//! offsets of the document *before* the change. Applying it produces an
//! [`AppliedChange`] that also records each edit in bytes and (row, byte
//! column) points, in application order, so incremental parsers can replay
//! it with `Tree::edit`.

use ropey::Rope;

/// One replacement: chars `start..end` of the original text become `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

impl Change {
    pub fn new(start: usize, end: usize, text: impl Into<String>) -> Self {
        debug_assert!(start <= end);
        Change {
            start,
            end,
            text: text.into(),
        }
    }

    pub fn insert(at: usize, text: impl Into<String>) -> Self {
        Change::new(at, at, text)
    }

    pub fn delete(start: usize, end: usize) -> Self {
        Change::new(start, end, String::new())
    }

    fn inserted_chars(&self) -> usize {
        self.text.chars().count()
    }
}

/// Which side a position sticks to when text is inserted exactly at it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assoc {
    /// Stays before the inserted text.
    Before,
    /// Moves after the inserted text (carets after typing).
    After,
}

/// Sorted, non-overlapping replacements over one document state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeSet {
    changes: Vec<Change>,
}

impl ChangeSet {
    /// Builds a change set. Changes are sorted; inserts at the same offset
    /// are concatenated in the given order. Overlapping ranges are merged
    /// into one replacement covering both.
    pub fn new(mut changes: Vec<Change>) -> Self {
        changes.retain(|c| c.start < c.end || !c.text.is_empty());
        changes.sort_by_key(|c| (c.start, c.end));
        let mut merged: Vec<Change> = Vec::with_capacity(changes.len());
        for change in changes {
            match merged.last_mut() {
                // Overlap, or an insertion followed by more at the same offset.
                Some(last)
                    if change.start < last.end
                        || (change.start == last.start && last.start == last.end) =>
                {
                    last.end = last.end.max(change.end);
                    last.text.push_str(&change.text);
                }
                _ => merged.push(change),
            }
        }
        ChangeSet { changes: merged }
    }

    pub fn single(start: usize, end: usize, text: impl Into<String>) -> Self {
        ChangeSet::new(vec![Change::new(start, end, text)])
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn changes(&self) -> &[Change] {
        &self.changes
    }

    /// Where `pos` (an offset before the change) ends up after it.
    pub fn map_pos(&self, pos: usize, assoc: Assoc) -> usize {
        let mut delta: isize = 0;
        for change in &self.changes {
            if pos < change.start {
                break;
            }
            let start = change.start as isize + delta;
            let inserted = change.inserted_chars() as isize;
            if change.start == change.end {
                if pos == change.start {
                    return match assoc {
                        Assoc::Before => start as usize,
                        Assoc::After => (start + inserted) as usize,
                    };
                }
                delta += inserted;
                continue;
            }
            if pos < change.end {
                // Inside (or at the start of) a replaced range.
                return match assoc {
                    Assoc::Before => start as usize,
                    Assoc::After if pos == change.start => start as usize,
                    Assoc::After => (start + inserted) as usize,
                };
            }
            delta += inserted - (change.end - change.start) as isize;
        }
        (pos as isize + delta).max(0) as usize
    }

    /// The change set that undoes this one, given the text it applies to.
    pub fn invert(&self, original: &Rope) -> ChangeSet {
        let mut delta: isize = 0;
        let mut out = Vec::with_capacity(self.changes.len());
        for change in &self.changes {
            let start = (change.start as isize + delta) as usize;
            let inserted = change.inserted_chars();
            out.push(Change::new(
                start,
                start + inserted,
                original.slice(change.start..change.end).to_string(),
            ));
            delta += inserted as isize - (change.end - change.start) as isize;
        }
        ChangeSet { changes: out }
    }

    /// Length of the text after applying, given its length before.
    pub fn new_len(&self, old_len: usize) -> usize {
        let delta: isize = self
            .changes
            .iter()
            .map(|c| c.inserted_chars() as isize - (c.end - c.start) as isize)
            .sum();
        (old_len as isize + delta) as usize
    }
}

/// A point as tree-sitter wants it: row and byte column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BytePoint {
    pub row: usize,
    pub column: usize,
}

/// One edit in byte offsets and points (tree-sitter `InputEdit` shape).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteEdit {
    pub start_byte: usize,
    pub old_end_byte: usize,
    pub new_end_byte: usize,
    pub start_point: BytePoint,
    pub old_end_point: BytePoint,
    pub new_end_point: BytePoint,
}

/// A change set as it was applied, with its byte edits in application
/// order (each relative to the text at that moment).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedChange {
    pub changes: ChangeSet,
    pub byte_edits: Vec<ByteEdit>,
}

fn point_at(rope: &Rope, byte: usize) -> BytePoint {
    let row = rope.byte_to_line(byte);
    BytePoint {
        row,
        column: byte - rope.line_to_byte(row),
    }
}

fn point_after(start: BytePoint, text: &str) -> BytePoint {
    match text.rfind('\n') {
        Some(last) => BytePoint {
            row: start.row + text.matches('\n').count(),
            column: text.len() - last - 1,
        },
        None => BytePoint {
            row: start.row,
            column: start.column + text.len(),
        },
    }
}

/// Applies `changes` to `rope` (last change first, so earlier offsets stay
/// valid) and returns the byte edits in that order.
pub(crate) fn apply(rope: &mut Rope, changes: &ChangeSet) -> Vec<ByteEdit> {
    let mut edits = Vec::with_capacity(changes.changes.len());
    for change in changes.changes.iter().rev() {
        let start_byte = rope.char_to_byte(change.start);
        let old_end_byte = rope.char_to_byte(change.end);
        let start_point = point_at(rope, start_byte);
        let old_end_point = point_at(rope, old_end_byte);
        if change.end > change.start {
            rope.remove(change.start..change.end);
        }
        if !change.text.is_empty() {
            rope.insert(change.start, &change.text);
        }
        edits.push(ByteEdit {
            start_byte,
            old_end_byte,
            new_end_byte: start_byte + change.text.len(),
            start_point,
            old_end_point,
            new_end_point: point_after(start_point, &change.text),
        });
    }
    edits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, set: &ChangeSet) -> String {
        let mut rope = Rope::from_str(text);
        apply(&mut rope, set);
        rope.to_string()
    }

    #[test]
    fn applies_multiple_changes_in_original_offsets() {
        let set = ChangeSet::new(vec![
            Change::insert(5, "!"),
            Change::new(0, 1, "H"),
            Change::delete(6, 11),
        ]);
        assert_eq!(run("hello world", &set), "Hello! ");
    }

    #[test]
    fn invert_restores_the_original() {
        let text = "alpha beta\ngamma";
        let set = ChangeSet::new(vec![
            Change::new(0, 5, "A"),
            Change::insert(10, "\nnew"),
            Change::delete(11, 16),
        ]);
        let rope = Rope::from_str(text);
        let inverse = set.invert(&rope);
        let after = run(text, &set);
        assert_eq!(run(&after, &inverse), text);
        assert_eq!(set.new_len(text.chars().count()), after.chars().count());
    }

    #[test]
    fn inserts_at_the_same_offset_concatenate() {
        let set = ChangeSet::new(vec![Change::insert(1, "a"), Change::insert(1, "b")]);
        assert_eq!(run("xy", &set), "xaby");
    }

    #[test]
    fn map_pos_handles_insertions_and_deletions() {
        let set = ChangeSet::new(vec![Change::insert(2, "abc"), Change::delete(5, 8)]);
        assert_eq!(set.map_pos(0, Assoc::Before), 0);
        assert_eq!(set.map_pos(2, Assoc::Before), 2);
        assert_eq!(set.map_pos(2, Assoc::After), 5);
        assert_eq!(set.map_pos(4, Assoc::After), 7);
        // Inside the deleted range: collapses to its start.
        assert_eq!(set.map_pos(6, Assoc::Before), 8);
        assert_eq!(set.map_pos(8, Assoc::Before), 8);
        assert_eq!(set.map_pos(10, Assoc::Before), 10);
    }

    #[test]
    fn byte_edits_track_rows_and_utf8_columns() {
        let mut rope = Rope::from_str("ação\nfim");
        let edits = apply(&mut rope, &ChangeSet::single(1, 3, "XY\nZ"));
        assert_eq!(rope.to_string(), "aXY\nZo\nfim");
        let edit = edits[0];
        assert_eq!((edit.start_byte, edit.old_end_byte), (1, 5));
        assert_eq!(edit.new_end_byte, 1 + "XY\nZ".len());
        assert_eq!(edit.new_end_point, BytePoint { row: 1, column: 1 });
        assert_eq!(edit.old_end_point, BytePoint { row: 0, column: 5 });
    }
}
