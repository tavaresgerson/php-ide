//! Detecção de repositório — `T-GIT-001-01` (REQUISITOS GIT-001).
//!
//! Detecta worktree (com root resolvido), branch atual, HEAD desanexada e
//! repositório inválido — tudo em chamadas curtas de `git` CLI que não
//! bloqueiam a UI (a chamadora decide quando rodar/polling).

use std::path::{Path, PathBuf};
use std::process::Command;

use phpide_core::{Component, ErrorKind, IdeError};

/// Estado do repositório detectado.
#[derive(Debug, Clone, PartialEq)]
pub enum RepoState {
    /// Worktree normal sobre uma branch.
    Branch { root: PathBuf, branch: String },
    /// HEAD desanexada (checkout de commit/tag): `sha` curto para exibição.
    Detached { root: PathBuf, sha: String },
}

impl RepoState {
    /// Rótulo curto para widgets da UI (window header / status bar).
    pub fn label(&self) -> String {
        match self {
            RepoState::Branch { branch, .. } => branch.clone(),
            RepoState::Detached { sha, .. } => format!("(detached {sha})"),
        }
    }

    /// Worktree root resolvido (pode ser ancestral de `root`).
    pub fn root(&self) -> &Path {
        match self {
            RepoState::Branch { root, .. } => root,
            RepoState::Detached { root, .. } => root,
        }
    }
}

/// Roda `git` com os args, mapeando falhas para `IdeError` recuperável.
pub(crate) fn run_git(root: &Path, args: &[&str]) -> Result<std::process::Output, IdeError> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| {
            IdeError::new(
                Component::Git,
                "git",
                ErrorKind::NotFound,
                "could not run git",
                "install Git (https://git-scm.com) and try again",
            )
            .with_detail(e.to_string())
        })
}

/// stderr do processo como detalhe de erro recuperável.
pub(crate) fn git_error(op: &str, out: &std::process::Output) -> IdeError {
    IdeError::new(
        Component::Git,
        op.to_string(),
        ErrorKind::Io,
        format!("git failed on '{op}' (exit {})", out.status),
        "check the repository state and try again",
    )
    .with_detail(String::from_utf8_lossy(&out.stderr).into_owned())
}

/// True se `root` pertence a um worktree git.
pub fn is_repo(root: &Path) -> bool {
    run_git(root, &["rev-parse", "--is-inside-work-tree"])
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Worktree root (resolvido) de `root`, se for um repositório.
pub fn worktree_root(root: &Path) -> Option<PathBuf> {
    let out = run_git(root, &["rev-parse", "--show-toplevel"]).ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(PathBuf::from(s))
    }
}

/// Detecta o estado do repositório contendo `root`.
///
/// - não é worktree (ou git ausente): `Err` recuperável — a UI exibe como
///   "sem repositório" sem bloquear;
/// - bare: `Err` (não há working tree para diff/stage);
/// - branch atual: `RepoState::Branch`;
/// - HEAD desanexada: `RepoState::Detached` com sha curto.
pub fn detect(root: &Path) -> Result<RepoState, IdeError> {
    let inside = run_git(root, &["rev-parse", "--is-inside-work-tree"])
        .map_err(|e| e.with_detail("git is not available to detect the repository".to_string()))?;
    if !inside.status.success() {
        return Err(IdeError::new(
            Component::Git,
            "detect",
            ErrorKind::InvalidInput,
            "the folder is not inside a Git repository",
            "open the root of a repository or create one with git init",
        )
        .with_detail(String::from_utf8_lossy(&inside.stderr).into_owned()));
    }
    let bare = run_git(root, &["rev-parse", "--is-bare-repository"])?;
    if bare.status.success() && String::from_utf8_lossy(&bare.stdout).trim() == "true" {
        return Err(IdeError::new(
            Component::Git,
            "detect",
            ErrorKind::InvalidInput,
            "a bare repository has no working tree",
            "open a worktree of this repository",
        ));
    }
    let wt = worktree_root(root).ok_or_else(|| {
        IdeError::new(
            Component::Git,
            "detect",
            ErrorKind::InvalidInput,
            "could not resolve the worktree",
            "check that the folder was not moved or damaged",
        )
    })?;
    // Branch: vazia = detached HEAD.
    let branch = run_git(root, &["branch", "--show-current"])?;
    if branch.status.success() {
        let name = String::from_utf8_lossy(&branch.stdout).trim().to_string();
        if !name.is_empty() {
            return Ok(RepoState::Branch {
                root: wt,
                branch: name,
            });
        }
    }
    let sha = run_git(root, &["rev-parse", "--short", "HEAD"])?;
    if sha.status.success() {
        let short = String::from_utf8_lossy(&sha.stdout).trim().to_string();
        if !short.is_empty() {
            return Ok(RepoState::Detached {
                root: wt,
                sha: short,
            });
        }
    }
    // Repo recém-criado sem nenhum commit e sem branch (HEAD aponta para
    // branch inexistente): trata como branch "sem commits" para a UI.
    let symbolic = run_git(root, &["symbolic-ref", "--short", "HEAD"])?;
    if symbolic.status.success() {
        let name = String::from_utf8_lossy(&symbolic.stdout).trim().to_string();
        if !name.is_empty() {
            return Ok(RepoState::Branch {
                root: wt,
                branch: name,
            });
        }
    }
    Err(IdeError::new(
        Component::Git,
        "detect",
        ErrorKind::InvalidInput,
        "HEAD does not point to a branch or commit",
        "make an initial commit (git commit)",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_formats_branch_and_detached() {
        let b = RepoState::Branch {
            root: PathBuf::from("/p"),
            branch: "main".into(),
        };
        assert_eq!(b.label(), "main");
        let d = RepoState::Detached {
            root: PathBuf::from("/p"),
            sha: "abc1234".into(),
        };
        assert_eq!(d.label(), "(detached abc1234)");
    }

    #[test]
    fn invalid_dir_is_recoverable_error() {
        // Diretório quase certamente fora de qualquer repo.
        let dir = std::env::temp_dir().join("phpide-vcs-não-repo-✓");
        let _ = std::fs::create_dir_all(&dir);
        if is_repo(&dir) {
            return; // ambiente do desenvolvedor é um repo; não assumir.
        }
        let err = detect(&dir).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
    }
}
