//! Status por arquivo (porcelain v1) — apoio a stage/unstage e diff.
//!
//! `git status --porcelain=v1 -z` dá pares XY (index/worktree) e caminhos
//! separados por NUL, sem ambiguidade de espaços, aspas ou quebras de linha.

use std::path::{Path, PathBuf};

use phpide_core::{Component, ErrorKind, IdeError};

use crate::repo::{git_error, run_git};

/// Tipo de mudança em um arquivo (códigos do porcelain).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    /// Outros códigos (ex.: `U` de unmerged — fora do recorte P0).
    Other,
}

impl ChangeKind {
    /// `Some(None)` = slot vazio (' '); `Some(Some(k))` = mudança;
    /// `None` = código desconhecido (ex.: '?' de untracked).
    pub const fn from_code(c: u8) -> Option<Option<Self>> {
        match c {
            b' ' => Some(None),
            b'A' => Some(Some(ChangeKind::Added)),
            b'M' => Some(Some(ChangeKind::Modified)),
            b'D' => Some(Some(ChangeKind::Deleted)),
            b'R' => Some(Some(ChangeKind::Renamed)),
            b'C' => Some(Some(ChangeKind::Copied)),
            b'T' => Some(Some(ChangeKind::TypeChanged)),
            b'U' => Some(Some(ChangeKind::Other)),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            ChangeKind::Added => "A",
            ChangeKind::Modified => "M",
            ChangeKind::Deleted => "D",
            ChangeKind::Renamed => "R",
            ChangeKind::Copied => "C",
            ChangeKind::TypeChanged => "T",
            ChangeKind::Other => "?",
        }
    }
}

/// Estado de um arquivo no worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStatus {
    /// Caminho relativo ao worktree root.
    pub path: PathBuf,
    /// Mudança na área de staging (coluna X); None = não stageado.
    pub staged: Option<ChangeKind>,
    /// Mudança no working tree (coluna Y); None = limpo.
    pub unstaged: Option<ChangeKind>,
    /// Arquivo não rastreado (`??`): entra no diff como arquivo novo.
    pub untracked: bool,
}

impl FileStatus {
    /// Há algo a mostrar em algum dos dois lados.
    pub fn has_changes(&self) -> bool {
        self.untracked || self.staged.is_some() || self.unstaged.is_some()
    }
}

/// Desfaz a quotação estilo C que o porcelain v1 aplica a paths especiais.
fn parse_porcelain_z(bytes: &[u8]) -> Vec<FileStatus> {
    let mut list = Vec::new();
    let mut records = bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .peekable();
    while let Some(raw) = records.next() {
        if raw.len() < 4 {
            continue;
        }
        let (x, y, untracked) = match (raw[0], raw[1]) {
            (b'?', b'?') => (None, None, true),
            (xc, yc) => {
                let x = ChangeKind::from_code(xc).and_then(|value| value);
                let y = ChangeKind::from_code(yc).and_then(|value| value);
                if x.is_none() && y.is_none() {
                    continue;
                }
                (x, y, false)
            }
        };
        list.push(FileStatus {
            path: PathBuf::from(String::from_utf8_lossy(&raw[3..]).into_owned()),
            staged: x,
            unstaged: y,
            untracked,
        });
        if matches!(raw[0], b'R' | b'C') {
            let _old_path = records.next();
        }
    }
    list
}

pub fn git_version_at_least(version: &str, minimum: (u32, u32, u32)) -> bool {
    let mut parts = version
        .split_whitespace()
        .find(|part| {
            part.bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_digit())
        })
        .unwrap_or("0.0.0")
        .split('.')
        .map(|part| part.parse::<u32>().unwrap_or(0));
    let actual = (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    );
    actual >= minimum
}

/// Status de todos os arquivos alterados sob `root`.
pub fn status(root: &Path) -> Result<Vec<FileStatus>, IdeError> {
    let out = run_git(root, &["status", "--porcelain=v1", "-z"])?;
    if !out.status.success() {
        return Err(git_error("status", &out));
    }
    Ok(parse_porcelain_z(&out.stdout))
}

/// Status de um arquivo específico (None = limpo/ausente).
pub fn status_of(root: &Path, path: &Path) -> Result<Option<FileStatus>, IdeError> {
    let rel = relpath(root, path)?;
    Ok(status(root)?.into_iter().find(|s| s.path == rel))
}

/// Caminho relativo ao worktree root (para endereçar o git).
pub(crate) fn relpath(root: &Path, path: &Path) -> Result<PathBuf, IdeError> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    abs.strip_prefix(root)
        .map(|p| p.to_path_buf())
        .map_err(|_| {
            IdeError::new(
                Component::Git,
                "relpath",
                ErrorKind::InvalidInput,
                format!("caminho fora do worktree: {}", abs.display()),
                "abra um arquivo dentro do projeto atual",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_xy_codes_and_untracked() {
        let list =
            parse_porcelain_z(b" M src/a.php\0M  src/b.php\0A  src/c.php\0?? src/novo.php\0");
        assert_eq!(list.len(), 4);
        assert_eq!(list[0].path, PathBuf::from("src/a.php"));
        assert_eq!(list[0].staged, None);
        assert_eq!(list[0].unstaged, Some(ChangeKind::Modified));
        assert_eq!(list[1].staged, Some(ChangeKind::Modified));
        assert_eq!(list[1].unstaged, None);
        assert_eq!(list[2].staged, Some(ChangeKind::Added));
        assert!(list[3].untracked);
        assert!(list[3].has_changes());
        assert!(list[1].has_changes());
    }

    #[test]
    fn skips_clean_lines_and_short_input() {
        let list = parse_porcelain_z(b"M  a.php\0\0x\0  limpo.php\0");
        // '  limpo.php' tem os dois slots vazios: ignorado.
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn parses_nul_separated_rename_and_special_paths() {
        let list = parse_porcelain_z(
            b"R  novo nome.php\0nome antigo.php\0?? unicode-\xF0\x9F\x98\x80.php\0",
        );
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].path, PathBuf::from("novo nome.php"));
        assert_eq!(list[0].staged, Some(ChangeKind::Renamed));
        assert!(list[1].untracked);
        assert!(list[1].path.to_string_lossy().contains('😀'));
    }

    #[test]
    fn porcelain_z_requires_only_a_stable_git_baseline() {
        assert!(git_version_at_least("git version 2.39.2", (2, 20, 0)));
        assert!(!git_version_at_least("git version 2.19.9", (2, 20, 0)));
        if let Ok(output) = std::process::Command::new("git").arg("--version").output() {
            let version = String::from_utf8_lossy(&output.stdout);
            assert!(git_version_at_least(&version, (2, 20, 0)), "{version}");
        }
    }

    #[test]
    fn kinds_from_codes() {
        assert_eq!(ChangeKind::from_code(b' '), Some(None));
        assert_eq!(ChangeKind::from_code(b'A'), Some(Some(ChangeKind::Added)));
        assert_eq!(ChangeKind::from_code(b'?'), None);
        assert_eq!(ChangeKind::Added.as_str(), "A");
    }
}
