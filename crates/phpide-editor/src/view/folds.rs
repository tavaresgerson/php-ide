//! Folded regions and the line ↔ row map they produce.
//!
//! A folded region keeps its header line visible and hides the lines after
//! it up to its end. Regions are anchored to char offsets and follow edits,
//! so typing above a fold keeps it in place.

use phpide_core::{Assoc, ChangeSet};
use ropey::Rope;

use crate::movement::LineVisibility;
use crate::textutil::line_end;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Folds {
    /// `(header line start, end line content end)` in chars.
    regions: Vec<(usize, usize)>,
}

impl Folds {
    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    /// Folds lines `start_line + 1 ..= end_line` under `start_line`.
    pub fn fold(&mut self, rope: &Rope, start_line: usize, end_line: usize) {
        if end_line <= start_line || end_line >= rope.len_lines() {
            return;
        }
        let region = (rope.line_to_char(start_line), line_end(rope, end_line));
        if !self.regions.contains(&region) {
            self.regions.push(region);
            self.regions.sort();
        }
    }

    /// Folded `(header, last)` line pairs.
    pub fn lines(&self, rope: &Rope) -> Vec<(usize, usize)> {
        self.regions
            .iter()
            .map(|&(start, end)| {
                let len = rope.len_chars();
                (
                    rope.char_to_line(start.min(len)),
                    rope.char_to_line(end.min(len)),
                )
            })
            .filter(|(a, b)| b > a)
            .collect()
    }

    pub fn is_folded(&self, rope: &Rope, header: usize) -> bool {
        self.lines(rope).iter().any(|(a, _)| *a == header)
    }

    /// Unfolds the region whose header is `line`; returns whether one was.
    pub fn unfold_header(&mut self, rope: &Rope, line: usize) -> bool {
        let before = self.regions.len();
        let len = rope.len_chars();
        self.regions
            .retain(|&(start, _)| rope.char_to_line(start.min(len)) != line);
        before != self.regions.len()
    }

    /// Unfolds every region hiding the char `pos` (the caret got there).
    pub fn reveal(&mut self, rope: &Rope, pos: usize) -> bool {
        let line = rope.char_to_line(pos.min(rope.len_chars()));
        let before = self.regions.len();
        let len = rope.len_chars();
        self.regions.retain(|&(start, end)| {
            let (a, b) = (
                rope.char_to_line(start.min(len)),
                rope.char_to_line(end.min(len)),
            );
            !(line > a && line <= b)
        });
        before != self.regions.len()
    }

    pub fn clear(&mut self) {
        self.regions.clear();
    }

    /// Follows an edit; regions reduced to one line disappear.
    pub fn map(&mut self, changes: &ChangeSet) {
        for region in &mut self.regions {
            region.0 = changes.map_pos(region.0, Assoc::After);
            region.1 = changes.map_pos(region.1, Assoc::Before);
        }
        self.regions.retain(|(a, b)| a < b);
        self.regions.sort();
        self.regions.dedup();
    }
}

/// Which lines are shown, as rows.
#[derive(Debug, Clone, Default)]
pub struct DisplayMap {
    /// Hidden line ranges, sorted and merged.
    hidden: Vec<(usize, usize)>,
    total_lines: usize,
}

impl DisplayMap {
    pub fn new(rope: &Rope, folds: &Folds) -> Self {
        let mut hidden: Vec<(usize, usize)> = folds
            .lines(rope)
            .into_iter()
            .map(|(header, last)| (header + 1, last))
            .collect();
        hidden.sort();
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for (a, b) in hidden {
            match merged.last_mut() {
                Some(last) if a <= last.1 + 1 => last.1 = last.1.max(b),
                _ => merged.push((a, b)),
            }
        }
        DisplayMap {
            hidden: merged,
            total_lines: rope.len_lines(),
        }
    }

    pub fn rows(&self) -> usize {
        self.total_lines - self.hidden.iter().map(|(a, b)| b - a + 1).sum::<usize>()
    }

    /// Row of `line`; a hidden line maps to the row of its fold header.
    pub fn row_of_line(&self, line: usize) -> usize {
        let mut row = line;
        for &(a, b) in &self.hidden {
            if line < a {
                break;
            }
            if line <= b {
                return a - 1 - self.hidden_before(a);
            }
            row -= b - a + 1;
        }
        row
    }

    fn hidden_before(&self, line: usize) -> usize {
        self.hidden
            .iter()
            .take_while(|(_, b)| *b < line)
            .map(|(a, b)| b - a + 1)
            .sum()
    }

    pub fn line_of_row(&self, row: usize) -> usize {
        let mut line = row;
        for &(a, b) in &self.hidden {
            if line < a {
                break;
            }
            line += b - a + 1;
        }
        line.min(self.total_lines.saturating_sub(1))
    }

    /// Whether the line right after `line` is hidden (it is a fold header).
    pub fn folds_after(&self, line: usize) -> bool {
        self.hidden.iter().any(|(a, _)| *a == line + 1)
    }
}

impl LineVisibility for DisplayMap {
    fn is_visible(&self, line: usize) -> bool {
        !self.hidden.iter().any(|(a, b)| line >= *a && line <= *b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rope(lines: usize) -> Rope {
        Rope::from_str(
            &(0..lines)
                .map(|i| format!("line {i}\n"))
                .collect::<String>(),
        )
    }

    #[test]
    fn rows_skip_folded_lines() {
        let rope = rope(10);
        let mut folds = Folds::default();
        folds.fold(&rope, 2, 5);
        let map = DisplayMap::new(&rope, &folds);
        assert_eq!(map.rows(), 11 - 3);
        assert_eq!(map.line_of_row(2), 2);
        assert_eq!(map.line_of_row(3), 6);
        assert_eq!(map.row_of_line(6), 3);
        assert_eq!(map.row_of_line(4), 2, "hidden line maps to its header");
        assert!(!map.is_visible(3));
        assert!(map.folds_after(2));
    }

    #[test]
    fn folds_follow_edits_and_reveal() {
        let mut r = rope(10);
        let mut folds = Folds::default();
        folds.fold(&r, 2, 5);
        let changes = ChangeSet::single(0, 0, "new\n");
        let mut doc = phpide_core::Document::new(&r.to_string());
        doc.apply(&changes);
        folds.map(&changes);
        r = doc.rope().clone();
        assert_eq!(folds.lines(&r), vec![(3, 6)]);
        let inside = r.line_to_char(5);
        assert!(folds.reveal(&r, inside));
        assert!(folds.is_empty());
    }
}
