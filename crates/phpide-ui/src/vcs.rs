//! VCS (Git) — widgets da window header e da status bar, agora via
//! `phpide-vcs` (Fase 5, GIT-001): branch atual, HEAD desanexada (mostra o
//! sha curto) e "sem repositório" como três estados de um mesmo detector.
//! O Git tool window completo fica para o próximo recorte (GIT-003-02 em
//! diante); aqui só leitura, que a UI polla sem bloquear (NFR-001-02).

use std::path::Path;

/// Rótulo do VCS para widgets (ex.: `main`, `(detached a1b2c3d)`);
/// None fora de um repositório.
pub fn current_branch(root: &Path) -> Option<String> {
    phpide_vcs::detect(root).ok().map(|state| state.label())
}

/// True se o diretório pertence a um repositório git.
pub fn is_repo(root: &Path) -> bool {
    phpide_vcs::is_repo(root)
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
