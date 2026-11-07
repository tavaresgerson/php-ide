//! Modelo versionado de Run Configuration — `T-RUN-001-01` (RUN-001).
//!
//! - Formato com `version` explícita e parse com migração incremental
//!   (NFR-010): versões futuras entram pela cadeia `migrate`, versão
//!   desconhecida vira `IdeError` de `InvalidConfig` — nunca panic;
//! - campos comuns de RUN-001: nome, working directory, ambiente,
//!   interpreter, allow multiple instances, store as project file;
//! - campos específicos por tipo (RUN-002) com validação de campos
//!   (`validate`) — erros bloqueiam a execução, avisos não;
//! - serialização legível e versionável (JSON), sem segredos derivados
//!   (o env fica no arquivo de projeto/configuração, fora de logs).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use phpide_core::{Component, ErrorKind, IdeError};

/// Versão corrente do formato de Run Configuration.
pub const RUN_CONFIG_FORMAT_VERSION: u32 = 1;

/// Configuração de execução versionada (RUN-001).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunConfig {
    /// Versão do formato (migração incremental, NFR-010).
    pub version: u32,
    /// ID estável para reruns e allow-multiple (não muda ao renomear).
    pub id: String,
    /// Nome exibido na lista de configurações e na aba de sessão.
    pub name: String,
    /// Tipo específico e seus campos (RUN-002).
    #[serde(flatten)]
    pub kind: RunKind,
    /// Working directory; `None` = raiz do projeto.
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    /// Variáveis de ambiente adicionais (nunca logadas em claro).
    #[serde(default)]
    pub env: Vec<(String, String)>,
    /// Interpretador PHP (caminho absoluto); `None` = descoberta padrão.
    #[serde(default)]
    pub interpreter: Option<String>,
    /// RUN-001: várias instâncias simultâneas da mesma configuração.
    #[serde(default)]
    pub allow_multiple_instances: bool,
    /// RUN-001: persistir como arquivo de projeto (compartilhável).
    #[serde(default)]
    pub store_as_project_file: bool,
}

/// Tipo de configuração e campos específicos (RUN-002).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunKind {
    /// Script PHP CLI (T-RUN-002-01).
    PhpScript {
        /// Arquivo a executar (relativo à raiz do projeto ou absoluto).
        file: PathBuf,
        /// Argumentos para o script.
        #[serde(default)]
        args: Vec<String>,
        /// Opções do interpretador antes do arquivo (ex.: `-d`, `-n`).
        #[serde(default)]
        interpreter_options: Vec<String>,
        /// Parar na primeira linha (breakpoint implícito no início).
        #[serde(default)]
        break_at_first_line: bool,
    },
    /// Script do composer.json (depende do executor Composer).
    ComposerScript {
        /// Nome do script em `composer.json` → `scripts`.
        script: String,
    },
    /// PHPUnit por escopo (T-RUN-002-03 / RUN-TST-001).
    PhpUnit {
        /// O que executar: tudo, diretório, classe, método ou data set.
        scope: TestScope,
        /// Arquivo de configuração (phpunit.xml[.dist]); `None` = autodetectar.
        #[serde(default)]
        config: Option<PathBuf>,
        /// Bootstrap (autoload); `None` = autodetectar a partir do config.
        #[serde(default)]
        bootstrap: Option<PathBuf>,
        /// Opções extras do runner (ex.: `--exclude-group`).
        #[serde(default)]
        runner_options: Vec<String>,
        /// Coletar cobertura (Clover) ao final (RUN-TST-003).
        #[serde(default)]
        coverage: bool,
    },
}

/// Escopo de execução de testes (RUN-TST-001-01).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum TestScope {
    /// Toda a suíte do projeto/config.
    All,
    /// Um diretório de testes.
    Directory { path: PathBuf },
    /// Uma classe de teste (opcionalmente restrita a um arquivo).
    Class {
        class: String,
        file: Option<PathBuf>,
    },
    /// Um método de teste.
    Method { class: String, method: String },
    /// Um data set específico de um método com provider.
    DataSet {
        class: String,
        method: String,
        data_set: String,
    },
}

