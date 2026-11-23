//! Registro dos drivers de rede e seleção pelo esquema da URL.

use phpide_core::IdeError;

use crate::contracts::*;

pub fn builtin_drivers() -> Vec<Box<dyn Driver>> {
    vec![
        crate::drivers::sqlite::SqliteDriver::boxed(),
        crate::drivers::fake::FakeDriver::boxed(),
        crate::drivers::postgres::PostgresDriver::boxed(),
        crate::drivers::mysql::MysqlDriver::boxed(),
    ]
}

pub fn driver_for(url: &str) -> Result<Box<dyn Driver>, IdeError> {
    let (kind, _) = parse_url(url)?;
    Ok(match kind {
        DriverKind::Sqlite => crate::drivers::sqlite::SqliteDriver::boxed(),
        DriverKind::Fake => crate::drivers::fake::FakeDriver::boxed(),
        DriverKind::PostgreSql => crate::drivers::postgres::PostgresDriver::boxed(),
        DriverKind::MySql => crate::drivers::mysql::MysqlDriver::boxed(),
    })
}
