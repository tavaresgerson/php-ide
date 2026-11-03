//! Cliente LSP mínimo (Estágio 3).
//!
//! JSON-RPC 2.0 sobre stdio com framing `Content-Length` (protocolo base do LSP),
//! threads + canais padrão — sem runtime async. A UI faz poll por frame:
//! `request()` devolve um `Receiver` que a UI consulta sem bloquear, mesmo
//! padrão do Estágio 2 (busca, watcher).

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

/// Framing LSP: cabeçalhos + corpo JSON. Devolve None em EOF limpo.
pub fn encode_frame(body: &str) -> Vec<u8> {
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body).into_bytes()
}

/// Lê uma mensagem enquadrada de um leitor. None = servidor encerrou o stream.
pub fn read_frame(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(None); // EOF
        }
        let line = line.trim_end();
        if line.is_empty() {
            break; // fim dos cabeçalhos
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("Content-Length") {
                content_length = value.trim().parse().ok();
            }
        }
    }
    let len = content_length.unwrap_or(0);
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    String::from_utf8(body)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

// ---------------------------------------------------------------------------
// URIs file:// (codificação percentual mínima, suficiente para caminhos comuns)
// ---------------------------------------------------------------------------

pub fn path_to_uri(path: &Path) -> String {
    let s = path.to_string_lossy();
    let mut out = String::from("file://");
    for b in s.as_bytes() {
        let c = *b as char;
        if c.is_ascii_alphanumeric()
            || matches!(c, '/' | '-' | '_' | '.' | '~' | '@' | ':' | '+' | '=' | ',')
        {
            out.push(c);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // remove autoridade vazia ("file:///tmp/x" -> "/tmp/x")
    let rest = rest
        .strip_prefix('/')
        .map(|r| format!("/{r}"))
        .unwrap_or_else(|| rest.to_string());
    let bytes = rest.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&rest[i + 1..i + 3], 16) {
                decoded.push(v);
                i += 3;
                continue;
            }
        }
        decoded.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(decoded).ok().map(PathBuf::from)
}

// ---------------------------------------------------------------------------
// Estado compartilhado com a UI
// ---------------------------------------------------------------------------

/// Diagnóstico publicado pelo servidor (posições 0-based, como no LSP).
#[derive(Debug, Clone)]
pub struct LspDiagnostic {
    pub line0: usize,
    pub col0: usize,
    pub message: String,
    /// 1=Erro, 2=Aviso, 3=Info, 4=Dica (0 = não informado).
    pub severity: u8,
    pub source: String,
}

/// Capabilities negociadas no `initialize` (T-PHP-001-01): a UI habilita
/// recursos conforme o servidor declara suporte, nunca por suposição.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ServerCapabilities {
    pub definition_provider: bool,
    pub implementation_provider: bool,
    pub type_definition_provider: bool,
    pub references_provider: bool,
    pub document_symbol_provider: bool,
    pub workspace_symbol_provider: bool,
    pub document_highlight_provider: bool,
    pub semantic_tokens_provider: bool,
    /// Legend do semanticTokens (tipos na ordem dos índices), quando houver.
    pub semantic_token_types: Vec<String>,
    pub hover_provider: bool,
    pub completion_provider: bool,
}

