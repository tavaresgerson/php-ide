//! Driver SQLite real — `T-DB-001-04` (rusqlite bundled, sem depender de
//! libsqlite3 do sistema). Cobre integração real do contrato: introspection,
//! execução, cancelamento (interrupt), transações e edição por chave.

use std::path::PathBuf;

use phpide_core::{Component, ErrorKind, IdeError};

use crate::contracts::*;
use crate::value::Value;

pub struct SqliteDriver;
pub struct SqliteSession {
    conn: rusqlite::Connection,
    interrupt: std::sync::Arc<rusqlite::InterruptHandle>,
    in_tx: bool,
}

impl SqliteDriver {
    pub fn boxed() -> Box<dyn Driver> {
        Box::new(SqliteDriver)
    }
}

fn io(op: &str, e: impl std::fmt::Display) -> IdeError {
    IdeError::new(
        Component::Db,
        op.to_string(),
        ErrorKind::Io,
        format!("SQLite falhou em {op}"),
        "verifique o caminho do arquivo e as permissões",
    )
    .with_detail(e.to_string())
}

impl Driver for SqliteDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Sqlite
    }
    fn test(&self, url: &str) -> Result<String, IdeError> {
        let (_, mut p) = parse_url(url)?;
        let path = p.remove("path").unwrap_or_default();
        // Memória sempre ok; arquivo exige diretório acessível.
        if path != ":memory:" && !path.is_empty() {
            let pb = PathBuf::from(&path);
            if let Some(dir) = pb.parent() {
                if !dir.exists() {
                    return Err(io(
                        "test",
                        format!("diretório inexistente: {}", dir.display()),
                    ));
                }
            }
        }
        let conn = rusqlite::Connection::open(if path.is_empty() { ":memory:" } else { &path })
            .map_err(|e| io("test connect", e))?;
        conn.query_row("SELECT sqlite_version()", [], |r| r.get::<_, String>(0))
            .map_err(|e| io("test ping", e))
            .map(|v| format!("SQLite {v}"))
    }
    fn connect(&self, url: &str) -> Result<Box<dyn DbSession>, IdeError> {
        let (_, mut p) = parse_url(url)?;
        let path = p.remove("path").unwrap_or_default();
        let conn = rusqlite::Connection::open(if path.is_empty() { ":memory:" } else { &path })
            .map_err(|e| io("connect", e))?;
        let interrupt = std::sync::Arc::new(conn.get_interrupt_handle());
        Ok(Box::new(SqliteSession {
            conn,
            interrupt,
            in_tx: false,
        }))
    }
}

fn val(v: &rusqlite::types::Value) -> Value {
    match v {
        rusqlite::types::Value::Null => Value::Null,
        rusqlite::types::Value::Integer(i) => Value::Int(*i),
        rusqlite::types::Value::Real(f) => Value::Float(*f),
        rusqlite::types::Value::Text(t) => Value::Text(t.clone()),
        rusqlite::types::Value::Blob(b) => Value::Bytes(b.clone()),
    }
}

impl DbSession for SqliteSession {
    fn kind(&self) -> DriverKind {
        DriverKind::Sqlite
    }

    fn schema(&mut self) -> Result<SchemaMeta, IdeError> {
        let mut meta = SchemaMeta::default();
        let mut stmt = self
            .conn
            .prepare("SELECT name, type FROM sqlite_master WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .map_err(|e| io("schema", e))?;
        let names: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| io("schema list", e))?
            .filter_map(|r| r.ok())
            .collect();
        for (name, kind) in names {
            let mut cols = Vec::new();
            match self.conn.prepare(&format!(
                "PRAGMA table_info({})",
                crate::grid::quote_ident(DriverKind::Sqlite, &name)
            )) {
                Ok(mut cstmt) => {
                    let mapped = cstmt.query_map([], |r| {
                        Ok(ColumnMeta {
                            name: r.get(1)?,
                            type_name: r.get(2)?,
                            nullable: r.get::<_, i64>(3)? == 0,
                            primary_key: r.get::<_, i64>(5)? > 0,
                        })
                    });
                    match mapped {
                        Ok(it) => cols = it.filter_map(|c| c.ok()).collect(),
                        Err(e) => meta.partial_errors.push(format!("{name}: {e}")),
                    }
                }
                Err(e) => meta.partial_errors.push(format!("{name}: {e}")),
            }
            meta.tables.push(TableMeta {
                name,
                kind,
                columns: cols,
            });
        }
        Ok(meta)
    }

