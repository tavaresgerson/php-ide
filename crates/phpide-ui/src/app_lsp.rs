//! Language intelligence: language server lifecycle, requests and their
//! routing, completion, symbols and structural navigation.

use super::*;

impl PhpIdeApp {
    /// Restart Language Server (T-PHP-001-02): stops the server and starts
    /// the one the settings choose; the restarts after crashes count again.
    pub fn restart_lsp(&mut self) {
        self.lsp_supervisor.reset();
        self.restart_lsp_now();
        self.ws.message = if self.lsp.is_some() {
            format!("restarting the language server: {}", self.lsp_name)
        } else {
            self.lsp_start_error
                .clone()
                .unwrap_or_else(|| "no language server to restart".to_string())
        };
    }

    /// Stops the server and forgets its session (T-PHP-001-04), then starts
    /// it again.
    pub(crate) fn restart_lsp_now(&mut self) {
        self.stop_lsp_session();
        self.spawn_lsp();
    }

    /// Stops the server and drops everything it answered.
    fn stop_lsp_session(&mut self) {
        if let Some(old) = self.lsp.take() {
            old.stop();
        }
        self.lsp_init = None;
        self.lsp_opened.clear();
        self.lsp_mirrors.clear();
        self.lsp_raw_diagnostics.clear();
        self.pending_requests.clear_origin(routing::LspOrigin::Php);
        self.pending_outline = None;
        self.last_outline_key = None;
        // The syntax-tree structure stands in until the new server answers.
        self.outline_from_server = false;
        self.outline_local_revision = None;
        self.pending_symbols = None;
        self.symbol_results.clear();
        self.semantic = None;
        self.pending_semantic = None;
        self.occurrences = None;
        self.pending_occurrences = None;
        self.lsp_crash_notified = false;
        self.code_lens.clear();
        self.document_links = Default::default();
        self.selection_ranges = Default::default();
    }

    /// PHPantom's file filters for this project now.
    pub(crate) fn lsp_indexing_settings(&self) -> Value {
        lsp_engine::indexing_settings(
            &self.ws.root,
            &self.project_marks,
            &self.settings.file_type_patterns,
        )
    }

    /// Starts the PHP language server the settings choose.
    pub(crate) fn spawn_lsp(&mut self) {
        let choice = lsp_engine::effective_choice(&self.settings, &self.ws.root);
        let folders = self.settings.attached_to(&self.ws.root);
        let filters = self.lsp_indexing_settings();
        let start = start_lsp(&self.ws.root, &choice, folders.clone(), filters.clone());
        let phpantom = start
            .server
            .as_ref()
            .map(|s| s.kind == phpide_lsp::engine::EngineKind::Phpantom)
            .unwrap_or(start.client.is_some());
        self.lsp = start.client;
        self.lsp_init = start.init;
        self.lsp_name = start.name;
        self.lsp_server = start.server;
        self.lsp_start_error = start.error;
        self.lsp_settings_sent = if phpantom { filters } else { Value::Null };
        self.lsp_folders = folders;
    }

    /// Tests: talk to the built-in fixture server.
    #[cfg(test)]
    pub(crate) fn use_fixture_lsp(&mut self) {
        self.stop_lsp_session();
        let folders = self.settings.attached_to(&self.ws.root);
        let filters = self.lsp_indexing_settings();
        let options = phpide_lsp::InitOptions {
            extra_folders: folders.clone(),
            initialization_options: filters.clone(),
        };
        let (client, init) =
            phpide_lsp::fixture::start_with(&self.ws.root, &options).expect("fixture server");
        self.lsp = Some(client);
        self.lsp_init = Some(init);
        self.lsp_name = "fixture-php".to_string();
        self.lsp_server = None;
        self.lsp_start_error = None;
        self.lsp_settings_sent = filters;
        self.lsp_folders = folders;
    }

    /// Work the server reports: its start and end go to the Event Log; the
    /// status bar shows it while it runs.
    fn poll_lsp_progress(&mut self) {
        let Some(lsp) = &self.lsp else { return };
        // PHPantom names itself in its titles ("PHPantom: Indexing").
        let name = self.lsp_name.clone();
        let titled = |title: &str| {
            if title.starts_with(&format!("{name}:")) {
                title.to_string()
            } else {
                format!("{name}: {title}")
            }
        };
        // Indexing is logged; shorter work (a hierarchy, a rename) only
        // shows in the status bar while it runs.
        let logged = |title: &str| title.to_lowercase().contains("index");
        let mut lines: Vec<String> = lsp
            .take_started()
            .iter()
            .filter(|title| logged(title))
            .map(|title| format!("{}…", titled(title)))
            .collect();
        for (title, message) in lsp.take_finished() {
            if !logged(&title) {
                continue;
            }
            lines.push(
                if message.is_empty() || message.eq_ignore_ascii_case("done") {
                    format!("{} done", titled(&title))
                } else {
                    format!("{} done: {message}", titled(&title))
                },
            );
        }
        // Each one is logged, even when several arrive in one frame.
        for line in lines {
            self.event_log.observe(&line, &mut self.toasts);
            self.ws.message = line;
        }
    }

