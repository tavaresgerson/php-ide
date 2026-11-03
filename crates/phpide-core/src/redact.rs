//! Redaction de segredos para logs — `T-NFR-005-03` (NFR-005/NFR-008).
//!
//! Nada de senhas, tokens, chaves ou credenciais pode aparecer em log
//! (`NFR-005`: "Credenciais nunca devem ser incluídas em logs"). Toda linha
//! que atravessa [`crate::log::Logger`] passa por [`redact_text`], e pontos
//! de captura adicionais (painéis, mensagens de erro) podem aplicar a mesma
//! função diretamente.
//!
//! Padrões cobertos (cada um com teste adversarial):
//! - `chave=valor` / `chave: valor` para chaves sensíveis (`TOKEN=ghp_x`,
//!   `password: hunter2`), incluindo JSON `"password": "hunter2"`;
//! - flags de CLI: `--token=abc`, `--token abc`, `-pS3creta` (mysql/psql);
//! - userinfo em URIs: `postgres://user:senha@host` → `postgres://***@host`;
//! - `Authorization: Bearer <jwt>`;
//! - chaves não sensíveis (`tokenize`, `monkey`) e valores não atribuídos
//!   a chave sensível permanecem intactos.

use regex::Regex;
use std::sync::OnceLock;

struct Patterns {
    /// `"chave": "valor"` (JSON com chave citada).
    json_kv: Regex,
    /// `chave=valor` / `chave: valor` com chave sensível.
    kv: Regex,
    /// `--chave=valor` / `--chave valor` / `-pvalor` em linhas de comando.
    flag: Regex,
    /// `esquema://user:pass@host` → `esquema://***@host`.
    uri_userinfo: Regex,
    /// `Bearer <token>`.
    bearer: Regex,
}

const SENSITIVE_KEYS: &str = "password|passwd|pwd|pass|token|secret|api[_-]?key|apikey|credential|credentials|private[_-]?key|client[_-]?secret|access[_-]?key|refresh[_-]?token|auth(?:orization)?|cookie|session";

fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    // A chave sensível pode vir composta (GITHUB_TOKEN) ou citada ('token').
    // Não usamos look-around (regex crate não suporta): exigir o separador
    // logo após a chave já impede casar "tokens"/"tokenize" por acidente.
    let key = format!(r"(?:[a-z0-9]+_)*(?:{SENSITIVE_KEYS})");
    P.get_or_init(|| Patterns {
        json_kv: Regex::new(&format!(
            r#"(?i)("(?:{SENSITIVE_KEYS})"\s*:\s*)("[^"]*")"#
        ))
        .expect("json_kv"),
        kv: Regex::new(&format!(
            r#"(?i)(^|[^\w-])({key})(['"]?)(\s*[:=]\s*)("[^"]*"|'[^']*'|[^\s,;&]+)"#
        ))
        .expect("kv"),
        flag: Regex::new(
            r#"(?i)(--?(?:password|passwd|pwd|token|secret|api[_-]?key|apikey|credential|auth)\b(?:=|\s+))(\S+)|(\s-p)\S+"#,
        )
        .expect("flag"),
        uri_userinfo: Regex::new(r"([a-zA-Z][a-zA-Z0-9+.-]*://)[^\s/@:]+:[^\s/@]+@")
            .expect("uri_userinfo"),
        bearer: Regex::new(r"(?i)\b(bearer\s+)[a-z0-9._~+/=-]{8,}").expect("bearer"),
    })
}

/// Aplica todas as regras de redaction. Idempotente e sem alocação extra
/// quando nada casa (retorna clone apenas se houve mudança).
pub fn redact_text(input: &str) -> String {
    let p = patterns();
    let mut out = p.uri_userinfo.replace_all(input, "${1}***@").into_owned();
    out = p.bearer.replace_all(&out, "${1}***").into_owned();
    out = p.json_kv.replace_all(&out, "${1}\"***\"").into_owned();
    out =
        p.kv.replace_all(&out, |c: &regex::Captures| {
            let value = c.get(5).map(|m| m.as_str()).unwrap_or("");
            let already = matches!(value, "***" | "\"***\"" | "'***'");
            let is_scheme = value.eq_ignore_ascii_case("bearer")
                || value.eq_ignore_ascii_case("basic")
                || value.eq_ignore_ascii_case("digest");
            if already || is_scheme {
                c.get(0).unwrap().as_str().to_string()
            } else {
                // Preserva aspas de valores citados: 'x' → '***'.
                let redacted = if value.len() >= 2 && value.starts_with('"') && value.ends_with('"')
                {
                    "\"***\"".to_string()
                } else if value.len() >= 2 && value.starts_with('\'') && value.ends_with('\'') {
                    "'***'".to_string()
                } else {
                    "***".to_string()
                };
                format!("{}{}{}{}{}", &c[1], &c[2], &c[3], &c[4], redacted)
            }
        })
        .into_owned();
    out = p
        .flag
        .replace_all(&out, |c: &regex::Captures| {
            if c.get(3).is_some() {
                format!("{}***", &c[3])
            } else {
                format!("{}***", c.get(1).unwrap().as_str())
            }
        })
        .into_owned();
    out
}

