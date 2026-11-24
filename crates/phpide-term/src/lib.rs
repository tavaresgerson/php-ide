//! Integrated terminal of PHP IDE: a pseudo-terminal running the user's
//! shell ([`pty`]) and an xterm-compatible emulator ([`term`]), plus the key
//! encoding and the `file:line` links the UI needs. No GUI code here.

pub mod pty;
pub mod term;

pub use pty::{Command, Pty};
pub use term::{Cell, Color, Row, Style, Terminal};

/// Keys the terminal encodes (the UI maps its own key type onto these).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Enter,
    Backspace,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    F(u8),
    /// A letter or symbol pressed with Ctrl and/or Alt.
    Char(char),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

/// Bytes a key sends, as xterm does. `None` when the key sends nothing.
pub fn encode_key(key: Key, mods: Mods, app_cursor: bool) -> Option<Vec<u8>> {
    // xterm modifier parameter: 1 + shift + 2·alt + 4·ctrl.
    let modifier = 1 + mods.shift as u8 + 2 * mods.alt as u8 + 4 * mods.ctrl as u8;
    let csi = |final_byte: char| -> Vec<u8> {
        if modifier > 1 {
            format!("\x1b[1;{modifier}{final_byte}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{final_byte}").into_bytes()
        } else {
            format!("\x1b[{final_byte}").into_bytes()
        }
    };
    let tilde = |code: u8| -> Vec<u8> {
        if modifier > 1 {
            format!("\x1b[{code};{modifier}~").into_bytes()
        } else {
            format!("\x1b[{code}~").into_bytes()
        }
    };
    let alt_prefix = |mut bytes: Vec<u8>| -> Vec<u8> {
        if mods.alt {
            bytes.insert(0, 0x1b);
        }
        bytes
    };
    Some(match key {
        Key::Enter => alt_prefix(vec![b'\r']),
        Key::Backspace => alt_prefix(vec![if mods.ctrl { 0x08 } else { 0x7f }]),
        Key::Tab if mods.shift => b"\x1b[Z".to_vec(),
        Key::Tab => vec![b'\t'],
        Key::Escape => vec![0x1b],
        Key::Up => csi('A'),
        Key::Down => csi('B'),
        Key::Right => csi('C'),
        Key::Left => csi('D'),
        Key::Home => csi('H'),
        Key::End => csi('F'),
        Key::Insert => tilde(2),
        Key::Delete => tilde(3),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::F(n @ 1..=4) => {
            let letter = (b'P' + n - 1) as char;
            if modifier > 1 {
                format!("\x1b[1;{modifier}{letter}").into_bytes()
            } else {
                format!("\x1bO{letter}").into_bytes()
            }
        }
        Key::F(n @ 5..=12) => tilde([15, 17, 18, 19, 20, 21, 23, 24][(n - 5) as usize]),
        Key::F(_) => return None,
        Key::Char(c) => {
            let mut bytes = Vec::new();
            if mods.ctrl {
                let lower = c.to_ascii_lowercase();
                let control = match lower {
                    'a'..='z' => Some(lower as u8 - b'a' + 1),
                    '@' | ' ' | '2' => Some(0),
                    '[' | '3' => Some(0x1b),
                    '\\' | '4' => Some(0x1c),
                    ']' | '5' => Some(0x1d),
                    '^' | '6' => Some(0x1e),
                    '_' | '-' | '7' => Some(0x1f),
                    '?' | '8' => Some(0x7f),
                    _ => None,
                };
                bytes.push(control?);
            } else {
                let mut buf = [0u8; 4];
                bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
            alt_prefix(bytes)
        }
    })
}

/// Text pasted into the terminal (bracketed when the program asked).
pub fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    // Line breaks are sent as CR, like a typed Enter.
    let body = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        let clean = body.replace("\x1b[201~", "");
        format!("\x1b[200~{clean}\x1b[201~").into_bytes()
    } else {
        body.into_bytes()
    }
}

/// A `path:line[:column]` reference found in a line of output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// Char columns `[start, end)` in the line.
    pub start: usize,
    pub end: usize,
    pub path: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

