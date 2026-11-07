//! Integração real de Run Configurations — PHP CLI real (`T-RUN-001-02`,
//! `T-RUN-001-03`, `T-RUN-001-05`, `T-RUN-002-01`).
//!
//! Executa um script PHP real em diretório temporário: sessão com cwd, env,
//! output incremental, Stop confiável, Rerun e redaction de segredos no log.
//! Sem PHP no ambiente o teste imprime o motivo e retorna (NFR-003).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use phpide_core::log::LogSink;
use phpide_core::ErrorKind;
use phpide_run::config::{RunConfig, RunKind};
use phpide_run::session::SessionState;
use phpide_run::{build_command, BuildContext, RunManager};

fn php() -> Option<String> {
    let php = std::env::var("PHPIDE_PHP").unwrap_or_else(|_| "php".into());
    let ok = Command::new(&php)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    ok.then_some(php)
}

const SCRIPT: &str = r#"<?php
echo "argv=" . ($argv[1] ?? '?') . "\n";
echo "marcador=" . getenv('MARCADOR') . "\n";
fwrite(STDERR, "linha-erro\n");
echo "cwd=" . getcwd() . "\n";
"#;

fn wait_state(m: &mut RunManager, id: u64, want: SessionState) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        m.poll();
        if m.session(id).is_some_and(|s| s.state == want) {
            return;
        }
        assert!(Instant::now() < deadline, "timeout esperando {want:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn wait_finished(m: &mut RunManager, id: u64) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        m.poll();
        let s = m.session(id).unwrap();
        if !s.state.is_live() {
            return;
        }
        assert!(Instant::now() < deadline, "timeout aguardando término");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn php_script_runs_with_cwd_env_args_and_stderr() {
    let Some(php) = php() else {
        eprintln!("SKIP: php não disponível");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("rodar.php"), SCRIPT).unwrap();

    let mut config = RunConfig::new(
        "cfg-php",
        "rodar.php",
        RunKind::PhpScript {
            file: PathBuf::from("rodar.php"),
            args: vec!["alpha".into()],
            interpreter_options: vec![],
            break_at_first_line: false,
        },
    );
    config.env = vec![("MARCADOR".into(), "presente-123".into())];

    let built = build_command(&config, dir.path(), &BuildContext::php_only(&php)).unwrap();
    let mut manager = RunManager::new();
    let id = manager.start(config, built.spec, None);
    wait_finished(&mut manager, id);
    manager.poll();

    let s = manager.session(id).unwrap();
    assert_eq!(s.state, SessionState::Finished);
    assert_eq!(s.exit_code, Some(0));
    let out = s.output();
    assert!(out.contains("argv=alpha"), "{out}");
    assert!(out.contains("marcador=presente-123"), "{out}");
    assert!(out.contains("[stderr] linha-erro"), "{out}");
    assert!(
        out.contains(&format!("cwd={}", dir.path().display())),
        "{out}"
    );
    // Linha de comando pública não vaza env.
    assert!(!s.command_line().contains("presente-123"));
}

#[test]
fn stop_kills_long_running_php() {
    let Some(php) = php() else {
        eprintln!("SKIP: php não disponível");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("longo.php"), "<?php sleep(60); echo 'não';").unwrap();

    let config = RunConfig::new(
        "cfg-stop",
        "longo.php",
        RunKind::PhpScript {
            file: PathBuf::from("longo.php"),
            args: vec![],
            interpreter_options: vec![],
            break_at_first_line: false,
        },
    );
    let built = build_command(&config, dir.path(), &BuildContext::php_only(&php)).unwrap();
    let mut manager = RunManager::new();
    let id = manager.start(config, built.spec, None);
    wait_state(&mut manager, id, SessionState::Running);
    manager.stop(id);
    wait_state(&mut manager, id, SessionState::Stopped);
    assert_eq!(manager.session(id).unwrap().exit_code, None);
    assert!(!manager.session(id).unwrap().is_running());
}

#[test]
fn rerun_reexecutes_with_same_spec() {
    let Some(php) = php() else {
        eprintln!("SKIP: php não disponível");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("x.php"), "<?php echo 'rodada';").unwrap();

    let config = RunConfig::new(
        "cfg-rerun",
        "x.php",
        RunKind::PhpScript {
            file: PathBuf::from("x.php"),
            args: vec![],
            interpreter_options: vec![],
            break_at_first_line: false,
        },
    );
    let built = build_command(&config, dir.path(), &BuildContext::php_only(&php)).unwrap();
    let mut manager = RunManager::new();
    let first = manager.start(config, built.spec, None);
    wait_finished(&mut manager, first);
    let second = manager.rerun(first).unwrap();
    assert_ne!(first, second);
    wait_finished(&mut manager, second);
    assert!(manager.session(second).unwrap().output().contains("rodada"));
}

#[test]
fn invalid_config_is_actionable_error_not_spawn() {
    let Some(php) = php() else {
        eprintln!("SKIP: php não disponível");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let config = RunConfig::new(
        "cfg-invalid",
        "fantasma.php",
        RunKind::PhpScript {
            file: PathBuf::from("fantasma.php"),
            args: vec![],
            interpreter_options: vec![],
            break_at_first_line: false,
        },
    );
    let err = build_command(&config, dir.path(), &BuildContext::php_only(&php)).unwrap_err();
    assert_eq!(err.kind, ErrorKind::InvalidConfig);
    assert!(
        err.action.contains("php_script.file"),
        "ação: {}",
        err.action
    );
}

#[derive(Clone, Default)]
struct SharedLog(std::sync::Arc<std::sync::Mutex<Vec<String>>>);
impl LogSink for SharedLog {
    fn write_line(&self, line: &str) {
        self.0.lock().unwrap().push(line.to_string());
    }
}

#[test]
fn env_secret_never_appears_in_session_logs() {
    let Some(php) = php() else {
        eprintln!("SKIP: php não disponível");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("s.php"), "<?php echo 'ok';").unwrap();
    let mut config = RunConfig::new(
        "cfg-secret",
        "s.php",
        RunKind::PhpScript {
            file: PathBuf::from("s.php"),
            args: vec![],
            interpreter_options: vec![],
            break_at_first_line: false,
        },
    );
    config.env = vec![
        ("API_KEY".into(), "chave-super-secreta-999".into()),
        ("DB_PASSWORD".into(), "senha-do-banco".into()),
    ];
    let built = build_command(&config, dir.path(), &BuildContext::php_only(&php)).unwrap();
    let log = SharedLog::default();
    let reader = log.clone();
    let mut manager = RunManager::with_sink(Box::new(log));
    let id = manager.start(config, built.spec, None);
    wait_finished(&mut manager, id);
    let lines = reader.0.lock().unwrap().clone();
    let joined = lines.join("\n");
    assert!(
        !joined.contains("chave-super-secreta-999"),
        "log vaza API_KEY:\n{joined}"
    );
    assert!(
        !joined.contains("senha-do-banco"),
        "log vaza DB_PASSWORD:\n{joined}"
    );
}

#[test]
fn allow_multiple_instances_runs_concurrent_sessions() {
    let Some(php) = php() else {
        eprintln!("SKIP: php não disponível");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("m.php"),
        "<?php usleep(200000); echo getmypid();",
    )
    .unwrap();
    let mut config = RunConfig::new(
        "cfg-multi",
        "m.php",
        RunKind::PhpScript {
            file: PathBuf::from("m.php"),
            args: vec![],
            interpreter_options: vec![],
            break_at_first_line: false,
        },
    );
    config.allow_multiple_instances = true;
    let built = build_command(&config, dir.path(), &BuildContext::php_only(&php)).unwrap();
    let mut manager = RunManager::new();
    let a = manager.start(config.clone(), built.spec.clone(), None);
    let b = manager.start(config.clone(), built.spec.clone(), None);
    assert_ne!(a, b, "allow multiple: sessões distintas");
    wait_finished(&mut manager, a);
    wait_finished(&mut manager, b);
    let out_a = manager.session(a).unwrap().output().to_string();
    let out_b = manager.session(b).unwrap().output().to_string();
    assert_ne!(
        out_a.trim(),
        out_b.trim(),
        "pids diferentes = processos reais"
    );
    let _ = Path::new(""); // uso simbólico para manter import
}
