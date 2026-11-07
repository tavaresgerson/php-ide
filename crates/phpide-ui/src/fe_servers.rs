//! Language Servers de frontend — `T-FE-001-01/02` e `T-FE-002-01`.
//!
//! Registro de servidores Node por linguagem, descobertos no ambiente
//! (mesma política NFR-003: sem servidor, o recurso se autopula e a UI
//! explica). Cada servidor é um `LspClient` genérico; o roteamento por
//! extensão decide quem atende completion/navegação/diagnósticos.
//!
//! - TypeScript/JavaScript: `typescript-language-server --stdio` (TS-LS;
//!   versão do tsserver resolvida do workspace ou do próprio servidor —
//!   seleção de versão FE-002-01 documentada);
//! - HTML: `vscode-html-language-server --stdio` (diagnósticos, Emmet nas
//!   completions, docs MDN na documentação);
//! - CSS: `vscode-css-language-server --stdio` (diagnósticos, MDN,
//!   documentColor para color preview).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use phpide_lsp::{LspClient, LspStatus};
use serde_json::Value;

/// Extensões atendidas por cada servidor.
pub const TS_EXTS: &[&str] = &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];
pub const HTML_EXTS: &[&str] = &["html", "htm"];
pub const CSS_EXTS: &[&str] = &["css", "scss", "less"];

/// Um servidor gerenciado (cliente LSP + estado de sync).
pub struct FeServer {
    pub client: LspClient,
    pub name: &'static str,
    pub init_rx: Option<mpsc::Receiver<Value>>,
    pub opened: HashMap<PathBuf, u64>,
    pub initialized: bool,
}

impl FeServer {
    /// Dispara didOpen/didChange para os buffers de sua linguagem.
    pub fn sync_buffers<'a>(&mut self, buffers: impl Iterator<Item = (&'a PathBuf, &'a str, u64)>) {
        for (path, text, revision) in buffers {
            match self.opened.get(path) {
                None => {
                    self.client.did_open(path, text, revision);
                    self.opened.insert(path.clone(), revision);
                }
                Some(r) if *r != revision => {
                    self.client.did_change(path, text, revision);
                    self.opened.insert(path.clone(), revision);
                }
                _ => {}
            }
        }
    }
}

/// Servidores de frontend ativos.
pub struct FeServers {
    pub ts: Option<FeServer>,
    pub html: Option<FeServer>,
    pub css: Option<FeServer>,
}

impl FeServers {
    pub fn empty() -> Self {
        FeServers {
            ts: None,
            html: None,
            css: None,
        }
    }

    /// True quando nenhum servidor foi encontrado (skip NFR-003).
    pub fn is_empty(&self) -> bool {
        self.ts.is_none() && self.html.is_none() && self.css.is_none()
    }

    /// Servidor responsável pelo arquivo (por extensão).
    pub fn for_path(&self, path: &Path) -> Option<&FeServer> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        if TS_EXTS.contains(&ext.as_str()) {
            self.ts.as_ref()
        } else if HTML_EXTS.contains(&ext.as_str()) {
            self.html.as_ref()
        } else if CSS_EXTS.contains(&ext.as_str()) {
            self.css.as_ref()
        } else {
            None
        }
    }

    #[allow(dead_code)]
    pub fn for_path_mut(&mut self, path: &Path) -> Option<&mut FeServer> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        if TS_EXTS.contains(&ext.as_str()) {
            self.ts.as_mut()
        } else if HTML_EXTS.contains(&ext.as_str()) {
            self.html.as_mut()
        } else if CSS_EXTS.contains(&ext.as_str()) {
            self.css.as_mut()
        } else {
            None
        }
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut FeServer> {
        [self.ts.as_mut(), self.html.as_mut(), self.css.as_mut()]
            .into_iter()
            .flatten()
    }

    /// Total de diagnósticos publicados pendentes (drena os três).
    pub fn drain_diagnostics(&mut self) -> Vec<(PathBuf, Vec<phpide_lsp::LspDiagnostic>)> {
        let mut out = Vec::new();
        for s in self.iter_mut() {
            out.extend(s.client.shared.diagnostics.lock().unwrap().drain());
        }
        out
    }
}

