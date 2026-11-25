//! Log de commits com arquivos — `T-GIT-003-01` (REQUISITOS GIT-003).
//!
//! `git log` com `--name-status` em um formato de registros separados por
//! \x1e e campos por \x1f (path nunca colide com os separadores). Filtros:
//! quantidade máxima, path (apenas commits que tocaram o arquivo) e revisão
//! inicial — o caso da navegação a partir do blame (GIT-004-02).

use std::path::{Path, PathBuf};

use phpide_core::IdeError;

use crate::repo::{git_error, run_git};
use crate::status::relpath;
use crate::status::ChangeKind;

/// Arquivo alterado por um commit (status do `--name-status`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitFile {
    pub kind: ChangeKind,
    pub path: PathBuf,
    /// Destino do rename (`R`/`C`); origem fica em `path`.
    pub renamed_to: Option<PathBuf>,
}

/// Entrada do log: commit + arquivos.
#[derive(Debug, Clone, PartialEq)]
pub struct CommitEntry {
    pub sha: String,
    pub short: String,
    pub author: String,
    pub email: String,
    /// epoch seconds.
    pub timestamp: i64,
    pub subject: String,
    pub files: Vec<CommitFile>,
    /// Parent shas (two or more for a merge).
    pub parents: Vec<String>,
    /// Branch and tag names pointing here (`HEAD -> main`, `origin/main`, `tag: v1`).
    pub refs: Vec<String>,
}

/// Filtros do log.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LogFilter {
    /// Máximo de commits (0 = sem limite explícito; o git usa o default).
    pub max_count: usize,
    /// Apenas commits que tocam este path.
    pub path: Option<PathBuf>,
    /// Revisão inicial (sha/branch/tag) — `git log <rev>`; usado pela
    /// navegação "ir para o commit" a partir do blame.
    pub rev: Option<String>,
    /// Commits to skip (paging).
    pub skip: usize,
    /// Message text, case-insensitive.
    pub text: Option<String>,
    /// Author name or e-mail, case-insensitive.
    pub author: Option<String>,
    /// Every branch and tag, not just HEAD.
    pub all: bool,
    /// Leave out the changed files (faster for long lists).
    pub without_files: bool,
}

const RECORD_SEP: char = '\u{1e}';
const FIELD_SEP: char = '\u{1f}';