/// Severidade de um problema de validação.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    /// Bloqueia a execução.
    Error,
    /// Não bloqueia; informa ao usuário.
    Warning,
}

/// Problema encontrado na validação de campos (T-RUN-002-01).
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationIssue {
    pub severity: IssueSeverity,
    /// Campo afetado (caminho lógico estável, ex.: `php_script.file`).
    pub field: &'static str,
    pub message: String,
}

impl ValidationIssue {
    fn error(field: &'static str, message: impl Into<String>) -> Self {
        ValidationIssue {
            severity: IssueSeverity::Error,
            field,
            message: message.into(),
        }
    }

    fn warning(field: &'static str, message: impl Into<String>) -> Self {
        ValidationIssue {
            severity: IssueSeverity::Warning,
            field,
            message: message.into(),
        }
    }
}

impl RunConfig {
    /// Constrói uma configuração nova na versão corrente do formato.
    pub fn new(id: impl Into<String>, name: impl Into<String>, kind: RunKind) -> Self {
        RunConfig {
            version: RUN_CONFIG_FORMAT_VERSION,
            id: id.into(),
            name: name.into(),
            kind,
            cwd: None,
            env: Vec::new(),
            interpreter: None,
            allow_multiple_instances: false,
            store_as_project_file: false,
        }
    }

    /// `true` quando há algum erro de validação (bloqueia o start).
    pub fn has_errors(issues: &[ValidationIssue]) -> bool {
        issues.iter().any(|i| i.severity == IssueSeverity::Error)
    }

    /// Valida os campos da configuração contra o projeto em `root`.
    ///
    /// Erros bloqueiam a execução; avisos são exibidos mas não impedem
    /// (RUN-002: "campos específicos devem ser validados antes de iniciar").
    pub fn validate(&self, root: &Path) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        if self.name.trim().is_empty() {
            issues.push(ValidationIssue::error(
                "name",
                "o nome da configuração é obrigatório",
            ));
        }
        if let Some(cwd) = &self.cwd {
            if !cwd.is_dir() {
                issues.push(ValidationIssue::warning(
                    "cwd",
                    format!("working directory não existe: {}", cwd.display()),
                ));
            }
        }
        for (k, _) in &self.env {
            if k.trim().is_empty() {
                issues.push(ValidationIssue::error(
                    "env",
                    "variável de ambiente com nome vazio",
                ));
            }
        }
        match &self.kind {
            RunKind::PhpScript { file, .. } => {
                if file.as_os_str().is_empty() {
                    issues.push(ValidationIssue::error(
                        "php_script.file",
                        "informe o arquivo PHP",
                    ));
                } else {
                    let abs = if file.is_absolute() {
                        file.clone()
                    } else {
                        root.join(file)
                    };
                    if !abs.exists() {
                        issues.push(ValidationIssue::error(
                            "php_script.file",
                            format!("arquivo não encontrado: {}", abs.display()),
                        ));
                    } else if abs.extension().is_some_and(|e| e != "php") {
                        issues.push(ValidationIssue::warning(
                            "php_script.file",
                            "a extensão não é .php",
                        ));
                    }
                }
            }
            RunKind::ComposerScript { script } => {
                if script.trim().is_empty() {
                    issues.push(ValidationIssue::error(
                        "composer_script.script",
                        "informe o nome do script",
                    ));
                } else if !root.join("composer.json").exists() {
                    issues.push(ValidationIssue::warning(
                        "composer_script.script",
                        "projeto sem composer.json — verifique o diretório de trabalho",
                    ));
                }
            }
            RunKind::PhpUnit { scope, .. } => match scope {
                TestScope::Directory { path } => {
                    let abs = if path.is_absolute() {
                        path.clone()
                    } else {
                        root.join(path)
                    };
                    if !abs.is_dir() {
                        issues.push(ValidationIssue::error(
                            "php_unit.scope.path",
                            format!("diretório de testes não encontrado: {}", abs.display()),
                        ));
                    }
                }
                TestScope::Class { class, .. } if class.trim().is_empty() => {
                    issues.push(ValidationIssue::error(
                        "php_unit.scope.class",
                        "informe a classe de teste",
                    ));
                }
                TestScope::Method { class, method } => {
                    let empty = class.trim().is_empty() || method.trim().is_empty();
                    if empty {
                        issues.push(ValidationIssue::error(
                            "php_unit.scope.method",
                            "classe e método são obrigatórios",
                        ));
                    }
                }
                TestScope::DataSet {
                    class,
                    method,
                    data_set,
                } => {
                    let empty = class.trim().is_empty()
                        || method.trim().is_empty()
                        || data_set.trim().is_empty();
                    if empty {
                        issues.push(ValidationIssue::error(
                            "php_unit.scope.data_set",
                            "classe, método e data set são obrigatórios",
                        ));
                    }
                }
                _ => {}
            },
        }
        issues
    }
}

