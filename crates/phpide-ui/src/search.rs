//! Busca em arquivos (Estágio 2).
//!
//! Roda em thread separada e envia resultados por canal; a UI faz poll por frame,
//! sem bloquear a thread de desenho — mesmo padrão do daemon de análise dos IDEs
//! clássicos (trabalho pesado fora da UI, resultados chegam assíncronos).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::thread;

const MAX_HITS: usize = 500;
const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub path: PathBuf,
    /// 1-based para exibição.
    pub line: usize,
    pub preview: String,
}

enum SearchMsg {
    Hit(SearchHit),
    Done { searched: usize, truncated: bool },
}

/// Arquivos de texto que entram na busca (código e config comuns em projetos PHP).
pub fn is_searchable(path: &Path) -> bool {
    const EXT: [&str; 14] = [
        "php", "phtml", "inc", "json", "xml", "yml", "yaml", "md", "txt", "js", "ts", "css",
        "html", "env",
    ];
    path.extension()
        .map(|e| EXT.contains(&e.to_string_lossy().to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

pub struct SearchState {
    pub query: String,
    pub case_sensitive: bool,
    pub results: Vec<SearchHit>,
    pub status: String,
    rx: Option<Receiver<SearchMsg>>,
}

impl SearchState {
    pub fn new() -> Self {
        SearchState {
            query: String::new(),
            case_sensitive: false,
            results: Vec::new(),
            status: String::new(),
            rx: None,
        }
    }

    pub fn busy(&self) -> bool {
        self.rx.is_some()
    }

    /// Dispara busca pelos arquivos fornecidos (já filtrados por `is_searchable`).
    pub fn start(&mut self, files: Vec<PathBuf>) {
        if self.query.is_empty() || self.busy() {
            return;
        }
        self.results.clear();
        self.status = "buscando...".to_string();
        let (tx, rx) = channel();
        self.rx = Some(rx);
        let needle = if self.case_sensitive {
            self.query.clone()
        } else {
            self.query.to_lowercase()
        };
        let case_sensitive = self.case_sensitive;
        thread::spawn(move || {
            let mut searched = 0usize;
            let mut sent = 0usize;
            let mut truncated = false;
            'files: for path in files {
                if sent >= MAX_HITS {
                    truncated = true;
                    break;
                }
                let Ok(bytes) = fs::read(&path) else { continue };
                if bytes.len() > MAX_FILE_BYTES {
                    continue;
                }
                let Ok(text) = String::from_utf8(bytes) else { continue };
                searched += 1;
                for (i, raw) in text.lines().enumerate() {
                    let hay = if case_sensitive {
                        raw.to_string()
                    } else {
                        raw.to_lowercase()
                    };
                    if hay.contains(&needle) {
                        sent += 1;
                        let hit = SearchHit {
                            path: path.clone(),
                            line: i + 1,
                            preview: raw.trim().chars().take(140).collect(),
                        };
                        if tx.send(SearchMsg::Hit(hit)).is_err() || sent >= MAX_HITS {
                            truncated = true;
                            break 'files;
                        }
                    }
                }
            }
            let _ = tx.send(SearchMsg::Done { searched, truncated });
        });
    }

    /// Consome resultados prontos (chamar a cada frame).
    pub fn poll(&mut self) {
        let mut done = None;
        if let Some(rx) = &self.rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    SearchMsg::Hit(h) => self.results.push(h),
                    SearchMsg::Done { searched, truncated } => done = Some((searched, truncated)),
                }
            }
        }
        if let Some((searched, truncated)) = done {
            self.rx = None;
            self.status = format!(
                "{} arquivos, {} ocorrências{}",
                searched,
                self.results.len(),
                if truncated { " (truncado)" } else { "" }
            );
        }
    }
}
