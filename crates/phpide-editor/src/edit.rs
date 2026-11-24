//! Edit commands: each turns the text and selection into a change set and
//! the selection after it. Nothing here touches the buffer; the caller
//! applies [`Edit`] through `Buffer::edit`, which records undo.

use phpide_core::{Assoc, Change, ChangeSet, EditKind};
use ropey::Rope;

use crate::movement::{step_lines, LineVisibility};
use crate::selection::{Range, Selection};
use crate::textutil::{
    cell_of, indentation, line_end, line_text, next_grapheme, next_word, pos_at_cell,
    prev_grapheme, prev_word, word_at,
};

/// Language and indentation settings used by the commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditConfig {
    pub tab_size: usize,
    pub insert_spaces: bool,
    pub auto_pairs: bool,
    pub line_comment: Option<String>,
    pub block_comment: Option<(String, String)>,
}

impl Default for EditConfig {
    fn default() -> Self {
        EditConfig {
            tab_size: 4,
            insert_spaces: true,
            auto_pairs: true,
            line_comment: Some("//".into()),
            block_comment: Some(("/*".into(), "*/".into())),
        }
    }
}

impl EditConfig {
    pub fn indent_unit(&self) -> String {
        if self.insert_spaces {
            " ".repeat(self.tab_size.max(1))
        } else {
            "\t".into()
        }
    }
}

/// The result of a command: what to change and where the carets go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub changes: ChangeSet,
    pub selection: Selection,
    pub kind: EditKind,
}

/// What a command does to one range.
enum Outcome {
    /// Replace; `anchor`/`head` are offsets from the change start in the
    /// new text.
    Change {
        change: Change,
        anchor: usize,
        head: usize,
    },
    /// Only move the range (offsets in the old text).
    Move(Range),
}

fn change(change: Change, anchor: usize, head: usize) -> Outcome {
    Outcome::Change {
        change,
        anchor,
        head,
    }
}

fn caret_after(c: Change) -> Outcome {
    let len = c.text.chars().count();
    change(c, len, len)
}

fn per_range(selection: &Selection, kind: EditKind, mut f: impl FnMut(Range) -> Outcome) -> Edit {
    let outcomes: Vec<Outcome> = selection.ranges().iter().map(|r| f(*r)).collect();
    let changes = ChangeSet::new(
        outcomes
            .iter()
            .filter_map(|o| match o {
                Outcome::Change { change, .. } => Some(change.clone()),
                Outcome::Move(_) => None,
            })
            .collect(),
    );
    let ranges = outcomes
        .iter()
        .map(|o| match o {
            Outcome::Change {
                change,
                anchor,
                head,
            } => {
                let base = changes.map_pos(change.start, Assoc::Before);
                Range::new(base + anchor, base + head)
            }
            Outcome::Move(r) => Range::new(
                changes.map_pos(r.anchor, Assoc::After),
                changes.map_pos(r.head, Assoc::After),
            ),
        })
        .collect();
    Edit {
        changes,
        selection: Selection::new(ranges, selection.primary_index()),
        kind,
    }
}

fn closer_of(c: char) -> Option<char> {
    match c {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' => Some('"'),
        '\'' => Some('\''),
        '`' => Some('`'),
        _ => None,
    }
}

fn is_closer(c: char) -> bool {
    matches!(c, ')' | ']' | '}' | '"' | '\'' | '`')
}

fn char_at(rope: &Rope, pos: usize) -> Option<char> {
    (pos < rope.len_chars()).then(|| rope.char(pos))
}

fn char_before(rope: &Rope, pos: usize) -> Option<char> {
    (pos > 0).then(|| rope.char(pos - 1))
}

