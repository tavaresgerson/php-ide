//! Code completion (Phase 5): the popup filters as you keep typing in the
//! editor, shows the documentation of the selected item (resolved lazily),
//! and accepting applies the item's `textEdit` / `insertText`, expanding
//! snippets into tab stops.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use crate::routing::LspOrigin;

use phpide_lsp::{CompletionItem, LspClient};
use serde_json::{json, Value};

pub struct CompletionState {
    pub open: bool,
    /// Char offset where the word being completed starts.
    pub word_start: usize,
    /// What the user typed since `word_start` (filters the list).
    pub query: String,
    pub request_path: Option<PathBuf>,
    pub origin: LspOrigin,
    items: Vec<CompletionItem>,
    /// Indices into `items`, filtered and ranked for `query`.
    visible: Vec<usize>,
    pub selected: usize,
    rx: Option<mpsc::Receiver<Value>>,
    /// Id of the request in flight (`$/cancelRequest` when closing).
    request_id: Option<i64>,
    /// `completionItem/resolve` in flight: (item index, answer).
    resolving: Option<(usize, mpsc::Receiver<Value>)>,
    resolved: std::collections::HashSet<usize>,
    /// The server said the list is incomplete: ask again as the user types.
    incomplete: bool,
}

impl Default for CompletionState {
    fn default() -> Self {
        Self::new()
    }
}

impl CompletionState {
    pub fn new() -> Self {
        CompletionState {
            open: false,
            word_start: 0,
            query: String::new(),
            request_path: None,
            origin: LspOrigin::Php,
            items: Vec::new(),
            visible: Vec::new(),
            selected: 0,
            rx: None,
            request_id: None,
            resolving: None,
            resolved: Default::default(),
            incomplete: false,
        }
    }

    pub fn loading(&self) -> bool {
        self.rx.is_some()
    }

    pub fn incomplete(&self) -> bool {
        self.incomplete
    }

    /// Asks the server for completions at `line0:col0` (UTF-16).
    #[allow(clippy::too_many_arguments)]
    pub fn begin(
        &mut self,
        lsp: &LspClient,
        path: &Path,
        origin: LspOrigin,
        line0: usize,
        col0: usize,
        word_start: usize,
        query: String,
        trigger: Option<char>,
    ) {
        self.cancel_request(lsp);
        let reopen = self.open && self.word_start == word_start;
        self.word_start = word_start;
        self.query = query;
        if !reopen {
            self.selected = 0;
            self.items.clear();
            self.visible.clear();
            self.resolved.clear();
            self.resolving = None;
        }
        self.open = true;
        self.request_path = Some(path.to_path_buf());
        self.origin = origin;
        let context = match trigger {
            Some(c) => json!({ "triggerKind": 2, "triggerCharacter": c.to_string() }),
            None if reopen => json!({ "triggerKind": 3 }),
            None => json!({ "triggerKind": 1 }),
        };
        let (id, rx) = lsp.request_with_id(
            "textDocument/completion",
            json!({
                "textDocument": { "uri": phpide_lsp::path_to_uri(path) },
                "position": { "line": line0, "character": col0 },
                "context": context,
            }),
        );
        self.request_id = Some(id);
        self.rx = Some(rx);
    }

    fn cancel_request(&mut self, lsp: &LspClient) {
        if let Some(id) = self.request_id.take() {
            lsp.cancel(id);
        }
        self.rx = None;
    }

