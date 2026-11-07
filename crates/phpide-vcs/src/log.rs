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
            "--format={0}%H{1}%h{1}%an{1}%ae{1}%at{1}%s",
            RECORD_SEP, FIELD_SEP
        ),
        "--name-status".into(),
        "--no-show-signature".into(),
    ];
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
        let mut files = Vec::new();
        for line in lines {
            let mut cols = line.split('\t');
            let code = cols.next().unwrap_or("");
            if code.is_empty() {
                continue;
            }
            let kind = code
                .as_bytes()
                .first()
                .and_then(|&c| ChangeKind::from_code(c).and_then(|o| o));
            let Some(kind) = kind else { continue };
            let path = PathBuf::from(cols.next().unwrap_or(""));
            let renamed_to = if matches!(kind, ChangeKind::Renamed | ChangeKind::Copied) {
                cols.next().map(PathBuf::from)
            } else {
                None
            };
            files.push(CommitFile {
                kind,
                path,
                renamed_to,
            });
        }
        entries.push(CommitEntry {
            sha: fields[0].to_string(),
            short: fields[1].to_string(),
            author: fields[2].to_string(),
            email: fields[3].to_string(),
            timestamp: fields[4].parse().unwrap_or(0),
            subject: fields[5].to_string(),
            files,
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
