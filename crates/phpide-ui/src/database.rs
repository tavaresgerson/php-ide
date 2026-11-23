use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;

use eframe::egui;
use phpide_db::console::split_statements;
use phpide_db::drivers::remote::driver_for;
use phpide_db::edit::{self, CellChange, PendingChanges};
use phpide_db::grid::{paginate, GridPage};
use phpide_db::secrets::{
    open_secret_store, redact, resolve_url, DataSource, DataSourceStore, SecretStore,
};
use phpide_db::{DbSession, DriverKind, ExecResult, QueryOptions, SchemaMeta, Value};

use crate::PhpIdeApp;

type CancelCallback = Arc<dyn Fn() + Send + Sync>;
type SharedCancel = Arc<Mutex<Option<CancelCallback>>>;

enum Command {
    Schema(u64),
    Execute(u64, String),
    Grid(u64, String, u64, u64, QueryOptions),
    Apply(u64, PendingChanges),
    Transaction(u64, TransactionAction),
    Shutdown,
}

#[derive(Debug, Clone, Copy)]
enum TransactionAction {
    Begin,
    Commit,
    Rollback,
}

enum Event {
    Connected(u64, Result<SchemaMeta, String>),
    Schema(u64, Result<SchemaMeta, String>),
    Execute(u64, Result<ExecResult, String>),
    Grid(u64, Result<GridPage, String>),
    Applied(u64, Result<usize, String>),
    Transaction(u64, TransactionAction, Result<(), String>),
    Test(u64, Result<String, String>),
}