/// Typed text (one keystroke or an IME commit).
pub fn insert_text(rope: &Rope, selection: &Selection, text: &str, config: &EditConfig) -> Edit {
    if text == "\n" || text == "\r\n" {
        return newline(rope, selection, config);
    }
    let mut chars = text.chars();
    let single = match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    };
    per_range(selection, EditKind::Typing, |range| {
        let (start, end) = (range.start(), range.end());
        if let (Some(c), true) = (single, config.auto_pairs) {
            if range.is_empty() {
                let next = char_at(rope, start);
                if is_closer(c) && next == Some(c) {
                    // Type over the closing char that was auto-inserted.
                    return Outcome::Move(Range::point(start + 1));
                }
                if let Some(close) = closer_of(c) {
                    let prev = char_before(rope, start);
                    let quote = c == close;
                    let word_around = prev.is_some_and(|p| p.is_alphanumeric() || p == '_')
                        || next.is_some_and(|n| n.is_alphanumeric() || n == '_');
                    let free_after = next.is_none_or(|n| {
                        n.is_whitespace() || matches!(n, ')' | ']' | '}' | ',' | ';')
                    });
                    if free_after && !(quote && word_around) {
                        return change(Change::new(start, end, format!("{c}{close}")), 1, 1);
                    }
                }
                if c == '}' {
                    let line = rope.char_to_line(start);
                    let line_start = rope.line_to_char(line);
                    let before = rope.slice(line_start..start).to_string();
                    if !before.is_empty() && before.chars().all(|ch| ch == ' ' || ch == '\t') {
                        let dedented = dedent_once(&before, config);
                        let text = format!("{dedented}}}");
                        return caret_after(Change::new(line_start, start, text));
                    }
                }
            } else if let Some(close) = closer_of(c) {
                // Wrap the selection, keeping it selected inside.
                let inner = rope.slice(start..end).to_string();
                let len = inner.chars().count();
                let wrapped = format!("{c}{inner}{close}");
                return if range.head < range.anchor {
                    change(Change::new(start, end, wrapped), len + 1, 1)
                } else {
                    change(Change::new(start, end, wrapped), 1, len + 1)
                };
            }
        }
        caret_after(Change::new(start, end, text.replace("\r\n", "\n")))
    })
}

fn dedent_once(indent: &str, config: &EditConfig) -> String {
    if let Some(rest) = indent.strip_suffix('\t') {
        return rest.to_string();
    }
    let spaces = indent.chars().rev().take_while(|c| *c == ' ').count();
    let remove = match spaces % config.tab_size.max(1) {
        0 => config.tab_size.min(spaces),
        rest => rest,
    };
    indent[..indent.len() - remove].to_string()
}

/// Enter: keeps the indentation, indents after an opening bracket (and puts
/// the closing one on its own line) and continues doc comments.
pub fn newline(rope: &Rope, selection: &Selection, config: &EditConfig) -> Edit {
    per_range(selection, EditKind::Typing, |range| {
        let (start, end) = (range.start(), range.end());
        let line = rope.char_to_line(start);
        let line_start = rope.line_to_char(line);
        let indent: String = indentation(rope, line)
            .chars()
            .take(start - line_start)
            .collect();
        let before_caret = rope.slice(line_start..start).to_string();
        let trimmed = before_caret.trim_start();
        if (trimmed.starts_with("/**") || trimmed.starts_with('*')) && !trimmed.contains("*/") {
            let star = if trimmed.starts_with("/**") {
                " * "
            } else {
                "* "
            };
            let text = format!("\n{indent}{star}");
            return caret_after(Change::new(start, end, text));
        }
        let before = before_caret.trim_end_matches([' ', '\t']).chars().last();
        let after = char_at(rope, end);
        let unit = config.indent_unit();
        match before.and_then(closer_of) {
            Some(close) if !matches!(before, Some('"' | '\'' | '`')) => {
                let inner = format!("{indent}{unit}");
                if after == Some(close) {
                    let text = format!("\n{inner}\n{indent}");
                    let caret = 1 + inner.chars().count();
                    change(Change::new(start, end, text), caret, caret)
                } else {
                    caret_after(Change::new(start, end, format!("\n{inner}")))
                }
            }
            _ => caret_after(Change::new(start, end, format!("\n{indent}"))),
        }
    })
}

/// Backspace.
pub fn delete_backward(rope: &Rope, selection: &Selection, config: &EditConfig) -> Edit {
    per_range(selection, EditKind::Deletion, |range| {
        let (start, end) = (range.start(), range.end());
        if !range.is_empty() {
            return change(Change::delete(start, end), 0, 0);
        }
        if start == 0 {
            return Outcome::Move(range);
        }
        let prev = rope.char(start - 1);
        if config.auto_pairs && closer_of(prev).is_some() && char_at(rope, start) == closer_of(prev)
        {
            return change(Change::delete(start - 1, start + 1), 0, 0);
        }
        let line = rope.char_to_line(start);
        let line_start = rope.line_to_char(line);
        let before = rope.slice(line_start..start).to_string();
        if config.insert_spaces && !before.is_empty() && before.chars().all(|c| c == ' ') {
            let col = before.chars().count();
            let tab = config.tab_size.max(1);
            let target = (col - 1) / tab * tab;
            return change(Change::delete(line_start + target, start), 0, 0);
        }
        change(Change::delete(prev_grapheme(rope, start), start), 0, 0)
    })
}

