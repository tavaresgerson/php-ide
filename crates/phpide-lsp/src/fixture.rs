//! A small PHP language server for tests.
//!
//! It speaks real LSP over a pair of byte streams, so the client, the sync
//! and the UI are exercised exactly as with intelephense or phpactor. The
//! "intelligence" is a line-based indexer that understands one-line
//! signatures, classes, docblocks and variables, which is enough for the
//! fixtures the tests write.
//!
//! [`start`] runs it on a thread and returns a connected [`LspClient`]. The
//! custom request `fixture/state` returns the server's copy of a document and
//! how many full and ranged changes it received, so tests can check the sync.

use std::collections::HashMap;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use serde_json::{json, Value};

use crate::{encode_frame, path_to_uri, read_frame, uri_to_path, LspClient};

/// Starts the fixture server for `root` on a thread and connects a client.
pub fn start(root: &Path) -> std::io::Result<(LspClient, mpsc::Receiver<Value>)> {
    let (server_in, client_out) = std::io::pipe()?;
    let (client_in, server_out) = std::io::pipe()?;
    std::thread::Builder::new()
        .name("fixture-lsp".into())
        .spawn(move || run(server_in, server_out))?;
    Ok(LspClient::connect(
        Box::new(client_out),
        Box::new(client_in),
        None,
        None,
        root,
    ))
}

/// Serves until `exit` or the end of `input`.
pub fn run(input: impl Read, mut output: impl Write) {
    let mut reader = BufReader::new(input);
    let mut server = Server::default();
    while let Ok(Some(body)) = read_frame(&mut reader) {
        let Ok(msg) = serde_json::from_str::<Value>(&body) else {
            continue;
        };
        let method = msg["method"].as_str().unwrap_or("").to_string();
        if method == "exit" {
            break;
        }
        let replies = server.handle(&method, &msg["params"], msg.get("id"));
        for reply in replies {
            if output.write_all(&encode_frame(&reply.to_string())).is_err() {
                return;
            }
        }
        let _ = output.flush();
    }
}

/// (URI, line, UTF-16 column) of a word.
type Occurrence = (String, usize, usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Class,
    Interface,
    Function,
    Method,
}

#[derive(Debug, Clone)]
struct Symbol {
    name: String,
    kind: Kind,
    container: Option<String>,
    params: Vec<String>,
    returns: Option<String>,
    doc: Option<String>,
    uri: String,
    line: usize,
    col: usize,
    /// Last line of the declaration (classes: the closing brace).
    end_line: usize,
}

impl Symbol {
    fn signature(&self) -> String {
        match self.kind {
            Kind::Class => format!("class {}", self.name),
            Kind::Interface => format!("interface {}", self.name),
            Kind::Function | Kind::Method => {
                let returns = self
                    .returns
                    .as_ref()
                    .map(|r| format!(": {r}"))
                    .unwrap_or_default();
                format!(
                    "function {}({}){returns}",
                    self.name,
                    self.params.join(", ")
                )
            }
        }
    }

    fn markdown(&self) -> String {
        let mut text = String::new();
        if let Some(container) = &self.container {
            text.push_str(&format!("`{container}`\n\n"));
        }
        text.push_str(&format!("```php\n<?php\n{}\n```", self.signature()));
        if let Some(doc) = &self.doc {
            text.push_str("\n\n");
            text.push_str(doc);
        }
        text
    }

    fn location(&self) -> Value {
        json!({
            "uri": self.uri,
            "range": crate::lsp_range((self.line, self.col), (self.line, self.col + utf16_len(&self.name)))
        })
    }

    fn lsp_kind(&self) -> u64 {
        match self.kind {
            Kind::Class => 5,
            Kind::Interface => 11,
            Kind::Function => 12,
            Kind::Method => 6,
        }
    }
}

