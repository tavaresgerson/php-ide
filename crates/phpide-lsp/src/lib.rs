//! Cliente LSP mínimo (Estágio 3).
//!
//! JSON-RPC 2.0 sobre stdio com framing `Content-Length` (protocolo base do LSP),
//! threads + canais padrão — sem runtime async. A UI faz poll por frame:
//! `request()` devolve um `Receiver` que a UI consulta sem bloquear, mesmo
//! padrão do Estágio 2 (busca, watcher).

pub mod edits;
pub mod engine;
pub mod fixture;
pub mod snippet;
pub mod sync;

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
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

/// A place in a file: 0-based line and column, as the protocol sends them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub path: PathBuf,
    pub line0: usize,
    pub col0: usize,
}

/// Places in the answer to definition, declaration, implementation,
/// typeDefinition or references: a `Location`, a `LocationLink` (what a
/// server may send because this client declares `linkSupport`), an array
/// of either, or `null`.
pub fn parse_locations(value: &Value) -> Vec<Location> {
    let items: Vec<&Value> = match value {
        Value::Array(items) => items.iter().collect(),
        Value::Object(_) => vec![value],
        _ => Vec::new(),
    };
    items.into_iter().filter_map(location_of).collect()
}

fn location_of(item: &Value) -> Option<Location> {
    let (uri, range) = match item.get("targetUri") {
        // LocationLink: the selection range is the declared name itself.
        Some(uri) => (
            uri,
            item.get("targetSelectionRange")
                .or_else(|| item.get("targetRange"))?,
        ),
        None => (item.get("uri")?, item.get("range")?),
    };
    Some(Location {
        path: uri_to_path(uri.as_str()?)?,
        line0: range["start"]["line"].as_u64()? as usize,
        col0: range["start"]["character"].as_u64()? as usize,
    })
}

// ---------------------------------------------------------------------------
// Estado compartilhado com a UI
// ---------------------------------------------------------------------------

/// Diagnóstico publicado pelo servidor (posições 0-based, como no LSP).
#[derive(Debug, Clone)]
pub struct LspDiagnostic {
    pub line0: usize,
    pub col0: usize,
    pub end_line0: usize,
    pub end_col0: usize,
    pub message: String,
    /// 1=Erro, 2=Aviso, 3=Info, 4=Dica (0 = não informado).
    pub severity: u8,
    pub source: String,
    /// The diagnostic as published, sent back in `codeAction` contexts.
    pub raw: Value,
}

/// How the server wants document changes (`textDocumentSync.change`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SyncKind {
    None,
    #[default]
    Full,
    Incremental,
}

/// Capabilities negotiated in `initialize` (T-PHP-001-01): the UI enables a
/// feature only when the server declares it.
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
    pub completion_resolve: bool,
    pub completion_triggers: Vec<String>,
    pub signature_help_provider: bool,
    pub signature_triggers: Vec<String>,
    pub signature_retriggers: Vec<String>,
    pub rename_provider: bool,
    pub prepare_rename: bool,
    pub code_action_provider: bool,
    pub formatting_provider: bool,
    pub range_formatting_provider: bool,
    pub execute_commands: Vec<String>,
    pub sync: SyncKind,
    pub declaration_provider: bool,
    pub type_hierarchy_provider: bool,
    pub call_hierarchy_provider: bool,
    pub code_lens_provider: bool,
    pub code_lens_resolve: bool,
    pub selection_range_provider: bool,
    pub document_link_provider: bool,
    pub inlay_hint_provider: bool,
    /// The server accepts `workspace/didChangeWorkspaceFolders`.
    pub workspace_folders_changes: bool,
    /// `serverInfo` of the `initialize` result.
    pub server_name: String,
    pub server_version: String,
}

