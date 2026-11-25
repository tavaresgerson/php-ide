//! Language server features added with PHPantom (Phase 11): Go to Super
//! Method, code lenses, Extend/Shrink Selection and document links. Type
//! and call hierarchies live in [`crate::hierarchy`].

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use phpide_core::{Document, PositionEncoding};
use serde_json::{json, Value};

use crate::*;
use phpide_lsp::Location;

/// A char position from an LSP `Position` (UTF-16 columns).
pub fn char_of(doc: &Document, position: &Value) -> Option<usize> {
    let line = position["line"].as_u64()? as usize;
    let col = position["character"].as_u64()? as usize;
    if line >= doc.line_count() {
        return Some(doc.len_chars());
    }
    Some(doc.lsp_position_to_char(line, col, PositionEncoding::Utf16))
}

/// A char range from an LSP `Range`.
pub fn char_range(doc: &Document, range: &Value) -> Option<(usize, usize)> {
    let start = char_of(doc, &range["start"])?;
    let end = char_of(doc, &range["end"])?;
    Some((start.min(end), start.max(end)))
}

// ---------------------------------------------------------------------------
// Code lenses
// ---------------------------------------------------------------------------

/// A code lens of the active file.
#[derive(Debug, Clone, PartialEq)]
pub struct Lens {
    pub line0: usize,
    /// Empty until resolved.
    pub title: String,
    /// The LSP `Command`, when there is one.
    pub command: Option<Value>,
    /// The lens as the server sent it (for `codeLens/resolve`).
    pub raw: Value,
}

/// Lenses from a `textDocument/codeLens` answer, by line.
pub fn parse_lenses(value: &Value) -> Vec<Lens> {
    let mut lenses: Vec<Lens> = value
        .as_array()
        .map(|a| a.iter().filter_map(lens_of).collect())
        .unwrap_or_default();
    lenses.sort_by_key(|l| (l.line0, l.raw["range"]["start"]["character"].as_u64()));
    lenses
}

fn lens_of(value: &Value) -> Option<Lens> {
    let line0 = value["range"]["start"]["line"].as_u64()? as usize;
    let command = value.get("command").filter(|c| c.is_object()).cloned();
    let title = command
        .as_ref()
        .and_then(|c| c["title"].as_str())
        .unwrap_or_default()
        .to_string();
    Some(Lens {
        line0,
        title,
        command,
        raw: value.clone(),
    })
}

/// The code lenses of the active file.
#[derive(Default)]
pub struct CodeLensState {
    /// `(file, revision, lenses)` shown now.
    pub current: Option<(PathBuf, u64, Vec<Lens>)>,
    pending: Option<(PathBuf, u64, mpsc::Receiver<Value>)>,
    /// Resolves in flight, by lens index.
    resolving: Vec<(usize, mpsc::Receiver<Value>)>,
    /// The server asked for a refresh.
    pub stale: bool,
}

impl CodeLensState {
    pub fn clear(&mut self) {
        *self = CodeLensState::default();
    }
}

/// What a click on a lens does.
#[derive(Debug, Clone, PartialEq)]
pub enum LensAction {
    /// Jump to the only location.
    Open(Location),
    /// List the locations (Find tool window), with the lens title.
    List(String, Vec<Location>),
    /// `workspace/executeCommand` on the server.
    Execute(Value),
    None,
}

/// The action of a lens command. `editor.action.showReferences` carries
/// `(uri, position, locations)`, the shape VS Code established and PHPantom
/// sends to clients without `window.showDocument`.
pub fn lens_action(command: &Value) -> LensAction {
    let name = command["command"].as_str().unwrap_or_default();
    match name {
        "" => LensAction::None,
        "editor.action.showReferences" => {
            let locations = phpide_lsp::parse_locations(&command["arguments"][2]);
            match locations.len() {
                0 => LensAction::None,
                1 => LensAction::Open(locations[0].clone()),
                _ => LensAction::List(
                    command["title"].as_str().unwrap_or("Locations").to_string(),
                    locations,
                ),
            }
        }
        _ => LensAction::Execute(command.clone()),
    }
}

// ---------------------------------------------------------------------------
// Selection ranges
// ---------------------------------------------------------------------------

