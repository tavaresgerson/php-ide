//! Construção de `CommandSpec` a partir da configuração — `T-RUN-002-01`.
//!
//! A validação de campos (`RunConfig::validate`) roda **antes** de qualquer
//! spawn: erro de campo bloqueia com `IdeError` acionável (InvalidConfig),
//! nunca um processo quebrado. Resolução de ferramentas (php/composer/phpunit)
//! fica no chamador (UI aplica NFR-003-01) e chega pronta em [`BuildContext`].

use std::path::{Path, PathBuf};

use phpide_core::{Component, ErrorKind, IdeError};
use phpide_exec::CommandSpec;

use crate::config::{RunConfig, RunKind};

/// Ferramentas já resolvidas pelo chamador (descoberta NFR-003-01).
pub struct BuildContext {
    /// Interpretador PHP (caminho absoluto ou nome no PATH).
    pub php: String,
    /// Composer resolvido (native ou .phar); `None` = não configurado.
    pub composer: Option<String>,
    /// PHPUnit resolvido (vendor/bin/phpunit, phar ou PATH); `None` = autodetectar.
    pub phpunit: Option<String>,
}

impl BuildContext {
    pub fn php_only(php: impl Into<String>) -> Self {
        BuildContext {
            php: php.into(),
            composer: None,
            phpunit: None,
        }
    }
}

/// Resultado da montagem: spec + artefatos do plano (ex.: cobertura Clover).
#[derive(Debug)]
pub struct BuiltCommand {
    pub spec: CommandSpec,
    /// Artefatos a pós-processar quando a sessão terminar.
    pub plan: Option<crate::phpunit::PhpUnitPlan>,
}

