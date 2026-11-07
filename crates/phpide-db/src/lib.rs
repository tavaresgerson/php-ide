//! phpide-db — Database Tools (REQUISITOS §10): drivers, segredos, data
//! sources, introspection, console, grade e edição transacional.
//!
//! Onda 1 (fundação e leitura) entregue neste recorte; onda 2 (edição) tem
//! modelo completo (`edit`) validado no SQLite/fake. PostgreSQL/MySQL:
//! contratos prontos, implementação bloqueada por ambiente (documentado em
//! `drivers::remote`).

pub mod console;
pub mod contracts;
pub mod edit;
pub mod grid;
pub mod secrets;
pub mod value;

pub mod drivers {
    pub mod fake;
    pub mod remote;
    pub mod sqlite;
}

pub use console::{split_statements, ConsoleSession, History};
pub use contracts::*;
pub use edit::{preview_dml, PendingChanges};
pub use grid::{paginate, GridPage};
pub use secrets::{DataSource, DataSourceStore};
pub use value::Value;

#[cfg(test)]
mod tests {
    //! Gate Fase 8 — explorer, console e grade read-only no SQLite real +
    //! fake; edição transacional validada separadamente (DB-006) — os dois
    //! lados do gate do recorte. PG/MySQL: sem servidor instalável sem root
    //! neste ambiente — drivers falham com diagnóstico claro (documentado).

    use super::drivers::fake::FakeDriver;
    use super::*;
    use phpide_core::ErrorKind;

