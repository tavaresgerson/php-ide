use std::collections::BTreeMap;

use mysql::prelude::Queryable;
use mysql::{Conn, Opts};
use mysql_common::constants::ColumnType;

use phpide_core::{Component, ErrorKind, IdeError};

use crate::contracts::*;
use crate::value::Value;

pub struct MysqlDriver;

pub struct MysqlSession {
    conn: Conn,
    url: String,
    connection_id: u32,
    in_tx: bool,
}

impl MysqlDriver {
    pub fn boxed() -> Box<dyn Driver> {
        Box::new(Self)
    }
}

fn db_error(operation: &str, error: impl std::fmt::Display) -> IdeError {
    let detail = error.to_string();
    let lower = detail.to_ascii_lowercase();
    let (kind, action) = if lower.contains("access denied") || lower.contains("password") {
        (
            ErrorKind::PermissionDenied,
            "verifique usuário e senha do data source",
        )
    } else if lower.contains("connection") || lower.contains("connect") || lower.contains("timeout")
    {
        (
            ErrorKind::Io,
            "verifique host, porta e se MySQL/MariaDB está aceitando conexões",
        )
    } else {
        (ErrorKind::Other, "verifique o SQL e o estado da conexão")
    };
    IdeError::new(
        Component::Db,
        operation.to_string(),
        kind,
        format!("MySQL/MariaDB falhou em {operation}"),
        action,
    )
    .with_detail(detail)
}

fn connect(url: &str) -> Result<Conn, IdeError> {
    let opts = Opts::from_url(url).map_err(|error| db_error("mysql.url", error))?;
    Conn::new(opts).map_err(|error| db_error("mysql.connect", error))
}

impl Driver for MysqlDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::MySql
    }

    fn test(&self, url: &str) -> Result<String, IdeError> {
        let mut conn = connect(url)?;
        let version: Option<String> = conn
            .query_first::<String, _>("SELECT VERSION()")
            .map_err(|error| db_error("mysql.test", error))?;
        Ok(format!("MySQL/MariaDB {}", version.unwrap_or_default()))
    }

    fn connect(&self, url: &str) -> Result<Box<dyn DbSession>, IdeError> {
        let mut conn = connect(url)?;
        conn.ping().map_err(|error| db_error("mysql.ping", error))?;
        let connection_id = conn.connection_id();
        Ok(Box::new(MysqlSession {
            conn,
            url: url.to_string(),
            connection_id,
            in_tx: false,
        }))
    }
}