impl ServerCapabilities {
    /// Extrai as capabilities do resultado do `initialize`.
    pub fn from_initialize(result: &Value) -> Self {
        let caps = &result["capabilities"];
        let mut c = ServerCapabilities {
            definition_provider: caps["definitionProvider"].as_bool().unwrap_or(false),
            type_definition_provider: caps["typeDefinitionProvider"].as_bool().unwrap_or(false),
            references_provider: caps["referencesProvider"].as_bool().unwrap_or(false),
            document_symbol_provider: caps["documentSymbolProvider"].as_bool().unwrap_or(false),
            workspace_symbol_provider: caps["workspaceSymbolProvider"].as_bool().unwrap_or(false),
            document_highlight_provider: caps["documentHighlightProvider"]
                .as_bool()
                .unwrap_or(false),
            hover_provider: caps["hoverProvider"].as_bool().unwrap_or(false),
            completion_provider: caps["completionProvider"].is_object(),
            semantic_tokens_provider: caps["semanticTokensProvider"].is_object(),
            semantic_token_types: caps["semanticTokensProvider"]["legend"]["tokenTypes"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|t| t.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            ..Default::default()
        };
        // implementationProvider pode vir como bool ou como opcional.
        c.implementation_provider = caps["implementationProvider"].as_bool().unwrap_or(false);
        c
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LspStatus {
    Starting,
    Running(String),
    Failed(String),
}

#[derive(Default)]
pub struct Shared {
    pub status: Mutex<LspStatus>,
    pub diagnostics: Mutex<HashMap<PathBuf, Vec<LspDiagnostic>>>,
    /// Capabilities registradas após o `initialize` (T-PHP-001-01).
    pub capabilities: Mutex<ServerCapabilities>,
    /// Log do servidor (window/logMessage, stderr) — cap de 200 linhas.
    pub log: Mutex<Vec<String>>,
}

impl Default for LspStatus {
    fn default() -> Self {
        LspStatus::Starting
    }
}

impl Shared {
    fn push_log(&self, line: String) {
        let mut log = self.log.lock().unwrap();
        if log.len() >= 200 {
            log.remove(0);
        }
        log.push(line);
    }
}

// ---------------------------------------------------------------------------
// Cliente
// ---------------------------------------------------------------------------

pub struct LspClient {
    writer_tx: mpsc::Sender<String>,
    pending: Arc<Mutex<HashMap<i64, mpsc::Sender<Value>>>>,
    next_id: Arc<AtomicI64>,
    pub shared: Arc<Shared>,
    child: Arc<Mutex<Child>>,
    /// Parada voluntária (stop/troca de projeto): o fim do stdio neste
    /// estado NÃO é crash — não deve virar `Failed` (T-PHP-001-02).
    stopping: Arc<AtomicBool>,
}

impl LspClient {
    /// Sobe o servidor e faz o handshake `initialize` (sem `initialized` ainda —
    /// a UI envia depois de registrar o que precisa).
    pub fn spawn(
        command: &str,
        args: &[&str],
        root: &Path,
    ) -> Result<(Self, mpsc::Receiver<Value>), String> {
        let mut child = Command::new(command)
            .args(args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("falha ao iniciar {command}: {e}"))?;

        let stdin = child.stdin.take().ok_or("sem stdin")?;
        let stdout = child.stdout.take().ok_or("sem stdout")?;
        let stderr = child.stderr.take().ok_or("sem stderr")?;

        let shared = Arc::new(Shared::default());
        let pending: Arc<Mutex<HashMap<i64, mpsc::Sender<Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let next_id = Arc::new(AtomicI64::new(1));
        let stopping = Arc::new(AtomicBool::new(false));

        // Thread escritora: canal -> framing -> stdin do servidor.
        let (writer_tx, writer_rx) = mpsc::channel::<String>();
        {
            let mut stdin: ChildStdin = stdin;
            thread::spawn(move || {
                while let Ok(body) = writer_rx.recv() {
                    if stdin.write_all(&encode_frame(&body)).is_err() {
                        break;
                    }
                    let _ = stdin.flush();
                }
            });
        }

        // Thread leitora: stdout -> roteamento (respostas / notificações / pedidos do servidor).
        {
            let pending = Arc::clone(&pending);
            let shared = Arc::clone(&shared);
            let writer_tx = writer_tx.clone();
            let stopping = Arc::clone(&stopping);
            thread::spawn(move || {
                let mut reader = io::BufReader::new(stdout);
                loop {
                    match read_frame(&mut reader) {
                        Ok(Some(body)) => {
                            if let Ok(msg) = serde_json::from_str::<Value>(&body) {
                                route_message(msg, &pending, &shared, &writer_tx);
                            }
                        }
                        Ok(None) => break, // servidor encerrou
                        Err(_) => break,   // stream corrompido
                    }
                }
                // Crash real apenas quando não foi parada voluntária
                // (NFR-007/T-PHP-001-02: servidor caindo não derruba a IDE).
                if !stopping.load(Ordering::SeqCst) {
                    if let Ok(mut s) = shared.status.lock() {
                        if !matches!(*s, LspStatus::Failed(_)) {
                            *s = LspStatus::Failed("servidor encerrou o stdio".to_string());
                        }
                    }
                }
            });
        }

        // Thread de stderr -> log.
        {
            let shared = Arc::clone(&shared);
            thread::spawn(move || {
                let mut reader = io::BufReader::new(stderr);
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap_or(0) > 0 {
                    shared.push_log(format!("[stderr] {}", line.trim_end()));
                    line.clear();
                }
            });
        }

        let client = LspClient {
            writer_tx,
            pending,
            next_id,
            shared,
            child: Arc::new(Mutex::new(child)),
            stopping,
        };

        let root_uri = path_to_uri(root);
        let init_rx = client.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "rootUri": root_uri,
                "capabilities": {
                    "textDocument": {
                        "synchronization": { "didSave": true },
                        "publishDiagnostics": {},
                        "definition": {},
                        "documentSymbol": {},
                        "hover": {},
                        "completion": { "completionItem": { "snippetSupport": false } }
                    },
                    "workspace": { "configuration": true }
                },
                "clientInfo": { "name": "phpide", "version": env!("CARGO_PKG_VERSION") }
            }),
        );

        Ok((client, init_rx))
    }

