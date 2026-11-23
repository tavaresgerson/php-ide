use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use phpide_core::{Component, ErrorKind, IdeError};

use crate::contracts::{Driver, ExecResult, QueryOptions};

pub fn split_statements(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = sql.chars().peekable();
    let mut quote = None;
    while let Some(character) = chars.next() {
        if let Some(active) = quote {
            current.push(character);
            if character == active {
                if chars.peek() == Some(&active) {
                    current.push(chars.next().unwrap());
                } else {
                    quote = None;
                }
            }
            continue;
        }
        match character {
            '\'' | '"' | '`' => {
                quote = Some(character);
                current.push(character);
            }
            '-' if chars.peek() == Some(&'-') => {
                chars.next();
                for next in chars.by_ref() {
                    if next == '\n' {
                        current.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = ' ';
                for next in chars.by_ref() {
                    if previous == '*' && next == '/' {
                        break;
                    }
                    previous = next;
                }
            }
            ';' => {
                let statement = current.trim();
                if !statement.is_empty() {
                    out.push(statement.to_string());
                }
                current.clear();
            }
            _ => current.push(character),
        }
    }
    let statement = current.trim();
    if !statement.is_empty() {
        out.push(statement.to_string());
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    pub sql: String,
    pub duration: Duration,
    pub summary: String,
}

#[derive(Debug, Clone, Default)]
pub struct History {
    entries: VecDeque<HistoryEntry>,
}

impl History {
    pub const MAX: usize = 200;

    pub fn push(&mut self, entry: HistoryEntry) {
        if self.entries.len() >= Self::MAX {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    pub fn entries(&self) -> impl Iterator<Item = &HistoryEntry> {
        self.entries.iter().rev()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

pub struct ConsoleSession {
    pub(crate) session: Box<dyn crate::contracts::DbSession>,
    pub history: History,
    closed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptErrorPolicy {
    Stop,
    Continue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptPolicy {
    pub transactional: bool,
    pub on_error: ScriptErrorPolicy,
}

impl Default for ScriptPolicy {
    fn default() -> Self {
        Self {
            transactional: true,
            on_error: ScriptErrorPolicy::Stop,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptStatementStatus {
    Executed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone)]
pub struct ScriptStatementReport {
    pub index: usize,
    pub sql: String,
    pub status: ScriptStatementStatus,
    pub result: Option<ExecResult>,
    pub duration: Duration,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ScriptReport {
    pub statements: Vec<ScriptStatementReport>,
    pub transactional: bool,
    pub rolled_back: bool,
    pub committed: bool,
    pub rollback_error: Option<String>,
    pub implicit_commit_warning: Option<String>,
}

impl ScriptReport {
    pub fn succeeded(&self) -> bool {
        self.rollback_error.is_none()
            && self
                .statements
                .iter()
                .all(|statement| statement.status == ScriptStatementStatus::Executed)
    }
}

impl ConsoleSession {
    pub fn new(session: Box<dyn crate::contracts::DbSession>) -> Self {
        Self {
            session,
            history: History::default(),
            closed: false,
        }
    }

    pub fn run(&mut self, sql: &str) -> Result<ExecResult, IdeError> {
        let started = Instant::now();
        let result = self.session.execute(sql, &QueryOptions::default());
        let summary = match &result {
            Ok(ExecResult::Rows { rows, .. }) => format!("{} linha(s)", rows.len()),
            Ok(ExecResult::Affected { count, message }) => {
                format!("{count} afetada(s) — {message}")
            }
            Err(error) => format!("erro: {error}"),
        };
        self.history.push(HistoryEntry {
            sql: sql.to_string(),
            duration: started.elapsed(),
            summary,
        });
        result
    }

    pub fn run_script(
        &mut self,
        sql: &str,
        policy: ScriptPolicy,
    ) -> Result<ScriptReport, IdeError> {
        let statements = split_statements(sql);
        let transactional = policy.transactional && !statements.is_empty();
        if transactional && !self.session.in_transaction() {
            self.session.begin()?;
        }
        let implicit_commit_warning = (transactional && self.session.kind() == crate::DriverKind::MySql)
            .then(|| "MySQL/MariaDB pode fazer commit implícito em DDL; atomicidade não é garantida para esses statements".to_string());
        let mut report = ScriptReport {
            statements: Vec::with_capacity(statements.len()),
            transactional,
            rolled_back: false,
            committed: false,
            rollback_error: None,
            implicit_commit_warning,
        };
        let mut failed = false;
        for (index, statement) in statements.iter().enumerate() {
            if failed && policy.on_error == ScriptErrorPolicy::Stop {
                report.statements.push(ScriptStatementReport {
                    index,
                    sql: statement.clone(),
                    status: ScriptStatementStatus::Skipped,
                    result: None,
                    duration: Duration::ZERO,
                    error: Some("ignorado após falha anterior".into()),
                });
                continue;
            }
            let started = Instant::now();
            match self.run(statement) {
                Ok(result) => report.statements.push(ScriptStatementReport {
                    index,
                    sql: statement.clone(),
                    status: ScriptStatementStatus::Executed,
                    result: Some(result),
                    duration: started.elapsed(),
                    error: None,
                }),
                Err(error) => {
                    failed = true;
                    report.statements.push(ScriptStatementReport {
                        index,
                        sql: statement.clone(),
                        status: ScriptStatementStatus::Failed,
                        result: None,
                        duration: started.elapsed(),
                        error: Some(error.to_string()),
                    });
                }
            }
        }
        if transactional {
            if failed {
                match self.session.rollback() {
                    Ok(()) => report.rolled_back = true,
                    Err(error) => report.rollback_error = Some(error.log_line()),
                }
            } else {
                self.session.commit()?;
                report.committed = true;
            }
        }
        Ok(report)
    }

    pub fn close(mut self) {
        self.session.cancel();
        self.closed = true;
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

type CancelCallback = Arc<dyn Fn() + Send + Sync>;

pub struct CancelHandle {
    requested: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    interrupt: Arc<Mutex<Option<CancelCallback>>>,
    receiver: Option<std::sync::mpsc::Receiver<Result<ExecResult, IdeError>>>,
    handle: Option<JoinHandle<()>>,
}

impl CancelHandle {
    pub fn cancel(&self) {
        if !self.finished.load(Ordering::SeqCst) {
            self.requested.store(true, Ordering::SeqCst);
            if let Some(interrupt) = self.interrupt.lock().unwrap().clone() {
                interrupt();
            }
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }

    pub fn try_result(&mut self) -> Option<Result<ExecResult, IdeError>> {
        let result = self.receiver.as_ref()?.try_recv().ok()?;
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        self.receiver = None;
        Some(if self.is_cancelled() && result.is_err() {
            Err(cancelled_err())
        } else {
            result
        })
    }

    pub fn wait(mut self, timeout: Duration) -> Result<ExecResult, IdeError> {
        let receiver = self.receiver.take().unwrap();
        match receiver.recv_timeout(timeout) {
            Ok(result) => {
                if let Some(handle) = self.handle.take() {
                    let _ = handle.join();
                }
                if self.is_cancelled() && result.is_err() {
                    Err(cancelled_err())
                } else {
                    result
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                self.cancel();
                let outcome = receiver.recv();
                if let Some(handle) = self.handle.take() {
                    let _ = handle.join();
                }
                Err(IdeError::new(
                    Component::Db,
                    "console.wait",
                    ErrorKind::Timeout,
                    "timeout aguardando execução; cancelamento confirmado",
                    "ajuste a consulta ou aumente o prazo",
                )
                .with_detail(format!("worker finalizado: {outcome:?}")))
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(IdeError::new(
                Component::Db,
                "console.wait",
                ErrorKind::Other,
                "worker terminou sem resultado",
                "verifique o log do driver",
            )),
        }
    }
}

impl Drop for CancelHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn cancelled_err() -> IdeError {
    IdeError::new(
        Component::Db,
        "console.cancel",
        ErrorKind::Cancelled,
        "execução cancelada pelo usuário",
        "ajuste a consulta e tente de novo",
    )
}

pub fn run_async(driver: Arc<dyn Driver>, url: String, sql: String) -> CancelHandle {
    let requested = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(AtomicBool::new(false));
    let interrupt: Arc<Mutex<Option<CancelCallback>>> = Arc::new(Mutex::new(None));
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker_requested = requested.clone();
    let worker_finished = finished.clone();
    let worker_interrupt = interrupt.clone();
    let handle = std::thread::spawn(move || {
        let result = (|| {
            let mut session = driver.connect(&url)?;
            *worker_interrupt.lock().unwrap() = session.cancellation_callback();
            if worker_requested.load(Ordering::SeqCst) {
                return Err(cancelled_err());
            }
            session.execute(&sql, &QueryOptions::default())
        })();
        worker_finished.store(true, Ordering::SeqCst);
        let _ = sender.send(result);
    });
    CancelHandle {
        requested,
        finished,
        interrupt,
        receiver: Some(receiver),
        handle: Some(handle),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::sqlite::SqliteDriver;
    use crate::value::Value;

    fn memory_console() -> ConsoleSession {
        let driver = SqliteDriver;
        let mut session = driver.connect("sqlite://:memory:").unwrap();
        session
            .execute(
                "CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT)",
                &QueryOptions::default(),
            )
            .unwrap();
        ConsoleSession::new(session)
    }

    fn count(console: &mut ConsoleSession) -> i64 {
        let ExecResult::Rows { rows, .. } = console.run("SELECT count(*) FROM items").unwrap()
        else {
            panic!("count")
        };
        match rows[0][0] {
            Value::Int(value) => value,
            _ => panic!("integer"),
        }
    }

    #[test]
    fn splits_respecting_quotes_and_comments() {
        let sql =
            "select ';' as x; -- comentário; fim\nselect 'it''s;ok'; /* bloco; */ select `a;b`";
        let statements = split_statements(sql);
        assert_eq!(statements.len(), 3, "{statements:?}");
        assert_eq!(statements[1], "select 'it''s;ok'");
        assert_eq!(statements[2], "select `a;b`");
        assert!(split_statements("").is_empty());
    }

    #[test]
    fn history_is_bounded_ring() {
        let mut history = History::default();
        for index in 0..(History::MAX + 20) {
            history.push(HistoryEntry {
                sql: format!("select {index}"),
                duration: Duration::from_millis(index as u64),
                summary: String::new(),
            });
        }
        assert_eq!(history.len(), History::MAX);
    }

    #[test]
    fn transactional_script_rolls_back_and_reports_each_statement() {
        let mut console = memory_console();
        let report = console
            .run_script(
                "INSERT INTO items VALUES (1, 'a'); INSERT INTO items VALUES (1, 'duplicate'); INSERT INTO items VALUES (2, 'c');",
                ScriptPolicy::default(),
            )
            .unwrap();
        assert!(report.rolled_back);
        assert!(!report.committed);
        assert_eq!(report.statements[0].status, ScriptStatementStatus::Executed);
        assert_eq!(report.statements[1].status, ScriptStatementStatus::Failed);
        assert_eq!(report.statements[2].status, ScriptStatementStatus::Skipped);
        assert_eq!(count(&mut console), 0);
        assert_eq!(console.history.len(), 3);
    }

    #[test]
    fn non_transactional_continue_reports_exact_persistence() {
        let mut console = memory_console();
        let report = console
            .run_script(
                "INSERT INTO items VALUES (1, 'a'); INSERT INTO items VALUES (1, 'duplicate'); INSERT INTO items VALUES (2, 'c');",
                ScriptPolicy { transactional: false, on_error: ScriptErrorPolicy::Continue },
            )
            .unwrap();
        assert!(!report.succeeded());
        assert!(!report.rolled_back);
        assert_eq!(report.statements[2].status, ScriptStatementStatus::Executed);
        assert_eq!(count(&mut console), 2);
    }

    #[test]
    fn empty_and_ddl_scripts_are_explicit() {
        let mut console = memory_console();
        let empty = console
            .run_script("-- vazio\n/* nada */", ScriptPolicy::default())
            .unwrap();
        assert!(empty.statements.is_empty());
        let ddl = console
            .run_script(
                "CREATE TABLE other (id INTEGER); invalid sql;",
                ScriptPolicy::default(),
            )
            .unwrap();
        assert!(ddl.rolled_back);
        assert!(console.run("SELECT * FROM other").is_err());
    }

    #[test]
    fn rollback_failure_is_visible_in_script_report() {
        let driver = crate::drivers::fake::FakeDriver::default();
        driver.fail_next_rollback();
        let mut console = ConsoleSession::new(driver.connect("fake://").unwrap());
        let report = console
            .run_script("SELECT 1; invalid; SELECT 2", ScriptPolicy::default())
            .unwrap();
        assert!(report.rollback_error.is_some());
        assert!(!report.rolled_back);
        assert!(!report.succeeded());
    }

    #[test]
    fn sqlite_interrupts_running_query_and_connection_is_reusable() {
        let driver = SqliteDriver;
        let mut session = driver.connect("sqlite://:memory:").unwrap();
        let interrupt = session.cancellation_callback().unwrap();
        let query =
            "WITH RECURSIVE c(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM c) SELECT sum(x) FROM c";
        let result = std::thread::scope(|scope| {
            let worker = scope.spawn(|| session.execute(query, &QueryOptions::default()));
            std::thread::sleep(Duration::from_millis(50));
            interrupt();
            worker.join().unwrap()
        });
        assert!(result.is_err());
        let reused = session
            .execute("SELECT 1", &QueryOptions::default())
            .unwrap();
        assert!(matches!(reused, ExecResult::Rows { .. }));
    }

    #[test]
    fn async_cancel_is_repeatable_and_finished_query_is_not_mislabeled() {
        let driver: Arc<dyn Driver> = Arc::new(SqliteDriver);
        let done = run_async(
            driver.clone(),
            "sqlite://:memory:".into(),
            "SELECT 1".into(),
        );
        assert!(done.wait(Duration::from_secs(2)).is_ok());

        let running = run_async(
            driver,
            "sqlite://:memory:".into(),
            "WITH RECURSIVE c(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM c) SELECT sum(x) FROM c"
                .into(),
        );
        std::thread::sleep(Duration::from_millis(50));
        running.cancel();
        running.cancel();
        let error = running.wait(Duration::from_secs(2)).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Cancelled);
    }
}