/// Localiza um binário em `node_modules/.bin` (do workspace) ou no PATH.
fn find_bin(root: &Path, name: &str) -> Option<String> {
    // ../../third_party/node_modules/.bin relativo ao crate da UI, e
    // <root>/third_party + <root>/node_modules/.bin para projetos.
    let candidates = [
        root.join("third_party/node_modules/.bin").join(name),
        root.join("node_modules/.bin").join(name),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../third_party/node_modules/.bin")
            .join(name),
    ];
    for c in candidates {
        if c.exists() {
            return Some(c.to_string_lossy().into_owned());
        }
    }
    // PATH puro.
    std::process::Command::new("which")
        .arg(name)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Sobe os servidores disponíveis. Falhas individuais não impedem os demais
/// (degradação independente — FE-003 exige incrementos independentes).
pub fn detect_and_spawn(root: &Path) -> FeServers {
    let mut out = FeServers::empty();
    if let Some(bin) = find_bin(root, "typescript-language-server") {
        // tsserver do third_party (peer do typescript-language-server) quando
        // presente: versão determinística (FE-002-01).
        let tsserver = find_bin(root, "tsserver").map(|b| {
            PathBuf::from(b)
                .parent()
                .map(|p| p.join("../lib/tsserver.js"))
                .unwrap_or_default()
        });
        let mut args = vec!["--stdio".to_string()];
        if let Some(ts) = tsserver.filter(|p| p.exists()) {
            args.push("--tsserver-path".to_string());
            args.push(ts.to_string_lossy().into_owned());
        }
        if let Ok((client, init_rx)) = LspClient::spawn(
            &bin,
            &args.iter().map(String::as_str).collect::<Vec<_>>(),
            root,
        ) {
            out.ts = Some(FeServer {
                client,
                name: "TypeScript",
                init_rx: Some(init_rx),
                opened: HashMap::new(),
                initialized: false,
            });
        }
    }
    for (name, bin_name, slot) in [
        ("HTML", "vscode-html-language-server", 1u8),
        ("CSS", "vscode-css-language-server", 2u8),
    ] {
        if let Some(bin) = find_bin(root, bin_name) {
            if let Ok((client, init_rx)) = LspClient::spawn(&bin, &["--stdio"], root) {
                let srv = FeServer {
                    client,
                    name,
                    init_rx: Some(init_rx),
                    opened: HashMap::new(),
                    initialized: false,
                };
                match slot {
                    1 => out.html = Some(srv),
                    _ => out.css = Some(srv),
                }
            }
        }
    }
    out
}

/// Inicialização pendente (initialize → capabilities → initialized).
pub fn poll_init(srv: &mut FeServer) {
    if srv.initialized {
        return;
    }
    let Some(rx) = &srv.init_rx else { return };
    if let Ok(init) = rx.try_recv() {
        srv.client.register_capabilities(&init);
        srv.client.notify("initialized", Value::Null);
        *srv.client.shared.status.lock().unwrap() = LspStatus::Running(srv.name.to_string());
        srv.initialized = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_by_extension() {
        let mut fe = FeServers::empty();
        assert!(fe.for_path(Path::new("a.ts")).is_none());
        // Sem servidores: roteamento vazio (skip NFR-003), sem pânico.
        assert!(fe.for_path(Path::new("a.tsx")).is_none());
        assert!(fe.for_path(Path::new("a.html")).is_none());
        assert!(fe.for_path(Path::new("a.scss")).is_none());
        assert!(fe.for_path(Path::new("a.php")).is_none());
        assert!(fe.is_empty());
        let _ = &mut fe;
    }

    #[test]
    fn find_bin_finds_workspace_third_party() {
        // O workspace deste projeto tem third_party/node_modules (intelephense
        // no ambiente de dev) — quando ausente, o teste se autopula.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        if !root.join("third_party/node_modules/.bin").exists() {
            eprintln!("SKIP: third_party ausente");
            return;
        }
        // tsc/tsserver são dependências do typescript local (se instalado).
        let hit = find_bin(&root, "intelephense").or_else(|| find_bin(&root, "tsserver"));
        assert!(hit.is_some(), "binário conhecido não localizado");
    }
}
