//! Side-by-side diff tabs (Phase 7).
//!
//! A diff tab sits in the editor tab strip after the file tabs. Both sides
//! are read-only and highlighted like the editor; rows are aligned (blank
//! filler on the side that has fewer lines in a change), the changed part of
//! a modified line is marked, unchanged stretches can be collapsed, and each
//! hunk carries its actions: Stage, Unstage or Revert, depending on what is
//! compared. F7 / Shift+F7 walk the changes.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;
use phpide_core::Document;
use phpide_editor::syntax::Span;
use phpide_editor::Syntax;
use phpide_icons::IconId;

use crate::design::components::IconButton;
use crate::services::vcs::{
    DiffData, DiffLineKind, DiffRequest, FileDiff, HunkAction, QuickOp, Side,
};
use crate::PhpIdeApp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowKind {
    Equal,
    /// Both sides have a line, and they differ.
    Changed,
    Added,
    Deleted,
}

/// One aligned row: a line of each side (0-based), or a blank filler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Row {
    pub left: Option<usize>,
    pub right: Option<usize>,
    pub kind: RowKind,
    /// Hunk of the diff this row belongs to.
    pub hunk: Option<usize>,
    /// First row of a run of changed rows.
    pub change_start: bool,
    /// First changed row of its hunk (where the hunk's actions go).
    pub hunk_start: bool,
}

/// Aligns both sides with the diff's hunks.
pub(crate) fn align(old_len: usize, new_len: usize, diff: &FileDiff) -> Vec<Row> {
    let mut rows = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    let equal = |rows: &mut Vec<Row>, i: usize, j: usize| {
        rows.push(Row {
            left: Some(i),
            right: Some(j),
            kind: RowKind::Equal,
            hunk: None,
            change_start: false,
            hunk_start: false,
        })
    };
    for (h, hunk) in diff.hunks.iter().enumerate() {
        let mut first_in_hunk = true;
        let mut k = 0;
        while k < hunk.lines.len() {
            let line = &hunk.lines[k];
            if line.kind == DiffLineKind::Context {
                let (o, n) = (
                    line.old_lineno.unwrap_or(1) as usize - 1,
                    line.new_lineno.unwrap_or(1) as usize - 1,
                );
                while i < o && j < n {
                    equal(&mut rows, i, j);
                    i += 1;
                    j += 1;
                }
                equal(&mut rows, o, n);
                i = o + 1;
                j = n + 1;
                k += 1;
                continue;
            }
            // A run of deleted and added lines.
            let mut dels = Vec::new();
            let mut adds = Vec::new();
            while k < hunk.lines.len() && hunk.lines[k].kind != DiffLineKind::Context {
                let line = &hunk.lines[k];
                match line.kind {
                    DiffLineKind::Del => dels.push(line.old_lineno.unwrap_or(1) as usize - 1),
                    _ => adds.push(line.new_lineno.unwrap_or(1) as usize - 1),
                }
                k += 1;
            }
            // Unchanged lines before the run.
            if let Some(&o) = dels.first() {
                while i < o && j < new_len {
                    equal(&mut rows, i, j);
                    i += 1;
                    j += 1;
                }
            } else if let Some(&n) = adds.first() {
                while j < n && i < old_len {
                    equal(&mut rows, i, j);
                    i += 1;
                    j += 1;
                }
            }
            for r in 0..dels.len().max(adds.len()) {
                let (left, right) = (dels.get(r).copied(), adds.get(r).copied());
                rows.push(Row {
                    left,
                    right,
                    kind: match (left, right) {
                        (Some(_), Some(_)) => RowKind::Changed,
                        (Some(_), None) => RowKind::Deleted,
                        _ => RowKind::Added,
                    },
                    hunk: Some(h),
                    change_start: r == 0,
                    hunk_start: r == 0 && first_in_hunk,
                });
            }
            first_in_hunk = false;
            if let Some(&o) = dels.last() {
                i = o + 1;
            }
            if let Some(&n) = adds.last() {
                j = n + 1;
            }
        }
    }
    while i < old_len && j < new_len {
        equal(&mut rows, i, j);
        i += 1;
        j += 1;
    }
    rows
}