    /// Keeps PHPantom's file filters and the workspace folders in step with
    /// the project (excluded marks, File Types, attached projects).
    fn push_lsp_configuration(&mut self) {
        let Some(lsp) = &self.lsp else { return };
        if !self.lsp_settings_sent.is_null() {
            let filters = self.lsp_indexing_settings();
            if filters != self.lsp_settings_sent {
                lsp.did_change_configuration(filters.clone());
                self.lsp_settings_sent = filters;
            }
        }
        let folders = self.settings.attached_to(&self.ws.root);
        if folders != self.lsp_folders {
            if lsp.capabilities().workspace_folders_changes {
                let added: Vec<PathBuf> = folders
                    .iter()
                    .filter(|f| !self.lsp_folders.contains(f))
                    .cloned()
                    .collect();
                let removed: Vec<PathBuf> = self
                    .lsp_folders
                    .iter()
                    .filter(|f| !folders.contains(f))
                    .cloned()
                    .collect();
                lsp.did_change_workspace_folders(&added, &removed);
            }
            self.lsp_folders = folders;
        }
    }

    /// Find Usages (T-PHP-004-03): referências alimentam o painel Find.
    pub fn request_usages(&mut self) {
        self.sync_documents();
        let Some(path0) = self.ws.active_buffer().and_then(|b| b.path.clone()) else {
            return;
        };
        let Some((lsp, origin)) = lsp_client_for_path(&self.lsp, &self.fe, &path0) else {
            self.ws.message = "no language server for this file".to_string();
            return;
        };
        if !matches!(lsp.status(), LspStatus::Running(_)) {
            self.ws.message = "the language server is not ready yet".to_string();
            return;
        }
        if !lsp.capabilities().references_provider {
            self.ws.message = "the language server does not support find usages".to_string();
            return;
        }
        let Some(buf) = self.ws.active_buffer() else {
            return;
        };
        let Some(path) = buf.path.clone() else { return };
        let (line, col) = buf.document.line_col_encoded(
            self.cursor_char.min(buf.document.len_chars()),
            phpide_core::PositionEncoding::Utf16,
        );
        self.pending_requests.insert(
            routing::RequestKey {
                origin,
                operation: routing::RequestOperation::Usages,
                path: path.clone(),
                revision: buf.document.revision(),
            },
            lsp.references(&path, line, col),
        );
        self.ws.message = "finding usages...".to_string();
    }

    /// Frontend (FE-001-01/02): inicializa servidores FE, sincroniza buffers
    /// por extensão e consome diagnósticos publicados → painel de problemas.
    /// Servidor ausente = degradação com skip (NFR-003).
    pub(crate) fn poll_fe(&mut self) {
        if self.fe.is_empty() {
            return;
        }
        for srv in self.fe.iter_mut() {
            fe_servers::poll_init(srv);
        }
        self.sync_fe_documents();
        let published = self.fe.drain_diagnostics();
        for (path, items) in published {
            let revision = self
                .ws
                .buffers
                .iter()
                .find(|b| b.path.as_deref() == Some(path.as_path()))
                .map(|b| b.document.revision())
                .unwrap_or(u64::MAX);
            let diags: Vec<phpide_core::Diagnostic> = items
                .iter()
                .map(|d| phpide_core::Diagnostic {
                    path: path.clone(),
                    line: d.line0 + 1,
                    col: d.col0 + 1,
                    message: d.message.clone(),
                    severity: match d.severity {
                        1 => phpide_core::Severity::Error,
                        2 => phpide_core::Severity::Warning,
                        3 => phpide_core::Severity::Information,
                        _ => phpide_core::Severity::Hint,
                    },
                    source: d.source.clone(),
                })
                .collect();
            self.ws.set_diagnostics(&path, revision, diags);
        }
    }

    /// Consome o resultado de Find Usages para o painel Find (com preview).
    pub(crate) fn handle_usages_result(&mut self, result: Value) {
        let locs = intelligence::parse_locations(&result);
        // Code Vision (UI-ED-005-05): lens de usages com resultados reais no
        // bloco estrutural sob o cursor.
        if let (Some((path, cache)), Some(buf)) = (&self.syntax, self.ws.active_buffer()) {
            if buf.path.as_deref() == Some(path.as_path()) {
                if let Some(blocks) = cache.get(&buf.document) {
                    let line0 = buf
                        .document
                        .line_col(self.cursor_char.min(buf.document.len_chars()))
                        .0;
                    if let Some(b) = blocks
                        .iter()
                        .filter(|b| b.start_line0 <= line0 && b.end_line0 >= line0)
                        .max_by_key(|b| b.start_line0)
                    {
                        let locations: Vec<hints::UsageLocation> = locs
                            .iter()
                            .map(|l| hints::UsageLocation {
                                path: l.path.to_string_lossy().into_owned(),
                                line0: l.line0,
                                preview: String::new(),
                            })
                            .collect();
                        self.code_vision.set_usages(b.start_line0, &locations);
                    }
                }
            }
        }
        if locs.is_empty() {
            self.ws.message = "no usages found".to_string();
            return;
        }
        let mut hits = Vec::new();
        for loc in &locs {
            // Preview: linha do arquivo no disco (buffer aberto tem prioridade).
            let preview = self
                .ws
                .buffers
                .iter()
                .find(|b| b.path.as_deref() == Some(loc.path.as_path()))
                .and_then(|b| {
                    b.document
                        .line(loc.line0)
                        .lines()
                        .next()
                        .map(str::to_string)
                })
                .or_else(|| {
                    std::fs::read_to_string(&loc.path)
                        .ok()
                        .and_then(|t| t.lines().nth(loc.line0).map(str::to_string))
                })
                .unwrap_or_default();
            hits.push(search::SearchHit {
                path: loc.path.clone(),
                line: loc.line0 + 1,
                preview: preview.trim().chars().take(140).collect(),
            });
        }
        let count = hits.len();
        self.search.results = hits;
        self.search.status = format!("{count} usages (Find Usages)");
        self.search.query.clear();
        self.tw.set_visible(ToolWindow::Find, true);
        self.ws.message = format!("{count} usages found");
    }

