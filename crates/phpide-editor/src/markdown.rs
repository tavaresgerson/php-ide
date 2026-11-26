//! Markdown editing commands: bold, italic, code, links, headings, lists,
//! quotes, tables. Like the other commands they turn the text and the
//! selection into an [`Edit`]; the Markdown toolbar and its shortcuts run
//! them through [`crate::ViewCommand::Markdown`].

use phpide_core::{Change, ChangeSet, EditKind};
use ropey::Rope;

use crate::edit::Edit;
use crate::selection::{Range, Selection};
use crate::textutil::line_text;

/// A Markdown formatting action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarkdownEdit {
    Bold,
    Italic,
    Strikethrough,
    Code,
    CodeBlock,
    Link,
    Image,
    /// `#` × level; 0 makes the lines plain paragraphs.
    Heading(u8),
    BulletList,
    NumberedList,
    TaskList,
    Quote,
    Table,
    Rule,
}

/// Runs `action` on every range of the selection.
pub fn run(rope: &Rope, selection: &Selection, action: MarkdownEdit) -> Option<Edit> {
    match action {
        MarkdownEdit::Bold => Some(wrap(rope, selection, "**")),
        MarkdownEdit::Italic => Some(wrap(rope, selection, "_")),
        MarkdownEdit::Strikethrough => Some(wrap(rope, selection, "~~")),
        MarkdownEdit::Code => Some(wrap(rope, selection, "`")),
        MarkdownEdit::CodeBlock => Some(code_block(rope, selection)),
        MarkdownEdit::Link => Some(link(rope, selection, false)),
        MarkdownEdit::Image => Some(link(rope, selection, true)),
        MarkdownEdit::Heading(level) => Some(lines(rope, selection, LinePrefix::Heading(level))),
        MarkdownEdit::BulletList => Some(lines(rope, selection, LinePrefix::Bullet)),
        MarkdownEdit::NumberedList => Some(lines(rope, selection, LinePrefix::Numbered)),
        MarkdownEdit::TaskList => Some(lines(rope, selection, LinePrefix::Task)),
        MarkdownEdit::Quote => Some(lines(rope, selection, LinePrefix::Quote)),
        MarkdownEdit::Table => Some(insert_block(
            rope,
            selection,
            "| Column 1 | Column 2 |\n| -------- | -------- |\n|          |          |\n",
            2,
            8,
        )),
        MarkdownEdit::Rule => Some(insert_block(rope, selection, "---\n", 0, 0)),
    }
}

/// Builds an edit from one change per range and the new ranges, given
/// as offsets from the start of each change in the new text.
fn assemble(selection: &Selection, parts: Vec<(Change, usize, usize)>, kind: EditKind) -> Edit {
    let changes = ChangeSet::new(parts.iter().map(|(c, _, _)| c.clone()).collect());
    let ranges = parts
        .iter()
        .map(|(c, a, h)| {
            let base = changes.map_pos(c.start, phpide_core::Assoc::Before);
            Range::new(base + a, base + h)
        })
        .collect();
    Edit {
        changes,
        selection: Selection::new(ranges, selection.primary_index()),
        kind,
    }
}

fn slice(rope: &Rope, start: usize, end: usize) -> String {
    rope.slice(start.min(rope.len_chars())..end.min(rope.len_chars()))
        .to_string()
}

/// The word around `pos` (letters, digits, `_`), or an empty range.
fn word_around(rope: &Rope, pos: usize) -> (usize, usize) {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut start = pos;
    while start > 0 && is_word(rope.char(start - 1)) {
        start -= 1;
    }
    let mut end = pos;
    while end < rope.len_chars() && is_word(rope.char(end)) {
        end += 1;
    }
    (start, end)
}