const KEYWORDS: &[&str] = &[
    "if",
    "elseif",
    "while",
    "for",
    "foreach",
    "switch",
    "match",
    "array",
    "isset",
    "empty",
    "list",
    "return",
    "echo",
    "print",
    "fn",
    "function",
    "catch",
    "unset",
    "exit",
    "die",
    "include",
    "require",
    "require_once",
    "include_once",
    "declare",
    "use",
    "new",
    "and",
    "or",
];
const BUILTINS: &[&str] = &[
    "strlen",
    "count",
    "array_map",
    "array_filter",
    "sprintf",
    "printf",
    "implode",
    "explode",
    "str_replace",
    "trim",
    "in_array",
    "json_encode",
    "var_dump",
    "is_null",
    "intval",
    "max",
    "min",
    "compact",
    "view",
    "route",
    "config",
    "env",
    "now",
    "response",
    "redirect",
];

#[derive(Default)]
struct Server {
    root: PathBuf,
    /// Open documents by URI.
    open: HashMap<String, String>,
    /// Files of the workspace on disk by URI.
    disk: HashMap<String, String>,
    full_changes: usize,
    ranged_changes: usize,
}

impl Server {
    fn handle(&mut self, method: &str, params: &Value, id: Option<&Value>) -> Vec<Value> {
        let mut out = Vec::new();
        let result = match method {
            "initialize" => {
                if let Some(root) = params["rootUri"].as_str().and_then(uri_to_path) {
                    self.root = root;
                    self.scan();
                }
                capabilities()
            }
            "shutdown" => Value::Null,
            "textDocument/didOpen" => {
                let uri = params["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let text = params["textDocument"]["text"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                self.open.insert(uri.clone(), text);
                out.push(self.diagnostics(&uri));
                return out;
            }
            "textDocument/didChange" => {
                let uri = params["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let changes = params["contentChanges"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                for change in &changes {
                    if change.get("range").is_some() {
                        self.ranged_changes += 1;
                    } else {
                        self.full_changes += 1;
                    }
                }
                let text = self.open.get(&uri).cloned().unwrap_or_default();
                self.open.insert(
                    uri.clone(),
                    crate::sync::apply_content_changes(&text, &changes),
                );
                out.push(self.diagnostics(&uri));
                return out;
            }
            "textDocument/didSave" => return out,
            "textDocument/didClose" => {
                let uri = params["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                self.open.remove(&uri);
                out.push(json!({
                    "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics",
                    "params": { "uri": uri, "diagnostics": [] }
                }));
                return out;
            }
            "fixture/state" => {
                let uri = params["uri"].as_str().unwrap_or("");
                json!({
                    "text": self.text(uri),
                    "fullChanges": self.full_changes,
                    "rangedChanges": self.ranged_changes,
                })
            }
            "textDocument/completion" => self.completion(params),
            "completionItem/resolve" => self.resolve(params),
            "textDocument/hover" => self.hover(params),
            "textDocument/signatureHelp" => self.signature_help(params),
            "textDocument/definition" => self.definition(params),
            "textDocument/references" => self.references(params),
            "textDocument/prepareRename" => self.prepare_rename(params),
            "textDocument/rename" => self.rename(params),
            "textDocument/documentSymbol" => self.document_symbols(params),
            "workspace/symbol" => self.workspace_symbols(params),
            "textDocument/codeAction" => self.code_actions(params),
            "workspace/executeCommand" => {
                if params["command"] == "fixture.addHeader" {
                    let uri = params["arguments"][0].as_str().unwrap_or("").to_string();
                    out.push(json!({
                        "jsonrpc": "2.0", "id": "fixture-apply-1", "method": "workspace/applyEdit",
                        "params": { "label": "Add header", "edit": { "changes": { uri: [{
                            "range": crate::lsp_range((1, 0), (1, 0)),
                            "newText": "// Generated by the fixture server.\n"
                        }] } } }
                    }));
                }
                Value::Null
            }
            "textDocument/formatting" => self.formatting(params),
            _ => {
                if id.is_none() {
                    return out;
                }
                Value::Null
            }
        };
        if let Some(id) = id {
            // Responses to our own requests (applyEdit) come back with an id too.
            if params.is_null() && method.is_empty() {
                return out;
            }
            out.insert(0, json!({ "jsonrpc": "2.0", "id": id, "result": result }));
        }
        out
    }

    fn scan(&mut self) {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
            if depth > 8 {
                return;
            }
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if !path.file_name().is_some_and(|n| n == ".git") {
                        walk(&path, out, depth + 1);
                    }
                } else if path.extension().is_some_and(|e| e == "php") {
                    out.push(path);
                }
            }
        }
        let mut files = Vec::new();
        walk(&self.root.clone(), &mut files, 0);
        for file in files {
            if let Ok(text) = std::fs::read_to_string(&file) {
                self.disk.insert(path_to_uri(&file), text);
            }
        }
    }

    fn text(&self, uri: &str) -> String {
        self.open
            .get(uri)
            .or_else(|| self.disk.get(uri))
            .cloned()
            .unwrap_or_default()
    }

    /// Every document: open copies win over the disk.
    fn documents(&self) -> Vec<(String, String)> {
        let mut docs: HashMap<String, String> = self.disk.clone();
        for (uri, text) in &self.open {
            docs.insert(uri.clone(), text.clone());
        }
        let mut docs: Vec<_> = docs.into_iter().collect();
        docs.sort();
        docs
    }

    fn symbols(&self) -> Vec<Symbol> {
        self.documents()
            .iter()
            .flat_map(|(uri, text)| index(uri, text))
            .collect()
    }

    fn diagnostics(&self, uri: &str) -> Value {
        let text = self.text(uri);
        let functions: Vec<String> = self
            .symbols()
            .into_iter()
            .filter(|s| s.kind == Kind::Function)
            .map(|s| s.name)
            .collect();
        let mut diagnostics = Vec::new();
        for (line_no, line) in text.lines().enumerate() {
            let code = strip_comment(line);
            let chars: Vec<char> = code.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                if is_ident_start(chars[i]) && (i == 0 || !is_ident(chars[i - 1])) {
                    let start = i;
                    while i < chars.len() && is_ident(chars[i]) {
                        i += 1;
                    }
                    let name: String = chars[start..i].iter().collect();
                    let before: String = chars[..start].iter().collect();
                    let before = before.trim_end();
                    let called = chars.get(i) == Some(&'(');
                    let member = before.ends_with("->")
                        || before.ends_with("::")
                        || before.ends_with('$')
                        || before.ends_with("function")
                        || before.ends_with("new")
                        || before.ends_with('\\');
                    if called
                        && !member
                        && !KEYWORDS.contains(&name.to_lowercase().as_str())
                        && !BUILTINS.contains(&name.as_str())
                        && !functions.contains(&name)
                    {
                        let col = utf16_len(&chars[..start].iter().collect::<String>());
                        diagnostics.push(json!({
                            "range": crate::lsp_range((line_no, col), (line_no, col + utf16_len(&name))),
                            "severity": 1,
                            "code": "undefined-function",
                            "source": "fixture",
                            "message": format!("Undefined function '{name}'"),
                            "data": { "name": name },
                        }));
                    }
                } else {
                    i += 1;
                }
            }
        }
        json!({
            "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics",
            "params": { "uri": uri, "diagnostics": diagnostics }
        })
    }

    fn completion(&self, params: &Value) -> Value {
        let (uri, line, col) = position(params);
        let text = self.text(&uri);
        let line_text = text.lines().nth(line).unwrap_or("");
        let upto: String = take_utf16(line_text, col);
        let word_start = upto
            .char_indices()
            .rev()
            .take_while(|(_, c)| is_ident(*c) || *c == '$')
            .last()
            .map(|(i, _)| i)
            .unwrap_or(upto.len());
        let before = upto[..word_start].trim_end();
        let typed = &upto[word_start..];
        let mut items = Vec::new();
        if typed.starts_with('$') {
            let mut seen = Vec::new();
            for var in variables(&text) {
                if !seen.contains(&var) && var != typed {
                    items.push(json!({ "label": var, "kind": 6 }));
                    seen.push(var);
                }
            }
        } else if before.ends_with("->") || before.ends_with("::") {
            for symbol in self.symbols().iter().filter(|s| s.kind == Kind::Method) {
                items.push(callable_item(symbol, 2));
            }
        } else {
            for symbol in self.symbols() {
                match symbol.kind {
                    Kind::Function => items.push(callable_item(&symbol, 3)),
                    Kind::Class | Kind::Interface => items.push(json!({
                        "label": symbol.name, "kind": if symbol.kind == Kind::Class { 7 } else { 8 },
                        "detail": symbol.signature(),
                        "data": { "name": symbol.name, "container": Value::Null }
                    })),
                    Kind::Method => {}
                }
            }
            for keyword in ["foreach", "function", "return", "public", "private"] {
                items.push(json!({ "label": keyword, "kind": 14 }));
            }
        }
        json!({ "isIncomplete": false, "items": items })
    }

    fn resolve(&self, item: &Value) -> Value {
        let mut item = item.clone();
        let name = item["data"]["name"].as_str().unwrap_or("");
        let container = item["data"]["container"].as_str();
        if let Some(symbol) = self
            .symbols()
            .into_iter()
            .find(|s| s.name == name && s.container.as_deref() == container)
        {
            item["documentation"] = json!({ "kind": "markdown", "value": symbol.markdown() });
        }
        item
    }

    /// The symbol named by the word at the position, preferring methods
    /// after `->`.
    fn symbol_at(&self, params: &Value) -> Option<(Symbol, (usize, usize, usize))> {
        let (uri, line, col) = position(params);
        let text = self.text(&uri);
        let line_text = text.lines().nth(line)?;
        let (word, start, end) = word_at(line_text, col)?;
        let before = take_utf16(line_text, start);
        let member = before.trim_end().ends_with("->") || before.trim_end().ends_with("::");
        let mut candidates: Vec<Symbol> = self
            .symbols()
            .into_iter()
            .filter(|s| s.name == word)
            .collect();
        candidates.sort_by_key(|s| (member != (s.kind == Kind::Method)) as u8);
        candidates
            .into_iter()
            .next()
            .map(|s| (s, (line, start, end)))
    }

    fn hover(&self, params: &Value) -> Value {
        match self.symbol_at(params) {
            Some((symbol, (line, start, end))) => json!({
                "contents": { "kind": "markdown", "value": symbol.markdown() },
                "range": crate::lsp_range((line, start), (line, end)),
            }),
            None => Value::Null,
        }
    }

    fn signature_help(&self, params: &Value) -> Value {
        let (uri, line, col) = position(params);
        let text = self.text(&uri);
        let Some(line_text) = text.lines().nth(line) else {
            return Value::Null;
        };
        let upto: Vec<char> = take_utf16(line_text, col).chars().collect();
        // Innermost unclosed parenthesis and the commas after it.
        let mut depth = 0i32;
        let mut commas = 0;
        let mut open = None;
        for i in (0..upto.len()).rev() {
            match upto[i] {
                ')' => depth += 1,
                '(' if depth == 0 => {
                    open = Some(i);
                    break;
                }
                '(' => depth -= 1,
                ',' if depth == 0 => commas += 1,
                _ => {}
            }
        }
        let Some(open) = open else {
            return Value::Null;
        };
        let mut start = open;
        while start > 0 && is_ident(upto[start - 1]) {
            start -= 1;
        }
        let name: String = upto[start..open].iter().collect();
        let Some(symbol) = self
            .symbols()
            .into_iter()
            .find(|s| s.name == name && matches!(s.kind, Kind::Function | Kind::Method))
        else {
            return Value::Null;
        };
        let label = symbol.signature();
        let mut parameters = Vec::new();
        let mut offset = utf16_len(&format!("function {}(", symbol.name));
        for param in &symbol.params {
            let len = utf16_len(param);
            parameters.push(json!({ "label": [offset, offset + len] }));
            offset += len + 2;
        }
        json!({
            "signatures": [{
                "label": label,
                "documentation": symbol.doc.as_ref().map(|d| json!({ "kind": "markdown", "value": d })),
                "parameters": parameters,
            }],
            "activeSignature": 0,
            "activeParameter": commas.min(symbol.params.len().saturating_sub(1)),
        })
    }

    fn definition(&self, params: &Value) -> Value {
        match self.symbol_at(params) {
            Some((symbol, _)) => json!([symbol.location()]),
            None => Value::Null,
        }
    }

    /// Every occurrence of the word at the position: variables in their
    /// document, other names in the whole workspace.
    fn occurrences(&self, params: &Value) -> Option<(String, Vec<Occurrence>)> {
        let (uri, line, col) = position(params);
        let text = self.text(&uri);
        let (word, _, _) = word_at(text.lines().nth(line)?, col)?;
        let docs = if word.starts_with('$') {
            vec![(uri.clone(), text)]
        } else {
            self.documents()
        };
        let mut found = Vec::new();
        for (doc_uri, doc_text) in docs {
            for (line_no, line_text) in doc_text.lines().enumerate() {
                for start in find_word(line_text, &word) {
                    found.push((doc_uri.clone(), line_no, start));
                }
            }
        }
        Some((word, found))
    }

    fn references(&self, params: &Value) -> Value {
        let Some((word, found)) = self.occurrences(params) else {
            return Value::Null;
        };
        let len = utf16_len(&word);
        Value::Array(
            found
                .into_iter()
                .map(|(uri, line, start)| {
                    json!({ "uri": uri, "range": crate::lsp_range((line, start), (line, start + len)) })
                })
                .collect(),
        )
    }

    fn prepare_rename(&self, params: &Value) -> Value {
        let (uri, line, col) = position(params);
        let text = self.text(&uri);
        let Some((word, start, end)) = text.lines().nth(line).and_then(|l| word_at(l, col)) else {
            return Value::Null;
        };
        let known = word.starts_with('$') || self.symbols().iter().any(|s| s.name == word);
        if !known || KEYWORDS.contains(&word.as_str()) {
            return Value::Null;
        }
        json!({ "range": crate::lsp_range((line, start), (line, end)), "placeholder": word })
    }

    fn rename(&self, params: &Value) -> Value {
        let Some((word, found)) = self.occurrences(params) else {
            return Value::Null;
        };
        let new_name = params["newName"].as_str().unwrap_or("");
        let len = utf16_len(&word);
        let mut changes: HashMap<String, Vec<Value>> = HashMap::new();
        for (uri, line, start) in found {
            changes.entry(uri).or_default().push(json!({
                "range": crate::lsp_range((line, start), (line, start + len)),
                "newText": new_name,
            }));
        }
        json!({ "changes": changes })
    }

    fn document_symbols(&self, params: &Value) -> Value {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
        let symbols = index(uri, &self.text(uri));
        let to_json = |s: &Symbol, children: Vec<Value>| {
            let range = crate::lsp_range((s.line, 0), (s.end_line, 0));
            let selection = crate::lsp_range((s.line, s.col), (s.line, s.col + utf16_len(&s.name)));
            json!({
                "name": s.name, "kind": s.lsp_kind(), "detail": s.signature(),
                "range": range, "selectionRange": selection, "children": children
            })
        };
        let mut out = Vec::new();
        for s in symbols.iter().filter(|s| s.container.is_none()) {
            let children = symbols
                .iter()
                .filter(|m| m.container.as_deref() == Some(s.name.as_str()))
                .map(|m| to_json(m, Vec::new()))
                .collect();
            out.push(to_json(s, children));
        }
        Value::Array(out)
    }

    fn workspace_symbols(&self, params: &Value) -> Value {
        let query = params["query"].as_str().unwrap_or("").to_lowercase();
        Value::Array(
            self.symbols()
                .into_iter()
                .filter(|s| s.name.to_lowercase().contains(&query))
                .map(|s| {
                    json!({
                        "name": s.name, "kind": s.lsp_kind(), "location": s.location(),
                        "containerName": s.container.clone().unwrap_or_default()
                    })
                })
                .collect(),
        )
    }

    fn code_actions(&self, params: &Value) -> Value {
        let uri = params["textDocument"]["uri"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let text = self.text(&uri);
        let end_line = text.lines().count();
        let mut actions = Vec::new();
        for diagnostic in params["context"]["diagnostics"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if diagnostic["code"] != "undefined-function" {
                continue;
            }
            let name = diagnostic["data"]["name"].as_str().unwrap_or("missing");
            actions.push(json!({
                "title": format!("Create function '{name}'"),
                "kind": "quickfix",
                "isPreferred": true,
                "diagnostics": [diagnostic],
                "edit": { "changes": { uri.clone(): [{
                    "range": crate::lsp_range((end_line, 0), (end_line, 0)),
                    "newText": format!("\nfunction {name}()\n{{\n}}\n"),
                }] } }
            }));
        }
        actions.push(json!({
            "title": "Add header comment",
            "kind": "source",
            "command": { "title": "Add header comment", "command": "fixture.addHeader", "arguments": [uri] }
        }));
        Value::Array(actions)
    }

    fn formatting(&self, params: &Value) -> Value {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
        let tab = " ".repeat(params["options"]["tabSize"].as_u64().unwrap_or(4) as usize);
        let text = self.text(uri);
        let mut edits = Vec::new();
        for (line_no, line) in text.lines().enumerate() {
            let indent: String = line
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            let formatted = format!("{}{}", indent.replace('\t', &tab), line.trim());
            if formatted != line {
                edits.push(json!({
                    "range": crate::lsp_range((line_no, 0), (line_no, utf16_len(line))),
                    "newText": formatted,
                }));
            }
        }
        Value::Array(edits)
    }
}

