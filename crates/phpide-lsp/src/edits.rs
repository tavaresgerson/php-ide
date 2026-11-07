//! WorkspaceEdit — `T-PHP-021-01` (infraestrutura) e alvo do auto-import
//! `T-PHP-003-05`.
//!
//! Modelo tipado do `WorkspaceEdit` LSP (documentChanges com versões,
//! formato legado `changes`) + parser tolerante. A aplicação multiarquivo,
//! preview e undo coeso vivem na UI (`phpide-ui::workspace_edit`) porque
//! precisam dos buffers; aqui fica só o contrato puro (testável).

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::uri_to_path;

/// Um edito textual num arquivo (range UTF-16 LSP 0-based → newText).
#[derive(Debug, Clone, PartialEq)]
pub struct TextEdit {
    pub start_line: u32,
    pub start_col_utf16: u32,
    pub end_line: u32,
    pub end_col_utf16: u32,
    pub new_text: String,
}

/// Arquivo alvo de um conjunto de editos.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentEdits {
    pub path: PathBuf,
    /// Versão do documento esperada pelo servidor (conflito se divergir).
    pub version: Option<i64>,
    pub edits: Vec<TextEdit>,
}

/// WorkspaceEdit normalizado: sempre por documento (changes legado vira
/// documentChanges sem versão).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorkspaceEdit {
    pub documents: Vec<DocumentEdits>,
}

impl WorkspaceEdit {
    pub fn is_empty(&self) -> bool {
        self.documents.iter().all(|d| d.edits.is_empty())
    }

    /// Número total de editos (resumo de preview).
    pub fn edit_count(&self) -> usize {
        self.documents.iter().map(|d| d.edits.len()).sum()
    }
}

/// Parse de `WorkspaceEdit` (tanto `documentChanges` quanto `changes`).
pub fn parse_workspace_edit(value: &Value) -> Option<WorkspaceEdit> {
    let mut out = WorkspaceEdit::default();
    // documentChanges: [{textDocument:{uri,version}, edits:[{range,newText}]}]
    // ou resourceOperations (create/rename/delete) — fora do recorte.
    if let Some(arr) = value.get("documentChanges").and_then(Value::as_array) {
        for change in arr {
            if change.get("kind").is_some() {
                continue; // ResourceOperation: não suportado (documentado)
            }
            let uri = change["textDocument"]["uri"].as_str()?;
            let path = uri_to_path(uri)?;
            let version = change["textDocument"]["version"].as_i64();
            let edits = change["edits"]
                .as_array()?
                .iter()
                .filter_map(parse_text_edit)
                .collect();
            out.documents.push(DocumentEdits {
                path,
                version,
                edits,
            });
        }
    }
    // changes legado: {uri: [edits]}
    if let Some(map) = value.get("changes").and_then(Value::as_object) {
        for (uri, edits) in map {
            let Some(path) = uri_to_path(uri) else {
                continue;
            };
            let edits = edits
                .as_array()
                .map(|a| a.iter().filter_map(parse_text_edit).collect())
                .unwrap_or_default();
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
        start_line: r["start"]["line"].as_u64()? as u32,
        start_col_utf16: r["start"]["character"].as_u64()? as u32,
        end_line: r["end"]["line"].as_u64()? as u32,
        end_col_utf16: r["end"]["character"].as_u64()? as u32,
        new_text: v["newText"].as_str()?.to_string(),
    })
}

/// additionalTextEdits de um CompletionItem (auto-import) como WorkspaceEdit
/// de um único documento — `T-PHP-003-05`.
pub fn parse_additional_edits(item: &Value, path: &Path) -> Option<WorkspaceEdit> {
    let edits = item.get("additionalTextEdits")?.as_array()?;
    let docs = vec![DocumentEdits {
        path: path.to_path_buf(),
        version: None,
        edits: edits.iter().filter_map(parse_text_edit).collect(),
    }];
    Some(WorkspaceEdit { documents: docs })
}

/// Converte os edits UTF-16 do LSP para ranges de char no texto (col por
/// linha, contagem UTF-16 por char — suficiente para BMP; astral conta 2).
pub fn utf16_range_to_char_range(text: &str, edit: &TextEdit) -> Option<(usize, usize)> {
    let lines: Vec<&str> = text.lines().collect();
    let line_start = |l: u32| -> Option<usize> {
        if l as usize >= lines.len() {
            return None;
        }
        Some(
            lines[..l as usize]
                .iter()
                .map(|s| s.chars().count() + 1)
                .sum(),
        )
    };
    let col_utf16_to_char = |line: &str, col: u32| -> usize {
        let mut u16s = 0u32;
        for (i, c) in line.chars().enumerate() {
            if u16s >= col {
                return i;
            }
            u16s += c.len_utf16() as u32;
        }
        line.chars().count()
    };
    let sl = lines.get(edit.start_line as usize)?;
    let el = lines.get(edit.end_line as usize)?;
    let start = line_start(edit.start_line)? + col_utf16_to_char(sl, edit.start_col_utf16);
    let end = line_start(edit.end_line)? + col_utf16_to_char(el, edit.end_col_utf16);
    Some((start, end))
}