/// Bold, italic, strikethrough and code: wraps the selection (or the
/// word at the caret) in `marker`, or removes the marker when it is
/// already there.
fn wrap(rope: &Rope, selection: &Selection, marker: &str) -> Edit {
    let m = marker.chars().count();
    let parts = selection
        .ranges()
        .iter()
        .map(|range| {
            let (mut start, mut end) = (range.start(), range.end());
            if start == end {
                (start, end) = word_around(rope, start);
            }
            let text = slice(rope, start, end);
            let inner_len = end - start;
            // The selection holds the markers: drop them.
            if inner_len >= 2 * m && text.starts_with(marker) && text.ends_with(marker) {
                let inner: String = text.chars().skip(m).take(inner_len - 2 * m).collect();
                let len = inner.chars().count();
                return (Change::new(start, end, inner), 0, len);
            }
            // The markers surround it: drop them.
            if start >= m
                && slice(rope, start - m, start) == marker
                && slice(rope, end, end + m) == marker
            {
                let len = text.chars().count();
                return (Change::new(start - m, end + m, text), 0, len);
            }
            let wrapped = format!("{marker}{text}{marker}");
            let len = text.chars().count();
            if len == 0 {
                (Change::new(start, end, wrapped), m, m)
            } else {
                (Change::new(start, end, wrapped), m, m + len)
            }
        })
        .collect();
    assemble(selection, parts, EditKind::Other)
}

/// A fenced code block around the selected lines, or an empty one at the
/// caret.
fn code_block(rope: &Rope, selection: &Selection) -> Edit {
    let parts = selection
        .ranges()
        .iter()
        .map(|range| {
            let first = rope.char_to_line(range.start());
            let last = rope.char_to_line(range.end().max(range.start()));
            let start = rope.line_to_char(first);
            let end = crate::textutil::line_end(rope, last);
            let body = slice(rope, start, end);
            if range.is_empty() && body.trim().is_empty() {
                let text = "```\n\n```".to_string();
                (Change::new(start, end, text), 4, 4)
            } else {
                let text = format!("```\n{body}\n```");
                let len = body.chars().count();
                (Change::new(start, end, text), 4, 4 + len)
            }
        })
        .collect();
    assemble(selection, parts, EditKind::Other)
}

/// `[text](url)` (or `![alt](url)`): the selected text becomes the label
/// and `url` is selected for typing; a selected address becomes the target.
fn link(rope: &Rope, selection: &Selection, image: bool) -> Edit {
    let bang = if image { "!" } else { "" };
    let parts = selection
        .ranges()
        .iter()
        .map(|range| {
            let (start, end) = (range.start(), range.end());
            let text = slice(rope, start, end);
            let is_url = text.starts_with("http://")
                || text.starts_with("https://")
                || text.starts_with("www.");
            if is_url {
                // Caret in the brackets.
                let at = bang.len() + 1;
                (Change::new(start, end, format!("{bang}[]({text})")), at, at)
            } else {
                let label = if text.is_empty() && image {
                    "alt"
                } else {
                    &text
                };
                let out = format!("{bang}[{label}](url)");
                let url_at = bang.len() + 1 + label.chars().count() + 2;
                (Change::new(start, end, out), url_at, url_at + 3)
            }
        })
        .collect();
    assemble(selection, parts, EditKind::Other)
}

#[derive(Clone, Copy, PartialEq)]
enum LinePrefix {
    Heading(u8),
    Bullet,
    Numbered,
    Task,
    Quote,
}

/// Length in chars of the list marker that starts `line` after its
/// indentation (`- `, `* `, `1. `, `- [ ] `), and whether it is a task.
pub fn list_marker(line: &str) -> Option<(usize, usize, bool)> {
    let indent = line.len() - line.trim_start().len();
    let rest = &line[indent..];
    let marker = if let Some(r) = rest
        .strip_prefix("- ")
        .or_else(|| rest.strip_prefix("* "))
        .or_else(|| rest.strip_prefix("+ "))
    {
        rest.len() - r.len()
    } else {
        let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 || digits > 9 {
            return None;
        }
        let after = &rest[digits..];
        if after.starts_with(". ") || after.starts_with(") ") {
            digits + 2
        } else {
            return None;
        }
    };
    let rest = &rest[marker..];
    let task = rest.starts_with("[ ] ") || rest.starts_with("[x] ") || rest.starts_with("[X] ");
    let len = marker + if task { 4 } else { 0 };
    Some((indent, len, task))
}

