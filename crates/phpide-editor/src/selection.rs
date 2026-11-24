//! Multi-cursor selections in char offsets.

use phpide_core::{Assoc, Carets, ChangeSet};

/// One selection range. `head` is where the caret is; `anchor` is the other
/// end. `goal` keeps the visual column (in cells) for vertical moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub anchor: usize,
    pub head: usize,
    pub goal: Option<usize>,
}

impl Range {
    pub fn new(anchor: usize, head: usize) -> Self {
        Range {
            anchor,
            head,
            goal: None,
        }
    }

    pub fn point(pos: usize) -> Self {
        Range::new(pos, pos)
    }

    pub fn start(&self) -> usize {
        self.anchor.min(self.head)
    }

    pub fn end(&self) -> usize {
        self.anchor.max(self.head)
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    pub fn with_goal(mut self, goal: Option<usize>) -> Self {
        self.goal = goal;
        self
    }

    fn map(self, changes: &ChangeSet) -> Range {
        let (anchor_assoc, head_assoc) = if self.is_empty() {
            (Assoc::After, Assoc::After)
        } else if self.anchor < self.head {
            (Assoc::After, Assoc::Before)
        } else {
            (Assoc::Before, Assoc::After)
        };
        Range::new(
            changes.map_pos(self.anchor, anchor_assoc),
            changes.map_pos(self.head, head_assoc),
        )
    }
}

/// One or more ranges, sorted and non-overlapping, with a primary range
/// (the one the view follows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    ranges: Vec<Range>,
    primary: usize,
}

impl Selection {
    pub fn point(pos: usize) -> Self {
        Selection {
            ranges: vec![Range::point(pos)],
            primary: 0,
        }
    }

    pub fn single(anchor: usize, head: usize) -> Self {
        Selection {
            ranges: vec![Range::new(anchor, head)],
            primary: 0,
        }
    }

    /// Builds a selection; the primary range is `primary` (clamped).
    pub fn new(ranges: Vec<Range>, primary: usize) -> Self {
        let mut selection = Selection {
            primary: primary.min(ranges.len().saturating_sub(1)),
            ranges: if ranges.is_empty() {
                vec![Range::point(0)]
            } else {
                ranges
            },
        };
        selection.normalize();
        selection
    }

    pub fn ranges(&self) -> &[Range] {
        &self.ranges
    }

    pub fn primary(&self) -> Range {
        self.ranges[self.primary]
    }

    pub fn primary_index(&self) -> usize {
        self.primary
    }

    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.iter().all(Range::is_empty)
    }

    /// Adds a range and makes it primary.
    pub fn push(mut self, range: Range) -> Self {
        self.ranges.push(range);
        self.primary = self.ranges.len() - 1;
        self.normalize();
        self
    }

    /// Replaces the primary range.
    pub fn with_primary(mut self, range: Range) -> Self {
        let primary = self.primary;
        self.ranges[primary] = range;
        self.normalize();
        self
    }

    /// Keeps only the primary range.
    pub fn into_primary(self) -> Self {
        Selection {
            ranges: vec![self.primary()],
            primary: 0,
        }
    }

    /// Removes the primary range (when more than one remains); the previous
    /// range becomes primary.
    pub fn remove_primary(mut self) -> Self {
        if self.ranges.len() > 1 {
            self.ranges.remove(self.primary);
            self.primary = self.primary.saturating_sub(1).min(self.ranges.len() - 1);
        }
        self
    }

    /// Applies `f` to every range.
    pub fn transform(&self, mut f: impl FnMut(Range) -> Range) -> Self {
        let mut out = Selection {
            ranges: self.ranges.iter().map(|r| f(*r)).collect(),
            primary: self.primary,
        };
        out.normalize();
        out
    }

    /// Follows the selection through an edit made elsewhere (another view,
    /// an external change).
    pub fn map(&self, changes: &ChangeSet) -> Self {
        self.transform(|r| r.map(changes))
    }

    /// Keeps every offset inside a text of `len` chars.
    pub fn clamp(&self, len: usize) -> Self {
        self.transform(|r| Range::new(r.anchor.min(len), r.head.min(len)).with_goal(r.goal))
    }

    pub fn to_carets(&self) -> Carets {
        // Primary last, so undo restores it as primary.
        let mut carets: Carets = self
            .ranges
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != self.primary)
            .map(|(_, r)| (r.anchor, r.head))
            .collect();
        carets.push((self.primary().anchor, self.primary().head));
        carets
    }

    pub fn from_carets(carets: &[(usize, usize)]) -> Option<Self> {
        if carets.is_empty() {
            return None;
        }
        let ranges: Vec<Range> = carets.iter().map(|&(a, h)| Range::new(a, h)).collect();
        let primary = ranges.len() - 1;
        Some(Selection::new(ranges, primary))
    }

    /// Sorts ranges and merges overlapping ones, keeping the primary.
    fn normalize(&mut self) {
        if self.ranges.len() == 1 {
            self.primary = 0;
            return;
        }
        let mut indexed: Vec<(usize, Range)> = self.ranges.iter().copied().enumerate().collect();
        indexed.sort_by_key(|(_, r)| (r.start(), r.end()));
        let mut merged: Vec<Range> = Vec::with_capacity(indexed.len());
        let mut primary = 0;
        for (index, range) in indexed {
            let is_primary = index == self.primary;
            match merged.last_mut() {
                Some(last)
                    if range.start() < last.end()
                        || (range.start() == last.start() && range.end() == last.end())
                        || (range.is_empty() && range.start() == last.end())
                        || (last.is_empty() && last.start() == range.start()) =>
                {
                    let start = last.start().min(range.start());
                    let end = last.end().max(range.end());
                    let backwards = if is_primary {
                        range.head < range.anchor
                    } else {
                        last.head < last.anchor
                    };
                    *last = if backwards {
                        Range::new(end, start)
                    } else {
                        Range::new(start, end)
                    };
                    if is_primary {
                        primary = merged.len() - 1;
                    }
                }
                _ => {
                    merged.push(range);
                    if is_primary {
                        primary = merged.len() - 1;
                    }
                }
            }
        }
        self.ranges = merged;
        self.primary = primary;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_are_sorted_and_merged_with_primary_kept() {
        let sel = Selection::new(
            vec![Range::new(10, 12), Range::point(1), Range::new(11, 15)],
            2,
        );
        assert_eq!(sel.len(), 2);
        assert_eq!(sel.ranges()[0], Range::point(1));
        assert_eq!((sel.primary().start(), sel.primary().end()), (10, 15));
        assert_eq!(sel.primary_index(), 1);
    }

    #[test]
    fn duplicate_carets_collapse() {
        let sel = Selection::new(vec![Range::point(3), Range::point(3)], 1);
        assert_eq!(sel.len(), 1);
    }

    #[test]
    fn map_follows_edits_before_and_inside() {
        let sel = Selection::new(vec![Range::point(5), Range::new(10, 12)], 0);
        let changes = ChangeSet::single(0, 0, "abc");
        let mapped = sel.map(&changes);
        assert_eq!(mapped.ranges()[0], Range::point(8));
        assert_eq!(mapped.ranges()[1], Range::new(13, 15));
    }

    #[test]
    fn carets_round_trip_with_primary() {
        let sel = Selection::new(vec![Range::point(1), Range::new(4, 6)], 0);
        let back = Selection::from_carets(&sel.to_carets()).unwrap();
        assert_eq!(back.primary(), Range::point(1));
        assert_eq!(back.len(), 2);
    }
}