/// Delete key.
pub fn delete_forward(rope: &Rope, selection: &Selection) -> Edit {
    per_range(selection, EditKind::Deletion, |range| {
        let (start, end) = (range.start(), range.end());
        if !range.is_empty() {
            return change(Change::delete(start, end), 0, 0);
        }
        let next = next_grapheme(rope, start);
        if next == start {
            return Outcome::Move(range);
        }
        change(Change::delete(start, next), 0, 0)
    })
}

/// Ctrl+Backspace / Ctrl+Delete.
pub fn delete_word(rope: &Rope, selection: &Selection, forward: bool) -> Edit {
    per_range(selection, EditKind::Deletion, |range| {
        let (start, end) = if !range.is_empty() {
            (range.start(), range.end())
        } else if forward {
            (range.head, next_word(rope, range.head))
        } else {
            (prev_word(rope, range.head), range.head)
        };
        if start == end {
            return Outcome::Move(range);
        }
        change(Change::delete(start, end), 0, 0)
    })
}

/// Lines touched by the selection, merged into sorted blocks `(first, last)`.
fn line_blocks(rope: &Rope, selection: &Selection) -> Vec<(usize, usize)> {
    let mut blocks: Vec<(usize, usize)> = selection
        .ranges()
        .iter()
        .map(|r| {
            let first = rope.char_to_line(r.start());
            let mut last = rope.char_to_line(r.end());
            // A selection ending at column 0 does not include that line.
            if !r.is_empty() && last > first && rope.line_to_char(last) == r.end() {
                last -= 1;
            }
            (first, last)
        })
        .collect();
    blocks.sort();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (first, last) in blocks {
        match merged.last_mut() {
            Some(prev) if first <= prev.1 + 1 => prev.1 = prev.1.max(last),
            _ => merged.push((first, last)),
        }
    }
    merged
}

fn edit_lines(selection: &Selection, changes: Vec<Change>, kind: EditKind) -> Edit {
    let changes = ChangeSet::new(changes);
    Edit {
        selection: selection.map(&changes),
        changes,
        kind,
    }
}

/// Tab: indents selected lines, or inserts indentation at the carets.
pub fn tab(rope: &Rope, selection: &Selection, config: &EditConfig) -> Edit {
    let multi_line = selection
        .ranges()
        .iter()
        .any(|r| rope.char_to_line(r.start()) != rope.char_to_line(r.end()));
    if multi_line {
        return indent_lines(rope, selection, config);
    }
    per_range(selection, EditKind::Typing, |range| {
        let text = if config.insert_spaces {
            let cell = cell_of(rope, range.start(), config.tab_size);
            " ".repeat(config.tab_size - cell % config.tab_size.max(1))
        } else {
            "\t".into()
        };
        caret_after(Change::new(range.start(), range.end(), text))
    })
}

pub fn indent_lines(rope: &Rope, selection: &Selection, config: &EditConfig) -> Edit {
    let unit = config.indent_unit();
    let mut changes = Vec::new();
    for (first, last) in line_blocks(rope, selection) {
        for line in first..=last {
            if !line_text(rope, line).trim().is_empty() {
                changes.push(Change::insert(rope.line_to_char(line), unit.clone()));
            }
        }
    }
    edit_lines(selection, changes, EditKind::Other)
}

/// Shift+Tab.
pub fn outdent_lines(rope: &Rope, selection: &Selection, config: &EditConfig) -> Edit {
    let mut changes = Vec::new();
    for (first, last) in line_blocks(rope, selection) {
        for line in first..=last {
            let indent = indentation(rope, line);
            let remove = if indent.starts_with('\t') {
                1
            } else {
                indent
                    .chars()
                    .take_while(|c| *c == ' ')
                    .count()
                    .min(config.tab_size.max(1))
            };
            if remove > 0 {
                let start = rope.line_to_char(line);
                changes.push(Change::delete(start, start + remove));
            }
        }
    }
    edit_lines(selection, changes, EditKind::Other)
}

