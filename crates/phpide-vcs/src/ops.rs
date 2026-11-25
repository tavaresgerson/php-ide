//! Stage/unstage por arquivo e commit — `T-GIT-002-02/03` (GIT-002 P0).
//!
//! - stage: `git add -- <path>` (arquivo untracked entra como added);
//! - unstage: `git restore --staged -- <path>` (mantém o arquivo no disco);
//! - commit: valida mensagem (vazia não executa — aceite P0), roda
//!   `git commit` (hooks inclusos) e preserva o índice em falha: o git não
//!   toca no staging area quando um hook recusa, e devolvemos o stderr como
//!   `IdeError` com detalhe (output do hook visível para o usuário).

use std::path::{Path, PathBuf};

use phpide_core::{Component, ErrorKind, IdeError};

use crate::repo::{git_error, run_git};
use crate::status::relpath;

/// Stage (adiciona ao índice) o arquivo `path` relativo a `root`.
pub fn stage(root: &Path, path: &Path) -> Result<(), IdeError> {
    let rel = relpath(root, path)?;
    let rel_s = rel.to_string_lossy().into_owned();
    let out = run_git(root, &["add", "--", &rel_s])?;
    if !out.status.success() {
        return Err(git_error("stage", &out));
    }
    Ok(())
}

/// Tira o arquivo do índice, mantendo o conteúdo no working tree.
pub fn unstage(root: &Path, path: &Path) -> Result<(), IdeError> {
    let rel = relpath(root, path)?;
    let rel_s = rel.to_string_lossy().into_owned();
    let out = run_git(root, &["restore", "--staged", "--", &rel_s])?;
    if !out.status.success() {
        return Err(git_error("unstage", &out));
    }
    Ok(())
}

/// `git add -A` for several paths: new, modified and deleted files alike.
pub fn stage_paths(root: &Path, paths: &[PathBuf]) -> Result<(), IdeError> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec!["add".to_string(), "-A".to_string(), "--".to_string()];
    args.extend(paths.iter().map(|p| p.to_string_lossy().into_owned()));
    run_checked(root, &args, "stage")
}

/// Takes several paths out of the index, keeping the files on disk.
pub fn unstage_paths(root: &Path, paths: &[PathBuf]) -> Result<(), IdeError> {
    if paths.is_empty() {
        return Ok(());
    }
    let unborn = !run_git(root, &["rev-parse", "--verify", "--quiet", "HEAD"])?
        .status
        .success();
    let mut args: Vec<String> = if unborn {
        // No HEAD to restore from: drop the entries from the index.
        vec!["rm".into(), "--cached".into(), "-r".into(), "-q".into()]
    } else {
        vec!["restore".into(), "--staged".into()]
    };
    args.push("--".into());
    args.extend(paths.iter().map(|p| p.to_string_lossy().into_owned()));
    run_checked(root, &args, "unstage")
}

/// Rollback: tracked files go back to HEAD on disk and in the index; files
/// added to the index (not in HEAD) leave the index and stay on disk as
/// unversioned. Untracked files are the caller's to delete.
pub fn rollback(root: &Path, paths: &[PathBuf]) -> Result<(), IdeError> {
    let mut in_head = Vec::new();
    let mut added = Vec::new();
    for path in paths {
        let spec = format!("HEAD:{}", path.to_string_lossy().replace('\\', "/"));
        let known = run_git(root, &["cat-file", "-e", &spec])?.status.success();
        if known {
            in_head.push(path.to_string_lossy().into_owned());
        } else {
            added.push(path.to_string_lossy().into_owned());
        }
    }
    if !in_head.is_empty() {
        let mut args: Vec<String> = ["restore", "--source=HEAD", "--staged", "--worktree", "--"]
            .map(String::from)
            .to_vec();
        args.extend(in_head);
        run_checked(root, &args, "rollback")?;
    }
    if !added.is_empty() {
        let mut args: Vec<String> = ["rm", "--cached", "-q", "--"].map(String::from).to_vec();
        args.extend(added);
        run_checked(root, &args, "rollback")?;
    }
    Ok(())
}

/// Full message of the last commit (to amend it).
pub fn last_commit_message(root: &Path) -> Result<String, IdeError> {
    let out = run_git(root, &["log", "-1", "--format=%B"])?;
    if !out.status.success() {
        return Err(git_error("last_commit_message", &out));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

/// Which side of a merge conflict to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictSide {
    /// The current branch.
    Ours,
    /// The branch being merged in.
    Theirs,
}

/// Resolves a conflict by taking one side whole, then marks it resolved.
pub fn accept_side(root: &Path, path: &Path, side: ConflictSide) -> Result<(), IdeError> {
    let rel = path.to_string_lossy().into_owned();
    let flag = match side {
        ConflictSide::Ours => "--ours",
        ConflictSide::Theirs => "--theirs",
    };
    let out = run_git(root, &["checkout", flag, "--", &rel])?;
    if !out.status.success() {
        // A side that deleted the file: resolve by removing it.
        let rm = run_git(root, &["rm", "-q", "--", &rel])?;
        if !rm.status.success() {
            return Err(git_error("accept", &out));
        }
        return Ok(());
    }
    stage_paths(root, &[path.to_path_buf()])
}

fn run_checked(root: &Path, args: &[String], op: &str) -> Result<(), IdeError> {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = run_git(root, &refs)?;
    if !out.status.success() {
        return Err(git_error(op, &out));
    }
    Ok(())
}

/// Resultado de um commit bem-sucedido.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitResult {
    /// SHA curto do commit criado.
    pub sha: String,
    /// stdout+stderr do git (inclui resumo do commit; vazio de hooks silenciosos).
    pub output: String,
}

/// Commita o índice com `message`.
///
/// - mensagem vazia (só espaços) → `InvalidInput` sem executar nada
///   (aceite GIT-002: "mensagem vazia não executa commit");
/// - falha de hook (exit ≠ 0) → erro recuperável com o output do hook no
///   detalhe; o índice fica exatamente como estava (o git não o reescreve);
/// - sucesso → sha + output (stdout e stderr concatenados, stderr primeiro
///   quando houver — hooks ecoam para stderr).
pub fn commit(root: &Path, message: &str) -> Result<CommitResult, IdeError> {
    if message.trim().is_empty() {
        return Err(IdeError::new(
            Component::Git,
            "commit",
            ErrorKind::InvalidInput,
            "empty commit message",
            "write a message describing the change and try again",
        ));
    }
    let out = run_git(root, &["commit", "-m", message])?;
    if !out.status.success() {
        return Err(IdeError::new(
            Component::Git,
            "commit",
            ErrorKind::Other,
            "git commit recusou o commit",
            "read the hook/error output, fix it and try again — your staged changes were kept",
        )
        .with_detail(
            [
                String::from_utf8_lossy(&out.stderr).into_owned(),
                String::from_utf8_lossy(&out.stdout).into_owned(),
            ]
            .join("\n"),
        ));
    }
    // SHA do commit recém-criado (HEAD).
    let head = run_git(root, &["rev-parse", "--short", "HEAD"])?;
    let sha = if head.status.success() {
        String::from_utf8_lossy(&head.stdout).trim().to_string()
    } else {
        String::new()
    };
    Ok(CommitResult {
        sha,
        output: [
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        ]
        .join("\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_message_is_rejected_before_running() {
        let err = commit(Path::new("/tmp"), "   \n  ").unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        assert_eq!(err.component, Component::Git);
    }
}