/// Nested ranges from a `textDocument/selectionRange` answer, innermost
/// first, without repeats.
pub fn parse_selection_ranges(doc: &Document, value: &Value) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut node = &value[0];
    let mut guard = 0;
    while node.is_object() && guard < 256 {
        if let Some(range) = char_range(doc, &node["range"]) {
            if out.last() != Some(&range) {
                out.push(range);
            }
        }
        node = &node["parent"];
        guard += 1;
    }
    out
}

/// The next range out from `current` (Extend Selection): the smallest that
/// strictly contains it.
pub fn extend(ranges: &[(usize, usize)], current: (usize, usize)) -> Option<(usize, usize)> {
    ranges
        .iter()
        .copied()
        .filter(|&(s, e)| s <= current.0 && current.1 <= e && (s, e) != current)
        .min_by_key(|&(s, e)| e - s)
}

/// The next range in from `current` (Shrink Selection): the largest that
/// it strictly contains, or the caret.
pub fn shrink(ranges: &[(usize, usize)], current: (usize, usize)) -> Option<(usize, usize)> {
    ranges
        .iter()
        .copied()
        .filter(|&(s, e)| current.0 <= s && e <= current.1 && (s, e) != current)
        .max_by_key(|&(s, e)| e - s)
}

/// Nested char ranges around a position, innermost first.
type Ranges = Vec<(usize, usize)>;

/// Extend/Shrink Selection (Ctrl+W / Ctrl+Shift+W).
#[derive(Default)]
pub struct SelectionRanges {
    /// `(file, revision, ranges)` of the last answer; Ctrl+W again reuses
    /// it while the selection is one of its ranges.
    cache: Option<(PathBuf, u64, Ranges)>,
    /// `(file, revision, extend, answer)`.
    pending: Option<(PathBuf, u64, bool, mpsc::Receiver<Value>)>,
    /// Selections the extensions came from, to shrink back exactly.
    history: Vec<(usize, usize)>,
}

// ---------------------------------------------------------------------------
// Document links
// ---------------------------------------------------------------------------

/// A link in the active file: char range and target URI.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub start: usize,
    pub end: usize,
    pub target: String,
}

pub fn parse_links(doc: &Document, value: &Value) -> Vec<Link> {
    value
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|l| {
                    let (start, end) = char_range(doc, &l["range"])?;
                    let target = l["target"].as_str()?.to_string();
                    (end > start).then_some(Link { start, end, target })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Default)]
pub struct LinkState {
    pub current: Option<(PathBuf, u64, Vec<Link>)>,
    pending: Option<(PathBuf, u64, mpsc::Receiver<Value>)>,
}

/// Where a link goes: a file (with a line from `#L12`) or the web.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkTarget {
    File(PathBuf, Option<usize>),
    Web(String),
}

pub fn link_target(target: &str) -> Option<LinkTarget> {
    if target.starts_with("http://") || target.starts_with("https://") {
        return Some(LinkTarget::Web(target.to_string()));
    }
    let (uri, fragment) = match target.split_once('#') {
        Some((uri, fragment)) => (uri, Some(fragment)),
        None => (target, None),
    };
    let path = phpide_lsp::uri_to_path(uri)?;
    let line = fragment
        .and_then(|f| f.strip_prefix('L'))
        .and_then(|n| n.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|n| n.parse::<usize>().ok())
        .map(|n| n.saturating_sub(1));
    Some(LinkTarget::File(path, line))
}

// ---------------------------------------------------------------------------
// The app
// ---------------------------------------------------------------------------

/// The running PHP server, when it is ready.
fn ready(lsp: &Option<LspClient>) -> Option<&LspClient> {
    lsp.as_ref()
        .filter(|l| matches!(l.status(), LspStatus::Running(_)))
}

impl PhpIdeApp {
    /// The active PHP file: path, revision and the caret's LSP position.
    fn active_php(&self) -> Option<(PathBuf, u64, usize, usize)> {
        let buf = self.ws.active_buffer()?;
        let path = buf.path.clone()?;
        if !phpide_php::Php::is_php_file(&path) {
            return None;
        }
        let (line, col) = buf.document.line_col_encoded(
            self.cursor_char.min(buf.document.len_chars()),
            PositionEncoding::Utf16,
        );
        Some((path, buf.document.revision(), line, col))
    }

