//! Gutter de blame — `T-UI-ED-007-01` (REQUISITOS UI-ED-007).
//!
//! Cache por (arquivo, revisão do documento) com dados **reais** do
//! `phpide-vcs::blame`. Aceite: linha sem autoria aparece como desconhecida
//! ("·"), nunca como sucesso; edição local invalida (o blame reflete o
//! disco: enquanto o buffer é editado sem salvar, as anotações ficam
//! *stale* e a UI sinaliza isso — mesma semântica do IntelliJ).

use phpide_vcs::BlameEntry;

/// Anotação de uma linha para exibição.
#[derive(Debug, Clone, PartialEq)]
pub struct BlameAnnotation {
    /// Autor curto (iniciais) ou None = desconhecido.
    pub author_initials: Option<String>,
    /// Data resumida (YYYY-MM-DD) ou None.
    pub date: Option<String>,
    /// Commit curto para navegação.
    pub sha_short: Option<String>,
    /// Linha ainda não commitada.
    pub uncommitted: bool,
}

/// Cache de blame por arquivo.
#[derive(Debug, Clone, Default)]
pub struct BlameCache {
    /// (path, conteúdo-salvo) da última consulta.
    loaded: Option<(String, u64)>,
    entries: Vec<BlameEntry>,
    /// Hash simples do texto salvo usado como "revisão" (FNV-1a 64).
    text_hash: u64,
}

impl BlameCache {
    /// Texto salvo mudou? (invalidação por edição/save — NFR-002-04).
    pub fn is_stale(&self, path: &str, text_hash: u64) -> bool {
        self.loaded
            .as_ref()
            .is_none_or(|(p, h)| p != path || *h != text_hash)
    }

    /// Registra resultado fresco do `phpide_vcs::blame`.
    pub fn load(&mut self, path: &str, text_hash: u64, entries: Vec<BlameEntry>) {
        self.loaded = Some((path.to_string(), text_hash));
        self.entries = entries;
        self.text_hash = text_hash;
    }

    /// Anotação da linha (1-based). Fora do conhecido → desconhecida.
    pub fn annotate(&self, line1: usize) -> BlameAnnotation {
        let entry = self
            .entries
            .iter()
            .find(|e| line1 >= e.final_start as usize && line1 <= e.final_end() as usize);
        match entry {
            None => BlameAnnotation {
                author_initials: None,
                date: None,
                sha_short: None,
                uncommitted: false,
            },
            Some(e) => BlameAnnotation {
                author_initials: if e.is_uncommitted() {
                    None
                } else {
                    Some(initials(&e.author))
                },
                date: if e.is_uncommitted() {
                    None
                } else {
                    Some(epoch_to_date(e.timestamp))
                },
                sha_short: if e.is_uncommitted() {
                    None
                } else {
                    Some(e.sha.chars().take(7).collect())
                },
                uncommitted: e.is_uncommitted(),
            },
        }
    }

    /// Sha da linha para navegação ao commit (UI-ED-007).
    #[allow(dead_code)]
    pub fn sha_at(&self, line1: usize) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| line1 >= e.final_start as usize && line1 <= e.final_end() as usize)
            .filter(|e| !e.is_uncommitted())
            .map(|e| e.sha.as_str())
    }
}

/// Iniciais do autor (primeiras letras dos primeiros dois tokens).
fn initials(author: &str) -> String {
    let mut out: String = author
        .split_whitespace()
        .filter_map(|t| t.chars().next())
        .take(2)
        .collect();
    if out.is_empty() {
        out.push('?');
    }
    out.to_uppercase()
}

/// Epoch → YYYY-MM-DD (UTC, sem crate de data).
fn epoch_to_date(ts: i64) -> String {
    let days = ts.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Algoritmo de Howard Hinnant (days_from_civil invertido).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// FNV-1a 64: "revisão" barata do texto salvo.
pub fn text_hash(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(start: u32, count: u32, author: &str, ts: i64, uncommitted: bool) -> BlameEntry {
        BlameEntry {
            sha: if uncommitted {
                "0".repeat(40)
            } else {
                "abc123def4567890abc123def4567890abc1234".into()
            },
            final_start: start,
            line_count: count,
            author: author.into(),
            email: String::new(),
            timestamp: ts,
            summary: String::new(),
        }
    }

    #[test]
    fn annotate_and_staleness() {
        let mut c = BlameCache::default();
        assert!(c.is_stale("a.php", 1));
        c.load(
            "a.php",
            42,
            vec![
                entry(1, 2, "Ana Souza", 1_700_000_000, false),
                entry(3, 1, "Beto", 0, true), // não commitado
            ],
        );
        assert!(!c.is_stale("a.php", 42));
        assert!(c.is_stale("a.php", 43), "edição invalida");
        assert!(c.is_stale("b.php", 42), "outro arquivo invalida");
        let a = c.annotate(2);
        assert_eq!(a.author_initials.as_deref(), Some("AS"));
        assert_eq!(a.date.as_deref(), Some("2023-11-14"));
        assert_eq!(a.sha_short.as_deref(), Some("abc123d"));
        let u = c.annotate(3);
        assert!(u.uncommitted);
        assert_eq!(u.author_initials, None, "não commitado = desconhecido");
        let none = c.annotate(99);
        assert_eq!(
            none.author_initials, None,
            "fora do blame = desconhecido, não sucesso"
        );
        assert_eq!(c.sha_at(2), Some("abc123def4567890abc123def4567890abc1234"));
        assert_eq!(c.sha_at(3), None, "não commitado não navega");
    }

    #[test]
    fn initials_and_hash() {
        assert_eq!(initials("Ana Souza"), "AS");
        assert_eq!(initials("Madonna"), "M");
        assert_eq!(initials(""), "?");
        assert_ne!(text_hash("a"), text_hash("b"));
        assert_eq!(text_hash("x"), text_hash("x"));
    }
}
