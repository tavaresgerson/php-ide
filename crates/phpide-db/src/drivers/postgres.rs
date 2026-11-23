use std::collections::BTreeMap;

use phpide_core::{Component, ErrorKind, IdeError};
use postgres::types::{FromSql, IsNull, ToSql, Type};
use postgres::{Client, NoTls};

use crate::contracts::*;
use crate::value::Value;

pub struct PostgresDriver;

pub struct PostgresSession {
    client: Client,
    cancel: postgres::CancelToken,
    in_tx: bool,
}

impl PostgresDriver {
    pub fn boxed() -> Box<dyn Driver> {
        Box::new(Self)
    }
}

fn db_error(operation: &str, error: impl std::fmt::Display) -> IdeError {
    let detail = error.to_string();
    let lower = detail.to_ascii_lowercase();
    let (kind, action) = if lower.contains("password")
        || lower.contains("authentication")
        || lower.contains("role")
    {
        (
            ErrorKind::PermissionDenied,
            "verifique usuário e senha do data source",
        )
    } else if lower.contains("connection") || lower.contains("connect") || lower.contains("timeout")
    {
        (
            ErrorKind::Io,
            "verifique host, porta e se o PostgreSQL está aceitando conexões",
        )
    } else {
        (ErrorKind::Other, "verifique o SQL e o estado da conexão")
    };
    IdeError::new(
        Component::Db,
        operation.to_string(),
        kind,
        format!("PostgreSQL falhou em {operation}"),
        action,
    )
    .with_detail(detail)
}

fn connect(url: &str) -> Result<Client, IdeError> {
    Client::connect(url, NoTls).map_err(|error| {
        let mut detail = error.to_string();
        let mut source = std::error::Error::source(&error);
        while let Some(cause) = source {
            detail.push_str(&format!(": {cause}"));
            source = cause.source();
        }
        db_error("postgresql.connect", detail)
    })
}

impl Driver for PostgresDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::PostgreSql
    }

    fn test(&self, url: &str) -> Result<String, IdeError> {
        let mut client = connect(url)?;
        client
            .query_one("SELECT version()", &[])
            .map_err(|error| db_error("postgresql.test", error))?
            .get::<_, String>(0)
            .pipe(Ok)
    }

    fn connect(&self, url: &str) -> Result<Box<dyn DbSession>, IdeError> {
        let mut client = connect(url)?;
        let cancel = client.cancel_token();
        client
            .simple_query("SELECT 1")
            .map_err(|error| db_error("postgresql.ping", error))?;
        Ok(Box::new(PostgresSession {
            client,
            cancel,
            in_tx: false,
        }))
    }
}

trait Pipe: Sized {
    fn pipe<T>(self, map: impl FnOnce(Self) -> T) -> T {
        map(self)
    }
}
impl<T> Pipe for T {}

#[derive(Debug)]
struct PgNumeric(String);

impl FromSql<'_> for PgNumeric {
    fn from_sql(_ty: &Type, raw: &[u8]) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        if raw.len() < 8 {
            return Err("NUMERIC curto".into());
        }
        let count = i16::from_be_bytes(raw[0..2].try_into()?) as isize;
        let weight = i16::from_be_bytes(raw[2..4].try_into()?) as isize;
        let sign = u16::from_be_bytes(raw[4..6].try_into()?);
        let scale = u16::from_be_bytes(raw[6..8].try_into()?) as usize;
        if sign == 0xC000 {
            return Ok(Self("NaN".into()));
        }
        if raw.len() != 8 + count as usize * 2 {
            return Err("NUMERIC com tamanho inválido".into());
        }
        let mut groups = Vec::new();
        for index in 0..count as usize {
            let at = 8 + index * 2;
            groups.push(i16::from_be_bytes(raw[at..at + 2].try_into()?) as u16);
        }
        let mut integer = String::new();
        if weight >= 0 {
            for position in (0..=weight).rev() {
                let group_index = weight - position;
                let value = groups.get(group_index as usize).copied().unwrap_or(0);
                if integer.is_empty() {
                    integer = value.to_string();
                } else {
                    integer.push_str(&format!("{value:04}"));
                }
            }
        } else {
            integer = "0".into();
        }
        let mut fraction = String::new();
        let lowest = weight - groups.len() as isize + 1;
        for position in (lowest..=-1).rev() {
            let group_index = weight - position;
            let value = if group_index >= 0 {
                groups.get(group_index as usize).copied().unwrap_or(0)
            } else {
                0
            };
            fraction.push_str(&format!("{value:04}"));
        }
        while fraction.len() < scale {
            fraction.push('0');
        }
        fraction.truncate(scale);
        let negative = sign == 0x4000;
        Ok(Self(if scale == 0 {
            format!("{}{}", if negative { "-" } else { "" }, integer)
        } else {
            format!(
                "{}{}.{}",
                if negative { "-" } else { "" },
                integer,
                fraction
            )
        }))
    }

    fn accepts(ty: &Type) -> bool {
        *ty == Type::NUMERIC
    }
}