    fn execute(&mut self, sql: &str, opts: &QueryOptions) -> Result<ExecResult, IdeError> {
        let trimmed = sql.trim().to_ascii_lowercase();
        if trimmed.starts_with("select")
            || trimmed.starts_with("with")
            || trimmed.starts_with("pragma")
        {
            let full = if let Some(limit) = opts.limit {
                format!(
                    "{} LIMIT {} OFFSET {}",
                    sql.trim_end_matches(';'),
                    limit,
                    opts.offset.unwrap_or(0)
                )
            } else {
                sql.to_string()
            };
            let mut stmt = self.conn.prepare(&full).map_err(|e| io("prepare", e))?;
            let cols: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
            let ncol = stmt.column_count();
            let mut rows = Vec::new();
            let mut q = stmt.query([]).map_err(|e| io("query", e))?;
            while let Some(r) = q.next().map_err(|e| io("next", e))? {
                let mut row = Vec::with_capacity(ncol);
                for i in 0..ncol {
                    let v: rusqlite::types::Value = r.get(i).map_err(|e| io("get", e))?;
                    row.push(val(&v));
                }
                rows.push(row);
            }
            let total = Some(rows.len() as u64);
            Ok(ExecResult::Rows {
                columns: cols,
                rows,
                total,
            })
        } else {
            let affected = self.conn.execute(sql, []).map_err(|e| io("execute", e))?;
            Ok(ExecResult::Affected {
                count: affected as u64,
                message: "ok".into(),
            })
        }
    }

    fn cancel(&self) {
        self.interrupt.interrupt();
    }

    fn cancellation_callback(&self) -> Option<std::sync::Arc<dyn Fn() + Send + Sync>> {
        let interrupt = self.interrupt.clone();
        Some(std::sync::Arc::new(move || interrupt.interrupt()))
    }

    fn in_transaction(&self) -> bool {
        self.in_tx
    }
    fn begin(&mut self) -> Result<(), IdeError> {
        self.conn
            .execute_batch("BEGIN")
            .map_err(|e| io("begin", e))?;
        self.in_tx = true;
        Ok(())
    }
    fn commit(&mut self) -> Result<(), IdeError> {
        self.conn
            .execute_batch("COMMIT")
            .map_err(|e| io("commit", e))?;
        self.in_tx = false;
        Ok(())
    }
    fn rollback(&mut self) -> Result<(), IdeError> {
        self.conn
            .execute_batch("ROLLBACK")
            .map_err(|e| io("rollback", e))?;
        self.in_tx = false;
        Ok(())
    }

    fn update_row(&mut self, update: &RowUpdate) -> Result<u64, IdeError> {
        if update.changes.is_empty() || update.key.is_empty() {
            return Err(IdeError::new(
                Component::Db,
                "sqlite.update",
                ErrorKind::InvalidInput,
                "edição exige chave e ao menos uma coluna",
                "selecione uma linha pela chave primária",
            ));
        }
        let table = crate::grid::quote_ident(DriverKind::Sqlite, &update.table);
        let sets = update
            .changes
            .iter()
            .enumerate()
            .map(|(index, change)| {
                format!(
                    "{} = ?{}",
                    crate::grid::quote_ident(DriverKind::Sqlite, &change.column),
                    index + 1
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let mut params: Vec<rusqlite::types::Value> = update
            .changes
            .iter()
            .map(|change| change.new.to_sql())
            .collect();
        let mut conditions = Vec::new();
        let mut append_condition = |column: &str, value: &Value| {
            let quoted = crate::grid::quote_ident(DriverKind::Sqlite, column);
            if value.is_null() {
                conditions.push(format!("{quoted} IS NULL"));
            } else {
                params.push(value.to_sql());
                conditions.push(format!("{quoted} = ?{}", params.len()));
            }
        };
        for (column, value) in &update.key {
            append_condition(column, value);
        }
        for change in &update.changes {
            append_condition(&change.column, &change.old);
        }
        let sql = format!(
            "UPDATE {table} SET {sets} WHERE {}",
            conditions.join(" AND ")
        );
        let affected = self
            .conn
            .execute(&sql, rusqlite::params_from_iter(params))
            .map_err(|error| io("update", error))?;
        Ok(affected as u64)
    }
}
