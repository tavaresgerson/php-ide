//! Integração real do provedor Git — `git` CLI sobre repositórios
//! temporários (`T-GIT-001-01/002-01/002-02/002-03/003-01/004-01/004-02`).
//!
//! Ferramenta real: o `git` do sistema. Sem o binário o arquivo inteiro se
//! autopula (ausência de ferramenta ≠ falha — NFR-003).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use phpide_core::ErrorKind;
use phpide_vcs::{
    blame, blame_rev, commit, detect, diff, diff_head, diff_staged, is_repo, line_blame, log,
    markers_for_file, stage, status, unstage, ChangeKind, LineMarker, LogFilter, RepoState,
};

fn git_ok() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn git(root: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("spawn git")
}

fn git_stdout(root: &Path, args: &[&str]) -> String {
    let out = git(root, args);
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Repo temporário com user configurado e commit inicial em `main`.
fn temp_repo() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let init = git(&root, &["init", "-b", "main"]);
    assert!(
        init.status.success(),
        "git init: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    git(&root, &["config", "user.name", "Ana Teste"]);
    git(&root, &["config", "user.email", "ana@exemplo.com"]);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/base.php"), "<?php // base\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "base"]);
    (dir, root)
}

fn write(root: &Path, rel: &str, content: &str) {
    std::fs::write(root.join(rel), content).unwrap();
}

#[test]
fn detect_branch_detached_and_invalid() {
    if !git_ok() {
        eprintln!("SKIP: git indisponível");
        return;
    }
    let (_d, root) = temp_repo();
    match detect(&root).unwrap() {
        RepoState::Branch { branch, .. } => assert_eq!(branch, "main"),
        other => panic!("esperava Branch: {other:?}"),
    }
    // Subdiretório também resolve para o mesmo worktree.
    let sub = root.join("src");
    match detect(&sub).unwrap() {
        RepoState::Branch { branch, .. } => assert_eq!(branch, "main"),
        other => panic!("esperava Branch a partir de subdir: {other:?}"),
    }
    // Detached HEAD.
    let sha = git_stdout(&root, &["rev-parse", "--short", "HEAD"]);
    git(&root, &["checkout", "--detach", "HEAD"]);
    match detect(&root).unwrap() {
        RepoState::Detached { sha: s, .. } => assert_eq!(s, sha.trim()),
        other => panic!("esperava Detached: {other:?}"),
    }
    assert_eq!(
        detect(&root).unwrap().label(),
        format!("(detached {})", sha.trim())
    );
    // Inválido: diretório sem repo.
    let out = tempfile::tempdir().unwrap();
    let err = detect(out.path()).unwrap_err();
    assert_eq!(err.kind, ErrorKind::InvalidInput);
    assert!(!is_repo(out.path()));
}

#[test]
fn status_and_selective_stage_unstage() {
    if !git_ok() {
        eprintln!("SKIP: git indisponível");
        return;
    }
    let (_d, root) = temp_repo();
    write(&root, "src/base.php", "<?php // base modificado\n");
    write(&root, "src/novo.php", "<?php // novo\n");
    let st = status(&root).unwrap();
    let a = st
        .iter()
        .find(|s| s.path == Path::new("src/base.php"))
        .unwrap();
    assert_eq!(a.unstaged, Some(ChangeKind::Modified));
    let novo = st
        .iter()
        .find(|s| s.path == Path::new("src/novo.php"))
        .unwrap();
    assert!(novo.untracked);
    // Stage seletivo: só base.php.
    stage(&root, Path::new("src/base.php")).unwrap();
    let st = status(&root).unwrap();
    let a = st
        .iter()
        .find(|s| s.path == Path::new("src/base.php"))
        .unwrap();
    assert_eq!(a.staged, Some(ChangeKind::Modified));
    assert_eq!(a.unstaged, None, "conteúdo igual ao índice após stage");
    let novo = st
        .iter()
        .find(|s| s.path == Path::new("src/novo.php"))
        .unwrap();
    assert!(novo.untracked, "não stageado permanece untracked");
    // Unstage devolve ao estado anterior.
    unstage(&root, Path::new("src/base.php")).unwrap();
    let st = status(&root).unwrap();
    let a = st
        .iter()
        .find(|s| s.path == Path::new("src/base.php"))
        .unwrap();
    assert_eq!(a.staged, None);
    assert_eq!(a.unstaged, Some(ChangeKind::Modified));
}

#[test]
fn diff_worktree_staged_and_head() {
    if !git_ok() {
        eprintln!("SKIP: git indisponível");
        return;
    }
    let (_d, root) = temp_repo();
    write(&root, "src/base.php", "<?php // base\nlinha dois\n");
    // Working tree: diff unstaged com a modificação.
    let d = diff(&root, Path::new("src/base.php"))
        .unwrap()
        .expect("diff unstaged");
    assert!(!d.is_empty());
    assert!(d
        .hunks
        .iter()
        .flat_map(|h| &h.lines)
        .any(|l| l.content.contains("linha dois")));
    // Stage: unstaged zera, staged aparece, vs HEAD continua (base markers).
    stage(&root, Path::new("src/base.php")).unwrap();
    assert!(diff(&root, Path::new("src/base.php")).unwrap().is_none());
    assert!(diff_staged(&root, Path::new("src/base.php"))
        .unwrap()
        .is_some());
    assert!(diff_head(&root, Path::new("src/base.php"))
        .unwrap()
        .is_some());
    // Arquivo untracked: diff sintético 100% adicionado.
    write(&root, "src/novo.php", "<?php\num\ndois\n");
    let d = diff(&root, Path::new("src/novo.php"))
        .unwrap()
        .expect("diff untracked");
    assert_eq!(d.old_path, PathBuf::from("/dev/null"));
    assert_eq!(
        d.hunks[0]
            .lines
            .iter()
            .filter(|l| l.new_lineno == Some(2))
            .count(),
        1
    );
}

#[test]
fn commit_empty_message_hook_failure_and_success() {
    if !git_ok() {
        eprintln!("SKIP: git indisponível");
        return;
    }
    let (_d, root) = temp_repo();
    write(&root, "src/a.php", "<?php // a\n");
    write(&root, "src/b.php", "<?php // b\n");
    stage(&root, Path::new("src/a.php")).unwrap();

    // Mensagem vazia: não executa (aceite GIT-002).
    let err = commit(&root, "   \n").unwrap_err();
    assert_eq!(err.kind, ErrorKind::InvalidInput);
    let n = git_stdout(&root, &["rev-list", "--count", "HEAD"]);
    assert_eq!(n.trim(), "1", "commit vazio não criou commit");

    // Hook que recusa: falha preserva output E índice.
    let hooks = root.join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let hook = hooks.join("pre-commit");
    std::fs::write(
        &hook,
        "#!/bin/sh\necho 'pre-commit: nada de commit agora' >&2\nexit 1\n",
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
    let detail = err.detail.unwrap_or_default();
    assert!(
        detail.contains("pre-commit: nada de commit agora"),
        "output do hook preservado: {detail}"
    );
    // Índice preservado: a.php continua stageado, b.php fora.
    let st = status(&root).unwrap();
    let a = st
        .iter()
        .find(|s| s.path == Path::new("src/a.php"))
        .unwrap();
    assert_eq!(
        a.staged,
        Some(ChangeKind::Added),
        "índice preservado após falha do hook: {a:?}"
    );
    assert!(st
        .iter()
        .any(|s| s.path == Path::new("src/b.php") && s.untracked));

    // Sem o hook: commit só do stageado.
    std::fs::remove_file(&hook).unwrap();
    let res = commit(&root, "feat: a").unwrap();
    assert!(!res.sha.is_empty());
    let st = status(&root).unwrap();
    assert!(
        st.iter()
            .any(|s| s.path == Path::new("src/b.php") && s.untracked),
        "b.php fora do commit"
    );
    assert!(
        !st.iter().any(|s| s.path == Path::new("src/a.php")),
        "a.php commitado"
    );
}

#[test]
fn log_with_files_filters_and_rev_navigation() {
    if !git_ok() {
        eprintln!("SKIP: git indisponível");
        return;
    }
    let (_d, root) = temp_repo();
    write(&root, "src/um.php", "<?php // um\n");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "adiciona um"]);
    let first_sha = git_stdout(&root, &["rev-parse", "HEAD"]).trim().to_string();
    write(&root, "src/dois.php", "<?php // dois\n");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "adiciona dois"]);

    let all = log(
        &root,
        &LogFilter {
            max_count: 10,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(all.len(), 3, "base + dois commits");
    assert_eq!(all[0].subject, "adiciona dois");
    assert!(all[0]
        .files
        .iter()
        .any(|f| f.path == Path::new("src/dois.php")));
    assert_eq!(all[0].files[0].kind, ChangeKind::Added);
    assert_eq!(all[2].subject, "base");

    // Filtro por path: só os commits que tocaram um.php.
    let por_path = log(
        &root,
        &LogFilter {
            max_count: 10,
            path: Some(PathBuf::from("src/um.php")),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(por_path.len(), 1);
    assert_eq!(por_path[0].subject, "adiciona um");

    // Navegação por revisão (usada a partir do blame): a história a partir
    // do commit — ele mesmo + ancestrais.
    let rev = log(
        &root,
        &LogFilter {
            rev: Some(first_sha.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(rev.len(), 2);
    assert_eq!(rev[0].sha, first_sha);
    assert_eq!(rev[1].subject, "base");
}

#[test]
fn markers_follow_save_stage_commit() {
    if !git_ok() {
        eprintln!("SKIP: git indisponível");
        return;
    }
    let (_d, root) = temp_repo();
    // Untracked: todas as linhas Added.
    write(&root, "src/novo.php", "<?php\n$um = 1;\n$dois = 2;\n");
    let m = markers_for_file(&root, Path::new("src/novo.php")).unwrap();
    assert_eq!(m.len(), 3);
    assert!(m.values().all(|k| *k == LineMarker::Added));
    // Stage: markers persistem (vs HEAD).
    stage(&root, Path::new("src/novo.php")).unwrap();
    let m = markers_for_file(&root, Path::new("src/novo.php")).unwrap();
    assert_eq!(m.len(), 3, "stage não limpa markers vs HEAD");
    // Commit: limpa.
    commit(&root, "adiciona novo").unwrap();
    assert!(markers_for_file(&root, Path::new("src/novo.php"))
        .unwrap()
        .is_empty());
    // Modificação: Modified na linha trocada.
    write(&root, "src/novo.php", "<?php\n$um = 10;\n$dois = 2;\n");
    let m = markers_for_file(&root, Path::new("src/novo.php")).unwrap();
    assert_eq!(m.get(&2), Some(&LineMarker::Modified));
    // Remoção: marcador ancorado na linha seguinte.
    write(&root, "src/novo.php", "<?php\n$dois = 2;\n");
    let m = markers_for_file(&root, Path::new("src/novo.php")).unwrap();
    assert_eq!(m.get(&2), Some(&LineMarker::Removed));
}

#[test]
fn blame_attributes_and_rev_navigation() {
    if !git_ok() {
        eprintln!("SKIP: git indisponível");
        return;
    }
    let (dir, root) = temp_repo();
    write(&root, "src/base.php", "<?php\nlinha ana\n");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "por ana"]);
    let ana_sha = git_stdout(&root, &["rev-parse", "HEAD"]).trim().to_string();
    // Segundo autor.
    git(&root, &["config", "user.name", "Beto Teste"]);
    git(&root, &["config", "user.email", "beto@exemplo.com"]);
    write(&root, "src/base.php", "<?php\nlinha ana\nlinha beto\n");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "por beto"]);

    let entries = blame(&root, Path::new("src/base.php")).unwrap();
    assert_eq!(entries.len(), 2, "duas origens: {entries:?}");
    assert_eq!(line_blame(&entries, 2).unwrap().author, "Ana Teste");
    let beto = line_blame(&entries, 3).unwrap();
    assert_eq!(beto.author, "Beto Teste");
    assert_eq!(beto.summary, "por beto");
    assert!(!beto.is_uncommitted());
    // Navegação ao commit: log a partir do sha do blame.
    let nav = log(
        &root,
        &LogFilter {
            rev: Some(ana_sha.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(nav[0].subject, "por ana");
    // Blame por revisão anterior: só a Ana.
    let old = blame_rev(&root, &ana_sha, Path::new("src/base.php")).unwrap();
    assert!(old.iter().all(|e| e.author == "Ana Teste"));
    // Linha não commitada: blame ainda não cobre (arquivo limpo agora) — edição
    // não commitada aparece como não commitada.
    write(
        &root,
        "src/base.php",
        "<?php\nlinha ana\nlinha beto\nlinha local\n",
    );
    let entries = blame(&root, Path::new("src/base.php")).unwrap();
    let local = line_blame(&entries, 4).expect("linha 4 blameada");
    assert!(
        local.is_uncommitted(),
        "linha local não commitada: {local:?}"
    );
    drop(dir);
}
