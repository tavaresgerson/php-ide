//! Incremental document synchronisation (`textDocument/didChange` with
//! ranges).
//!
//! The client keeps a [`Mirror`] of each open document as the server last
//! saw it. On every revision it replays the editor's change log against the
//! mirror and turns each change into a ranged content change in UTF-16
//! positions. When the log no longer reaches back far enough, or the mirror
//! somehow diverged, the caller falls back to a full-text sync.

use phpide_core::{Document, PositionEncoding};
use serde_json::{json, Value};

/// The server's view of one document.
pub struct Mirror {
    doc: Document,
    revision: u64,
}

impl Mirror {
    pub fn new(text: &str, revision: u64) -> Self {
        Mirror {
            doc: Document::new(text),
            revision,
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Ranged content changes that take the server from this mirror to
    /// `current`, oldest first, and advances the mirror. `None` when a full
    /// sync is needed; the mirror is then reset to `current`.
    pub fn changes_to(&mut self, current: &Document) -> Option<Vec<Value>> {
        let revision = current.revision();
        if revision == self.revision {
            return Some(Vec::new());
        }
        let Some(applied) = current.changes_since(self.revision) else {
            self.reset(current);
            return None;
        };
        let mut events = Vec::new();
        for change_set in &applied {
            // Changes of one set are sorted and refer to the text before
            // the set; sending them last-first keeps every position valid.
            for change in change_set.changes.changes().iter().rev() {
                let start = self
                    .doc
                    .line_col_encoded(change.start, PositionEncoding::Utf16);
                let end = self
                    .doc
                    .line_col_encoded(change.end, PositionEncoding::Utf16);
                events.push(json!({
                    "range": crate::lsp_range(start, end),
                    "text": change.text,
                }));
            }
            self.doc.apply(&change_set.changes);
        }
        if self.doc.len_chars() != current.len_chars() {
            self.reset(current);
            return None;
        }
        self.revision = revision;
        Some(events)
    }

    fn reset(&mut self, current: &Document) {
        self.doc = Document::new(&current.text());
        self.revision = current.revision();
    }

    #[cfg(test)]
    fn text(&self) -> String {
        self.doc.text()
    }
}

/// Applies ranged content changes to `text` the way a server does (UTF-16
/// positions, applied in order). Used by the fixture server and tests.
pub fn apply_content_changes(text: &str, changes: &[Value]) -> String {
    let mut doc = Document::new(text);
    for change in changes {
        let new_text = change["text"].as_str().unwrap_or("");
        if change.get("range").is_none() {
            doc = Document::new(new_text);
            continue;
        }
        let start = char_at(&doc, &change["range"]["start"]);
        let end = char_at(&doc, &change["range"]["end"]);
        doc.apply(&phpide_core::ChangeSet::single(
            start.min(end),
            end.max(start),
            new_text,
        ));
    }
    doc.text()
}

/// Character offset of an LSP position (UTF-16 columns) in `doc`.
pub fn char_at(doc: &Document, position: &Value) -> usize {
    let line = position["line"].as_u64().unwrap_or(0) as usize;
    let col = position["character"].as_u64().unwrap_or(0) as usize;
    if line >= doc.line_count() {
        return doc.len_chars();
    }
    let start = doc.line_start(line);
    let mut units = 0;
    let mut offset = start;
    for c in doc.rope().line(line).chars() {
        if units >= col || c == '\n' || c == '\r' {
            break;
        }
        units += c.len_utf16();
        offset += 1;
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;
    use phpide_core::ChangeSet;

    #[test]
    fn replays_the_change_log_as_ranged_changes() {
        let mut doc = Document::new("<?php\necho 'olá';\n$a = 1;\n");
        let mut mirror = Mirror::new(&doc.text(), doc.revision());

        // One set with two changes (multi-caret) plus a later single edit.
        doc.apply(&ChangeSet::new(vec![
            phpide_core::Change::insert(6, "// 🚀\n"),
            phpide_core::Change::new(18, 20, "$b"),
        ]));
        doc.apply(&ChangeSet::single(0, 0, "#!/usr/bin/env php\n"));

        let events = mirror.changes_to(&doc).expect("incremental");
        assert_eq!(events.len(), 3);
        assert!(events.iter().all(|e| e.get("range").is_some()));
        let server = apply_content_changes("<?php\necho 'olá';\n$a = 1;\n", &events);
        assert_eq!(server, doc.text());
        assert_eq!(mirror.text(), doc.text());
        assert_eq!(mirror.revision(), doc.revision());
        assert_eq!(mirror.changes_to(&doc).unwrap().len(), 0);
    }

    #[test]
    fn utf16_columns_count_surrogate_pairs() {
        let doc = Document::new("a🚀b\n");
        let position = json!({ "line": 0, "character": 3 });
        assert_eq!(char_at(&doc, &position), 2);
    }
}
