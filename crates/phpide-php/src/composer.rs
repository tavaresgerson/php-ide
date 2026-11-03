//! Descoberta de projeto Composer — T-PHP-030-01/02/03 (PHP-030).
//!
//! Leitura **declarativa** de `composer.json`: constraint de PHP (language
//! level), autoload PSR-0/PSR-4 (source roots), autoload-dev (test roots),
//! presença de `vendor/` e scripts. Nada é executado; configuração manual
//! explícita (settings) nunca é sobrescrita — a descoberta apenas informa.
//!
//! Tratamento de falhas (T-PHP-030-03): `composer.json` ausente ⇒ projeto
//! não-Composer (None, sem erro); inválido ⇒ `Err` com diagnóstico e o
//! projeto segue editável; `vendor/` ausente ⇒ aviso acionável no campo
//! `warnings` (a IDE nunca executa `composer install` sozinha).

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Visão do projeto Composer para a IDE.
#[derive(Debug, Clone, PartialEq)]
pub struct ComposerInfo {
    pub root: PathBuf,
    /// Nome do pacote ("vendor/project"), quando declarado.
    pub name: Option<String>,
    /// Language level inferido da constraint `require.php` (ex.: "8.1").
    /// `None` quando a constraint não permite inferir (ex.: "*").
    pub php_level: Option<String>,
    /// Source roots de autoload PSR-4/PSR-0 (namespace prefix → diretório).
    pub source_roots: Vec<(String, PathBuf)>,
    /// Test roots de autoload-dev PSR-4/PSR-0.
    pub test_roots: Vec<(String, PathBuf)>,
    /// `vendor/` existe no disco?
    pub vendor_present: bool,
    /// Pacotes diretos requeridos (nome → constraint).
    pub packages: Vec<(String, String)>,
    /// Avisos acionáveis (vendor ausente, json inválido não entra aqui —
    /// inválido é erro de descoberta).
    pub warnings: Vec<String>,
}

/// Erro de leitura do composer.json (arquivo existe mas não é utilizável).
#[derive(Debug, Clone, PartialEq)]
pub struct ComposerError {
    pub message: String,
    /// Ação recomendada para o usuário (NFR-005).
    pub action: String,
}

/// Extrai o language level de uma constraint PHP ("^8.1", ">=8.0 <9.0",
/// "8.1.*", "8.1.2"). Pega o menor minor declarado; `None` se não houver
/// versão identificável (ex.: "*", ">=7").
pub fn php_level_from_constraint(constraint: &str) -> Option<String> {
    let c = constraint.trim();
    let mut best: Option<(u32, u32)> = None;
    for token in c.split(|ch: char| !(ch.is_ascii_digit() || ch == '.')) {
        let mut parts = token.split('.');
        let major: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        let has_dot = token.contains('.');
        let minor: u32 = if has_dot {
            parts.next().and_then(|p| p.parse().ok()).unwrap_or(0)
        } else {
            0
        };
        if major == 0 && !token.starts_with('0') {
            continue;
        }
        let candidate = (major, if has_dot { minor } else { 0 });
        best = Some(match best {
            Some(b) if b <= candidate => b,
            _ => candidate,
        });
    }
    best.map(|(major, minor)| format!("{major}.{minor}"))
}

fn psr_roots(json: &Value, section: &str) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for kind in ["psr-4", "psr-0"] {
        if let Some(map) = json[section][kind].as_object() {
            for (prefix, dir) in map {
                let dir = dir.as_str().unwrap_or("").trim_matches('/');
                let path = PathBuf::from(if dir.is_empty() { "." } else { dir });
                out.push((prefix.trim_matches('\\').to_string(), path));
            }
        }
    }
    out
}