/// Ctrl+/: comments or uncomments the selected lines.
pub fn toggle_line_comment(
    rope: &Rope,
    selection: &Selection,
    config: &EditConfig,
) -> Option<Edit> {
    let prefix = config.line_comment.as_deref()?;
    let mut lines = Vec::new();
    for (first, last) in line_blocks(rope, selection) {
        lines.extend(first..=last);
    }
    let content: Vec<(usize, String)> = lines
        .iter()
        .map(|&l| (l, line_text(rope, l)))
        .filter(|(_, text)| !text.trim().is_empty())
        .collect();
    if content.is_empty() {
        return None;
    }
    let all_commented = content
        .iter()
        .all(|(_, text)| text.trim_start().starts_with(prefix));
    let mut changes = Vec::new();
    if all_commented {
        for (line, text) in &content {
            let indent = text.len() - text.trim_start().len();
            let indent_chars = text[..indent].chars().count();
            let after = &text.trim_start()[prefix.len()..];
            let remove = prefix.chars().count() + usize::from(after.starts_with(' '));
            let start = rope.line_to_char(*line) + indent_chars;
            changes.push(Change::delete(start, start + remove));
        }
    } else {
        let column = content
            .iter()
            .map(|(_, text)| text.chars().take_while(|c| *c == ' ' || *c == '\t').count())
            .min()
            .unwrap_or(0);
        for (line, _) in &content {
            changes.push(Change::insert(
                rope.line_to_char(*line) + column,
                format!("{prefix} "),
            ));
        }
    }
    Some(edit_lines(selection, changes, EditKind::Other))
}

/// Ctrl+Shift+/: wraps or unwraps each selection in a block comment.
pub fn toggle_block_comment(
    rope: &Rope,
    selection: &Selection,
    config: &EditConfig,
) -> Option<Edit> {
    let (open, close) = config.block_comment.clone()?;
    Some(per_range(selection, EditKind::Other, |range| {
        let (start, end) = (range.start(), range.end());
        if range.is_empty() {
            let caret = open.chars().count();
            return change(
                Change::insert(start, format!("{open}{close}")),
                caret,
                caret,
            );
        }
        let text = rope.slice(start..end).to_string();
        let new = if text.starts_with(&open)
            && text.ends_with(&close)
            && text.len() >= open.len() + close.len()
        {
            let inner = &text[open.len()..text.len() - close.len()];
            inner
                .strip_prefix(' ')
                .and_then(|i| i.strip_suffix(' '))
                .unwrap_or(inner)
                .to_string()
        } else {
            format!("{open}{text}{close}")
        };
        let len = new.chars().count();
        change(Change::new(start, end, new), 0, len)
    }))
}

/// Duplicates the caret lines, or the selected text after itself.
pub fn duplicate(rope: &Rope, selection: &Selection) -> Edit {
    per_range(selection, EditKind::Other, |range| {
        if range.is_empty() {
            let line = rope.char_to_line(range.head);
            let column = range.head - rope.line_to_char(line);
            let text = line_text(rope, line);
            let end = line_end(rope, line);
            let caret = 1 + column;
            change(Change::insert(end, format!("\n{text}")), caret, caret)
        } else {
            let text = rope.slice(range.start()..range.end()).to_string();
            let len = text.chars().count();
            change(Change::insert(range.end(), text), 0, len)
        }
    })
}

/// Ctrl+Y: deletes the lines touched by the selection.
pub fn delete_lines(rope: &Rope, selection: &Selection) -> Edit {
    let last_line = rope.len_lines().saturating_sub(1);
    let mut changes = Vec::new();
    for (first, last) in line_blocks(rope, selection) {
        let (start, end) = if last < last_line {
            (rope.line_to_char(first), rope.line_to_char(last + 1))
        } else if first > 0 {
            (line_end(rope, first - 1), rope.len_chars())
        } else {
            (0, rope.len_chars())
        };
        changes.push(Change::delete(start, end));
    }
    let changes = ChangeSet::new(changes);
    let selection = selection.transform(|r| Range::point(r.head)).map(&changes);
    Edit {
        changes,
        selection,
        kind: EditKind::Other,
    }
}

