//! Popup de completamento (Estágio 3.1).
//!
//! A popup tem seu próprio campo de filtro (foco próprio) — combater por teclas
//! com o TextEdit embutido não é confiável. Ctrl+Espaço abre; gatilhos
//! (`$`, `->`, `\`) abrem com o prefixo já digitado; Enter/click aceita;
//! Esc fecha; o aceite substitui a palavra sob o cursor no buffer.

use std::path::PathBuf;
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
    base: Vec<CompletionItem>,
    pub selected: usize,
    rx: Option<mpsc::Receiver<Value>>,
}

impl CompletionState {
    pub fn new() -> Self {
        CompletionState {
            open: false,
            just_opened: false,
            query: String::new(),
            word_start: 0,
            base: Vec::new(),
            selected: 0,
            rx: None,
        }
    }

    pub fn loading(&self) -> bool {
        self.rx.is_some()
    }

    pub fn begin(
        &mut self,
        lsp: &LspClient,
        path: &PathBuf,
        line0: usize,
        col0: usize,
        word_start: usize,
        prefix: String,
    ) {
        self.word_start = word_start;
        self.query = prefix;
        self.selected = 0;
        self.base.clear();
        self.open = true;
        self.just_opened = true;
        self.rx = Some(lsp.completion(path, line0, col0));
    }

    /// Consome a resposta do servidor (por frame).
    pub fn poll(&mut self) {
        if let Some(rx) = &self.rx {
            if let Ok(value) = rx.try_recv() {
                self.rx = None;
                self.base = phpide_lsp::parse_completions(&value);
                if self.base.is_empty() {
                    self.close();
                }
            }
        }
    }

    pub fn filtered(&self) -> Vec<CompletionItem> {
        phpide_lsp::filter_completions(&self.base, &self.query)
    }

    pub fn close(&mut self) {
        self.open = false;
        self.rx = None;
        self.base.clear();
        self.query.clear();
    }
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
