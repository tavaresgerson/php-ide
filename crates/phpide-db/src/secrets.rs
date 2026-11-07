//! Segredos e data sources — `T-NFR-004-03/04/05`, `T-DB-002-01/02/03`.
//!
//! - **NFR-004-03**: configurações de *projeto* (`.phpide/datasources.json`)
//!   separadas das *globais* (`~/.config/phpide/` via config_dir) e dos
//!   *segredos* (`~/.local/share/phpide/secrets`, modo 0600). Segredos nunca
//!   vivem no diretório do projeto.
//! - **NFR-004-04**: porta [`SecretStore`] com implementação por **keyring**
//!   (`secret-tool`, quando disponível) e **fallback explícito** para arquivo
//!   0600 — a UI deve mostrar qual está ativa.
//! - **NFR-004-05**: serialização de `DataSource` nunca inclui a senha —
//!   só `secret_ref`; logs e exportação recebem `redact` por padrão.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use phpide_core::{Component, ErrorKind, IdeError};
use serde::{Deserialize, Serialize};

use crate::contracts::DriverKind;

// ---------------------------------------------------------------------------
// Segredos (porta + keyring + fallback explícito)
// ---------------------------------------------------------------------------

/// Porta de armazenamento de segredos (NFR-004-04).
pub trait SecretStore: Send + Sync {
    /// Nome da implementação ativa ("keyring" | "arquivo-local").
    fn backend_name(&self) -> &'static str;
    fn set(&mut self, key: &str, value: &str) -> Result<(), IdeError>;
    fn get(&self, key: &str) -> Option<String>;
    fn delete(&mut self, key: &str);
}

/// Keyring real via `secret-tool` (libsecret), quando presente no sistema.
pub struct KeyringStore;

impl SecretStore for KeyringStore {
    fn backend_name(&self) -> &'static str {
        "keyring (secret-tool)"
    }
    fn set(&mut self, key: &str, value: &str) -> Result<(), IdeError> {
        let st = std::process::Command::new("secret-tool")
            .args(["store", "--label=phpide", "chave", key])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| io_err("secret-tool store", e))?;
        use std::io::Write;
        let mut stdin = st.stdin.unwrap();
        stdin.write_all(value.as_bytes()).ok();
        drop(stdin);
        Ok(())
    }
    fn get(&self, key: &str) -> Option<String> {
        let out = std::process::Command::new("secret-tool")
            .args(["lookup", "chave", key])
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
    }
    fn delete(&mut self, key: &str) {
        let _ = std::process::Command::new("secret-tool")
            .args(["clear", "chave", key])
            .output();
    }
}

/// Fallback explícito: arquivo JSON 0600 no diretório de dados do usuário
/// (fora do projeto, fora de qualquer exportação versionável).
pub struct FileKeyring {
    path: PathBuf,
    map: BTreeMap<String, String>,
}

impl FileKeyring {
    pub fn open() -> Result<Self, IdeError> {
        let dir = data_dir();
        std::fs::create_dir_all(&dir).map_err(|e| io_err("segredos mkdir", e))?;
        let path = dir.join("secrets.json");
        let map = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        // Garante permissão restrita na primeira criação.
        if !path.exists() {
            std::fs::write(&path, b"{}").map_err(|e| io_err("segredos create", e))?;
            set_0600(&path);
        }
        Ok(FileKeyring { path, map })
    }
}

impl SecretStore for FileKeyring {
    fn backend_name(&self) -> &'static str {
        "arquivo-local (0600, fallback explícito)"
    }
    fn set(&mut self, key: &str, value: &str) -> Result<(), IdeError> {
        self.map.insert(key.to_string(), value.to_string());
        self.persist()
    }
    fn get(&self, key: &str) -> Option<String> {
        self.map.get(key).cloned()
    }
    fn delete(&mut self, key: &str) {
        self.map.remove(key);
        let _ = self.persist();
    }
}

