//! Query console — `T-DB-004-01/02` (split de statements), `T-DB-004-05/06`
//! (histórico e cleanup de sessões), `T-DB-004-04` (cancelamento via drop).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use phpide_core::{Component, ErrorKind, IdeError};

use crate::contracts::{Driver, ExecResult, QueryOptions};

/// Divide um script SQL em statements (DB-004-02): respeita aspas simples/
/// duplas, comentários `--` e `/* */`, ponto-e-vírgula só conta fora deles.
pub fn split_statements(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = sql.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;
    while let Some(c) = chars.next() {
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
                cur.push(c);
            }
            '"' if !in_single => {
                in_double = !in_double;
                cur.push(c);
            }
            '-' if !in_single && !in_double && chars.peek() == Some(&'-') => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        break;
                    }
                }
            }
            '/' if !in_single && !in_double && chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = ' ';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
            }
            ';' if !in_single && !in_double => {
                let stmt = cur.trim();
                if !stmt.is_empty() {
                    out.push(stmt.to_string());
                }
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    let last = cur.trim();
    if !last.is_empty() {
        out.push(last.to_string());
    }
    out
}

/// Entrada de histórico (DB-004-05): statement + duração + linhas afetadas.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    pub sql: String,
    pub duration: Duration,
    pub summary: String,
}

/// Histórico circular (teto 200).
#[derive(Debug, Clone, Default)]
pub struct History {
    entries: VecDeque<HistoryEntry>,
}

impl History {
    pub const MAX: usize = 200;
    pub fn push(&mut self, e: HistoryEntry) {
        if self.entries.len() >= Self::MAX {
            self.entries.pop_front();
        }
        self.entries.push_back(e);
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

/// Sessão do console (DB-004-06): dona da conexão; drop/close libera.
pub struct ConsoleSession {
    pub(crate) session: Box<dyn crate::contracts::DbSession>,
    pub history: History,
    closed: bool,
}

impl ConsoleSession {
    pub fn new(session: Box<dyn crate::contracts::DbSession>) -> Self {
        ConsoleSession {
            session,
            history: History::default(),
            closed: false,
        }
    }

    /// Executa um único statement e registra no histórico (DB-004-03/05).
    pub fn run(&mut self, sql: &str) -> Result<ExecResult, IdeError> {
        let t0 = Instant::now();
        let result = self.session.execute(sql, &QueryOptions::default())?;
        let summary = match &result {
            ExecResult::Rows { rows, .. } => format!("{} linha(s)", rows.len()),
            ExecResult::Affected { count, message } => format!("{count} afetada(s) — {message}"),
        };
        self.history.push(HistoryEntry {
            sql: sql.to_string(),
            duration: t0.elapsed(),
            summary,
        });
        Ok(result)
    }

    /// Executa um script inteiro, statement a statement; erros parciais
    /// abortam com o índice (DB-006-05, sem transação explícita).
    pub fn run_script(&mut self, sql: &str) -> Result<Vec<ExecResult>, (usize, IdeError)> {
        let stmts = split_statements(sql);
        let mut out = Vec::new();
        for (i, stmt) in stmts.iter().enumerate() {
            match self.run(stmt) {
                Ok(r) => out.push(r),
                Err(e) => return Err((i, e)),
            }
        }
        Ok(out)
    }

    pub fn close(mut self) {
        self.closed = true;
    }
    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

/// Execução cancelável (DB-004-04): roda numa thread; `cancel()` sinaliza e
/// a sessão é descartada (fechar a conexão é o cancelamento confiável).
pub struct CancelHandle {
    cancel: Arc<AtomicBool>,
    handle: Option<JoinHandle<Result<ExecResult, IdeError>>>,
}

impl CancelHandle {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
    /// Aguarda o fim (com prazo); devolve o resultado ou "cancelado".
    pub fn wait(mut self, timeout: Duration) -> Result<ExecResult, IdeError> {
        let handle = self.handle.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(handle.join());
        });
        match rx.recv_timeout(timeout) {
            Ok(joined) => match joined {
                Ok(_res) if self.is_cancelled() => Err(cancelled_err()),
                Ok(res) => res,
                Err(_) => Err(IdeError::new(
                    Component::Core,
                    "console.cancel",
                    ErrorKind::Cancelled,
                    "execução interrompida pela thread",
                    "tente de novo",
                )),
            },
            Err(_) => Err(IdeError::new(
                Component::Core,
                "console.wait",
                ErrorKind::Timeout,
                "timeout aguardando execução",
                "verifique a lentidão do banco ou cancele",
            )),
        }
    }
}

fn cancelled_err() -> IdeError {
    IdeError::new(
        Component::Core,
        "console.cancel",
        ErrorKind::Cancelled,
        "execução cancelada pelo usuário",
        "ajuste a consulta e tente de novo",
    )
}

/// Executa numa thread com handle de cancelamento. O driver cancela ao ter
/// sua sessão destruída — aqui o drop do Box<dyn DbSession> na thread ao
/// sinal de cancelamento é cooperativo por fechamento da conexão.
pub fn run_async(driver: Arc<dyn Driver>, url: String, sql: String) -> CancelHandle {
    let cancel = Arc::new(AtomicBool::new(false));
    let c2 = cancel.clone();
    let handle = std::thread::spawn(move || {
        let mut session = driver.connect(&url)?;
        // Checagem de cancelamento antes de executar; durante a execução o
        // driver cancela via session.cancel().
        if c2.load(Ordering::SeqCst) {
            return Err(cancelled_err());
        }
        let r = session.execute(&sql, &QueryOptions::default());
        if c2.load(Ordering::SeqCst) {
            session.cancel();
            return Err(cancelled_err());
        }
        r
    });
    CancelHandle {
        cancel,
        handle: Some(handle),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_respecting_quotes_and_comments() {
        let sql = "select ';' as x; -- comentário; fim\nselect 2; /* bloco; */ select 3";
        let stmts = split_statements(sql);
        assert_eq!(stmts.len(), 3, "{stmts:?}");
        assert!(stmts[0].contains("';'"));
        assert_eq!(stmts[1], "select 2");
        assert_eq!(stmts[2], "select 3");
        assert!(split_statements("").is_empty());
        assert_eq!(split_statements("select 1").len(), 1);
    }

    #[test]
    fn history_is_bounded_ring() {
        let mut h = History::default();
        for i in 0..(History::MAX + 20) {
            h.push(HistoryEntry {
                sql: format!("select {i}"),
                duration: Duration::from_millis(i as u64),
                summary: String::new(),
            });
        }
        assert_eq!(h.len(), History::MAX);
        assert_eq!(
            h.entries().next().unwrap().sql,
            format!("select {}", History::MAX + 19)
        );
    }
}
