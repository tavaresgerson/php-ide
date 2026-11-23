use phpide_core::{Component, ErrorKind, IdeError};

use crate::contracts::{ColumnUpdate, DbSession, DriverKind, RowUpdate};
use crate::value::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct CellChange {
    pub table: String,
    pub key: Vec<(String, Value)>,
    pub column: String,
    pub old: Value,
    pub new: Value,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PendingChanges {
    pub changes: Vec<CellChange>,
}

impl PendingChanges {
    pub fn push(&mut self, change: CellChange) {
        if let Some(existing) = self.changes.iter_mut().find(|current| {
            current.table == change.table
                && current.key == change.key
                && current.column == change.column
        }) {
            existing.new = change.new;
        } else {
            self.changes.push(change);
        }
    }

    pub fn revert(&mut self, table: &str) {
        self.changes.retain(|change| change.table != table);
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn by_row(&self) -> Vec<RowUpdate> {
        let mut rows: Vec<RowUpdate> = Vec::new();
        for change in &self.changes {
            let column = ColumnUpdate {
                column: change.column.clone(),
                old: change.old.clone(),
                new: change.new.clone(),
            };
            if let Some(row) = rows
                .iter_mut()
                .find(|row| row.table == change.table && row.key == change.key)
            {
                row.changes.push(column);
            } else {
                rows.push(RowUpdate {
                    table: change.table.clone(),
                    key: change.key.clone(),
                    changes: vec![column],
                });
            }
        }
        rows
    }
}

fn condition(kind: DriverKind, column: &str, value: &Value) -> String {
    let column = crate::grid::quote_ident(kind, column);
    if value.is_null() {
        format!("{column} IS NULL")
    } else {
        format!("{column} = {}", literal(value))
    }
}

pub fn preview_dml(changes: &PendingChanges, kind: DriverKind) -> String {
    let mut out = String::new();
    for row in changes.by_row() {
        let sets = row
            .changes
            .iter()
            .map(|change| {
                format!(
                    "{} = {}",
                    crate::grid::quote_ident(kind, &change.column),
                    literal(&change.new)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let conditions = row
            .key
            .iter()
            .map(|(column, value)| condition(kind, column, value))
            .chain(
                row.changes
                    .iter()
                    .map(|change| condition(kind, &change.column, &change.old)),
            )
            .collect::<Vec<_>>()
            .join(" AND ");
        out.push_str(&format!(
            "UPDATE {} SET {sets} WHERE {conditions};\n",
            crate::grid::quote_ident(kind, &row.table)
        ));
    }
    out
}

fn literal(value: &Value) -> String {
    match value {
        Value::Null => "NULL".into(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Bytes(value) => format!(
            "X'{}'",
            value
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<String>()
        ),
        other => format!("'{}'", other.display().replace('\'', "''")),
    }
}

fn conflict(table: &str, affected: u64) -> IdeError {
    IdeError::new(
        Component::Db,
        "db.edit",
        ErrorKind::Other,
        format!("conflito em {table}: {affected} linha(s) afetada(s), esperado exatamente 1"),
        "a linha mudou no banco ou a chave não é única; recarregue a grade antes de editar",
    )
}

pub fn apply(session: &mut dyn DbSession, changes: &PendingChanges) -> Result<usize, IdeError> {
    let started_transaction = !session.in_transaction();
    if started_transaction {
        session.begin()?;
    }
    let mut applied = 0;
    let result = (|| {
        for row in changes.by_row() {
            let affected = session.update_row(&row)?;
            if affected != 1 {
                return Err(conflict(&row.table, affected));
            }
            applied += 1;
        }
        Ok(applied)
    })();
    match result {
        Ok(applied) => {
            if started_transaction {
                if let Err(error) = session.commit() {
                    let rollback = session.rollback();
                    return Err(error.with_detail(format!(
                        "commit falhou; rollback executado: {}",
                        rollback.is_ok()
                    )));
                }
            }
            Ok(applied)
        }
        Err(error) => {
            if started_transaction {
                match session.rollback() {
                    Ok(()) => Err(error.with_detail(format!(
                        "lote revertido após {applied} update(s); nenhuma mudança do lote persistiu"
                    ))),
                    Err(rollback) => Err(error.with_detail(format!(
                        "falha parcial após {applied} update(s); rollback também falhou: {}",
                        rollback.log_line()
                    ))),
                }
            } else {
                Err(error.with_detail(format!(
                    "falha após {applied} update(s) na transação ativa; execute Rollback para descartar o lote"
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(column: &str, old: Value, new: Value) -> CellChange {
        CellChange {
            table: "users".into(),
            key: vec![("id".into(), Value::Int(1))],
            column: column.into(),
            old,
            new,
        }
    }

    #[test]
    fn pending_preserves_original_old_and_previews_optimistic_condition() {
        let mut pending = PendingChanges::default();
        pending.push(change(
            "name",
            Value::Text("ana".into()),
            Value::Text("bia".into()),
        ));
        pending.push(change(
            "name",
            Value::Text("bia".into()),
            Value::Text("carla".into()),
        ));
        pending.push(change("age", Value::Null, Value::Int(2)));
        let rows = pending.by_row();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].changes.len(), 2);
        assert_eq!(rows[0].changes[0].old, Value::Text("ana".into()));
        let dml = preview_dml(&pending, DriverKind::Sqlite);
        assert!(
            dml.contains("\"id\" = 1 AND \"name\" = 'ana' AND \"age\" IS NULL"),
            "{dml}"
        );
        assert!(dml.contains("\"name\" = 'carla'"));
    }

    #[test]
    fn preview_quotes_mysql_identifiers_and_values() {
        let mut pending = PendingChanges::default();
        pending.push(change("n`ame", Value::Null, Value::Text("o'p".into())));
        let dml = preview_dml(&pending, DriverKind::MySql);
        assert!(dml.contains("`n``ame` = 'o''p'"), "{dml}");
        assert!(dml.contains("`n``ame` IS NULL"));
    }
}