/// What a row of the view shows: a line or a folded unchanged stretch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shown {
    Row(usize),
    /// Rows `start..end` folded.
    Fold(usize, usize),
}

/// Unchanged stretches longer than this fold when collapsing.
const FOLD_MIN: usize = 8;
/// Context kept around changes when collapsing.
const FOLD_CONTEXT: usize = 3;

fn layout_rows(rows: &[Row], collapse: bool, opened: &HashSet<usize>) -> Vec<Shown> {
    if !collapse {
        return (0..rows.len()).map(Shown::Row).collect();
    }
    let mut shown = Vec::new();
    let mut r = 0;
    while r < rows.len() {
        if rows[r].kind != RowKind::Equal {
            shown.push(Shown::Row(r));
            r += 1;
            continue;
        }
        let start = r;
        while r < rows.len() && rows[r].kind == RowKind::Equal {
            r += 1;
        }
        let keep_head = if start == 0 { 0 } else { FOLD_CONTEXT };
        let keep_tail = if r == rows.len() { 0 } else { FOLD_CONTEXT };
        let fold_start = start + keep_head;
        let fold_end = r.saturating_sub(keep_tail);
        if fold_end > fold_start
            && fold_end - fold_start >= FOLD_MIN
            && !opened.contains(&fold_start)
        {
            shown.extend((start..fold_start).map(Shown::Row));
            shown.push(Shown::Fold(fold_start, fold_end));
            shown.extend((fold_end..r).map(Shown::Row));
        } else {
            shown.extend((start..r).map(Shown::Row));
        }
    }
    shown
}

/// Both texts with their highlighting.
struct Side2 {
    lines: Vec<String>,
    doc: Document,
    syntax: Syntax,
}

impl Side2 {
    fn new(text: &str, path: &std::path::Path) -> Self {
        let lines = text
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l).replace('\t', "    "))
            .collect::<Vec<_>>();
        // `split` gives a last empty piece after the final newline.
        let mut lines = lines;
        if text.ends_with('\n') || text.is_empty() {
            lines.pop();
        }
        Side2 {
            doc: Document::new(&lines.join("\n")),
            syntax: Syntax::for_path(Some(path)),
            lines,
        }
    }
}

pub(crate) struct DiffTab {
    pub id: u64,
    pub title: String,
    pub request: DiffRequest,
    pub data: Option<Result<DiffData, String>>,
    rows: Vec<Row>,
    shown: Vec<Shown>,
    old: Option<Side2>,
    new: Option<Side2>,
    pub collapse: bool,
    opened_folds: HashSet<usize>,
    /// Index into the change starts of the current change.
    current: usize,
    scroll_to: Option<usize>,
    /// The first load scrolls to the first change.
    first_load: bool,
    /// Horizontal scroll shared by both sides, in points.
    hscroll: f32,
}

impl DiffTab {
    fn set_data(&mut self, data: Result<DiffData, String>) {
        if let Ok(d) = &data {
            let path = d.request.path.clone();
            self.old = d.old_text.as_deref().map(|t| Side2::new(t, &path));
            self.new = d.new_text.as_deref().map(|t| Side2::new(t, &path));
            let old_len = self.old.as_ref().map_or(0, |s| s.lines.len());
            let new_len = self.new.as_ref().map_or(0, |s| s.lines.len());
            self.rows = match &d.diff {
                Some(diff) if !d.binary && !d.too_large => align(old_len, new_len, diff),
                _ if d.binary || d.too_large => Vec::new(),
                _ => (0..old_len.min(new_len))
                    .map(|i| Row {
                        left: Some(i),
                        right: Some(i),
                        kind: RowKind::Equal,
                        hunk: None,
                        change_start: false,
                        hunk_start: false,
                    })
                    .collect(),
            };
        } else {
            self.rows.clear();
        }
        self.data = Some(data);
        self.relayout();
        if std::mem::take(&mut self.first_load) {
            self.current = 0;
            self.scroll_to = self.change_rows().first().copied();
        } else {
            self.current = self.current.min(self.change_rows().len().saturating_sub(1));
        }
    }

    fn relayout(&mut self) {
        self.shown = layout_rows(&self.rows, self.collapse, &self.opened_folds);
    }