    /// Go to Symbol (T-PHP-004-02): dispara workspace/symbol quando a
    /// consulta da palette muda.
    pub(crate) fn poll_symbol_query(&mut self) {
        if !self.palette.open || !self.palette.mode.wants_symbols() {
            return;
        }
        let query = self.palette.query.clone();
        if query == self.last_symbol_query {
            return;
        }
        self.last_symbol_query = query.clone();
        self.symbol_results.clear();
        if query.trim().len() < 2 {
            return;
        }
        let Some(lsp) = &self.lsp else { return };
        if !matches!(lsp.status(), LspStatus::Running(_)) {
            return;
        }
        if !lsp.capabilities().workspace_symbol_provider {
            return;
        }
        self.pending_symbols = Some(lsp.workspace_symbol(&query));
    }

    /// Sends the open PHP documents' changes to the server. Every request
    /// calls this first, so the server answers about the text on screen.
    /// Frontend servers get the open buffers of their file types: didOpen,
    /// then didChange with the whole text when the revision moved. Only the
    /// buffers that changed are copied.
    pub(crate) fn sync_fe_documents(&mut self) {
        if self.fe.is_empty() {
            return;
        }
        let buffers = &self.ws.buffers;
        for srv in self.fe.iter_mut() {
            let stale: Vec<(PathBuf, String, u64)> = buffers
                .iter()
                .filter_map(|b| {
                    let path = b.path.as_ref()?;
                    let revision = b.document.revision();
                    (srv_accepts_ext(srv.name, path) && srv.opened.get(path) != Some(&revision))
                        .then(|| (path.clone(), b.document.text(), revision))
                })
                .collect();
            srv.sync_buffers(stale.iter().map(|(p, t, r)| (p, t.as_str(), *r)));
        }
    }

    pub(crate) fn sync_documents(&mut self) {
        // Every request is preceded by the document sync, for the frontend
        // servers too: an edit made this frame reaches them before the
        // request that follows it (completion while typing).
        self.sync_fe_documents();
        let Some(lsp) = &self.lsp else { return };
        if !matches!(lsp.status(), LspStatus::Running(_)) || self.lsp_init.is_some() {
            return;
        }
        // One ordered plan (T-PHP-001-03): didOpen → didChange → didClose.
        let buffers: Vec<(PathBuf, u64)> = self
            .ws
            .buffers
            .iter()
            .filter_map(|b| {
                let path = b.path.clone()?;
                if !phpide_php::Php::is_php_file(&path) {
                    return None;
                }
                Some((path, b.document.revision()))
            })
            .collect();
        for op in intelligence::plan_sync(&buffers, &self.lsp_opened) {
            match op {
                intelligence::SyncOp::Open(path, version) => {
                    let text = self
                        .ws
                        .buffers
                        .iter()
                        .find(|b| b.path.as_deref() == Some(path.as_path()))
                        .map(|b| b.document.text())
                        .unwrap_or_default();
                    lsp.did_open(&path, &text, version);
                    if lsp.capabilities().sync == phpide_lsp::SyncKind::Incremental {
                        self.lsp_mirrors
                            .insert(path.clone(), phpide_lsp::sync::Mirror::new(&text, version));
                    }
                    self.lsp_opened.insert(path, version);
                }
                intelligence::SyncOp::Change(path, version) => {
                    let Some(buffer) = self
                        .ws
                        .buffers
                        .iter()
                        .find(|b| b.path.as_deref() == Some(path.as_path()))
                    else {
                        continue;
                    };
                    // Ranged changes from the edit log; full text when the
                    // server wants it or the log no longer reaches back.
                    let events = self
                        .lsp_mirrors
                        .get_mut(&path)
                        .and_then(|mirror| mirror.changes_to(&buffer.document));
                    match events {
                        Some(events) => {
                            if !events.is_empty() {
                                lsp.did_change_events(&path, version, events);
                            }
                        }
                        None => lsp.did_change(&path, &buffer.document.text(), version),
                    }
                    self.lsp_opened.insert(path, version);
                }
                intelligence::SyncOp::Close(path) => {
                    lsp.did_close(&path);
                    self.lsp_opened.remove(&path);
                    self.lsp_mirrors.remove(&path);
                    self.ws.set_diagnostics(&path, 0, Vec::new());
                }
            }
        }
    }

