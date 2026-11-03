//! Descoberta configurável de executáveis — `T-NFR-003-01`, `T-NFR-003-04` (NFR-003).
//!
//! Prioridade: **configuração explícita antes do PATH** (NFR-003). A origem
//! da descoberta é reportada em [`ToolInfo::source`] para diagnóstico. Toda
//! falha devolve [`IdeError`] com ação recomendada — ausência, permissão e
//! versão incompatível são casos distintos (matriz mínima de falhas, §5.2).

use phpide_core::{Component, ErrorKind, IdeError};
use std::path::{Path, PathBuf};

/// De onde veio o executável escolhido (diagnóstico de origem, NFR-003-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Caminho explícito (settings / variável de ambiente da IDE).
    Explicit,
    /// Encontrado no PATH do sistema.
    Path,
}

impl Source {
    pub const fn as_str(self) -> &'static str {
        match self {
            Source::Explicit => "explicit",
            Source::Path => "path",
        }
    }
}

/// Pedido de descoberta de uma ferramenta.
pub struct ToolRequest<'a> {
    /// Nome do executável ("php", "composer", "git") e rótulo humano para
    /// mensagens ("PHP", "Composer").
    pub name: &'a str,
    pub label: &'a str,
    /// Configuração explícita (settings ou env); tem prioridade sobre PATH.
    pub explicit: Option<PathBuf>,
    /// Argumentos para consultar a versão (default: `["--version"]`).
    pub version_args: &'a [&'a str],
    /// Versão mínima exigida `(maior, menor)`, quando aplicável.
    pub min_version: Option<(u64, u64)>,
}

impl<'a> ToolRequest<'a> {
    pub fn new(name: &'a str, label: &'a str) -> Self {
        ToolRequest {
            name,
            label,
            explicit: None,
            version_args: &["--version"],
            min_version: None,
        }
    }

    pub fn explicit(mut self, path: impl Into<PathBuf>) -> Self {
        self.explicit = Some(path.into());
        self
    }

    pub fn min_version(mut self, major: u64, minor: u64) -> Self {
        self.min_version = Some((major, minor));
        self
    }
}

/// Executável localizado + versão detectada.
#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub program: PathBuf,
    pub source: Source,
    /// Primeira linha da saída de `--version` (quando disponível).
    pub version: Option<String>,
    /// Versão parseada `(major, minor, patch)` quando identificável.
    pub version_parsed: Option<(u64, u64, u64)>,
}

/// `true` se o arquivo existe e tem bit executável (Unix) ou existe (outros).
fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Procura `name` no PATH, sem shell (paths com espaços são naturais).
pub fn which_in_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// Extrai a primeira versão `x.y.z` (ou `x.y`) de um texto.
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut it = text
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty());
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    let patch = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    Some((major, minor, patch))
}

/// Executa `--version` com timeout curto; falha não são fatais para a
/// descoberta (a ferramenta existe mesmo se a versão não foi obtida).
/// Retries com backoff em ETXTBSY/EAGAIN: executar um binário imediatamente
/// após criá-lo pode colidir com a liberação do inode no kernel (condição
/// transiente observada sob carga de spawns concorrentes).
fn probe_version(program: &Path, args: &[&str]) -> (Option<String>, Option<(u64, u64, u64)>) {
    let mut attempt = 0;
    loop {
        let output = std::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output();
        match output {
            Ok(out) => {
                let text = String::from_utf8_lossy(&out.stdout);
                let first_line = text.lines().next().unwrap_or("").trim().to_string();
                if first_line.is_empty() {
                    let stderr_text = String::from_utf8_lossy(&out.stderr);
                    let line = stderr_text.lines().next().unwrap_or("").trim().to_string();
                    return (None, parse_version(&line));
                }
                let parsed = parse_version(&first_line);
                return (Some(first_line), parsed);
            }
            Err(e)
                if attempt < 5
                    && matches!(
                        e.kind(),
                        std::io::ErrorKind::ExecutableFileBusy | std::io::ErrorKind::WouldBlock
                    ) =>
            {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(20 * attempt));
            }
            Err(_) => return (None, None),
        }
    }
}

