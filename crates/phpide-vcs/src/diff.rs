//! Diff de working tree e staged — `T-GIT-002-01` (REQUISITOS GIT-002 P0).
//!
//! Modelo de diff unificado parseado de `git diff [--cached] -- <path>`.
//! Arquivos untracked não aparecem no `git diff`; para eles o diff é
//! sintetizado com todas as linhas como adicionadas (é o que a UI espera
//! ao abrir um arquivo novo não rastreado).

use std::path::{Path, PathBuf};

use phpide_core::IdeError;

use crate::repo::{git_error, run_git};
use crate::status::{relpath, status_of};

/// Linha de um hunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    /// Conteúdo SEM o prefixo (+/-/espaço).
    pub content: String,
    /// Linha no arquivo antigo (1-based); None = linha adicionada.
    pub old_lineno: Option<u32>,
    /// Linha no arquivo novo (1-based); None = linha removida.
    pub new_lineno: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Add,
    Del,
    Context,
}

/// Hunk do diff unificado (`@@ -a,b +c,d @@`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub lines: Vec<DiffLine>,
}

/// Diff de um arquivo (rename: old_path != new_path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub old_path: PathBuf,
    pub new_path: PathBuf,
    pub hunks: Vec<Hunk>,
}

impl FileDiff {
    /// Há mudanças? (diff vazio de arquivo sem alteração)
    pub fn is_empty(&self) -> bool {
        self.hunks
            .iter()
            .all(|h| h.lines.iter().all(|l| l.kind == DiffLineKind::Context))
    }
}

/// Parse de diff unificado (o que o `git diff` produz).
///
/// Tolera `\ No newline at end of file`, arquivos novos/removidos
/// (`--- /dev/null`) e renomes (`--- a/old` / `+++ b/new`).
pub fn parse_unified(text: &str) -> Vec<FileDiff> {
    let mut files = Vec::new();
    let mut cur: Option<FileDiff> = None;
    let mut old_lineno = 0u32;
    let mut new_lineno = 0u32;

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("--- ") {
            let old_path = strip_prefix_path(rest);
            if let Some(f) = cur.take() {
                files.push(f);
            }
            cur = Some(FileDiff {
                old_path,
                new_path: PathBuf::new(),
                hunks: Vec::new(),
            });
        } else if let Some(rest) = line.strip_prefix("+++ ") {
            if let Some(f) = cur.as_mut() {
                f.new_path = strip_prefix_path(rest);
            }
        } else if let Some(rest) = line.strip_prefix("@@ ") {
            // @@ -a[,b] +c[,d] @@ <contexto opcional>
            let header = match rest.split_once(" @@") {
                Some((h, _)) => h,
                None => rest,
            };
            let header = header.trim_start_matches('-');
            let (old_part, new_part) = match header.split_once(" +") {
                Some(t) => t,
                None => continue,
            };
            let (old_start, old_count) = parse_range(old_part);
            let (new_start, new_count) = parse_range(new_part);
            old_lineno = old_start;
            new_lineno = new_start;
            if let Some(f) = cur.as_mut() {
                f.hunks.push(Hunk {
                    old_start,
                    old_count,
                    new_start,
                    new_count,
                    lines: Vec::new(),
                });
            }
        } else if line.starts_with("\\ No newline") {
            // Mantido implícito: a linha anterior já foi registrada.
        } else if line.starts_with("diff ") || line.starts_with("index ") {
            // Metadados entre arquivos; nada a fazer.
        } else if let Some(rest) = line.strip_prefix("rename from ") {
            if let Some(f) = cur.as_mut() {
                f.old_path = PathBuf::from(rest.trim());
            }
        } else if let Some(rest) = line.strip_prefix("rename to ") {
            if let Some(f) = cur.as_mut() {
                f.new_path = PathBuf::from(rest.trim());
            }
        } else if !line.is_empty() {
            let (kind, content) = if let Some(c) = line.strip_prefix('+') {
                (DiffLineKind::Add, c)
            } else if let Some(c) = line.strip_prefix('-') {
                (DiffLineKind::Del, c)
            } else if let Some(c) = line.strip_prefix(' ') {
                (DiffLineKind::Context, c)
            } else {
                continue; // ex.: "Binary files ... differ"
            };
            let entry = DiffLine {
                kind,
                content: content.to_string(),
                old_lineno: (kind != DiffLineKind::Add).then_some(old_lineno),
                new_lineno: (kind != DiffLineKind::Del).then_some(new_lineno),
            };
            match kind {
                DiffLineKind::Add => new_lineno += 1,
                DiffLineKind::Del => old_lineno += 1,
                DiffLineKind::Context => {
                    old_lineno += 1;
                    new_lineno += 1;
                }
            }
            if let Some(f) = cur.as_mut() {
                if let Some(h) = f.hunks.last_mut() {
                    h.lines.push(entry);
                }
            }
        }
    }
    if let Some(f) = cur.take() {
        files.push(f);
    }
    files
}