    /// Shown indices where a change starts.
    fn change_rows(&self) -> Vec<usize> {
        self.shown
            .iter()
            .enumerate()
            .filter_map(|(i, s)| match s {
                Shown::Row(r) if self.rows[*r].change_start => Some(i),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn change_count(&self) -> usize {
        self.rows.iter().filter(|r| r.change_start).count()
    }

    fn go(&mut self, forward: bool) {
        let starts = self.change_rows();
        if starts.is_empty() {
            return;
        }
        self.current = if forward {
            (self.current + 1).min(starts.len() - 1)
        } else {
            self.current.saturating_sub(1)
        };
        self.scroll_to = Some(starts[self.current]);
    }

    /// What the hunks of this comparison can do.
    fn hunk_actions(&self) -> Vec<HunkAction> {
        match (&self.request.from, &self.request.to) {
            (Side::Index, Side::WorkTree) => vec![HunkAction::Stage, HunkAction::Revert],
            (Side::Head, Side::Index) => vec![HunkAction::Unstage],
            (_, Side::WorkTree) => vec![HunkAction::Revert],
            _ => Vec::new(),
        }
    }
}

#[derive(Default)]
pub(crate) struct DiffTabs {
    pub tabs: Vec<DiffTab>,
    /// The diff shown instead of the editor.
    pub active: Option<usize>,
    next_id: u64,
    /// File tab that was active when the diff was shown: switching to
    /// another file tab hides the diff.
    shown_over: Option<Option<u64>>,
}

impl DiffTabs {
    /// Opens (or focuses) a diff; `(id, newly opened)`.
    pub(crate) fn open(
        &mut self,
        request: DiffRequest,
        title: String,
        active_buffer: Option<u64>,
    ) -> (u64, bool) {
        self.shown_over = Some(active_buffer);
        if let Some(i) = self.tabs.iter().position(|t| t.request == request) {
            self.active = Some(i);
            return (self.tabs[i].id, false);
        }
        self.next_id += 1;
        let id = self.next_id;
        self.tabs.push(DiffTab {
            id,
            title,
            request,
            data: None,
            rows: Vec::new(),
            shown: Vec::new(),
            old: None,
            new: None,
            collapse: false,
            opened_folds: HashSet::new(),
            current: 0,
            scroll_to: None,
            first_load: true,
            hscroll: 0.0,
        });
        self.active = Some(self.tabs.len() - 1);
        (id, true)
    }

    pub(crate) fn focus(&mut self, index: usize, active_buffer: Option<u64>) {
        if index < self.tabs.len() {
            self.active = Some(index);
            self.shown_over = Some(active_buffer);
        }
    }

    pub(crate) fn close(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        self.tabs.remove(index);
        self.active = match self.active {
            Some(a) if a == index => {
                if self.tabs.is_empty() {
                    None
                } else {
                    Some(index.min(self.tabs.len() - 1))
                }
            }
            Some(a) if a > index => Some(a - 1),
            other => other,
        };
    }

    /// Hides the diff when another file tab became active.
    pub(crate) fn follow_editor(&mut self, active_buffer: Option<u64>) {
        if self.active.is_some() && self.shown_over.is_some_and(|b| b != active_buffer) {
            self.active = None;
        }
    }

    pub(crate) fn loaded(&mut self, id: u64, data: Result<DiffData, String>) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.set_data(data);
        }
    }

    pub(crate) fn requests(&self) -> Vec<(u64, DiffRequest)> {
        self.tabs
            .iter()
            .map(|t| (t.id, t.request.clone()))
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn active_tab(&self) -> Option<&DiffTab> {
        self.active.and_then(|a| self.tabs.get(a))
    }
}

enum Act {
    Hunk(usize, HunkAction),
    OpenSource(Option<usize>),
    Copy(String),
    ToggleFold(usize),
    Collapse(bool),
    Go(bool),
    Refresh,
    Close,
}

/// The active diff tab, in place of the editor.
pub(crate) fn show(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let Some(index) = app.diffs.active else {
        return;
    };
    let theme = crate::design::current(ui.ctx());
    let p = theme.palette;
    let m = theme.metrics;
    let base_style = theme.editor_style();
    let mut acts = Vec::new();
    {
        let tab = &mut app.diffs.tabs[index];
        let language = tab
            .new
            .as_ref()
            .or(tab.old.as_ref())
            .map(|s| crate::app_editor::language_id(s.syntax.file_type()));
        let syntax_style = theme.syntax_style_for(language.as_deref());
        // Toolbar.
        ui.horizontal(|ui| {
            ui.set_height(m.tool_window_header);
            ui.add_space(4.0);
            let changes = tab.change_count();
            if IconButton::new(IconId::ChevronUp, "Previous Difference")
                .shortcut("Shift+F7")
                .enabled(changes > 0)
                .show(ui)
                .clicked()
            {
                acts.push(Act::Go(false));
            }
            if IconButton::new(IconId::ChevronDown, "Next Difference")
                .shortcut("F7")
                .enabled(changes > 0)
                .show(ui)
                .clicked()
            {
                acts.push(Act::Go(true));
            }
            if IconButton::new(IconId::CollapseAll, "Collapse Unchanged Fragments")
                .selected(tab.collapse)
                .show(ui)
                .clicked()
            {
                acts.push(Act::Collapse(!tab.collapse));
            }
            if IconButton::new(IconId::Refresh, "Refresh")
                .show(ui)
                .clicked()
            {
                acts.push(Act::Refresh);
            }
            if IconButton::new(IconId::ExternalLink, "Open in Editor")
                .shortcut("F4")
                .show(ui)
                .clicked()
            {
                acts.push(Act::OpenSource(None));
            }
            ui.add_space(8.0);
            let status = match &tab.data {
                None => "Loading…".to_string(),
                Some(Err(e)) => e.clone(),
                Some(Ok(d)) if d.binary => "Binary files differ".to_string(),
                Some(Ok(d)) if d.too_large => "The file is too large to compare".to_string(),
                Some(Ok(_)) if changes == 0 => "No differences".to_string(),
                Some(Ok(_)) => format!(
                    "{} difference{}",
                    changes,
                    if changes == 1 { "" } else { "s" }
                ),
            };
            ui.label(egui::RichText::new(status).color(p.text_secondary));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                if IconButton::new(IconId::Close, "Close Diff")
                    .shortcut("Ctrl+F4")
                    .show(ui)
                    .clicked()
                {
                    acts.push(Act::Close);
                }
            });
        });
        let font = syntax_style.font.clone();
        let row_height = (font.size * base_style.line_spacing).round();
        let char_width = ui.ctx().fonts_mut(|f| f.glyph_width(&font, 'm'));
        let full = ui.available_rect_before_wrap();
        let gutter = char_width * 5.0 + 12.0;
        let middle = if tab.hunk_actions().is_empty() {
            8.0
        } else {
            2.0 * m.icon_button + 8.0
        };
        let half = ((full.width() - middle) / 2.0).max(60.0);
        // Side headers.
        let header_h = 24.0;
        let header = egui::Rect::from_min_size(full.min, egui::vec2(full.width(), header_h));
        ui.painter().rect_filled(header, 0.0, p.bg_panel);
        let (left_label, right_label) = (tab.request.from.label(), tab.request.to.label());
        let header_font = egui::FontId::proportional(12.0);
        ui.painter().text(
            header.left_center() + egui::vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            left_label,
            header_font.clone(),
            p.text_secondary,
        );
        ui.painter().text(
            egui::pos2(header.left() + half + middle + 8.0, header.center().y),
            egui::Align2::LEFT_CENTER,
            right_label,
            header_font,
            p.text_secondary,
        );
        ui.allocate_rect(header, egui::Sense::hover());
        let body = egui::Rect::from_min_max(egui::pos2(full.left(), header.bottom()), full.max);
        ui.painter()
            .rect_filled(body, 0.0, base_style.colors.background);
        match &tab.data {
            Some(Ok(d)) if !d.binary && !d.too_large => {}
            _ => {
                ui.allocate_rect(body, egui::Sense::hover());
                drop_tab_acts(app, ui.ctx(), index, acts);
                return;
            }
        }
        let actions = tab.hunk_actions();
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt(("diff-scroll", tab.id))
            .auto_shrink([false, false]);
        if let Some(target) = tab.scroll_to.take() {
            let offset = (target as f32 * row_height - body.height() / 3.0).max(0.0);
            scroll = scroll.vertical_scroll_offset(offset);
        }
        // Horizontal scroll (Shift+wheel, trackpads): both sides move together.
        let widest = tab
            .old
            .iter()
            .chain(tab.new.iter())
            .flat_map(|side| side.lines.iter().map(|l| l.chars().count()))
            .max()
            .unwrap_or(0) as f32
            * char_width;
        let visible_text = half - gutter - 8.0;
        if ui.rect_contains_pointer(body) {
            let dx = ui.input(|i| i.smooth_scroll_delta.x);
            tab.hscroll -= dx;
        }
        tab.hscroll = tab.hscroll.clamp(0.0, (widest - visible_text).max(0.0));
        let hscroll = tab.hscroll;
        let shown = tab.shown.clone();
        let rows = tab.rows.clone();
        let mut current_hunk_rows = HashSet::new();
        if let Some(&start) = tab.change_rows().get(tab.current) {
            current_hunk_rows.insert(start);
        }
        ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
            scroll.show_rows(ui, row_height, shown.len(), |ui, range| {
                // Highlight spans for the visible lines of each side.
                let visible: Vec<Shown> = shown[range.clone()].to_vec();
                let span_range = |pick: &dyn Fn(&Row) -> Option<usize>| {
                    let lines: Vec<usize> = visible
                        .iter()
                        .filter_map(|s| match s {
                            Shown::Row(r) => pick(&rows[*r]),
                            _ => None,
                        })
                        .collect();
                    Some((*lines.iter().min()?, *lines.iter().max()?))
                };
                let left_range = span_range(&|r: &Row| r.left);
                let right_range = span_range(&|r: &Row| r.right);
                let left_spans = match (&mut tab.old, left_range) {
                    (Some(side), Some((a, b))) => (a, side.syntax.line_spans(&side.doc, a, b)),
                    _ => (0, Vec::new()),
                };
                let right_spans = match (&mut tab.new, right_range) {
                    (Some(side), Some((a, b))) => (a, side.syntax.line_spans(&side.doc, a, b)),
                    _ => (0, Vec::new()),
                };
                let top = ui.cursor().top();
                let left_x = ui.max_rect().left();
                for (k, shown_row) in visible.iter().enumerate() {
                    let y = top + k as f32 * row_height;
                    let row_rect = egui::Rect::from_min_size(
                        egui::pos2(left_x, y),
                        egui::vec2(full.width(), row_height),
                    );
                    match *shown_row {
                        Shown::Fold(a, b) => {
                            let response = ui.interact(
                                row_rect,
                                ui.id().with(("fold", a)),
                                egui::Sense::click(),
                            );
                            ui.painter().rect_filled(
                                row_rect,
                                0.0,
                                base_style.colors.fold_placeholder,
                            );
                            ui.painter().text(
                                row_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                format!("⋯ {} unchanged lines", b - a),
                                egui::FontId::proportional(12.0),
                                base_style.colors.fold_placeholder_text,
                            );
                            if response.clicked() {
                                acts.push(Act::ToggleFold(a));
                            }
                        }
                        Shown::Row(r) => {
                            let row = rows[r];
                            let (left_bg, right_bg, word_bg) = match row.kind {
                                RowKind::Equal => (None, None, None),
                                RowKind::Changed => (
                                    Some(p.git_modified.gamma_multiply(0.16)),
                                    Some(p.git_modified.gamma_multiply(0.16)),
                                    Some(p.git_modified.gamma_multiply(0.38)),
                                ),
                                RowKind::Added => (
                                    Some(p.git_deleted.gamma_multiply(0.08)),
                                    Some(p.diff_added_bg),
                                    None,
                                ),
                                RowKind::Deleted => (
                                    Some(p.diff_removed_bg),
                                    Some(p.git_deleted.gamma_multiply(0.08)),
                                    None,
                                ),
                            };
                            let left_rect = egui::Rect::from_min_size(
                                row_rect.min,
                                egui::vec2(half, row_height),
                            );
                            let right_rect = egui::Rect::from_min_size(
                                egui::pos2(row_rect.left() + half + middle, y),
                                egui::vec2(half, row_height),
                            );
                            if let Some(bg) = left_bg {
                                ui.painter().rect_filled(left_rect, 0.0, bg);
                            }
                            if let Some(bg) = right_bg {
                                ui.painter().rect_filled(right_rect, 0.0, bg);
                            }
                            // Changed part of a modified line.
                            let (lt, rt) = (
                                row.left
                                    .and_then(|l| tab.old.as_ref().and_then(|s| s.lines.get(l))),
                                row.right
                                    .and_then(|l| tab.new.as_ref().and_then(|s| s.lines.get(l))),
                            );
                            let words = match (row.kind, lt, rt) {
                                (RowKind::Changed, Some(a), Some(b)) => Some(inline_change(a, b)),
                                _ => None,
                            };
                            for (side_rect, line, text, spans, word_range) in [
                                (left_rect, row.left, lt, &left_spans, words.map(|w| w.0)),
                                (right_rect, row.right, rt, &right_spans, words.map(|w| w.1)),
                            ] {
                                let Some(line) = line else { continue };
                                let number_pos = egui::pos2(
                                    side_rect.left() + gutter - 8.0,
                                    side_rect.center().y,
                                );
                                ui.painter().text(
                                    number_pos,
                                    egui::Align2::RIGHT_CENTER,
                                    (line + 1).to_string(),
                                    base_style.gutter_font.clone(),
                                    base_style.colors.gutter_text,
                                );
                                let text = text.map(String::as_str).unwrap_or("");
                                let line_spans = spans.1.get(line.wrapping_sub(spans.0)).cloned();
                                let job = line_job(
                                    text,
                                    line_spans,
                                    &syntax_style,
                                    base_style.colors.text,
                                    word_range,
                                    word_bg,
                                );
                                let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
                                let clip = egui::Rect::from_min_max(
                                    egui::pos2(side_rect.left() + gutter, side_rect.top()),
                                    side_rect.max,
                                );
                                ui.painter().with_clip_rect(clip).galley(
                                    egui::pos2(
                                        clip.left() - hscroll,
                                        side_rect.center().y - galley.size().y / 2.0,
                                    ),
                                    galley,
                                    base_style.colors.text,
                                );
                            }
                            // Clicks: double click opens the source; menu copies.
                            let response = ui.interact(
                                row_rect,
                                ui.id().with(("diff-row", r)),
                                egui::Sense::click(),
                            );
                            let on_left = response
                                .interact_pointer_pos()
                                .or(response.hover_pos())
                                .is_some_and(|pos| pos.x < left_rect.right());
                            if response.double_clicked() {
                                acts.push(Act::OpenSource(row.right.or(row.left)));
                            }
                            let copy_text = if on_left { lt } else { rt }.cloned();
                            response.context_menu(|ui| {
                                if ui.button("Copy Line").clicked() {
                                    if let Some(text) = &copy_text {
                                        acts.push(Act::Copy(text.clone()));
                                    }
                                    ui.close();
                                }
                                if ui.button("Open in Editor").clicked() {
                                    acts.push(Act::OpenSource(row.right.or(row.left)));
                                    ui.close();
                                }
                            });
                            // Hunk actions in the middle column.
                            if row.hunk_start && !actions.is_empty() {
                                let at = egui::pos2(row_rect.left() + half + 4.0, y);
                                let rect = egui::Rect::from_min_size(
                                    at,
                                    egui::vec2(middle - 8.0, m.icon_button),
                                );
                                let hunk = row.hunk.unwrap_or(0);
                                ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 0.0;
                                        for action in &actions {
                                            let (icon, label) = match action {
                                                HunkAction::Stage => (IconId::Plus, "Stage"),
                                                HunkAction::Unstage => (IconId::Minus, "Unstage"),
                                                HunkAction::Revert => (IconId::Undo, "Revert"),
                                            };
                                            if IconButton::new(icon, label).show(ui).clicked() {
                                                acts.push(Act::Hunk(hunk, *action));
                                            }
                                        }
                                    });
                                });
                            }
                        }
                    }
                }
                ui.allocate_space(egui::vec2(full.width(), row_height * visible.len() as f32));
            });
        });
        // Divider between the sides.
        let x = body.left() + half + middle / 2.0;
        ui.painter().line_segment(
            [egui::pos2(x, body.top()), egui::pos2(x, body.bottom())],
            egui::Stroke::new(1.0, p.border_subtle),
        );
        let _ = current_hunk_rows;
        // Keys: F7 / Shift+F7, F4.
        if ui.memory(|m| m.focused().is_none()) {
            let (next, previous, source) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::F7) && !i.modifiers.shift,
                    i.key_pressed(egui::Key::F7) && i.modifiers.shift,
                    i.key_pressed(egui::Key::F4),
                )
            });
            if next {
                acts.push(Act::Go(true));
            }
            if previous {
                acts.push(Act::Go(false));
            }
            if source {
                acts.push(Act::OpenSource(None));
            }
        }
    }
    drop_tab_acts(app, ui.ctx(), index, acts);
}

