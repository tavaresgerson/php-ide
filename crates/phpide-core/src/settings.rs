//! Settings versionados com migração incremental e persistência atômica —
//! `T-NFR-004-01` e `T-NFR-004-02` (NFR-004).
//!
//! - O arquivo JSON carrega `settings_version`; campos novos ganham default
//!   e migrações são incrementais (1→2→3…), testáveis com configs antigas.
//! - Configuração inválida gera **erro recuperável**: o arquivo defeituoso é
//!   preservado com a extensão `.invalid` e a IDE segue com defaults.
//! - A escrita é atômica (tmp + fsync + rename, ver `fsutil`) e mantém um
//!   `.bak` da versão anterior.

use crate::error::{Component, ErrorKind, IdeError};
use crate::fsutil::write_atomic;
use serde_json::Value;
use std::fs;
use std::path::Path;

/// Versão atual do formato de settings (bump a cada mudança de schema).
pub const SETTINGS_VERSION: u32 = 2;

/// Migração de uma versão para a seguinte. Deve ser total: campos novos
/// ganham default e campos removidos são ignorados sem erro.
pub struct Migration {
    pub from: u32,
    pub apply: fn(Value) -> Value,
}

/// Cadeia ordenada de migrações incrementais.
pub struct MigrationChain {
    migrations: Vec<Migration>,
}

impl MigrationChain {
    pub fn new(mut migrations: Vec<Migration>) -> Self {
        migrations.sort_by_key(|m| m.from);
        MigrationChain { migrations }
    }

    /// Migra `value` de `version` até [`SETTINGS_VERSION`].
    /// Erro recuperável para versão mais nova que a IDE (ação: atualizar a
    /// IDE ou remover a chave `settings_version`).
    pub fn migrate(&self, mut value: Value, mut version: u32) -> Result<Value, IdeError> {
        if version > SETTINGS_VERSION {
            return Err(IdeError::new(
                Component::Settings,
                "migrate",
                ErrorKind::InvalidConfig,
                format!(
                    "settings versão {version} é mais novo que o suportado ({SETTINGS_VERSION})"
                ),
                "atualize a IDE ou remova a chave settings_version do arquivo",
            )
            .with_detail(format!("arquivo em versão {version}")));
        }
        while version < SETTINGS_VERSION {
            let Some(m) = self.migrations.iter().find(|m| m.from == version) else {
                return Err(IdeError::new(
                    Component::Settings,
                    "migrate",
                    ErrorKind::InvalidConfig,
                    format!("sem migração da versão {version} para {}", version + 1),
                    "restaure um backup de settings.json",
                ));
            };
            value = (m.apply)(value);
            version += 1;
        }
        Ok(value)
    }
}

/// Lê a versão declarada; ausência da chave significa formato v1 (Estágio 5).
pub fn version_of(value: &Value) -> u32 {
    value
        .get("settings_version")
        .and_then(Value::as_u64)
        .map(|v| v as u32)
        .unwrap_or(1)
}

/// Resultado de uma carga: valor (possivelmente migrado) + diagnóstico.
#[derive(Debug)]
pub struct LoadReport {
    pub value: Value,
    /// Versão encontrada no arquivo (1 quando a chave não existia).
    pub from_version: u32,
    /// Migrações aplicadas?
    pub migrated: bool,
    /// Erro recuperável (arquivo inválido, versão futura, E/S): a IDE segue
    /// com o valor deste relatório (defaults) e deve sinalizar ao usuário.
    pub recovered: Option<IdeError>,
}

impl LoadReport {
    pub fn is_recovered(&self) -> bool {
        self.recovered.is_some()
    }
}

/// Caminho de backup do arquivo defeituoso (preservado, nunca sobrescrito).
fn invalid_path(path: &Path) -> std::path::PathBuf {
    path.with_extension("json.invalid")
}

/// Caminho do backup da última versão válida.
fn bak_path(path: &Path) -> std::path::PathBuf {
    path.with_extension("json.bak")
}

/// Carrega settings versionados de `path`.
///
/// - Arquivo ausente → valor vazio na versão atual, sem erro.
/// - JSON inválido → o arquivo é movido para `<nome>.invalid` (preservado),
///   defaults são usados e o erro recuperável vai em `recovered`.
/// - Versão antiga → migrações incrementais aplicadas.
/// - Versão futura → erro recuperável (valor não migrado; defaults do
///   chamador devem ser usados conforme a política dele).
pub fn load(path: &Path, chain: &MigrationChain) -> LoadReport {
    let empty = || serde_json::json!({});
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return LoadReport {
                value: empty(),
                from_version: SETTINGS_VERSION,
                migrated: false,
                recovered: None,
            }
        }
        Err(e) => {
            return LoadReport {
                value: empty(),
                from_version: SETTINGS_VERSION,
                migrated: false,
                recovered: Some(IdeError::from_io(
                    Component::Settings,
                    "load",
                    &path.display().to_string(),
                    &e,
                )),
            }
        }
    };
    let parsed = match serde_json::from_str::<Value>(&text) {
        Ok(v) => v,
        Err(e) => {
            // Preserva o arquivo defeituoso antes de seguir com defaults.
            let preserved = invalid_path(path);
            let _ = fs::rename(path, &preserved);
            return LoadReport {
                value: empty(),
                from_version: SETTINGS_VERSION,
                migrated: false,
                recovered: Some(
                    IdeError::new(
                        Component::Settings,
                        "parse",
                        ErrorKind::InvalidConfig,
                        "settings.json inválido; configuração padrão em uso",
                        format!(
                            "corrija ou remova {} (original preservado)",
                            preserved.display()
                        ),
                    )
                    .with_detail(format!("serde: {e}")),
                ),
            };
        }
    };
    let from = version_of(&parsed);
    match chain.migrate(parsed, from) {
        Ok(value) => LoadReport {
            migrated: from != SETTINGS_VERSION,
            value,
            from_version: from,
            recovered: None,
        },
        Err(e) => LoadReport {
            value: empty(),
            from_version: from,
            migrated: false,
            recovered: Some(e),
        },
    }
}