/// Arquivo de configurações de execução (uma lista versionada).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunConfigFile {
    pub version: u32,
    #[serde(default)]
    pub configs: Vec<RunConfig>,
}

impl RunConfigFile {
    pub fn new() -> Self {
        RunConfigFile {
            version: RUN_CONFIG_FORMAT_VERSION,
            configs: Vec::new(),
        }
    }
}

impl Default for RunConfigFile {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse de uma configuração com migração incremental de versão (NFR-010).
///
/// Versão corrente parseia direto; versões futuras seguem pela cadeia de
/// migração; versão desconhecida (maior que a corrente sem migrador, ou
/// ausente/inválida) vira `IdeError` recuperável — nunca panic.
pub fn parse_run_config(json: &str) -> Result<RunConfig, IdeError> {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| {
        IdeError::new(
            Component::Core,
            "parse_run_config",
            ErrorKind::InvalidConfig,
            "arquivo de run configuration inválido",
            "corrija o JSON ou restaure o arquivo de configuração",
        )
        .with_detail(e.to_string())
    })?;
    let version = value.get("version").and_then(serde_json::Value::as_u64);
    match version {
        Some(v) if v == RUN_CONFIG_FORMAT_VERSION as u64 => {
            serde_json::from_value(value).map_err(|e| {
                IdeError::new(
                    Component::Core,
                    "parse_run_config",
                    ErrorKind::InvalidConfig,
                    "campos de run configuration incompatíveis",
                    "verifique o arquivo de configuração",
                )
                .with_detail(e.to_string())
            })
        }
        Some(v) => migrate(value, v),
        None => Err(IdeError::new(
            Component::Core,
            "parse_run_config",
            ErrorKind::InvalidConfig,
            "run configuration sem versão",
            "adicione \"version\": 1 ao arquivo",
        )),
    }
}