/// Log de commits sob `root`.
pub fn log(root: &Path, filter: &LogFilter) -> Result<Vec<CommitEntry>, IdeError> {
    let mut args: Vec<String> = vec![
        "log".into(),
        // \x1e só no INÍCIO: o --name-status precisa ficar no MESMO chunk
        // do cabeçalho quando o texto é dividido pelo separador.
        format!(
            "--format={0}%H{1}%h{1}%an{1}%ae{1}%at{1}%s{1}%P{1}%D",
            RECORD_SEP, FIELD_SEP
        ),
        "--no-show-signature".into(),
        "--decorate-refs-exclude=refs/remotes/*/HEAD".into(),
        "--date-order".into(),
    ];
    if !filter.without_files {
        args.push("--name-status".into());
    }
    if filter.skip > 0 {
        args.push(format!("--skip={}", filter.skip));
    }
    if let Some(text) = filter.text.as_ref().filter(|t| !t.trim().is_empty()) {
        args.push("-i".into());
        args.push("--fixed-strings".into());
        args.push(format!("--grep={}", text.trim()));
    }
    if let Some(author) = filter.author.as_ref().filter(|t| !t.trim().is_empty()) {
        args.push("-i".into());
        args.push(format!("--author={}", regex_escape(author.trim())));
    }
    if filter.all {
        args.push("--all".into());
    }
    if filter.max_count > 0 {
        args.push(format!("--max-count={}", filter.max_count));
    }
    if let Some(rev) = &filter.rev {
        args.push(rev.clone());
    }
    let rel;
    if let Some(p) = &filter.path {
        rel = relpath(root, p)?;
        args.push("--".into());
        args.push(rel.to_string_lossy().into_owned());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = run_git(root, &refs)?;
    if !out.status.success() {
        return Err(git_error("log", &out));
    }
    Ok(parse_log(&String::from_utf8_lossy(&out.stdout)))
}

/// `--author` takes a regex; the filter is plain text.
fn regex_escape(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if "\\.^$|?*+()[]{}".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Full message (subject and body) of a commit.
pub fn commit_message(root: &Path, sha: &str) -> Result<String, IdeError> {
    let out = run_git(root, &["show", "-s", "--format=%B", sha])?;
    if !out.status.success() {
        return Err(git_error("commit_message", &out));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

/// Changed files of one commit against its first parent (a merge lists
/// what it brought in; a root commit lists every file as added).
pub fn commit_files(root: &Path, sha: &str) -> Result<Vec<CommitFile>, IdeError> {
    let parent = format!("{sha}^");
    let has_parent = run_git(root, &["rev-parse", "--verify", "--quiet", &parent])?
        .status
        .success();
    let mut args = vec!["diff-tree", "-r", "--name-status", "-M", "--no-commit-id"];
    if has_parent {
        args.push(&parent);
    } else {
        args.push("--root");
    }
    args.push(sha);
    let out = run_git(root, &args)?;
    if !out.status.success() {
        return Err(git_error("commit_files", &out));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(parse_name_status)
        .collect())
}

/// `M\tpath` or `R087\told\tnew`.
fn parse_name_status(line: &str) -> Option<CommitFile> {
    let mut cols = line.split('\t');
    let code = cols.next()?;
    let kind = code
        .as_bytes()
        .first()
        .and_then(|&c| ChangeKind::from_code(c).and_then(|o| o))?;
    let path = PathBuf::from(cols.next()?);
    let renamed_to = if matches!(kind, ChangeKind::Renamed | ChangeKind::Copied) {
        cols.next().map(PathBuf::from)
    } else {
        None
    };
    Some(CommitFile {
        kind,
        path,
        renamed_to,
    })
}

/// Parse do output `--format=%x1e… --name-status`.
fn parse_log(text: &str) -> Vec<CommitEntry> {
    let mut entries = Vec::new();
    for record in text.split(RECORD_SEP) {
        let record = record.trim_matches('\n');
        if record.is_empty() {
            continue;
        }
        let mut lines = record.lines();
        let header = match lines.next() {
            Some(h) if h.contains(FIELD_SEP) => h,
            _ => continue,
        };
        let fields: Vec<&str> = header.split(FIELD_SEP).collect();
        if fields.len() < 6 {
            continue;
        }
        let files: Vec<CommitFile> = lines.filter_map(parse_name_status).collect();
        entries.push(CommitEntry {
            sha: fields[0].to_string(),
            short: fields[1].to_string(),
            author: fields[2].to_string(),
            email: fields[3].to_string(),
            timestamp: fields[4].parse().unwrap_or(0),
            subject: fields[5].to_string(),
            files,
            parents: fields
                .get(6)
                .map(|p| p.split_whitespace().map(str::to_string).collect())
                .unwrap_or_default(),
            refs: fields
                .get(7)
                .map(|r| {
                    r.split(", ")
                        .map(str::trim)
                        .filter(|r| !r.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
        });
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_records_with_files() {
        let text = format!(
            "{0}abc123def456{1}a1b2c3d{1}Ana{1}ana@ex.com{1}1700000000{1}assunto aqui\nA\tsrc/novo.php\nM\tsrc/mexido.php\n{0}fff111eee222{1}f1e2d3c{1}Beto{1}beto@ex.com{1}1700000100{1}outro{1}subject com {1} dentro\nD\tapagado.php\n",
            RECORD_SEP, FIELD_SEP
        );
        let entries = parse_log(&text);
        assert_eq!(entries.len(), 2);
        let e = &entries[0];
        assert_eq!(e.sha, "abc123def456");
        assert_eq!(e.short, "a1b2c3d");
        assert_eq!(e.author, "Ana");
        assert_eq!(e.timestamp, 1700000000);
        assert_eq!(e.subject, "assunto aqui");
        assert_eq!(e.files.len(), 2);
        assert_eq!(e.files[0].kind, ChangeKind::Added);
        assert_eq!(e.files[0].path, PathBuf::from("src/novo.php"));
        assert_eq!(e.files[1].kind, ChangeKind::Modified);
        // campo extra no subject (sep dentro do %s) é tolerado:
        assert_eq!(entries[1].subject, "outro");
        assert_eq!(entries[1].files[0].kind, ChangeKind::Deleted);
    }

    #[test]
    fn parses_parents_and_refs() {
        let text = format!(
            "{0}m{1}m{1}A{1}a@x{1}1{1}Merge{1}p1 p2{1}HEAD -> main, origin/main, tag: v1\n",
            RECORD_SEP, FIELD_SEP
        );
        let e = &parse_log(&text)[0];
        assert_eq!(e.parents, ["p1", "p2"]);
        assert_eq!(e.refs, ["HEAD -> main", "origin/main", "tag: v1"]);
        assert_eq!(regex_escape("a.b (c)"), "a\\.b \\(c\\)");
    }

    #[test]
    fn parses_rename_lines() {
        let text = format!(
            "{0}s{1}s{1}A{1}a@x{1}1{1}renomeia\nR087\told.php\tnew.php\n",
            RECORD_SEP, FIELD_SEP
        );
        let entries = parse_log(&text);
        assert_eq!(entries[0].files[0].kind, ChangeKind::Renamed);
        assert_eq!(entries[0].files[0].path, PathBuf::from("old.php"));
        assert_eq!(
            entries[0].files[0].renamed_to,
            Some(PathBuf::from("new.php"))
        );
    }

    #[test]
    fn empty_output_gives_empty_log() {
        assert!(parse_log("").is_empty());
        assert!(parse_log("only a random line\n").is_empty());
    }
}
