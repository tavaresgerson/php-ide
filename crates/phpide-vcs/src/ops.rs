//! Stage/unstage por arquivo e commit — `T-GIT-002-02/03` (GIT-002 P0).
//!
//! - stage: `git add -- <path>` (arquivo untracked entra como added);
//! - unstage: `git restore --staged -- <path>` (mantém o arquivo no disco);
//! - commit: valida mensagem (vazia não executa — aceite P0), roda
//!   `git commit` (hooks inclusos) e preserva o índice em falha: o git não
//!   toca no staging area quando um hook recusa, e devolvemos o stderr como
//!   `IdeError` com detalhe (output do hook visível para o usuário).

use std::path::Path;

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
            "mensagem de commit vazia",
            "escreva uma mensagem descrevendo a mudança e tente de novo",
        ));
    }
    let out = run_git(root, &["commit", "-m", message])?;
    if !out.status.success() {
        return Err(IdeError::new(
            Component::Git,
            "commit",
            ErrorKind::Other,
            "git commit recusou o commit",
            "leia o output do hook/erro, corrija e tente de novo — o staging foi preservado",
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