/// Aplica os editos a um texto (offsets calculados de trás para frente para
/// não invalidar ranges). Retorna o novo texto.
pub fn apply_to_text(text: &str, edit: &WorkspaceEdit, path: &Path) -> Option<String> {
    let doc = edit.documents.iter().find(|d| d.path == path)?;
    let mut out = text.to_string();
    let mut spans: Vec<(usize, usize, &str)> = Vec::new();
    for e in &doc.edits {
        let (s, en) = utf16_range_to_char_range(&out, e)?;
        if s <= en && en <= out.chars().count() {
            spans.push((s, en, e.new_text.as_str()));
        }
    }
    // Aplica da maior posição para a menor.
    spans.sort_by_key(|(s, _, _)| std::cmp::Reverse(*s));
    for (s, en, new) in spans {
        out.replace_range(
            out.char_indices().nth(s)?.0
                ..out
                    .char_indices()
                    .nth(en)
                    .map(|(b, _)| b)
                    .unwrap_or(out.len()),
            new,
        );
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_document_changes_and_legacy_changes() {
        let v = json!({
            "documentChanges": [{
                "textDocument": { "uri": "file:///p/a.php", "version": 3 },
                "edits": [{ "range": { "start": {"line":0,"character":0}, "end": {"line":0,"character":5} }, "newText": "use X;" }]
            }]
        });
        let we = parse_workspace_edit(&v).unwrap();
        assert_eq!(we.documents.len(), 1);
        assert_eq!(we.documents[0].path, PathBuf::from("/p/a.php"));
        assert_eq!(we.documents[0].version, Some(3));
        assert_eq!(we.documents[0].edits[0].new_text, "use X;");

        let legacy = json!({ "changes": { "file:///p/b.php": [{ "range": { "start": {"line":1,"character":2}, "end": {"line":1,"character":2} }, "newText": ";" }] } });
        let we = parse_workspace_edit(&legacy).unwrap();
        assert_eq!(we.documents[0].version, None);
        assert_eq!(we.edit_count(), 1);
    }

    #[test]
    fn applies_edits_and_skips_out_of_range() {
        let we = WorkspaceEdit {
            documents: vec![DocumentEdits {
                path: PathBuf::from("/a"),
                version: None,
                edits: vec![
                    TextEdit {
                        start_line: 0,
                        start_col_utf16: 0,
                        end_line: 0,
                        end_col_utf16: 5,
                        new_text: "use App\\X;".into(),
                    },
                    TextEdit {
                        start_line: 1,
                        start_col_utf16: 0,
                        end_line: 1,
                        end_col_utf16: 0,
                        new_text: "$x = 1;\n".into(),
                    },
                ],
            }],
        };
        let new = apply_to_text("<?php\necho 1;\n", &we, PathBuf::from("/a").as_path()).unwrap();
        assert!(new.starts_with("use App\\X;"));
        assert!(new.lines().nth(1).unwrap().starts_with("$x = 1;"));
        // Fora do arquivo: Some(None) — edito descartado por segurança.
        let bad = WorkspaceEdit {
            documents: vec![DocumentEdits {
                path: PathBuf::from("/a"),
                version: None,
                edits: vec![TextEdit {
                    start_line: 99,
                    start_col_utf16: 0,
                    end_line: 99,
                    end_col_utf16: 1,
                    new_text: "x".into(),
                }],
            }],
        };
        assert!(apply_to_text("a\n", &bad, PathBuf::from("/a").as_path()).is_none());
    }

    #[test]
    fn utf16_columns_with_unicode() {
        // '😀' é astral: 1 char = 2 unidades UTF-16 (ocupa [1,3)).
        let e = TextEdit {
            start_line: 0,
            start_col_utf16: 3,
            end_line: 0,
            end_col_utf16: 3,
            new_text: "!".into(),
        };
        let (s, en) = utf16_range_to_char_range("a😀b", &e).unwrap();
        assert_eq!((s, en), (2, 2), "col UTF-16 3 = após o emoji (char 2)");
        // Remove 'b' (UTF-16 [3,4)).
        let e2 = TextEdit {
            start_line: 0,
            start_col_utf16: 3,
            end_line: 0,
            end_col_utf16: 4,
            new_text: String::new(),
        };
        let we = WorkspaceEdit {
            documents: vec![DocumentEdits {
                path: PathBuf::from("/a"),
                version: None,
                edits: vec![e2],
            }],
        };
        assert_eq!(
            apply_to_text("a😀b", &we, PathBuf::from("/a").as_path()).unwrap(),
            "a😀"
        );
    }
}