fn strip_heading(line: &str) -> &str {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && line[hashes..].starts_with(' ') {
        &line[hashes + 1..]
    } else if (1..=6).contains(&hashes) && line.len() == hashes {
        ""
    } else {
        line
    }
}

/// Headings, lists and quotes: sets (or, when every line has it, removes)
/// the prefix on each selected line.
fn lines(rope: &Rope, selection: &Selection, prefix: LinePrefix) -> Edit {
    let mut changes: Vec<Change> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for range in selection.ranges() {
        let first = rope.char_to_line(range.start());
        let last_pos = if range.end() > range.start()
            && rope.char_to_line(range.end()) > first
            && range.end() == rope.line_to_char(rope.char_to_line(range.end()))
        {
            range.end() - 1
        } else {
            range.end()
        };
        let last = rope.char_to_line(last_pos);
        let block: Vec<usize> = (first..=last).filter(|l| seen.insert(*l)).collect();
        let texts: Vec<String> = block.iter().map(|l| line_text(rope, *l)).collect();
        let has = |t: &str| -> bool {
            match prefix {
                LinePrefix::Heading(level) => {
                    let hashes = t.chars().take_while(|c| *c == '#').count();
                    hashes == level as usize && t[hashes..].starts_with(' ')
                }
                LinePrefix::Bullet => {
                    matches!(list_marker(t), Some((_, len, false)) if t.trim_start().starts_with(['-', '*', '+']) && len == 2)
                }
                LinePrefix::Numbered => {
                    matches!(list_marker(t), Some((_, _, false)) if t.trim_start().starts_with(|c: char| c.is_ascii_digit()))
                }
                LinePrefix::Task => matches!(list_marker(t), Some((_, _, true))),
                LinePrefix::Quote => t.trim_start().starts_with('>'),
            }
        };
        let remove = !texts.is_empty()
            && texts.iter().all(|t| has(t) || t.trim().is_empty())
            && texts.iter().any(|t| has(t))
            && !matches!(prefix, LinePrefix::Heading(0));
        let mut number = 0;
        for (line, text) in block.iter().zip(&texts) {
            if text.trim().is_empty() && block.len() > 1 {
                continue;
            }
            let line_start = rope.line_to_char(*line);
            let line_stop = line_start + text.chars().count();
            let indent: String = text
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            let body_with_marker = &text[indent.len()..];
            let new = match prefix {
                LinePrefix::Heading(level) => {
                    let body = strip_heading(text);
                    if remove || level == 0 {
                        body.to_string()
                    } else {
                        format!("{} {body}", "#".repeat(level.min(6) as usize))
                    }
                }
                LinePrefix::Quote => {
                    if remove {
                        let t = text.trim_start();
                        let t = t.strip_prefix('>').unwrap_or(t);
                        format!("{indent}{}", t.strip_prefix(' ').unwrap_or(t))
                    } else {
                        format!("> {text}")
                    }
                }
                LinePrefix::Bullet | LinePrefix::Numbered | LinePrefix::Task => {
                    let body = match list_marker(text) {
                        Some((i, len, _)) => &text[i + len..],
                        None => body_with_marker,
                    };
                    if remove {
                        format!("{indent}{body}")
                    } else {
                        number += 1;
                        let marker = match prefix {
                            LinePrefix::Bullet => "- ".to_string(),
                            LinePrefix::Numbered => format!("{number}. "),
                            _ => "- [ ] ".to_string(),
                        };
                        format!("{indent}{marker}{body}")
                    }
                }
            };
            if &new != text {
                changes.push(Change::new(line_start, line_stop, new));
            }
        }
    }
    let changes = ChangeSet::new(changes);
    Edit {
        selection: selection.map(&changes),
        changes,
        kind: EditKind::Other,
    }
}

#[cfg(test)]
fn rope_after(rope: &Rope, changes: &ChangeSet) -> Rope {
    let mut text = rope.clone();
    for change in changes.changes().iter().rev() {
        text.remove(change.start..change.end);
        text.insert(change.start, &change.text);
    }
    text
}