/// Wrapper para valores que são segredos: `Display`/`Debug` emitem `***`,
/// e o valor só é legível via [`Secret::expose`] (nunca em log).
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Secret(value.into())
    }

    /// Acesso explícito ao valor (somente onde o segredo é realmente usado).
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("***")
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_and_kv_forms_are_redacted() {
        assert_eq!(
            redact_text("GITHUB_TOKEN=ghp_abcdef123456"),
            "GITHUB_TOKEN=***"
        );
        assert_eq!(redact_text("password=hunter2"), "password=***");
        assert_eq!(redact_text("DB_PASS= s3creta"), "DB_PASS= ***");
        assert_eq!(redact_text("api-key: abc123"), "api-key: ***");
    }

    #[test]
    fn json_values_are_redacted_keys_intact() {
        let s = r#"{"username": "ada", "password": "hunter2", "note": "ok"}"#;
        let out = redact_text(s);
        assert!(out.contains("\"username\": \"ada\""), "{out}");
        assert!(out.contains("\"password\": \"***\""), "{out}");
        assert!(out.contains("\"note\": \"ok\""), "{out}");
    }

    #[test]
    fn cli_flags_in_all_forms() {
        assert_eq!(
            redact_text("--token=abc123 --verbose"),
            "--token=*** --verbose"
        );
        assert_eq!(
            redact_text("tool --password abc x"),
            "tool --password *** x"
        );
        assert_eq!(
            redact_text("mysql -u root -pS3creta! db"),
            "mysql -u root -p*** db"
        );
    }

    #[test]
    fn uri_userinfo_collapses() {
        assert_eq!(
            redact_text("postgres://app:sup3r@db.internal:5432/loja"),
            "postgres://***@db.internal:5432/loja"
        );
        // Sem senha (só user) não é redigido: não há segredo.
        assert_eq!(
            redact_text("https://usuario@exemplo.com/x"),
            "https://usuario@exemplo.com/x"
        );
    }

    #[test]
    fn bearer_tokens_are_redacted() {
        assert_eq!(
            redact_text("Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.abc.def"),
            "Authorization: Bearer ***"
        );
    }

    #[test]
    fn non_sensitive_content_survives() {
        let s = "tokenize=abc monkey=banana --tokenizer=nltk path=/opt/app key_name=x";
        assert_eq!(redact_text(s), s);
    }

    #[test]
    fn case_insensitive_and_multiple_occurrences() {
        let s = "TOKEN=a PASSWORD=b SeCrEt=c";
        let out = redact_text(s);
        assert_eq!(out, "TOKEN=*** PASSWORD=*** SeCrEt=***");
    }

    #[test]
    fn adversarial_partial_words_and_quotes() {
        // "tokens" (plural) não casa: exigimos o separador logo após a chave.
        let s = "tokens=keep password=\"p a s s\" 'token'='v'";
        let out = redact_text(s);
        assert!(out.contains("tokens=keep"), "{out}");
        assert!(out.contains("password=\"***\""), "{out}");
        assert!(out.contains("'token'='***'"), "{out}");
    }

    #[test]
    fn secret_wrapper_never_leaks() {
        let s = Secret::new("segredo");
        assert_eq!(format!("{s}"), "***");
        assert_eq!(format!("{s:?}"), "Secret(***)");
        assert_eq!(s.expose(), "segredo");
    }

    #[test]
    fn redaction_is_idempotent() {
        let once = redact_text("token=abc password=def");
        assert_eq!(redact_text(&once), once);
    }
}
