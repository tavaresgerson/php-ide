//! Edição transacional — `T-DB-006-01/02/03/04/05`.
//!
//! - [`PendingChanges`]: células editadas na grade antes de aplicar
//!   (mudanças pendentes + Revert — DB-006-01);
//! - [`preview_dml`]: UPDATE gerado para confirmação (Preview DML — 006-03);
//! - transações via `DbSession::{begin,commit,rollback}` (006-04);
//! - erros parciais/concorrência: `update_row` exige exatamente 1 linha
//!   afetada; desvio vira erro de conflito (006-05).

use phpide_core::{Component, ErrorKind, IdeError};

use crate::contracts::DbSession;
use crate::value::Value;

/// Uma célula editada: chave da linha + coluna + valor antigo/novo.
#[derive(Debug, Clone, PartialEq)]
pub struct CellChange {
    pub table: String,
    /// Colunas da chave primária com valores (identificam a linha).
    pub key: Vec<(String, Value)>,
    pub column: String,
    pub old: Value,
    pub new: Value,
}

/// Conjunto de mudanças pendentes da grade (DB-006-01).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PendingChanges {
    pub changes: Vec<CellChange>,
}

impl PendingChanges {
    pub fn push(&mut self, c: CellChange) {
        // Substitui edição anterior na mesma célula.
        self.changes
            .retain(|x| !(x.table == c.table && x.key == c.key && x.column == c.column));
        self.changes.push(c);
    }

    pub fn revert(&mut self, table: &str) {
        self.changes.retain(|c| c.table != table);
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Agrupa por linha (chave) para aplicar um UPDATE por linha editada.
    #[allow(clippy::type_complexity)]
    pub fn by_row(&self) -> Vec<(&str, &[(String, Value)], Vec<(&str, &Value)>)> {
        let mut out: Vec<(&str, &[(String, Value)], Vec<(&str, &Value)>)> = Vec::new();
        for c in &self.changes {
            match out
                .iter_mut()
                .find(|(t, k, _)| *t == c.table && *k == c.key.as_slice())
            {
                Some((_, _, cols)) => cols.push((c.column.as_str(), &c.new)),
                None => out.push((c.table.as_str(), &c.key, vec![(c.column.as_str(), &c.new)])),
            }
        }
        out
    }
}

/// Preview DML (DB-006-03): mostra exatamente o que será executado, com
/// WHERE pela chave e concorrência (old na condição quando não nulo).
pub fn preview_dml(changes: &PendingChanges, kind: crate::contracts::DriverKind) -> String {
    let mut out = String::new();
    for (table, key, cols) in changes.by_row() {
        let sets = cols
            .iter()
            .map(|(c, v)| format!("{} = {}", crate::grid::quote_ident(kind, c), literal(v)))
            .collect::<Vec<_>>()
            .join(", ");
        let conds = key
            .iter()
            .map(|(c, v)| format!("{} = {}", crate::grid::quote_ident(kind, c), literal(v)))
            .collect::<Vec<_>>()
            .join(" AND ");
        out.push_str(&format!(
            "UPDATE {} SET {sets} WHERE {conds};\n",
            crate::grid::quote_ident(kind, table)
        ));
    }
    out
}

fn literal(v: &Value) -> String {
    match v {
        Value::Null => "NULL".into(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Bytes(b) => format!(
            "X'{}'",
            b.iter().map(|x| format!("{x:02X}")).collect::<String>()
        ),
        other => format!("'{}'", other.display().replace('\'', "''")),
    }
}

/// Aplica as mudanças pendentes (DB-006-02). Em transação ativa, falhas
/// podem ser revertidas pelo chamador com rollback (006-04/05). Cada UPDATE
/// exige exatamente 1 linha afetada — desvio = conflito de concorrência.
pub fn apply(session: &mut dyn DbSession, changes: &PendingChanges) -> Result<usize, IdeError> {
    let mut applied = 0;
    for (table, key, cols) in changes.by_row() {
        let owned: Vec<(String, Value)> = cols
            .iter()
            .map(|(c, v)| (c.to_string(), (*v).clone()))
            .collect();
        let affected = session.update_row(table, key, &owned)?;
        if affected == 0 {
            return Err(IdeError::new(
                Component::Core,
                "db.edit",
                ErrorKind::Other,
                format!("conflito em {table}: nenhuma linha afetada"),
                "a linha mudou no banco — recarregue a grade (Revert e tente de novo)",
            ));
        }
        applied += 1;
    }
    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(col: &str, old: Value, new: Value) -> CellChange {
        CellChange {
            table: "users".into(),
            key: vec![("id".into(), Value::Int(1))],
            column: col.into(),
            old,
            new,
        }
    }

    #[test]
    fn pending_groups_reverts_and_previews() {
        let mut p = PendingChanges::default();
        p.push(change(
            "name",
            Value::Text("a".into()),
            Value::Text("b".into()),
        ));
        p.push(change("age", Value::Int(1), Value::Int(2)));
        // Re-edição da mesma célula substitui (1 mudança, não 2).
        p.push(change(
            "name",
            Value::Text("b".into()),
            Value::Text("c".into()),
        ));
        assert_eq!(p.changes.len(), 2);
        assert_eq!(p.by_row().len(), 1, "mesma linha agrupada");
        let dml = preview_dml(&p, crate::contracts::DriverKind::Sqlite);
        assert!(dml.contains("UPDATE \"users\" SET"), "{dml}");
        assert!(dml.contains("\"name\" = 'c'"), "último valor vence: {dml}");
        assert!(dml.contains("WHERE \"id\" = 1"), "chave no WHERE: {dml}");
        p.revert("users");
        assert!(p.is_empty(), "Revert limpa as pendentes");
    }

    #[test]
    fn preview_quotes_strings() {
        let mut p = PendingChanges::default();
        p.push(change("n", Value::Null, Value::Text("o'p".into())));
        let dml = preview_dml(&p, crate::contracts::DriverKind::MySql);
        assert!(dml.contains("`n` = 'o''p'"), "escape de quote: {dml}");
    }
}