/// Alt+Shift+Up/Down: moves the selected lines.
pub fn move_lines(rope: &Rope, selection: &Selection, up: bool) -> Option<Edit> {
    let blocks = line_blocks(rope, selection);
    let last_line = rope.len_lines().saturating_sub(1);
    if up && blocks.first()?.0 == 0 || !up && blocks.last()?.1 >= last_line {
        return None;
    }
    let crlf = rope.line(0).chars().collect::<String>().ends_with("\r\n");
    let sep = if crlf { "\r\n" } else { "\n" };
    let mut changes = Vec::new();
    let mut shifts: Vec<(usize, usize, isize)> = Vec::new();
    for (first, last) in &blocks {
        let (first, last) = (*first, *last);
        let (region_first, region_last, other) = if up {
            (first - 1, last, first - 1)
        } else {
            (first, last + 1, last + 1)
        };
        let block: Vec<String> = (first..=last).map(|l| line_text(rope, l)).collect();
        let other_text = line_text(rope, other);
        let moved = if up {
            let mut lines = block.clone();
            lines.push(other_text.clone());
            lines
        } else {
            let mut lines = vec![other_text.clone()];
            lines.extend(block.clone());
            lines
        };
        let start = rope.line_to_char(region_first);
        let end = line_end(rope, region_last);
        changes.push(Change::new(start, end, moved.join(sep)));
        let distance = (other_text.chars().count() + sep.chars().count()) as isize;
        shifts.push((
            rope.line_to_char(first),
            line_end(rope, last),
            if up { -distance } else { distance },
        ));
    }
    let selection = selection.transform(|r| {
        let shift = shifts
            .iter()
            .find(|(s, e, _)| r.start() >= *s && r.end() <= *e + 1)
            .map(|(_, _, d)| *d)
            .unwrap_or(0);
        let mv = |p: usize| (p as isize + shift).max(0) as usize;
        Range::new(mv(r.anchor), mv(r.head)).with_goal(r.goal)
    });
    Some(Edit {
        changes: ChangeSet::new(changes),
        selection,
        kind: EditKind::Other,
    })
}

/// Text to copy. With no selection, whole caret lines are copied and
/// `line_mode` is true (pasting them inserts above the caret line).
pub fn copy_text(rope: &Rope, selection: &Selection) -> (String, bool) {
    if selection.is_empty() {
        let mut lines: Vec<usize> = selection
            .ranges()
            .iter()
            .map(|r| rope.char_to_line(r.head))
            .collect();
        lines.dedup();
        let text = lines
            .iter()
            .map(|l| format!("{}\n", line_text(rope, *l)))
            .collect::<String>();
        (text, true)
    } else {
        let parts: Vec<String> = selection
            .ranges()
            .iter()
            .filter(|r| !r.is_empty())
            .map(|r| rope.slice(r.start()..r.end()).to_string())
            .collect();
        (parts.join("\n"), false)
    }
}

/// Cut: copies and removes the selection (or the caret lines).
pub fn cut(rope: &Rope, selection: &Selection) -> (String, bool, Edit) {
    let (text, line_mode) = copy_text(rope, selection);
    let edit = if line_mode {
        delete_lines(rope, selection)
    } else {
        per_range(selection, EditKind::Other, |r| {
            change(Change::delete(r.start(), r.end()), 0, 0)
        })
    };
    (text, line_mode, edit)
}

/// Paste. One line per caret when the line count matches; whole-line
/// clipboard content goes above the caret line.
pub fn paste(rope: &Rope, selection: &Selection, text: &str, line_mode: bool) -> Edit {
    let text = text.replace("\r\n", "\n");
    if line_mode && selection.is_empty() {
        return per_range(selection, EditKind::Other, |range| {
            let line = rope.char_to_line(range.head);
            let start = rope.line_to_char(line);
            let column = range.head - start;
            let len = text.chars().count();
            change(
                Change::insert(start, text.clone()),
                len + column,
                len + column,
            )
        });
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let distribute = selection.len() > 1 && lines.len() == selection.len();
    let mut index = 0;
    per_range(selection, EditKind::Other, |range| {
        let piece = if distribute {
            lines[index].to_string()
        } else {
            text.clone()
        };
        index += 1;
        caret_after(Change::new(range.start(), range.end(), piece))
    })
}

pub fn select_all(rope: &Rope) -> Selection {
    Selection::single(0, rope.len_chars())
}

fn find_from(rope: &Rope, needle: &str, from: usize, whole_word: bool) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }
    let text = rope.to_string();
    let from_byte = rope.char_to_byte(from.min(rope.len_chars()));
    let needle_chars = needle.chars().count();
    let is_match = |byte: usize| {
        let start = rope.byte_to_char(byte);
        let end = start + needle_chars;
        if whole_word {
            let before_ok = start == 0 || !crate::textutil::is_word_char(rope.char(start - 1));
            let after_ok =
                end >= rope.len_chars() || !crate::textutil::is_word_char(rope.char(end));
            if !(before_ok && after_ok) {
                return None;
            }
        }
        Some((start, end))
    };
    text[from_byte..]
        .match_indices(needle)
        .find_map(|(i, _)| is_match(from_byte + i))
        .or_else(|| {
            text[..from_byte]
                .match_indices(needle)
                .find_map(|(i, _)| is_match(i))
        })
}