fn drop_tab_acts(app: &mut PhpIdeApp, ctx: &egui::Context, index: usize, acts: Vec<Act>) {
    for act in acts {
        let Some(tab) = app.diffs.tabs.get_mut(index) else {
            return;
        };
        match act {
            Act::Go(forward) => tab.go(forward),
            Act::Collapse(on) => {
                tab.collapse = on;
                tab.opened_folds.clear();
                tab.relayout();
            }
            Act::ToggleFold(start) => {
                tab.opened_folds.insert(start);
                tab.relayout();
            }
            Act::Refresh => {
                let request = (tab.id, tab.request.clone());
                app.vcs.request_diff(&app.jobs, ctx, request.0, request.1);
            }
            Act::Copy(text) => ctx.copy_text(text),
            Act::Close => app.diffs.close(index),
            Act::OpenSource(line) => {
                let line = line.or_else(|| {
                    let starts = tab.change_rows();
                    starts.get(tab.current).and_then(|&s| match tab.shown[s] {
                        Shown::Row(r) => tab.rows[r].right.or(tab.rows[r].left),
                        _ => None,
                    })
                });
                let path: Option<PathBuf> = tab
                    .data
                    .as_ref()
                    .and_then(|d| d.as_ref().ok())
                    .map(|d| d.toplevel.join(&d.request.path));
                if let Some(path) = path.filter(|p| p.exists()) {
                    app.diffs.active = None;
                    app.open_file_tab(&path, false);
                    if let (Some(line), Some(buffer)) = (line, app.ws.active_buffer_mut()) {
                        buffer.request_cursor(line, 0);
                    }
                } else {
                    app.ws.message = "The file is not on disk".into();
                }
            }
            Act::Hunk(hunk, action) => {
                let diff: Option<FileDiff> = tab
                    .data
                    .as_ref()
                    .and_then(|d| d.as_ref().ok())
                    .and_then(|d| d.diff.clone());
                if let Some(diff) = diff {
                    app.git_quick(QuickOp::Hunk { diff, hunk, action });
                }
            }
        }
    }
}