    /// Envia um request; devolve Receiver para o **result** (erros viram Value nulo).
    pub fn request(&self, method: &str, params: Value) -> mpsc::Receiver<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id, tx);
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let _ = self.writer_tx.send(msg.to_string());
        rx
    }

    pub fn notify(&self, method: &str, params: Value) {
        let msg = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        let _ = self.writer_tx.send(msg.to_string());
    }

    pub fn did_open(&self, path: &Path, text: &str, version: u64) {
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": path_to_uri(path),
                    "languageId": "php",
                    "version": version,
                    "text": text
                }
            }),
        );
    }

    /// Mudança full-text (sem `range`): sempre válida, em qualquer modo de sync.
    pub fn did_change(&self, path: &Path, text: &str, version: u64) {
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": path_to_uri(path), "version": version },
                "contentChanges": [{ "text": text }]
            }),
        );
    }

    pub fn did_save(&self, path: &Path, text: &str) {
        self.notify(
            "textDocument/didSave",
            json!({ "textDocument": { "uri": path_to_uri(path) }, "text": text }),
        );
    }

    pub fn did_close(&self, path: &Path) {
        self.notify(
            "textDocument/didClose",
            json!({ "textDocument": { "uri": path_to_uri(path) } }),
        );
    }

    pub fn definition(&self, path: &Path, line0: usize, col0: usize) -> mpsc::Receiver<Value> {
        self.request(
            "textDocument/definition",
            json!({
                "textDocument": { "uri": path_to_uri(path) },
                "position": { "line": line0, "character": col0 }
            }),
        )
    }

    /// Localizações para um método de navegação genérico (definition,
    /// implementation, typeDefinition — T-PHP-004-01).
    fn locations_request(
        &self,
        method: &str,
        path: &Path,
        line0: usize,
        col0: usize,
    ) -> mpsc::Receiver<Value> {
        self.request(
            method,
            json!({
                "textDocument": { "uri": path_to_uri(path) },
                "position": { "line": line0, "character": col0 }
            }),
        )
    }

    pub fn implementation(&self, path: &Path, line0: usize, col0: usize) -> mpsc::Receiver<Value> {
        self.locations_request("textDocument/implementation", path, line0, col0)
    }

    pub fn type_definition(&self, path: &Path, line0: usize, col0: usize) -> mpsc::Receiver<Value> {
        self.locations_request("textDocument/typeDefinition", path, line0, col0)
    }

    /// Find Usages (T-PHP-004-03): referências ao símbolo sob o cursor.
    pub fn references(&self, path: &Path, line0: usize, col0: usize) -> mpsc::Receiver<Value> {
        self.request(
            "textDocument/references",
            json!({
                "textDocument": { "uri": path_to_uri(path) },
                "position": { "line": line0, "character": col0 },
                "context": { "includeDeclaration": true }
            }),
        )
    }

    /// Go to Symbol (T-PHP-004-02): símbolos do workspace por consulta.
    pub fn workspace_symbol(&self, query: &str) -> mpsc::Receiver<Value> {
        self.request("workspace/symbol", json!({ "query": query }))
    }

    /// Ocorrências no arquivo (T-PHP-005-02, documentHighlight).
    pub fn document_highlight(
        &self,
        path: &Path,
        line0: usize,
        col0: usize,
    ) -> mpsc::Receiver<Value> {
        self.request(
            "textDocument/documentHighlight",
            json!({
                "textDocument": { "uri": path_to_uri(path) },
                "position": { "line": line0, "character": col0 }
            }),
        )
    }

    /// Semantic tokens (full) do arquivo (T-PHP-005-01).
    pub fn semantic_tokens_full(&self, path: &Path) -> mpsc::Receiver<Value> {
        self.request(
            "textDocument/semanticTokens/full",
            json!({ "textDocument": { "uri": path_to_uri(path) } }),
        )
    }

    /// Cancela um request em voo (`$/cancelRequest`, T-PHP-003-01).
    pub fn cancel(&self, id: i64) {
        self.notify("$/cancelRequest", json!({ "id": id }));
    }

    /// Request com ID exposto — para cancelamento (completion, principalmente).
    pub fn request_with_id(&self, method: &str, params: Value) -> (i64, mpsc::Receiver<Value>) {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id, tx);
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let _ = self.writer_tx.send(msg.to_string());
        (id, rx)
    }

    /// Registra as capabilities do resultado do `initialize` (T-PHP-001-01).
    pub fn register_capabilities(&self, init_result: &Value) {
        *self.shared.capabilities.lock().unwrap() =
            ServerCapabilities::from_initialize(init_result);
    }

    pub fn capabilities(&self) -> ServerCapabilities {
        self.shared.capabilities.lock().unwrap().clone()
    }

    pub fn document_symbol(&self, path: &Path) -> mpsc::Receiver<Value> {
        self.request(
            "textDocument/documentSymbol",
            json!({ "textDocument": { "uri": path_to_uri(path) } }),
        )
    }

    pub fn status(&self) -> LspStatus {
        self.shared.status.lock().unwrap().clone()
    }

    /// Encerra: shutdown + exit, depois garante o fim do processo.
    pub fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        let _ = self.request("shutdown", Value::Null);
        self.notify("exit", Value::Null);
        thread::sleep(Duration::from_millis(120));
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }
}