    pub(crate) fn poll_lsp(&mut self) {
        let Some(lsp) = &self.lsp else { return };

        // 1. Handshake do initialize.
        if let Some(rx) = &self.lsp_init {
            match rx.try_recv() {
                Ok(result) => {
                    self.lsp_init = None;
                    if result.get("capabilities").is_some() {
                        // Capabilities negociadas (T-PHP-001-01): a UI
                        // habilita recursos conforme o servidor declara.
                        lsp.register_capabilities(&result);
                        let caps = lsp.capabilities();
                        *lsp.shared.status.lock().unwrap() =
                            LspStatus::Running(self.lsp_name.clone());
                        lsp.initialized();
                        // Diagnósticos passam a ser do LSP; reanálise local do zero.
                        self.ws.clear_diagnostics();
                        self.syntax = None;
                        let _ = caps;
                        self.ws.message = format!("Language server connected: {}", self.lsp_name);
                    } else {
                        *lsp.shared.status.lock().unwrap() =
                            LspStatus::Failed(format!("initialize rejected: {result}"));
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.lsp_init = None;
                    *lsp.shared.status.lock().unwrap() =
                        LspStatus::Failed("no response to initialize".into());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        // The server stopped by itself: editing goes on offline
        // (T-PHP-001-05) and it is started again, waiting longer each time;
        // after three crashes in five minutes the user restarts it.
        if let LspStatus::Failed(reason) = lsp.status() {
            if !self.lsp_crash_notified {
                self.lsp_crash_notified = true;
                self.logger.error(
                    Component::Lsp,
                    &Correlation::session(self.session),
                    "lsp_crashed",
                    &[("reason", &reason)],
                );
                self.ws.message = match self.lsp_supervisor.crashed(Instant::now()) {
                    Some(wait) => format!(
                        "Warning: the language server {} stopped ({reason}); restarting it in {} s",
                        self.lsp_name,
                        wait.as_secs()
                    ),
                    None => format!(
                        "The language server {} stopped ({reason}) and failed {} times in 5 minutes. Editing and local diagnostics still work; restart it from Find Action (Ctrl+Shift+A > 'Restart Language Server').",
                        self.lsp_name,
                        lsp_engine::MAX_RESTARTS + 1
                    ),
                };
            }
            if self.lsp_supervisor.due(Instant::now()) {
                self.restart_lsp_now();
            }
            return;
        }
        if !matches!(lsp.status(), LspStatus::Running(_)) {
            return;
        }

        // 2. Document sync (also flushed before every request), filters
        // and workspace folders.
        self.sync_documents();
        self.push_lsp_configuration();
        self.poll_lsp_progress();
        let Some(lsp) = &self.lsp else { return };

        // 3. Diagnósticos publicados -> painel de problemas (anexados à
        // revisão do buffer: edição posterior invalida até nova publicação).
        let published: Vec<(PathBuf, Vec<LspDiagnostic>)> =
            lsp.shared.diagnostics.lock().unwrap().drain().collect();
        for (path, items) in published {
            self.lsp_raw_diagnostics.insert(
                path.clone(),
                items
                    .iter()
                    .map(|d| (d.line0, d.end_line0, d.raw.clone()))
                    .collect(),
            );

            let revision = self
                .ws
                .buffers
                .iter()
                .find(|b| b.path.as_deref() == Some(path.as_path()))
                .map(|b| b.document.revision())
                .unwrap_or(u64::MAX);
            let diags: Vec<phpide_core::Diagnostic> = items
                .iter()
                .map(|d| phpide_core::Diagnostic {
                    path: path.clone(),
                    line: d.line0 + 1,
                    col: d.col0 + 1,
                    message: d.message.clone(),
                    severity: match d.severity {
                        1 => phpide_core::Severity::Error,
                        2 => phpide_core::Severity::Warning,
                        3 => phpide_core::Severity::Information,
                        _ => phpide_core::Severity::Hint,
                    },
                    source: d.source.clone(),
                })
                .collect();
            self.ws.set_diagnostics(&path, revision, diags);
        }

        // 4. Navegação FE/PHP é consumida pelo dispatcher neutro.

        // 5. Outline pendente (só aplica se o arquivo ainda for o ativo e na
        // mesma revisão pedida).
        if let Some((req_path, rx)) = &self.pending_outline {
            let mut done = false;
            if let Ok(result) = rx.try_recv() {
                done = true;
                let active = self.ws.active_buffer();
                let matches = active
                    .and_then(|b| b.path.clone())
                    .map(|p| p == *req_path)
                    .unwrap_or(false);
                let revision_ok = active
                    .map(|b| {
                        self.last_outline_key
                            .as_ref()
                            .map(|(kp, kv)| kp == req_path && kv == &b.document.revision())
                            .unwrap_or(false)
                    })
                    .unwrap_or(false);
                if matches && revision_ok {
                    let mut entries = Vec::new();
                    collect_symbols(&result, 0, &mut entries);
                    if entries.is_empty() {
                        // An empty answer, `null` or an error (some servers
                        // answer before indexing the file): the syntax-tree
                        // structure stays, and is refreshed from the tree.
                        self.outline_from_server = false;
                        self.outline_local_revision = None;
                    } else {
                        self.ws.outline = entries;
                        self.outline_from_server = true;
                    }
                }
            }
            if done {
                self.pending_outline = None;
            }
        }

        // 6. Solicita outline quando o arquivo ativo muda de versão.
        if self.pending_outline.is_none() {
            if let Some(buf) = self.ws.active_buffer() {
                if let Some(path) = buf.path.clone() {
                    if phpide_php::Php::is_php_file(&path) {
                        let key = (path.clone(), buf.document.revision());
                        if self.last_outline_key.as_ref() != Some(&key) {
                            self.last_outline_key = Some(key);
                            let rx = lsp.document_symbol(&path);
                            self.pending_outline = Some((path, rx));
                        }
                    }
                }
            }
        }

        // 7. Go to Symbol (workspace/symbol): respostas da consulta vigente.
        if let Some(rx) = &self.pending_symbols {
            if let Ok(result) = rx.try_recv() {
                self.pending_symbols = None;
                let kinds: Vec<(u64, String)> = result
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .map(|s| {
                                (
                                    s["kind"].as_u64().unwrap_or(0),
                                    s["containerName"].as_str().unwrap_or("").to_string(),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let mut kinds = kinds.into_iter();
                self.symbol_results = intelligence::parse_workspace_symbols(&result)
                    .into_iter()
                    .map(|(name, loc)| {
                        let (kind, container) = kinds.next().unwrap_or_default();
                        palette::SymbolHit {
                            name,
                            container,
                            path: loc.path,
                            line0: loc.line0,
                            col0: loc.col0,
                            kind,
                        }
                    })
                    .collect();
            }
        }

        // 8. Semantic tokens (T-PHP-005-01): aplica só na revisão pedida.
        if let Some((req_path, req_revision, rx)) = &self.pending_semantic {
            if let Ok(result) = rx.try_recv() {
                let (req_path, req_revision) = (req_path.clone(), *req_revision);
                self.pending_semantic = None;
                let current = self
                    .ws
                    .buffers
                    .iter()
                    .find(|b| b.path.as_deref() == Some(req_path.as_path()));
                if let Some(buf) = current {
                    if buf.document.revision() == req_revision {
                        if let Some(data) = result["data"].as_array() {
                            let flat: Vec<u32> = data
                                .iter()
                                .filter_map(|v| v.as_u64().map(|n| n as u32))
                                .collect();
                            let tokens = intelligence::decode_semantic_tokens(
                                &buf.document,
                                &lsp.capabilities().semantic_token_types,
                                &flat,
                            );
                            // Spans de byte uma vez por revisão: char absoluto
                            // = início da linha + coluna, convertido pela rope.
                            let spans: Vec<(usize, usize, String)> = tokens
                                .iter()
                                .map(|t| {
                                    let abs = t.start_char.min(buf.document.len_chars());
                                    let start = buf.document.char_to_byte(abs);
                                    let end = buf.document.char_to_byte(
                                        (abs + t.len_chars).min(buf.document.len_chars()),
                                    );
                                    (start, end, t.token_type.clone())
                                })
                                .collect();
                            self.semantic = Some((req_path, req_revision, tokens, spans));
                        }
                    }
                }
            }
        }
        // Dispara semantic tokens quando o arquivo/revisão muda e há suporte.
        if lsp.capabilities().semantic_tokens_provider {
            let active = self.ws.active_buffer().and_then(|b| {
                let path = b.path.clone()?;
                if !phpide_php::Php::is_php_file(&path) {
                    return None;
                }
                Some((path, b.document.revision()))
            });
            let needs = match (&active, &self.semantic, &self.pending_semantic) {
                (Some((p, r)), cached, pending) => {
                    let cached_stale = cached
                        .as_ref()
                        .map(|(cp, cr, _, _)| cp != p || cr != r)
                        .unwrap_or(true);
                    let pending_stale = pending
                        .as_ref()
                        .map(|(pp, pr, _)| pp != p || pr != r)
                        .unwrap_or(false);
                    cached_stale && !pending_stale
                }
                (None, _, _) => false,
            };
            if needs {
                if let Some((path, revision)) = active {
                    let rx = lsp.semantic_tokens_full(&path);
                    self.pending_semantic = Some((path, revision, rx));
                }
            }
        } else {
            self.semantic = None;
        }

        // 9. Ocorrências (documentHighlight) com fallback local (005-02).
        if let Some(rx) = &self.pending_occurrences {
            if let Ok(result) = rx.try_recv() {
                self.pending_occurrences = None;
                if let Some(b) = self.ws.active_buffer() {
                    if let Some(path) = b.path.clone() {
                        let (word, _) = completion::word_around(&b.document, self.cursor_char);
                        if word.len() >= 2 {
                            let ranges = intelligence::parse_highlights(&b.document, &result);
                            self.occurrences = Some((path, b.document.revision(), word, ranges));
                        }
                    }
                }
            }
        }
        let occurrences_key = self.ws.active_buffer().and_then(|b| {
            let path = b.path.clone()?;
            let (word, _) = completion::word_around(&b.document, self.cursor_char);
            Some((path, b.document.revision(), word))
        });
        match (&occurrences_key, &self.occurrences) {
            (Some((p, r, w)), Some((op, or_, ow, _))) if op == p && or_ == r && ow == w => {}
            (Some((p, r, w)), _) => {
                let word_ctx = self
                    .ws
                    .active_buffer()
                    .map(|b| completion::word_around(&b.document, self.cursor_char));
                if let Some((word, _start)) = word_ctx {
                    if word.len() >= 2 && &word == w {
                        // Server first; without support, a local fallback
                        // (only computed when the word changes).
                        let ranges = if lsp.capabilities().document_highlight_provider {
                            if self.pending_occurrences.is_none() {
                                let (line, col) = self
                                    .ws
                                    .active_buffer()
                                    .map(|b| {
                                        b.document.line_col_encoded(
                                            self.cursor_char.min(b.document.len_chars()),
                                            phpide_core::PositionEncoding::Utf16,
                                        )
                                    })
                                    .unwrap_or((0, 0));
                                let path = p.clone();
                                let rx = lsp.document_highlight(&path, line, col);
                                self.pending_occurrences = Some(rx);
                            }
                            None
                        } else {
                            let chars: Vec<char> = self
                                .ws
                                .active_buffer()
                                .map(|b| b.document.text().chars().collect())
                                .unwrap_or_default();
                            Some(intelligence::local_occurrences(
                                &chars,
                                &word,
                                self.cursor_char,
                            ))
                        };
                        if let Some(ranges) = ranges {
                            self.occurrences = Some((p.clone(), *r, w.clone(), ranges));
                        }
                    }
                    if word.len() < 2 {
                        self.occurrences = None;
                    }
                }
            }
            (None, _) => self.occurrences = None,
        }
    }

    pub(crate) fn poll_routed_requests(&mut self) {
        for (key, result) in self.pending_requests.poll() {
            let current_revision = self
                .ws
                .buffers
                .iter()
                .find(|buffer| buffer.path.as_deref() == Some(key.path.as_path()))
                .map(|buffer| buffer.document.revision());
            if !key.is_current(&key.path, current_revision.unwrap_or(u64::MAX)) {
                self.logger.info(
                    Component::Lsp,
                    &Correlation::session(self.session).with_document(key.path, key.revision),
                    "stale_response_discarded",
                    &[("operation", &format!("{:?}", key.operation))],
                );
                continue;
            }
            match key.operation {
                routing::RequestOperation::Definition
                | routing::RequestOperation::Implementation
                | routing::RequestOperation::TypeDefinition
                | routing::RequestOperation::Declaration => {
                    let locations = intelligence::parse_locations(&result);
                    let title = match key.operation {
                        routing::RequestOperation::Implementation => "Implementations",
                        routing::RequestOperation::Declaration => "Super methods",
                        _ => "Declarations",
                    };
                    match locations.as_slice() {
                        [] => {
                            self.ws.message = match key.operation {
                                routing::RequestOperation::Declaration => {
                                    "no super method found".to_string()
                                }
                                routing::RequestOperation::Implementation => {
                                    "no implementations found".to_string()
                                }
                                _ => "no declaration found".to_string(),
                            };
                        }
                        [location] => {
                            self.ws
                                .open_file_at(&location.path, location.line0, location.col0);
                        }
                        [first, ..]
                            if matches!(
                                key.operation,
                                routing::RequestOperation::Definition
                                    | routing::RequestOperation::TypeDefinition
                            ) =>
                        {
                            self.ws.open_file_at(&first.path, first.line0, first.col0);
                        }
                        many => {
                            let many = many.to_vec();
                            self.show_location_list(title, &many);
                        }
                    }
                }
                routing::RequestOperation::Hints => {
                    if active_path_matches(&self.ws, &key.path, key.revision) {
                        let parsed = parse_inlay_hints(&result);
                        self.hints.apply(hints::HintResponse {
                            revision: hints::Revision(key.revision),
                            hints: parsed,
                            status: result
                                .is_null()
                                .then(|| "the language server has no inlay hints".to_string()),
                        });
                        self.hints_path = Some(key.path);
                    }
                }
                routing::RequestOperation::Usages => {
                    if active_path_matches(&self.ws, &key.path, key.revision) {
                        self.handle_usages_result(result);
                    }
                }
            }
        }
    }

    pub(crate) fn poll_hints(&mut self) {
        if !self.settings.inlay_hints {
            if self.hints.current.is_some() {
                self.hints.invalidate();
            }
            self.hints_path = None;
            return;
        }
        let Some(buffer) = self.ws.active_buffer() else {
            return;
        };
        let Some(path) = buffer.path.clone() else {
            return;
        };
        let revision = buffer.document.revision();
        let line_count = buffer.document.line_count();
        let Some((client, origin)) = lsp_client_for_path(&self.lsp, &self.fe, &path) else {
            return;
        };
        if !matches!(client.status(), LspStatus::Running(_)) {
            return;
        }
        let key = routing::RequestKey {
            origin,
            operation: routing::RequestOperation::Hints,
            path: path.clone(),
            revision,
        };
        let cached = self.hints_path.as_ref() == Some(&path)
            && self.hints.current == Some(hints::Revision(revision));
        if cached || self.pending_requests.contains(&key) {
            return;
        }
        let end_line = line_count.saturating_sub(1) as u64;
        let receiver = client.request(
            "textDocument/inlayHint",
            serde_json::json!({
                "textDocument": { "uri": phpide_lsp::path_to_uri(&path) },
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": end_line, "character": 10_000 }
                }
            }),
        );
        self.pending_requests.insert(key, receiver);
        self.hints.begin(hints::Revision(revision));
    }

    pub(crate) fn request_definition(&mut self) {
        self.request_locations("definition", |lsp, path, line, col| {
            lsp.definition(path, line, col)
        });
    }

    /// Navegação por localizações genérica (T-PHP-004-01): definition,
    /// implementation e typeDefinition compartilham tratamento de resposta,
    /// gate por capability e descarte por revisão (T-PHP-001-04).
    pub(crate) fn request_locations(
        &mut self,
        what: &'static str,
        call: impl Fn(&LspClient, &std::path::Path, usize, usize) -> mpsc::Receiver<Value>,
    ) {
        self.sync_documents();
        let Some(path0) = self.ws.active_buffer().and_then(|b| b.path.clone()) else {
            return;
        };
        let Some((lsp, origin)) = lsp_client_for_path(&self.lsp, &self.fe, &path0) else {
            self.ws.message = "no language server for this file".to_string();
            return;
        };
        if !matches!(lsp.status(), LspStatus::Running(_)) {
            self.ws.message = "the language server is not ready yet".to_string();
            return;
        }
        let caps = lsp.capabilities();
        let supported = match what {
            "definition" => caps.definition_provider,
            "implementation" => caps.implementation_provider,
            "typeDefinition" => caps.type_definition_provider,
            "declaration" => caps.declaration_provider,
            _ => false,
        };
        if !supported {
            self.ws.message = format!("the language server does not support {what}");
            return;
        }
        let Some(buf) = self.ws.active_buffer() else {
            return;
        };
        let Some(path) = buf.path.clone() else { return };
        let revision = buf.document.revision();
        let (line, col) = buf.document.line_col_encoded(
            self.cursor_char.min(buf.document.len_chars()),
            phpide_core::PositionEncoding::Utf16,
        );
        let operation = match what {
            "definition" => routing::RequestOperation::Definition,
            "implementation" => routing::RequestOperation::Implementation,
            "typeDefinition" => routing::RequestOperation::TypeDefinition,
            "declaration" => routing::RequestOperation::Declaration,
            _ => return,
        };
        let receiver = call(lsp, &path, line, col);
        self.pending_requests.insert(
            routing::RequestKey {
                origin,
                operation,
                path,
                revision,
            },
            receiver,
        );
    }

    pub(crate) fn request_implementation(&mut self) {
        self.request_locations("implementation", |lsp, path, line, col| {
            lsp.implementation(path, line, col)
        });
    }

    pub(crate) fn request_type_definition(&mut self) {
        self.request_locations("typeDefinition", |lsp, path, line, col| {
            lsp.type_definition(path, line, col)
        });
    }

    pub(crate) fn lsp_label(&self) -> String {
        match &self.lsp {
            None => "LSP: off".to_string(),
            Some(lsp) => match lsp.status() {
                LspStatus::Starting => "LSP: starting...".to_string(),
                LspStatus::Running(name) => format!("LSP: {name}"),
                LspStatus::Failed(e) => format!("LSP: failed ({e})"),
            },
        }
    }

    /// Análise estrutural do buffer ativo (tree-sitter). Diagnósticos locais só
    /// entram quando o LSP está fora; os blocos alimentam Ctrl+[ / Ctrl+] e a
    /// navigation bar sempre.
    pub(crate) fn poll_syntax(&mut self) {
        let lsp_running = self
            .lsp
            .as_ref()
            .map(|l| matches!(l.status(), LspStatus::Running(_)))
            .unwrap_or(false);
        let active_php = self
            .ws
            .active_buffer()
            .and_then(|b| b.path.clone())
            .filter(|path| phpide_php::Php::is_php_file(path));
        let Some(path) = active_php else {
            if self.markdown_outline() {
                return;
            }
            // The structure always describes the active PHP file.
            if self.outline_path.take().is_some() {
                self.ws.outline.clear();
                self.outline_from_server = false;
                self.outline_local_revision = None;
            }
            return;
        };
        if self.outline_path.as_ref() != Some(&path) {
            // Another file: nothing of the previous one stays on screen.
            self.ws.outline.clear();
            self.outline_path = Some(path.clone());
            self.outline_from_server = false;
            self.outline_local_revision = None;
        }
        let index = self.ws.active;
        let buffer = &self.ws.buffers[index];
        let syntax = self.editors.syntax(buffer);
        let Some(structure) = syntax.structure(&buffer.document) else {
            return;
        };
        let Some(version) = syntax.structure_revision() else {
            return;
        };
        // Structure tool window and File Structure: the syntax tree's
        // declarations, unless the language server answered for this file.
        if (!self.outline_from_server || !lsp_running)
            && self.outline_local_revision != Some(version)
        {
            self.ws.outline = structure
                .report
                .outline
                .iter()
                .map(|symbol| phpide_core::OutlineEntry {
                    name: symbol.name.clone(),
                    kind: symbol.kind.to_string(),
                    line0: symbol.line0,
                    depth: symbol.depth,
                })
                .collect();
            self.outline_from_server = false;
            self.outline_local_revision = Some(version);
        }
        let fresh =
            matches!(&self.syntax, Some((p, cache)) if p == &path && cache.revision() == version);
        if fresh {
            return;
        }
        let report = structure.report.clone();
        self.syntax = Some((
            path.clone(),
            Revisioned::new(version, report.blocks.clone()),
        ));
        if !lsp_running {
            let diags: Vec<phpide_core::Diagnostic> = report
                .errors
                .iter()
                .map(|e| phpide_core::Diagnostic {
                    path: path.clone(),
                    line: e.line0 + 1,
                    col: e.col0 + 1,
                    message: e.message.clone(),
                    severity: phpide_core::Severity::Error,
                    source: "tree-sitter".to_string(),
                })
                .collect();
            self.ws.set_diagnostics(&path, version, diags);
        }
    }

    /// Salta para o início/fim do bloco estrutural que contém a linha do cursor.
    pub(crate) fn jump_block(&mut self, to_start: bool) {
        let target = {
            let Some(buf) = self.ws.active_buffer_mut() else {
                return;
            };
            if !buf
                .path
                .as_deref()
                .map(phpide_php::Php::is_php_file)
                .unwrap_or(false)
            {
                return;
            }
            let line0 = buf
                .document
                .line_col(self.cursor_char.min(buf.document.len_chars()))
                .0
                .saturating_sub(1);
            phpide_syntax::enclosing_block(&buf.document.text(), line0).map(|b| {
                if to_start {
                    b.start_line0
                } else {
                    b.end_line0
                }
            })
        };
        match target {
            Some(line0) => {
                if let Some(buf) = self.ws.active_buffer_mut() {
                    buf.request_cursor(line0, 0);
                }
            }
            None => self.ws.message = "no code block on this line".to_string(),
        }
    }

    // -----------------------------------------------------------------
    // Estágio 4 — executor: php / composer / phpunit
    // -----------------------------------------------------------------
}

pub(crate) fn lsp_client_for_path<'a>(
    php: &'a Option<LspClient>,
    fe: &'a fe_servers::FeServers,
    path: &Path,
) -> Option<(&'a LspClient, routing::LspOrigin)> {
    if phpide_php::Php::is_php_file(path) {
        return php.as_ref().map(|client| (client, routing::LspOrigin::Php));
    }
    let server = fe.for_path(path)?;
    let origin = routing::LspOrigin::from_server_name(server.name)?;
    Some((&server.client, origin))
}

pub(crate) fn lsp_client_for_origin<'a>(
    php: &'a Option<LspClient>,
    fe: &'a fe_servers::FeServers,
    origin: routing::LspOrigin,
) -> Option<&'a LspClient> {
    match origin {
        routing::LspOrigin::Php => php.as_ref(),
        _ => fe.for_origin(origin).map(|server| &server.client),
    }
}

/// Servidor FE aceita o arquivo? (nome × extensão).
pub(crate) fn srv_accepts_ext(name: &str, path: &std::path::Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    match name {
        "TypeScript" => fe_servers::TS_EXTS.contains(&ext.to_ascii_lowercase().as_str()),
        "HTML" => fe_servers::HTML_EXTS.contains(&ext.to_ascii_lowercase().as_str()),
        "CSS" => fe_servers::CSS_EXTS.contains(&ext.to_ascii_lowercase().as_str()),
        _ => false,
    }
}

/// Converte a resposta LSP `textDocument/inlayHint` no modelo interno
/// (label pode ser string ou partes com formatação).
pub(crate) fn parse_inlay_hints(result: &Value) -> Vec<hints::InlayHint> {
    result
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|h| {
                    let line0 = h["position"]["line"].as_u64()? as usize;
                    let label = match &h["label"] {
                        Value::String(s) => s.clone(),
                        Value::Array(parts) => parts
                            .iter()
                            .filter_map(|p| p["value"].as_str().map(String::from))
                            .collect(),
                        _ => return None,
                    };
                    Some(hints::InlayHint { line0, label })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Short kind label for the navigation bar.
pub(crate) fn short_kind(kind: &str) -> &'static str {
    match kind {
        "namespace_definition" => "ns",
        "class_declaration" => "class",
        "interface_declaration" => "interface",
        "trait_declaration" => "trait",
        "enum_declaration" => "enum",
        "function_definition" => "fn",
        "method_declaration" => "method",
        "if_statement" => "if",
        "foreach_statement" => "foreach",
        "while_statement" => "while",
        "for_statement" => "for",
        "switch_statement" => "switch",
        _ => "block",
    }
}

/// Converte a resposta de documentSymbol (SymbolInformation plano ou
/// DocumentSymbol hierárquico) em entradas de outline com profundidade.
pub(crate) fn collect_symbols(
    value: &Value,
    depth: usize,
    out: &mut Vec<phpide_core::OutlineEntry>,
) {
    let Some(arr) = value.as_array() else { return };
    for item in arr {
        let name = item["name"].as_str().unwrap_or("?").to_string();
        let kind = symbol_kind_label(item["kind"].as_u64().unwrap_or(0));
        if item.get("location").is_some() {
            // SymbolInformation: { name, kind, location: { uri, range } }
            if let Some(line) = item["location"]["range"]["start"]["line"].as_u64() {
                out.push(phpide_core::OutlineEntry {
                    name,
                    kind,
                    line0: line as usize,
                    depth,
                });
            }
        } else if let Some(line) = item["range"]["start"]["line"].as_u64() {
            // DocumentSymbol: { name, kind, range, selectionRange, children }
            out.push(phpide_core::OutlineEntry {
                name,
                kind,
                line0: line as usize,
                depth,
            });
            if let Some(children) = item.get("children") {
                collect_symbols(children, depth + 1, out);
            }
        }
    }
}

pub(crate) fn symbol_kind_label(kind: u64) -> String {
    match kind {
        1 => "file",
        2 => "module",
        3 => "namespace",
        4 => "package",
        5 => "class",
        6 => "method",
        7 => "property",
        8 => "field",
        9 => "constructor",
        10 => "enum",
        11 => "interface",
        12 => "function",
        13 => "variable",
        14 => "constant",
        22 => "enum case",
        23 => "struct",
        26 => "type parameter",
        _ => "symbol",
    }
    .to_string()
}