fn encode_numeric(text: &str) -> Result<Vec<u8>, String> {
    let (negative, unsigned) = text
        .strip_prefix('-')
        .map(|value| (true, value))
        .unwrap_or((false, text));
    if unsigned.contains(['e', 'E']) {
        return Err("NUMERIC com expoente ainda não é aceito para bind".into());
    }
    let (integer, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    if integer.is_empty()
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.len() > u16::MAX as usize
    {
        return Err(format!("NUMERIC inválido: {text}"));
    }
    let combined = format!("{integer}{fraction}");
    let left_pad = (4 - integer.len() % 4) % 4;
    let right_pad = (4 - (left_pad + combined.len()) % 4) % 4;
    let padded = format!(
        "{}{}{}",
        "0".repeat(left_pad),
        combined,
        "0".repeat(right_pad)
    );
    let mut groups: Vec<u16> = padded
        .as_bytes()
        .chunks(4)
        .map(|chunk| std::str::from_utf8(chunk).unwrap().parse().unwrap())
        .collect();
    let mut weight = ((integer.len() + left_pad) / 4) as i16 - 1;
    while groups.first() == Some(&0) {
        groups.remove(0);
        weight -= 1;
    }
    while groups.last() == Some(&0) {
        groups.pop();
    }
    let mut out = Vec::with_capacity(8 + groups.len() * 2);
    out.extend_from_slice(&(groups.len() as i16).to_be_bytes());
    out.extend_from_slice(&weight.to_be_bytes());
    out.extend_from_slice(&(if negative { 0x4000u16 } else { 0 }).to_be_bytes());
    out.extend_from_slice(&(fraction.len() as u16).to_be_bytes());
    for group in groups {
        out.extend_from_slice(&group.to_be_bytes());
    }
    Ok(out)
}

#[derive(Debug)]
enum PgBind {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
    Decimal(String),
}

impl ToSql for PgBind {
    fn to_sql(
        &self,
        ty: &Type,
        out: &mut bytes::BytesMut,
    ) -> Result<IsNull, Box<dyn std::error::Error + Sync + Send>> {
        match self {
            Self::Null => Ok(IsNull::Yes),
            Self::Bool(value) => value.to_sql(ty, out),
            Self::Int(value) => match *ty {
                Type::INT2 => (*value as i16).to_sql(ty, out),
                Type::INT4 => (*value as i32).to_sql(ty, out),
                _ => value.to_sql(ty, out),
            },
            Self::Float(value) => value.to_sql(ty, out),
            Self::Text(value) => match *ty {
                Type::DATE => {
                    let date = time::Date::parse(
                        value,
                        &time::macros::format_description!("[year]-[month]-[day]"),
                    )?;
                    date.to_sql(ty, out)
                }
                Type::TIME => {
                    let time = time::Time::parse(
                        value,
                        &time::macros::format_description!("[hour]:[minute]:[second]"),
                    )?;
                    time.to_sql(ty, out)
                }
                Type::TIMESTAMP => {
                    let format = time::macros::format_description!(
                        "[year]-[month]-[day] [hour]:[minute]:[second]"
                    );
                    let timestamp =
                        time::PrimitiveDateTime::parse(&value.replace('T', " "), format)?;
                    timestamp.to_sql(ty, out)
                }
                Type::TIMESTAMPTZ => {
                    let timestamp = time::OffsetDateTime::parse(
                        value,
                        &time::format_description::well_known::Rfc3339,
                    )?;
                    timestamp.to_sql(ty, out)
                }
                Type::UUID => {
                    let uuid = uuid::Uuid::parse_str(value)?;
                    uuid.to_sql(ty, out)
                }
                _ => value.to_sql(ty, out),
            },
            Self::Bytes(value) => value.to_sql(ty, out),
            Self::Decimal(value) if *ty == Type::NUMERIC => {
                out.extend_from_slice(&encode_numeric(value)?);
                Ok(IsNull::No)
            }
            Self::Decimal(value) => value.to_sql(ty, out),
        }
    }

    fn accepts(_ty: &Type) -> bool {
        true
    }

    postgres::types::to_sql_checked!();
}

fn bind(value: &Value) -> PgBind {
    match value {
        Value::Null => PgBind::Null,
        Value::Bool(value) => PgBind::Bool(*value),
        Value::Int(value) => PgBind::Int(*value),
        Value::Float(value) => PgBind::Float(*value),
        Value::Decimal(value) => PgBind::Decimal(value.clone()),
        Value::Text(value) => PgBind::Text(value.clone()),
        Value::Bytes(value) => PgBind::Bytes(value.clone()),
        Value::Date(value) | Value::Time(value) | Value::Timestamp(value) => {
            PgBind::Text(value.clone())
        }
    }
}

fn row_value(row: &postgres::Row, index: usize) -> Result<Value, IdeError> {
    let ty = row.columns()[index].type_().clone();
    macro_rules! get {
        ($rust:ty, $variant:expr) => {
            row.try_get::<_, Option<$rust>>(index)
                .map(|value| value.map($variant).unwrap_or(Value::Null))
                .map_err(|error| db_error("postgresql.value", error))
        };
    }
    match ty {
        Type::BOOL => get!(bool, Value::Bool),
        Type::INT2 => get!(i16, |value| Value::Int(value as i64)),
        Type::INT4 => get!(i32, |value| Value::Int(value as i64)),
        Type::INT8 => get!(i64, Value::Int),
        Type::FLOAT4 => get!(f32, |value| Value::Float(value as f64)),
        Type::FLOAT8 => get!(f64, Value::Float),
        Type::NUMERIC => get!(PgNumeric, |value| Value::Decimal(value.0)),
        Type::BYTEA => get!(Vec<u8>, Value::Bytes),
        Type::DATE => get!(time::Date, |value| Value::Date(value.to_string())),
        Type::TIME => get!(time::Time, |value| Value::Time(value.to_string())),
        Type::TIMESTAMP => get!(time::PrimitiveDateTime, |value| Value::Timestamp(
            value.to_string()
        )),
        Type::TIMESTAMPTZ => get!(time::OffsetDateTime, |value| Value::Timestamp(
            value.to_string()
        )),
        Type::UUID => get!(uuid::Uuid, |value| Value::Text(value.to_string())),
        _ => get!(String, Value::Text),
    }
}

fn read_statement(sql: &str) -> bool {
    let normalized = sql.trim_start().to_ascii_lowercase();
    ["select", "with", "show", "explain", "values"]
        .iter()
        .any(|prefix| normalized.starts_with(prefix))
}

impl DbSession for PostgresSession {
    fn kind(&self) -> DriverKind {
        DriverKind::PostgreSql
    }

    fn schema(&mut self) -> Result<SchemaMeta, IdeError> {
        let sql = r#"
            SELECT c.table_name, t.table_type, c.column_name, c.data_type, c.is_nullable,
                   EXISTS (
                       SELECT 1 FROM information_schema.table_constraints tc
                       JOIN information_schema.key_column_usage kcu
                         ON tc.constraint_name = kcu.constraint_name
                        AND tc.table_schema = kcu.table_schema
                        AND tc.table_name = kcu.table_name
                       WHERE tc.constraint_type = 'PRIMARY KEY'
                         AND tc.table_schema = c.table_schema
                         AND tc.table_name = c.table_name
                         AND kcu.column_name = c.column_name
                   ) AS primary_key
            FROM information_schema.columns c
            JOIN information_schema.tables t
              ON t.table_schema = c.table_schema AND t.table_name = c.table_name
            WHERE c.table_schema = current_schema()
            ORDER BY c.table_name, c.ordinal_position
        "#;
        let rows = self
            .client
            .query(sql, &[])
            .map_err(|error| db_error("postgresql.schema", error))?;
        let mut tables: BTreeMap<String, TableMeta> = BTreeMap::new();
        for row in rows {
            let table: String = row.get(0);
            let kind: String = row.get(1);
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
                name: row.get(2),
                type_name: row.get(3),
                nullable: row.get::<_, String>(4) == "YES",
                primary_key: row.get(5),
            });
        }
        Ok(SchemaMeta {
            tables: tables.into_values().collect(),
            partial_errors: Vec::new(),
        })
    }

    fn execute(&mut self, sql: &str, opts: &QueryOptions) -> Result<ExecResult, IdeError> {
        if read_statement(sql) {
            let statement = self
                .client
                .prepare(sql)
                .map_err(|error| db_error("postgresql.prepare", error))?;
            let columns = statement
                .columns()
                .iter()
                .map(|column| column.name().to_string())
                .collect::<Vec<_>>();
            let rows = self
                .client
                .query(&statement, &[])
                .map_err(|error| db_error("postgresql.query", error))?;
            let mut values = Vec::with_capacity(rows.len());
            for row in &rows {
                values.push(
                    (0..columns.len())
                        .map(|index| row_value(row, index))
                        .collect::<Result<Vec<_>, _>>()?,
                );
            }
            let total = if opts.limit.is_some() {
                None
            } else {
                Some(values.len() as u64)
            };
            Ok(ExecResult::Rows {
                columns,
                rows: values,
                total,
            })
        } else {
            let affected = self
                .client
                .execute(sql, &[])
                .map_err(|error| db_error("postgresql.execute", error))?;
            Ok(ExecResult::Affected {
                count: affected,
                message: "ok".into(),
            })
        }
    }

    fn cancel(&self) {
        let _ = self.cancel.cancel_query(NoTls);
    }

    fn cancellation_callback(&self) -> Option<std::sync::Arc<dyn Fn() + Send + Sync>> {
        let token = self.cancel.clone();
        Some(std::sync::Arc::new(move || {
            std::thread::spawn({
                let token = token.clone();
                move || {
                    let _ = token.cancel_query(NoTls);
                }
            });
        }))
    }

    fn in_transaction(&self) -> bool {
        self.in_tx
    }

    fn begin(&mut self) -> Result<(), IdeError> {
        self.client
            .batch_execute("BEGIN")
            .map_err(|error| db_error("postgresql.begin", error))?;
        self.in_tx = true;
        Ok(())
    }

    fn commit(&mut self) -> Result<(), IdeError> {
        self.client
            .batch_execute("COMMIT")
            .map_err(|error| db_error("postgresql.commit", error))?;
        self.in_tx = false;
        Ok(())
    }

    fn rollback(&mut self) -> Result<(), IdeError> {
        self.client
            .batch_execute("ROLLBACK")
            .map_err(|error| db_error("postgresql.rollback", error))?;
        self.in_tx = false;
        Ok(())
    }

    fn update_row(&mut self, update: &RowUpdate) -> Result<u64, IdeError> {
        if update.key.is_empty() || update.changes.is_empty() {
            return Err(db_error(
                "postgresql.update",
                "chave e mudanças são obrigatórias",
            ));
        }
        let table = crate::grid::quote_ident(self.kind(), &update.table);
        let mut params: Vec<PgBind> = update
            .changes
            .iter()
            .map(|change| bind(&change.new))
            .collect();
        let sets = update
            .changes
            .iter()
            .enumerate()
            .map(|(index, change)| {
                format!(
                    "{} = ${}",
                    crate::grid::quote_ident(self.kind(), &change.column),
                    index + 1
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
                params.push(bind(value));
                conditions.push(format!("{quoted} = ${}", params.len()));
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
        let refs = params
            .iter()
            .map(|param| param as &(dyn ToSql + Sync))
            .collect::<Vec<_>>();
        self.client
            .execute(&sql, &refs)
            .map_err(|error| db_error("postgresql.update", error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_binary_roundtrip_preserves_decimal() {
        for value in [
            "0",
            "12.34",
            "-0.0012",
            "12345678901234567890.123",
            "10000",
            "0.0001",
        ] {
            let encoded = encode_numeric(value).unwrap();
            let decoded = PgNumeric::from_sql(&Type::NUMERIC, &encoded).unwrap().0;
            let expected = value.trim_start_matches('+');
            assert_eq!(
                decoded.parse::<f64>().unwrap(),
                expected.parse::<f64>().unwrap()
            );
            if value.contains('.') {
                assert_eq!(
                    decoded.split('.').nth(1).unwrap().len(),
                    value.split('.').nth(1).unwrap().len()
                );
            }
        }
    }

    #[test]
    fn refused_connection_is_an_error_not_support_claim() {
        let driver = PostgresDriver;
        assert!(driver.test("postgresql://u@127.0.0.1:1/db").is_err());
    }

    #[test]
    fn real_postgres_contract_when_fixture_is_configured() {
        let Ok(url) = std::env::var("PHPIDE_TEST_POSTGRES_URL") else {
            eprintln!("BLOCKED: PHPIDE_TEST_POSTGRES_URL ausente; driver real não verificado");
            return;
        };
        let driver = PostgresDriver;
        let invalid = url.replace("gate@", "wrong:bad@");
        let invalid_error = driver.test(&invalid).unwrap_err();
        assert_eq!(
            invalid_error.kind,
            ErrorKind::PermissionDenied,
            "{invalid_error:?}"
        );
        assert!(driver.test(&url).unwrap().contains("PostgreSQL"));
        let mut a = driver.connect(&url).unwrap();
        a.execute("DROP TABLE IF EXISTS phpide_gate", &QueryOptions::default())
            .unwrap();
        a.execute(
            "CREATE TABLE phpide_gate (id INTEGER PRIMARY KEY, name TEXT, amount NUMERIC)",
            &QueryOptions::default(),
        )
        .unwrap();
        a.execute(
            "INSERT INTO phpide_gate VALUES (1, 'ana', 12.34), (2, NULL, NULL)",
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
            .map(|id| format!("({id}, 'user{id}', {id}.25)"))
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
        let decimal_update = RowUpdate {
            table: "phpide_gate".into(),
            key: vec![("id".into(), Value::Int(1))],
            changes: vec![ColumnUpdate {
                column: "amount".into(),
                old: Value::Decimal("12.34".into()),
                new: Value::Decimal("999999999999.1250".into()),
            }],
        };
        assert_eq!(b.update_row(&decimal_update).unwrap(), 1);
        let ExecResult::Rows { rows, .. } = b
            .execute(
                "SELECT amount FROM phpide_gate WHERE id = 1",
                &QueryOptions::default(),
            )
            .unwrap()
        else {
            panic!("rows")
        };
        assert_eq!(rows[0][0], Value::Decimal("999999999999.1250".into()));
        assert_eq!(a.update_row(&update("ana", "carla")).unwrap(), 0);
        b.begin().unwrap();
        b.execute(
            "UPDATE phpide_gate SET name = 'rollback' WHERE id = 1",
            &QueryOptions::default(),
        )
        .unwrap();
        b.rollback().unwrap();
        let interrupt = b.cancellation_callback().unwrap();
        let query = "SELECT pg_sleep(30)";
        let result = std::thread::scope(|scope| {
            let worker = scope.spawn(|| b.execute(query, &QueryOptions::default()));
            std::thread::sleep(std::time::Duration::from_millis(100));
            interrupt();
            worker.join().unwrap()
        });
        assert!(result.is_err());
        assert!(b.execute("SELECT 1", &QueryOptions::default()).is_ok());
        b.begin().unwrap();
        b.execute(
            "CREATE TABLE phpide_ddl_gate (id INTEGER)",
            &QueryOptions::default(),
        )
        .unwrap();
        b.rollback().unwrap();
        assert!(!b
            .schema()
            .unwrap()
            .tables
            .iter()
            .any(|table| table.name == "phpide_ddl_gate"));
        b.execute("DROP TABLE phpide_gate", &QueryOptions::default())
            .unwrap();
    }
}
