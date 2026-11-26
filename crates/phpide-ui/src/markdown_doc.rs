//! A Markdown document model for the preview: CommonMark blocks (headings,
//! paragraphs, lists, quotes, fenced and indented code, rules, HTML) plus
//! the GitHub extensions people write every day (tables, task lists,
//! strikethrough, bare links). Each block keeps the source line it starts
//! on, so the preview scrolls with the editor and a task box can be ticked
//! in the text.

/// An inline run of a paragraph, heading or table cell.
#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    Text(String),
    Code(String),
    Strong(Vec<Inline>),
    Emphasis(Vec<Inline>),
    Strike(Vec<Inline>),
    Link { label: Vec<Inline>, url: String },
    Image { alt: String, url: String },
    LineBreak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListItem {
    /// `Some(done)` for `- [ ]` / `- [x]`, with the source line of the box.
    pub task: Option<(bool, usize)>,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BlockKind {
    Heading {
        level: u8,
        text: Vec<Inline>,
        anchor: String,
    },
    Paragraph(Vec<Inline>),
    Code {
        lang: String,
        text: String,
    },
    Quote(Vec<Block>),
    List {
        ordered: bool,
        start: u64,
        items: Vec<ListItem>,
    },
    Table {
        aligns: Vec<Align>,
        header: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    Rule,
    Html(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    /// First source line (0-based).
    pub line: usize,
    pub kind: BlockKind,
}

/// Parses a whole document.
pub fn parse(text: &str) -> Vec<Block> {
    let lines: Vec<&str> = text.lines().collect();
    let owned: Vec<String> = lines.iter().map(|l| l.replace('\t', "    ")).collect();
    let refs: Vec<(usize, &str)> = owned
        .iter()
        .enumerate()
        .map(|(i, l)| (i, l.as_str()))
        .collect();
    parse_blocks(&refs)
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

fn is_rule(line: &str) -> bool {
    let t = line.trim();
    if indent_of(line) > 3 || t.len() < 3 {
        return false;
    }
    let first = t.chars().next().unwrap_or(' ');
    matches!(first, '-' | '*' | '_')
        && t.chars().all(|c| c == first || c == ' ')
        && t.chars().filter(|c| *c == first).count() >= 3
}

fn atx_heading(line: &str) -> Option<(u8, &str)> {
    if indent_of(line) > 3 {
        return None;
    }
    let t = line.trim_start();
    let level = t.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &t[level..];
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    // Closing hashes go.
    let rest = rest.trim();
    let rest = rest.trim_end_matches('#');
    let rest = if rest.ends_with(' ') || rest.is_empty() {
        rest.trim_end()
    } else {
        t[level..].trim()
    };
    Some((level as u8, rest))
}

fn fence(line: &str) -> Option<(char, usize, String)> {
    if indent_of(line) > 3 {
        return None;
    }
    let t = line.trim_start();
    let c = t.chars().next()?;
    if c != '`' && c != '~' {
        return None;
    }
    let n = t.chars().take_while(|x| *x == c).count();
    if n < 3 {
        return None;
    }
    let info = t[n..].trim();
    if c == '`' && info.contains('`') {
        return None;
    }
    Some((
        c,
        n,
        info.split_whitespace().next().unwrap_or("").to_string(),
    ))
}

/// `(content indent, marker width, ordered, start number)` of a list item
/// line.
fn list_item(line: &str) -> Option<(usize, bool, u64, char)> {
    let indent = indent_of(line);
    if indent > 3 {
        return None;
    }
    let t = &line[indent..];
    let (width, ordered, start, delim) =
        if let Some(c) = t.chars().next().filter(|c| matches!(c, '-' | '*' | '+')) {
            (1, false, 1, c)
        } else {
            let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
            if digits == 0 || digits > 9 {
                return None;
            }
            let delim = t[digits..].chars().next()?;
            if delim != '.' && delim != ')' {
                return None;
            }
            (digits + 1, true, t[..digits].parse().unwrap_or(1), delim)
        };
    let after = &t[width..];
    if !(after.is_empty() || after.starts_with(' ')) {
        return None;
    }
    let spaces = after.len() - after.trim_start_matches(' ').len();
    // An empty item, or code indented past the marker: one space.
    let spaces = if after.trim().is_empty() || spaces > 4 {
        1
    } else {
        spaces.max(1)
    };
    Some((indent + width + spaces, ordered, start, delim))
}

fn table_cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cell.push('|');
                chars.next();
            }
            '|' => cells.push(std::mem::take(&mut cell).trim().to_string()),
            _ => cell.push(c),
        }
    }
    cells.push(cell.trim().to_string());
    cells
}

