//! Blame por revisão — `T-GIT-004-02` (REQUISITOS GIT-004, segue UI-ED-007).
//!
//! Parse do `git blame --porcelain` (estável entre versões, diferente do
//! formato humano). Cada entrada cobre um intervalo de linhas do arquivo
//! corrente; entradas consecutivas com mesmo commit são agregadas. A
//! navegação ao commit usa `sha` + [`crate::log`] com `LogFilter::rev`.

use std::path::Path;

use phpide_core::IdeError;

use crate::repo::{git_error, run_git};
use crate::status::relpath;

/// Origem de um intervalo de linhas.
#[derive(Debug, Clone, PartialEq)]
pub struct BlameEntry {
    /// SHA do commit que introduziu a linha; 40 zeros = não commitado.
    pub sha: String,
    /// Linha inicial no arquivo corrente (1-based).
    pub final_start: u32,
    /// Quantas linhas o intervalo cobre.
    pub line_count: u32,
    pub author: String,
    pub email: String,
    /// epoch seconds do autor.
    pub timestamp: i64,
    /// Assunto do commit (summary).
    pub summary: String,
}

impl BlameEntry {
    /// True se a linha ainda não foi commitada (working tree/index).
    pub fn is_uncommitted(&self) -> bool {
        self.sha.chars().all(|c| c == '0')
    }

    /// Última linha do intervalo (inclusive).
    pub fn final_end(&self) -> u32 {
        self.final_start + self.line_count.saturating_sub(1)
    }
}

/// Blame de HEAD para `path`.
pub fn blame(root: &Path, path: &Path) -> Result<Vec<BlameEntry>, IdeError> {
    blame_cmd(root, None, path)
}

/// Blame de uma revisão específica (sha/branch/tag).
pub fn blame_rev(root: &Path, rev: &str, path: &Path) -> Result<Vec<BlameEntry>, IdeError> {
    blame_cmd(root, Some(rev), path)
}

fn blame_cmd(root: &Path, rev: Option<&str>, path: &Path) -> Result<Vec<BlameEntry>, IdeError> {
    let rel = relpath(root, path)?;
    let rel_s = rel.to_string_lossy().into_owned();
    let rev_s = rev.unwrap_or("").to_string();
    let args: Vec<&str> = match rev {
        Some(_) => vec!["blame", "--porcelain", &rev_s, "--", &rel_s],
        None => vec!["blame", "--porcelain", "--", &rel_s],
    };
    let out = run_git(root, &args)?;
    if !out.status.success() {
        return Err(git_error("blame", &out));
    }
    Ok(parse_blame(&String::from_utf8_lossy(&out.stdout)))
}

/// Entrada que abriga o parse parcial entre o cabeçalho e a linha de conteúdo.
#[derive(Default)]
struct Pending {
    sha: String,
    final_start: u32,
    author: String,
    email: String,
    timestamp: i64,
    summary: String,
}

/// Parse do porcelain: cabeçalho `<sha> <orig> <final> [count]`, chaves
/// `author/author-mail/author-time/summary`, conteúdo em linha `\t…`.
/// Linhas de continuação de um grupo repetem o cabeçalho SEM as chaves — a
/// meta (autor/hora/summary) é herdada do último commit igual; entradas
/// consecutivas com mesmo sha+meta são agregadas ao final.
fn parse_blame(text: &str) -> Vec<BlameEntry> {
    let mut raw: Vec<BlameEntry> = Vec::new();
    let mut pending: Option<Pending> = None;
    // sha → meta já vista (autor, email, timestamp, summary).
    let mut meta_by_sha: std::collections::HashMap<String, (String, String, i64, String)> =
        std::collections::HashMap::new();
    for line in text.lines() {
        if line.starts_with('\t') {
            // Linha de conteúdo: fecha uma linha para o pending corrente,
            // herdando a meta do grupo quando o cabeçalho não a trouxe.
            if let Some(mut p) = pending.take() {
                if p.author.is_empty() {
                    if let Some((a, e, t, s)) = meta_by_sha.get(&p.sha) {
                        p.author.clone_from(a);
                        p.email.clone_from(e);
                        p.timestamp = *t;
                        p.summary.clone_from(s);
                    }
                }
                meta_by_sha.entry(p.sha.clone()).or_insert_with(|| {
                    (
                        p.author.clone(),
                        p.email.clone(),
                        p.timestamp,
                        p.summary.clone(),
                    )
                });
                push_line(&mut raw, p);
            }
            continue;
        }
        let bytes = line.as_bytes();
        // Cabeçalho: sha de 40 hex (possível prefixo '^' de boundary).
        let header_end = bytes
            .iter()
            .position(|b| *b == b' ')
            .filter(|_| line.len() >= 41);
        if let Some(pos) = header_end {
            let sha_raw = &line[..pos];
            let sha = sha_raw.strip_prefix('^').unwrap_or(sha_raw).to_string();
            if sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit()) {
                let mut cols = line[pos + 1..].split_whitespace();
                let _orig = cols.next();
                let final_start: u32 = cols.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                pending = Some(Pending {
                    sha,
                    final_start,
                    ..Pending::default()
                });
                continue;
            }
        }
        // Chaves de metadados aplicam-se ao pending.
        if let Some(p) = pending.as_mut() {
            if let Some(v) = line.strip_prefix("author ") {
                p.author = v.to_string();
            } else if let Some(v) = line.strip_prefix("author-mail ") {
                p.email = v.trim_start_matches('<').trim_end_matches('>').to_string();
            } else if let Some(v) = line.strip_prefix("author-time ") {
                p.timestamp = v.parse().unwrap_or(0);
            } else if let Some(v) = line.strip_prefix("summary ") {
                p.summary = v.to_string();
            }
        }
    }
    if let Some(p) = pending.take() {
        push_line(&mut raw, p);
    }
    // Agrega consecutivos com a mesma origem.
    let mut merged: Vec<BlameEntry> = Vec::new();
    for e in raw {
        match merged.last_mut() {
            Some(prev)
                if prev.sha == e.sha
                    && prev.author == e.author
                    && prev.timestamp == e.timestamp
                    && prev.summary == e.summary
                    && prev.final_end() + 1 == e.final_start =>
            {
                prev.line_count += e.line_count;
            }
            _ => merged.push(e),
        }
    }
    merged
}

