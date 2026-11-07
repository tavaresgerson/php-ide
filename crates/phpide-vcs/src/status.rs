//! Status por arquivo (porcelain v1) — apoio a stage/unstage e diff.
//!
//! `git status --porcelain=v1` dá pares XY (index/worktree) por arquivo;
//! paths com caracteres especiais vêm entre aspas com escapes estilo C —
//! o parser abaixo desfaz só o que o git produz (`\"` e `\\`).

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
fn unquote(path: &str) -> String {
    let p = path.as_bytes();
    if p.len() < 2 || p[0] != b'"' || p[p.len() - 1] != b'"' {
        return path.to_string();
    }
    let inner = &p[1..p.len() - 1];
    let mut out = Vec::with_capacity(inner.len());
    let mut i = 0;
    while i < inner.len() {
        if inner[i] == b'\\' && i + 1 < inner.len() {
            match inner[i + 1] {
                b'"' => out.push(b'"'),
                b'\\' => out.push(b'\\'),
                b'n' => out.push(b'\n'),
                b't' => out.push(b'\t'),
                other => {
                    out.push(b'\\');
                    out.push(other);
                }
            }
            i += 2;
        } else {
            out.push(inner[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parse do porcelain v1 (mesma função que `status` usa — UT cobre o real).
fn parse_porcelain(text: &str) -> Vec<FileStatus> {
    let mut list = Vec::new();
    for raw in text.lines() {
        if raw.len() < 4 {
            continue;
        }
        let bytes = raw.as_bytes();
        let (x, y, untracked) = match (bytes[0], bytes[1]) {
            (b'?', b'?') => (None, None, true),
            (xc, yc) => {
                let x = ChangeKind::from_code(xc).and_then(|o| o);
                let y = ChangeKind::from_code(yc).and_then(|o| o);
                if x.is_none() && y.is_none() {
                    continue;
                }
                (x, y, false)
            }
        };
        list.push(FileStatus {
            path: PathBuf::from(unquote(raw[3..].trim())),
            staged: x,
            unstaged: y,
            untracked,
        });
    }
    list
}

/// Status de todos os arquivos alterados sob `root`.
pub fn status(root: &Path) -> Result<Vec<FileStatus>, IdeError> {
    let out = run_git(root, &["status", "--porcelain=v1"])?;
    if !out.status.success() {
        return Err(git_error("status", &out));
    }
    Ok(parse_porcelain(&String::from_utf8_lossy(&out.stdout)))
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
        let list = parse_porcelain(" M src/a.php\nM  src/b.php\nA  src/c.php\n?? src/novo.php\n");
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
        let list = parse_porcelain("M  a.php\n\nx\n  limpo.php\n");
        // '  limpo.php' tem os dois slots vazios: ignorado.
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn unquotes_c_style_paths() {
        assert_eq!(unquote("src/a b.php"), "src/a b.php");
        assert_eq!(unquote("\"src/a b.php\""), "src/a b.php");
        assert_eq!(unquote("\"src/\\\"aspas\\\".php\""), "src/\"aspas\".php");
    }

    #[test]
    fn kinds_from_codes() {
        assert_eq!(ChangeKind::from_code(b' '), Some(None));
        assert_eq!(ChangeKind::from_code(b'A'), Some(Some(ChangeKind::Added)));
        assert_eq!(ChangeKind::from_code(b'?'), None);
        assert_eq!(ChangeKind::Added.as_str(), "A");
    }
}
