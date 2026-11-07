//! Driver fake — `T-DB-001-03`: cobre o contrato inteiro da UI sem banco
//! (aceite DB-001). Scriptável: tabelas fixas com tipos variados (NULL,
//! binário, decimal, data) e falha simulável de conflito.

use phpide_core::{Component, ErrorKind, IdeError};

use crate::contracts::*;
use crate::value::Value;

pub struct FakeDriver;
pub struct FakeSession {
    in_tx: bool,
    /// Simula conflito de concorrência no próximo update.
    pub conflict_next: bool,
    rows: Vec<Vec<Value>>,
}

impl FakeDriver {
    pub fn boxed() -> Box<dyn Driver> {
        Box::new(FakeDriver)
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
                Component::Core,
                "fake.test",
                ErrorKind::InvalidInput,
                "URL fake:// esperada",
                "use fake://local",
            ))
        }
    }
    fn connect(&self, _url: &str) -> Result<Box<dyn DbSession>, IdeError> {
        Ok(Box::new(FakeSession {
            in_tx: false,
            conflict_next: false,
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
        let s = sql.trim().to_ascii_lowercase();
        if s.starts_with("select") {
            Ok(ExecResult::Rows {
                columns: vec![
                    "id".into(),
                    "name".into(),
                    "flag".into(),
                    "saldo".into(),
                    "blob".into(),
                    "quando".into(),
                ],
                rows: self.rows.clone(),
                total: Some(self.rows.len() as u64),
            })
        } else if s.starts_with("update") && self.conflict_next {
            self.conflict_next = false;
            Ok(ExecResult::Affected {
                count: 0,
                message: "conflito simulado".into(),
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
        self.in_tx = true;
        Ok(())
    }
    fn commit(&mut self) -> Result<(), IdeError> {
        self.in_tx = false;
        Ok(())
    }
    fn rollback(&mut self) -> Result<(), IdeError> {
        self.in_tx = false;
        Ok(())
    }
    fn update_row(
        &mut self,
        _t: &str,
        _k: &[(String, Value)],
        _c: &[(String, Value)],
    ) -> Result<u64, IdeError> {
        if self.conflict_next {
            self.conflict_next = false;
            return Ok(0);
        }
        Ok(1)
    }
}
