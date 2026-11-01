//! VCS mínimo (parcial REQUISITOS 1.3/7): branch atual do Git para os widgets
//! da window header e da status bar. O Git tool window completo (Alt+9) fica
//! para o próximo estágio; aqui só leitura via `git` CLI.

use std::path::Path;
use std::process::Command;

/// Branch atual se `root` for um repositório git; None caso contrário.
pub fn current_branch(root: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["-C", &root.display().to_string(), "branch", "--show-current"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if branch.is_empty() {
        None
    } else {
        Some(branch)
    }
}

/// True se o diretório pertence a um repositório git.
pub fn is_repo(root: &Path) -> bool {
    Command::new("git")
        .args([
            "-C",
            &root.display().to_string(),
            "rev-parse",
            "--is-inside-work-tree",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_repo_has_no_branch() {
        let dir = std::env::temp_dir();
        if is_repo(&dir) {
            // Ambiente do desenvolvedor pode ser um repo; não assumir.
            return;
        }
        assert!(current_branch(&dir).is_none());
    }
}
