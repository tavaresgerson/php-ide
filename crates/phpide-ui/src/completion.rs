//! Popup de completamento (Estágio 3.1; T-PHP-003-01/02/03).
//!
//! A popup tem seu próprio campo de filtro (foco próprio) — combater por teclas
//! com o TextEdit embutido não é confiável. Ctrl+Espaço abre; gatilhos
//! (`$`, `->`, `\`) abrem com o prefixo já digitado; Enter/click aceita;
//! Esc fecha (enviando `$/cancelRequest`); o aceite aplica `textEdit`
//! (range UTF-16) ou `insertText`, com fallback para o rótulo.

use std::sync::mpsc;

use phpide_lsp::{CompletionItem, LspClient};
use serde_json::Value;

pub struct CompletionState {
    pub open: bool,
    pub just_opened: bool,
    /// Campo de filtro da popup (começa com o prefixo sob o cursor).
    pub query: String,
    /// Offset em caracteres onde a palavra sob o cursor começa.
    pub word_start: usize,
    /// Revisão do documento no disparo — resposta de outra revisão é
    /// descartada (T-PHP-001-04: resultado atrasado não se aplica).
    pub request_revision: u64,
    base: Vec<CompletionItem>,
    /// Itens crus (JSON) para textEdit/documentação no aceite.
    raw: Vec<Value>,
    pub selected: usize,
    rx: Option<mpsc::Receiver<Value>>,
    /// Id do request em voo (para `$/cancelRequest` no fechamento).
    request_id: Option<i64>,
}

impl CompletionState {
    pub fn new() -> Self {
        CompletionState {
            open: false,
            just_opened: false,
            query: String::new(),
            word_start: 0,
            request_revision: 0,
            base: Vec::new(),
            raw: Vec::new(),
            selected: 0,
            rx: None,
            request_id: None,
        }
    }

    pub fn loading(&self) -> bool {
        self.rx.is_some()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn begin(
        &mut self,
        lsp: &LspClient,
        path: &std::path::Path,
        line0: usize,
        col0: usize,
        word_start: usize,
        prefix: String,
        revision: u64,
    ) {
        // Cancela request anterior ainda em voo (digitação rápida).
        self.cancel_request(lsp);
        self.word_start = word_start;
        self.query = prefix;
        self.selected = 0;
        self.base.clear();
        self.raw.clear();
        self.open = true;
        self.just_opened = true;
        self.request_revision = revision;
        let (id, rx) = lsp.completion_with_id(path, line0, col0);
        self.request_id = Some(id);
        self.rx = Some(rx);
    }

    fn cancel_request(&mut self, lsp: &LspClient) {
        if let Some(id) = self.request_id.take() {
            lsp.cancel(id);
        }
        self.rx = None;
    }

    /// Consome a resposta do servidor (por frame). `revision` é a revisão
    /// atual do documento: divergindo, a resposta é descartada.
    pub fn poll(&mut self, lsp: &LspClient, revision: u64) {
        if revision != self.request_revision {
            // Texto mudou desde o disparo: sugestões obsoletas.
            if self.rx.is_some() {
                self.cancel_request(lsp);
                self.base.clear();
                self.raw.clear();
            }
            return;
        }
        if let Some(rx) = &self.rx {
            if let Ok(value) = rx.try_recv() {
                self.rx = None;
                self.request_id = None;
                self.raw = value
                    .get("items")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_else(|| value.as_array().cloned().unwrap_or_default());
                self.base = phpide_lsp::parse_completions(&value);
                if self.base.is_empty() {
                    self.close(lsp);
                }
            }
        }
    }

    pub fn filtered(&self) -> Vec<CompletionItem> {
        phpide_lsp::filter_completions(&self.base, &self.query)
    }

    /// Item cru selecionado (textEdit/documentação), alinhado com `filtered`.
    pub fn selected_raw(&self, visible_index: usize) -> Option<&Value> {
        let items = self.filtered();
        let label = items.get(visible_index)?.label.clone();
        // `filtered` pode reordenar: localiza o primeiro raw com o mesmo rótulo.
        self.raw
            .iter()
            .find(|v| v["label"].as_str() == Some(&label))
    }

    pub fn close(&mut self, lsp: &LspClient) {
        self.cancel_request(lsp);
        self.open = false;
        self.base.clear();
        self.raw.clear();
        self.query.clear();
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
    if let Some(range) = item["textEdit"]["range"].as_object() {
        let start = lsp_pos(doc, &range["start"])?;
        let end = lsp_pos(doc, &range["end"])?;
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

/// Extrai (prefixo, início) da palavra que termina no offset `cursor`.
/// Aceita identificadores PHP: letras, dígitos, `_`, `$` e `\` (namespaces).
pub fn word_at(chars: &[char], cursor: usize) -> (String, usize) {
    let mut start = cursor;
    while start > 0 {
        let c = chars[start - 1];
        if c.is_alphanumeric() || matches!(c, '_' | '$' | '\\') {
            start -= 1;
        } else {
            break;
        }
    }
    (chars[start..cursor].iter().collect(), start)
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