    /// Go to Super Method (Ctrl+U): the method this one overrides or
    /// implements, by `textDocument/declaration`.
    pub(crate) fn request_super_method(&mut self) {
        self.request_locations("declaration", |lsp, path, line, col| {
            lsp.declaration(path, line, col)
        });
    }

    /// Shows several locations in the Find tool window.
    pub(crate) fn show_location_list(&mut self, title: &str, locations: &[Location]) {
        let hits: Vec<search::SearchHit> = locations
            .iter()
            .map(|loc| search::SearchHit {
                path: loc.path.clone(),
                line: loc.line0 + 1,
                preview: self
                    .line_preview(&loc.path, loc.line0)
                    .trim()
                    .chars()
                    .take(140)
                    .collect(),
            })
            .collect();
        let count = hits.len();
        self.search.results = hits;
        self.search.status = format!("{title} ({count})");
        self.search.query.clear();
        self.tw.set_visible(ToolWindow::Find, true);
        self.ws.message = format!("{title}: {count} found");
    }

    /// The text of a line: from the open buffer, else from disk.
    pub(crate) fn line_preview(&self, path: &Path, line0: usize) -> String {
        self.ws
            .buffers
            .iter()
            .find(|b| b.path.as_deref() == Some(path))
            .and_then(|b| b.document.line(line0).lines().next().map(str::to_string))
            .or_else(|| {
                std::fs::read_to_string(path)
                    .ok()
                    .and_then(|t| t.lines().nth(line0).map(str::to_string))
            })
            .unwrap_or_default()
    }

    // ---- Code lenses ----

