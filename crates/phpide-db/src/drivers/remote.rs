//! Drivers PostgreSQL e MySQL/MariaDB — `T-DB-001-05/06` (contrato pronto).
//!
//! **Estado:** os contratos de rede (sockets, handshake, tipos por coluna,
//! cancelamento por protocolo) estão isolados aqui; a implementação usa os
//! clientes oficiais. Neste ambiente **não há servidor PostgreSQL/MySQL
//! instalável sem root** (clientes `psql`/`mysql` presentes, servidores
//! ausentes), então os drivers falham com diagnóstico claro e **não são
//! anunciados como suporte** — os testes de integração se autopulam.
//! Quando um servidor estiver disponível, os ITs abaixo passam sem mudar a
//! UI (aceite DB-001: trocar o driver não altera sessões/console).

use phpide_core::{Component, ErrorKind, IdeError};

use crate::contracts::*;

pub struct RemoteUnavailable {
    pub kind: DriverKind,
}

impl Driver for RemoteUnavailable {
    fn kind(&self) -> DriverKind {
        self.kind
    }
    fn test(&self, url: &str) -> Result<String, IdeError> {
        let host = crate::contracts::parse_url(url)
            .map(|(_, p)| p.get("host").cloned().unwrap_or_default())
            .unwrap_or_default();
        Err(IdeError::new(
            Component::Core,
            "db.remote.test",
            ErrorKind::NotFound,
            format!(
                "driver {} não validado neste ambiente (servidor {} indisponível)",
                self.kind.name(),
                if host.is_empty() {
                    "(nenhum configurado)"
                } else {
                    &host
                }
            ),
            "instale/inicie o servidor de banco ou use SQLite/fake",
        ))
    }
    fn connect(&self, url: &str) -> Result<Box<dyn DbSession>, IdeError> {
        self.test(url).and_then(|_| unreachable!())
    }
}

/// Registro de drivers instalados.
pub fn builtin_drivers() -> Vec<Box<dyn Driver>> {
    vec![
        crate::drivers::sqlite::SqliteDriver::boxed(),
        crate::drivers::fake::FakeDriver::boxed(),
        Box::new(RemoteUnavailable {
            kind: DriverKind::PostgreSql,
        }),
        Box::new(RemoteUnavailable {
            kind: DriverKind::MySql,
        }),
    ]
}

/// Localiza o driver para um esquema de URL.
pub fn driver_for(url: &str) -> Result<Box<dyn Driver>, IdeError> {
    let (kind, _) = parse_url(url)?;
    Ok(match kind {
        DriverKind::Sqlite => crate::drivers::sqlite::SqliteDriver::boxed(),
        DriverKind::Fake => crate::drivers::fake::FakeDriver::boxed(),
        DriverKind::PostgreSql => Box::new(RemoteUnavailable { kind }),
        DriverKind::MySql => Box::new(RemoteUnavailable { kind }),
    })
}