fn table_delimiter(line: &str) -> Option<Vec<Align>> {
    if !line.contains('-') {
        return None;
    }
    let cells = table_cells(line);
    let mut aligns = Vec::new();
    for cell in cells {
        let c = cell.trim();
        if c.is_empty() || !c.chars().all(|x| matches!(x, '-' | ':')) || !c.contains('-') {
            return None;
        }
        aligns.push(match (c.starts_with(':'), c.ends_with(':')) {
            (true, true) => Align::Center,
            (true, false) => Align::Left,
            (false, true) => Align::Right,
            _ => Align::None,
        });
    }
    Some(aligns)
}

fn html_start(line: &str) -> bool {
    let t = line.trim_start();
    indent_of(line) <= 3
        && t.starts_with('<')
        && t[1..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '/' || c == '!')
}

/// Whether `line` would start a block that interrupts a paragraph.
fn interrupts(line: &str) -> bool {
    atx_heading(line).is_some()
        || fence(line).is_some()
        || is_rule(line)
        || line.trim_start().starts_with('>') && indent_of(line) <= 3
        || list_item(line).is_some_and(|(_, ordered, start, _)| !ordered || start == 1)
            && !is_blank(&line[list_item(line).map_or(0, |l| l.0.min(line.len()))..])
        || html_start(line)
}

/// The anchor GitHub gives a heading (`## Getting Started` → `getting-started`).
pub fn anchor(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                Some(c)
            } else if c == ' ' {
                Some('-')
            } else {
                None
            }
        })
        .collect()
}

pub fn plain_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Text(t) | Inline::Code(t) => out.push_str(t),
            Inline::Strong(i) | Inline::Emphasis(i) | Inline::Strike(i) => {
                out.push_str(&plain_text(i))
            }
            Inline::Link { label, .. } => out.push_str(&plain_text(label)),
            Inline::Image { alt, .. } => out.push_str(alt),
            Inline::LineBreak => out.push(' '),
        }
    }
    out
}