fn capabilities() -> Value {
    json!({
        "capabilities": {
            "textDocumentSync": { "openClose": true, "change": 2, "save": { "includeText": false } },
            "completionProvider": { "resolveProvider": true, "triggerCharacters": ["$", ">", ":"] },
            "hoverProvider": true,
            "signatureHelpProvider": { "triggerCharacters": ["(", ","], "retriggerCharacters": [")"] },
            "definitionProvider": true,
            "referencesProvider": true,
            "renameProvider": { "prepareProvider": true },
            "documentSymbolProvider": true,
            "workspaceSymbolProvider": true,
            "codeActionProvider": { "codeActionKinds": ["quickfix", "source"] },
            "documentFormattingProvider": true,
            "executeCommandProvider": { "commands": ["fixture.addHeader"] }
        },
        "serverInfo": { "name": "fixture-php", "version": "1" }
    })
}

fn callable_item(symbol: &Symbol, kind: u64) -> Value {
    let args: Vec<String> = symbol
        .params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let name = p
                .split_whitespace()
                .last()
                .unwrap_or("$arg")
                .replace('$', "\\$");
            format!("${{{}:{}}}", i + 1, name)
        })
        .collect();
    json!({
        "label": symbol.name,
        "kind": kind,
        "detail": symbol.signature(),
        "insertText": format!("{}({})$0", symbol.name, args.join(", ")),
        "insertTextFormat": 2,
        "data": { "name": symbol.name, "container": symbol.container },
    })
}