/// Descobre o projeto Composer em `root`.
///
/// - Sem `composer.json` ⇒ `Ok(None)` (projeto comum, sem inteligência Composer).
/// - JSON inválido ⇒ `Err(ComposerError)` (ação: corrigir o arquivo).
pub fn discover(root: &Path) -> Result<Option<ComposerInfo>, ComposerError> {
    let file = root.join("composer.json");
    let Ok(text) = fs::read_to_string(&file) else {
        return Ok(None);
    };
    let json: Value = serde_json::from_str(&text).map_err(|e| ComposerError {
        message: format!("composer.json inválido: {e}"),
        action: "corrija a sintaxe do composer.json para reativar a descoberta do projeto"
            .to_string(),
    })?;
    if !json.is_object() {
        return Err(ComposerError {
            message: "composer.json não é um objeto JSON".to_string(),
            action: "corrija a estrutura do composer.json".to_string(),
        });
    }

    let php_level = json["require"]["php"]
        .as_str()
        .and_then(php_level_from_constraint);
    let source_roots = psr_roots(&json, "autoload");
    let test_roots = psr_roots(&json, "autoload-dev");
    let vendor_present = root.join("vendor").is_dir();
    let packages = json["require"]
        .as_object()
        .map(|req| {
            req.iter()
                .filter(|(k, _)| k.as_str() != "php")
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("*").to_string()))
                .collect()
        })
        .unwrap_or_default();
    let name = json["name"].as_str().map(String::from);

    let mut warnings = Vec::new();
    if !vendor_present {
        warnings.push(
            "vendor/ ausente — execute `composer install` para indexar dependências".to_string(),
        );
    }

    Ok(Some(ComposerInfo {
        root: root.to_path_buf(),
        name,
        php_level,
        source_roots,
        test_roots,
        vendor_present,
        packages,
        warnings,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("phpide-composer-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn constraint_parses_to_language_level() {
        assert_eq!(php_level_from_constraint("^8.1").as_deref(), Some("8.1"));
        assert_eq!(
            php_level_from_constraint(">=8.0 <9.0").as_deref(),
            Some("8.0")
        );
        assert_eq!(php_level_from_constraint("8.1.*").as_deref(), Some("8.1"));
        assert_eq!(php_level_from_constraint("7.4|8.0").as_deref(), Some("7.4"));
        assert!(php_level_from_constraint("*").is_none());
        // Sem minor ainda infere o minor 0 (nível mínimo da constraint).
        assert_eq!(php_level_from_constraint(">=7").as_deref(), Some("7.0"));
    }

    #[test]
    fn missing_composer_is_not_an_error() {
        let dir = scratch("missing");
        assert_eq!(discover(&dir).unwrap(), None);
    }

    #[test]
    fn invalid_composer_is_actionable_error() {
        let dir = scratch("invalid");
        fs::write(dir.join("composer.json"), "{ quebrado").unwrap();
        let err = discover(&dir).unwrap_err();
        assert!(err.message.contains("inválido"));
        assert!(err.action.contains("composer.json"));
    }

    #[test]
    fn full_project_maps_roots_level_and_vendor() {
        let dir = scratch("full");
        fs::write(
            dir.join("composer.json"),
            r#"{
                "name": "acme/loja",
                "require": { "php": ">=8.1", "symfony/console": "^6.4" },
                "autoload": { "psr-4": { "Acme\\Loja\\": "src/", "": "lib/" } },
                "autoload-dev": { "psr-4": { "Acme\\Loja\\Tests\\": "tests/" } }
            }"#,
        )
        .unwrap();
        fs::create_dir_all(dir.join("vendor")).unwrap();
        let info = discover(&dir).unwrap().unwrap();
        assert_eq!(info.name.as_deref(), Some("acme/loja"));
        assert_eq!(info.php_level.as_deref(), Some("8.1"));
        assert!(info
            .source_roots
            .iter()
            .any(|(p, d)| p == "Acme\\Loja" && d == Path::new("src")));
        assert!(info
            .test_roots
            .iter()
            .any(|(p, _)| p == "Acme\\Loja\\Tests"));
        assert!(info.vendor_present);
        assert!(info.packages.iter().any(|(n, _)| n == "symfony/console"));
        assert!(info.warnings.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_vendor_produces_actionable_warning() {
        let dir = scratch("novendor");
        fs::write(dir.join("composer.json"), r#"{"require": {"php": "^8.2"}}"#).unwrap();
        let info = discover(&dir).unwrap().unwrap();
        assert!(!info.vendor_present);
        assert!(info.warnings.iter().any(|w| w.contains("composer install")));
        fs::remove_dir_all(&dir).ok();
    }
}
