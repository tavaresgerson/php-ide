//! Rope helpers: line bounds, grapheme and word boundaries, visual cells.

use ropey::Rope;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthChar;

/// Char offset where the content of `line` ends (before `\n` / `\r\n`).
pub fn line_end(rope: &Rope, line: usize) -> usize {
    let start = rope.line_to_char(line);
    let slice = rope.line(line);
    let mut len = slice.len_chars();
    if len > 0 && slice.char(len - 1) == '\n' {
        len -= 1;
        if len > 0 && slice.char(len - 1) == '\r' {
            len -= 1;
        }
    }
    start + len
}

/// Line content without its line break.
pub fn line_text(rope: &Rope, line: usize) -> String {
    let start = rope.line_to_char(line);
    rope.slice(start..line_end(rope, line)).to_string()
}

/// Number of lines, not counting the empty line after a final newline as
/// missing: ropey counts it, and so does the editor (a caret can be there).
pub fn line_count(rope: &Rope) -> usize {
    rope.len_lines()
}

/// Leading whitespace of `line`.
pub fn indentation(rope: &Rope, line: usize) -> String {
    rope.line(line)
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}

/// Char boundaries of the graphemes in `text` (relative char offsets).
fn grapheme_starts(text: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut chars = 0;
    for grapheme in text.graphemes(true) {
        out.push(chars);
        chars += grapheme.chars().count();
    }
    out.push(chars);
    out
}

/// Offset of the grapheme boundary after `pos` (a line break counts as one).
pub fn next_grapheme(rope: &Rope, pos: usize) -> usize {
    let len = rope.len_chars();
    if pos >= len {
        return len;
    }
    let line = rope.char_to_line(pos);
    let end = line_end(rope, line);
    if pos >= end {
        // At the line break: jump to the next line start.
        return if line + 1 < rope.len_lines() {
            rope.line_to_char(line + 1)
        } else {
            len
        };
    }
    let start = rope.line_to_char(line);
    let text = rope.slice(start..end).to_string();
    let rel = pos - start;
    grapheme_starts(&text)
        .into_iter()
        .find(|b| *b > rel)
        .map(|b| start + b)
        .unwrap_or(end)
}