fn push_line(raw: &mut Vec<BlameEntry>, p: Pending) {
    if p.final_start == 0 {
        return;
    }
    let last_same = raw.last().filter(|e| e.sha == p.sha);
    let final_start = if let Some(prev) = last_same {
        prev.final_end() + 1
    } else {
        p.final_start
    };
    raw.push(BlameEntry {
        sha: p.sha,
        final_start,
        line_count: 1,
        author: p.author,
        email: p.email,
        timestamp: p.timestamp,
        summary: p.summary,
    });
}

/// Entrada responsável pela linha `line` (1-based).
pub fn line_blame(entries: &[BlameEntry], line: u32) -> Option<&BlameEntry> {
    entries
        .iter()
        .find(|e| line >= e.final_start && line <= e.final_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PORCELAIN: &str = "abc0000000000000000000000000000000000001 1 1 2
author Ana
author-mail <ana@ex.com>
author-time 1700000000
author-tz -0300
summary inicial
boundary
filename f.php
\tlinha um
abc0000000000000000000000000000000000001 2 2
\tlinha dois
def0000000000000000000000000000000000002 3 3
author Beto
author-mail <beto@ex.com>
author-time 1700000100
summary segunda
filename f.php
\tlinha tres
";

    #[test]
    fn parses_and_merges_consecutive_same_commit() {
        let entries = parse_blame(PORCELAIN);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].sha, "abc0000000000000000000000000000000000001");
        assert_eq!(entries[0].final_start, 1);
        assert_eq!(
            entries[0].line_count, 2,
            "duas linhas do mesmo commit agregadas"
        );
        assert_eq!(entries[0].author, "Ana");
        assert_eq!(entries[0].email, "ana@ex.com");
        assert_eq!(entries[1].final_start, 3);
        assert_eq!(entries[1].summary, "segunda");
    }

    #[test]
    fn line_lookup_and_uncommitted() {
        let entries = parse_blame(PORCELAIN);
        assert_eq!(line_blame(&entries, 2).unwrap().author, "Ana");
        assert_eq!(line_blame(&entries, 3).unwrap().author, "Beto");
        assert!(line_blame(&entries, 4).is_none());
        let un = BlameEntry {
            sha: "0".repeat(40),
            final_start: 1,
            line_count: 1,
            author: "Not Committed Yet".into(),
            email: String::new(),
            timestamp: 0,
            summary: String::new(),
        };
        assert!(un.is_uncommitted());
        assert!(!entries[0].is_uncommitted());
    }

    #[test]
    fn boundary_caret_is_stripped() {
        // Mesmo sha com '^' de boundary (construído a partir da fixture para
        // não depender de contar zeros na mão).
        let sha = "abc0000000000000000000000000000000000001";
        let header = format!("{sha} 1 1 2");
        let caret_sha = format!("^{}", &sha[1..]);
        let text = PORCELAIN.replacen(&header, &format!("{caret_sha} 1 1 2"), 1);
        let entries = parse_blame(&text);
        assert_eq!(entries[0].sha, sha);
    }
}