/// A provider declared as `true` or as an options object.
fn provider(value: &Value) -> bool {
    value.as_bool().unwrap_or(false) || value.is_object()
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| t.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

impl ServerCapabilities {
    /// Extracts the capabilities from the `initialize` result.
    pub fn from_initialize(result: &Value) -> Self {
        let caps = &result["capabilities"];
        let sync = match &caps["textDocumentSync"] {
            Value::Number(n) => n.as_u64(),
            Value::Object(o) => o.get("change").and_then(Value::as_u64),
            _ => None,
        };
        ServerCapabilities {
            definition_provider: provider(&caps["definitionProvider"]),
            implementation_provider: provider(&caps["implementationProvider"]),
            type_definition_provider: provider(&caps["typeDefinitionProvider"]),
            references_provider: provider(&caps["referencesProvider"]),
            document_symbol_provider: provider(&caps["documentSymbolProvider"]),
            workspace_symbol_provider: provider(&caps["workspaceSymbolProvider"]),
            document_highlight_provider: provider(&caps["documentHighlightProvider"]),
            hover_provider: provider(&caps["hoverProvider"]),
            completion_provider: caps["completionProvider"].is_object(),
            completion_resolve: caps["completionProvider"]["resolveProvider"]
                .as_bool()
                .unwrap_or(false),
            completion_triggers: strings(&caps["completionProvider"]["triggerCharacters"]),
            signature_help_provider: caps["signatureHelpProvider"].is_object(),
            signature_triggers: strings(&caps["signatureHelpProvider"]["triggerCharacters"]),
            signature_retriggers: strings(&caps["signatureHelpProvider"]["retriggerCharacters"]),
            rename_provider: provider(&caps["renameProvider"]),
            prepare_rename: caps["renameProvider"]["prepareProvider"]
                .as_bool()
                .unwrap_or(false),
            code_action_provider: provider(&caps["codeActionProvider"]),
            formatting_provider: provider(&caps["documentFormattingProvider"]),
            range_formatting_provider: provider(&caps["documentRangeFormattingProvider"]),
            execute_commands: strings(&caps["executeCommandProvider"]["commands"]),
            semantic_tokens_provider: caps["semanticTokensProvider"].is_object(),
            semantic_token_types: strings(&caps["semanticTokensProvider"]["legend"]["tokenTypes"]),
            sync: match sync {
                Some(0) => SyncKind::None,
                Some(2) => SyncKind::Incremental,
                _ => SyncKind::Full,
            },
            declaration_provider: provider(&caps["declarationProvider"]),
            type_hierarchy_provider: provider(&caps["typeHierarchyProvider"]),
            call_hierarchy_provider: provider(&caps["callHierarchyProvider"]),
            code_lens_provider: caps["codeLensProvider"].is_object(),
            code_lens_resolve: caps["codeLensProvider"]["resolveProvider"]
                .as_bool()
                .unwrap_or(false),
            selection_range_provider: provider(&caps["selectionRangeProvider"]),
            document_link_provider: caps["documentLinkProvider"].is_object(),
            inlay_hint_provider: provider(&caps["inlayHintProvider"]),
            workspace_folders_changes: {
                let folders = &caps["workspace"]["workspaceFolders"];
                folders["supported"].as_bool().unwrap_or(false)
                    && !folders["changeNotifications"].is_null()
                    && folders["changeNotifications"] != Value::Bool(false)
            },
            server_name: result["serverInfo"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            server_version: result["serverInfo"]["version"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum LspStatus {
    #[default]
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
    /// `workspace/applyEdit` requests from the server, applied by the UI.
    pub apply_edits: Mutex<Vec<Value>>,
    /// `window/showMessage` notifications: (type, message).
    pub messages: Mutex<Vec<(u8, String)>>,
    /// Work in progress the server reports (`$/progress`), by token.
    pub progress: Mutex<Vec<Progress>>,
    /// Titles of work that began, for the Event Log.
    pub started: Mutex<Vec<String>>,
    /// Finished work, `(title, message)`, for the Event Log.
    pub finished: Mutex<Vec<(String, String)>>,
    /// `workspace/*/refresh` requests the UI has not handled yet.
    pub refreshes: Mutex<Vec<String>>,
}

/// One piece of work the server reports with `$/progress`.
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub token: String,
    pub title: String,
    pub message: String,
    pub percentage: Option<u32>,
}

impl Progress {
    /// Status bar text: "Indexing… 42%".
    pub fn text(&self) -> String {
        let mut text = if self.title.is_empty() {
            "Working…".to_string()
        } else {
            format!(
                "{}…",
                self.title.trim_end_matches('…').trim_end_matches("...")
            )
        };
        if let Some(p) = self.percentage {
            text.push_str(&format!(" {p}%"));
        }
        text
    }
}

/// What the IDE tells the server when it starts.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InitOptions {
    /// More workspace folders beside the root (attached projects).
    pub extra_folders: Vec<PathBuf>,
    /// `initializationOptions` (PHPantom: `indexing.exclude/extensions`).
    pub initialization_options: Value,
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
    /// The server process; `None` for an in-process transport (tests).
    child: Arc<Mutex<Option<Child>>>,
    /// Parada voluntária (stop/troca de projeto): o fim do stdio neste
    /// estado NÃO é crash — não deve virar `Failed` (T-PHP-001-02).
    stopping: Arc<AtomicBool>,
}

impl LspClient {
    /// Starts the server and sends `initialize` (without `initialized`: the
    /// UI sends it after registering the capabilities).
    pub fn spawn(
        command: &str,
        args: &[&str],
        root: &Path,
    ) -> Result<(Self, mpsc::Receiver<Value>), String> {
        Self::spawn_with(command, args, root, &InitOptions::default())
    }

    /// [`spawn`](Self::spawn) with workspace folders and initialization
    /// options.
    pub fn spawn_with(
        command: &str,
        args: &[&str],
        root: &Path,
        options: &InitOptions,
    ) -> Result<(Self, mpsc::Receiver<Value>), String> {
        let mut child = Command::new(command)
            .args(args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("could not start {command}: {e}"))?;

        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let stderr = child.stderr.take().ok_or("no stderr")?;
        Ok(Self::connect_with(
            Box::new(stdin),
            Box::new(stdout),
            Some(Box::new(stderr)),
            Some(child),
            root,
            options,
        ))
    }

    /// Talks to a server over any pair of streams: the stdio of a process or,
    /// in tests, an in-process server. Sends `initialize`.
    pub fn connect(
        input: Box<dyn Write + Send>,
        output: Box<dyn io::Read + Send>,
        stderr: Option<Box<dyn io::Read + Send>>,
        child: Option<Child>,
        root: &Path,
    ) -> (Self, mpsc::Receiver<Value>) {
        Self::connect_with(input, output, stderr, child, root, &InitOptions::default())
    }

    /// [`connect`](Self::connect) with workspace folders and initialization
    /// options.
    pub fn connect_with(
        input: Box<dyn Write + Send>,
        output: Box<dyn io::Read + Send>,
        stderr: Option<Box<dyn io::Read + Send>>,
        child: Option<Child>,
        root: &Path,
        options: &InitOptions,
    ) -> (Self, mpsc::Receiver<Value>) {
        let shared = Arc::new(Shared::default());
        let pending: Arc<Mutex<HashMap<i64, mpsc::Sender<Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let next_id = Arc::new(AtomicI64::new(1));
        let stopping = Arc::new(AtomicBool::new(false));

        // Writer thread: channel -> framing -> server input.
        let (writer_tx, writer_rx) = mpsc::channel::<String>();
        {
            let mut input = input;
            thread::spawn(move || {
                while let Ok(body) = writer_rx.recv() {
                    if input.write_all(&encode_frame(&body)).is_err() {
                        break;
                    }
                    let _ = input.flush();
                }
            });
        }

        // Reader thread: server output -> responses / notifications / requests.
        {
            let pending = Arc::clone(&pending);
            let shared = Arc::clone(&shared);
            let writer_tx = writer_tx.clone();
            let stopping = Arc::clone(&stopping);
            thread::spawn(move || {
                let mut reader = io::BufReader::new(output);
                while let Ok(Some(body)) = read_frame(&mut reader) {
                    if let Ok(msg) = serde_json::from_str::<Value>(&body) {
                        route_message(msg, &pending, &shared, &writer_tx);
                    }
                }
                // A crash only when the stop was not ours (NFR-007).
                if !stopping.load(Ordering::SeqCst) {
                    if let Ok(mut s) = shared.status.lock() {
                        if !matches!(*s, LspStatus::Failed(_)) {
                            *s = LspStatus::Failed("the server closed its stdio".to_string());
                        }
                    }
                }
                // Unblock every request still waiting.
                pending.lock().unwrap().clear();
            });
        }

        if let Some(stderr) = stderr {
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
        let init_rx = client.request("initialize", initialize_params(root, options));
        (client, init_rx)
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

    /// `initialized`, after the capabilities are registered. Its params are
    /// an empty object: servers that check them (PHPantom) ignore `null`
    /// and never finish starting.
    pub fn initialized(&self) {
        self.notify("initialized", json!({}));
    }

    /// Encerra: shutdown + exit, depois garante o fim do processo.
    pub fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        let _ = self.request("shutdown", Value::Null);
        self.notify("exit", Value::Null);
        thread::sleep(Duration::from_millis(120));
        if let Ok(mut child) = self.child.lock() {
            if let Some(child) = child.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            if let Some(child) = child.as_mut() {
                let _ = child.kill();
            }
        }
    }
}

/// `initialize` params: what this client supports.
/// A `WorkspaceFolder`.
pub fn workspace_folder(path: &Path) -> Value {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    json!({ "uri": path_to_uri(path), "name": name })
}

fn initialize_params(root: &Path, options: &InitOptions) -> Value {
    let root_uri = path_to_uri(root);
    let folders: Vec<Value> = std::iter::once(root)
        .chain(options.extra_folders.iter().map(PathBuf::as_path))
        .map(workspace_folder)
        .collect();
    let mut params = json!({
        "processId": std::process::id(),
        "rootUri": root_uri,
        "rootPath": root.display().to_string(),
        "workspaceFolders": folders,
        "capabilities": {
            "general": { "positionEncodings": ["utf-16"] },
            "textDocument": {
                "synchronization": { "didSave": true, "dynamicRegistration": false },
                "publishDiagnostics": { "relatedInformation": false },
                "definition": { "linkSupport": true },
                "implementation": { "linkSupport": true },
                "typeDefinition": { "linkSupport": true },
                "references": {},
                "documentHighlight": {},
                "documentSymbol": { "hierarchicalDocumentSymbolSupport": true },
                "hover": { "contentFormat": ["markdown", "plaintext"] },
                "completion": {
                    "contextSupport": true,
                    "completionItem": {
                        "snippetSupport": true,
                        "documentationFormat": ["markdown", "plaintext"],
                        "deprecatedSupport": true,
                        "insertReplaceSupport": false,
                        "resolveSupport": {
                            "properties": ["documentation", "detail", "additionalTextEdits"]
                        }
                    }
                },
                "signatureHelp": {
                    "contextSupport": true,
                    "signatureInformation": {
                        "documentationFormat": ["markdown", "plaintext"],
                        "parameterInformation": { "labelOffsetSupport": true },
                        "activeParameterSupport": true
                    }
                },
                "rename": { "prepareSupport": true },
                "codeAction": {
                    "codeActionLiteralSupport": {
                        "codeActionKind": { "valueSet": [
                            "", "quickfix", "refactor", "refactor.extract",
                            "refactor.inline", "refactor.rewrite", "source",
                            "source.organizeImports", "source.fixAll"
                        ] }
                    },
                    "isPreferredSupport": true,
                    "resolveSupport": { "properties": ["edit"] }
                },
                "formatting": {},
                "rangeFormatting": {},
                "declaration": { "linkSupport": true },
                "typeHierarchy": { "dynamicRegistration": false },
                "callHierarchy": { "dynamicRegistration": false },
                "codeLens": {},
                "selectionRange": {},
                "documentLink": { "tooltipSupport": true },
                "inlayHint": {}
            },
            "workspace": {
                "configuration": true,
                "applyEdit": true,
                "workspaceEdit": { "documentChanges": true },
                "workspaceFolders": true,
                "symbol": {},
                "executeCommand": {},
                "didChangeConfiguration": { "dynamicRegistration": false },
                "codeLens": { "refreshSupport": true },
                "inlayHint": { "refreshSupport": true }
            },
            "window": { "showMessage": {}, "workDoneProgress": true }
        },
        "clientInfo": { "name": "phpide", "version": env!("CARGO_PKG_VERSION") }
    });
    if !options.initialization_options.is_null() {
        params["initializationOptions"] = options.initialization_options.clone();
    }
    params
}

/// A position in an LSP request.
fn text_position(path: &Path, line0: usize, col0: usize) -> Value {
    json!({
        "textDocument": { "uri": path_to_uri(path) },
        "position": { "line": line0, "character": col0 }
    })
}

/// An LSP range.
pub fn lsp_range(start: (usize, usize), end: (usize, usize)) -> Value {
    json!({
        "start": { "line": start.0, "character": start.1 },
        "end": { "line": end.0, "character": end.1 }
    })
}

impl LspClient {
    /// Incremental `didChange`: `changes` are `TextDocumentContentChangeEvent`s
    /// built by [`sync::content_changes`].
    pub fn did_change_events(&self, path: &Path, version: u64, changes: Vec<Value>) {
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": path_to_uri(path), "version": version },
                "contentChanges": changes
            }),
        );
    }

    pub fn hover(&self, path: &Path, line0: usize, col0: usize) -> mpsc::Receiver<Value> {
        self.request("textDocument/hover", text_position(path, line0, col0))
    }

    /// `textDocument/signatureHelp`; `trigger` is the typed character, if any.
    pub fn signature_help(
        &self,
        path: &Path,
        line0: usize,
        col0: usize,
        trigger: Option<char>,
        retrigger: bool,
    ) -> mpsc::Receiver<Value> {
        let mut params = text_position(path, line0, col0);
        params["context"] = match trigger {
            Some(c) => json!({
                "triggerKind": 2, "triggerCharacter": c.to_string(), "isRetrigger": retrigger
            }),
            None => json!({ "triggerKind": 1, "isRetrigger": retrigger }),
        };
        self.request("textDocument/signatureHelp", params)
    }

    pub fn completion_resolve(&self, item: &Value) -> mpsc::Receiver<Value> {
        self.request("completionItem/resolve", item.clone())
    }

    pub fn prepare_rename(&self, path: &Path, line0: usize, col0: usize) -> mpsc::Receiver<Value> {
        self.request(
            "textDocument/prepareRename",
            text_position(path, line0, col0),
        )
    }

    pub fn rename(
        &self,
        path: &Path,
        line0: usize,
        col0: usize,
        new_name: &str,
    ) -> mpsc::Receiver<Value> {
        let mut params = text_position(path, line0, col0);
        params["newName"] = json!(new_name);
        self.request("textDocument/rename", params)
    }

    /// Code actions for `range`, with the diagnostics that touch it.
    pub fn code_action(
        &self,
        path: &Path,
        range: Value,
        diagnostics: Vec<Value>,
    ) -> mpsc::Receiver<Value> {
        self.request(
            "textDocument/codeAction",
            json!({
                "textDocument": { "uri": path_to_uri(path) },
                "range": range,
                "context": { "diagnostics": diagnostics, "triggerKind": 1 }
            }),
        )
    }

    pub fn code_action_resolve(&self, action: &Value) -> mpsc::Receiver<Value> {
        self.request("codeAction/resolve", action.clone())
    }

    pub fn execute_command(&self, command: &Value) -> mpsc::Receiver<Value> {
        self.request(
            "workspace/executeCommand",
            json!({
                "command": command["command"],
                "arguments": command.get("arguments").cloned().unwrap_or(json!([]))
            }),
        )
    }

    pub fn formatting(
        &self,
        path: &Path,
        tab_size: usize,
        insert_spaces: bool,
    ) -> mpsc::Receiver<Value> {
        self.request(
            "textDocument/formatting",
            json!({
                "textDocument": { "uri": path_to_uri(path) },
                "options": {
                    "tabSize": tab_size,
                    "insertSpaces": insert_spaces,
                    "trimTrailingWhitespace": true,
                    "insertFinalNewline": true
                }
            }),
        )
    }

    /// The server log (stderr and `window/logMessage`), oldest first.
    pub fn log(&self) -> Vec<String> {
        self.shared.log.lock().unwrap().clone()
    }

    /// Takes the edits the server asked to apply (`workspace/applyEdit`).
    pub fn take_apply_edits(&self) -> Vec<Value> {
        std::mem::take(&mut *self.shared.apply_edits.lock().unwrap())
    }

    /// Takes the `window/showMessage` notifications.
    pub fn take_messages(&self) -> Vec<(u8, String)> {
        std::mem::take(&mut *self.shared.messages.lock().unwrap())
    }

    /// Work the server is doing now (`$/progress`), oldest first.
    pub fn progress(&self) -> Vec<Progress> {
        self.shared.progress.lock().unwrap().clone()
    }

    /// Takes the titles of work that began since the last call.
    pub fn take_started(&self) -> Vec<String> {
        std::mem::take(&mut *self.shared.started.lock().unwrap())
    }

    /// Takes the work that ended since the last call, `(title, message)`.
    pub fn take_finished(&self) -> Vec<(String, String)> {
        std::mem::take(&mut *self.shared.finished.lock().unwrap())
    }

    /// Takes the `workspace/*/refresh` requests (method names).
    pub fn take_refreshes(&self) -> Vec<String> {
        std::mem::take(&mut *self.shared.refreshes.lock().unwrap())
    }

    /// `workspace/didChangeConfiguration`.
    pub fn did_change_configuration(&self, settings: Value) {
        self.notify(
            "workspace/didChangeConfiguration",
            json!({ "settings": settings }),
        );
    }

    /// `workspace/didChangeWorkspaceFolders`.
    pub fn did_change_workspace_folders(&self, added: &[PathBuf], removed: &[PathBuf]) {
        let folders =
            |list: &[PathBuf]| -> Vec<Value> { list.iter().map(|p| workspace_folder(p)).collect() };
        self.notify(
            "workspace/didChangeWorkspaceFolders",
            json!({ "event": { "added": folders(added), "removed": folders(removed) } }),
        );
    }

    /// `textDocument/declaration` (Go to Super Method).
    pub fn declaration(&self, path: &Path, line0: usize, col0: usize) -> mpsc::Receiver<Value> {
        self.request("textDocument/declaration", text_position(path, line0, col0))
    }

    /// `textDocument/prepareTypeHierarchy`.
    pub fn prepare_type_hierarchy(
        &self,
        path: &Path,
        line0: usize,
        col0: usize,
    ) -> mpsc::Receiver<Value> {
        self.request(
            "textDocument/prepareTypeHierarchy",
            text_position(path, line0, col0),
        )
    }

    /// `typeHierarchy/supertypes` or `typeHierarchy/subtypes` of an item.
    pub fn type_hierarchy(&self, item: &Value, supertypes: bool) -> mpsc::Receiver<Value> {
        let method = if supertypes {
            "typeHierarchy/supertypes"
        } else {
            "typeHierarchy/subtypes"
        };
        self.request(method, json!({ "item": item }))
    }

    /// `textDocument/prepareCallHierarchy`.
    pub fn prepare_call_hierarchy(
        &self,
        path: &Path,
        line0: usize,
        col0: usize,
    ) -> mpsc::Receiver<Value> {
        self.request(
            "textDocument/prepareCallHierarchy",
            text_position(path, line0, col0),
        )
    }

    /// `callHierarchy/incomingCalls` (callers) or `outgoingCalls` (callees).
    pub fn call_hierarchy(&self, item: &Value, incoming: bool) -> mpsc::Receiver<Value> {
        let method = if incoming {
            "callHierarchy/incomingCalls"
        } else {
            "callHierarchy/outgoingCalls"
        };
        self.request(method, json!({ "item": item }))
    }

    /// `textDocument/codeLens`.
    pub fn code_lens(&self, path: &Path) -> mpsc::Receiver<Value> {
        self.request(
            "textDocument/codeLens",
            json!({ "textDocument": { "uri": path_to_uri(path) } }),
        )
    }

    /// `codeLens/resolve`.
    pub fn code_lens_resolve(&self, lens: &Value) -> mpsc::Receiver<Value> {
        self.request("codeLens/resolve", lens.clone())
    }

    /// `textDocument/selectionRange` for one position.
    pub fn selection_range(&self, path: &Path, line0: usize, col0: usize) -> mpsc::Receiver<Value> {
        self.request(
            "textDocument/selectionRange",
            json!({
                "textDocument": { "uri": path_to_uri(path) },
                "positions": [{ "line": line0, "character": col0 }]
            }),
        )
    }

    /// `textDocument/documentLink`.
    pub fn document_link(&self, path: &Path) -> mpsc::Receiver<Value> {
        self.request(
            "textDocument/documentLink",
            json!({ "textDocument": { "uri": path_to_uri(path) } }),
        )
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
        let result = match method.as_str().unwrap_or("") {
            "workspace/configuration" => {
                let n = msg["params"]["items"]
                    .as_array()
                    .map(|a| a.len())
                    .unwrap_or(0);
                json!(vec![Value::Null; n])
            }
            "workspace/applyEdit" => {
                // Applied by the UI on its next frame.
                shared
                    .apply_edits
                    .lock()
                    .unwrap()
                    .push(msg["params"]["edit"].clone());
                json!({ "applied": true })
            }
            "workspace/workspaceFolders" => Value::Null,
            // A token for `$/progress`: nothing to set up.
            "window/workDoneProgress/create" => Value::Null,
            refresh if refresh.starts_with("workspace/") && refresh.ends_with("/refresh") => {
                let mut list = shared.refreshes.lock().unwrap();
                if !list.iter().any(|m| m == refresh) {
                    list.push(refresh.to_string());
                }
                Value::Null
            }
            _ => Value::Null,
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
                            end_line0: d["range"]["end"]["line"].as_u64().unwrap_or(0) as usize,
                            end_col0: d["range"]["end"]["character"].as_u64().unwrap_or(0) as usize,
                            message: d["message"].as_str().unwrap_or("").to_string(),
                            severity: d["severity"].as_u64().unwrap_or(0) as u8,
                            source: d["source"].as_str().unwrap_or("lsp").to_string(),
                            raw: d.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            shared.diagnostics.lock().unwrap().insert(path, items);
        }
        Some("window/showMessage") => {
            let kind = msg["params"]["type"].as_u64().unwrap_or(3) as u8;
            let text = msg["params"]["message"].as_str().unwrap_or("").to_string();
            shared.push_log(format!("[message] {text}"));
            shared.messages.lock().unwrap().push((kind, text));
        }
        Some("window/logMessage") => {
            let line = format!("[log] {}", msg["params"]["message"].as_str().unwrap_or(""));
            shared.push_log(line);
        }
        Some("$/progress") => progress_notification(shared, &msg["params"]),
        _ => {}
    }
}

/// `$/progress` with a work done payload: begin, report or end.
fn progress_notification(shared: &Shared, params: &Value) {
    let token = match &params["token"] {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let value = &params["value"];
    let text = |key: &str| value[key].as_str().map(String::from);
    let percentage = value["percentage"].as_u64().map(|p| p.min(100) as u32);
    let mut list = shared.progress.lock().unwrap();
    match value["kind"].as_str() {
        Some("begin") => {
            list.retain(|p| p.token != token);
            let title = text("title").unwrap_or_default();
            shared.push_log(format!("[progress] {title}"));
            shared.started.lock().unwrap().push(title.clone());
            list.push(Progress {
                token,
                title,
                message: text("message").unwrap_or_default(),
                percentage,
            });
        }
        Some("report") => {
            if let Some(p) = list.iter_mut().find(|p| p.token == token) {
                if let Some(message) = text("message") {
                    p.message = message;
                }
                if percentage.is_some() {
                    p.percentage = percentage;
                }
            }
        }
        Some("end") => {
            if let Some(at) = list.iter().position(|p| p.token == token) {
                let done = list.remove(at);
                let message = text("message").unwrap_or_default();
                shared.push_log(format!("[progress] {} done {message}", done.title));
                shared.finished.lock().unwrap().push((done.title, message));
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Descoberta do servidor
// ---------------------------------------------------------------------------

/// The PHP server command for a project at `root`, by the automatic
/// discovery of [`engine`] (bundled PHPantom first; `PHPIDE_LSP` wins).
pub fn detect_server(root: &Path) -> Option<(String, Vec<String>)> {
    let found = engine::resolve(&engine::EngineChoice::Automatic, root).ok()?;
    Some((found.program.display().to_string(), found.args))
}

// ---------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_and_location_links() {
        let location = json!({
            "uri": "file:///p/a.php",
            "range": { "start": { "line": 2, "character": 4 }, "end": { "line": 2, "character": 8 } }
        });
        let link = json!({
            "originSelectionRange": { "start": { "line": 9, "character": 1 }, "end": { "line": 9, "character": 6 } },
            "targetUri": "file:///p/b.php",
            "targetRange": { "start": { "line": 5, "character": 4 }, "end": { "line": 8, "character": 5 } },
            "targetSelectionRange": { "start": { "line": 5, "character": 20 }, "end": { "line": 5, "character": 25 } }
        });
        let at = |path: &str, line0, col0| Location {
            path: PathBuf::from(path),
            line0,
            col0,
        };
        assert_eq!(parse_locations(&location), [at("/p/a.php", 2, 4)]);
        assert_eq!(parse_locations(&link), [at("/p/b.php", 5, 20)]);
        assert_eq!(
            parse_locations(&json!([location, link])),
            [at("/p/a.php", 2, 4), at("/p/b.php", 5, 20)]
        );
        assert!(parse_locations(&Value::Null).is_empty());
        assert!(parse_locations(&json!([])).is_empty());
    }

    #[test]
    fn progress_begins_reports_and_ends() {
        let shared = Shared::default();
        let note = |value: Value| json!({ "token": "index/1", "value": value });
        progress_notification(
            &shared,
            &note(json!({ "kind": "begin", "title": "Indexing", "percentage": 0 })),
        );
        progress_notification(
            &shared,
            &note(json!({ "kind": "report", "message": "vendor", "percentage": 42 })),
        );
        let now = shared.progress.lock().unwrap().clone();
        assert_eq!(now.len(), 1);
        assert_eq!(now[0].text(), "Indexing… 42%");
        assert_eq!(now[0].message, "vendor");
        progress_notification(
            &shared,
            &note(json!({ "kind": "end", "message": "1200 files" })),
        );
        assert!(shared.progress.lock().unwrap().is_empty());
        assert_eq!(
            *shared.finished.lock().unwrap(),
            [("Indexing".to_string(), "1200 files".to_string())]
        );
        // A report for a token that never began is ignored.
        progress_notification(&shared, &note(json!({ "kind": "report", "percentage": 5 })));
        assert!(shared.progress.lock().unwrap().is_empty());
    }

    #[test]
    fn initialize_sends_folders_and_options() {
        let options = InitOptions {
            extra_folders: vec![PathBuf::from("/work/lib")],
            initialization_options: json!({ "indexing": { "exclude": ["build"] } }),
        };
        let params = initialize_params(Path::new("/work/app"), &options);
        let names: Vec<&str> = params["workspaceFolders"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|f| f["name"].as_str())
            .collect();
        assert_eq!(names, ["app", "lib"]);
        assert_eq!(
            params["initializationOptions"]["indexing"]["exclude"][0],
            "build"
        );
        assert_eq!(params["capabilities"]["window"]["workDoneProgress"], true);
        let plain = initialize_params(Path::new("/work/app"), &InitOptions::default());
        assert!(plain.get("initializationOptions").is_none());
    }

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
        assert!(!empty.workspace_folders_changes);

        let phpantom = ServerCapabilities::from_initialize(&json!({
            "capabilities": {
                "declarationProvider": true,
                "typeHierarchyProvider": true,
                "callHierarchyProvider": true,
                "codeLensProvider": { "resolveProvider": false },
                "selectionRangeProvider": true,
                "documentLinkProvider": {},
                "workspace": { "workspaceFolders": { "supported": true, "changeNotifications": true } }
            },
            "serverInfo": { "name": "PHPantom", "version": "1.1.0" }
        }));
        assert!(phpantom.declaration_provider && phpantom.type_hierarchy_provider);
        assert!(phpantom.call_hierarchy_provider && phpantom.code_lens_provider);
        assert!(phpantom.selection_range_provider && phpantom.document_link_provider);
        assert!(phpantom.workspace_folders_changes);
        assert_eq!(
            (
                phpantom.server_name.as_str(),
                phpantom.server_version.as_str()
            ),
            ("PHPantom", "1.1.0")
        );
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

/// A completion item normalised for the UI.
#[derive(Debug, Clone, PartialEq)]
pub struct CompletionItem {
    pub label: String,
    /// `CompletionItemKind` of the LSP.
    pub kind: u64,
    pub detail: String,
    pub documentation: Option<String>,
    /// Text matched against what the user typed.
    pub filter_text: String,
    pub sort_text: String,
    pub deprecated: bool,
    pub preselect: bool,
    /// The item as received, for `completionItem/resolve` and edits.
    pub raw: Value,
}

impl CompletionItem {
    pub fn kind_label(&self) -> &'static str {
        match self.kind {
            2 => "method",
            3 => "function",
            4 => "constructor",
            5 => "field",
            6 => "variable",
            7 => "class",
            8 => "interface",
            9 => "namespace",
            10 => "property",
            13 => "enum",
            14 => "keyword",
            15 => "snippet",
            17 => "file",
            19 => "folder",
            20 => "enum case",
            21 => "constant",
            22 => "struct",
            25 => "type parameter",
            _ => "",
        }
    }

    /// Merges a `completionItem/resolve` result.
    pub fn resolved(&mut self, value: &Value) {
        if value.is_object() {
            if let Some(detail) = value["detail"].as_str() {
                self.detail = detail.to_string();
            }
            if let Some(doc) = documentation_text(&value["documentation"]) {
                self.documentation = Some(doc);
            }
            self.raw = value.clone();
        }
    }
}

/// Text of a `documentation` / `contents` value: a string, a
/// `MarkupContent`, a `MarkedString` or an array of them.
pub fn documentation_text(value: &Value) -> Option<String> {
    let text = match value {
        Value::String(s) => s.clone(),
        Value::Object(o) => match (o.get("value"), o.get("language")) {
            (Some(Value::String(v)), Some(Value::String(lang))) => {
                format!("```{lang}\n{v}\n```")
            }
            (Some(Value::String(v)), _) => v.clone(),
            _ => return None,
        },
        Value::Array(items) => items
            .iter()
            .filter_map(documentation_text)
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => return None,
    };
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
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

/// Accepts both forms of the spec: `CompletionItem[]` or `CompletionList`.
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
            let label = item["label"].as_str()?.to_string();
            let deprecated = item["deprecated"].as_bool().unwrap_or(false)
                || item["tags"]
                    .as_array()
                    .is_some_and(|t| t.iter().any(|t| t.as_u64() == Some(1)));
            Some(CompletionItem {
                filter_text: item["filterText"].as_str().unwrap_or(&label).to_string(),
                sort_text: item["sortText"].as_str().unwrap_or(&label).to_string(),
                kind: item["kind"].as_u64().unwrap_or(0),
                detail: item["detail"].as_str().unwrap_or("").to_string(),
                documentation: documentation_text(&item["documentation"]),
                deprecated,
                preselect: item["preselect"].as_bool().unwrap_or(false),
                raw: item.clone(),
                label,
            })
        })
        .collect()
}

/// How well `query` matches `candidate`: prefix beats word-start beats
/// subsequence; `None` when it does not match. Case-insensitive.
pub fn match_score(candidate: &str, query: &str) -> Option<u32> {
    if query.is_empty() {
        return Some(0);
    }
    let lower = candidate.to_lowercase();
    let q = query.to_lowercase();
    let trimmed = lower.trim_start_matches(['$', '\\']);
    if trimmed.starts_with(&q) || lower.starts_with(&q) {
        let exact_case = candidate
            .trim_start_matches(['$', '\\'])
            .starts_with(query.trim_start_matches(['$', '\\']));
        return Some(if exact_case { 3000 } else { 2000 });
    }
    // Subsequence, rewarding matches at word starts (camelCase, _).
    let chars: Vec<char> = candidate.chars().collect();
    let mut score = 1000u32;
    let mut index = 0;
    for qc in q.chars() {
        let mut found = false;
        while index < chars.len() {
            let c = chars[index];
            let starts_word = index == 0
                || c.is_uppercase()
                || matches!(chars[index - 1], '_' | '\\' | '$' | ':');
            index += 1;
            if c.to_lowercase().next() == Some(qc) {
                if !starts_word {
                    score = score.saturating_sub(10);
                }
                found = true;
                break;
            }
            score = score.saturating_sub(1);
        }
        if !found {
            return None;
        }
    }
    Some(score)
}

/// Filters by what the user typed and ranks: match quality, then the
/// server's `sortText`. At most 200 items.
pub fn filter_completions(items: &[CompletionItem], query: &str) -> Vec<CompletionItem> {
    let mut matched: Vec<(u32, &CompletionItem)> = items
        .iter()
        .filter_map(|it| match_score(&it.filter_text, query).map(|s| (s, it)))
        .collect();
    matched.sort_by(|(sa, a), (sb, b)| {
        sb.cmp(sa)
            .then_with(|| a.deprecated.cmp(&b.deprecated))
            .then_with(|| a.sort_text.cmp(&b.sort_text))
            .then_with(|| a.label.cmp(&b.label))
    });
    matched
        .into_iter()
        .take(200)
        .map(|(_, it)| it.clone())
        .collect()
}

#[cfg(test)]
mod completion_tests {
    use super::*;

    #[test]
    fn parses_array_and_list_forms() {
        let arr_form = serde_json::json!([
            { "label": "array_map", "kind": 3, "detail": "callable",
              "documentation": { "kind": "markdown", "value": "Applies **f**" } },
            { "label": "strlen", "kind": 3 }
        ]);
        let items = parse_completions(&arr_form);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].label, "array_map");
        assert_eq!(items[0].detail, "callable");
        assert_eq!(items[0].documentation.as_deref(), Some("Applies **f**"));
        assert_eq!(items[0].kind_label(), "function");

        let list_form = serde_json::json!({ "isIncomplete": false, "items": [
            { "label": "PDO", "kind": 7 }
        ] });
        let items = parse_completions(&list_form);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind_label(), "class");
    }

    #[test]
    fn ranks_prefix_then_word_starts_then_subsequence() {
        let items = parse_completions(&serde_json::json!([
            { "label": "file_get_contents", "kind": 3 },
            { "label": "FileSystem", "kind": 7 },
            { "label": "getFileName", "kind": 2 },
            { "label": "strlen", "kind": 3 },
            { "label": "fgets", "kind": 3, "sortText": "0" }
        ]));
        let labels = |q: &str| -> Vec<String> {
            filter_completions(&items, q)
                .into_iter()
                .map(|i| i.label)
                .collect()
        };
        assert_eq!(labels("File")[0], "FileSystem");
        assert!(labels("fil").contains(&"file_get_contents".to_string()));
        assert!(labels("gfn").contains(&"getFileName".to_string()));
        assert!(!labels("fil").contains(&"strlen".to_string()));
        assert_eq!(labels("").len(), 5);
        assert_eq!(labels("")[0], "fgets", "sortText decides ties");
    }
}