    /// Reads the server's answers (once per frame). Returns false when the
    /// popup should close (nothing to offer).
    pub fn poll(&mut self, lsp: &LspClient) -> bool {
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(value) => {
                    self.rx = None;
                    self.request_id = None;
                    self.incomplete = value["isIncomplete"].as_bool().unwrap_or(false);
                    let selected_label = self.selected_item().map(|i| i.label.clone());
                    self.items = phpide_lsp::parse_completions(&value);
                    self.resolved.clear();
                    self.resolving = None;
                    self.refilter();
                    if let Some(label) = selected_label {
                        if let Some(at) = self
                            .visible
                            .iter()
                            .position(|i| self.items[*i].label == label)
                        {
                            self.selected = at;
                        }
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.rx = None;
                    self.request_id = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some((index, rx)) = &self.resolving {
            if let Ok(value) = rx.try_recv() {
                let index = *index;
                if let Some(item) = self.items.get_mut(index) {
                    item.resolved(&value);
                }
                self.resolving = None;
            }
        }
        let _ = lsp;
        self.loading() || !self.visible.is_empty()
    }

    /// Asks the server for the documentation of the selected item once.
    pub fn resolve_selected(&mut self, lsp: &LspClient, supported: bool) {
        if !supported || self.resolving.is_some() {
            return;
        }
        let Some(&index) = self.visible.get(self.selected) else {
            return;
        };
        if self.resolved.insert(index) && self.items[index].documentation.is_none() {
            self.resolving = Some((index, lsp.completion_resolve(&self.items[index].raw)));
        }
    }

    pub fn set_query(&mut self, query: String) {
        if query != self.query {
            self.query = query;
            self.refilter();
        }
    }

    fn refilter(&mut self) {
        let ranked = phpide_lsp::filter_completions(&self.items, &self.query);
        // Map back to indices (labels plus sort text identify an item).
        self.visible = ranked
            .iter()
            .filter_map(|r| {
                self.items.iter().position(|i| {
                    i.label == r.label && i.sort_text == r.sort_text && i.kind == r.kind
                })
            })
            .collect();
        self.visible.dedup();
        let preselect = self.visible.iter().position(|i| self.items[*i].preselect);
        self.selected = preselect
            .unwrap_or(0)
            .min(self.visible.len().saturating_sub(1));
    }

    pub fn visible(&self) -> Vec<&CompletionItem> {
        self.visible.iter().map(|i| &self.items[*i]).collect()
    }

    pub fn selected_item(&self) -> Option<&CompletionItem> {
        self.visible.get(self.selected).map(|i| &self.items[*i])
    }

    pub fn move_selection(&mut self, delta: isize) {
        let len = self.visible.len();
        if len == 0 {
            return;
        }
        let next = self.selected as isize + delta;
        self.selected = if next < 0 {
            if delta == -1 {
                len - 1
            } else {
                0
            }
        } else if next as usize >= len {
            if delta == 1 {
                0
            } else {
                len - 1
            }
        } else {
            next as usize
        };
    }

    pub fn close(&mut self, lsp: Option<&LspClient>) {
        if let Some(lsp) = lsp {
            self.cancel_request(lsp);
        }
        self.rx = None;
        self.request_id = None;
        self.open = false;
        self.items.clear();
        self.visible.clear();
        self.resolving = None;
        self.resolved.clear();
        self.query.clear();
        self.request_path = None;
        self.incomplete = false;
    }
}

/// A snippet being filled in: its remaining tab stops, as char ranges at
/// `revision` of `path`.
pub struct SnippetSession {
    pub path: PathBuf,
    pub revision: u64,
    pub stops: Vec<(usize, usize)>,
    /// Next stop Tab visits.
    pub next: usize,
}

impl SnippetSession {
    /// Maps the stops through the edits made since `revision`. False when
    /// the log no longer reaches back (the session ends).
    pub fn follow(&mut self, doc: &phpide_core::Document) -> bool {
        let Some(applied) = doc.changes_since(self.revision) else {
            return false;
        };
        for change in applied {
            for stop in &mut self.stops {
                stop.0 = change.changes.map_pos(stop.0, phpide_core::Assoc::Before);
                stop.1 = change.changes.map_pos(stop.1, phpide_core::Assoc::After);
            }
        }
        self.revision = doc.revision();
        true
    }
}

/// Edição de aceite normalizada: intervalo em caracteres + texto inserido.
#[derive(Debug, Clone, PartialEq)]
pub struct AcceptEdit {
    pub start_char: usize,
    pub end_char: usize,
    pub new_text: String,
}

/// Resolve a edição de aceite de um item de completion sobre o documento
/// (T-PHP-003-02): `textEdit.range` (UTF-16) > `insertText` > `label`.
/// `word_start`/`cursor` em caracteres; posições LSP convertidas pelas
/// conversões centralizadas (NFR-002). `None` = aceitar só move o cursor.
pub fn accept_edit(
    doc: &phpide_core::Document,
    item: &Value,
    word_start: usize,
    cursor: usize,
) -> Option<AcceptEdit> {
    let range = item["textEdit"]["range"]
        .as_object()
        .or_else(|| item["textEdit"]["replace"].as_object());
    if let Some(range) = range {
        let start = lsp_pos(doc, &range["start"])?;
        // The range was computed when the list was asked for; what was
        // typed since then is replaced too.
        let end = lsp_pos(doc, &range["end"])?.max(cursor.min(doc.len_chars()));
        let new_text = item["textEdit"]["newText"]
            .as_str()
            .or_else(|| item["insertText"].as_str())
            .unwrap_or("")
            .to_string();
        return Some(AcceptEdit {
            start_char: start,
            end_char: end,
            new_text,
        });
    }
    // textEditText (LSP 3.16+) sem range: substitui a palavra sob o cursor.
    if let Some(text) = item["textEditText"].as_str() {
        return Some(AcceptEdit {
            start_char: word_start,
            end_char: cursor,
            new_text: text.to_string(),
        });
    }
    let new_text = item["insertText"]
        .as_str()
        .or_else(|| item["label"].as_str())?
        .to_string();
    Some(AcceptEdit {
        start_char: word_start,
        end_char: cursor,
        new_text,
    })
}

fn lsp_pos(doc: &phpide_core::Document, pos: &Value) -> Option<usize> {
    let line = pos["line"].as_u64()? as usize;
    let character = pos["character"].as_u64()? as usize;
    // O servidor negocia UTF-16 (default LSP); as conversões centrais
    // fazem o caminho exato, inclusive com astrais (NFR-002).
    Some(
        doc.lsp_position_to_char(line, character, phpide_core::PositionEncoding::Utf16)
            .min(doc.len_chars()),
    )
}

/// The identifier that ends at `cursor` and where it starts: letters,
/// digits, `_`, `$` and `\` (namespaces). Reads only the current line.
pub fn word_before(doc: &phpide_core::Document, cursor: usize) -> (String, usize) {
    let rope = doc.rope();
    let cursor = cursor.min(rope.len_chars());
    let line_start = rope.line_to_char(rope.char_to_line(cursor));
    let mut start = cursor;
    while start > line_start {
        let c = rope.char(start - 1);
        if c.is_alphanumeric() || matches!(c, '_' | '$' | '\\') {
            start -= 1;
        } else {
            break;
        }
    }
    (rope.slice(start..cursor).to_string(), start)
}

/// The identifier touching `cursor` (on either side) and where it starts.
pub fn word_around(doc: &phpide_core::Document, cursor: usize) -> (String, usize) {
    let rope = doc.rope();
    let cursor = cursor.min(rope.len_chars());
    let (_, start) = word_before(doc, cursor);
    let line = rope.char_to_line(cursor);
    let line_end = rope.line_to_char(line) + rope.line(line).len_chars();
    let mut end = cursor;
    while end < line_end {
        let c = rope.char(end);
        if c.is_alphanumeric() || c == '_' {
            end += 1;
        } else {
            break;
        }
    }
    (rope.slice(start..end).to_string(), start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use phpide_core::Document;

    #[test]
    fn accept_edit_uses_textedit_range_with_unicode() {
        // Texto com astral ANTES/NO range: as posições UTF-16 do servidor
        // convertem para os chars corretos da rope (NFR-002).
        // Linha 1: '$'(u0) '😀'(u1-2) 'x'(u3) ' '(u4) '='(u5).
        let doc = Document::new("<?php\n$😀x = 1;\n");
        let item = serde_json::json!({
            "label": "nome",
            "textEdit": {
                "range": { "start": { "line": 1, "character": 0 }, "end": { "line": 1, "character": 3 } },
                "newText": "$y"
            }
        });
        let edit = accept_edit(&doc, &item, 0, 0).unwrap();
        // Linha 1 começa em char 6; unidades 0..3 = '$'+'😀' (2 chars).
        assert_eq!((edit.start_char, edit.end_char), (6, 8), "😀 conta 1 char");
        assert_eq!(edit.new_text, "$y");
        // Range só sobre o 'x' (unidade 3..4).
        let item_x = serde_json::json!({
            "label": "z",
            "textEdit": {
                "range": { "start": { "line": 1, "character": 3 }, "end": { "line": 1, "character": 4 } },
                "newText": "z"
            }
        });
        let edit = accept_edit(&doc, &item_x, 0, 0).unwrap();
        assert_eq!((edit.start_char, edit.end_char), (8, 9));
    }

    #[test]
    fn accept_edit_falls_back_to_insert_text_and_label() {
        let doc = Document::new("<?php eco");
        let item = serde_json::json!({ "label": "echo", "insertText": "echo " });
        let edit = accept_edit(&doc, &item, 6, 9).unwrap();
        assert_eq!((edit.start_char, edit.end_char), (6, 9));
        assert_eq!(edit.new_text, "echo ");

        let label_only = serde_json::json!({ "label": "strlen" });
        let edit = accept_edit(&doc, &label_only, 6, 9).unwrap();
        assert_eq!(edit.new_text, "strlen");
    }

    #[test]
    fn accept_edit_handles_crlf_positions() {
        let doc = Document::new("<?php\r\n$a = 1;\r\n");
        // Linha 1 (após CRLF): substituir '$a' (unidades 0..=2 da linha).
        let item = serde_json::json!({
            "label": "b",
            "textEdit": {
                "range": { "start": { "line": 1, "character": 0 }, "end": { "line": 1, "character": 2 } },
                "newText": "$b"
            }
        });
        let edit = accept_edit(&doc, &item, 0, 0).unwrap();
        assert_eq!(
            (edit.start_char, edit.end_char),
            (7, 9),
            "linha 1 após \\r\\n"
        );
    }

    #[test]
    fn accept_edit_textedit_text_without_range_replaces_word() {
        let doc = Document::new("<?php str");
        let item = serde_json::json!({ "label": "x", "textEditText": "string" });
        let edit = accept_edit(&doc, &item, 6, 9).unwrap();
        assert_eq!((edit.start_char, edit.end_char), (6, 9));
        assert_eq!(edit.new_text, "string");
    }
}