impl FileKeyring {
    fn persist(&self) -> Result<(), IdeError> {
        std::fs::write(
            &self.path,
            serde_json::to_string(&self.map).unwrap_or_default(),
        )
        .map_err(|e| io_err("segredos persist", e))?;
        set_0600(&self.path);
        Ok(())
    }
}

#[cfg(unix)]
fn set_0600(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut p = meta.permissions();
        p.set_mode(0o600);
        let _ = std::fs::set_permissions(path, p);
    }
}
#[cfg(not(unix))]
fn set_0600(_path: &Path) {}

fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".local/share")
        })
        .join("phpide")
}

/// Escolhe a melhor implementação disponível (keyring > fallback explícito).
pub fn open_secret_store() -> Box<dyn SecretStore> {
    if std::process::Command::new("secret-tool")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        Box::new(KeyringStore)
    } else {
        match FileKeyring::open() {
            Ok(k) => Box::new(k),
            Err(_) => Box::new(FileKeyring {
                path: PathBuf::new(),
                map: Default::default(),
            }),
        }
    }
}

fn io_err(op: &str, e: std::io::Error) -> IdeError {
    IdeError::new(
        Component::Core,
        op.to_string(),
        ErrorKind::Io,
        format!("falha de E/S em {op}"),
        "verifique permissões do armazenamento de segredos",
    )
    .with_detail(e.to_string())
}

// ---------------------------------------------------------------------------
// Data sources (CRUD — DB-002-01; credenciais — DB-002-02)
// ---------------------------------------------------------------------------

/// Uma conexão configurada. **Nunca** serializa senha (NFR-004-05): só a
/// referência ao segredo.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DataSource {
    pub id: String,
    pub name: String,
    pub driver: String,
    /// URL sem credenciais — as credenciais vivem no SecretStore sob
    /// `secret_ref` (DB-002-02).
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_ref: Option<String>,
}

impl DataSource {
    pub fn new(name: &str, driver: DriverKind, url: &str) -> Self {
        DataSource {
            id: format!("ds-{}", std::process::id()),
            name: name.to_string(),
            driver: driver.name().to_string(),
            url: url.to_string(),
            secret_ref: None,
        }
    }
}

/// Resolve a URL efetiva (credenciais injetadas do SecretStore, em memória).
/// O valor retornado **não** deve ser logado nem serializado.
pub fn resolve_url(ds: &DataSource, secrets: &dyn SecretStore) -> Result<String, IdeError> {
    match ds.secret_ref.as_deref() {
        None => Ok(ds.url.clone()),
        Some(key) => {
            let cred = secrets.get(key).ok_or_else(|| {
                IdeError::new(
                    Component::Core,
                    "datasource.resolve",
                    ErrorKind::NotFound,
                    format!("segredo ausente para '{}'", ds.name),
                    "configure a credencial novamente em Settings",
                )
            })?;
            // Injeta user:pass@ após o esquema (postgresql/mysql).
            if let Some((scheme, rest)) = ds.url.split_once("://") {
                if rest.contains('@') {
                    return Ok(ds.url.clone());
                }
                Ok(format!("{scheme}://{cred}@{rest}"))
            } else {
                Ok(ds.url.clone())
            }
        }
    }
}

/// Redação para logs/exportação (NFR-004-05): nunca inclui o segredo.
pub fn redact(ds: &DataSource) -> String {
    format!(
        "DataSource({}, driver={}, url={}, secret={})",
        ds.name,
        ds.driver,
        ds.url,
        if ds.secret_ref.is_some() {
            "<armazenado>"
        } else {
            "nenhum"
        }
    )
}

/// Repositório de data sources do **projeto** (NFR-004-03):
/// `.phpide/datasources.json` — versionável **sem segredos**.
pub struct DataSourceStore {
    path: PathBuf,
    pub list: Vec<DataSource>,
}