/// Alt+J: selects the word at the caret, then adds the next occurrence of
/// the primary selection. Returns the selection and whether matching is by
/// whole word (true when it started from a caret).
pub fn add_next_occurrence(
    rope: &Rope,
    selection: &Selection,
    whole_word: bool,
) -> Option<(Selection, bool)> {
    let primary = selection.primary();
    if primary.is_empty() {
        let (start, end) = word_at(rope, primary.head)?;
        let ranges: Vec<Range> = selection
            .ranges()
            .iter()
            .enumerate()
            .map(|(i, r)| {
                if i == selection.primary_index() {
                    Range::new(start, end)
                } else {
                    *r
                }
            })
            .collect();
        return Some((Selection::new(ranges, selection.primary_index()), true));
    }
    let needle = rope.slice(primary.start()..primary.end()).to_string();
    let after = selection
        .ranges()
        .iter()
        .map(|r| r.end())
        .max()
        .unwrap_or(0);
    let mut from = after;
    for _ in 0..selection.len() + 1 {
        let (start, end) = find_from(rope, &needle, from, whole_word)?;
        if !selection
            .ranges()
            .iter()
            .any(|r| r.start() == start && r.end() == end)
        {
            return Some((selection.clone().push(Range::new(start, end)), whole_word));
        }
        from = end;
    }
    None
}

/// Ctrl+Alt+Shift+J: every occurrence of the primary selection (or word).
pub fn select_all_occurrences(rope: &Rope, selection: &Selection) -> Option<Selection> {
    let (selection, whole_word) = if selection.primary().is_empty() {
        add_next_occurrence(rope, selection, true)?
    } else {
        (selection.clone(), false)
    };
    let primary = selection.primary();
    let needle = rope.slice(primary.start()..primary.end()).to_string();
    let text = rope.to_string();
    let needle_chars = needle.chars().count();
    let mut ranges = Vec::new();
    let mut primary_index = 0;
    for (byte, _) in text.match_indices(&needle) {
        let start = rope.byte_to_char(byte);
        let end = start + needle_chars;
        if whole_word {
            let before_ok = start == 0 || !crate::textutil::is_word_char(rope.char(start - 1));
            let after_ok =
                end >= rope.len_chars() || !crate::textutil::is_word_char(rope.char(end));
            if !(before_ok && after_ok) {
                continue;
            }
        }
        if start == primary.start() {
            primary_index = ranges.len();
        }
        ranges.push(Range::new(start, end));
    }
    Some(Selection::new(ranges, primary_index))
}

/// Adds a caret on the visible line above or below the primary caret, at
/// the same visual column.
pub fn add_caret_vertical(
    rope: &Rope,
    selection: &Selection,
    up: bool,
    tab_size: usize,
    visibility: &dyn LineVisibility,
) -> Selection {
    let primary = selection.primary();
    let goal = primary
        .goal
        .unwrap_or_else(|| cell_of(rope, primary.head, tab_size));
    let line = rope.char_to_line(primary.head);
    let target = step_lines(rope, line, if up { -1 } else { 1 }, visibility);
    if target == line {
        return selection.clone();
    }
    let pos = pos_at_cell(rope, target, goal, tab_size);
    selection
        .clone()
        .push(Range::point(pos).with_goal(Some(goal)))
}