    fn sqlite_memory_with_data() -> Box<dyn DbSession> {
        let d = drivers::sqlite::SqliteDriver::boxed();
        let mut s = d.connect("sqlite://:memory:").unwrap();
        s.execute(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, saldo REAL, blob BLOB, criado_em TEXT)",
            &QueryOptions::default(),
        )
        .unwrap();
        s.execute(
            "INSERT INTO users VALUES (1, 'ana', 10.5, X'CAFE', '2024-01-01'), (2, NULL, NULL, NULL, NULL)",
            &QueryOptions::default(),
        )
        .unwrap();
        s
    }

    #[test]
    fn gate_explorer_console_grid_sqlite_real() {
        let mut s = sqlite_memory_with_data();

        // Explorer/introspection (DB-003-01) + tipos preservados (DB-001-02).
        let meta = s.schema().unwrap();
        let users = meta.tables.iter().find(|t| t.name == "users").unwrap();
        assert_eq!(users.columns[0].name, "id");
        assert!(users.columns[0].primary_key);
        assert!(users.columns[1].nullable);

        // Console (DB-004-03) com identificação de statements (DB-004-02).
        let stmts = split_statements("select id from users; select name from users;");
        assert_eq!(stmts.len(), 2);
        let mut session = ConsoleSession::new(s);
        let r = session
            .run("SELECT id, name, saldo, blob, criado_em FROM users ORDER BY id")
            .unwrap();
        let ExecResult::Rows { columns, rows, .. } = r else {
            panic!("rows")
        };
        assert_eq!(columns.len(), 5);
        assert_eq!(rows.len(), 2);
        assert!(matches!(rows[1][1], Value::Null), "NULL preservado");
        assert!(matches!(rows[0][2], Value::Float(_)), "REAL preservado");
        assert!(matches!(rows[0][3], Value::Bytes(_)), "binário preservado");
        assert_eq!(rows[0][4].display(), "2024-01-01");
        assert_eq!(session.history.len(), 1);

        // Grade paginada read-only (DB-005-01/03): server-side LIMIT/ORDER.
        let mut raw = session.session;
        let page = paginate(
            &mut *raw,
            "users",
            0,
            1,
            &QueryOptions {
                order_by: Some(("name".into(), true)),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page.rows.len(), 1, "LIMIT 1 server-side");
        assert_eq!(page.offset, 0);
    }

    #[test]
    fn gate_fake_covers_contract_and_driver_swap() {
        // Aceite DB-001: driver fake cobre o contrato da UI sem banco.
        let d = FakeDriver::boxed();
        assert_eq!(d.kind(), DriverKind::Fake);
        let ping = d.test("fake://local").unwrap();
        assert!(ping.contains("ok"));
        let mut s = d.connect("fake://local").unwrap();
        let meta = s.schema().unwrap();
        assert_eq!(meta.tables[0].name, "users");
        let ExecResult::Rows { rows, .. } = s
            .execute("SELECT * FROM users", &QueryOptions::default())
            .unwrap()
        else {
            panic!("rows")
        };
        assert!(rows.iter().any(|r| r.iter().any(|v| v.is_null())));
        assert!(rows
            .iter()
            .any(|r| r.iter().any(|v| matches!(v, Value::Decimal(_)))));
        assert!(rows
            .iter()
            .any(|r| r.iter().any(|v| matches!(v, Value::Bytes(_)))));
    }

    #[test]
    fn gate_editing_transactional_sqlite_real() {
        let mut s = sqlite_memory_with_data();

        // Edição (DB-006-02) com Preview DML (DB-006-03) e pendentes/Revert.
        let mut p = PendingChanges::default();
        p.push(edit::CellChange {
            table: "users".into(),
            key: vec![("id".into(), Value::Int(1))],
            column: "name".into(),
            old: Value::Text("ana".into()),
            new: Value::Text("ana maria".into()),
        });
        let dml = preview_dml(&p, DriverKind::Sqlite);
        assert!(
            dml.contains("UPDATE \"users\" SET \"name\" = 'ana maria' WHERE \"id\" = 1;"),
            "{dml}"
        );
        assert_eq!(edit::apply(&mut *s, &p).unwrap(), 1);
        let r = s
            .execute(
                "SELECT name FROM users WHERE id = 1",
                &QueryOptions::default(),
            )
            .unwrap();
        let ExecResult::Rows { rows, .. } = r else {
            panic!("rows")
        };
        assert_eq!(rows[0][0].display(), "ana maria");

        // Transação (DB-006-04): begin → mudança → rollback não persiste.
        s.begin().unwrap();
        assert!(s.in_transaction());
        let mut p2 = PendingChanges::default();
        p2.push(edit::CellChange {
            table: "users".into(),
            key: vec![("id".into(), Value::Int(2))],
            column: "name".into(),
            old: Value::Null,
            new: Value::Text("beto".into()),
        });
        edit::apply(&mut *s, &p2).unwrap();
        s.rollback().unwrap();
        let r = s
            .execute(
                "SELECT name FROM users WHERE id = 2",
                &QueryOptions::default(),
            )
            .unwrap();
        let ExecResult::Rows { rows, .. } = r else {
            panic!("rows")
        };
        assert!(rows[0][0].is_null(), "rollback desfaz a edição");

        // Conflito de concorrência (DB-006-05): chave inexistente → 0 linhas.
        let mut p3 = PendingChanges::default();
        p3.push(edit::CellChange {
            table: "users".into(),
            key: vec![("id".into(), Value::Int(999))],
            column: "name".into(),
            old: Value::Text("?".into()),
            new: Value::Text("x".into()),
        });
        let err = edit::apply(&mut *s, &p3).unwrap_err();
        assert!(err.message.contains("conflito"), "{err:?}");
    }

    #[test]
    fn gate_remote_drivers_fail_with_clear_diagnostic_not_fake_support() {
        // PG/MySQL: sem servidor no ambiente — driver falha explícito e NÃO
        // anuncia suporte (sem conteúdo fictício, como os demais domínios).
        let pg = drivers::remote::driver_for("postgresql://u@localhost:1/db").unwrap();
        let err = pg.test("postgresql://u@localhost:1/db").unwrap_err();
        assert_eq!(err.kind, ErrorKind::NotFound);
        assert!(err.message.contains("postgresql"), "{err:?}");
        let my = drivers::remote::driver_for("mysql://u@localhost:1/db").unwrap();
        assert_eq!(my.kind(), DriverKind::MySql);
        // Test Connection do SQLite real (DB-002-03) — diagnóstico real.
        let sq = drivers::remote::driver_for("sqlite://:memory:").unwrap();
        assert!(sq.test("sqlite://:memory:").unwrap().starts_with("SQLite"));
    }
}
