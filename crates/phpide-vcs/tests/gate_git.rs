//! Gate da Fase 5 — Git como provedor do editor.
//!
//! Narrativa em repositório temporário real:
//! 1. detect: worktree/branch válidos; repositório inválido é erro
//!    recuperável (não bloqueia a UI);
//! 2. diff de working tree e staged por arquivo;
//! 3. stage seletivo: só um dos dois arquivos modificações entra;
//! 4. commit: mensagem vazia não executa; falha de hook preserva output e
//!    índice; commit real leva só o stageado (o outro continua sujo);
//! 5. markers derivados do diff, atualizando após save/stage/commit;
//! 6. blame por linha com navegação ao commit (log a partir do sha).
//!
//! Sem o `git` no ambiente o gate se autopula (NFR-003).

use std::path::Path;
use std::process::Command;

use phpide_core::ErrorKind;
use phpide_vcs::{
    blame, commit, detect, diff, diff_staged, line_blame, log, markers_for_file, stage, status,
    LineMarker, LogFilter, RepoState,
};

fn git_ok() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn git(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("spawn git")
}

#[test]
fn gate_git_diff_stage_commit_markers_blame() {
    if !git_ok() {
        eprintln!("GATE SKIP: git indisponível");
        return;
    }

    // 0) Repositório inválido: erro recuperável antes de tudo.
    let fora = tempfile::tempdir().unwrap();
    let err = detect(fora.path()).unwrap_err();
    assert_eq!(err.kind, ErrorKind::InvalidInput);
    assert!(!err.action.is_empty(), "erro acionável (NFR-005)");

    // 1) Repo temporário com base commitada.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    assert!(git(&root, &["init", "-b", "main"]).status.success());
    git(&root, &["config", "user.name", "Gate Tester"]);
    git(&root, &["config", "user.email", "gate@phpide.dev"]);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.php"), "<?php\n$a = 1;\n").unwrap();
    std::fs::write(root.join("src/b.php"), "<?php\n$b = 1;\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "base"]);
    match detect(&root).unwrap() {
        RepoState::Branch { branch, .. } => assert_eq!(branch, "main"),
        other => panic!("gate: esperava Branch main: {other:?}"),
    }

    // 2) Modifica dois arquivos: diff de working tree nos dois.
    std::fs::write(root.join("src/a.php"), "<?php\n$a = 2;\n").unwrap();
    std::fs::write(root.join("src/b.php"), "<?php\n$b = 2;\n").unwrap();
    let da = diff(&root, Path::new("src/a.php"))
        .unwrap()
        .expect("diff a");
    let db = diff(&root, Path::new("src/b.php"))
        .unwrap()
        .expect("diff b");
    assert!(da
        .hunks
        .iter()
        .flat_map(|h| &h.lines)
        .any(|l| l.content.contains("$a = 2")));
    assert!(db
        .hunks
        .iter()
        .flat_map(|h| &h.lines)
        .any(|l| l.content.contains("$b = 2")));
    // Staged ainda vazio.
    assert!(diff_staged(&root, Path::new("src/a.php"))
        .unwrap()
        .is_none());

    // 3) Stage seletivo: só a.php.
    stage(&root, Path::new("src/a.php")).unwrap();
    let st = status(&root).unwrap();
    let sa = st
        .iter()
        .find(|s| s.path == Path::new("src/a.php"))
        .unwrap();
    assert!(sa.staged.is_some() && sa.unstaged.is_none(), "{sa:?}");
    let sb = st
        .iter()
        .find(|s| s.path == Path::new("src/b.php"))
        .unwrap();
    assert!(sb.unstaged.is_some() && sb.staged.is_none(), "{sb:?}");
    assert!(diff_staged(&root, Path::new("src/a.php"))
        .unwrap()
        .is_some());
    assert!(
        diff(&root, Path::new("src/b.php")).unwrap().is_some(),
        "b continua com diff"
    );

    // 4a) Mensagem vazia não executa commit.
    let err = commit(&root, "  \n ").unwrap_err();
    assert_eq!(err.kind, ErrorKind::InvalidInput);

    // 4b) Falha de hook preserva output E índice.
    let hooks = root.join(".git/hooks");
    let hook = hooks.join("pre-commit");
    std::fs::write(
        &hook,
        "#!/bin/sh\necho 'gate-hook: commit bloqueado' >&2\nexit 1\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&hook).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&hook, perms).unwrap();
    }
    let err = commit(&root, "feat: a").unwrap_err();
    assert!(
        err.detail
            .as_deref()
            .unwrap_or("")
            .contains("gate-hook: commit bloqueado"),
        "output do hook preservado: {err:?}"
    );
    let st = status(&root).unwrap();
    assert!(
        st.iter()
            .find(|s| s.path == Path::new("src/a.php"))
            .unwrap()
            .staged
            .is_some(),
        "índice preservado após falha do hook"
    );
    std::fs::remove_file(&hook).unwrap();

    // 4c) Commit de verdade: só a.php entra; b.php continua sujo.
    let res = commit(&root, "feat: a").unwrap();
    assert!(!res.sha.is_empty());
    let st = status(&root).unwrap();
    assert!(st
        .iter()
        .any(|s| s.path == Path::new("src/b.php") && s.unstaged.is_some()));
    assert!(!st.iter().any(|s| s.path == Path::new("src/a.php")));

    // 5) Markers: a.php commitado limpa; b.php modificado marca a linha 2.
    assert!(markers_for_file(&root, Path::new("src/a.php"))
        .unwrap()
        .is_empty());
    let m = markers_for_file(&root, Path::new("src/b.php")).unwrap();
    assert_eq!(m.get(&2), Some(&LineMarker::Modified), "{m:?}");
    // Novo arquivo (save): todas as linhas Added; stage não limpa; commit limpa.
    std::fs::write(root.join("src/c.php"), "<?php\n$c = 1;\n$c2 = 2;\n").unwrap();
    let m = markers_for_file(&root, Path::new("src/c.php")).unwrap();
    assert_eq!(m.len(), 3);
    assert!(m.values().all(|k| *k == LineMarker::Added));
    stage(&root, Path::new("src/c.php")).unwrap();
    assert_eq!(
        markers_for_file(&root, Path::new("src/c.php"))
            .unwrap()
            .len(),
        3
    );
    commit(&root, "feat: c").unwrap();
    assert!(markers_for_file(&root, Path::new("src/c.php"))
        .unwrap()
        .is_empty());

    // 6) Blame disponível para a UI com navegação ao commit.
    let entries = blame(&root, Path::new("src/a.php")).unwrap();
    let e = line_blame(&entries, 2).expect("blame linha 2 de a.php");
    assert_eq!(e.author, "Gate Tester");
    assert_eq!(e.summary, "feat: a");
    let nav = log(
        &root,
        &LogFilter {
            rev: Some(e.sha.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        nav.iter().any(|c| c.sha == e.sha),
        "navegação ao commit do blame"
    );
}