fn parse_blocks(lines: &[(usize, &str)]) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let (number, line) = lines[i];
        if is_blank(line) {
            i += 1;
            continue;
        }
        // Fenced code.
        if let Some((c, n, lang)) = fence(line) {
            let indent = indent_of(line);
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() {
                let l = lines[i].1;
                if let Some((c2, n2, info)) = fence(l) {
                    if c2 == c && n2 >= n && info.is_empty() {
                        i += 1;
                        break;
                    }
                }
                let strip = indent_of(l).min(indent);
                body.push(&l[strip..]);
                i += 1;
            }
            blocks.push(Block {
                line: number,
                kind: BlockKind::Code {
                    lang,
                    text: body.join("\n"),
                },
            });
            continue;
        }
        if let Some((level, text)) = atx_heading(line) {
            let inlines = parse_inlines(text);
            blocks.push(Block {
                line: number,
                kind: BlockKind::Heading {
                    level,
                    anchor: anchor(&plain_text(&inlines)),
                    text: inlines,
                },
            });
            i += 1;
            continue;
        }
        if is_rule(line) {
            blocks.push(Block {
                line: number,
                kind: BlockKind::Rule,
            });
            i += 1;
            continue;
        }
        // Indented code.
        if indent_of(line) >= 4 {
            let mut body = Vec::new();
            while i < lines.len() && (indent_of(lines[i].1) >= 4 || is_blank(lines[i].1)) {
                let l = lines[i].1;
                body.push(if l.len() >= 4 { &l[4..] } else { "" });
                i += 1;
            }
            while body.last().is_some_and(|l| l.trim().is_empty()) {
                body.pop();
            }
            blocks.push(Block {
                line: number,
                kind: BlockKind::Code {
                    lang: String::new(),
                    text: body.join("\n"),
                },
            });
            continue;
        }
        // Block quote: `>` lines, and lazy paragraph lines after them.
        if line.trim_start().starts_with('>') {
            let mut inner: Vec<(usize, &str)> = Vec::new();
            while i < lines.len() {
                let (n, l) = lines[i];
                let t = l.trim_start();
                if let Some(rest) = t.strip_prefix('>') {
                    inner.push((n, rest.strip_prefix(' ').unwrap_or(rest)));
                } else if !is_blank(l)
                    && !interrupts(l)
                    && inner.last().is_some_and(|x| !is_blank(x.1))
                {
                    inner.push((n, l));
                } else {
                    break;
                }
                i += 1;
            }
            blocks.push(Block {
                line: number,
                kind: BlockKind::Quote(parse_blocks(&inner)),
            });
            continue;
        }
        // Lists.
        if let Some((_, ordered, start, delim)) = list_item(line) {
            let mut items = Vec::new();
            let mut loose_gap = false;
            while i < lines.len() {
                let (n, l) = lines[i];
                let Some((content, o, _, d)) = list_item(l) else {
                    break;
                };
                if o != ordered || d != delim {
                    break;
                }
                let _ = loose_gap;
                // The item's lines: its first line after the marker, then
                // lines indented to its content (and blank lines between).
                let mut body: Vec<(usize, &str)> =
                    vec![(n, if l.len() > content { &l[content..] } else { "" })];
                i += 1;
                while i < lines.len() {
                    let (n2, l2) = lines[i];
                    if is_blank(l2) {
                        // A blank line belongs to the item if more of it follows.
                        let next = lines[i + 1..].iter().find(|x| !is_blank(x.1));
                        match next {
                            Some((_, nl)) if indent_of(nl) >= content => {
                                body.push((n2, ""));
                                i += 1;
                                continue;
                            }
                            _ => break,
                        }
                    }
                    if indent_of(l2) >= content {
                        body.push((n2, &l2[content..]));
                        i += 1;
                    } else if list_item(l2).is_none()
                        && !interrupts(l2)
                        && body.last().is_some_and(|x| !is_blank(x.1))
                    {
                        // Lazy continuation of the item's paragraph.
                        body.push((n2, l2.trim_start()));
                        i += 1;
                    } else {
                        break;
                    }
                }
                // A task box.
                let mut task = None;
                let first = body[0].1;
                for (mark, done) in [("[ ] ", false), ("[x] ", true), ("[X] ", true)] {
                    if first.starts_with(mark) || first == mark.trim_end() {
                        task = Some((done, body[0].0));
                        body[0].1 = first.get(4..).unwrap_or("");
                    }
                }
                items.push(ListItem {
                    task,
                    blocks: parse_blocks(&body),
                });
                // Blank lines between items.
                let mut j = i;
                while j < lines.len() && is_blank(lines[j].1) {
                    j += 1;
                }
                if j < lines.len()
                    && j > i
                    && list_item(lines[j].1).is_some_and(|(_, o, _, d)| o == ordered && d == delim)
                {
                    loose_gap = true;
                    i = j;
                }
            }
            blocks.push(Block {
                line: number,
                kind: BlockKind::List {
                    ordered,
                    start,
                    items,
                },
            });
            continue;
        }
        // Table: a header row, then the delimiter row.
        if line.contains('|') && i + 1 < lines.len() {
            if let Some(aligns) = table_delimiter(lines[i + 1].1) {
                let header = table_cells(line);
                if header.len() == aligns.len() {
                    i += 2;
                    let mut rows = Vec::new();
                    while i < lines.len() && !is_blank(lines[i].1) && !interrupts(lines[i].1) {
                        let mut cells = table_cells(lines[i].1);
                        cells.resize(aligns.len(), String::new());
                        rows.push(cells.iter().map(|c| parse_inlines(c)).collect());
                        i += 1;
                    }
                    blocks.push(Block {
                        line: number,
                        kind: BlockKind::Table {
                            aligns,
                            header: header.iter().map(|c| parse_inlines(c)).collect(),
                            rows,
                        },
                    });
                    continue;
                }
            }
        }
        if html_start(line) {
            let mut body = Vec::new();
            while i < lines.len() && !is_blank(lines[i].1) {
                body.push(lines[i].1);
                i += 1;
            }
            blocks.push(Block {
                line: number,
                kind: BlockKind::Html(body.join("\n")),
            });
            continue;
        }
        // Paragraph (or setext heading).
        let mut text: Vec<&str> = vec![line.trim_start()];
        i += 1;
        let mut setext = None;
        while i < lines.len() {
            let l = lines[i].1;
            if is_blank(l) {
                break;
            }
            let t = l.trim();
            if indent_of(l) <= 3 && !t.is_empty() && t.chars().all(|c| c == '=') {
                setext = Some(1);
                i += 1;
                break;
            }
            if indent_of(l) <= 3 && !t.is_empty() && t.chars().all(|c| c == '-') {
                setext = Some(2);
                i += 1;
                break;
            }
            if interrupts(l) {
                break;
            }
            text.push(l.trim_start());
            i += 1;
        }
        let joined = join_lines(&text);
        let inlines = parse_inlines(&joined);
        blocks.push(Block {
            line: number,
            kind: match setext {
                Some(level) => BlockKind::Heading {
                    level,
                    anchor: anchor(&plain_text(&inlines)),
                    text: inlines,
                },
                None => BlockKind::Paragraph(inlines),
            },
        });
    }
    blocks
}

