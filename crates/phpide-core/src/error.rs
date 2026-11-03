//! Catálogo de erros — `T-NFR-005-01` (NFR-005).
//!
//! Toda falha exposta ao usuário indica **componente**, **operação** e uma
//! **ação possível** (NFR-005). A mensagem curta vai para a UI; o detalhe
//! técnico (cause, caminho, código de saída) fica no log correlacionado
//! (`log`). Formatos:
//!
//! - Display (UI): `falha ao salvar arquivo (core/save) — ação: verifique as permissões do diretório`
//! - Debug/log: estrutura completa com `detail`.
//!
//! O catálogo de códigos vive em `ErrorKind`; novos pontos de falha devem
//! reutilizar um kind existente ou acrescentar um variant com teste.

use std::borrow::Cow;
use std::fmt;
use std::io;
use std::path::Path;

/// Componente da IDE que produziu o erro (origem para correlação de logs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Component {
    Core,
    Ui,
    Lsp,
    Dbgp,
    Exec,
    Git,
    Search,
    Syntax,
    Php,
    Settings,
    Watcher,
    Plugins,
}

impl Component {
    pub const fn as_str(self) -> &'static str {
        match self {
            Component::Core => "core",
            Component::Ui => "ui",
            Component::Lsp => "lsp",
            Component::Dbgp => "dbgp",
            Component::Exec => "exec",
            Component::Git => "git",
            Component::Search => "search",
            Component::Syntax => "syntax",
            Component::Php => "php",
            Component::Settings => "settings",
            Component::Watcher => "watcher",
            Component::Plugins => "plugins",
        }
    }
}

impl fmt::Display for Component {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Código estável da falha (catálogo). Aparece nos logs para triagem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// Executável/arquivo não encontrado.
    NotFound,
    /// Permissão negada (arquivo não executável, diretório proibido).
    PermissionDenied,
    /// Tempo esgotado.
    Timeout,
    /// Operação cancelada pelo usuário.
    Cancelled,
    /// Falha ao iniciar processo.
    SpawnFailed,
    /// Configuração inválida ou incompatível (versão futura de formato).
    InvalidConfig,
    /// Versão de ferramenta incompatível com o requisito.
    IncompatibleVersion,
    /// Falha de leitura/escrita genérica.
    Io,
    /// Entrada inesperada (resposta inválida, JSON quebrado).
    InvalidInput,
    /// Falha sem categoria específica.
    Other,
}

impl ErrorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorKind::NotFound => "not_found",
            ErrorKind::PermissionDenied => "permission_denied",
            ErrorKind::Timeout => "timeout",
            ErrorKind::Cancelled => "cancelled",
            ErrorKind::SpawnFailed => "spawn_failed",
            ErrorKind::InvalidConfig => "invalid_config",
            ErrorKind::IncompatibleVersion => "incompatible_version",
            ErrorKind::Io => "io",
            ErrorKind::InvalidInput => "invalid_input",
            ErrorKind::Other => "other",
        }
    }
}

/// Erro padronizado da IDE (NFR-005-01).
#[derive(Debug, Clone)]
pub struct IdeError {
    pub component: Component,
    /// Operação em curso, em inglês curto e estável: "save", "spawn", "scan".
    pub operation: Cow<'static, str>,
    pub kind: ErrorKind,
    /// Mensagem curta e acionável para o usuário (uma linha).
    pub message: String,
    /// Ação recomendada para resolver (uma linha, imperativa).
    pub action: String,
    /// Detalhe técnico para o log (causa, path, stderr); nunca segredos.
    pub detail: Option<String>,
}