/// `a/src/x.php` → `src/x.php`; `/dev/null` fica como está.
fn strip_prefix_path(rest: &str) -> PathBuf {
    let p = rest.trim();
    if let Some(rel) = p.strip_prefix("a/").or_else(|| p.strip_prefix("b/")) {
        PathBuf::from(rel)
    } else {
        PathBuf::from(p)
    }
}

/// `"-1,3"` → (1,3); `"-5"` → (5,1).
fn parse_range(part: &str) -> (u32, u32) {
    match part.split_once(',') {
        Some((s, c)) => (s.parse().unwrap_or(0), c.parse().unwrap_or(0)),
        None => (part.parse().unwrap_or(0), 1),
    }
}

/// Diff do working tree (não stageado) de `path` relativo a `root`.
/// None = sem alterações (arquivo limpo e rastreado).
pub fn diff(root: &Path, path: &Path) -> Result<Option<FileDiff>, IdeError> {
    diff_cmd(root, path, false)
}

/// Diff staged (index vs HEAD) de `path`. None = não stageado/sem mudança.
pub fn diff_staged(root: &Path, path: &Path) -> Result<Option<FileDiff>, IdeError> {
    diff_cmd(root, path, true)
}

/// Diff combinado working tree + index vs HEAD — base dos line markers
/// (GIT-004-01): continua visível após stage e só zera no commit.
pub fn diff_head(root: &Path, path: &Path) -> Result<Option<FileDiff>, IdeError> {
    let rel = relpath(root, path)?;
    // Untracked: HEAD diff não o inclui; sintetiza 100% adicionado.
    if let Some(st) = status_of(root, path)? {
        if st.untracked {
            return Ok(Some(untracked_diff(&rel, root)));
        }
    }
    let rel_s = rel.to_string_lossy().into_owned();
    let out = run_git(
        root,
        &["diff", "HEAD", "--no-color", "--no-ext-diff", "--", &rel_s],
    )?;
    if !out.status.success() {
        return Err(git_error("diff_head", &out));
    }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    Ok(parse_unified(&text)
        .into_iter()
        .next()
        .filter(|f| !f.is_empty()))
}

fn diff_cmd(root: &Path, path: &Path, staged: bool) -> Result<Option<FileDiff>, IdeError> {
    let rel = relpath(root, path)?;
    // Untracked: git diff não o inclui; sintetiza arquivo 100% adicionado.
    if !staged {
        if let Some(st) = status_of(root, path)? {
            if st.untracked {
                return Ok(Some(untracked_diff(&rel, root)));
            }
        }
    }
    let mut args = vec!["diff", "--no-color", "--no-ext-diff"];
    if staged {
        args.push("--cached");
    }
    args.push("--");
    let rel_s = rel.to_string_lossy().into_owned();
    args.push(&rel_s);
    let out = run_git(root, &args)?;
    if !out.status.success() {
        return Err(git_error("diff", &out));
    }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    Ok(parse_unified(&text)
        .into_iter()
        .next()
        .filter(|f| !f.is_empty()))
}