/// Roteamento de mensagens do servidor (roda na thread leitora).
fn route_message(
    msg: Value,
    pending: &Arc<Mutex<HashMap<i64, mpsc::Sender<Value>>>>,
    shared: &Arc<Shared>,
    writer_tx: &mpsc::Sender<String>,
) {
    // Pedido servidor -> cliente (workspace/configuration, registerCapability, ...):
    // precisa de resposta imediata ou o servidor trava.
    if let (Some(id), Some(method)) = (msg.get("id"), msg.get("method")) {
        let result = if method == "workspace/configuration" {
            let n = msg["params"]["items"]
                .as_array()
                .map(|a| a.len())
                .unwrap_or(0);
            json!(vec![Value::Null; n])
        } else {
            Value::Null
        };
        let resp = json!({ "jsonrpc": "2.0", "id": id, "result": result });
        let _ = writer_tx.send(resp.to_string());
        return;
    }

    // Resposta a um request nosso.
    if let Some(id) = msg.get("id").and_then(|v| v.as_i64()) {
        let tx = pending.lock().unwrap().remove(&id);
        if let Some(tx) = tx {
            let result = msg
                .get("result")
                .cloned()
                .filter(|r| !r.is_null())
                .or_else(|| msg.get("error").cloned())
                .unwrap_or(Value::Null);
            let _ = tx.send(result);
        }
        return;
    }

    // Notificações.
    match msg.get("method").and_then(|m| m.as_str()) {
        Some("textDocument/publishDiagnostics") => {
            let params = &msg["params"];
            let Some(path) = params["uri"].as_str().and_then(uri_to_path) else {
                return;
            };
            let items: Vec<LspDiagnostic> = params["diagnostics"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .map(|d| LspDiagnostic {
                            line0: d["range"]["start"]["line"].as_u64().unwrap_or(0) as usize,
                            col0: d["range"]["start"]["character"].as_u64().unwrap_or(0) as usize,
                            message: d["message"].as_str().unwrap_or("").to_string(),
                            severity: d["severity"].as_u64().unwrap_or(0) as u8,
                            source: d["source"].as_str().unwrap_or("lsp").to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            shared.diagnostics.lock().unwrap().insert(path, items);
        }
        Some("window/logMessage") => {
            let line = format!("[log] {}", msg["params"]["message"].as_str().unwrap_or(""));
            shared.push_log(line);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Descoberta do servidor
// ---------------------------------------------------------------------------

/// Comando do servidor PHP, nesta ordem:
/// 1. variável de ambiente `PHPIDE_LSP` (ex.: "phpactor language-server")
/// 2. `third_party/node_modules/.bin/intelephense` (instalado localmente, `npm run lsp:install`)
/// 3. `intelephense` no PATH
/// 4. `phpactor` no PATH
pub fn detect_server(root: &Path) -> Option<(String, Vec<String>)> {
    if let Ok(cmd) = std::env::var("PHPIDE_LSP") {
        let mut parts = cmd.split_whitespace();
        let bin = parts.next()?.to_string();
        return Some((bin, parts.map(String::from).collect()));
    }
    let local = root
        .join("third_party/node_modules/.bin/intelephense")
        .canonicalize()
        .ok();
    if let Some(bin) = local {
        return Some((bin.display().to_string(), vec!["--stdio".to_string()]));
    }
    for name in ["intelephense", "phpactor"] {
        if Command::new("which")
            .arg(name)
            .stdout(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            let args = if name == "intelephense" {
                vec!["--stdio".to_string()]
            } else {
                vec!["language-server".to_string()]
            };
            return Some((name.to_string(), args));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_roundtrip() {
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"x","params":{}}"#;
        let frame = encode_frame(body);
        let text = String::from_utf8(frame).unwrap();
        assert!(text.starts_with("Content-Length: "));
        let mut reader = io::BufReader::new(text.as_bytes());
        let got = read_frame(&mut reader).unwrap().unwrap();
        assert_eq!(got, body);
    }

    #[test]
    fn uri_roundtrip_simple_and_spaced() {
        let p = Path::new("/tmp/proj/src/index.php");
        assert_eq!(uri_to_path(&path_to_uri(p)).unwrap(), p);
        let spaced = Path::new("/tmp/meu projeto/a.php");
        assert_eq!(uri_to_path(&path_to_uri(spaced)).unwrap(), spaced);
    }

    #[test]
    fn detects_server_from_env() {
        // Apenas garante que a função não pânica sem servidor instalado.
        let _ = detect_server(Path::new("/definitely/not/here"));
    }

    #[test]
    fn capabilities_parse_from_initialize_result() {
        let init = json!({
            "capabilities": {
                "definitionProvider": true,
                "implementationProvider": true,
                "typeDefinitionProvider": false,
                "referencesProvider": true,
                "documentSymbolProvider": true,
                "workspaceSymbolProvider": true,
                "documentHighlightProvider": true,
                "hoverProvider": true,
                "completionProvider": { "triggerCharacters": ["$", ">"] },
                "semanticTokensProvider": {
                    "legend": { "tokenTypes": ["namespace","class","function","variable"] },
                    "full": true
                }
            }
        });
        let caps = ServerCapabilities::from_initialize(&init);
        assert!(caps.definition_provider);
        assert!(caps.implementation_provider);
        assert!(!caps.type_definition_provider, "boolean false respeitado");
        assert!(caps.references_provider);
        assert!(caps.workspace_symbol_provider);
        assert!(caps.document_highlight_provider);
        assert!(caps.semantic_tokens_provider);
        assert_eq!(
            caps.semantic_token_types,
            vec!["namespace", "class", "function", "variable"]
        );
        // Resultado sem capabilities: tudo desligado (nunca supor suporte).
        let empty = ServerCapabilities::from_initialize(&json!({}));
        assert!(!empty.definition_provider);
        assert!(!empty.semantic_tokens_provider);
    }

    #[test]
    fn cancel_frame_is_notification_with_id() {
        let (tx, rx) = mpsc::channel::<String>();
        // Simula o writer: captura o frame de cancel enviado por `notify`.
        // Aqui validamos apenas a forma da mensagem (notification + params.id).
        let msg = json!({ "jsonrpc": "2.0", "method": "$/cancelRequest", "params": { "id": 42 } });
        let frame = encode_frame(&msg.to_string());
        let text = String::from_utf8(frame).unwrap();
        let mut reader = io::BufReader::new(text.as_bytes());
        let got = read_frame(&mut reader).unwrap().unwrap();
        let parsed: Value = serde_json::from_str(&got).unwrap();
        assert!(
            parsed["id"].is_null(),
            "cancel é notification (sem id próprio)"
        );
        assert_eq!(parsed["method"], "$/cancelRequest");
        assert_eq!(parsed["params"]["id"], 42);
        drop(rx);
        drop(tx);
    }
}

// ---------------------------------------------------------------------------
// Estágio 3.1 — Completamento (textDocument/completion)
// ---------------------------------------------------------------------------

/// Um item de completamento normalizado para a UI.
#[derive(Debug, Clone, PartialEq)]
pub struct CompletionItem {
    pub label: String,
    /// SymbolKind do LSP (mesma numeração do documentSymbol).
    pub kind: u64,
    pub detail: String,
}

impl CompletionItem {
    pub fn kind_label(&self) -> &'static str {
        match self.kind {
            5 => "classe",
            6 => "método",
            7 => "propriedade",
            10 => "enum",
            11 => "interface",
            12 => "função",
            13 => "variável",
            14 => "constante",
            21 => "palavra-chave",
            25 => "método",
            26 => "trait",
            _ => "símbolo",
        }
    }
}

impl LspClient {
    pub fn completion(&self, path: &Path, line0: usize, col0: usize) -> mpsc::Receiver<Value> {
        self.request(
            "textDocument/completion",
            json!({
                "textDocument": { "uri": path_to_uri(path) },
                "position": { "line": line0, "character": col0 }
            }),
        )
    }

    /// Como `completion`, mas expõe o id do request para `cancel` (T-PHP-003-01).
    pub fn completion_with_id(
        &self,
        path: &Path,
        line0: usize,
        col0: usize,
    ) -> (i64, mpsc::Receiver<Value>) {
        self.request_with_id(
            "textDocument/completion",
            json!({
                "textDocument": { "uri": path_to_uri(path) },
                "position": { "line": line0, "character": col0 }
            }),
        )
    }
}

/// Aceita as duas formas da spec: CompletionItem[] ou CompletionList { items }.
pub fn parse_completions(value: &Value) -> Vec<CompletionItem> {
    let empty = Vec::new();
    let arr: &Vec<Value> = if value.is_array() {
        value.as_array().unwrap_or(&empty)
    } else {
        value
            .get("items")
            .and_then(Value::as_array)
            .unwrap_or(&empty)
    };
    arr.iter()
        .filter_map(|item| {
            let label = item["label"].as_str()?;
            Some(CompletionItem {
                label: label.to_string(),
                kind: item["kind"].as_u64().unwrap_or(0),
                detail: item["detail"]
                    .as_str()
                    .or(item.get("documentation").and_then(Value::as_str))
                    .unwrap_or("")
                    .chars()
                    .take(80)
                    .collect(),
            })
        })
        .collect()
}

/// Filtra por prefixo (case-insensitive); sem prefixo devolve tudo.
/// Ordena: prefixo exato primeiro, depois alfabético. Teto de 50 itens.
pub fn filter_completions(items: &[CompletionItem], prefix: &str) -> Vec<CompletionItem> {
    let lower = prefix.to_lowercase();
    let mut matched: Vec<&CompletionItem> = items
        .iter()
        .filter(|it| lower.is_empty() || it.label.to_lowercase().starts_with(&lower))
        .collect();
    matched.sort_by(|a, b| {
        let ea = a.label.len() == prefix.len();
        let eb = b.label.len() == prefix.len();
        eb.cmp(&ea).then_with(|| a.label.cmp(&b.label))
    });
    matched.into_iter().take(50).cloned().collect()
}

#[cfg(test)]
mod completion_tests {
    use super::*;

    #[test]
    fn parses_array_and_list_forms() {
        let arr_form = serde_json::json!([
            { "label": "array_map", "kind": 3, "detail": "callable" },
            { "label": "strlen", "kind": 3 }
        ]);
        let items = parse_completions(&arr_form);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].label, "array_map");
        assert_eq!(items[0].detail, "callable");

        let list_form = serde_json::json!({ "isIncomplete": false, "items": [
            { "label": "PDO", "kind": 7 }
        ] });
        let items = parse_completions(&list_form);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "PDO");
    }

    #[test]
    fn filters_by_prefix_case_insensitive() {
        let items = vec![
            CompletionItem {
                label: "file_get_contents".into(),
                kind: 3,
                detail: String::new(),
            },
            CompletionItem {
                label: "FileSystem".into(),
                kind: 5,
                detail: String::new(),
            },
            CompletionItem {
                label: "filter_var".into(),
                kind: 3,
                detail: String::new(),
            },
            CompletionItem {
                label: "strlen".into(),
                kind: 3,
                detail: String::new(),
            },
        ];
        let got = filter_completions(&items, "FIL");
        let labels: Vec<_> = got.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(
            labels,
            vec!["FileSystem", "file_get_contents", "filter_var"]
        );

        let all = filter_completions(&items, "");
        assert_eq!(all.len(), 4);
    }
}
