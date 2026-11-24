//! An xterm-compatible terminal emulator: bytes in, a grid of styled cells
//! out.
//!
//! It covers what shells and common tools use: UTF-8, C0 controls, CSI
//! cursor movement and editing, SGR (16, 256 and true colours), scroll
//! regions, the alternate screen, the usual private modes, OSC titles and
//! the replies programs ask for (cursor position, device attributes).
//! Unknown sequences are parsed and ignored, never printed.

use std::collections::VecDeque;

/// Lines kept above the screen.
pub const SCROLLBACK: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Color {
    #[default]
    Default,
    /// One of the 256 xterm colours (0–15 are the theme's ANSI colours).
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub hidden: bool,
    pub strike: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub style: Style,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            ch: ' ',
            style: Style::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Row {
    pub cells: Vec<Cell>,
    /// The line continues on the next row (soft wrap).
    pub wrapped: bool,
}

impl Row {
    fn blank(cols: usize, style: Style) -> Row {
        Row {
            cells: vec![
                Cell {
                    ch: ' ',
                    style: Style {
                        bg: style.bg,
                        ..Style::default()
                    },
                };
                cols
            ],
            wrapped: false,
        }
    }

    pub fn text(&self) -> String {
        let text: String = self.cells.iter().map(|c| c.ch).collect();
        text.trim_end().to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Cursor {
    row: usize,
    col: usize,
    style: Style,
    /// Wrap before the next character (the cursor sits past the last column).
    pending_wrap: bool,
}

#[derive(Debug, Clone)]
struct Screen {
    rows: Vec<Row>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    Ground,
    Escape,
    /// `ESC (` and friends: one more byte selects a charset (ignored).
    Charset,
    Csi,
    Osc,
    /// Inside a DCS / SOS / PM / APC string: ignored until ST.
    Ignore,
}

/// Modes programs switch with `CSI ? n h/l`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Modes {
    pub cursor_visible: bool,
    pub app_cursor: bool,
    pub bracketed_paste: bool,
    pub autowrap: bool,
    pub origin: bool,
    pub insert: bool,
    /// Mouse reporting requested (1000/1002/1003 with 1006).
    pub mouse: bool,
}

impl Default for Modes {
    fn default() -> Self {
        Modes {
            cursor_visible: true,
            app_cursor: false,
            bracketed_paste: false,
            autowrap: true,
            origin: false,
            insert: false,
            mouse: false,
        }
    }
}

pub struct Terminal {
    cols: usize,
    rows: usize,
    screen: Screen,
    /// The normal screen while the alternate one is shown.
    saved_screen: Option<Screen>,
    pub scrollback: VecDeque<Row>,
    cursor: Cursor,
    saved_cursor: Cursor,
    /// Scroll region, inclusive rows.
    top: usize,
    bottom: usize,
    tabs: Vec<bool>,
    pub modes: Modes,
    pub title: String,
    state: State,
    params: Vec<u16>,
    /// Sub-parameters separated by `:` count as parameters here.
    param_started: bool,
    private: Option<u8>,
    intermediate: Option<u8>,
    osc: Vec<u8>,
    utf8: Vec<u8>,
    last_char: char,
    /// Bytes to send back to the program (reports).
    replies: Vec<u8>,
    /// Increases on every change (renderers can cache by it).
    pub revision: u64,
    /// The bell rang since the last [`Terminal::take_bell`].
    bell: bool,
}

impl Terminal {
    pub fn new(cols: usize, rows: usize) -> Terminal {
        let cols = cols.max(2);
        let rows = rows.max(1);
        Terminal {
            cols,
            rows,
            screen: Screen {
                rows: (0..rows)
                    .map(|_| Row::blank(cols, Style::default()))
                    .collect(),
            },
            saved_screen: None,
            scrollback: VecDeque::new(),
            cursor: Cursor::default(),
            saved_cursor: Cursor::default(),
            top: 0,
            bottom: rows - 1,
            tabs: (0..cols).map(|c| c % 8 == 0).collect(),
            modes: Modes::default(),
            title: String::new(),
            state: State::Ground,
            params: Vec::new(),
            param_started: false,
            private: None,
            intermediate: None,
            osc: Vec::new(),
            utf8: Vec::new(),
            last_char: ' ',
            replies: Vec::new(),
            revision: 0,
            bell: false,
        }
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Visible rows, top to bottom.
    pub fn screen(&self) -> &[Row] {
        &self.screen.rows
    }

    /// `(row, col)` of the cursor on the screen.
    pub fn cursor(&self) -> (usize, usize) {
        (self.cursor.row, self.cursor.col.min(self.cols - 1))
    }

    pub fn alternate_screen(&self) -> bool {
        self.saved_screen.is_some()
    }

    /// Replies the program asked for; write them to the pty.
    pub fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.replies)
    }

    pub fn take_bell(&mut self) -> bool {
        std::mem::take(&mut self.bell)
    }

    /// The screen as text, one line per row (tests, copy).
    pub fn screen_text(&self) -> String {
        self.screen
            .rows
            .iter()
            .map(Row::text)
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end()
            .to_string()
    }

    /// Row `index` counting from the oldest scrollback line.
    pub fn line(&self, index: usize) -> Option<&Row> {
        if index < self.scrollback.len() {
            self.scrollback.get(index)
        } else {
            self.screen.rows.get(index - self.scrollback.len())
        }
    }

    pub fn total_lines(&self) -> usize {
        self.scrollback.len() + self.rows
    }

    pub fn clear_scrollback(&mut self) {
        self.scrollback.clear();
        self.revision += 1;
    }

    /// Changes the size, keeping the bottom of the screen in view.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(2);
        let rows = rows.max(1);
        if cols == self.cols && rows == self.rows {
            return;
        }
        for screen in std::iter::once(&mut self.screen).chain(self.saved_screen.as_mut()) {
            for row in &mut screen.rows {
                row.cells.resize(cols, Cell::default());
            }
        }
        for row in &mut self.scrollback {
            row.cells.resize(cols, Cell::default());
        }
        // Fewer rows: the top rows go to the scrollback (only those above
        // the cursor, so the prompt stays).
        while self.screen.rows.len() > rows {
            if self.cursor.row > 0 && self.saved_screen.is_none() {
                let row = self.screen.rows.remove(0);
                self.push_scrollback(row);
                self.cursor.row -= 1;
            } else {
                self.screen.rows.pop();
            }
        }
        while self.screen.rows.len() < rows {
            // More rows: bring lines back from the scrollback.
            match (self.saved_screen.is_none(), self.scrollback.pop_back()) {
                (true, Some(row)) => {
                    self.screen.rows.insert(0, row);
                    self.cursor.row += 1;
                }
                _ => self.screen.rows.push(Row::blank(cols, Style::default())),
            }
        }
        if let Some(saved) = self.saved_screen.as_mut() {
            saved.rows.resize(rows, Row::blank(cols, Style::default()));
        }
        self.cols = cols;
        self.rows = rows;
        self.top = 0;
        self.bottom = rows - 1;
        self.tabs = (0..cols).map(|c| c % 8 == 0).collect();
        self.cursor.row = self.cursor.row.min(rows - 1);
        self.cursor.col = self.cursor.col.min(cols - 1);
        self.cursor.pending_wrap = false;
        self.revision += 1;
    }

    /// Feeds output of the program.
    pub fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.byte(byte);
        }
        self.revision += 1;
    }

    fn byte(&mut self, byte: u8) {
        match self.state {
            State::Ground => self.ground(byte),
            State::Escape => self.escape(byte),
            State::Charset => self.state = State::Ground,
            State::Csi => self.csi_byte(byte),
            State::Osc => match byte {
                0x07 => self.end_osc(),
                0x1b => {
                    // ESC \ ends it; take the ESC as the terminator.
                    self.end_osc();
                    self.state = State::Escape;
                }
                _ => {
                    if self.osc.len() < 4096 {
                        self.osc.push(byte);
                    }
                }
            },
            State::Ignore => {
                if byte == 0x1b || byte == 0x07 {
                    self.state = if byte == 0x1b {
                        State::Escape
                    } else {
                        State::Ground
                    };
                }
            }
        }
    }

    fn ground(&mut self, byte: u8) {
        if !self.utf8.is_empty() || byte >= 0x80 {
            self.utf8_byte(byte);
            return;
        }
        match byte {
            0x1b => {
                self.state = State::Escape;
            }
            0x07 => self.bell = true,
            0x08 => {
                self.cursor.pending_wrap = false;
                self.cursor.col = self.cursor.col.saturating_sub(1);
            }
            0x09 => self.tab(),
            0x0a..=0x0c => self.linefeed(),
            0x0d => {
                self.cursor.col = 0;
                self.cursor.pending_wrap = false;
            }
            0x00..=0x1f | 0x7f => {}
            _ => self.print(byte as char),
        }
    }

    fn utf8_byte(&mut self, byte: u8) {
        if self.utf8.is_empty() {
            let len = match byte {
                0xc0..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf7 => 4,
                _ => {
                    self.print('\u{fffd}');
                    return;
                }
            };
            self.utf8.reserve(len);
            self.utf8.push(byte);
            return;
        }
        if byte & 0xc0 != 0x80 {
            // Broken sequence: replace it and handle this byte afresh.
            self.utf8.clear();
            self.print('\u{fffd}');
            self.ground(byte);
            return;
        }
        self.utf8.push(byte);
        let need = match self.utf8[0] {
            0xc0..=0xdf => 2,
            0xe0..=0xef => 3,
            _ => 4,
        };
        if self.utf8.len() == need {
            let c = std::str::from_utf8(&self.utf8)
                .ok()
                .and_then(|s| s.chars().next())
                .unwrap_or('\u{fffd}');
            self.utf8.clear();
            self.print(c);
        }
    }

    fn escape(&mut self, byte: u8) {
        self.state = State::Ground;
        match byte {
            b'[' => {
                self.state = State::Csi;
                self.params.clear();
                self.param_started = false;
                self.private = None;
                self.intermediate = None;
            }
            b']' => {
                self.state = State::Osc;
                self.osc.clear();
            }
            b'P' | b'X' | b'^' | b'_' => self.state = State::Ignore,
            b'(' | b')' | b'*' | b'+' | b'#' | b'%' => self.state = State::Charset,
            b'7' => self.saved_cursor = self.cursor,
            b'8' => {
                self.cursor = self.saved_cursor;
                self.clamp_cursor();
            }
            b'D' => self.linefeed(),
            b'E' => {
                self.linefeed();
                self.cursor.col = 0;
            }
            b'M' => self.reverse_index(),
            b'H' => {
                if self.cursor.col < self.cols {
                    self.tabs[self.cursor.col] = true;
                }
            }
            b'c' => {
                let (cols, rows) = (self.cols, self.rows);
                let scrollback = std::mem::take(&mut self.scrollback);
                *self = Terminal::new(cols, rows);
                self.scrollback = scrollback;
            }
            b'\\' => {}
            _ => {}
        }
    }

    fn csi_byte(&mut self, byte: u8) {
        match byte {
            b'0'..=b'9' => {
                if !self.param_started {
                    self.params.push(0);
                    self.param_started = true;
                }
                let last = self.params.last_mut().unwrap();
                *last = last.saturating_mul(10).saturating_add((byte - b'0') as u16);
            }
            b';' | b':' => {
                if !self.param_started {
                    self.params.push(0);
                }
                self.param_started = false;
            }
            b'?' | b'>' | b'<' | b'=' => self.private = Some(byte),
            b' '..=b'/' => self.intermediate = Some(byte),
            0x40..=0x7e => {
                self.state = State::Ground;
                self.csi_dispatch(byte);
            }
            0x1b => self.state = State::Escape,
            // Controls inside a CSI still act.
            0x08 | 0x0d | 0x0a => self.ground(byte),
            _ => {}
        }
    }

    fn param(&self, index: usize, default: u16) -> usize {
        match self.params.get(index) {
            Some(0) | None => default as usize,
            Some(&value) => value as usize,
        }
    }

    fn csi_dispatch(&mut self, final_byte: u8) {
        if self.private == Some(b'?') {
            match final_byte {
                b'h' => self.private_modes(true),
                b'l' => self.private_modes(false),
                _ => {}
            }
            return;
        }
        if self.private.is_some() {
            // `CSI > c` (secondary DA) and the like.
            if final_byte == b'c' && self.private == Some(b'>') {
                self.replies.extend_from_slice(b"\x1b[>0;0;0c");
            }
            return;
        }
        if self.intermediate.is_some() {
            // `CSI Ps SP q` (cursor shape) and friends: ignored.
            return;
        }
        let n = self.param(0, 1);
        match final_byte {
            b'A' => self.move_cursor_rows(-(n as isize)),
            b'B' | b'e' => self.move_cursor_rows(n as isize),
            b'C' | b'a' => self.set_col(self.cursor.col + n),
            b'D' => self.set_col(self.cursor.col.saturating_sub(n)),
            b'E' => {
                self.move_cursor_rows(n as isize);
                self.cursor.col = 0;
            }
            b'F' => {
                self.move_cursor_rows(-(n as isize));
                self.cursor.col = 0;
            }
            b'G' | b'`' => self.set_col(n - 1),
            b'H' | b'f' => {
                let row = self.param(0, 1) - 1;
                let col = self.param(1, 1) - 1;
                let base = if self.modes.origin { self.top } else { 0 };
                self.cursor.row = (base + row).min(self.rows - 1);
                self.set_col(col);
            }
            b'd' => {
                self.cursor.row = (n - 1).min(self.rows - 1);
                self.cursor.pending_wrap = false;
            }
            b'J' => self.erase_display(self.param(0, 0)),
            b'K' => self.erase_line(self.param(0, 0)),
            b'L' => self.insert_lines(n),
            b'M' => self.delete_lines(n),
            b'@' => self.insert_chars(n),
            b'P' => self.delete_chars(n),
            b'X' => {
                let style = self.cursor.style;
                let row = self.cursor.row;
                let from = self.cursor.col.min(self.cols);
                let to = (from + n).min(self.cols);
                for cell in &mut self.screen.rows[row].cells[from..to] {
                    *cell = blank_cell(style);
                }
            }
            b'S' => self.scroll_up(n),
            b'T' => self.scroll_down(n),
            b'b' => {
                for _ in 0..n.min(4096) {
                    self.print(self.last_char);
                }
            }
            b'r' => {
                let top = self.param(0, 1) - 1;
                let bottom = self.param(1, self.rows as u16).min(self.rows) - 1;
                if top < bottom {
                    self.top = top;
                    self.bottom = bottom;
                    self.cursor.row = if self.modes.origin { top } else { 0 };
                    self.cursor.col = 0;
                    self.cursor.pending_wrap = false;
                }
            }
            b's' => self.saved_cursor = self.cursor,
            b'u' => {
                self.cursor = self.saved_cursor;
                self.clamp_cursor();
            }
            b'm' => self.sgr(),
            b'h' | b'l' => {
                if self.params.first() == Some(&4) {
                    self.modes.insert = final_byte == b'h';
                }
            }
            b'n' => match self.param(0, 0) {
                5 => self.replies.extend_from_slice(b"\x1b[0n"),
                6 => {
                    let report = format!(
                        "\x1b[{};{}R",
                        self.cursor.row + 1,
                        self.cursor.col.min(self.cols - 1) + 1
                    );
                    self.replies.extend_from_slice(report.as_bytes());
                }
                _ => {}
            },
            b'c' => self.replies.extend_from_slice(b"\x1b[?62;22c"),
            b'g' => match self.param(0, 0) {
                0 => {
                    if self.cursor.col < self.cols {
                        self.tabs[self.cursor.col] = false;
                    }
                }
                3 => self.tabs.iter_mut().for_each(|t| *t = false),
                _ => {}
            },
            _ => {}
        }
    }

    fn private_modes(&mut self, on: bool) {
        let params = self.params.clone();
        for mode in params {
            match mode {
                1 => self.modes.app_cursor = on,
                6 => {
                    self.modes.origin = on;
                    self.cursor.row = if on { self.top } else { 0 };
                    self.cursor.col = 0;
                }
                7 => self.modes.autowrap = on,
                25 => self.modes.cursor_visible = on,
                1000 | 1002 | 1003 => self.modes.mouse = on,
                47 | 1047 | 1049 => {
                    if mode == 1049 && on {
                        self.saved_cursor = self.cursor;
                    }
                    self.alternate(on);
                    if mode == 1049 && !on {
                        self.cursor = self.saved_cursor;
                        self.clamp_cursor();
                    }
                }
                2004 => self.modes.bracketed_paste = on,
                _ => {}
            }
        }
    }

    fn alternate(&mut self, on: bool) {
        match (on, self.saved_screen.is_some()) {
            (true, false) => {
                let blank = Screen {
                    rows: (0..self.rows)
                        .map(|_| Row::blank(self.cols, Style::default()))
                        .collect(),
                };
                self.saved_screen = Some(std::mem::replace(&mut self.screen, blank));
            }
            (false, true) => {
                self.screen = self.saved_screen.take().unwrap();
            }
            _ => {}
        }
        self.top = 0;
        self.bottom = self.rows - 1;
    }

    fn sgr(&mut self) {
        if self.params.is_empty() {
            self.cursor.style = Style::default();
            return;
        }
        let params = self.params.clone();
        let mut i = 0;
        while i < params.len() {
            let style = &mut self.cursor.style;
            match params[i] {
                0 => *style = Style::default(),
                1 => style.bold = true,
                2 => style.dim = true,
                3 => style.italic = true,
                4 => style.underline = true,
                7 => style.inverse = true,
                8 => style.hidden = true,
                9 => style.strike = true,
                21 | 22 => {
                    style.bold = false;
                    style.dim = false;
                }
                23 => style.italic = false,
                24 => style.underline = false,
                27 => style.inverse = false,
                28 => style.hidden = false,
                29 => style.strike = false,
                n @ 30..=37 => style.fg = Color::Indexed((n - 30) as u8),
                n @ 40..=47 => style.bg = Color::Indexed((n - 40) as u8),
                n @ 90..=97 => style.fg = Color::Indexed((n - 90 + 8) as u8),
                n @ 100..=107 => style.bg = Color::Indexed((n - 100 + 8) as u8),
                39 => style.fg = Color::Default,
                49 => style.bg = Color::Default,
                which @ (38 | 48) => {
                    let color = match params.get(i + 1) {
                        Some(5) => {
                            let c = params.get(i + 2).copied().unwrap_or(0).min(255) as u8;
                            i += 2;
                            Some(Color::Indexed(c))
                        }
                        Some(2) => {
                            let at =
                                |k: usize| params.get(i + k).copied().unwrap_or(0).min(255) as u8;
                            let c = Color::Rgb(at(2), at(3), at(4));
                            i += 4;
                            Some(c)
                        }
                        _ => None,
                    };
                    if let Some(color) = color {
                        if which == 38 {
                            style.fg = color;
                        } else {
                            style.bg = color;
                        }
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }

    fn end_osc(&mut self) {
        self.state = State::Ground;
        let text = String::from_utf8_lossy(&self.osc).into_owned();
        if let Some((code, value)) = text.split_once(';') {
            if code == "0" || code == "2" {
                self.title = value.chars().take(200).collect();
            }
        }
        self.osc.clear();
    }

    fn print(&mut self, c: char) {
        // Combining marks and zero-width joiners join the previous cell.
        if is_zero_width(c) {
            return;
        }
        if self.cursor.pending_wrap && self.modes.autowrap {
            self.screen.rows[self.cursor.row].wrapped = true;
            self.cursor.col = 0;
            self.linefeed();
        }
        let wide = is_wide(c);
        if wide && self.cursor.col + 1 >= self.cols && self.modes.autowrap {
            self.screen.rows[self.cursor.row].wrapped = true;
            self.cursor.col = 0;
            self.linefeed();
        }
        let row = self.cursor.row;
        let col = self.cursor.col.min(self.cols - 1);
        if self.modes.insert {
            let cells = &mut self.screen.rows[row].cells;
            cells.insert(col, Cell::default());
            cells.truncate(self.cols);
        }
        self.screen.rows[row].cells[col] = Cell {
            ch: c,
            style: self.cursor.style,
        };
        self.last_char = c;
        let mut next = col + 1;
        if wide && next < self.cols {
            // The second half of a wide character.
            self.screen.rows[row].cells[next] = Cell {
                ch: '\u{0}',
                style: self.cursor.style,
            };
            next += 1;
        }
        if next >= self.cols {
            self.cursor.col = self.cols - 1;
            self.cursor.pending_wrap = true;
        } else {
            self.cursor.col = next;
            self.cursor.pending_wrap = false;
        }
    }

    fn tab(&mut self) {
        let mut col = self.cursor.col + 1;
        while col < self.cols - 1 && !self.tabs[col] {
            col += 1;
        }
        self.cursor.col = col.min(self.cols - 1);
        self.cursor.pending_wrap = false;
    }

    fn linefeed(&mut self) {
        self.cursor.pending_wrap = false;
        if self.cursor.row == self.bottom {
            self.scroll_up(1);
        } else if self.cursor.row < self.rows - 1 {
            self.cursor.row += 1;
        }
    }

    fn reverse_index(&mut self) {
        if self.cursor.row == self.top {
            self.scroll_down(1);
        } else {
            self.cursor.row = self.cursor.row.saturating_sub(1);
        }
    }

    fn push_scrollback(&mut self, row: Row) {
        self.scrollback.push_back(row);
        if self.scrollback.len() > SCROLLBACK {
            self.scrollback.pop_front();
        }
    }

    fn scroll_up(&mut self, n: usize) {
        let style = self.cursor.style;
        for _ in 0..n.min(self.bottom - self.top + 1) {
            let row = self.screen.rows.remove(self.top);
            // Only the full normal screen feeds the scrollback.
            if self.top == 0 && self.saved_screen.is_none() {
                self.push_scrollback(row);
            }
            self.screen
                .rows
                .insert(self.bottom, Row::blank(self.cols, style));
        }
    }

    fn scroll_down(&mut self, n: usize) {
        let style = self.cursor.style;
        for _ in 0..n.min(self.bottom - self.top + 1) {
            self.screen.rows.remove(self.bottom);
            self.screen
                .rows
                .insert(self.top, Row::blank(self.cols, style));
        }
    }

    fn insert_lines(&mut self, n: usize) {
        if self.cursor.row < self.top || self.cursor.row > self.bottom {
            return;
        }
        let style = self.cursor.style;
        for _ in 0..n.min(self.bottom - self.cursor.row + 1) {
            self.screen.rows.remove(self.bottom);
            self.screen
                .rows
                .insert(self.cursor.row, Row::blank(self.cols, style));
        }
        self.cursor.col = 0;
    }

    fn delete_lines(&mut self, n: usize) {
        if self.cursor.row < self.top || self.cursor.row > self.bottom {
            return;
        }
        let style = self.cursor.style;
        for _ in 0..n.min(self.bottom - self.cursor.row + 1) {
            self.screen.rows.remove(self.cursor.row);
            self.screen
                .rows
                .insert(self.bottom, Row::blank(self.cols, style));
        }
        self.cursor.col = 0;
    }

    fn insert_chars(&mut self, n: usize) {
        let style = self.cursor.style;
        let col = self.cursor.col.min(self.cols - 1);
        let cells = &mut self.screen.rows[self.cursor.row].cells;
        for _ in 0..n.min(self.cols - col) {
            cells.insert(col, blank_cell(style));
        }
        cells.truncate(self.cols);
    }

    fn delete_chars(&mut self, n: usize) {
        let style = self.cursor.style;
        let col = self.cursor.col.min(self.cols - 1);
        let cells = &mut self.screen.rows[self.cursor.row].cells;
        for _ in 0..n.min(self.cols - col) {
            cells.remove(col);
            cells.push(blank_cell(style));
        }
    }

    fn erase_line(&mut self, mode: usize) {
        let style = self.cursor.style;
        let col = self.cursor.col.min(self.cols - 1);
        let row = &mut self.screen.rows[self.cursor.row];
        let range = match mode {
            0 => col..self.cols,
            1 => 0..col + 1,
            _ => 0..self.cols,
        };
        for cell in &mut row.cells[range] {
            *cell = blank_cell(style);
        }
        if mode != 1 {
            row.wrapped = false;
        }
    }

    fn erase_display(&mut self, mode: usize) {
        let style = self.cursor.style;
        match mode {
            0 => {
                self.erase_line(0);
                for row in self.cursor.row + 1..self.rows {
                    self.screen.rows[row] = Row::blank(self.cols, style);
                }
            }
            1 => {
                self.erase_line(1);
                for row in 0..self.cursor.row {
                    self.screen.rows[row] = Row::blank(self.cols, style);
                }
            }
            2 => {
                for row in 0..self.rows {
                    self.screen.rows[row] = Row::blank(self.cols, style);
                }
            }
            3 => self.scrollback.clear(),
            _ => {}
        }
    }

    fn move_cursor_rows(&mut self, delta: isize) {
        let (min, max) = if self.cursor.row >= self.top && self.cursor.row <= self.bottom {
            (self.top, self.bottom)
        } else {
            (0, self.rows - 1)
        };
        let row = (self.cursor.row as isize + delta).clamp(min as isize, max as isize);
        self.cursor.row = row as usize;
        self.cursor.pending_wrap = false;
    }

    fn set_col(&mut self, col: usize) {
        self.cursor.col = col.min(self.cols - 1);
        self.cursor.pending_wrap = false;
    }

    fn clamp_cursor(&mut self) {
        self.cursor.row = self.cursor.row.min(self.rows - 1);
        self.cursor.col = self.cursor.col.min(self.cols - 1);
    }
}

fn blank_cell(style: Style) -> Cell {
    Cell {
        ch: ' ',
        style: Style {
            bg: style.bg,
            ..Style::default()
        },
    }
}

fn is_zero_width(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036f | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0x20d0..=0x20ff
        | 0xfe00..=0xfe0f | 0xfe20..=0xfe2f | 0x200b..=0x200f | 0x2060..=0x2064)
}

/// East Asian wide and emoji characters take two cells.
pub fn is_wide(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x115f | 0x2e80..=0x303e | 0x3041..=0x33ff | 0x3400..=0x4dbf
        | 0x4e00..=0x9fff | 0xa000..=0xa4cf | 0xac00..=0xd7a3 | 0xf900..=0xfaff
        | 0xfe30..=0xfe4f | 0xff00..=0xff60 | 0xffe0..=0xffe6
        | 0x1f300..=0x1f64f | 0x1f900..=0x1f9ff | 0x20000..=0x3fffd)
}

/// RGB of an xterm colour index ≥ 16 (the 6×6×6 cube and the grey ramp).
pub fn xterm_rgb(index: u8) -> (u8, u8, u8) {
    match index {
        16..=231 => {
            let i = index - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            (level(i / 36), level((i / 6) % 6), level(i % 6))
        }
        232..=255 => {
            let g = 8 + (index - 232) * 10;
            (g, g, g)
        }
        _ => (0, 0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(input: &str) -> Terminal {
        let mut t = Terminal::new(20, 5);
        t.feed(input.as_bytes());
        t
    }

    #[test]
    fn prints_wraps_and_scrolls_into_the_scrollback() {
        let t = term("hello\r\nworld olá 🚀!");
        assert_eq!(t.screen_text(), "hello\nworld olá 🚀!");
        let mut t = Terminal::new(5, 2);
        t.feed(b"abcdefgh\r\nij\r\nkl");
        assert_eq!(t.screen_text(), "ij\nkl");
        assert_eq!(t.scrollback.len(), 2);
        assert_eq!(t.scrollback[0].text(), "abcde");
        assert!(t.scrollback[0].wrapped);
    }

    #[test]
    fn cursor_moves_and_erases() {
        let t = term("abcdef\x1b[3D\x1b[K\x1b[2;3Hxy\x1b[1;1H\x1b[2@");
        assert_eq!(t.screen_text(), "  abc\n  xy");
        let t = term("line1\r\nline2\r\nline3\x1b[2;1H\x1b[M");
        assert_eq!(t.screen_text(), "line1\nline3");
        let t = term("abc\x1b[2J\x1b[Hz");
        assert_eq!(t.screen_text(), "z");
    }

    #[test]
    fn sgr_colours_and_attributes() {
        let t = term("\x1b[1;31mR\x1b[38;5;208mO\x1b[48;2;1;2;3mB\x1b[0mn");
        let cells = &t.screen()[0].cells;
        assert_eq!(cells[0].style.fg, Color::Indexed(1));
        assert!(cells[0].style.bold);
        assert_eq!(cells[1].style.fg, Color::Indexed(208));
        assert_eq!(cells[2].style.bg, Color::Rgb(1, 2, 3));
        assert_eq!(cells[2].style.fg, Color::Indexed(208));
        assert_eq!(cells[3].style, Style::default());
    }

    #[test]
    fn alternate_screen_and_scroll_region() {
        let mut t = term("shell$ ");
        t.feed(b"\x1b[?1049h\x1b[Hvim\x1b[?1049l");
        assert_eq!(t.screen_text(), "shell$");
        assert_eq!(t.cursor(), (0, 7));
        let mut t = Terminal::new(10, 4);
        t.feed(b"a\r\nb\r\nc\r\nd\x1b[2;3r\x1b[3;1H\n");
        assert_eq!(t.screen_text(), "a\nc\n\nd");
    }

    #[test]
    fn replies_titles_and_modes() {
        let mut t = term("\x1b[3;4H\x1b[6n\x1b]0;my title\x07\x1b[?2004h\x1b[?25l");
        assert_eq!(t.take_replies(), b"\x1b[3;4R");
        assert_eq!(t.title, "my title");
        assert!(t.modes.bracketed_paste);
        assert!(!t.modes.cursor_visible);
        // Unknown sequences are swallowed.
        let t = term("a\x1bP1$r0m\x1b\\b\x1b[>4;1mc");
        assert_eq!(t.screen_text(), "abc");
    }

    #[test]
    fn resize_keeps_the_prompt_and_the_scrollback() {
        let mut t = Terminal::new(10, 4);
        t.feed(b"1\r\n2\r\n3\r\n4$ ");
        t.resize(12, 2);
        assert_eq!(t.screen_text(), "3\n4$");
        assert_eq!(t.cursor(), (1, 3));
        t.resize(12, 4);
        assert_eq!(t.screen_text(), "1\n2\n3\n4$");
    }

    #[test]
    fn xterm_palette() {
        assert_eq!(xterm_rgb(16), (0, 0, 0));
        assert_eq!(xterm_rgb(231), (255, 255, 255));
        assert_eq!(xterm_rgb(232), (8, 8, 8));
    }
}