/// Declarations of a document, one-line signatures only.
fn index(uri: &str, text: &str) -> Vec<Symbol> {
    let mut symbols: Vec<Symbol> = Vec::new();
    let mut depth = 0i32;
    // (class name, depth of its body, index in `symbols`)
    let mut class: Option<(String, i32, usize)> = None;
    let mut doc: Vec<String> = Vec::new();
    let mut in_doc = false;
    for (line_no, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.starts_with("/**") {
            in_doc = true;
            doc.clear();
        }
        if in_doc {
            let content = line
                .trim_start_matches("/**")
                .trim_end_matches("*/")
                .trim_start_matches('*')
                .trim();
            if !content.is_empty() {
                doc.push(content.to_string());
            }
            if line.ends_with("*/") {
                in_doc = false;
            }
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        let declared = |keyword: &str| -> Option<String> {
            let at = words.iter().position(|w| *w == keyword)?;
            let name: String = words
                .get(at + 1)?
                .chars()
                .take_while(|c| is_ident(*c))
                .collect();
            (!name.is_empty()).then_some(name)
        };
        let col_of = |name: &str| -> usize { find_word(raw, name).into_iter().next().unwrap_or(0) };
        let take_doc = |doc: &mut Vec<String>| -> Option<String> {
            let text = doc.join("\n");
            doc.clear();
            (!text.is_empty()).then_some(text)
        };
        if let Some(name) = declared("class").or_else(|| declared("trait")) {
            symbols.push(Symbol {
                col: col_of(&name),
                name: name.clone(),
                kind: Kind::Class,
                container: None,
                params: Vec::new(),
                returns: None,
                doc: take_doc(&mut doc),
                uri: uri.to_string(),
                line: line_no,
                end_line: line_no,
            });
            class = Some((name, depth + 1, symbols.len() - 1));
        } else if let Some(name) = declared("interface") {
            symbols.push(Symbol {
                col: col_of(&name),
                name: name.clone(),
                kind: Kind::Interface,
                container: None,
                params: Vec::new(),
                returns: None,
                doc: take_doc(&mut doc),
                uri: uri.to_string(),
                line: line_no,
                end_line: line_no,
            });
            class = Some((name, depth + 1, symbols.len() - 1));
        } else if let Some(name) = declared("function") {
            let open = line.find('(');
            let close = line.rfind(')');
            let params: Vec<String> = match (open, close) {
                (Some(o), Some(c)) if c > o => line[o + 1..c]
                    .split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect(),
                _ => Vec::new(),
            };
            let returns = close.and_then(|c| {
                let rest = line[c + 1..].trim();
                rest.strip_prefix(':').map(|r| {
                    r.trim()
                        .trim_end_matches('{')
                        .trim_end_matches(';')
                        .trim()
                        .to_string()
                })
            });
            let container = class
                .as_ref()
                .filter(|(_, body, _)| depth >= *body)
                .map(|(n, _, _)| n.clone());
            symbols.push(Symbol {
                col: col_of(&name),
                kind: if container.is_some() {
                    Kind::Method
                } else {
                    Kind::Function
                },
                name,
                container,
                params,
                returns,
                doc: take_doc(&mut doc),
                uri: uri.to_string(),
                line: line_no,
                end_line: line_no,
            });
        } else if !line.is_empty() && !line.starts_with("#[") {
            doc.clear();
        }
        for c in strip_comment(raw).chars() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if let Some((_, body, index)) = &class {
                        if depth < *body {
                            symbols[*index].end_line = line_no;
                            class = None;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    symbols
}

fn variables(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '$' && chars.get(i + 1).is_some_and(|c| is_ident_start(*c)) {
                let start = i;
                i += 1;
                while i < chars.len() && is_ident(chars[i]) {
                    i += 1;
                }
                out.push(chars[start..i].iter().collect());
            } else {
                i += 1;
            }
        }
    }
    out
}

fn position(params: &Value) -> (String, usize, usize) {
    (
        params["textDocument"]["uri"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        params["position"]["line"].as_u64().unwrap_or(0) as usize,
        params["position"]["character"].as_u64().unwrap_or(0) as usize,
    )
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// The prefix of `line` up to a UTF-16 column.
fn take_utf16(line: &str, col: usize) -> String {
    let mut units = 0;
    line.chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= col
        })
        .collect()
}

/// The identifier (or `$variable`) touching a UTF-16 column, with its UTF-16
/// start and end.
fn word_at(line: &str, col: usize) -> Option<(String, usize, usize)> {
    let chars: Vec<char> = line.chars().collect();
    let mut index = take_utf16(line, col).chars().count();
    if index > 0 && (index >= chars.len() || !is_ident(chars[index])) && is_ident(chars[index - 1])
    {
        index -= 1;
    }
    if index >= chars.len() || !(is_ident(chars[index]) || chars[index] == '$') {
        return None;
    }
    let mut start = index;
    while start > 0 && is_ident(chars[start - 1]) {
        start -= 1;
    }
    if start > 0 && chars[start - 1] == '$' {
        start -= 1;
    }
    let mut end = index + 1;
    while end < chars.len() && is_ident(chars[end]) {
        end += 1;
    }
    let word: String = chars[start..end].iter().collect();
    if word == "$" {
        return None;
    }
    let prefix: String = chars[..start].iter().collect();
    let start16 = utf16_len(&prefix);
    Some((word.clone(), start16, start16 + utf16_len(&word)))
}

/// UTF-16 columns where `word` occurs as a whole word in `line`.
fn find_word(line: &str, word: &str) -> Vec<usize> {
    let chars: Vec<char> = line.chars().collect();
    let target: Vec<char> = word.chars().collect();
    let mut out = Vec::new();
    if target.is_empty() {
        return out;
    }
    let mut i = 0;
    while i + target.len() <= chars.len() {
        let before_ok =
            i == 0 || !(is_ident(chars[i - 1]) || (target[0] != '$' && chars[i - 1] == '$'));
        let after_ok = chars.get(i + target.len()).is_none_or(|c| !is_ident(*c));
        if chars[i..i + target.len()] == target[..] && before_ok && after_ok {
            out.push(utf16_len(&chars[..i].iter().collect::<String>()));
            i += target.len();
        } else {
            i += 1;
        }
    }
    out
}

fn strip_comment(line: &str) -> &str {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") || trimmed.starts_with('*') || trimmed.starts_with("/*") {
        return "";
    }
    match line.find("//") {
        Some(at) => &line[..at],
        None => line,
    }
}

/// Reads one `fixture/state` answer synchronously (test helper).
pub fn state(client: &LspClient, path: &Path) -> Option<Value> {
    client
        .request("fixture/state", json!({ "uri": path_to_uri(path) }))
        .recv_timeout(std::time::Duration::from_secs(5))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const APP: &str = "<?php\n\nclass Greeter\n{\n    /** Says hello to someone. */\n    public function greet(string $name, int $times): string\n    {\n        return str_repeat(\"Hi $name\", $times);\n    }\n}\n\n/** Adds two numbers. */\nfunction add(int $a, int $b): int\n{\n    return $a + $b;\n}\n\n$greeter = new Greeter();\necho $greeter->greet('Ana', add(1, 2));\nmissing_call();\n";

    fn recv(rx: mpsc::Receiver<Value>) -> Value {
        rx.recv_timeout(Duration::from_secs(5)).expect("answer")
    }

    #[test]
    fn serves_the_php_features_over_real_lsp() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("app.php");
        std::fs::write(&file, APP).unwrap();
        let (client, init) = start(dir.path()).unwrap();
        let init = recv(init);
        client.register_capabilities(&init);
        let caps = client.capabilities();
        assert_eq!(caps.sync, crate::SyncKind::Incremental);
        assert!(caps.rename_provider && caps.prepare_rename && caps.signature_help_provider);
        client.did_open(&file, APP, 1);

        // Diagnostics: str_repeat is not in the builtin list on purpose.
        let mut published = None;
        for _ in 0..50 {
            if let Some(d) = client.shared.diagnostics.lock().unwrap().get(&file) {
                published = Some(d.clone());
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let messages: Vec<String> = published.unwrap().into_iter().map(|d| d.message).collect();
        assert!(messages.contains(&"Undefined function 'missing_call'".to_string()));

        let hover = recv(client.hover(&file, 18, 18));
        assert!(hover["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("Says hello"));

        let help = recv(client.signature_help(&file, 18, 30, Some(','), false));
        assert_eq!(
            help["signatures"][0]["label"],
            "function greet(string $name, int $times): string"
        );
        assert_eq!(help["activeParameter"], 1);

        let definition = recv(client.definition(&file, 18, 29));
        assert_eq!(definition[0]["range"]["start"]["line"], 12);

        let rename = recv(client.rename(&file, 18, 18, "welcome"));
        assert_eq!(
            rename["changes"][path_to_uri(&file)]
                .as_array()
                .unwrap()
                .len(),
            2
        );

        let symbols = recv(client.document_symbol(&file));
        assert_eq!(symbols[0]["name"], "Greeter");
        assert_eq!(symbols[0]["children"][0]["name"], "greet");

        // Incremental change: the server's copy follows.
        client.did_change_events(
            &file,
            2,
            vec![json!({ "range": crate::lsp_range((19, 0), (19, 15)), "text": "add(3, 4);" })],
        );
        let state = state(&client, &file).unwrap();
        assert!(state["text"].as_str().unwrap().ends_with("add(3, 4);\n"));
        assert_eq!(state["rangedChanges"], 1);

        let format = recv(client.formatting(&file, 4, true));
        assert!(format.as_array().unwrap().is_empty());
        client.stop();
    }

    #[test]
    fn words_and_columns() {
        assert_eq!(
            word_at("echo $user->name;", 7),
            Some(("$user".into(), 5, 10))
        );
        assert_eq!(word_at("$a->greet()", 9), Some(("greet".into(), 4, 9)));
        assert_eq!(find_word("$name . $names . name", "$name"), vec![0]);
        assert_eq!(find_word("$name . $names . name", "name"), vec![17]);
    }
}