/// Cadeia de migração (NFR-010). Hoje só existe a v1; a função existe para
/// que versões futuras registrem aqui seus passos incrementais, testados com
/// arquivos antigos.
fn migrate(_value: serde_json::Value, from: u64) -> Result<RunConfig, IdeError> {
    // Cadeia de migração incremental (NFR-010): quando surgir a v2, o passo
    // v1→v2 é registrado aqui e testado com arquivos antigos. Enquanto não
    // há passos, qualquer versão diferente da corrente é erro recuperável.
    Err(IdeError::new(
        Component::Core,
        "migrate_run_config",
        ErrorKind::InvalidConfig,
        format!("versão de run configuration não suportada: {from}"),
        "atualize a IDE ou recrie a configuração",
    )
    .with_detail(format!(
        "version={from}, suportada<={RUN_CONFIG_FORMAT_VERSION}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_script() -> RunConfig {
        RunConfig::new(
            "cfg-1",
            "rodar index",
            RunKind::PhpScript {
                file: PathBuf::from("src/index.php"),
                args: vec!["--verbose".into()],
                interpreter_options: vec!["-d".into(), "memory_limit=256M".into()],
                break_at_first_line: false,
            },
        )
    }

    #[test]
    fn roundtrip_preserves_all_fields() {
        let mut cfg = sample_script();
        cfg.cwd = Some(PathBuf::from("sub"));
        cfg.env = vec![("APP_ENV".into(), "dev".into())];
        cfg.interpreter = Some("/usr/bin/php8.2".into());
        cfg.allow_multiple_instances = true;
        cfg.store_as_project_file = true;
        let json = serde_json::to_string(&cfg).unwrap();
        let back: RunConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, back);
        assert_eq!(back.version, RUN_CONFIG_FORMAT_VERSION);
    }

    #[test]
    fn parse_rejects_unknown_version_without_panic() {
        let json =
            r#"{"version": 99, "id": "x", "name": "n", "type": "php_script", "file": "a.php"}"#;
        let err = parse_run_config(json).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidConfig);
        assert!(err.message.contains("99"));
        let no_version = r#"{"id": "x"}"#;
        assert_eq!(
            parse_run_config(no_version).unwrap_err().kind,
            ErrorKind::InvalidConfig
        );
        let bad_json = parse_run_config("{não json").unwrap_err();
        assert_eq!(bad_json.kind, ErrorKind::InvalidConfig);
    }

    #[test]
    fn parse_accepts_current_version() {
        let json = serde_json::to_string(&sample_script()).unwrap();
        let cfg = parse_run_config(&json).unwrap();
        assert_eq!(cfg.name, "rodar index");
        assert!(matches!(cfg.kind, RunKind::PhpScript { .. }));
    }

    #[test]
    fn validates_php_script_fields() {
        let root = Path::new("/tmp");
        let cfg = sample_script();
        let issues = cfg.validate(root);
        assert!(
            RunConfig::has_errors(&issues),
            "arquivo inexistente bloqueia: {issues:?}"
        );

        let cfg = RunConfig::new(
            "c",
            "vazio",
            RunKind::PhpScript {
                file: PathBuf::from(""),
                args: vec![],
                interpreter_options: vec![],
                break_at_first_line: false,
            },
        );
        let issues = cfg.validate(root);
        assert!(issues
            .iter()
            .any(|i| i.field == "php_script.file" && i.severity == IssueSeverity::Error));

        let cfg = RunConfig::new(
            "c",
            "",
            RunKind::ComposerScript {
                script: "test".into(),
            },
        );
        let issues = cfg.validate(root);
        assert!(issues.iter().any(|i| i.field == "name"));
    }

    #[test]
    fn validates_test_scopes() {
        let root = Path::new("/tmp");
        let mk = |scope: TestScope| {
            RunConfig::new(
                "c",
                "t",
                RunKind::PhpUnit {
                    scope,
                    config: None,
                    bootstrap: None,
                    runner_options: vec![],
                    coverage: false,
                },
            )
        };
        let issues = mk(TestScope::Directory {
            path: PathBuf::from("/sem/esse/dir"),
        })
        .validate(root);
        assert!(RunConfig::has_errors(&issues));
        let issues = mk(TestScope::Method {
            class: "FooTest".into(),
            method: String::new(),
        })
        .validate(root);
        assert!(issues.iter().any(|i| i.field == "php_unit.scope.method"));
        let issues = mk(TestScope::DataSet {
            class: "F".into(),
            method: "m".into(),
            data_set: String::new(),
        })
        .validate(root);
        assert!(issues.iter().any(|i| i.field == "php_unit.scope.data_set"));
        // Escopo bem formado sem diretório obrigatório não gera erro.
        let issues = mk(TestScope::Class {
            class: "FooTest".into(),
            file: None,
        })
        .validate(root);
        assert!(!RunConfig::has_errors(&issues), "{issues:?}");
    }
}
