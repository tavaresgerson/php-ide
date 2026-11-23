use std::sync::{Arc, Mutex};

use phpide_core::{Component, ErrorKind, IdeError};

use crate::contracts::*;
use crate::value::Value;

#[derive(Clone)]
pub struct FakeDriver {
    state: Arc<Mutex<FakeState>>,
}

struct FakeState {
    rows: Vec<Vec<Value>>,
    affected_override: Option<u64>,
    fail_rollback: bool,
}

pub struct FakeSession {
    state: Arc<Mutex<FakeState>>,
    in_tx: bool,
    snapshot: Option<Vec<Vec<Value>>>,
}

impl Default for FakeDriver {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(FakeState {
                rows: vec![
                    vec![
                        Value::Int(1),
                        Value::Text("ana".into()),
                        Value::Null,
                        Value::Decimal("10.5".into()),
                        Value::Bytes(vec![1, 2]),
                        Value::Date("2024-01-01".into()),
                    ],
                    vec![
                        Value::Int(2),
                        Value::Text("beto".into()),
                        Value::Bool(true),
                        Value::Decimal("0.00".into()),
                        Value::Bytes(vec![]),
                        Value::Timestamp("2024-06-01T10:00:00Z".into()),
                    ],
                ],
                affected_override: None,
                fail_rollback: false,
            })),
        }
    }
}

impl FakeDriver {
    pub fn boxed() -> Box<dyn Driver> {
        Box::new(Self::default())
    }

    pub fn force_next_affected(&self, affected: u64) {
        self.state.lock().unwrap().affected_override = Some(affected);
    }

    pub fn fail_next_rollback(&self) {
        self.state.lock().unwrap().fail_rollback = true;
    }
}

impl Driver for FakeDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Fake
    }

    fn test(&self, url: &str) -> Result<String, IdeError> {
        if url.starts_with("fake://") {
            Ok("fake ok".into())
        } else {
            Err(IdeError::new(
                Component::Db,
                "fake.test",
                ErrorKind::InvalidInput,
                "URL fake:// esperada",
                "use fake://local",
            ))
        }
    }

    fn connect(&self, _url: &str) -> Result<Box<dyn DbSession>, IdeError> {
        Ok(Box::new(FakeSession {
            state: self.state.clone(),
            in_tx: false,
            snapshot: None,
        }))
    }
}

impl DbSession for FakeSession {
    fn kind(&self) -> DriverKind {
        DriverKind::Fake
    }

    fn schema(&mut self) -> Result<SchemaMeta, IdeError> {
        Ok(SchemaMeta {
            tables: vec![TableMeta {
                name: "users".into(),
                kind: "table".into(),
                columns: vec![
                    ColumnMeta {
                        name: "id".into(),
                        type_name: "INTEGER".into(),
                        nullable: false,
                        primary_key: true,
                    },
                    ColumnMeta {
                        name: "name".into(),
                        type_name: "TEXT".into(),
                        nullable: true,
                        primary_key: false,
                    },
                ],
            }],
            partial_errors: vec![],
        })
    }

    fn execute(&mut self, sql: &str, _opts: &QueryOptions) -> Result<ExecResult, IdeError> {
        let state = self.state.lock().unwrap();
        if sql.to_ascii_lowercase().contains("invalid") {
            return Err(IdeError::new(
                Component::Db,
                "fake.execute",
                ErrorKind::InvalidInput,
                "SQL inválido simulado",
                "corrija o statement",
            ));
        }
        if sql.trim().to_ascii_lowercase().starts_with("select") {
            Ok(ExecResult::Rows {
                columns: vec![
                    "id".into(),
                    "name".into(),
                    "flag".into(),
                    "saldo".into(),
                    "blob".into(),
                    "quando".into(),
                ],
                rows: state.rows.clone(),
                total: Some(state.rows.len() as u64),
            })
        } else {
            Ok(ExecResult::Affected {
                count: 1,
                message: "ok".into(),
            })
        }
    }

    fn cancel(&self) {}

    fn in_transaction(&self) -> bool {
        self.in_tx
    }

    fn begin(&mut self) -> Result<(), IdeError> {
        self.snapshot = Some(self.state.lock().unwrap().rows.clone());
        self.in_tx = true;
        Ok(())
    }

    fn commit(&mut self) -> Result<(), IdeError> {
        self.snapshot = None;
        self.in_tx = false;
        Ok(())
    }

    fn rollback(&mut self) -> Result<(), IdeError> {
        let mut state = self.state.lock().unwrap();
        if state.fail_rollback {
            state.fail_rollback = false;
            return Err(IdeError::new(
                Component::Db,
                "fake.rollback",
                ErrorKind::Io,
                "falha de rollback simulada",
                "feche a sessão e descarte a transação",
            ));
        }
        if let Some(snapshot) = self.snapshot.take() {
            state.rows = snapshot;
        }
        self.in_tx = false;
        Ok(())
    }

    fn update_row(&mut self, update: &RowUpdate) -> Result<u64, IdeError> {
        let mut state = self.state.lock().unwrap();
        if let Some(affected) = state.affected_override.take() {
            return Ok(affected);
        }
        let Some(id) = update.key.iter().find_map(|(column, value)| {
            (column == "id")
                .then_some(value)
                .and_then(|value| match value {
                    Value::Int(id) => Some(*id),
                    _ => None,
                })
        }) else {
            return Ok(0);
        };
        let mut matched = 0;
        for row in &mut state.rows {
            if row.first() != Some(&Value::Int(id)) {
                continue;
            }
            let matches_old = update.changes.iter().all(|change| {
                let index = match change.column.as_str() {
                    "name" => 1,
                    "flag" => 2,
                    "saldo" => 3,
                    "blob" => 4,
                    "quando" => 5,
                    _ => return false,
                };
                row.get(index) == Some(&change.old)
            });
            if !matches_old {
                continue;
            }
            for change in &update.changes {
                let index = match change.column.as_str() {
                    "name" => 1,
                    "flag" => 2,
                    "saldo" => 3,
                    "blob" => 4,
                    "quando" => 5,
                    _ => continue,
                };
                row[index] = change.new.clone();
            }
            matched += 1;
        }
        Ok(matched)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{self, CellChange, PendingChanges};

    fn change(old: Value, new: Value) -> PendingChanges {
        let mut changes = PendingChanges::default();
        changes.push(CellChange {
            table: "users".into(),
            key: vec![("id".into(), Value::Int(1))],
            column: "name".into(),
            old,
            new,
        });
        changes
    }

    #[test]
    fn two_sessions_detect_external_change() {
        let driver = FakeDriver::default();
        let mut a = driver.connect("fake://").unwrap();
        let mut b = driver.connect("fake://").unwrap();
        edit::apply(
            &mut *b,
            &change(Value::Text("ana".into()), Value::Text("bia".into())),
        )
        .unwrap();
        let error = edit::apply(
            &mut *a,
            &change(Value::Text("ana".into()), Value::Text("carla".into())),
        )
        .unwrap_err();
        assert!(error.message.contains("conflito"));
        edit::apply(
            &mut *a,
            &change(Value::Text("bia".into()), Value::Text("carla".into())),
        )
        .unwrap();
    }

    #[test]
    fn affected_greater_than_one_is_not_success() {
        let driver = FakeDriver::default();
        driver.force_next_affected(2);
        let mut session = driver.connect("fake://").unwrap();
        let error = edit::apply(
            &mut *session,
            &change(Value::Text("ana".into()), Value::Text("bia".into())),
        )
        .unwrap_err();
        assert!(error.message.contains("esperado exatamente 1"));
    }
}