/// Diff sintético de arquivo untracked: todas as linhas adicionadas.
fn untracked_diff(rel: &Path, root: &Path) -> FileDiff {
    let content = std::fs::read_to_string(root.join(rel)).unwrap_or_default();
    let count = content.lines().count() as u32;
    let lines = content
        .lines()
        .enumerate()
        .map(|(i, l)| DiffLine {
            kind: DiffLineKind::Add,
            content: l.to_string(),
            old_lineno: None,
            new_lineno: Some(i as u32 + 1),
        })
        .collect();
    FileDiff {
        old_path: PathBuf::from("/dev/null"),
        new_path: rel.to_path_buf(),
        hunks: vec![Hunk {
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count: count,
            lines,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phpide_core::{Component, ErrorKind};

    const SAMPLE: &str = "diff --git a/src/a.php b/src/a.php
index 111..222 100644
--- a/src/a.php
+++ b/src/a.php
@@ -2,3 +2,4 @@ ctx
 linha ctx
-linha removida
+linha adicionada
+outra adicionada
\\ No newline at end of file
";

    #[test]
    fn parses_unified_with_line_numbers() {
        let files = parse_unified(SAMPLE);
        assert_eq!(files.len(), 1);
        let f = &files[0];
        assert_eq!(f.old_path, PathBuf::from("src/a.php"));
        assert_eq!(f.new_path, PathBuf::from("src/a.php"));
        assert_eq!(f.hunks.len(), 1);
        let h = &f.hunks[0];
        assert_eq!((h.old_start, h.old_count), (2, 3));
        assert_eq!((h.new_start, h.new_count), (2, 4));
        assert_eq!(h.lines.len(), 4); // ctx + del + add + add
        assert_eq!(h.lines[0].kind, DiffLineKind::Context);
        assert_eq!(h.lines[0].old_lineno, Some(2));
        assert_eq!(h.lines[0].new_lineno, Some(2));
        assert_eq!(h.lines[1].kind, DiffLineKind::Del);
        assert_eq!(h.lines[1].old_lineno, Some(3));
        assert_eq!(h.lines[1].new_lineno, None);
        assert_eq!(h.lines[2].kind, DiffLineKind::Add);
        assert_eq!(h.lines[2].old_lineno, None);
        assert_eq!(h.lines[2].new_lineno, Some(3));
        assert_eq!(h.lines[3].new_lineno, Some(4));
    }

    #[test]
    fn parses_new_file_and_rename() {
        let new_file = "--- /dev/null\n+++ b/novo.php\n@@ -0,0 +1,2 @@\n+um\n+dois\n";
        let files = parse_unified(new_file);
        assert_eq!(files[0].old_path, PathBuf::from("/dev/null"));
        assert_eq!(files[0].hunks[0].new_count, 2);

        let renamed = "--- a/old.php\n+++ b/new.php\n@@ -1,1 +1,1 @@\n x\n";
        let files = parse_unified(renamed);
        assert_eq!(files[0].old_path, PathBuf::from("old.php"));
        assert_eq!(files[0].new_path, PathBuf::from("new.php"));
    }

    #[test]
    fn empty_diff_is_none_semantics() {
        let files = parse_unified("--- a/x\n+++ b/x\n@@ -1,1 +1,1 @@\n igual\n");
        assert!(files[0].is_empty());
    }

    #[test]
    fn error_kind_for_missing_component() {
        // relpath fora do root é InvalidInput (defesa da UI).
        let root = Path::new("/tmp/repo-x");
        let err = relpath(root, Path::new("/outro/lugar.php")).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        assert_eq!(err.component, Component::Git);
    }
}