/// Grava settings de forma atômica, versionando o documento e mantendo
/// `.bak` da versão anterior.
pub fn save(path: &Path, value: &Value) -> Result<(), IdeError> {
    let mut v = value.clone();
    v["settings_version"] = Value::from(SETTINGS_VERSION);
    let bytes = serde_json::to_vec_pretty(&v).map_err(|e| {
        IdeError::new(
            Component::Settings,
            "serialize",
            ErrorKind::InvalidInput,
            "configuração não serializável",
            "verifique o log e tente novamente",
        )
        .with_detail(format!("serde: {e}"))
    })?;
    if path.exists() {
        // Backup best effort da versão anterior.
        let _ = fs::copy(path, bak_path(path));
    }
    write_atomic(path, &bytes).map_err(|e| {
        IdeError::from_io(Component::Settings, "save", &path.display().to_string(), &e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("phpide-settings-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Cadeia de teste: v1 (Estágio 5: theme dark/light) → v2 (atual).
    fn test_chain() -> MigrationChain {
        MigrationChain::new(vec![Migration {
            from: 1,
            apply: |mut v| {
                if let Some(t) = v.get("theme").and_then(Value::as_str) {
                    let mapped = match t {
                        "dark" => "islands-dark",
                        "light" => "islands-light",
                        other => other,
                    };
                    v["theme"] = Value::from(mapped);
                }
                v
            },
        }])
    }

    #[test]
    fn old_config_migrates_incrementally() {
        let dir = scratch("migrate");
        let file = dir.join("settings.json");
        fs::write(&file, r#"{"theme": "dark", "zoom": 1.2}"#).unwrap();
        let report = load(&file, &test_chain());
        assert!(!report.is_recovered());
        assert_eq!(report.from_version, 1);
        assert!(report.migrated);
        assert_eq!(report.value["theme"], "islands-dark");
        assert_eq!(report.value["zoom"], 1.2);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn current_config_roundtrips_without_migration() {
        let dir = scratch("roundtrip");
        let file = dir.join("settings.json");
        save(&file, &serde_json::json!({"theme": "darcula", "zoom": 1.5})).unwrap();
        let report = load(&file, &test_chain());
        assert!(!report.is_recovered());
        assert!(!report.migrated);
        assert_eq!(report.from_version, SETTINGS_VERSION);
        assert_eq!(report.value["theme"], "darcula");
        assert_eq!(report.value["settings_version"], SETTINGS_VERSION);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn invalid_json_is_preserved_and_recoverable() {
        let dir = scratch("invalid");
        let file = dir.join("settings.json");
        fs::write(&file, "{ isso não é json").unwrap();
        let report = load(&file, &test_chain());
        assert!(report.is_recovered());
        let err = report.recovered.unwrap();
        assert_eq!(err.kind, ErrorKind::InvalidConfig);
        assert!(err.action.contains("preservado"));
        assert!(
            report.value.as_object().unwrap().is_empty(),
            "defaults em uso"
        );
        let preserved = dir.join("settings.json.invalid");
        assert!(preserved.exists(), "arquivo original preservado");
        assert_eq!(fs::read_to_string(&preserved).unwrap(), "{ isso não é json");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn future_version_is_recoverable_error() {
        let dir = scratch("future");
        let file = dir.join("settings.json");
        fs::write(&file, r#"{"settings_version": 999, "theme": "x"}"#).unwrap();
        let report = load(&file, &test_chain());
        assert!(report.is_recovered());
        let err = report.recovered.unwrap();
        assert_eq!(err.kind, ErrorKind::InvalidConfig);
        assert!(err.message.contains("999"));
        // O arquivo não foi destruído (o usuário pode instalá-lo numa versão nova).
        assert!(file.exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_file_loads_defaults_cleanly() {
        let dir = scratch("missing");
        let report = load(&dir.join("nope.json"), &test_chain());
        assert!(!report.is_recovered());
        assert_eq!(report.value, serde_json::json!({}));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_keeps_previous_as_bak() {
        let dir = scratch("bak");
        let file = dir.join("settings.json");
        save(&file, &serde_json::json!({"theme": "light"})).unwrap();
        save(&file, &serde_json::json!({"theme": "darcula"})).unwrap();
        let bak = bak_path(&file);
        assert!(bak.exists(), "backup da versão anterior");
        assert_eq!(
            serde_json::from_str::<Value>(&fs::read_to_string(&bak).unwrap()).unwrap()["theme"],
            "light"
        );
        assert_eq!(
            serde_json::from_str::<Value>(&fs::read_to_string(&file).unwrap()).unwrap()["theme"],
            "darcula"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn migration_gap_reports_recoverable_error() {
        let dir = scratch("gap");
        let file = dir.join("settings.json");
        // Cadeia vazia: qualquer versão antiga não tem para onde migrar.
        fs::write(&file, r#"{"settings_version": 1}"#).unwrap();
        let report = load(&file, &MigrationChain::new(vec![]));
        assert!(report.is_recovered());
        assert_eq!(report.recovered.unwrap().kind, ErrorKind::InvalidConfig);
        fs::remove_dir_all(&dir).ok();
    }
}