/// Offset of the grapheme boundary before `pos`.
pub fn prev_grapheme(rope: &Rope, pos: usize) -> usize {
    if pos == 0 {
        return 0;
    }
    let line = rope.char_to_line(pos);
    let start = rope.line_to_char(line);
    if pos == start {
        return line_end(rope, line - 1);
    }
    let end = line_end(rope, line);
    let text = rope.slice(start..end).to_string();
    let rel = (pos - start).min(text.chars().count());
    grapheme_starts(&text)
        .into_iter()
        .rev()
        .find(|b| *b < rel)
        .map(|b| start + b)
        .unwrap_or(start)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharClass {
    LineBreak,
    Space,
    Word,
    Punct,
}

pub fn class(c: char) -> CharClass {
    if c == '\n' || c == '\r' {
        CharClass::LineBreak
    } else if c.is_whitespace() {
        CharClass::Space
    } else if c.is_alphanumeric() || c == '_' {
        CharClass::Word
    } else {
        CharClass::Punct
    }
}

pub fn is_word_char(c: char) -> bool {
    class(c) == CharClass::Word
}

/// End of the word (or punctuation run) after `pos`; a line break is a
/// stop of its own.
pub fn next_word(rope: &Rope, pos: usize) -> usize {
    let len = rope.len_chars();
    let mut i = pos;
    if i >= len {
        return len;
    }
    if class(rope.char(i)) == CharClass::LineBreak {
        return next_grapheme(rope, i);
    }
    while i < len && class(rope.char(i)) == CharClass::Space {
        i += 1;
    }
    if i < len {
        let run = class(rope.char(i));
        if run != CharClass::LineBreak {
            while i < len && class(rope.char(i)) == run {
                i += 1;
            }
        }
    }
    i
}

/// Start of the word (or punctuation run) before `pos`.
pub fn prev_word(rope: &Rope, pos: usize) -> usize {
    if pos == 0 {
        return 0;
    }
    let mut i = pos;
    if class(rope.char(i - 1)) == CharClass::LineBreak {
        return prev_grapheme(rope, i);
    }
    while i > 0 && class(rope.char(i - 1)) == CharClass::Space {
        i -= 1;
    }
    if i > 0 {
        let run = class(rope.char(i - 1));
        if run != CharClass::LineBreak {
            while i > 0 && class(rope.char(i - 1)) == run {
                i -= 1;
            }
        }
    }
    i
}

/// The word around `pos` (identifier characters), if any.
pub fn word_at(rope: &Rope, pos: usize) -> Option<(usize, usize)> {
    let len = rope.len_chars();
    let mut start = pos.min(len);
    let mut end = start;
    while start > 0 && is_word_char(rope.char(start - 1)) {
        start -= 1;
    }
    while end < len && is_word_char(rope.char(end)) {
        end += 1;
    }
    (start < end).then_some((start, end))
}

/// Width of a char in cells, with tabs advancing to the next tab stop.
pub fn char_cells(c: char, cell: usize, tab_size: usize) -> usize {
    if c == '\t' {
        tab_size - cell % tab_size.max(1)
    } else {
        UnicodeWidthChar::width(c)
            .unwrap_or(0)
            .max(usize::from(!c.is_control()))
    }
}

/// Visual column (cells) of `pos` on its line.
pub fn cell_of(rope: &Rope, pos: usize, tab_size: usize) -> usize {
    let line = rope.char_to_line(pos);
    let start = rope.line_to_char(line);
    let mut cell = 0;
    for c in rope.slice(start..pos).chars() {
        cell += char_cells(c, cell, tab_size);
    }
    cell
}

/// Offset on `line` closest to visual column `cell` (never past the line
/// content).
pub fn pos_at_cell(rope: &Rope, line: usize, cell: usize, tab_size: usize) -> usize {
    let start = rope.line_to_char(line);
    let end = line_end(rope, line);
    let mut current = 0;
    let mut pos = start;
    for c in rope.slice(start..end).chars() {
        if current >= cell {
            return pos;
        }
        let width = char_cells(c, current, tab_size);
        if current + width > cell {
            // Inside a wide char or tab: snap to the nearer edge.
            return if cell - current > width / 2 {
                pos + 1
            } else {
                pos
            };
        }
        current += width;
        pos += 1;
    }
    pos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_bounds_ignore_crlf() {
        let rope = Rope::from_str("ab\r\ncd\nlast");
        assert_eq!(line_end(&rope, 0), 2);
        assert_eq!(line_end(&rope, 1), 6);
        assert_eq!(line_end(&rope, 2), 11);
        assert_eq!(line_text(&rope, 0), "ab");
    }

    #[test]
    fn graphemes_keep_combined_characters_together() {
        let rope = Rope::from_str("e\u{301}x\nnext");
        assert_eq!(next_grapheme(&rope, 0), 2);
        assert_eq!(prev_grapheme(&rope, 2), 0);
        assert_eq!(next_grapheme(&rope, 3), 4, "line break is one step");
        assert_eq!(prev_grapheme(&rope, 4), 3);
    }

    #[test]
    fn word_motions_stop_at_runs_and_line_breaks() {
        let rope = Rope::from_str("$user->name = 1;\nnext");
        assert_eq!(next_word(&rope, 0), 1, "$ is punctuation");
        assert_eq!(next_word(&rope, 1), 5);
        assert_eq!(next_word(&rope, 5), 7);
        assert_eq!(next_word(&rope, 11), 13);
        assert_eq!(next_word(&rope, 16), 17);
        assert_eq!(prev_word(&rope, 11), 7);
        assert_eq!(prev_word(&rope, 17), 16);
        assert_eq!(word_at(&rope, 3), Some((1, 5)));
    }

    #[test]
    fn cells_expand_tabs_and_wide_chars() {
        let rope = Rope::from_str("\tab\n日本x");
        assert_eq!(cell_of(&rope, 1, 4), 4);
        assert_eq!(cell_of(&rope, 3, 4), 6);
        assert_eq!(pos_at_cell(&rope, 0, 4, 4), 1);
        assert_eq!(pos_at_cell(&rope, 0, 99, 4), 3);
        let second = rope.line_to_char(1);
        assert_eq!(cell_of(&rope, second + 2, 4), 4);
        assert_eq!(pos_at_cell(&rope, 1, 4, 4), second + 2);
        assert_eq!(pos_at_cell(&rope, 1, 2, 4), second + 1);
    }
}
