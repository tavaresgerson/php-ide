//! Contratos dos drivers — `T-DB-001-01` (DB-001 P0).
//!
//! Interfaces separadas para conexão, metadados, execução, cancelamento,
//! transações e edição. Trocar de driver (fake ↔ SQLite ↔ PostgreSQL ↔
//! MySQL) não altera o modelo de sessões nem o query console — o aceite
//! DB-001.

use std::collections::BTreeMap;

use phpide_core::IdeError;

use crate::value::Value;

/// Identidade de um driver instalado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverKind {
    Fake,
    Sqlite,
    PostgreSql,
    MySql,
}

impl DriverKind {
    pub const fn name(self) -> &'static str {
        match self {
            DriverKind::Fake => "fake",
            DriverKind::Sqlite => "sqlite",
            DriverKind::PostgreSql => "postgresql",
            DriverKind::MySql => "mysql",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "fake" => Some(DriverKind::Fake),
            "sqlite" => Some(DriverKind::Sqlite),
            "postgresql" | "postgres" => Some(DriverKind::PostgreSql),
            "mysql" | "mariadb" => Some(DriverKind::MySql),
            _ => None,
        }
    }
}

/// Coluna de uma tabela (introspection).
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnMeta {
    pub name: String,
    pub type_name: String,
    pub nullable: bool,
    pub primary_key: bool,
}

/// Visão de uma tabela/vista.
#[derive(Debug, Clone, PartialEq)]
pub struct TableMeta {
    pub name: String,
    /// "table" | "view".
    pub kind: String,
    pub columns: Vec<ColumnMeta>,
}

/// Esquema completo (introspection DB-003-01).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SchemaMeta {
    pub tables: Vec<TableMeta>,
    /// Erros parciais por objeto (introspection com falhas não derruba tudo).
    pub partial_errors: Vec<String>,
}

/// Resultado de uma execução no console.
#[derive(Debug, Clone, PartialEq)]
pub enum ExecResult {
    /// SELECT: colunas + linhas (janela paginada pela grade).
    Rows {
        columns: Vec<String>,
        rows: Vec<Vec<Value>>,
        total: Option<u64>,
    },
    /// DML/DDL: linhas afetadas + mensagem.
    Affected { count: u64, message: String },
}

/// Filtro/ordenação server-side (DB-005-03).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct QueryOptions {
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    /// (coluna, ascendente).
    pub order_by: Option<(String, bool)>,
    /// Expressão WHERE (já validada/quotada pelo chamador — grade interna).
    pub filter: Option<String>,
}

/// Uma sessão conectada (cancelável, transacional).
pub trait DbSession: Send {
    fn kind(&self) -> DriverKind;
    /// Introspection do catálogo (DB-003-01).
    fn schema(&mut self) -> Result<SchemaMeta, IdeError>;
    /// Executa um statement (DB-004-03). Não executa múltiplos statements.
    fn execute(&mut self, sql: &str, opts: &QueryOptions) -> Result<ExecResult, IdeError>;
    /// Cancela a execução em curso (DB-004-04).
    fn cancel(&self);
    /// Estado da transação (DB-006-04).
    fn in_transaction(&self) -> bool;
    fn begin(&mut self) -> Result<(), IdeError>;
    fn commit(&mut self) -> Result<(), IdeError>;
    fn rollback(&mut self) -> Result<(), IdeError>;
    /// Edição de linha: aplica um UPDATE por chave (DB-006-02), com checagem
    /// de concorrência (linhas afetadas ≠ 1 → conflito, DB-006-05).
    fn update_row(
        &mut self,
        table: &str,
        key: &[(String, Value)],
        changes: &[(String, Value)],
    ) -> Result<u64, IdeError>;
}

/// Fábrica de sessões ( Driver provider ).
pub trait Driver: Send + Sync {
    fn kind(&self) -> DriverKind;
    /// Test Connection (DB-002-03): abre, pinga, fecha; diagnóstico no erro.
    fn test(&self, url: &str) -> Result<String, IdeError>;
    fn connect(&self, url: &str) -> Result<Box<dyn DbSession>, IdeError>;
}

/// Parser de URL de conexão (sqlite://path, fake://, postgresql://user:pass@host/db).
pub fn parse_url(url: &str) -> Result<(DriverKind, BTreeMap<String, String>), IdeError> {
    let (scheme, rest) = url.split_once("://").ok_or_else(|| {
        IdeError::new(
            phpide_core::Component::Core,
            "db.parse_url",
            phpide_core::ErrorKind::InvalidInput,
            format!("URL sem esquema: {url}"),
            "use sqlite://caminho.db, fake:// ou postgresql://…",
        )
    })?;
    let kind = DriverKind::from_name(scheme).ok_or_else(|| {
        IdeError::new(
            phpide_core::Component::Core,
            "db.parse_url",
            phpide_core::ErrorKind::InvalidInput,
            format!("driver desconhecido: {scheme}"),
            "use sqlite, fake, postgresql ou mysql",
        )
    })?;
    let mut params = BTreeMap::new();
    match kind {
        DriverKind::Sqlite | DriverKind::Fake => {
            params.insert("path".into(), rest.to_string());
        }
        DriverKind::PostgreSql | DriverKind::MySql => {
            // user:pass@host:port/db?sslmode=disable
            let (auth, hostdb) = rest.split_once('@').unwrap_or(("", rest));
            if let Some((u, p)) = auth.split_once(':') {
                params.insert("user".into(), u.to_string());
                params.insert("password".into(), p.to_string());
            }
            let (hostport, db) = hostdb.split_once('/').unwrap_or((hostdb, ""));
            let (host, port) = hostport.split_once(':').unwrap_or((hostport, ""));
            params.insert("host".into(), host.to_string());
            params.insert("port".into(), port.to_string());
            params.insert("db".into(), db.split('?').next().unwrap_or("").to_string());
        }
    }
    Ok((kind, params))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_urls() {
        let (k, p) = parse_url("sqlite:///tmp/x.db").unwrap();
        assert_eq!(k, DriverKind::Sqlite);
        assert_eq!(p["path"], "/tmp/x.db");
        let (k, p) = parse_url("postgresql://u:pw@localhost:5432/appdb").unwrap();
        assert_eq!(k, DriverKind::PostgreSql);
        assert_eq!(
            (p["user"].as_str(), p["password"].as_str(), p["db"].as_str()),
            ("u", "pw", "appdb")
        );
        let err = parse_url("sem-esquema").unwrap_err();
        assert_eq!(err.kind, phpide_core::ErrorKind::InvalidInput);
    }
}