/// Column (box) selection between two (line, cell) corners.
pub fn column_selection(
    rope: &Rope,
    anchor: (usize, usize),
    head: (usize, usize),
    tab_size: usize,
    visibility: &dyn LineVisibility,
) -> Selection {
    let (first, last) = (anchor.0.min(head.0), anchor.0.max(head.0));
    let mut ranges = Vec::new();
    let mut primary = 0;
    for line in first..=last.min(rope.len_lines().saturating_sub(1)) {
        if !visibility.is_visible(line) {
            continue;
        }
        let a = pos_at_cell(rope, line, anchor.1, tab_size);
        let h = pos_at_cell(rope, line, head.1, tab_size);
        if line == head.0 {
            primary = ranges.len();
        }
        ranges.push(Range::new(a, h).with_goal(Some(head.1)));
    }
    Selection::new(ranges, primary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movement::AllVisible;

    /// Applies an edit to text with carets marked as `|` (and selections as
    /// `[...]` is not needed here: tests use carets).
    fn run(
        text: &str,
        carets: &[usize],
        f: impl Fn(&Rope, &Selection) -> Edit,
    ) -> (String, Vec<usize>) {
        let mut rope = Rope::from_str(text);
        let ranges = carets.iter().map(|&c| Range::point(c)).collect();
        let selection = Selection::new(ranges, 0);
        let edit = f(&rope, &selection);
        let mut doc = phpide_core::Document::new(&rope.to_string());
        doc.apply(&edit.changes);
        rope = doc.rope().clone();
        (
            rope.to_string(),
            edit.selection.ranges().iter().map(|r| r.head).collect(),
        )
    }

    fn cfg() -> EditConfig {
        EditConfig::default()
    }

    #[test]
    fn typing_at_several_carets() {
        let (text, carets) = run("a\nb\nc", &[1, 3, 5], |r, s| insert_text(r, s, "x", &cfg()));
        assert_eq!(text, "ax\nbx\ncx");
        assert_eq!(carets, vec![2, 5, 8]);
    }

    #[test]
    fn brackets_and_quotes_pair_and_type_over() {
        let (text, carets) = run("f", &[1], |r, s| insert_text(r, s, "(", &cfg()));
        assert_eq!((text.as_str(), carets[0]), ("f()", 2));
        let (text, carets) = run("f()", &[2], |r, s| insert_text(r, s, ")", &cfg()));
        assert_eq!((text.as_str(), carets[0]), ("f()", 3));
        // No pairing for a quote right after a word char (it's an apostrophe).
        let (text, _) = run("don", &[3], |r, s| insert_text(r, s, "'", &cfg()));
        assert_eq!(text, "don'");
        let (text, _) = run("$a = ", &[5], |r, s| insert_text(r, s, "'", &cfg()));
        assert_eq!(text, "$a = ''");
    }

    #[test]
    fn typing_a_bracket_wraps_the_selection() {
        let rope = Rope::from_str("abc");
        let edit = insert_text(&rope, &Selection::single(0, 3), "[", &cfg());
        let mut doc = phpide_core::Document::new("abc");
        doc.apply(&edit.changes);
        assert_eq!(doc.text(), "[abc]");
        assert_eq!(
            (
                edit.selection.primary().anchor,
                edit.selection.primary().head
            ),
            (1, 4)
        );
    }

    #[test]
    fn enter_indents_and_splits_braces() {
        let (text, carets) = run("    if (x) {}", &[12], |r, s| newline(r, s, &cfg()));
        assert_eq!(text, "    if (x) {\n        \n    }");
        assert_eq!(carets[0], 13 + 8);
        let (text, _) = run("    echo 1;", &[11], |r, s| newline(r, s, &cfg()));
        assert_eq!(text, "    echo 1;\n    ");
        let (text, _) = run("    /**", &[7], |r, s| newline(r, s, &cfg()));
        assert_eq!(text, "    /**\n     * ");
    }

    #[test]
    fn closing_brace_dedents_a_blank_line() {
        let (text, _) = run("{\n        ", &[10], |r, s| insert_text(r, s, "}", &cfg()));
        assert_eq!(text, "{\n    }");
    }

    #[test]
    fn backspace_removes_pairs_and_indent_stops() {
        let (text, _) = run("f()", &[2], |r, s| delete_backward(r, s, &cfg()));
        assert_eq!(text, "f");
        let (text, carets) = run("      x", &[6], |r, s| delete_backward(r, s, &cfg()));
        assert_eq!((text.as_str(), carets[0]), ("    x", 4));
        let (text, _) = run("ab\ncd", &[3], |r, s| delete_backward(r, s, &cfg()));
        assert_eq!(text, "abcd");
    }

    #[test]
    fn word_deletion() {
        let (text, _) = run("foo bar", &[7], |r, s| delete_word(r, s, false));
        assert_eq!(text, "foo ");
        let (text, _) = run("foo bar", &[0], |r, s| delete_word(r, s, true));
        assert_eq!(text, " bar");
    }

    #[test]
    fn indent_and_outdent_lines() {
        let rope = Rope::from_str("a\nb\n\nc");
        let edit = tab(&rope, &Selection::single(0, 6), &cfg());
        let mut doc = phpide_core::Document::new("a\nb\n\nc");
        doc.apply(&edit.changes);
        assert_eq!(doc.text(), "    a\n    b\n\n    c");
        let edit = outdent_lines(doc.rope(), &edit.selection, &cfg());
        doc.apply(&edit.changes);
        assert_eq!(doc.text(), "a\nb\n\nc");
        let (text, _) = run("ab", &[1], |r, s| tab(r, s, &cfg()));
        assert_eq!(text, "a   b");
    }

    #[test]
    fn line_comments_toggle() {
        let src = "    a();\n    b();";
        let rope = Rope::from_str(src);
        let edit = toggle_line_comment(&rope, &Selection::single(0, 12), &cfg()).unwrap();
        let mut doc = phpide_core::Document::new(src);
        doc.apply(&edit.changes);
        assert_eq!(doc.text(), "    // a();\n    // b();");
        let edit = toggle_line_comment(doc.rope(), &edit.selection, &cfg()).unwrap();
        doc.apply(&edit.changes);
        assert_eq!(doc.text(), src);
    }

    #[test]
    fn block_comment_wraps_and_unwraps() {
        let rope = Rope::from_str("abc");
        let edit = toggle_block_comment(&rope, &Selection::single(0, 3), &cfg()).unwrap();
        let mut doc = phpide_core::Document::new("abc");
        doc.apply(&edit.changes);
        assert_eq!(doc.text(), "/*abc*/");
        let edit = toggle_block_comment(doc.rope(), &edit.selection, &cfg()).unwrap();
        doc.apply(&edit.changes);
        assert_eq!(doc.text(), "abc");
    }

    #[test]
    fn duplicate_delete_and_move_lines() {
        let (text, carets) = run("one\ntwo", &[1], duplicate);
        assert_eq!((text.as_str(), carets[0]), ("one\none\ntwo", 5));
        let (text, _) = run("one\ntwo\nthree", &[5], delete_lines);
        assert_eq!(text, "one\nthree");
        let (text, _) = run("one\ntwo", &[5], delete_lines);
        assert_eq!(text, "one");
        let (text, carets) = run("one\ntwo\nthree", &[5], |r, s| {
            move_lines(r, s, true).unwrap()
        });
        assert_eq!((text.as_str(), carets[0]), ("two\none\nthree", 1));
        let (text, carets) = run("one\ntwo\nthree", &[1], |r, s| {
            move_lines(r, s, false).unwrap()
        });
        assert_eq!((text.as_str(), carets[0]), ("two\none\nthree", 5));
        assert!(move_lines(&Rope::from_str("a\nb"), &Selection::point(0), true).is_none());
    }

    #[test]
    fn copy_cut_paste_lines_and_distribution() {
        let rope = Rope::from_str("one\ntwo");
        assert_eq!(
            copy_text(&rope, &Selection::point(1)),
            ("one\n".into(), true)
        );
        let (text, _) = run("one\ntwo", &[5], |r, s| paste(r, s, "one\n", true));
        assert_eq!(text, "one\none\ntwo");
        let (text, _) = run("a\nb", &[1, 3], |r, s| paste(r, s, "x\ny", false));
        assert_eq!(text, "ax\nby");
        let (_, line_mode, edit) = cut(&rope, &Selection::point(1));
        assert!(line_mode);
        let mut doc = phpide_core::Document::new("one\ntwo");
        doc.apply(&edit.changes);
        assert_eq!(doc.text(), "two");
    }

    #[test]
    fn occurrences_and_vertical_carets() {
        let rope = Rope::from_str("foo bar foo food");
        let (sel, whole) = add_next_occurrence(&rope, &Selection::point(1), false).unwrap();
        assert!(whole);
        assert_eq!((sel.primary().start(), sel.primary().end()), (0, 3));
        let (sel, _) = add_next_occurrence(&rope, &sel, whole).unwrap();
        assert_eq!(sel.len(), 2);
        assert_eq!(sel.primary().start(), 8);
        assert!(
            add_next_occurrence(&rope, &sel, whole).is_none(),
            "food is not a whole-word match"
        );
        let all = select_all_occurrences(&rope, &Selection::point(1)).unwrap();
        assert_eq!(all.len(), 2);

        let rope = Rope::from_str("abc\nabc\nabc");
        let sel = add_caret_vertical(&rope, &Selection::point(1), false, 4, &AllVisible);
        let sel = add_caret_vertical(&rope, &sel, false, 4, &AllVisible);
        assert_eq!(
            sel.ranges().iter().map(|r| r.head).collect::<Vec<_>>(),
            vec![1, 5, 9]
        );
        let boxed = column_selection(&rope, (0, 1), (2, 3), 4, &AllVisible);
        assert_eq!(boxed.len(), 3);
        assert_eq!((boxed.ranges()[2].anchor, boxed.ranges()[2].head), (9, 11));
    }
}