/// Inserts `block` on its own line after the caret's line; the caret goes
/// to `line` / `column` inside it.
fn insert_block(
    rope: &Rope,
    selection: &Selection,
    block: &str,
    line: usize,
    column: usize,
) -> Edit {
    let range = selection.primary();
    let caret_line = rope.char_to_line(range.end());
    let text = line_text(rope, caret_line);
    let line_start = rope.line_to_char(caret_line);
    let (at, lead) = if text.trim().is_empty() {
        (line_start, "")
    } else {
        (line_start + text.chars().count(), "\n\n")
    };
    let content = format!("{lead}{block}");
    let lead_len = lead.chars().count();
    let offset: usize = lead_len
        + block
            .split_inclusive('\n')
            .take(line)
            .map(|l| l.chars().count())
            .sum::<usize>()
        + column;
    let (insert_at, end) = if text.trim().is_empty() {
        (at, at + text.chars().count())
    } else {
        (at, at)
    };
    let one = Selection::single(range.anchor, range.head);
    assemble(
        &one,
        vec![(Change::new(insert_at, end, content), offset, offset)],
        EditKind::Other,
    )
}

/// Enter in a Markdown list or quote: the next line gets the same marker
/// (numbers count up, tasks start unchecked); Enter on an empty item ends
/// the list. `None` when the caret's line is no list item or quote.
pub fn continue_list(rope: &Rope, start: usize, end: usize) -> Option<(Change, usize)> {
    let line = rope.char_to_line(start);
    let line_start = rope.line_to_char(line);
    let before: String = rope.slice(line_start..start).to_string();
    if let Some((indent, len, task)) = list_marker(&before) {
        let marker = &before[indent..indent + len];
        let indent_text = &before[..indent];
        if before[indent + len..].trim().is_empty() && end == crate::textutil::line_end(rope, line)
        {
            // An empty item: the list ends here.
            return Some((Change::new(line_start, end, String::new()), 0));
        }
        let next = if task {
            "- [ ] ".to_string()
        } else if marker.starts_with(|c: char| c.is_ascii_digit()) {
            let digits: String = marker.chars().take_while(|c| c.is_ascii_digit()).collect();
            let n: u64 = digits.parse().unwrap_or(0);
            let sep = &marker[digits.len()..digits.len() + 1];
            format!("{}{sep} ", n + 1)
        } else {
            marker.to_string()
        };
        let text = format!("\n{indent_text}{next}");
        let caret = text.chars().count();
        return Some((Change::new(start, end, text), caret));
    }
    let trimmed = before.trim_start();
    if let Some(rest) = trimmed.strip_prefix('>') {
        let indent = &before[..before.len() - trimmed.len()];
        if rest.trim().is_empty() && end == crate::textutil::line_end(rope, line) {
            return Some((Change::new(line_start, end, String::new()), 0));
        }
        let text = format!("\n{indent}> ");
        let caret = text.chars().count();
        return Some((Change::new(start, end, text), caret));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(text: &str, anchor: usize, head: usize, action: MarkdownEdit) -> (String, String) {
        let rope = Rope::from_str(text);
        let selection = Selection::single(anchor, head);
        let edit = run(&rope, &selection, action).unwrap();
        let after = rope_after(&rope, &edit.changes).to_string();
        let r = edit.selection.primary();
        let selected: String = after
            .chars()
            .skip(r.start())
            .take(r.end() - r.start())
            .collect();
        (after, selected)
    }

    #[test]
    fn bold_wraps_and_unwraps() {
        assert_eq!(
            apply("say hello now", 4, 9, MarkdownEdit::Bold),
            ("say **hello** now".into(), "hello".into())
        );
        assert_eq!(
            apply("say **hello** now", 6, 11, MarkdownEdit::Bold),
            ("say hello now".into(), "hello".into())
        );
        assert_eq!(
            apply("say **hello** now", 4, 13, MarkdownEdit::Bold),
            ("say hello now".into(), "hello".into())
        );
        // The caret in a word takes the word.
        assert_eq!(
            apply("say hello now", 6, 6, MarkdownEdit::Italic).0,
            "say _hello_ now"
        );
        // Nothing to take: the markers and the caret between them.
        let (text, _) = apply("a  b", 2, 2, MarkdownEdit::Code);
        assert_eq!(text, "a `` b");
        assert_eq!(apply("x", 0, 1, MarkdownEdit::Strikethrough).0, "~~x~~");
    }

    #[test]
    fn links_select_the_part_to_type() {
        assert_eq!(
            apply("see docs", 4, 8, MarkdownEdit::Link),
            ("see [docs](url)".into(), "url".into())
        );
        let (text, selected) = apply("https://php.net", 0, 15, MarkdownEdit::Link);
        assert_eq!(
            (text.as_str(), selected.as_str()),
            ("[](https://php.net)", "")
        );
        assert_eq!(apply("", 0, 0, MarkdownEdit::Image).0, "![alt](url)");
    }

    #[test]
    fn headings_and_lists_on_lines() {
        assert_eq!(
            apply("Title\n", 2, 2, MarkdownEdit::Heading(2)).0,
            "## Title\n"
        );
        assert_eq!(
            apply("# Title\n", 2, 2, MarkdownEdit::Heading(3)).0,
            "### Title\n"
        );
        assert_eq!(
            apply("## Title\n", 2, 2, MarkdownEdit::Heading(0)).0,
            "Title\n"
        );
        assert_eq!(
            apply("## Title\n", 2, 2, MarkdownEdit::Heading(2)).0,
            "Title\n",
            "same level toggles off"
        );
        assert_eq!(
            apply("a\nb\n", 0, 3, MarkdownEdit::BulletList).0,
            "- a\n- b\n"
        );
        assert_eq!(
            apply("- a\n- b\n", 0, 7, MarkdownEdit::BulletList).0,
            "a\nb\n"
        );
        assert_eq!(
            apply("- a\n- b\n", 0, 7, MarkdownEdit::NumberedList).0,
            "1. a\n2. b\n"
        );
        assert_eq!(apply("a\n", 0, 0, MarkdownEdit::TaskList).0, "- [ ] a\n");
        assert_eq!(apply("a\nb", 0, 3, MarkdownEdit::Quote).0, "> a\n> b");
        assert_eq!(apply("> a\n> b", 0, 7, MarkdownEdit::Quote).0, "a\nb");
    }

    #[test]
    fn blocks_go_on_their_own_lines() {
        let (text, _) = apply("intro", 5, 5, MarkdownEdit::Table);
        assert!(
            text.starts_with("intro\n\n| Column 1 | Column 2 |\n| -------- |"),
            "{text}"
        );
        assert_eq!(apply("", 0, 0, MarkdownEdit::Rule).0, "---\n");
        let (text, _) = apply("let x = 1;", 0, 10, MarkdownEdit::CodeBlock);
        assert_eq!(text, "```\nlet x = 1;\n```");
    }

    #[test]
    fn enter_continues_lists_and_quotes() {
        let go = |text: &str| {
            let rope = Rope::from_str(text);
            let pos = rope.len_chars();
            continue_list(&rope, pos, pos).map(|(c, caret)| {
                let mut after = rope.clone();
                after.remove(c.start..c.end);
                after.insert(c.start, &c.text);
                (after.to_string(), c.start + caret)
            })
        };
        assert_eq!(go("- one").unwrap().0, "- one\n- ");
        assert_eq!(go("  * one").unwrap().0, "  * one\n  * ");
        assert_eq!(go("9. nine").unwrap().0, "9. nine\n10. ");
        assert_eq!(go("- [x] done").unwrap().0, "- [x] done\n- [ ] ");
        assert_eq!(go("> said").unwrap().0, "> said\n> ");
        assert_eq!(
            go("- one\n- ").unwrap().0,
            "- one\n",
            "an empty item ends the list"
        );
        assert_eq!(go("plain"), None);
        assert_eq!(list_marker("12) x"), Some((0, 4, false)));
        assert_eq!(list_marker("-x"), None);
    }
}