/// The changed middle of two lines: char ranges `(left, right)` after the
/// common prefix and before the common suffix.
pub(crate) fn inline_change(a: &str, b: &str) -> ((usize, usize), (usize, usize)) {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let max_suffix = a.len().min(b.len()) - prefix;
    let suffix = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take(max_suffix)
        .take_while(|(x, y)| x == y)
        .count();
    ((prefix, a.len() - suffix), (prefix, b.len() - suffix))
}

pub(crate) fn line_job(
    text: &str,
    spans: Option<Arc<Vec<Span>>>,
    style: &phpide_php::SyntaxStyle,
    default: egui::Color32,
    word: Option<(usize, usize)>,
    word_bg: Option<egui::Color32>,
) -> egui::text::LayoutJob {
    let chars: Vec<char> = text.chars().collect();
    // Color per char.
    let mut colors = vec![default; chars.len()];
    if let Some(spans) = spans {
        for span in spans.iter() {
            let color = style.color(span.role);
            for c in colors
                .iter_mut()
                .take(span.end.min(chars.len()))
                .skip(span.start)
            {
                *c = color;
            }
        }
    }
    let mut job = egui::text::LayoutJob::default();
    let mut start = 0;
    while start < chars.len() {
        let in_word = |i: usize| word.is_some_and(|(a, b)| i >= a && i < b);
        let mut end = start + 1;
        while end < chars.len() && colors[end] == colors[start] && in_word(end) == in_word(start) {
            end += 1;
        }
        let piece: String = chars[start..end].iter().collect();
        job.append(
            &piece,
            0.0,
            egui::TextFormat {
                font_id: style.font.clone(),
                color: colors[start],
                background: if in_word(start) {
                    word_bg.unwrap_or(egui::Color32::TRANSPARENT)
                } else {
                    egui::Color32::TRANSPARENT
                },
                ..Default::default()
            },
        );
        start = end;
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;
    use phpide_vcs::{DiffLine, Hunk};

    fn line(kind: DiffLineKind, old: Option<u32>, new: Option<u32>) -> DiffLine {
        DiffLine {
            kind,
            content: String::new(),
            old_lineno: old,
            new_lineno: new,
            no_newline: false,
        }
    }

    #[test]
    fn rows_align_changes_additions_and_deletions() {
        use DiffLineKind::*;
        // old: a b c d e f      new: a B c x d f
        let diff = FileDiff {
            old_path: "f".into(),
            new_path: "f".into(),
            binary: false,
            hunks: vec![Hunk {
                old_start: 1,
                old_count: 6,
                new_start: 1,
                new_count: 6,
                lines: vec![
                    line(Context, Some(1), Some(1)),
                    line(Del, Some(2), None),
                    line(Add, None, Some(2)),
                    line(Context, Some(3), Some(3)),
                    line(Add, None, Some(4)),
                    line(Context, Some(4), Some(5)),
                    line(Del, Some(5), None),
                    line(Context, Some(6), Some(6)),
                ],
            }],
        };
        let rows = align(6, 6, &diff);
        let kinds: Vec<RowKind> = rows.iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            [
                RowKind::Equal,
                RowKind::Changed,
                RowKind::Equal,
                RowKind::Added,
                RowKind::Equal,
                RowKind::Deleted,
                RowKind::Equal
            ]
        );
        assert_eq!((rows[3].left, rows[3].right), (None, Some(3)));
        assert_eq!((rows[5].left, rows[5].right), (Some(4), None));
        assert_eq!((rows[6].left, rows[6].right), (Some(5), Some(5)));
        // One hunk: its actions sit on the first change only.
        assert_eq!(rows.iter().filter(|r| r.hunk_start).count(), 1);
        assert_eq!(rows.iter().filter(|r| r.change_start).count(), 3);
    }

    #[test]
    fn unchanged_lines_between_hunks_and_at_the_end_are_kept() {
        use DiffLineKind::*;
        let diff = FileDiff {
            old_path: "f".into(),
            new_path: "f".into(),
            binary: false,
            hunks: vec![Hunk {
                old_start: 20,
                old_count: 1,
                new_start: 20,
                new_count: 2,
                lines: vec![line(Context, Some(20), Some(20)), line(Add, None, Some(21))],
            }],
        };
        let rows = align(40, 41, &diff);
        assert_eq!(rows.len(), 41);
        assert_eq!(rows[20].kind, RowKind::Added);
        assert_eq!((rows[40].left, rows[40].right), (Some(39), Some(40)));
        // Collapsing folds the long unchanged stretches, keeping 3 lines of
        // context next to the change.
        let shown = layout_rows(&rows, true, &HashSet::new());
        assert!(shown.contains(&Shown::Fold(0, 17)), "{shown:?}");
        assert!(shown.contains(&Shown::Fold(24, 41)), "{shown:?}");
        assert_eq!(shown.len(), 1 + 3 + 1 + 3 + 1);
        // An opened fold stays open.
        let opened: HashSet<usize> = [0].into();
        assert!(!layout_rows(&rows, true, &opened).contains(&Shown::Fold(0, 17)));
    }

    #[test]
    fn inline_change_is_the_middle_between_common_ends() {
        assert_eq!(inline_change("echo $a;", "echo $b;"), ((6, 7), (6, 7)));
        assert_eq!(inline_change("abc", "abXYc"), ((2, 2), (2, 4)));
        assert_eq!(inline_change("same", "same"), ((4, 4), (4, 4)));
    }
}