/// Lines of a paragraph: a line ending in two spaces or `\` breaks.
fn join_lines(lines: &[&str]) -> String {
    let mut out = String::new();
    for (n, line) in lines.iter().enumerate() {
        let last = n + 1 == lines.len();
        if !last && line.ends_with("  ") {
            out.push_str(line.trim_end());
            out.push('\u{2028}');
        } else if !last && line.ends_with('\\') {
            out.push_str(&line[..line.len() - 1]);
            out.push('\u{2028}');
        } else {
            out.push_str(line.trim_end());
            if !last {
                out.push(' ');
            }
        }
    }
    out
}

/// Parses the inline content of one block.
pub fn parse_inlines(text: &str) -> Vec<Inline> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    inlines_into(&chars, &mut out);
    merge_text(out)
}

fn merge_text(list: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::new();
    for inline in list {
        match (out.last_mut(), inline) {
            (Some(Inline::Text(a)), Inline::Text(b)) => a.push_str(&b),
            (_, other) => out.push(other),
        }
    }
    out
}

fn find_seq(chars: &[char], from: usize, seq: &[char]) -> Option<usize> {
    (from..chars.len().saturating_sub(seq.len() - 1)).find(|&i| chars[i..].starts_with(seq))
}

/// The closing `delim` of an emphasis opened before `from`: not after a
/// space, and (for `_`) not inside a word.
fn closing(chars: &[char], from: usize, delim: &[char]) -> Option<usize> {
    let mut i = from;
    while let Some(at) = find_seq(chars, i, delim) {
        let before = at.checked_sub(1).map(|b| chars[b]);
        let after = chars.get(at + delim.len()).copied();
        let flanking = before.is_some_and(|c| !c.is_whitespace()) && at > from;
        let word_after = delim[0] == '_' && after.is_some_and(|c| c.is_alphanumeric());
        // Skip a longer run (`***` when looking for `*`).
        let longer = delim.len() == 1 && after == Some(delim[0]) && delim[0] != '`';
        if flanking && !word_after && !longer {
            return Some(at);
        }
        i = at + delim.len();
        if longer {
            i += 1;
        }
    }
    None
}