impl DataSourceStore {
    pub fn for_project(root: &Path) -> Self {
        let path = root.join(".phpide/datasources.json");
        let list = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        DataSourceStore { path, list }
    }

    pub fn save(&mut self, ds: DataSource) -> Result<(), IdeError> {
        if let Some(existing) = self.list.iter_mut().find(|d| d.id == ds.id) {
            *existing = ds;
        } else {
            self.list.push(ds);
        }
        self.persist()
    }

    pub fn remove(&mut self, id: &str) -> Result<(), IdeError> {
        self.list.retain(|d| d.id != id);
        self.persist()
    }

    fn persist(&self) -> Result<(), IdeError> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| io_err("datasources mkdir", e))?;
        }
        let json = serde_json::to_string_pretty(&self.list).unwrap_or_default();
        phpide_core::fsutil::write_atomic(&self.path, json.as_bytes())
            .map_err(|e| io_err("datasources persist", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MemSecrets(BTreeMap<String, String>);
    impl SecretStore for MemSecrets {
        fn backend_name(&self) -> &'static str {
            "mem"
        }
        fn set(&mut self, k: &str, v: &str) -> Result<(), IdeError> {
            self.0.insert(k.into(), v.into());
            Ok(())
        }
        fn get(&self, k: &str) -> Option<String> {
            self.0.get(k).cloned()
        }
        fn delete(&mut self, k: &str) {
            self.0.remove(k);
        }
    }

    #[test]
    fn datasource_never_serializes_password() {
        let mut ds = DataSource::new(
            "pg",
            DriverKind::PostgreSql,
            "postgresql://localhost:5432/app",
        );
        ds.secret_ref = Some("pg-main".into());
        let json = serde_json::to_string(&ds).unwrap();
        assert!(!json.contains("pw"), "segredo fora da serialização: {json}");
        assert!(json.contains("secret_ref"));
        // Redação não vaza credencial.
        let mut sec = MemSecrets(BTreeMap::new());
        sec.set("pg-main", "user:pw").unwrap();
        let url = resolve_url(&ds, &sec).unwrap();
        assert_eq!(url, "postgresql://user:pw@localhost:5432/app");
        let red = redact(&ds);
        assert!(!red.contains("user:pw"), "redact vaza: {red}");
    }

    #[test]
    fn file_keyring_roundtrip_0600() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_DATA_HOME", dir.path());
        let mut k = FileKeyring::open().unwrap();
        assert!(k.backend_name().contains("fallback"));
        k.set("x", "y").unwrap();
        assert_eq!(k.get("x").as_deref(), Some("y"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&k.path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "permissão restrita");
        }
        k.delete("x");
        assert!(k.get("x").is_none());
        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn store_crud_project_file_without_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = DataSourceStore::for_project(dir.path());
        let mut ds = DataSource::new("local", DriverKind::Sqlite, "sqlite:///tmp/x.db");
        ds.id = "ds-1".into();
        ds.secret_ref = Some("k".into());
        store.save(ds.clone()).unwrap();
        assert_eq!(store.list.len(), 1);
        // Recarrega do disco: intacto, sem segredo.
        let reloaded = DataSourceStore::for_project(dir.path());
        assert_eq!(reloaded.list[0].id, "ds-1");
        assert_eq!(reloaded.list[0].secret_ref.as_deref(), Some("k"));
        assert!(!reloaded.list[0].url.contains("pw"));
        store.remove("ds-1").unwrap();
        assert!(DataSourceStore::for_project(dir.path()).list.is_empty());
    }

    #[test]
    fn resolve_fails_when_secret_missing() {
        let ds = DataSource {
            secret_ref: Some("nope".into()),
            ..DataSource::new("x", DriverKind::MySql, "mysql://h/db")
        };
        let sec = MemSecrets(BTreeMap::new());
        let err = resolve_url(&ds, &sec).unwrap_err();
        assert_eq!(err.kind, ErrorKind::NotFound);
    }
}