impl IdeError {
    pub fn new(
        component: Component,
        operation: impl Into<Cow<'static, str>>,
        kind: ErrorKind,
        message: impl Into<String>,
        action: impl Into<String>,
    ) -> Self {
        IdeError {
            component,
            operation: operation.into(),
            kind,
            message: message.into(),
            action: action.into(),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Executável/ferramenta não encontrada: orienta configuração.
    pub fn not_found(component: Component, operation: &str, what: &str) -> Self {
        IdeError::new(
            component,
            short_op(operation),
            ErrorKind::NotFound,
            format!("{what} não encontrado"),
            format!("instale {what} ou configure o caminho em Settings"),
        )
    }

    /// Permissão negada: orienta ajuste de modo/credencial.
    pub fn permission_denied(component: Component, operation: &str, what: &str) -> Self {
        IdeError::new(
            component,
            short_op(operation),
            ErrorKind::PermissionDenied,
            format!("permissão negada em {what}"),
            "ajuste as permissões (chmod/owner) ou escolha outro caminho",
        )
    }

    pub fn timeout(component: Component, operation: &str, limit: std::time::Duration) -> Self {
        IdeError::new(
            component,
            short_op(operation),
            ErrorKind::Timeout,
            format!("operação excedeu {:?}", limit),
            "tente novamente; se persistir, verifique a carga do sistema",
        )
    }

    pub fn cancelled(component: Component, operation: &str) -> Self {
        IdeError::new(
            component,
            short_op(operation),
            ErrorKind::Cancelled,
            "operação cancelada",
            "repita a operação se ainda for necessária",
        )
    }

    /// Converte io::Error nos kinds acionáveis (NFR-003: distinguir ausência
    /// de ferramenta, permissão e falha genérica).
    pub fn from_io(component: Component, operation: &str, what: &str, e: &io::Error) -> Self {
        use io::ErrorKind::*;
        let base = match e.kind() {
            NotFound => Self::not_found(component, operation, what),
            PermissionDenied => Self::permission_denied(component, operation, what),
            _ => IdeError::new(
                component,
                short_op(operation),
                ErrorKind::Io,
                format!("falha em {what}: {e}"),
                "verifique o log para detalhes e tente novamente",
            ),
        };
        base.with_detail(format!("io error {what}: {e}"))
    }

    /// Falha ao iniciar processo (mapa de io::Error de spawn).
    pub fn spawn(component: Component, program: &Path, e: &io::Error) -> Self {
        use io::ErrorKind::*;
        let what = program.display().to_string();
        match e.kind() {
            NotFound => Self::not_found(component, "spawn", &what),
            PermissionDenied => Self::permission_denied(component, "spawn", &what),
            _ => IdeError::new(
                component,
                "spawn",
                ErrorKind::SpawnFailed,
                format!("falha ao iniciar {what}"),
                "verifique o comando e as permissões; detalhes no log",
            )
            .with_detail(format!("spawn {what}: {e}")),
        }
    }

    /// Formato de uma linha de log: `[component/operação/kind] mensagem — ação: ...`
    pub fn log_line(&self) -> String {
        let detail = self
            .detail
            .as_ref()
            .map(|d| format!(" | {d}"))
            .unwrap_or_default();
        format!(
            "[{}/{}/{}] {} — ação: {}{}",
            self.component,
            self.operation,
            self.kind.as_str(),
            self.message,
            self.action,
            detail
        )
    }
}

/// Operações curtas e estáveis para correlação (sem espaços/paths).
fn short_op(operation: &str) -> Cow<'static, str> {
    let owned: String = operation
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    Cow::Owned(owned)
}

impl fmt::Display for IdeError {
    /// Mensagem para a UI: curta, com componente/operação e ação (NFR-005).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}/{}) — ação: {}",
            self.message, self.component, self.operation, self.action
        )
    }
}

impl std::error::Error for IdeError {}

impl From<IdeError> for io::Error {
    fn from(e: IdeError) -> Self {
        io::Error::other(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_has_component_operation_and_action() {
        let e = IdeError::not_found(Component::Exec, "discover", "php");
        let s = e.to_string();
        assert!(s.contains("(exec/discover)"), "{s}");
        assert!(s.contains("ação:"), "{s}");
        assert!(s.contains("php não encontrado"), "{s}");
    }

    #[test]
    fn log_line_includes_code_and_detail() {
        let e = IdeError::from_io(
            Component::Core,
            "save",
            "/p/a.php",
            &io::Error::other("disco cheio"),
        )
        .with_detail("errno 28");
        let line = e.log_line();
        assert!(line.contains("[core/save/io]"), "{line}");
        assert!(line.contains("disco cheio"), "{line}");
        assert!(line.contains("errno 28"), "{line}");
    }

    #[test]
    fn io_mapping_distinguishes_cases() {
        let nf = IdeError::from_io(
            Component::Git,
            "status",
            "git",
            &io::Error::from(io::ErrorKind::NotFound),
        );
        assert_eq!(nf.kind, ErrorKind::NotFound);
        assert!(nf.action.contains("Settings") || nf.action.contains("instale"));
        let pd = IdeError::from_io(
            Component::Core,
            "open",
            "/etc/shadow",
            &io::Error::from(io::ErrorKind::PermissionDenied),
        );
        assert_eq!(pd.kind, ErrorKind::PermissionDenied);
        assert!(pd.action.contains("permiss"));
    }

    #[test]
    fn spawn_mapping_paths_with_spaces_survive_message() {
        let e = IdeError::spawn(
            Component::Exec,
            Path::new("/opt/minhas ferramentas/php"),
            &io::Error::from(io::ErrorKind::NotFound),
        );
        assert_eq!(e.kind, ErrorKind::NotFound);
        assert!(e.message.contains("/opt/minhas ferramentas/php"));
    }
}