/// Absolutiza um caminho de configuração contra a raiz do projeto.
fn absolutize(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

/// Monta o comando da configuração, validando campos antes.
pub fn build_command(
    config: &RunConfig,
    root: &Path,
    ctx: &BuildContext,
) -> Result<BuiltCommand, IdeError> {
    let issues = config.validate(root);
    if RunConfig::has_errors(&issues) {
        let summary = issues
            .iter()
            .filter(|i| i.severity == crate::config::IssueSeverity::Error)
            .map(|i| format!("{}: {}", i.field, i.message))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(IdeError::new(
            Component::Exec,
            "build_command",
            ErrorKind::InvalidConfig,
            format!("run configuration inválida ({})", config.name),
            format!("corrija os campos — {summary}"),
        ));
    }

    let cwd = config
        .cwd
        .as_ref()
        .map(|p| absolutize(root, p))
        .unwrap_or_else(|| root.to_path_buf());
    let mut plan = None;
    let mut spec = match &config.kind {
        RunKind::PhpScript {
            file,
            args,
            interpreter_options,
            ..
        } => {
            let program = config
                .interpreter
                .clone()
                .unwrap_or_else(|| ctx.php.clone());
            let mut full_args = interpreter_options.clone();
            full_args.push(absolutize(root, file).to_string_lossy().into_owned());
            full_args.extend(args.iter().cloned());
            CommandSpec::new(program, full_args).cwd(cwd)
        }
        RunKind::ComposerScript { script } => {
            let composer = ctx.composer.clone().ok_or_else(|| {
                IdeError::new(
                    Component::Exec,
                    "build_command",
                    ErrorKind::NotFound,
                    "Composer não configurado",
                    "instale o Composer ou configure o caminho em Settings",
                )
            })?;
            if composer.ends_with(".phar") {
                CommandSpec::new(
                    config
                        .interpreter
                        .clone()
                        .unwrap_or_else(|| ctx.php.clone()),
                    vec![
                        "-f".into(),
                        composer,
                        "--".into(),
                        "run-script".into(),
                        script.clone(),
                    ],
                )
                .cwd(cwd)
            } else {
                CommandSpec::new(composer, vec!["run-script".into(), script.clone()]).cwd(cwd)
            }
        }
        RunKind::PhpUnit { .. } => {
            let (spec, phpunit_plan) = crate::phpunit::build_command(config, root, ctx, cwd)?;
            plan = Some(phpunit_plan);
            spec
        }
    };
    for (k, v) in &config.env {
        spec = spec.env(k.clone(), v.clone());
    }
    Ok(BuiltCommand { spec, plan })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TestScope;

    fn root_with_script() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("index.php");
        std::fs::write(&script, "<?php echo 'oi';").unwrap();
        (dir, script)
    }

    #[test]
    fn php_script_builds_absolute_file_and_cwd() {
        let (dir, script) = root_with_script();
        let cfg = RunConfig::new(
            "c1",
            "rodar",
            RunKind::PhpScript {
                file: PathBuf::from("index.php"),
                args: vec!["a".into(), "b c".into()],
                interpreter_options: vec!["-n".into()],
                break_at_first_line: false,
            },
        );
        let built =
            build_command(&cfg, dir.path(), &BuildContext::php_only("/usr/bin/php")).unwrap();
        assert_eq!(built.spec.program, "/usr/bin/php");
        assert_eq!(built.spec.args[0], "-n");
        assert_eq!(built.spec.args[1], script.to_string_lossy());
        assert_eq!(built.spec.args[2], "a");
        assert_eq!(built.spec.args[3], "b c");
        assert_eq!(built.spec.cwd.unwrap(), dir.path());
    }

    #[test]
    fn interpreter_override_wins() {
        let (dir, _) = root_with_script();
        let mut cfg = RunConfig::new(
            "c1",
            "rodar",
            RunKind::PhpScript {
                file: PathBuf::from("index.php"),
                args: vec![],
                interpreter_options: vec![],
                break_at_first_line: false,
            },
        );
        cfg.interpreter = Some("/opt/php/bin/php".into());
        let built = build_command(&cfg, dir.path(), &BuildContext::php_only("php")).unwrap();
        assert_eq!(built.spec.program, "/opt/php/bin/php");
    }

    #[test]
    fn invalid_fields_block_with_actionable_error() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = RunConfig::new(
            "c1",
            "rodar",
            RunKind::PhpScript {
                file: PathBuf::from("nao-existe.php"),
                args: vec![],
                interpreter_options: vec![],
                break_at_first_line: false,
            },
        );
        let err = build_command(&cfg, dir.path(), &BuildContext::php_only("php")).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidConfig);
        assert!(
            err.action.contains("php_script.file"),
            "ação nomeia o campo: {}",
            err.action
        );
    }

    #[test]
    fn env_pairs_flow_to_spec_but_not_to_description() {
        let (dir, _) = root_with_script();
        let mut cfg = RunConfig::new(
            "c1",
            "rodar",
            RunKind::PhpScript {
                file: PathBuf::from("index.php"),
                args: vec![],
                interpreter_options: vec![],
                break_at_first_line: false,
            },
        );
        cfg.env = vec![("APP_TOKEN".into(), "segredo-123".into())];
        let built = build_command(&cfg, dir.path(), &BuildContext::php_only("php")).unwrap();
        assert_eq!(
            built.spec.env,
            vec![("APP_TOKEN".to_string(), "segredo-123".to_string())]
        );
    }

    #[test]
    fn composer_script_requires_tool() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("composer.json"), "{}").unwrap();
        let cfg = RunConfig::new(
            "c1",
            "cs",
            RunKind::ComposerScript {
                script: "test".into(),
            },
        );
        let err = build_command(&cfg, dir.path(), &BuildContext::php_only("php")).unwrap_err();
        assert_eq!(err.kind, ErrorKind::NotFound);
        assert!(err.action.contains("Composer"));
        // Com o tool resolvido, o comando sai pronto.
        let built = build_command(
            &cfg,
            dir.path(),
            &BuildContext {
                php: "php".into(),
                composer: Some("/usr/bin/composer".into()),
                phpunit: None,
            },
        )
        .unwrap();
        assert_eq!(built.spec.program, "/usr/bin/composer");
        assert_eq!(built.spec.args, vec!["run-script", "test"]);
    }

    #[test]
    fn composer_phar_runs_via_php() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("composer.json"), "{}").unwrap();
        let cfg = RunConfig::new(
            "c1",
            "cs",
            RunKind::ComposerScript {
                script: "test".into(),
            },
        );
        let built = build_command(
            &cfg,
            dir.path(),
            &BuildContext {
                php: "/usr/bin/php".into(),
                composer: Some("/opt/composer.phar".into()),
                phpunit: None,
            },
        )
        .unwrap();
        assert_eq!(built.spec.program, "/usr/bin/php");
        assert!(built.spec.args.iter().any(|a| a == "/opt/composer.phar"));
    }

    #[test]
    fn phpunit_delegates_to_phpunit_module() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = RunConfig::new(
            "c1",
            "t",
            RunKind::PhpUnit {
                scope: TestScope::All,
                config: None,
                bootstrap: None,
                runner_options: vec![],
                coverage: false,
            },
        );
        // Sem phpunit resolvido no contexto de build: erro acionável.
        let err = build_command(&cfg, dir.path(), &BuildContext::php_only("php")).unwrap_err();
        assert_eq!(err.kind, ErrorKind::NotFound);
    }
}