    /// Requests and resolves the lenses of the active file.
    pub(crate) fn poll_code_lenses(&mut self) {
        let Some(lsp) = ready(&self.lsp) else {
            self.code_lens.clear();
            return;
        };
        let caps = lsp.capabilities();
        if !caps.code_lens_provider || !self.settings.code_lens {
            self.code_lens.clear();
            return;
        }
        let state = &mut self.code_lens;
        if let Some((path, revision, rx)) = &state.pending {
            match rx.try_recv() {
                Ok(result) => {
                    let mut lenses = parse_lenses(&result);
                    // Until they are resolved, lenses keep the title they
                    // had, so they do not blink while typing.
                    if let Some((old_path, _, old)) = &state.current {
                        if old_path == path {
                            for lens in lenses.iter_mut().filter(|l| l.command.is_none()) {
                                if let Some(before) = old.iter().find(|o| {
                                    o.line0 == lens.line0
                                        && o.raw["data"]["kind"] == lens.raw["data"]["kind"]
                                }) {
                                    lens.title = before.title.clone();
                                }
                            }
                        }
                    }
                    state.current = Some((path.clone(), *revision, lenses));
                    state.pending = None;
                    state.resolving.clear();
                }
                Err(mpsc::TryRecvError::Disconnected) => state.pending = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some((_, _, lenses)) = state.current.as_mut() {
            state.resolving.retain(|(index, rx)| match rx.try_recv() {
                Ok(resolved) => {
                    if let (Some(lens), Some(new)) = (lenses.get_mut(*index), lens_of(&resolved)) {
                        lens.title = new.title;
                        lens.command = new.command;
                    }
                    false
                }
                Err(mpsc::TryRecvError::Empty) => true,
                Err(mpsc::TryRecvError::Disconnected) => false,
            });
            if caps.code_lens_resolve {
                for (index, lens) in lenses.iter_mut().enumerate() {
                    if lens.command.is_none() && !lens.raw["resolving"].as_bool().unwrap_or(false) {
                        let rx = lsp.code_lens_resolve(&lens.raw);
                        lens.raw["resolving"] = json!(true);
                        state.resolving.push((index, rx));
                    }
                }
            }
        }
        let Some((path, revision, _, _)) = self.active_php() else {
            return;
        };
        let state = &mut self.code_lens;
        let fresh = state
            .current
            .as_ref()
            .is_some_and(|(p, r, _)| *p == path && *r == revision)
            && !state.stale;
        let asked = state
            .pending
            .as_ref()
            .is_some_and(|(p, r, _)| *p == path && *r == revision);
        if !fresh && !asked {
            state.stale = false;
            state.pending = Some((path.clone(), revision, lsp.code_lens(&path)));
        }
    }

    /// The lenses to paint for `path` (the last answer while a newer one is
    /// on its way, so they do not blink while typing).
    pub(crate) fn lens_decorations(
        &self,
        path: &Path,
    ) -> Vec<phpide_editor::decorations::CodeLens> {
        match &self.code_lens.current {
            Some((p, _, lenses)) if p == path => lenses
                .iter()
                .map(|l| phpide_editor::decorations::CodeLens {
                    line: l.line0,
                    label: l.title.clone(),
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// A click on lens `index` of the active file.
    pub(crate) fn click_lens(&mut self, index: usize) {
        let Some(command) = self
            .code_lens
            .current
            .as_ref()
            .and_then(|(_, _, lenses)| lenses.get(index))
            .and_then(|l| l.command.clone())
        else {
            return;
        };
        match lens_action(&command) {
            LensAction::Open(location) => {
                self.ws
                    .open_file_at(&location.path, location.line0, location.col0);
                self.editors.request_focus();
            }
            LensAction::List(title, locations) => self.show_location_list(&title, &locations),
            LensAction::Execute(command) => {
                if let Some(lsp) = ready(&self.lsp) {
                    let _ = lsp.execute_command(&command);
                }
            }
            LensAction::None => {}
        }
    }

    // ---- Extend / Shrink Selection ----

    /// Extend Selection (Ctrl+W) or, with `extend == false`, Shrink
    /// Selection (Ctrl+Shift+W).
    pub(crate) fn select_range(&mut self, extend_selection: bool) {
        let Some((path, revision, line, col)) = self.active_php() else {
            return;
        };
        let Some(current) = self.editors.selection(&self.ws) else {
            return;
        };
        let cached = match &self.selection_ranges.cache {
            Some((p, r, ranges)) if *p == path && *r == revision && ranges.contains(&current) => {
                Some(ranges.clone())
            }
            _ => None,
        };
        if let Some(ranges) = cached {
            let next = if extend_selection {
                extend(&ranges, current)
            } else {
                self.selection_ranges
                    .history
                    .pop()
                    .filter(|h| h.0 >= current.0 && h.1 <= current.1 && *h != current)
                    .or_else(|| shrink(&ranges, current))
            };
            if let Some(next) = next {
                self.apply_selection_step(current, next, extend_selection);
            }
            return;
        }
        self.selection_ranges.history.clear();
        if !extend_selection {
            return;
        }
        let Some(lsp) = ready(&self.lsp) else {
            self.ws.message = "Extend Selection needs the language server".to_string();
            return;
        };
        if !lsp.capabilities().selection_range_provider {
            self.ws.message = "the language server does not support selection ranges".to_string();
            return;
        }
        self.sync_documents();
        let Some(lsp) = ready(&self.lsp) else { return };
        // Asked from the selection's start: its ranges contain the selection.
        let (line, col) = match self.ws.active_buffer() {
            Some(buf) if current.0 != current.1 => buf
                .document
                .line_col_encoded(current.0, PositionEncoding::Utf16),
            _ => (line, col),
        };
        let rx = lsp.selection_range(&path, line, col);
        self.selection_ranges.pending = Some((path, revision, extend_selection, rx));
    }

    /// Tests: a selection range answer arrived.
    #[cfg(test)]
    pub(crate) fn selection_ranges_cached(&self) -> bool {
        self.selection_ranges.pending.is_none() && self.selection_ranges.cache.is_some()
    }

    fn apply_selection_step(&mut self, from: (usize, usize), to: (usize, usize), extended: bool) {
        if extended {
            self.selection_ranges.history.push(from);
        }
        self.editors.select(&mut self.ws, to.0, to.1);
        self.cursor_char = to.1;
    }

    pub(crate) fn poll_selection_ranges(&mut self) {
        let Some((path, revision, extend_selection, rx)) = &self.selection_ranges.pending else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.selection_ranges.pending = None;
                return;
            }
        };
        let (path, revision, extend_selection) = (path.clone(), *revision, *extend_selection);
        self.selection_ranges.pending = None;
        let Some(buf) = self.ws.active_buffer() else {
            return;
        };
        if buf.path.as_deref() != Some(path.as_path()) || buf.document.revision() != revision {
            return;
        }
        let ranges = parse_selection_ranges(&buf.document, &result);
        self.selection_ranges.cache = Some((path, revision, ranges.clone()));
        let Some(current) = self.editors.selection(&self.ws) else {
            return;
        };
        let next = if extend_selection {
            extend(&ranges, current)
        } else {
            shrink(&ranges, current)
        };
        if let Some(next) = next {
            self.apply_selection_step(current, next, extend_selection);
        }
    }

    // ---- Document links ----

    pub(crate) fn poll_document_links(&mut self) {
        let Some(lsp) = ready(&self.lsp) else {
            self.document_links = LinkState::default();
            return;
        };
        if !lsp.capabilities().document_link_provider {
            self.document_links = LinkState::default();
            return;
        }
        if let Some((path, revision, rx)) = &self.document_links.pending {
            match rx.try_recv() {
                Ok(result) => {
                    let (path, revision) = (path.clone(), *revision);
                    self.document_links.pending = None;
                    let buf = self
                        .ws
                        .buffers
                        .iter()
                        .find(|b| b.path.as_deref() == Some(path.as_path()));
                    if let Some(buf) = buf.filter(|b| b.document.revision() == revision) {
                        let links = parse_links(&buf.document, &result);
                        self.document_links.current = Some((path, revision, links));
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => self.document_links.pending = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let Some((path, revision, _, _)) = self.active_php() else {
            return;
        };
        let fresh = self
            .document_links
            .current
            .as_ref()
            .is_some_and(|(p, r, _)| *p == path && *r == revision);
        let asked = self
            .document_links
            .pending
            .as_ref()
            .is_some_and(|(p, r, _)| *p == path && *r == revision);
        if !fresh && !asked {
            let Some(lsp) = ready(&self.lsp) else { return };
            let rx = lsp.document_link(&path);
            self.document_links.pending = Some((path, revision, rx));
        }
    }

    /// Links of `path` at `revision`, as char ranges.
    pub(crate) fn link_decorations(&self, path: &Path, revision: u64) -> Vec<(usize, usize)> {
        match &self.document_links.current {
            Some((p, r, links)) if p == path && *r == revision => {
                links.iter().map(|l| (l.start, l.end)).collect()
            }
            _ => Vec::new(),
        }
    }

    /// Ctrl+click at `pos`: follows the document link there. Returns false
    /// when there is none (the click goes to Go to Definition).
    pub(crate) fn follow_link_at(&mut self, pos: usize, ctx: &egui::Context) -> bool {
        let Some(buf) = self.ws.active_buffer() else {
            return false;
        };
        let Some(path) = buf.path.clone() else {
            return false;
        };
        let revision = buf.document.revision();
        let target = match &self.document_links.current {
            Some((p, r, links)) if *p == path && *r == revision => links
                .iter()
                .find(|l| l.start <= pos && pos < l.end)
                .map(|l| l.target.clone()),
            _ => None,
        };
        let Some(target) = target else {
            return false;
        };
        match link_target(&target) {
            Some(LinkTarget::File(dir, _)) if dir.is_dir() => {
                self.files.reveal(&self.jobs, ctx, &dir);
                self.tree.select_only(dir);
                self.tw.set_visible(ToolWindow::Project, true);
            }
            Some(LinkTarget::File(file, line)) => {
                if file.exists() {
                    self.ws.open_file_at(&file, line.unwrap_or(0), 0);
                    self.editors.request_focus();
                } else {
                    self.ws.message = format!("{} does not exist", file.display());
                }
            }
            Some(LinkTarget::Web(url)) => {
                if let Err(e) = services::desktop::open_url(&url) {
                    self.ws.message = e;
                }
            }
            None => self.ws.message = format!("cannot open {target}"),
        }
        true
    }

    /// The server asked to refresh lenses or hints.
    pub(crate) fn handle_lsp_refreshes(&mut self) {
        let Some(lsp) = &self.lsp else { return };
        for method in lsp.take_refreshes() {
            match method.as_str() {
                "workspace/codeLens/refresh" => self.code_lens.stale = true,
                "workspace/semanticTokens/refresh" => self.semantic = None,
                "workspace/inlayHint/refresh" => {
                    self.hints.invalidate();
                    self.hints_path = None;
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slice(doc: &Document, (a, b): (usize, usize)) -> String {
        doc.text().chars().skip(a).take(b - a).collect()
    }

    #[test]
    fn lenses_and_their_actions() {
        let refs = json!([
            { "uri": "file:///p/a.php", "range": { "start": { "line": 3, "character": 1 }, "end": { "line": 3, "character": 4 } } },
            { "uri": "file:///p/b.php", "range": { "start": { "line": 8, "character": 0 }, "end": { "line": 8, "character": 3 } } }
        ]);
        let answer = json!([
            { "range": { "start": { "line": 5, "character": 4 }, "end": { "line": 5, "character": 9 } },
              "command": { "title": "2 references", "command": "editor.action.showReferences",
                           "arguments": ["file:///p/a.php", { "line": 5, "character": 4 }, refs] } },
            { "range": { "start": { "line": 1, "character": 0 }, "end": { "line": 1, "character": 5 } },
              "data": { "id": 7 } }
        ]);
        let lenses = parse_lenses(&answer);
        assert_eq!(lenses.len(), 2);
        assert_eq!((lenses[0].line0, lenses[0].title.as_str()), (1, ""));
        assert!(lenses[0].command.is_none());
        assert_eq!(lenses[1].title, "2 references");
        match lens_action(lenses[1].command.as_ref().unwrap()) {
            LensAction::List(title, locations) => {
                assert_eq!(title, "2 references");
                assert_eq!(locations.len(), 2);
            }
            other => panic!("{other:?}"),
        }
        let one = json!({ "title": "1 implementation", "command": "editor.action.showReferences",
                          "arguments": ["file:///p/a.php", { "line": 0, "character": 0 }, [refs[0]]] });
        assert!(matches!(lens_action(&one), LensAction::Open(l) if l.line0 == 3));
        let pending = json!({ "title": "- references", "command": "" });
        assert_eq!(lens_action(&pending), LensAction::None);
        let custom = json!({ "title": "Run", "command": "phpantom.run", "arguments": [1] });
        assert!(matches!(lens_action(&custom), LensAction::Execute(_)));
    }

    #[test]
    fn selection_ranges_extend_and_shrink() {
        let doc = Document::new("<?php\n$total = add($a, $b);\n");
        let range = |sl, sc, el, ec| json!({ "start": { "line": sl, "character": sc }, "end": { "line": el, "character": ec } });
        let answer = json!([{
            "range": range(1, 13, 1, 15),
            "parent": { "range": range(1, 13, 1, 15),
                "parent": { "range": range(1, 9, 1, 20),
                    "parent": { "range": range(1, 0, 1, 21),
                        "parent": { "range": range(0, 0, 2, 0) } } } }
        }]);
        let ranges = parse_selection_ranges(&doc, &answer);
        assert_eq!(ranges.len(), 4, "repeats dropped: {ranges:?}");
        let caret = (19, 19);
        let a = extend(&ranges, caret).unwrap();
        assert_eq!(slice(&doc, a), "$a");
        let call = extend(&ranges, a).unwrap();
        assert_eq!(slice(&doc, call), "add($a, $b)");
        let statement = extend(&ranges, call).unwrap();
        assert_eq!(slice(&doc, statement), "$total = add($a, $b);");
        assert_eq!(shrink(&ranges, statement), Some(call));
        assert_eq!(shrink(&ranges, a), None);
        let all = extend(&ranges, statement).unwrap();
        assert_eq!(extend(&ranges, all), None);
    }

    #[test]
    fn link_targets() {
        assert_eq!(
            link_target("file:///p/config/app.php#L12"),
            Some(LinkTarget::File(
                PathBuf::from("/p/config/app.php"),
                Some(11)
            ))
        );
        assert_eq!(
            link_target("file:///p/views"),
            Some(LinkTarget::File(PathBuf::from("/p/views"), None))
        );
        assert_eq!(
            link_target("https://php.net/array_map"),
            Some(LinkTarget::Web("https://php.net/array_map".into()))
        );
        let doc = Document::new("<?php\nrequire 'lib.php';\n");
        let links = parse_links(
            &doc,
            &json!([{ "range": { "start": { "line": 1, "character": 8 }, "end": { "line": 1, "character": 17 } },
                      "target": "file:///p/lib.php" }]),
        );
        assert_eq!(links.len(), 1);
        assert_eq!(slice(&doc, (links[0].start, links[0].end)), "'lib.php'");
    }
}