/// `[label](url "title")` starting at `[`: (label end, url, end).
fn link_at(chars: &[char], open: usize) -> Option<(usize, String, usize)> {
    let mut depth = 0;
    let mut close = None;
    let mut i = open;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 1,
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            }
            _ => {}
        }
        i += 1;
    }
    let close = close?;
    if chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let mut depth = 0;
    let mut end = None;
    for (j, c) in chars.iter().enumerate().skip(close + 1) {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(j);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end?;
    let inside: String = chars[close + 2..end].iter().collect();
    let inside = inside.trim();
    let url = if let Some(rest) = inside.strip_prefix('<') {
        rest.split('>').next().unwrap_or("").to_string()
    } else {
        inside.split_whitespace().next().unwrap_or("").to_string()
    };
    Some((close, url, end))
}

fn inlines_into(chars: &[char], out: &mut Vec<Inline>) {
    let mut text = String::new();
    let mut i = 0;
    let flush = |text: &mut String, out: &mut Vec<Inline>| {
        if !text.is_empty() {
            out.push(Inline::Text(std::mem::take(text)));
        }
    };
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' if chars.get(i + 1).is_some_and(|n| n.is_ascii_punctuation()) => {
                text.push(chars[i + 1]);
                i += 2;
            }
            '\u{2028}' => {
                flush(&mut text, out);
                out.push(Inline::LineBreak);
                i += 1;
            }
            '`' => {
                let run = chars[i..].iter().take_while(|x| **x == '`').count();
                let delim = vec!['`'; run];
                let close = (i + run..chars.len()).find(|&j| {
                    chars[j..].starts_with(&delim)
                        && chars.get(j + run) != Some(&'`')
                        && (j == 0 || chars[j - 1] != '`')
                });
                match close {
                    Some(end) => {
                        flush(&mut text, out);
                        let code: String = chars[i + run..end].iter().collect();
                        let code =
                            if code.starts_with(' ') && code.ends_with(' ') && code.trim() != "" {
                                code[1..code.len() - 1].to_string()
                            } else {
                                code
                            };
                        out.push(Inline::Code(code));
                        i = end + run;
                    }
                    None => {
                        text.extend(std::iter::repeat_n('`', run));
                        i += run;
                    }
                }
            }
            '!' if chars.get(i + 1) == Some(&'[') => match link_at(chars, i + 1) {
                Some((close, url, end)) => {
                    flush(&mut text, out);
                    out.push(Inline::Image {
                        alt: chars[i + 2..close].iter().collect(),
                        url,
                    });
                    i = end + 1;
                }
                None => {
                    text.push('!');
                    i += 1;
                }
            },
            '[' => match link_at(chars, i) {
                Some((close, url, end)) => {
                    flush(&mut text, out);
                    let mut label = Vec::new();
                    inlines_into(&chars[i + 1..close], &mut label);
                    out.push(Inline::Link {
                        label: merge_text(label),
                        url,
                    });
                    i = end + 1;
                }
                None => {
                    text.push('[');
                    i += 1;
                }
            },
            '<' => {
                let rest: String = chars[i + 1..].iter().take(200).collect();
                let end = rest.find('>');
                match end {
                    Some(e)
                        if rest[..e].starts_with("http://")
                            || rest[..e].starts_with("https://")
                            || rest[..e].starts_with("mailto:") =>
                    {
                        flush(&mut text, out);
                        let url = rest[..e].to_string();
                        out.push(Inline::Link {
                            label: vec![Inline::Text(url.clone())],
                            url,
                        });
                        i += 1 + rest[..e].chars().count() + 1;
                    }
                    _ => {
                        text.push('<');
                        i += 1;
                    }
                }
            }
            'h' if (chars[i..].starts_with(&['h', 't', 't', 'p', ':', '/', '/'])
                || chars[i..].starts_with(&['h', 't', 't', 'p', 's', ':', '/', '/']))
                && (i == 0 || !chars[i - 1].is_alphanumeric()) =>
            {
                let mut end = i;
                while end < chars.len() && !chars[end].is_whitespace() && chars[end] != '<' {
                    end += 1;
                }
                while end > i && matches!(chars[end - 1], '.' | ',' | ':' | ';' | ')' | '!' | '?') {
                    end -= 1;
                }
                flush(&mut text, out);
                let url: String = chars[i..end].iter().collect();
                out.push(Inline::Link {
                    label: vec![Inline::Text(url.clone())],
                    url,
                });
                i = end;
            }
            '*' | '_' | '~' => {
                let run = chars[i..].iter().take_while(|x| **x == c).count();
                let next = chars.get(i + run).copied();
                let prev = i.checked_sub(1).map(|p| chars[p]);
                let opens = next.is_some_and(|n| !n.is_whitespace())
                    && !(c == '_' && prev.is_some_and(|p| p.is_alphanumeric()));
                let mut done = false;
                if opens {
                    // Strong first (`**`), then emphasis, then both (`***`).
                    let tries: Vec<(usize, u8)> = match (c, run) {
                        ('~', n) if n >= 2 => vec![(2, b's')],
                        ('~', _) => vec![],
                        (_, 1) => vec![(1, b'e')],
                        (_, 2) => vec![(2, b'b')],
                        _ => vec![(3, b'x'), (2, b'b'), (1, b'e')],
                    };
                    for (len, kind) in tries {
                        let delim = vec![c; len];
                        if let Some(end) = closing(chars, i + len, &delim) {
                            flush(&mut text, out);
                            let mut inner = Vec::new();
                            inlines_into(&chars[i + len..end], &mut inner);
                            let inner = merge_text(inner);
                            out.push(match kind {
                                b's' => Inline::Strike(inner),
                                b'b' => Inline::Strong(inner),
                                b'x' => Inline::Strong(vec![Inline::Emphasis(inner)]),
                                _ => Inline::Emphasis(inner),
                            });
                            i = end + len;
                            done = true;
                            break;
                        }
                    }
                }
                if !done {
                    text.extend(std::iter::repeat_n(c, run));
                    i += run;
                }
            }
            '&' => {
                let rest: String = chars[i..].iter().take(10).collect();
                let entity = [
                    ("&amp;", '&'),
                    ("&lt;", '<'),
                    ("&gt;", '>'),
                    ("&quot;", '"'),
                    ("&nbsp;", '\u{a0}'),
                    ("&#39;", '\''),
                ]
                .into_iter()
                .find(|(e, _)| rest.starts_with(e));
                match entity {
                    Some((e, ch)) => {
                        text.push(ch);
                        i += e.len();
                    }
                    None => {
                        text.push('&');
                        i += 1;
                    }
                }
            }
            _ => {
                text.push(c);
                i += 1;
            }
        }
    }
    flush(&mut text, out);
}

