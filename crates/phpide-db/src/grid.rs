//! Grade paginada, views e exportação — `T-DB-005-01/02/03/04`.
//!
//! - [`paginate`]: query server-side com LIMIT/OFFSET/ORDER BY/WHERE por
//!   dialeto (ordenação e filtros **no servidor**, DB-005-03);
//! - [`export_csv`] / [`export_sql`]: exportação (DB-005-04);
//! - [`CellView`]: Table/Text/Record (DB-005-02) — Value Editor age sobre
//!   `edit::PendingChanges` (DB-006-02).

use phpide_core::{Component, ErrorKind, IdeError};

use crate::contracts::{DbSession, ExecResult, QueryOptions};
use crate::value::Value;

/// Página da grade (DB-005-01): janela LIMIT/OFFSET server-side.
#[derive(Debug, Clone, PartialEq)]
pub struct GridPage {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub offset: u64,
    /// Total real quando o driver o informa (COUNT paralelo), senão None.
    pub total: Option<u64>,
}

/// Monta SELECT paginado por dialeto (quoting de identificadores).
pub fn paginate(
    session: &mut dyn DbSession,
    table: &str,
    page: u64,
    page_size: u64,
    opts: &QueryOptions,
) -> Result<GridPage, IdeError> {
    let ident = quote_ident(session.kind(), table);
    let mut sql = format!("SELECT * FROM {ident}");
    if let Some(f) = &opts.filter {
        if !f.trim().is_empty() {
            sql.push_str(&format!(" WHERE {}", f.trim()));
        }
    }
    if let Some((col, asc)) = &opts.order_by {
        sql.push_str(&format!(
            " ORDER BY {} {}",
            quote_ident(session.kind(), col),
            if *asc { "ASC" } else { "DESC" }
        ));
    }
    let limit = opts.limit.unwrap_or(page_size);
    let offset = opts.offset.unwrap_or(page * page_size);
    sql.push_str(&format!(" LIMIT {limit} OFFSET {offset}"));
    match session.execute(&sql, &QueryOptions::default())? {
        ExecResult::Rows { columns, rows, .. } => Ok(GridPage {
            columns,
            rows,
            offset,
            total: None,
        }),
        other => Err(IdeError::new(
            Component::Core,
            "grid.paginate",
            ErrorKind::InvalidInput,
            format!("paginação inesperada: {other:?}"),
            "verifique o nome da tabela",
        )),
    }
}

/// Quoting de identificador por dialeto.
pub fn quote_ident(kind: crate::contracts::DriverKind, name: &str) -> String {
    let (l, r) = match kind {
        crate::contracts::DriverKind::MySql => ('`', '`'),
        _ => ('"', '"'),
    };
    format!("{l}{}{r}", name.replace('"', ""))
}

/// Célula formatada para a view Text (uma coluna por linha, Record por
/// chave=valor) — DB-005-02.
pub enum CellView {
    Table,
    /// Text: linhas "col: valor" de uma linha do resultset.
    Text(Vec<String>),
    /// Record: pares (coluna, valor) da linha.
    Record(Vec<(String, Value)>),
}

pub fn text_view(columns: &[String], row: &[Value]) -> CellView {
    CellView::Text(
        columns
            .iter()
            .zip(row.iter())
            .map(|(c, v)| format!("{c}: {}", v.display()))
            .collect(),
    )
}

pub fn record_view(columns: &[String], row: &[Value]) -> CellView {
    CellView::Record(columns.iter().cloned().zip(row.iter().cloned()).collect())
}

/// Exporta CSV (DB-005-04): cabeçalho + valores display; NULL vazio.
pub fn export_csv(columns: &[String], rows: &[Vec<Value>]) -> String {
    let mut out = String::new();
    let esc = |s: &str| {
        if s.contains([',', '"', '\n']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    };
    out.push_str(&columns.iter().map(|c| esc(c)).collect::<Vec<_>>().join(","));
    out.push('\n');
    for row in rows {
        out.push_str(
            &row.iter()
                .map(|v| {
                    if v.is_null() {
                        String::new()
                    } else {
                        esc(&v.display())
                    }
                })
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push('\n');
    }
    out
}

/// Exporta como INSERTs (dump da página) — DB-005-04.
pub fn export_sql(
    kind: crate::contracts::DriverKind,
    table: &str,
    columns: &[String],
    rows: &[Vec<Value>],
) -> String {
    let t = quote_ident(kind, table);
    let cols = columns
        .iter()
        .map(|c| quote_ident(kind, c))
        .collect::<Vec<_>>()
        .join(", ");
    let literal = |v: &Value| -> String {
        match v {
            Value::Null => "NULL".into(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Decimal(d) => d.clone(),
            Value::Bytes(b) => {
                let hex: String = b.iter().map(|x| format!("{x:02X}")).collect();
                format!("X'{hex}'")
            }
            other => format!("'{}'", other.display().replace('\'', "''")),
        }
    };
    rows.iter()
        .map(|row| {
            let vals = row.iter().map(literal).collect::<Vec<_>>().join(", ");
            format!("INSERT INTO {t} ({cols}) VALUES ({vals});")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_escapes_and_nulls() {
        let csv = export_csv(
            &["id".into(), "nome".into()],
            &[
                vec![Value::Int(1), Value::Text("a,b".into())],
                vec![Value::Null, Value::Text("x\"y".into())],
            ],
        );
        let lines: Vec<_> = csv.lines().collect();
        assert_eq!(lines[0], "id,nome");
        assert_eq!(lines[1], "1,\"a,b\"");
        assert_eq!(lines[2], ",\"x\"\"y\"");
    }

    #[test]
    fn sql_dump_quotes_by_dialect_and_types() {
        let cols = vec!["id".to_string(), "d".to_string(), "b".to_string()];
        let rows = vec![vec![
            Value::Int(5),
            Value::Decimal("1.10".into()),
            Value::Bytes(vec![0xCA, 0xFE]),
        ]];
        let pg = export_sql(crate::contracts::DriverKind::Sqlite, "t", &cols, &rows);
        assert!(pg.contains("INSERT INTO \"t\" (\"id\", \"d\", \"b\") VALUES (5, 1.10, X'CAFE');"));
        let my = export_sql(crate::contracts::DriverKind::MySql, "t", &cols, &rows);
        assert!(my.contains("INSERT INTO `t` (`id`, `d`, `b`) VALUES"));
        let n = export_sql(
            crate::contracts::DriverKind::Sqlite,
            "t",
            &cols,
            &[vec![Value::Null]],
        );
        assert!(n.contains("VALUES (NULL);"));
    }

    #[test]
    fn views_build_text_and_record() {
        let cols = vec!["a".to_string(), "b".to_string()];
        let row = vec![Value::Int(1), Value::Text("x".into())];
        match text_view(&cols, &row) {
            CellView::Text(lines) => assert_eq!(lines, vec!["a: 1", "b: x"]),
            _ => panic!("text view"),
        }
        match record_view(&cols, &row) {
            CellView::Record(pairs) => {
                assert_eq!(pairs[1], ("b".to_string(), Value::Text("x".into())))
            }
            _ => panic!("record view"),
        }
        // Text/Record preservam NULL como "NULL" textual (não sucesso falso).
        match text_view(&cols, &[Value::Null, Value::Null]) {
            CellView::Text(lines) => assert_eq!(lines[0], "a: NULL"),
            _ => panic!(),
        }
    }
}