/// Descobre uma ferramenta com prioridade configuração > PATH e valida a
/// versão mínima exigida. Ver `ToolRequest` para os campos.
pub fn discover(req: &ToolRequest) -> Result<ToolInfo, IdeError> {
    // 1. Configuração explícita (settings / env) tem prioridade absoluta.
    if let Some(explicit) = &req.explicit {
        if !explicit.exists() {
            return Err(IdeError::new(
                Component::Exec,
                "discover",
                ErrorKind::NotFound,
                format!("{} não encontrado em {}", req.label, explicit.display()),
                format!("ajuste o caminho de {} em Settings", req.label),
            )
            .with_detail(format!("caminho configurado: {}", explicit.display())));
        }
        if !is_executable(explicit) {
            return Err(IdeError::new(
                Component::Exec,
                "discover",
                ErrorKind::PermissionDenied,
                format!(
                    "{} configurado sem permissão de execução: {}",
                    req.label,
                    explicit.display()
                ),
                "chmod +x no arquivo ou escolha outro interpretador em Settings",
            ));
        }
        return finish(
            ToolInfo {
                program: explicit.clone(),
                source: Source::Explicit,
                version: None,
                version_parsed: None,
            },
            req,
        );
    }

    // 2. PATH do sistema. Arquivo existe mas sem bit de execução é erro
    // distinto de "não encontrado" (matriz de falhas, §5.2).
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(req.name);
            if candidate.exists() && !is_executable(&candidate) {
                return Err(IdeError::new(
                    Component::Exec,
                    "discover",
                    ErrorKind::PermissionDenied,
                    format!(
                        "{} existe em {} sem permissão de execução",
                        req.label,
                        candidate.display()
                    ),
                    "ajuste as permissões (chmod +x) ou configure outro caminho em Settings",
                ));
            }
        }
    }
    let Some(found) = which_in_path(req.name) else {
        return Err(IdeError::not_found(Component::Exec, "discover", req.name)
            .with_detail(format!("{} não está no PATH nem configurado", req.label)));
    };
    finish(
        ToolInfo {
            program: found,
            source: Source::Path,
            version: None,
            version_parsed: None,
        },
        req,
    )
}

/// Sonda a versão e valida o mínimo exigido.
fn finish(mut info: ToolInfo, req: &ToolRequest) -> Result<ToolInfo, IdeError> {
    let (version, parsed) = probe_version(&info.program, req.version_args);
    info.version = version;
    info.version_parsed = parsed;
    if let (Some((want_major, want_minor)), Some((major, minor, _))) =
        (req.min_version, info.version_parsed)
    {
        if (major, minor) < (want_major, want_minor) {
            return Err(IdeError::new(
                Component::Exec,
                "discover",
                ErrorKind::IncompatibleVersion,
                format!(
                    "{} {}.{} é incompatível (mínimo {}.{})",
                    req.label, major, minor, want_major, want_minor
                ),
                format!(
                    "atualize {} ou configure outro caminho em Settings",
                    req.label
                ),
            )
            .with_detail(format!(
                "programa: {} | detectado: {}.{}",
                info.program.display(),
                major,
                minor
            )));
        }
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn parses_versions_from_tool_output() {
        assert_eq!(parse_version("PHP 8.3.6 (cli)"), Some((8, 3, 6)));
        assert_eq!(
            parse_version("Composer version 2.7.4 2024-03-27"),
            Some((2, 7, 4))
        );
        assert_eq!(parse_version("git version 2.43.0"), Some((2, 43, 0)));
        assert_eq!(parse_version("v1.2"), Some((1, 2, 0)));
        assert_eq!(parse_version("sem versão"), None);
    }

    #[test]
    fn missing_explicit_path_is_actionable_not_found() {
        let req = ToolRequest::new("php", "PHP").explicit("/nao/existe/php");
        let e = discover(&req).unwrap_err();
        assert_eq!(e.kind, ErrorKind::NotFound);
        assert!(e.action.contains("Settings"));
        assert!(e.message.contains("/nao/existe/php"), "path com ação");
    }

    #[test]
    fn explicit_non_executable_is_permission_error() {
        let dir = std::env::temp_dir().join(format!("phpide-disc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("php");
        std::fs::write(&fake, "#!/bin/sh\necho 8.3\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o644)).unwrap();
            let req = ToolRequest::new("php", "PHP").explicit(&fake);
            let e = discover(&req).unwrap_err();
            assert_eq!(e.kind, ErrorKind::PermissionDenied);
            assert!(e.action.contains("chmod"));
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn path_lookup_finds_system_tools() {
        // sh existe em qualquer Linux; valida o lookup sem depender de php.
        let found = which_in_path("sh");
        assert!(
            found.is_some(),
            "sh deve estar no PATH do ambiente de teste"
        );
    }

    #[test]
    fn io_error_kind_mapping_keeps_cases_distinct() {
        let io_err = io::Error::from(io::ErrorKind::NotFound);
        let e = IdeError::from_io(Component::Exec, "open", "php", &io_err);
        assert_eq!(e.kind, ErrorKind::NotFound);
    }
}