/// The heading outline of a document: (level, text, line).
pub fn outline(blocks: &[Block]) -> Vec<(u8, String, usize)> {
    let mut out = Vec::new();
    for block in blocks {
        match &block.kind {
            BlockKind::Heading { level, text, .. } => {
                out.push((*level, plain_text(text), block.line))
            }
            BlockKind::Quote(inner) => out.extend(outline(inner)),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Inline {
        Inline::Text(s.into())
    }

    #[test]
    fn headings_paragraphs_and_rules() {
        let doc = parse("# Title #\n\nSome *text*\nnext line.\n\nSub\n---\n\n***\n");
        assert_eq!(doc.len(), 4);
        assert_eq!(
            doc[0].kind,
            BlockKind::Heading {
                level: 1,
                text: vec![text("Title")],
                anchor: "title".into()
            }
        );
        assert_eq!(doc[1].line, 2);
        assert_eq!(
            doc[1].kind,
            BlockKind::Paragraph(vec![
                text("Some "),
                Inline::Emphasis(vec![text("text")]),
                text(" next line.")
            ])
        );
        assert!(matches!(&doc[2].kind, BlockKind::Heading { level: 2, .. }));
        assert_eq!(doc[2].line, 5);
        assert_eq!(doc[3].kind, BlockKind::Rule);
        assert_eq!(anchor("Getting Started, v2!"), "getting-started-v2");
    }

    #[test]
    fn inline_styles_links_and_code() {
        let inl = parse_inlines("**bold** _it_ ~~gone~~ `a*b` [site](https://x.dev \"t\") ![logo](img/a.png) <https://a.b> see https://php.net.");
        assert_eq!(inl[0], Inline::Strong(vec![text("bold")]));
        assert_eq!(inl[2], Inline::Emphasis(vec![text("it")]));
        assert_eq!(inl[4], Inline::Strike(vec![text("gone")]));
        assert_eq!(inl[6], Inline::Code("a*b".into()));
        assert_eq!(
            inl[8],
            Inline::Link {
                label: vec![text("site")],
                url: "https://x.dev".into()
            }
        );
        assert_eq!(
            inl[10],
            Inline::Image {
                alt: "logo".into(),
                url: "img/a.png".into()
            }
        );
        assert_eq!(
            inl[12],
            Inline::Link {
                label: vec![text("https://a.b")],
                url: "https://a.b".into()
            }
        );
        assert_eq!(
            inl[14],
            Inline::Link {
                label: vec![text("https://php.net")],
                url: "https://php.net".into()
            }
        );
        assert_eq!(inl[15], text("."));
        // Not emphasis: snake_case, a lone star, escapes.
        assert_eq!(
            parse_inlines("snake_case_name"),
            vec![text("snake_case_name")]
        );
        assert_eq!(parse_inlines("2 * 3 = 6"), vec![text("2 * 3 = 6")]);
        assert_eq!(
            parse_inlines("\\*no\\* &amp; <b>"),
            vec![text("*no* & <b>")]
        );
        assert_eq!(
            parse_inlines("***both***"),
            vec![Inline::Strong(vec![Inline::Emphasis(vec![text("both")])])]
        );
    }

    #[test]
    fn lists_tasks_and_nesting() {
        let doc = parse("- one\n- [x] done\n  - inner\n- [ ] todo\n\n3. three\n4. four\n");
        let BlockKind::List { ordered, items, .. } = &doc[0].kind else {
            panic!("{:?}", doc[0]);
        };
        assert!(!ordered);
        assert_eq!(items.len(), 3);
        assert_eq!(items[1].task, Some((true, 1)));
        assert_eq!(items[2].task, Some((false, 3)));
        assert!(
            matches!(&items[1].blocks[1].kind, BlockKind::List { .. }),
            "{:?}",
            items[1].blocks
        );
        let BlockKind::List {
            ordered,
            start,
            items,
        } = &doc[1].kind
        else {
            panic!();
        };
        assert!(*ordered);
        assert_eq!((*start, items.len()), (3, 2));
        assert_eq!(doc[1].line, 5);
    }

    #[test]
    fn code_quotes_tables_and_html() {
        let doc = parse("```php\n<?php echo 1;\n```\n\n    indented\n\n> quoted\nlazy\n> > nested\n\n| A | B:|\n|:--|--:|\n| 1 | 2 |\n| x |\n\n<div>\nhi\n</div>\n");
        assert_eq!(
            doc[0].kind,
            BlockKind::Code {
                lang: "php".into(),
                text: "<?php echo 1;".into()
            }
        );
        assert_eq!(
            doc[1].kind,
            BlockKind::Code {
                lang: String::new(),
                text: "indented".into()
            }
        );
        let BlockKind::Quote(inner) = &doc[2].kind else {
            panic!("{:?}", doc[2])
        };
        assert_eq!(
            inner[0].kind,
            BlockKind::Paragraph(vec![text("quoted lazy")])
        );
        assert!(matches!(&inner[1].kind, BlockKind::Quote(_)));
        let BlockKind::Table {
            aligns,
            header,
            rows,
        } = &doc[3].kind
        else {
            panic!("{:?}", doc[3])
        };
        assert_eq!(aligns, &[Align::Left, Align::Right]);
        assert_eq!(header[1], vec![text("B:")]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1][1], Vec::<Inline>::new(), "short rows are padded");
        assert_eq!(doc[4].kind, BlockKind::Html("<div>\nhi\n</div>".into()));
        assert_eq!(doc[3].line, 10);
    }

    #[test]
    fn outline_lists_headings() {
        let doc = parse("# A\ntext\n## B\n> ### C\n");
        assert_eq!(
            outline(&doc),
            vec![(1, "A".into(), 0), (2, "B".into(), 2), (3, "C".into(), 3)]
        );
    }
}
