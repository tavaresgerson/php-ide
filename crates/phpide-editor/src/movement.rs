//! Caret motions over a selection.

use ropey::Rope;

use crate::selection::{Range, Selection};
use crate::textutil::{
    cell_of, indentation, line_end, next_grapheme, next_word, pos_at_cell, prev_grapheme, prev_word,
};

/// Which document lines are shown (folded lines are not).
pub trait LineVisibility {
    fn is_visible(&self, line: usize) -> bool;
}

/// Every line visible (no folding).
pub struct AllVisible;

impl LineVisibility for AllVisible {
    fn is_visible(&self, _line: usize) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    WordLeft,
    WordRight,
    Up,
    Down,
    /// Up or down by this many visible lines.
    PageUp(usize),
    PageDown(usize),
    /// Smart Home: first non-blank char, then column 0.
    LineStart,
    LineEnd,
    DocStart,
    DocEnd,
}

/// The visible line `count` steps away from `line` (clamped).
pub fn step_lines(
    rope: &Rope,
    line: usize,
    count: isize,
    visibility: &dyn LineVisibility,
) -> usize {
    let last = rope.len_lines().saturating_sub(1);
    let mut current = line;
    let mut remaining = count.unsigned_abs();
    while remaining > 0 {
        let mut next = current;
        loop {
            if count < 0 {
                if next == 0 {
                    return current;
                }
                next -= 1;
            } else {
                if next >= last {
                    return current;
                }
                next += 1;
            }
            if visibility.is_visible(next) {
                break;
            }
        }
        current = next;
        remaining -= 1;
    }
    current
}

fn vertical(
    rope: &Rope,
    range: Range,
    lines: isize,
    extend: bool,
    tab_size: usize,
    visibility: &dyn LineVisibility,
) -> Range {
    let head = range.head;
    let goal = range.goal.unwrap_or_else(|| cell_of(rope, head, tab_size));
    let line = rope.char_to_line(head);
    let target = step_lines(rope, line, lines, visibility);
    let new_head = if target == line {
        // First/last line: go to the start/end of the document.
        if lines < 0 {
            0
        } else {
            rope.len_chars()
        }
    } else {
        pos_at_cell(rope, target, goal, tab_size)
    };
    let anchor = if extend { range.anchor } else { new_head };
    Range::new(anchor, new_head).with_goal(Some(goal))
}

/// Moves (or extends, with Shift) every range.
pub fn move_selection(
    rope: &Rope,
    selection: &Selection,
    motion: Motion,
    extend: bool,
    tab_size: usize,
    visibility: &dyn LineVisibility,
) -> Selection {
    selection.transform(|range| {
        let collapse = |pos: usize| {
            if extend {
                Range::new(range.anchor, pos)
            } else {
                Range::point(pos)
            }
        };
        match motion {
            Motion::Left if !extend && !range.is_empty() => Range::point(range.start()),
            Motion::Right if !extend && !range.is_empty() => Range::point(range.end()),
            Motion::Left => collapse(prev_grapheme(rope, range.head)),
            Motion::Right => collapse(next_grapheme(rope, range.head)),
            Motion::WordLeft => collapse(prev_word(rope, range.head)),
            Motion::WordRight => collapse(next_word(rope, range.head)),
            Motion::Up => vertical(rope, range, -1, extend, tab_size, visibility),
            Motion::Down => vertical(rope, range, 1, extend, tab_size, visibility),
            Motion::PageUp(rows) => vertical(
                rope,
                range,
                -(rows.max(1) as isize),
                extend,
                tab_size,
                visibility,
            ),
            Motion::PageDown(rows) => vertical(
                rope,
                range,
                rows.max(1) as isize,
                extend,
                tab_size,
                visibility,
            ),
            Motion::LineStart => {
                let line = rope.char_to_line(range.head);
                let start = rope.line_to_char(line);
                let first_non_blank = start + indentation(rope, line).chars().count();
                let first_non_blank = first_non_blank.min(line_end(rope, line));
                let target = if range.head == first_non_blank {
                    start
                } else {
                    first_non_blank
                };
                collapse(target)
            }
            Motion::LineEnd => collapse(line_end(rope, rope.char_to_line(range.head))),
            Motion::DocStart => collapse(0),
            Motion::DocEnd => collapse(rope.len_chars()),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moved(text: &str, pos: usize, motion: Motion) -> usize {
        let rope = Rope::from_str(text);
        move_selection(&rope, &Selection::point(pos), motion, false, 4, &AllVisible)
            .primary()
            .head
    }

    #[test]
    fn horizontal_and_word_moves() {
        assert_eq!(moved("abc", 1, Motion::Right), 2);
        assert_eq!(moved("abc", 0, Motion::Left), 0);
        assert_eq!(moved("foo bar", 0, Motion::WordRight), 3);
        assert_eq!(moved("foo bar", 7, Motion::WordLeft), 4);
    }

    #[test]
    fn vertical_moves_keep_the_goal_column() {
        let text = "long line\nab\nanother line";
        let rope = Rope::from_str(text);
        let sel = Selection::point(7);
        let down = move_selection(&rope, &sel, Motion::Down, false, 4, &AllVisible);
        assert_eq!(down.primary().head, 12, "clamped to the short line");
        let down = move_selection(&rope, &down, Motion::Down, false, 4, &AllVisible);
        assert_eq!(down.primary().head, 13 + 7, "goal column restored");
        let up = move_selection(
            &rope,
            &Selection::point(3),
            Motion::Up,
            false,
            4,
            &AllVisible,
        );
        assert_eq!(up.primary().head, 0, "first line goes to the start");
    }

    struct Hidden(std::ops::RangeInclusive<usize>);
    impl LineVisibility for Hidden {
        fn is_visible(&self, line: usize) -> bool {
            !self.0.contains(&line)
        }
    }

    #[test]
    fn vertical_moves_skip_folded_lines() {
        let rope = Rope::from_str("a\nb\nc\nd\n");
        let down = move_selection(
            &rope,
            &Selection::point(0),
            Motion::Down,
            false,
            4,
            &Hidden(1..=2),
        );
        assert_eq!(rope.char_to_line(down.primary().head), 3);
    }

    #[test]
    fn smart_home_toggles_between_indent_and_column_zero() {
        assert_eq!(moved("    echo", 8, Motion::LineStart), 4);
        assert_eq!(moved("    echo", 4, Motion::LineStart), 0);
        assert_eq!(moved("    echo\nx", 2, Motion::LineEnd), 8);
    }

    #[test]
    fn shift_extends_and_plain_moves_collapse() {
        let rope = Rope::from_str("hello");
        let sel = move_selection(
            &rope,
            &Selection::point(0),
            Motion::WordRight,
            true,
            4,
            &AllVisible,
        );
        assert_eq!((sel.primary().anchor, sel.primary().head), (0, 5));
        let sel = move_selection(&rope, &sel, Motion::Left, false, 4, &AllVisible);
        assert_eq!(sel.primary(), Range::point(0));
    }
}
