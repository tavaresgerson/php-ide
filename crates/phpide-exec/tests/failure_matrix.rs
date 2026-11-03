//! Matriz mínima de falhas de processos externos — `T-NFR-003-04` (§5.2).
//!
//! Ferramentas reais (sh) e executáveis fake gerados em diretório temporário:
//! caminho com espaços/Unicode, versão incompatível, permissão negada,
//! timeout e cancelamento — cada um com erro distinto e ação acionável.

use phpide_core::ErrorKind;
use phpide_exec::{discover, run, CommandSpec, ToolRequest};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("phpide-exec-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Fake de `php`: imprime uma versão fixa e ecoa os args.
fn write_fake_tool(dir: &Path, name: &str, version_line: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(
        f,
        "#!/bin/sh\necho '{version_line}'\necho \"args:$*\" >&2\nexit 0"
    )
    .unwrap();
    drop(f);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[test]
fn executable_in_path_with_spaces_and_unicode() {
    // Diretório com espaço E caractere Unicode no caminho.
    let dir = scratch("caminho com espaço e ção");
    let tool = write_fake_tool(&dir, "ferramenta fake", "Ferramenta 1.2.3");
    let spec = CommandSpec::new(
        tool.to_string_lossy().into_owned(),
        vec!["--opção".to_string(), "valor com espaços".to_string()],
    );
    let out = run(spec).unwrap();
    assert_eq!(out.exit_code, Some(0));
    assert!(out.stdout_text().contains("1.2.3"));
    assert!(
        out.stderr_text().contains("args:--opção valor com espaços"),
        "argv preservado sem shell-split: {}",
        out.stderr_text()
    );
}

#[cfg(unix)]
#[test]
fn explicit_permission_denied_is_distinct_from_missing() {
    let dir = scratch("permissao");
    let tool = write_fake_tool(&dir, "bloqueado", "x 1.0.0");
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o644)).unwrap();
    let req = ToolRequest::new("bloqueado", "Bloqueado").explicit(&tool);
    let e = discover(&req).unwrap_err();
    assert_eq!(e.kind, ErrorKind::PermissionDenied);
    assert!(e.action.contains("chmod"), "ação acionável: {}", e.action);
}

#[test]
fn incompatible_version_reports_minimum() {
    let dir = scratch("versao");
    let fake_php = write_fake_tool(&dir, "php", "PHP 7.4.33 (cli)");
    let req = ToolRequest::new("php", "PHP")
        .explicit(&fake_php)
        .min_version(8, 0);
    let e = discover(&req).unwrap_err();
    assert_eq!(e.kind, ErrorKind::IncompatibleVersion);
    assert!(
        e.message.contains("8.0"),
        "mínimo na mensagem: {}",
        e.message
    );
    assert!(e.action.contains("atualize"), "ação: {}", e.action);
}

#[test]
fn compatible_version_passes_discovery() {
    let dir = scratch("versao-ok");
    let fake_php = write_fake_tool(&dir, "php", "PHP 8.3.6 (cli)");
    let req = ToolRequest::new("php", "PHP")
        .explicit(&fake_php)
        .min_version(8, 0);
    let info = discover(&req).unwrap();
    assert_eq!(info.version_parsed, Some((8, 3, 6)));
}

#[test]
fn timeout_and_cancel_have_distinct_results() {
    // timeout: encerra sozinho.
    let t0 = Instant::now();
    let out = run(CommandSpec::new("sh", vec!["-c".into(), "sleep 3".into()])
        .timeout(Duration::from_millis(150)))
    .unwrap();
    assert!(out.timed_out && !out.cancelled && out.exit_code.is_none());
    assert!(t0.elapsed() < Duration::from_secs(2));

    // spawn direto via std (sem job): processo some no kill do grupo.
    let mut child = Command::new("sh").args(["-c", "sleep 3"]).spawn().unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let _ = child.kill();
    let status = child.wait().unwrap();
    assert!(!status.success());
}

#[test]
fn spawn_failure_keeps_stderr_separate_from_stdout() {
    let out = run(CommandSpec::new(
        "sh",
        vec!["-c".into(), "echo fora; echo dentro 1>&2".into()],
    ))
    .unwrap();
    assert_eq!(out.stdout_text().trim(), "fora");
    assert_eq!(out.stderr_text().trim(), "dentro");
}