fn mysql_value(value: &Value) -> mysql::Value {
    match value {
        Value::Null => mysql::Value::NULL,
        Value::Bool(value) => mysql::Value::Int(*value as i64),
        Value::Int(value) => mysql::Value::Int(*value),
        Value::Float(value) => mysql::Value::Double(*value),
        Value::Decimal(value) | Value::Text(value) => {
            mysql::Value::Bytes(value.as_bytes().to_vec())
        }
        Value::Bytes(value) => mysql::Value::Bytes(value.clone()),
        Value::Date(value) | Value::Time(value) | Value::Timestamp(value) => {
            mysql::Value::Bytes(value.as_bytes().to_vec())
        }
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn row_value(value: mysql::Value, column: &mysql::Column) -> Value {
    let kind = column.column_type();
    match value {
        mysql::Value::NULL => Value::Null,
        mysql::Value::Int(value) => Value::Int(value),
        mysql::Value::UInt(value) => i64::try_from(value)
            .map(Value::Int)
            .unwrap_or_else(|_| Value::Decimal(value.to_string())),
        mysql::Value::Float(value) => Value::Float(value as f64),
        mysql::Value::Double(value) => Value::Float(value),
        mysql::Value::Bytes(bytes) => match kind {
            ColumnType::MYSQL_TYPE_DECIMAL | ColumnType::MYSQL_TYPE_NEWDECIMAL => {
                Value::Decimal(text(&bytes))
            }
            ColumnType::MYSQL_TYPE_DATE => Value::Date(text(&bytes)),
            ColumnType::MYSQL_TYPE_TIME => Value::Time(text(&bytes)),
            ColumnType::MYSQL_TYPE_DATETIME | ColumnType::MYSQL_TYPE_TIMESTAMP => {
                Value::Timestamp(text(&bytes))
            }
            ColumnType::MYSQL_TYPE_TINY
            | ColumnType::MYSQL_TYPE_SHORT
            | ColumnType::MYSQL_TYPE_INT24
            | ColumnType::MYSQL_TYPE_LONG
            | ColumnType::MYSQL_TYPE_LONGLONG
            | ColumnType::MYSQL_TYPE_YEAR => text(&bytes)
                .parse()
                .map(Value::Int)
                .unwrap_or_else(|_| Value::Text(text(&bytes))),
            ColumnType::MYSQL_TYPE_FLOAT | ColumnType::MYSQL_TYPE_DOUBLE => text(&bytes)
                .parse()
                .map(Value::Float)
                .unwrap_or_else(|_| Value::Text(text(&bytes))),
            ColumnType::MYSQL_TYPE_TINY_BLOB
            | ColumnType::MYSQL_TYPE_MEDIUM_BLOB
            | ColumnType::MYSQL_TYPE_LONG_BLOB
            | ColumnType::MYSQL_TYPE_BLOB
                if column.character_set() == 63 =>
            {
                Value::Bytes(bytes)
            }
            ColumnType::MYSQL_TYPE_TINY_BLOB
            | ColumnType::MYSQL_TYPE_MEDIUM_BLOB
            | ColumnType::MYSQL_TYPE_LONG_BLOB
            | ColumnType::MYSQL_TYPE_BLOB => String::from_utf8(bytes)
                .map(Value::Text)
                .unwrap_or_else(|error| Value::Bytes(error.into_bytes())),
            _ => String::from_utf8(bytes)
                .map(Value::Text)
                .unwrap_or_else(|error| Value::Bytes(error.into_bytes())),
        },
        mysql::Value::Date(year, month, day, hour, minute, second, micros) => {
            let base = format!("{year:04}-{month:02}-{day:02}");
            if matches!(kind, ColumnType::MYSQL_TYPE_DATE) {
                Value::Date(base)
            } else {
                Value::Timestamp(format!(
                    "{base} {hour:02}:{minute:02}:{second:02}.{:06}",
                    micros
                ))
            }
        }
        mysql::Value::Time(negative, days, hours, minutes, seconds, micros) => {
            Value::Time(format!(
                "{}{:02}:{:02}:{:02}.{:06}",
                if negative { "-" } else { "" },
                days * 24 + hours as u32,
                minutes,
                seconds,
                micros
            ))
        }
    }
}

fn read_statement(sql: &str) -> bool {
    let normalized = sql.trim_start().to_ascii_lowercase();
    ["select", "with", "show", "explain", "describe", "values"]
        .iter()
        .any(|prefix| normalized.starts_with(prefix))
}

impl DbSession for MysqlSession {
    fn kind(&self) -> DriverKind {
        DriverKind::MySql
    }

    fn schema(&mut self) -> Result<SchemaMeta, IdeError> {
        let sql = r#"
            SELECT c.table_name, t.table_type, c.column_name, c.data_type, c.is_nullable,
                   EXISTS (
                       SELECT 1 FROM information_schema.key_column_usage k
                       WHERE k.table_schema = c.table_schema
                         AND k.table_name = c.table_name
                         AND k.column_name = c.column_name
                         AND k.constraint_name = 'PRIMARY'
                   ) AS primary_key
            FROM information_schema.columns c
            JOIN information_schema.tables t
              ON t.table_schema = c.table_schema AND t.table_name = c.table_name
            WHERE c.table_schema = DATABASE()
            ORDER BY c.table_name, c.ordinal_position
        "#;
        let rows = self
            .conn
            .query_iter(sql)
            .map_err(|error| db_error("mysql.schema", error))?
            .map(|row| row.map_err(|error| db_error("mysql.schema.row", error)))
            .collect::<Result<Vec<_>, _>>()?;
        let mut tables: BTreeMap<String, TableMeta> = BTreeMap::new();
        for row in rows {
            let table: String = row.get(0).unwrap_or_default();
            let kind: String = row.get(1).unwrap_or_default();
            let primary: i64 = row.get(5).unwrap_or(0);
            let entry = tables.entry(table.clone()).or_insert_with(|| TableMeta {
                name: table,
                kind: if kind == "BASE TABLE" {
                    "table".into()
                } else {
                    "view".into()
                },
                columns: Vec::new(),
            });
            entry.columns.push(ColumnMeta {
                name: row.get(2).unwrap_or_default(),
                type_name: row.get(3).unwrap_or_default(),
                nullable: row.get::<String, _>(4).unwrap_or_default() == "YES",
                primary_key: primary != 0,
            });
        }
        Ok(SchemaMeta {
            tables: tables.into_values().collect(),
            partial_errors: Vec::new(),
        })
    }

    fn execute(&mut self, sql: &str, opts: &QueryOptions) -> Result<ExecResult, IdeError> {
        if read_statement(sql) {
            let mut result = self
                .conn
                .query_iter(sql)
                .map_err(|error| db_error("mysql.query", error))?;
            let columns_meta = result.columns().as_ref().to_vec();
            let columns = columns_meta
                .iter()
                .map(|column| column.name_str().into_owned())
                .collect::<Vec<_>>();
            let mut rows = Vec::new();
            for row in result.by_ref() {
                let row = row.map_err(|error| db_error("mysql.row", error))?;
                rows.push(
                    columns_meta
                        .iter()
                        .enumerate()
                        .map(|(index, column)| {
                            row_value(row.get(index).unwrap_or(mysql::Value::NULL), column)
                        })
                        .collect(),
                );
            }
            let total = if opts.limit.is_some() {
                None
            } else {
                Some(rows.len() as u64)
            };
            Ok(ExecResult::Rows {
                columns,
                rows,
                total,
            })
        } else {
            self.conn
                .query_drop(sql)
                .map_err(|error| db_error("mysql.execute", error))?;
            Ok(ExecResult::Affected {
                count: self.conn.affected_rows(),
                message: "ok".into(),
            })
        }
    }

    fn cancel(&self) {
        if let Ok(mut conn) = connect(&self.url) {
            let _ = conn.exec_drop("KILL QUERY ?", (self.connection_id,));
        }
    }

    fn cancellation_callback(&self) -> Option<std::sync::Arc<dyn Fn() + Send + Sync>> {
        let url = self.url.clone();
        let id = self.connection_id;
        Some(std::sync::Arc::new(move || {
            std::thread::spawn({
                let url = url.clone();
                move || {
                    if let Ok(mut conn) = connect(&url) {
                        let _ = conn.exec_drop("KILL QUERY ?", (id,));
                    }
                }
            });
        }))
    }

    fn in_transaction(&self) -> bool {
        self.in_tx
    }

    fn begin(&mut self) -> Result<(), IdeError> {
        self.conn
            .query_drop("BEGIN")
            .map_err(|error| db_error("mysql.begin", error))?;
        self.in_tx = true;
        Ok(())
    }

    fn commit(&mut self) -> Result<(), IdeError> {
        self.conn
            .query_drop("COMMIT")
            .map_err(|error| db_error("mysql.commit", error))?;
        self.in_tx = false;
        Ok(())
    }

    fn rollback(&mut self) -> Result<(), IdeError> {
        self.conn
            .query_drop("ROLLBACK")
            .map_err(|error| db_error("mysql.rollback", error))?;
        self.in_tx = false;
        Ok(())
    }

    fn update_row(&mut self, update: &RowUpdate) -> Result<u64, IdeError> {
        if update.key.is_empty() || update.changes.is_empty() {
            return Err(db_error(
                "mysql.update",
                "chave e mudanças são obrigatórias",
            ));
        }
        let table = crate::grid::quote_ident(self.kind(), &update.table);
        let sets = update
            .changes
            .iter()
            .map(|change| {
                format!(
                    "{} = ?",
                    crate::grid::quote_ident(self.kind(), &change.column)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let mut conditions = Vec::new();
        let mut append = |column: &str, value: &Value| {
            let quoted = crate::grid::quote_ident(self.kind(), column);
            if value.is_null() {
                conditions.push(format!("{quoted} IS NULL"));
            } else {
                conditions.push(format!("{quoted} = ?"));
            }
        };
        for (column, value) in &update.key {
            append(column, value);
        }
        for change in &update.changes {
            append(&change.column, &change.old);
        }
        let sql = format!(
            "UPDATE {table} SET {sets} WHERE {}",
            conditions.join(" AND ")
        );
        let params: Vec<mysql::Value> = update
            .changes
            .iter()
            .map(|change| mysql_value(&change.new))
            .chain(
                update
                    .key
                    .iter()
                    .filter(|(_, value)| !value.is_null())
                    .map(|(_, value)| mysql_value(value)),
            )
            .chain(
                update
                    .changes
                    .iter()
                    .filter(|change| !change.old.is_null())
                    .map(|change| mysql_value(&change.old)),
            )
            .collect();
        self.conn
            .exec_drop(&sql, params)
            .map_err(|error| db_error("mysql.update", error))?;
        Ok(self.conn.affected_rows())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refused_connection_is_an_error_not_support_claim() {
        assert!(MysqlDriver.test("mysql://u@127.0.0.1:1/db").is_err());
    }

    #[test]
    fn real_mysql_contract_when_fixture_is_configured() {
        let Ok(url) = std::env::var("PHPIDE_TEST_MYSQL_URL") else {
            eprintln!("BLOCKED: PHPIDE_TEST_MYSQL_URL ausente; driver real não verificado");
            return;
        };
        let driver = MysqlDriver;
        let invalid = url.replace("root@", "root:wrong@");
        assert_eq!(
            driver.test(&invalid).unwrap_err().kind,
            ErrorKind::PermissionDenied
        );
        assert!(driver.test(&url).unwrap().contains("MySQL"));
        let mut a = driver.connect(&url).unwrap();
        a.execute("DROP TABLE IF EXISTS phpide_gate", &QueryOptions::default())
            .unwrap();
        a.execute("CREATE TABLE phpide_gate (id INTEGER PRIMARY KEY, name TEXT, amount DECIMAL(20,4), data BLOB) ENGINE=InnoDB", &QueryOptions::default()).unwrap();
        a.execute(
            "INSERT INTO phpide_gate VALUES (1, 'ana', 12.3400, X'CAFE'), (2, NULL, NULL, NULL)",
            &QueryOptions::default(),
        )
        .unwrap();
        assert!(a
            .schema()
            .unwrap()
            .tables
            .iter()
            .any(|table| table.name == "phpide_gate"));
        let values = (3..=103)
            .map(|id| format!("({id}, 'user{id}', {id}.2500, NULL)"))
            .collect::<Vec<_>>()
            .join(",");
        a.execute(
            &format!("INSERT INTO phpide_gate VALUES {values}"),
            &QueryOptions::default(),
        )
        .unwrap();
        let page =
            crate::grid::paginate(&mut *a, "phpide_gate", 2, 25, &QueryOptions::default()).unwrap();
        assert_eq!(page.rows.len(), 25);
        assert_eq!(page.offset, 50);
        let mut b = driver.connect(&url).unwrap();
        let update = |old: &str, new: &str| RowUpdate {
            table: "phpide_gate".into(),
            key: vec![("id".into(), Value::Int(1))],
            changes: vec![ColumnUpdate {
                column: "name".into(),
                old: Value::Text(old.into()),
                new: Value::Text(new.into()),
            }],
        };
        assert_eq!(b.update_row(&update("ana", "bia")).unwrap(), 1);
        assert_eq!(a.update_row(&update("ana", "carla")).unwrap(), 0);
        b.begin().unwrap();
        b.execute(
            "UPDATE phpide_gate SET name = 'rollback' WHERE id = 1",
            &QueryOptions::default(),
        )
        .unwrap();
        b.rollback().unwrap();
        let interrupt = b.cancellation_callback().unwrap();
        let result = std::thread::scope(|scope| {
            let worker = scope.spawn(|| b.execute("SELECT SLEEP(30)", &QueryOptions::default()));
            std::thread::sleep(std::time::Duration::from_millis(100));
            interrupt();
            worker.join().unwrap()
        });
        assert!(result.is_err());
        assert!(b.execute("SELECT 1", &QueryOptions::default()).is_ok());
        b.begin().unwrap();
        b.execute(
            "CREATE TABLE phpide_ddl_gate (id INTEGER) ENGINE=InnoDB",
            &QueryOptions::default(),
        )
        .unwrap();
        b.rollback().unwrap();
        assert!(
            b.schema()
                .unwrap()
                .tables
                .iter()
                .any(|table| table.name == "phpide_ddl_gate"),
            "DDL faz commit implícito no MySQL"
        );
        b.execute("DROP TABLE phpide_ddl_gate", &QueryOptions::default())
            .unwrap();
        b.execute("DROP TABLE phpide_gate", &QueryOptions::default())
            .unwrap();
    }
}
