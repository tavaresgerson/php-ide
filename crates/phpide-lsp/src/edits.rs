//! Modelo e aplicação pura de `WorkspaceEdit` LSP. Positions são UTF-16 e
//! convertidas pelo núcleo antes de qualquer mutação de buffers ou disco.

use std::path::{Path, PathBuf};

use phpide_core::{Document, PositionEncoding};
use serde_json::Value;

use crate::uri_to_path;

#[derive(Debug, Clone, PartialEq)]
pub struct TextEdit {
    pub start_line: u32,
    pub start_col_utf16: u32,
    pub end_line: u32,
    pub end_col_utf16: u32,
    pub new_text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentEdits {
    pub path: PathBuf,
    pub version: Option<i64>,
    pub edits: Vec<TextEdit>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorkspaceEdit {
    pub documents: Vec<DocumentEdits>,
}

impl WorkspaceEdit {
    pub fn is_empty(&self) -> bool {
        self.documents.iter().all(|d| d.edits.is_empty())
    }

    pub fn edit_count(&self) -> usize {
        self.documents.iter().map(|d| d.edits.len()).sum()
    }
}

pub fn parse_workspace_edit(value: &Value) -> Option<WorkspaceEdit> {
    let mut out = WorkspaceEdit::default();
    if let Some(arr) = value.get("documentChanges").and_then(Value::as_array) {
        for change in arr {
            if change.get("kind").is_some() {
                return None;
            }
            let uri = change["textDocument"]["uri"].as_str()?;
            let path = uri_to_path(uri)?;
            let version = change["textDocument"]["version"].as_i64();
            let edits = change["edits"]
                .as_array()?
                .iter()
                .map(parse_text_edit)
                .collect::<Option<Vec<_>>>()?;
            out.documents.push(DocumentEdits {
                path,
                version,
                edits,
            });
        }
    }
    if let Some(map) = value.get("changes").and_then(Value::as_object) {
        for (uri, edits) in map {
            let path = uri_to_path(uri)?;
            let edits = edits
                .as_array()?
                .iter()
                .map(parse_text_edit)
                .collect::<Option<Vec<_>>>()?;
            out.documents.push(DocumentEdits {
                path,
                version: None,
                edits,
            });
        }
    }
    Some(out)
}

fn parse_text_edit(v: &Value) -> Option<TextEdit> {
    let r = &v["range"];
    Some(TextEdit {
        start_line: u32::try_from(r["start"]["line"].as_u64()?).ok()?,
        start_col_utf16: u32::try_from(r["start"]["character"].as_u64()?).ok()?,
        end_line: u32::try_from(r["end"]["line"].as_u64()?).ok()?,
        end_col_utf16: u32::try_from(r["end"]["character"].as_u64()?).ok()?,
        new_text: v["newText"].as_str()?.to_string(),
    })
}

pub fn parse_additional_edits(item: &Value, path: &Path) -> Option<WorkspaceEdit> {
    parse_additional_edits_at_version(item, path, None)
}

pub fn parse_additional_edits_at_version(
    item: &Value,
    path: &Path,
    version: Option<i64>,
) -> Option<WorkspaceEdit> {
    let edits = item.get("additionalTextEdits")?.as_array()?;
    Some(WorkspaceEdit {
        documents: vec![DocumentEdits {
            path: path.to_path_buf(),
            version,
            edits: edits
                .iter()
                .map(parse_text_edit)
                .collect::<Option<Vec<_>>>()?,
        }],
    })
}

fn position_to_char(text: &str, line: u32, character: u32) -> Option<usize> {
    let doc = Document::new(text);
    let line = line as usize;
    if line >= doc.line_count() {
        return None;
    }
    let raw = doc.line(line);
    let content = raw
        .strip_suffix('\n')
        .unwrap_or(&raw)
        .strip_suffix('\r')
        .unwrap_or_else(|| raw.strip_suffix('\n').unwrap_or(&raw));
    let line_doc = Document::new(content);
    let units = line_doc.char_to_encoded_units(line_doc.len_chars(), PositionEncoding::Utf16);
    let character = character as usize;
    if character > units {
        return None;
    }
    let local = line_doc.lsp_position_to_char(0, character, PositionEncoding::Utf16);
    if line_doc.char_to_encoded_units(local, PositionEncoding::Utf16) != character {
        return None;
    }
    Some(doc.line_start(line) + local)
}

pub fn utf16_range_to_char_range(text: &str, edit: &TextEdit) -> Option<(usize, usize)> {
    let start = position_to_char(text, edit.start_line, edit.start_col_utf16)?;
    let end = position_to_char(text, edit.end_line, edit.end_col_utf16)?;
    (start <= end).then_some((start, end))
}

pub fn apply_to_text(text: &str, edit: &WorkspaceEdit, path: &Path) -> Option<String> {
    let doc_edit = edit.documents.iter().find(|d| d.path == path)?;
    let doc = Document::new(text);
    let mut spans = doc_edit
        .edits
        .iter()
        .map(|edit| {
            let (start, end) = utf16_range_to_char_range(text, edit)?;
            Some((start, end, edit.new_text.as_str()))
        })
        .collect::<Option<Vec<_>>>()?;
    spans.sort_by_key(|(start, end, _)| (*start, *end));
    for pair in spans.windows(2) {
        let (_, previous_end, _) = pair[0];
        let (next_start, _, _) = pair[1];
        if next_start < previous_end {
            return None;
        }
    }
    let mut out = text.to_string();
    for (start, end, new_text) in spans.into_iter().rev() {
        let start_byte = doc.char_to_byte(start);
        let end_byte = doc.char_to_byte(end);
        out.replace_range(start_byte..end_byte, new_text);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn edit(start_line: u32, start: u32, end_line: u32, end: u32, new_text: &str) -> TextEdit {
        TextEdit {
            start_line,
            start_col_utf16: start,
            end_line,
            end_col_utf16: end,
            new_text: new_text.into(),
        }
    }

    fn workspace(text_edits: Vec<TextEdit>) -> WorkspaceEdit {
        WorkspaceEdit {
            documents: vec![DocumentEdits {
                path: PathBuf::from("/a"),
                version: None,
                edits: text_edits,
            }],
        }
    }

    #[test]
    fn parses_document_changes_and_legacy_changes() {
        let value = json!({
            "documentChanges": [{
                "textDocument": { "uri": "file:///p/a.php", "version": 3 },
                "edits": [{ "range": { "start": {"line":0,"character":0}, "end": {"line":0,"character":5} }, "newText": "use X;" }]
            }]
        });
        let parsed = parse_workspace_edit(&value).unwrap();
        assert_eq!(parsed.documents[0].version, Some(3));
        assert_eq!(parsed.documents[0].edits[0].new_text, "use X;");

        let legacy = json!({ "changes": { "file:///p/b.php": [{ "range": { "start": {"line":1,"character":2}, "end": {"line":1,"character":2} }, "newText": ";" }] } });
        assert_eq!(parse_workspace_edit(&legacy).unwrap().edit_count(), 1);
    }

    #[test]
    fn rejects_malformed_edits_and_resource_operations() {
        let malformed = json!({"changes": {"file:///a": [{"range": {}}]}});
        assert!(parse_workspace_edit(&malformed).is_none());
        let resource = json!({"documentChanges": [{"kind": "rename", "oldUri": "file:///a", "newUri": "file:///b"}]});
        assert!(parse_workspace_edit(&resource).is_none());
    }

    #[test]
    fn applies_multiple_edits_against_original_positions() {
        let workspace = workspace(vec![
            edit(0, 0, 0, 0, " primeira\n"),
            edit(1, 0, 1, 0, " segunda\n"),
        ]);
        assert_eq!(
            apply_to_text("zero\num\n", &workspace, Path::new("/a")).unwrap(),
            " primeira\nzero\n segunda\num\n"
        );
    }

    #[test]
    fn rejects_out_of_range_overlap_and_surrogate_split() {
        assert!(apply_to_text(
            "a\n",
            &workspace(vec![edit(9, 0, 9, 0, "x")]),
            Path::new("/a")
        )
        .is_none());
        assert!(apply_to_text(
            "ab\n",
            &workspace(vec![edit(0, 9, 0, 9, "x")]),
            Path::new("/a")
        )
        .is_none());
        assert!(apply_to_text(
            "abc",
            &workspace(vec![edit(0, 0, 0, 2, "x"), edit(0, 1, 0, 3, "y")]),
            Path::new("/a")
        )
        .is_none());
        assert!(utf16_range_to_char_range("😀", &edit(0, 1, 0, 1, "x")).is_none());
    }

    #[test]
    fn supports_empty_file_eof_and_crlf() {
        assert_eq!(
            apply_to_text("", &workspace(vec![edit(0, 0, 0, 0, "x")]), Path::new("/a")).unwrap(),
            "x"
        );
        assert_eq!(
            apply_to_text(
                "a\n",
                &workspace(vec![edit(1, 0, 1, 0, "b")]),
                Path::new("/a")
            )
            .unwrap(),
            "a\nb"
        );
        assert_eq!(
            apply_to_text(
                "a\r\nb\r\n",
                &workspace(vec![edit(0, 1, 0, 1, "X"), edit(1, 1, 1, 1, "Y")]),
                Path::new("/a")
            )
            .unwrap(),
            "aX\r\nbY\r\n"
        );
        assert_eq!(
            apply_to_text(
                "a\r\n",
                &workspace(vec![edit(1, 0, 1, 0, "b")]),
                Path::new("/a")
            )
            .unwrap(),
            "a\r\nb"
        );
    }

    #[test]
    fn utf16_columns_with_unicode() {
        let (start, end) = utf16_range_to_char_range("a😀b", &edit(0, 3, 0, 4, "")).unwrap();
        assert_eq!((start, end), (2, 3));
        assert_eq!(
            apply_to_text(
                "a😀b",
                &workspace(vec![edit(0, 3, 0, 4, "")]),
                Path::new("/a")
            )
            .unwrap(),
            "a😀"
        );
    }
}