struct Worker {
    commands: mpsc::Sender<Command>,
    events: mpsc::Receiver<Event>,
    cancel: SharedCancel,
    cancelled: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Worker {
    fn start(id: u64, url: String) -> Self {
        let (command_tx, command_rx) = mpsc::channel::<Command>();
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let cancel: SharedCancel = Arc::new(Mutex::new(None));
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker_cancelled = cancelled.clone();
        let join = std::thread::spawn(move || {
            let mut session: Option<Box<dyn DbSession>> =
                match driver_for(&url).and_then(|driver| driver.connect(&url)) {
                    Ok(session) => {
                        *worker_cancel.lock().unwrap() = session.cancellation_callback();
                        Some(session)
                    }
                    Err(error) => {
                        let _ = event_tx.send(Event::Connected(id, Err(error.to_string())));
                        None
                    }
                };
            if let Some(session) = session.as_mut() {
                let result = session.schema().map_err(|error| error.to_string());
                let _ = event_tx.send(Event::Connected(id, result));
            }
            while let Ok(command) = command_rx.recv() {
                let Some(session) = session.as_mut() else {
                    if matches!(command, Command::Shutdown) {
                        break;
                    }
                    continue;
                };
                let cancelled = || worker_cancelled.load(Ordering::SeqCst);
                let map_error = |error: phpide_core::IdeError| {
                    if cancelled() {
                        "consulta cancelada pelo usuário".to_string()
                    } else {
                        error.to_string()
                    }
                };
                match command {
                    Command::Schema(command_id) => {
                        let result = session.schema().map_err(map_error);
                        let _ = event_tx.send(Event::Schema(command_id, result));
                    }
                    Command::Execute(command_id, sql) => {
                        let result = session
                            .execute(&sql, &QueryOptions::default())
                            .map_err(map_error);
                        let _ = event_tx.send(Event::Execute(command_id, result));
                    }
                    Command::Grid(command_id, table, page, page_size, options) => {
                        let result = paginate(&mut **session, &table, page, page_size, &options)
                            .map_err(map_error);
                        let _ = event_tx.send(Event::Grid(command_id, result));
                    }
                    Command::Apply(command_id, changes) => {
                        let result = edit::apply(&mut **session, &changes).map_err(map_error);
                        let _ = event_tx.send(Event::Applied(command_id, result));
                    }
                    Command::Transaction(command_id, action) => {
                        let result = match action {
                            TransactionAction::Begin => session.begin(),
                            TransactionAction::Commit => session.commit(),
                            TransactionAction::Rollback => session.rollback(),
                        }
                        .map_err(map_error);
                        let _ = event_tx.send(Event::Transaction(command_id, action, result));
                    }
                    Command::Shutdown => {
                        session.cancel();
                        break;
                    }
                }
                worker_cancelled.store(false, Ordering::SeqCst);
            }
        });
        Self {
            commands: command_tx,
            events: event_rx,
            cancel,
            cancelled,
            join: Some(join),
        }
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(cancel) = self.cancel.lock().unwrap().clone() {
            cancel();
        }
    }

    fn shutdown(&mut self) {
        self.cancel();
        let _ = self.commands.send(Command::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

pub struct DatabaseState {
    store: DataSourceStore,
    secrets: Box<dyn SecretStore>,
    worker: Option<Worker>,
    test_receiver: Option<mpsc::Receiver<Event>>,
    session_id: u64,
    test_id: u64,
    selected_id: Option<String>,
    form_name: String,
    form_url: String,
    form_password: String,
    schema: Option<SchemaMeta>,
    selected_table: Option<String>,
    sql: String,
    grid: Option<GridPage>,
    page: u64,
    page_size: u64,
    running: bool,
    status: String,
    history: Vec<String>,
    pending: PendingChanges,
    edit_buffers: HashMap<(usize, usize), String>,
    preview: Option<String>,
    in_transaction: bool,
    confirm_destructive: Option<String>,
    show_form: bool,
}

impl DatabaseState {
    pub fn new(root: &Path) -> Self {
        Self::with_secret_store(root, open_secret_store())
    }

    fn with_secret_store(root: &Path, secrets: Box<dyn SecretStore>) -> Self {
        let store = DataSourceStore::for_project(root);
        let status = store
            .load_error
            .as_ref()
            .map(|error| format!("Data sources inválidos; gravação bloqueada: {error}"))
            .unwrap_or_else(|| format!("Segredos: {}", secrets.backend_name()));
        let selected_id = store.list.first().map(|source| source.id.clone());
        let mut state = Self {
            store,
            secrets,
            worker: None,
            test_receiver: None,
            session_id: 0,
            test_id: 0,
            selected_id,
            form_name: "SQLite local".into(),
            form_url: "sqlite://:memory:".into(),
            form_password: String::new(),
            schema: None,
            selected_table: None,
            sql: "SELECT 1;".into(),
            grid: None,
            page: 0,
            page_size: 50,
            running: false,
            status,
            history: Vec::new(),
            pending: PendingChanges::default(),
            edit_buffers: HashMap::new(),
            preview: None,
            in_transaction: false,
            confirm_destructive: None,
            show_form: false,
        };
        state.sync_form();
        state
    }

    pub fn connect_url_for_smoke(&mut self, url: &str) {
        self.form_url = url.to_string();
        self.save_source();
        self.connect();
    }

    pub fn reset_project(&mut self, root: &Path) {
        self.shutdown();
        *self = Self::new(root);
    }

    pub fn shutdown(&mut self) {
        if let Some(mut worker) = self.worker.take() {
            worker.shutdown();
        }
        self.running = false;
    }

    fn selected_source(&self) -> Option<DataSource> {
        self.store
            .list
            .iter()
            .find(|source| Some(&source.id) == self.selected_id.as_ref())
            .cloned()
    }

    fn sync_form(&mut self) {
        if let Some(source) = self.selected_source() {
            self.form_name = source.name;
            self.form_url = source.url;
            self.form_password.clear();
        }
    }

    fn save_source(&mut self) {
        let (kind, _) = match phpide_db::parse_url(&self.form_url) {
            Ok(parsed) => parsed,
            Err(error) => {
                self.status = error.to_string();
                return;
            }
        };
        let mut source = match self.selected_source() {
            Some(source) => source,
            None => match DataSource::new(&self.form_name, kind, &self.form_url) {
                Ok(source) => source,
                Err(error) => {
                    self.status = error.to_string();
                    return;
                }
            },
        };
        source.name = self.form_name.trim().to_string();
        source.url = self.form_url.trim().to_string();
        if let Err(error) = DataSource::new(&source.name, kind, &source.url) {
            self.status = error.to_string();
            return;
        }
        if !self.form_password.is_empty() {
            let secret_ref = format!("datasource-{}", source.id);
            let credential = if self.form_url.contains("://") && !self.form_password.contains(':') {
                format!(":{}", self.form_password)
            } else {
                self.form_password.clone()
            };
            if let Err(error) = self.secrets.set(&secret_ref, &credential) {
                self.status = error.to_string();
                return;
            }
            source.secret_ref = Some(secret_ref);
        }
        if let Err(error) = self.store.save(source.clone()) {
            self.status = error.to_string();
            return;
        }
        self.selected_id = Some(source.id);
        self.form_password.clear();
        self.status = format!(
            "Data source salvo; segredos: {}",
            self.secrets.backend_name()
        );
    }

    fn remove_source(&mut self) {
        if let Some(source) = self.selected_source() {
            if let Err(error) = self.store.remove(&source.id) {
                self.status = error.to_string();
                return;
            }
            if let Some(secret_ref) = source.secret_ref {
                let _ = self.secrets.delete(&secret_ref);
            }
            self.selected_id = self.store.list.first().map(|source| source.id.clone());
            self.shutdown();
            self.schema = None;
            self.grid = None;
            self.sync_form();
        }
    }

    fn test_connection(&mut self) {
        let Some(source) = self.selected_source() else {
            self.status = "Salve e selecione um data source".into();
            return;
        };
        let url = match resolve_url(&source, &*self.secrets) {
            Ok(url) => url,
            Err(error) => {
                self.status = error.to_string();
                return;
            }
        };
        self.test_id += 1;
        let id = self.test_id;
        let (sender, receiver) = mpsc::channel();
        self.test_receiver = Some(receiver);
        std::thread::spawn(move || {
            let result = driver_for(&url)
                .and_then(|driver| driver.test(&url))
                .map_err(|error| error.to_string());
            let _ = sender.send(Event::Test(id, result));
        });
        self.running = true;
        self.status = format!("Testando {} sem abrir console...", redact(&source));
    }

    fn connect(&mut self) {
        let Some(source) = self.selected_source() else {
            self.status = "Selecione um data source".into();
            return;
        };
        let url = match resolve_url(&source, &*self.secrets) {
            Ok(url) => url,
            Err(error) => {
                self.status = error.to_string();
                return;
            }
        };
        self.shutdown();
        self.session_id += 1;
        let id = self.session_id;
        self.worker = Some(Worker::start(id, url));
        self.running = true;
        self.schema = None;
        self.grid = None;
        self.pending = PendingChanges::default();
        self.edit_buffers.clear();
        self.status = format!("Conectando {}...", source.name);
    }

    fn send(&mut self, command: Command) -> bool {
        if self.running {
            self.status = "Outra operação ainda está em execução; cancele ou aguarde".into();
            return false;
        }
        let Some(worker) = &self.worker else {
            self.status = "Conecte um data source primeiro".into();
            return false;
        };
        if worker.commands.send(command).is_ok() {
            self.running = true;
            true
        } else {
            self.status = "Sessão encerrada; conecte novamente".into();
            false
        }
    }

    fn refresh_schema(&mut self) {
        let id = self.session_id;
        let _ = self.send(Command::Schema(id));
    }

    fn execute(&mut self, sql: String) {
        if sql.trim().is_empty() {
            return;
        }
        if !is_read_only(&sql) && self.confirm_destructive.as_deref() != Some(sql.as_str()) {
            self.confirm_destructive = Some(sql);
            return;
        }
        self.confirm_destructive = None;
        let id = self.session_id;
        if self.send(Command::Execute(id, sql.clone())) {
            self.push_history(format!("SQL: {}", phpide_core::redact::redact_text(&sql)));
        }
    }

    fn select_table(&mut self, table: String) {
        self.selected_table = Some(table);
        self.page = 0;
        self.pending = PendingChanges::default();
        self.edit_buffers.clear();
        self.request_grid();
    }

    fn request_grid(&mut self) {
        let Some(table) = self.selected_table.clone() else {
            return;
        };
        let id = self.session_id;
        let _ = self.send(Command::Grid(
            id,
            table,
            self.page,
            self.page_size,
            QueryOptions::default(),
        ));
    }

    fn apply_pending(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let id = self.session_id;
        let _ = self.send(Command::Apply(id, self.pending.clone()));
    }

    fn transaction(&mut self, action: TransactionAction) {
        let id = self.session_id;
        let _ = self.send(Command::Transaction(id, action));
    }

    fn push_history(&mut self, entry: String) {
        if self.history.len() >= 200 {
            self.history.remove(0);
        }
        self.history.push(entry);
    }

    pub fn poll(&mut self) {
        let mut events = Vec::new();
        if let Some(worker) = &self.worker {
            while let Ok(event) = worker.events.try_recv() {
                events.push(event);
            }
        }
        if let Some(receiver) = &self.test_receiver {
            while let Ok(event) = receiver.try_recv() {
                events.push(event);
            }
        }
        for event in events {
            match event {
                Event::Test(id, result) if id == self.test_id => {
                    self.running = false;
                    self.status = match result {
                        Ok(version) => format!("Conexão válida: {version}"),
                        Err(error) => format!("Test Connection falhou: {error}"),
                    };
                }
                Event::Connected(id, result) if id == self.session_id => {
                    self.running = false;
                    match result {
                        Ok(schema) => {
                            self.status = format!("Conectado; {} objeto(s)", schema.tables.len());
                            self.schema = Some(schema.clone());
                            if self.selected_table.is_none() {
                                if let Some(table) = schema.tables.first() {
                                    self.select_table(table.name.clone());
                                }
                            }
                        }
                        Err(error) => self.status = format!("Falha de conexão: {error}"),
                    }
                }
                Event::Schema(id, result) if id == self.session_id => {
                    self.running = false;
                    match result {
                        Ok(schema) => {
                            self.status = if schema.partial_errors.is_empty() {
                                "Explorer atualizado".into()
                            } else {
                                format!("Explorer parcial: {} erro(s)", schema.partial_errors.len())
                            };
                            self.schema = Some(schema);
                        }
                        Err(error) => self.status = format!("Refresh falhou: {error}"),
                    }
                }
                Event::Execute(id, result) if id == self.session_id => {
                    self.running = false;
                    match result {
                        Ok(ExecResult::Rows { columns, rows, .. }) => {
                            self.status = format!("{} linha(s)", rows.len());
                            self.grid = Some(GridPage {
                                columns,
                                rows,
                                offset: 0,
                                total: None,
                            });
                            self.selected_table = None;
                        }
                        Ok(ExecResult::Affected { count, message }) => {
                            self.status = format!("{count} linha(s) afetada(s): {message}");
                            self.push_history(self.status.clone());
                            if let Some(table) = self.selected_table.clone() {
                                self.selected_table = Some(table);
                                self.refresh_schema();
                            }
                        }
                        Err(error) => {
                            self.status = format!("SQL falhou: {error}");
                            self.push_history(self.status.clone());
                        }
                    }
                }
                Event::Grid(id, result) if id == self.session_id => {
                    self.running = false;
                    match result {
                        Ok(page) => {
                            self.status = format!("Página {} (total desconhecido)", self.page + 1);
                            self.grid = Some(page);
                        }
                        Err(error) => self.status = format!("Grade falhou: {error}"),
                    }
                }
                Event::Applied(id, result) if id == self.session_id => {
                    self.running = false;
                    match result {
                        Ok(count) => {
                            self.status = format!("{count} registro(s) atualizado(s)");
                            self.pending = PendingChanges::default();
                            self.edit_buffers.clear();
                            self.preview = None;
                            self.request_grid();
                        }
                        Err(error) => {
                            self.status = format!("Submit falhou; pendentes preservados: {error}");
                        }
                    }
                }
                Event::Transaction(id, action, result) if id == self.session_id => {
                    self.running = false;
                    match result {
                        Ok(()) => {
                            self.in_transaction = matches!(action, TransactionAction::Begin);
                            self.status = format!("Transação: {action:?} concluída");
                            if matches!(action, TransactionAction::Rollback) {
                                self.pending = PendingChanges::default();
                                self.edit_buffers.clear();
                                self.request_grid();
                            }
                        }
                        Err(error) => self.status = format!("Transação falhou: {error}"),
                    }
                }
                _ => {}
            }
        }
    }

    fn key_for(&self, table: &str, columns: &[String], row: &[Value]) -> Vec<(String, Value)> {
        let Some(table_meta) = self
            .schema
            .as_ref()
            .and_then(|schema| schema.tables.iter().find(|item| item.name == table))
        else {
            return Vec::new();
        };
        table_meta
            .columns
            .iter()
            .filter(|column| column.primary_key)
            .filter_map(|key| {
                columns
                    .iter()
                    .position(|column| column == &key.name)
                    .map(|index| (key.name.clone(), row[index].clone()))
            })
            .collect()
    }

    fn edit_cell(&mut self, row: usize, column: usize, text: String) {
        let Some(grid) = &self.grid else { return };
        let Some(table) = self.selected_table.clone() else {
            return;
        };
        let old = grid.rows[row][column].clone();
        let key = self.key_for(&table, &grid.columns, &grid.rows[row]);
        if key.is_empty() {
            self.status = "Edição exige chave primária introspectada".into();
            return;
        }
        let new = parse_edited_value(&old, &text);
        self.pending.push(CellChange {
            table,
            key,
            column: grid.columns[column].clone(),
            old,
            new,
        });
        self.edit_buffers.insert((row, column), text);
    }
}

impl Drop for DatabaseState {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn is_read_only(sql: &str) -> bool {
    split_statements(sql).iter().all(|statement| {
        let normalized = statement.trim_start().to_ascii_lowercase();
        [
            "select", "with", "pragma", "explain", "show", "describe", "values",
        ]
        .iter()
        .any(|prefix| normalized.starts_with(prefix))
    })
}

fn parse_edited_value(old: &Value, text: &str) -> Value {
    if text.eq_ignore_ascii_case("null") {
        return Value::Null;
    }
    match old {
        Value::Bool(_) => match text.to_ascii_lowercase().as_str() {
            "true" | "1" => Value::Bool(true),
            "false" | "0" => Value::Bool(false),
            _ => Value::Text(text.into()),
        },
        Value::Int(_) => text
            .parse()
            .map(Value::Int)
            .unwrap_or_else(|_| Value::Text(text.into())),
        Value::Float(_) => text
            .parse()
            .map(Value::Float)
            .unwrap_or_else(|_| Value::Text(text.into())),
        Value::Decimal(_) => Value::Decimal(text.into()),
        Value::Bytes(_) => Value::Text(text.into()),
        Value::Date(_) => Value::Date(text.into()),
        Value::Time(_) => Value::Time(text.into()),
        Value::Timestamp(_) => Value::Timestamp(text.into()),
        Value::Null | Value::Text(_) => text
            .parse::<i64>()
            .map(Value::Int)
            .unwrap_or_else(|_| Value::Text(text.into())),
    }
}

fn statement_ranges(sql: &str) -> Vec<(usize, usize)> {
    let chars: Vec<char> = sql.chars().collect();
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut index = 0;
    while index < chars.len() {
        let character = chars[index];
        if let Some(active) = quote {
            if character == active {
                if chars.get(index + 1) == Some(&active) {
                    index += 1;
                } else {
                    quote = None;
                }
            }
        } else {
            match character {
                '\'' | '"' | '`' => quote = Some(character),
                ';' => {
                    push_statement_range(&chars, start, index, &mut ranges);
                    start = index + 1;
                }
                _ => {}
            }
        }
        index += 1;
    }
    push_statement_range(&chars, start, chars.len(), &mut ranges);
    ranges
}

fn push_statement_range(
    chars: &[char],
    start: usize,
    end: usize,
    ranges: &mut Vec<(usize, usize)>,
) {
    let Some(first) = (start..end).find(|index| !chars[*index].is_whitespace()) else {
        return;
    };
    let last = (first..end)
        .rev()
        .find(|index| !chars[*index].is_whitespace())
        .unwrap()
        + 1;
    ranges.push((first, last));
}

pub fn sql_for_execution(sql: &str, selection: Option<(usize, usize)>, cursor: usize) -> String {
    if let Some((start, end)) = selection.filter(|(start, end)| start != end) {
        return sql
            .chars()
            .skip(start)
            .take(end.saturating_sub(start))
            .collect();
    }
    statement_ranges(sql)
        .into_iter()
        .find(|(start, end)| (*start..=*end).contains(&cursor))
        .map(|(start, end)| sql.chars().skip(start).take(end - start).collect())
        .unwrap_or_else(|| sql.trim().to_string())
}

fn show_form(state: &mut DatabaseState, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label("Nome");
        ui.text_edit_singleline(&mut state.form_name);
        ui.label("URL");
        ui.add(egui::TextEdit::singleline(&mut state.form_url).desired_width(280.0));
        ui.label("Credencial (opcional)");
        ui.add(
            egui::TextEdit::singleline(&mut state.form_password)
                .password(true)
                .desired_width(130.0),
        );
        if ui.button("Salvar data source").clicked() {
            state.save_source();
        }
    });
    ui.label(
        egui::RichText::new(format!(
            "Backend de segredos: {}. A URL nunca aceita user/password inline.",
            state.secrets.backend_name()
        ))
        .weak(),
    );
}

pub fn show(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    app.db.poll();
    let state = &mut app.db;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Data Sources and Drivers").strong());
        if ui
            .button(if state.show_form {
                "Ocultar formulário"
            } else {
                "Novo/Editar"
            })
            .clicked()
        {
            state.show_form = !state.show_form;
        }
        if ui.button("Test Connection").clicked() {
            state.test_connection();
        }
        if ui.button("Conectar").clicked() {
            state.connect();
        }
        if ui.button("Refresh").clicked() {
            state.refresh_schema();
        }
        if ui.button("Remover").clicked() {
            state.remove_source();
        }
        if state.running {
            ui.spinner();
            if ui.button("Cancel").clicked() {
                if let Some(worker) = &state.worker {
                    worker.cancel();
                }
                state.status = "Cancelamento solicitado...".into();
            }
        }
    });
    if state.show_form {
        show_form(state, ui);
    }
    ui.horizontal(|ui| {
        let mut selected = state.selected_id.clone();
        egui::ComboBox::from_id_salt("database-source")
            .selected_text(
                state
                    .selected_source()
                    .map(|source| source.name)
                    .unwrap_or_else(|| "Selecione".into()),
            )
            .show_ui(ui, |ui| {
                for source in &state.store.list {
                    ui.selectable_value(&mut selected, Some(source.id.clone()), &source.name);
                }
            });
        if selected != state.selected_id {
            state.selected_id = selected;
            state.sync_form();
        }
        ui.label(egui::RichText::new(&state.status).weak());
    });
    if let Some(sql) = state.confirm_destructive.clone() {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(
                egui::RichText::new("Confirme o statement potencialmente destrutivo:").strong(),
            );
            ui.monospace(&sql);
            ui.horizontal(|ui| {
                if ui.button("Executar statement destacado").clicked() {
                    state.execute(sql);
                }
                if ui.button("Cancelar").clicked() {
                    state.confirm_destructive = None;
                }
            });
        });
    }
    ui.columns(3, |columns| {
        columns[0].label(egui::RichText::new("Explorer").strong());
        if let Some(schema) = state.schema.clone() {
            for error in &schema.partial_errors {
                columns[0].label(egui::RichText::new(format!("Parcial: {error}")).weak());
            }
            egui::ScrollArea::vertical()
                .id_salt("database-explorer")
                .show(&mut columns[0], |ui| {
                    for table in schema.tables {
                        if ui
                            .selectable_label(
                                state.selected_table.as_deref() == Some(table.name.as_str()),
                                format!("{} ({})", table.name, table.kind),
                            )
                            .clicked()
                        {
                            state.select_table(table.name);
                        }
                    }
                });
        } else {
            columns[0].label("Conecte para introspecção.");
        }

        columns[1].label(egui::RichText::new("Query Console (Ctrl+Enter executa)").strong());
        let output = egui::TextEdit::multiline(&mut state.sql)
            .font(egui::TextStyle::Monospace)
            .desired_rows(7)
            .desired_width(f32::INFINITY)
            .show(&mut columns[1]);
        let execute_shortcut =
            columns[1].input(|input| input.key_pressed(egui::Key::Enter) && input.modifiers.ctrl);
        columns[1].horizontal(|ui| {
            if ui.button("Executar statement/seleção").clicked() || execute_shortcut {
                let selection = output.cursor_range.map(|range| {
                    let start = range
                        .primary
                        .ccursor
                        .index
                        .min(range.secondary.ccursor.index);
                    let end = range
                        .primary
                        .ccursor
                        .index
                        .max(range.secondary.ccursor.index);
                    (start, end)
                });
                let cursor = output
                    .cursor_range
                    .map(|range| range.primary.ccursor.index)
                    .unwrap_or(0);
                let sql = sql_for_execution(&state.sql, selection, cursor);
                state.execute(sql);
            }
        });
        columns[1].label(egui::RichText::new("History").strong());
        egui::ScrollArea::vertical()
            .id_salt("database-history")
            .max_height(90.0)
            .stick_to_bottom(true)
            .show(&mut columns[1], |ui| {
                for entry in state.history.iter().rev().take(20) {
                    ui.label(egui::RichText::new(entry).monospace().small());
                }
            });

        columns[2].horizontal(|ui| {
            if ui.button("◀").clicked() && state.page > 0 && !state.running {
                state.page -= 1;
                state.request_grid();
            }
            ui.label(format!("Página {}", state.page + 1));
            if ui.button("▶").clicked() && !state.running {
                state.page += 1;
                state.request_grid();
            }
            if ui.button("Preview DML").clicked() && !state.pending.is_empty() {
                if let Some(source) = state.selected_source() {
                    if let Some(kind) = DriverKind::from_name(&source.driver) {
                        state.preview = Some(edit::preview_dml(&state.pending, kind));
                    }
                }
            }
            if ui.button("Revert").clicked() {
                if let Some(table) = &state.selected_table {
                    state.pending.revert(table);
                }
                state.edit_buffers.clear();
                state.preview = None;
            }
            if ui
                .add_enabled(
                    !state.pending.is_empty() && !state.running,
                    egui::Button::new("Submit"),
                )
                .clicked()
            {
                state.apply_pending();
            }
        });
        columns[2].horizontal(|ui| {
            if ui.button("Begin").clicked() {
                state.transaction(TransactionAction::Begin);
            }
            if ui
                .add_enabled(state.in_transaction, egui::Button::new("Commit"))
                .clicked()
            {
                state.transaction(TransactionAction::Commit);
            }
            if ui
                .add_enabled(state.in_transaction, egui::Button::new("Rollback"))
                .clicked()
            {
                state.transaction(TransactionAction::Rollback);
            }
            ui.label(if state.in_transaction {
                "transação manual"
            } else {
                "auto"
            });
        });
        if let Some(preview) = state.preview.clone() {
            columns[2].label(egui::RichText::new(preview).monospace().small());
        }
        if let Some(grid) = state.grid.clone() {
            let table_name = state.selected_table.clone();
            egui::ScrollArea::both()
                .id_salt("database-grid")
                .max_height(230.0)
                .show(&mut columns[2], |ui| {
                    egui::Grid::new("database-result-grid")
                        .striped(true)
                        .show(ui, |ui| {
                            for column in &grid.columns {
                                ui.label(egui::RichText::new(column).strong());
                            }
                            ui.end_row();
                            for (row_index, row) in grid.rows.iter().enumerate() {
                                for (column_index, value) in row.iter().enumerate() {
                                    let key = state.key_for(
                                        table_name.as_deref().unwrap_or(""),
                                        &grid.columns,
                                        row,
                                    );
                                    if table_name.is_some() && !key.is_empty() {
                                        let text = state
                                            .edit_buffers
                                            .entry((row_index, column_index))
                                            .or_insert_with(|| value.display());
                                        let before = text.clone();
                                        if egui::TextEdit::singleline(text)
                                            .desired_width(90.0)
                                            .show(ui)
                                            .response
                                            .lost_focus()
                                            && *text != before
                                        {
                                            let edited = text.clone();
                                            state.edit_cell(row_index, column_index, edited);
                                        }
                                    } else {
                                        ui.label(value.display());
                                    }
                                }
                                ui.end_row();
                            }
                        });
                });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use phpide_db::secrets::SessionOnly;
    use std::time::Duration;

    fn wait_for(state: &mut DatabaseState, ready: impl Fn(&DatabaseState) -> bool) {
        let started = std::time::Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            state.poll();
            if ready(state) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("timeout: {}", state.status);
    }

    #[test]
    fn statement_selection_and_caret_are_exact() {
        let sql = "SELECT 1;\nSELECT ';';\nSELECT 3";
        assert_eq!(sql_for_execution(sql, None, 11), "SELECT ';'");
        assert_eq!(sql_for_execution(sql, Some((10, 20)), 0), "SELECT ';'");
        assert_eq!(sql_for_execution("SELECT 1", None, 0), "SELECT 1");
    }

    fn remote_controller_flow(url: String, sleep_sql: &str) {
        let dir = tempfile::tempdir().unwrap();
        let mut setup = driver_for(&url).unwrap().connect(&url).unwrap();
        setup
            .execute(
                "DROP TABLE IF EXISTS phpide_ui_gate",
                &QueryOptions::default(),
            )
            .unwrap();
        setup
            .execute(
                "CREATE TABLE phpide_ui_gate (id INTEGER PRIMARY KEY, name TEXT)",
                &QueryOptions::default(),
            )
            .unwrap();
        setup
            .execute(
                "INSERT INTO phpide_ui_gate VALUES (1, 'ana')",
                &QueryOptions::default(),
            )
            .unwrap();
        let mut state = DatabaseState::with_secret_store(dir.path(), Box::<SessionOnly>::default());
        state.form_name = "remote".into();
        let (scheme, rest) = url.split_once("://").unwrap();
        if let Some((auth, host)) = rest.split_once('@') {
            state.form_url = format!("{scheme}://{host}");
            state.form_password = if auth.contains(':') {
                auth.into()
            } else {
                format!("{auth}:")
            };
        } else {
            state.form_url = url.clone();
        }
        state.save_source();
        state.connect();
        wait_for(&mut state, |state| state.schema.is_some());
        state.select_table("phpide_ui_gate".into());
        wait_for(&mut state, |state| state.grid.is_some() && !state.running);
        state.execute(sleep_sql.into());
        std::thread::sleep(Duration::from_millis(100));
        state.worker.as_ref().unwrap().cancel();
        wait_for(&mut state, |state| !state.running);
        assert!(state.status.contains("cancelada"), "{}", state.status);
        state.execute("SELECT 1".into());
        wait_for(&mut state, |state| !state.running);
        state.select_table("phpide_ui_gate".into());
        wait_for(&mut state, |state| state.grid.is_some() && !state.running);
        setup
            .execute(
                "UPDATE phpide_ui_gate SET name = 'externo' WHERE id = 1",
                &QueryOptions::default(),
            )
            .unwrap();
        state.edit_cell(0, 1, "conflito".into());
        state.apply_pending();
        wait_for(&mut state, |state| !state.running);
        assert!(state.status.contains("conflito"), "{}", state.status);
        state.transaction(TransactionAction::Begin);
        wait_for(&mut state, |state| !state.running);
        state.transaction(TransactionAction::Rollback);
        wait_for(&mut state, |state| !state.running);
        state.shutdown();
        setup
            .execute("DROP TABLE phpide_ui_gate", &QueryOptions::default())
            .unwrap();
    }

    #[test]
    fn postgres_controller_flow_when_fixture_is_configured() {
        let Ok(url) = std::env::var("PHPIDE_TEST_POSTGRES_URL") else {
            eprintln!("BLOCKED: PHPIDE_TEST_POSTGRES_URL ausente");
            return;
        };
        remote_controller_flow(url, "SELECT pg_sleep(30)");
    }

    #[test]
    fn mysql_controller_flow_when_fixture_is_configured() {
        let Ok(url) = std::env::var("PHPIDE_TEST_MYSQL_URL") else {
            eprintln!("BLOCKED: PHPIDE_TEST_MYSQL_URL ausente");
            return;
        };
        remote_controller_flow(url, "SELECT SLEEP(30)");
    }

    #[test]
    fn sqlite_controller_full_flow_and_optimistic_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui.db");
        let url = format!("sqlite://{}", path.display());
        let mut state = DatabaseState::with_secret_store(dir.path(), Box::<SessionOnly>::default());
        state.form_name = "test".into();
        state.form_url = url.clone();
        state.save_source();
        assert_eq!(state.store.list.len(), 1);
        state.test_connection();
        wait_for(&mut state, |state| state.status.contains("Conexão válida"));
        state.connect();
        wait_for(&mut state, |state| state.schema.is_some());
        for sql in [
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)",
            "INSERT INTO users VALUES (1, 'ana'), (2, NULL)",
        ] {
            state.execute(sql.into());
            state.execute(sql.into());
            wait_for(&mut state, |state| !state.running);
        }
        state.refresh_schema();
        wait_for(&mut state, |state| !state.running);
        state.execute(
            "WITH RECURSIVE c(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM c) SELECT sum(x) FROM c"
                .into(),
        );
        std::thread::sleep(Duration::from_millis(50));
        state.worker.as_ref().unwrap().cancel();
        wait_for(&mut state, |state| !state.running);
        assert!(state.status.contains("cancelada"), "{}", state.status);
        state.execute("SELECT 1".into());
        wait_for(&mut state, |state| !state.running);
        state.select_table("users".into());
        wait_for(&mut state, |state| state.grid.is_some() && !state.running);
        state.edit_cell(0, 1, "ana maria".into());
        assert!(!state.pending.is_empty());
        state.preview = Some(edit::preview_dml(&state.pending, DriverKind::Sqlite));
        assert!(state.preview.as_ref().unwrap().contains("'ana'"));
        state.apply_pending();
        wait_for(&mut state, |state| {
            !state.running && state.pending.is_empty()
        });

        let mut external = driver_for(&url).unwrap().connect(&url).unwrap();
        external
            .execute(
                "UPDATE users SET name = 'externo' WHERE id = 1",
                &QueryOptions::default(),
            )
            .unwrap();
        state.edit_cell(0, 1, "conflito".into());
        state.apply_pending();
        wait_for(&mut state, |state| !state.running);
        assert!(state.status.contains("conflito"), "{}", state.status);
        assert!(!state.pending.is_empty());
        state.transaction(TransactionAction::Begin);
        wait_for(&mut state, |state| !state.running);
        assert!(state.in_transaction);
        state.transaction(TransactionAction::Rollback);
        wait_for(&mut state, |state| {
            !state.running && state.pending.is_empty()
        });
        state.shutdown();
    }
}
