//! Per-line text layout with a cache.
//!
//! Each visible line becomes its own galley: syntax spans, semantic tokens
//! and inlay hints are sections of one layout job; tabs are expanded to
//! spaces. A column map converts document columns to galley chars when
//! tabs or hints make them differ. Galleys are cached by content, so
//! scrolling and typing only lay out lines that changed.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use eframe::egui::{self, text::LayoutJob, Galley, TextFormat};
use phpide_php::SyntaxRole;

use crate::style::EditorStyle;
use crate::syntax::Span;
use crate::textutil::char_cells;

#[derive(Clone)]
pub struct LineLayout {
    pub galley: Arc<Galley>,
    /// Galley char index of every document column (`len + 1` entries), or
    /// `None` when they are equal.
    columns: Option<Arc<[u32]>>,
    pub chars: usize,
}

impl LineLayout {
    pub fn galley_index(&self, column: usize) -> usize {
        match &self.columns {
            Some(map) => map[column.min(map.len() - 1)] as usize,
            None => column.min(self.chars),
        }
    }

    /// Document column for a galley char index (hint chars snap to the
    /// column they precede).
    pub fn column_of(&self, index: usize) -> usize {
        match &self.columns {
            Some(map) => match map.binary_search(&(index as u32)) {
                Ok(column) => column,
                Err(next) => next.min(self.chars),
            },
            None => index.min(self.chars),
        }
    }

    /// X offset of a document column from the start of the line.
    pub fn x_of(&self, column: usize) -> f32 {
        let index = self.galley_index(column);
        self.galley
            .pos_from_cursor(egui::text::CCursor::new(index))
            .min
            .x
    }

    /// Document column nearest to an x offset.
    pub fn column_at(&self, x: f32) -> usize {
        let cursor = self
            .galley
            .cursor_from_pos(egui::vec2(x, self.galley.size().y * 0.5));
        self.column_of(cursor.index.0)
    }

    pub fn width(&self) -> f32 {
        self.galley.size().x
    }
}

/// What a line needs to be laid out.
pub struct LineInput<'a> {
    pub text: &'a str,
    pub spans: &'a [Span],
    /// `(start, end, role)` in line columns.
    pub semantic: &'a [(usize, usize, SyntaxRole)],
    /// `(column, label)`.
    pub hints: &'a [(usize, &'a str)],
    pub tab_size: usize,
}

#[derive(Default)]
pub struct LayoutCache {
    lines: HashMap<u64, (LineLayout, u64)>,
    frame: u64,
}

impl LayoutCache {
    pub fn begin_frame(&mut self) {
        self.frame += 1;
        if self.lines.len() > 4000 {
            let keep_after = self.frame.saturating_sub(2);
            self.lines.retain(|_, (_, used)| *used >= keep_after);
        }
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn layout(
        &mut self,
        ctx: &egui::Context,
        style: &EditorStyle,
        style_key: u64,
        input: &LineInput,
    ) -> LineLayout {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        style_key.hash(&mut hasher);
        input.text.hash(&mut hasher);
        input.tab_size.hash(&mut hasher);
        for span in input.spans {
            (span.start, span.end, span.role as u8).hash(&mut hasher);
        }
        for (s, e, role) in input.semantic {
            (s, e, *role as u8).hash(&mut hasher);
        }
        input.hints.hash(&mut hasher);
        let key = hasher.finish();
        let frame = self.frame;
        if let Some((layout, used)) = self.lines.get_mut(&key) {
            *used = frame;
            return layout.clone();
        }
        let layout = build(ctx, style, input);
        self.lines.insert(key, (layout.clone(), frame));
        layout
    }
}

/// Role of each column, walking the sorted spans once.
struct Roles<'a> {
    input: &'a LineInput<'a>,
    span: usize,
    semantic: usize,
}

impl Roles<'_> {
    fn at(&mut self, column: usize) -> SyntaxRole {
        let semantic = self.input.semantic;
        while self.semantic < semantic.len() && semantic[self.semantic].1 <= column {
            self.semantic += 1;
        }
        if let Some((s, e, role)) = semantic.get(self.semantic) {
            if *s <= column && column < *e {
                return *role;
            }
        }
        let spans = self.input.spans;
        while self.span < spans.len() && spans[self.span].end <= column {
            self.span += 1;
        }
        match spans.get(self.span) {
            Some(span) if span.start <= column => span.role,
            _ => SyntaxRole::Plain,
        }
    }
}

fn build(ctx: &egui::Context, style: &EditorStyle, input: &LineInput) -> LineLayout {
    let font = style.font().clone();
    let hint_font = egui::FontId::new(font.size * 0.86, egui::FontFamily::Proportional);
    let mut job = LayoutJob::default();
    let chars: Vec<char> = input.text.chars().collect();
    let needs_map = !input.hints.is_empty() || chars.contains(&'\t');
    let mut map: Vec<u32> = if needs_map {
        Vec::with_capacity(chars.len() + 1)
    } else {
        Vec::new()
    };
    let mut galley_chars: u32 = 0;
    let mut cell = 0;
    let mut hints = input.hints.iter().peekable();
    let mut run = String::new();
    let mut run_role: Option<SyntaxRole> = None;
    let mut roles = Roles {
        input,
        span: 0,
        semantic: 0,
    };
    let flush = |job: &mut LayoutJob, run: &mut String, role: Option<SyntaxRole>| {
        if run.is_empty() {
            return;
        }
        let role = role.unwrap_or(SyntaxRole::Plain);
        let color = style.syntax.color(role);
        let format = TextFormat {
            font_id: font.clone(),
            color,
            italics: matches!(role, SyntaxRole::Comment | SyntaxRole::Documentation),
            strikethrough: if role == SyntaxRole::Deprecated {
                egui::Stroke::new(1.0, color)
            } else {
                egui::Stroke::NONE
            },
            ..Default::default()
        };
        job.append(&std::mem::take(run), 0.0, format);
    };
    for column in 0..=chars.len() {
        while let Some((at, label)) = hints.peek() {
            if *at != column {
                break;
            }
            flush(&mut job, &mut run, run_role);
            let text = format!("{label}\u{2009}");
            galley_chars += text.chars().count() as u32;
            job.append(
                &text,
                0.0,
                TextFormat {
                    font_id: hint_font.clone(),
                    color: style.colors.inlay_text,
                    background: style.colors.inlay_background,
                    valign: egui::Align::Center,
                    ..Default::default()
                },
            );
            hints.next();
        }
        if needs_map {
            map.push(galley_chars);
        }
        let Some(&c) = chars.get(column) else {
            break;
        };
        let role = roles.at(column);
        if run_role != Some(role) {
            flush(&mut job, &mut run, run_role);
            run_role = Some(role);
        }
        let width = char_cells(c, cell, input.tab_size);
        cell += width;
        if c == '\t' {
            run.extend(std::iter::repeat_n(' ', width));
            galley_chars += width as u32;
        } else {
            run.push(c);
            galley_chars += 1;
        }
    }
    flush(&mut job, &mut run, run_role);
    if job.sections.is_empty() {
        job.append(
            "",
            0.0,
            TextFormat::simple(font.clone(), style.syntax.color(SyntaxRole::Plain)),
        );
    }
    job.wrap.max_width = f32::INFINITY;
    let galley = ctx.fonts_mut(|fonts| fonts.layout_job(job));
    LineLayout {
        galley,
        columns: needs_map.then(|| Arc::from(map.into_boxed_slice())),
        chars: chars.len(),
    }
}