/// File references in `text`: `src/a.php:12`, `/abs/b.php:3:7`, PHP's
/// `in /path/x.php on line 9` and `/path/x.php(42)` (stack traces).
pub fn find_links(text: &str) -> Vec<Link> {
    let chars: Vec<char> = text.chars().collect();
    let mut links = Vec::new();
    let is_path_char = |c: char| {
        c.is_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | '~' | '\\' | '@' | '+')
    };
    let mut i = 0;
    while i < chars.len() {
        if !is_path_char(chars[i]) || (i > 0 && is_path_char(chars[i - 1])) {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = i;
        while end < chars.len() && is_path_char(chars[end]) {
            end += 1;
        }
        let token: String = chars[start..end].iter().collect();
        let token = token.trim_end_matches('.').to_string();
        let token_end = start + token.chars().count();
        let looks_like_file = token.contains('.')
            && token.rsplit('.').next().is_some_and(|ext| {
                (1..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric())
            })
            && (token.contains('/') || token.ends_with(".php"));
        if !looks_like_file {
            i = end.max(i + 1);
            continue;
        }
        let number = |from: usize| -> Option<(usize, usize)> {
            let mut to = from;
            while to < chars.len() && chars[to].is_ascii_digit() {
                to += 1;
            }
            (to > from).then(|| {
                (
                    chars[from..to]
                        .iter()
                        .collect::<String>()
                        .parse()
                        .unwrap_or(0),
                    to,
                )
            })
        };
        let mut link = Link {
            start,
            end: token_end,
            path: token.clone(),
            line: None,
            column: None,
        };
        let rest: String = chars[token_end..].iter().take(12).collect();
        if chars.get(token_end) == Some(&':') {
            if let Some((line, after)) = number(token_end + 1) {
                link.line = Some(line);
                link.end = after;
                if chars.get(after) == Some(&':') {
                    if let Some((column, after)) = number(after + 1) {
                        link.column = Some(column);
                        link.end = after;
                    }
                }
            }
        } else if chars.get(token_end) == Some(&'(') {
            if let Some((line, after)) = number(token_end + 1) {
                if chars.get(after) == Some(&')') {
                    link.line = Some(line);
                    link.end = after + 1;
                }
            }
        } else if rest.starts_with(" on line ") {
            if let Some((line, after)) = number(token_end + 9) {
                link.line = Some(line);
                link.end = after;
            }
        }
        i = link.end.max(end);
        links.push(link);
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_like_xterm() {
        let none = Mods::default();
        let ctrl = Mods { ctrl: true, ..none };
        let alt = Mods { alt: true, ..none };
        assert_eq!(encode_key(Key::Up, none, false).unwrap(), b"\x1b[A");
        assert_eq!(encode_key(Key::Up, none, true).unwrap(), b"\x1bOA");
        assert_eq!(encode_key(Key::Right, ctrl, false).unwrap(), b"\x1b[1;5C");
        assert_eq!(encode_key(Key::Char('c'), ctrl, false).unwrap(), vec![3]);
        assert_eq!(encode_key(Key::Char('b'), alt, false).unwrap(), b"\x1bb");
        assert_eq!(encode_key(Key::Delete, none, false).unwrap(), b"\x1b[3~");
        assert_eq!(encode_key(Key::F(5), none, false).unwrap(), b"\x1b[15~");
        assert_eq!(encode_key(Key::Backspace, none, false).unwrap(), vec![0x7f]);
        assert_eq!(encode_paste("a\nb", true), b"\x1b[200~a\rb\x1b[201~");
    }

    #[test]
    fn links_in_output() {
        let links = find_links("PHP Fatal error: Boom in /app/src/A.php on line 12");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].path, "/app/src/A.php");
        assert_eq!(links[0].line, Some(12));
        let links = find_links("#0 /app/src/B.php(42): run() see tests/CTest.php:7:3.");
        assert_eq!(links[0].path, "/app/src/B.php");
        assert_eq!(links[0].line, Some(42));
        assert_eq!(links[1].path, "tests/CTest.php");
        assert_eq!((links[1].line, links[1].column), (Some(7), Some(3)));
        assert!(find_links("version 8.3 and 1.2.3").is_empty());
    }

    #[test]
    fn a_real_shell_on_a_pty() {
        let dir = std::env::temp_dir();
        let command = Command {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                "printf 'hi \\033[1;32mthere\\033[0m\\n'; stty size".into(),
            ],
            cwd: dir,
            env: vec![("TERM".into(), "xterm-256color".into())],
        };
        let mut pty = Pty::spawn(&command, 33, 7).expect("pty");
        let mut term = Terminal::new(33, 7);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let (bytes, exited) = pty.read_timeout(std::time::Duration::from_millis(50));
            term.feed(&bytes);
            if exited || std::time::Instant::now() > deadline {
                break;
            }
        }
        let text = term.screen_text();
        assert!(text.contains("hi there"), "{text:?}");
        assert!(text.contains("7 33"), "the program sees the size: {text:?}");
        assert_eq!(term.screen()[0].cells[3].style.fg, Color::Indexed(2));
    }
}
