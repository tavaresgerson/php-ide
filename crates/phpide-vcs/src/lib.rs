//! Git como provedor do editor — `T-GIT-001/002/003/004` (REQUISITOS §11).
//!
//! Wrap do `git` CLI (mesma política dos demais crates: ferramenta real, sem
//! biblioteca nativa — comparação de output acontece contra o Git de verdade).
//! Nada aqui bloqueia a UI: toda operação é uma chamada síncrona curta que a
//! UI dispara em background/polling (NFR-001-02).
//!
//! - [`repo`]: detecção de worktree, branch, detached HEAD e repo inválido;
//! - [`status`]: status por arquivo (staged/unstaged) em porcelain v1;
//! - [`diff`]: diff de working tree e staged com parser unified;
//! - [`ops`]: stage/unstage por arquivo e commit com hooks e preservação
//!   do índice em falha;
//! - [`log`]: log de commits com arquivos (`--name-status`) e filtros;
//! - [`markers`]: line markers derivados do diff (added/modified/removed);
//! - [`blame`]: blame por revisão (porcelain) e navegação ao commit.

pub mod blame;
pub mod diff;
pub mod log;
pub mod markers;
pub mod ops;
pub mod repo;
pub mod status;

pub use blame::{blame, blame_rev, line_blame, BlameEntry};
pub use diff::{
    diff, diff_head, diff_staged, parse_unified, DiffLine, DiffLineKind, FileDiff, Hunk,
};
pub use log::{log, CommitEntry, CommitFile, LogFilter};
pub use markers::{markers, markers_for_file, LineMarker};
pub use ops::{commit, stage, unstage, CommitResult};
pub use repo::{detect, is_repo, worktree_root, RepoState};
pub use status::{status, ChangeKind, FileStatus};
