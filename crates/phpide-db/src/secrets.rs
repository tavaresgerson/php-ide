use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use phpide_core::{Component, ErrorKind, IdeError};
use serde::{Deserialize, Serialize};

use crate::contracts::DriverKind;

pub trait SecretStore: Send + Sync {
    fn backend_name(&self) -> &'static str;
    fn set(&mut self, key: &str, value: &str) -> Result<(), IdeError>;
    fn get(&self, key: &str) -> Result<Option<String>, IdeError>;
    fn delete(&mut self, key: &str) -> Result<(), IdeError>;
}

pub struct KeyringStore {
    program: PathBuf,
}

impl Default for KeyringStore {
    fn default() -> Self {
        Self {
            program: PathBuf::from("secret-tool"),
        }
    }
}

impl KeyringStore {
    pub fn with_program(program: PathBuf) -> Self {
        Self { program }
    }

    fn command(&self, operation: &str, key: &str) -> std::process::Command {
        let mut command = std::process::Command::new(&self.program);
        command.args([operation, "chave", key]);
        command
    }
}

impl SecretStore for KeyringStore {
    fn backend_name(&self) -> &'static str {
        "keyring (secret-tool)"
    }

    fn set(&mut self, key: &str, value: &str) -> Result<(), IdeError> {
        let mut child = self
            .command("store", key)
            .arg("--label=phpide")
            .stdin(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| io_err("secret-tool store", error))?;
        {
            use std::io::Write;
            let mut stdin = child.stdin.take().ok_or_else(|| {
                io_err(
                    "secret-tool stdin",
                    std::io::Error::new(std::io::ErrorKind::BrokenPipe, "stdin indisponível"),
                )
            })?;
            stdin
                .write_all(value.as_bytes())
                .map_err(|error| io_err("secret-tool write", error))?;
        }
        let output = child
            .wait_with_output()
            .map_err(|error| io_err("secret-tool wait", error))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(IdeError::new(
                Component::Db,
                "secret-tool store",
                ErrorKind::Other,
                "keyring recusou a gravação do segredo",
                "desbloqueie o keyring e tente novamente; o segredo não foi confirmado como salvo",
            )
            .with_detail(format!(
                "status={:?}, stderr={}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr)
            )))
        }
    }

    fn get(&self, key: &str) -> Result<Option<String>, IdeError> {
        let output = self
            .command("lookup", key)
            .output()
            .map_err(|error| io_err("secret-tool lookup", error))?;
        if !output.status.success() {
            return Err(IdeError::new(
                Component::Db,
                "secret-tool lookup",
                ErrorKind::Other,
                "falha ao consultar o keyring",
                "desbloqueie o keyring ou configure a credencial novamente",
            )
            .with_detail(String::from_utf8_lossy(&output.stderr)));
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string()
            .into())
    }

    fn delete(&mut self, key: &str) -> Result<(), IdeError> {
        let output = self
            .command("clear", key)
            .output()
            .map_err(|error| io_err("secret-tool clear", error))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(IdeError::new(
                Component::Db,
                "secret-tool clear",
                ErrorKind::Other,
                "keyring recusou a remoção do segredo",
                "verifique o estado do keyring",
            )
            .with_detail(String::from_utf8_lossy(&output.stderr)))
        }
    }
}

pub struct FileKeyring {
    path: PathBuf,
    map: BTreeMap<String, String>,
}

impl FileKeyring {
    pub fn open() -> Result<Self, IdeError> {
        Self::open_at(data_dir()?.join("secrets.json"))
    }

    pub fn open_at(path: PathBuf) -> Result<Self, IdeError> {
        let dir = path.parent().ok_or_else(|| {
            io_err(
                "segredos path",
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "caminho sem diretório"),
            )
        })?;
        std::fs::create_dir_all(dir).map_err(|error| io_err("segredos mkdir", error))?;
        let map = if path.exists() {
            let json =
                std::fs::read_to_string(&path).map_err(|error| io_err("segredos read", error))?;
            match serde_json::from_str(&json) {
                Ok(map) => map,
                Err(error) => {
                    let backup = path.with_extension("invalid");
                    let _ = std::fs::copy(&path, backup);
                    return Err(IdeError::new(
                        Component::Db,
                        "segredos parse",
                        ErrorKind::InvalidConfig,
                        "armazenamento local de segredos contém JSON inválido",
                        "revise o arquivo preservado e autorize sua recriação; nada foi sobrescrito",
                    )
                    .with_detail(error.to_string()));
                }
            }
        } else {
            BTreeMap::new()
        };
        set_0600(&path).map_err(|error| io_err("segredos chmod", error))?;
        Ok(Self { path, map })
    }

    fn persist(&self) -> Result<(), IdeError> {
        let json = serde_json::to_vec(&self.map).map_err(|error| {
            IdeError::new(
                Component::Db,
                "segredos serialize",
                ErrorKind::Other,
                "falha ao serializar segredos",
                "tente novamente sem alterar o armazenamento anterior",
            )
            .with_detail(error.to_string())
        })?;
        phpide_core::fsutil::write_atomic(&self.path, &json)
            .map_err(|error| io_err("segredos persist", error))?;
        set_0600(&self.path).map_err(|error| io_err("segredos chmod", error))
    }
}

impl SecretStore for FileKeyring {
    fn backend_name(&self) -> &'static str {
        "arquivo-local (0600, fallback explícito)"
    }

    fn set(&mut self, key: &str, value: &str) -> Result<(), IdeError> {
        let previous = self.map.insert(key.to_string(), value.to_string());
        if let Err(error) = self.persist() {
            match previous {
                Some(previous) => self.map.insert(key.to_string(), previous),
                None => self.map.remove(key),
            };
            return Err(error);
        }
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<String>, IdeError> {
        Ok(self.map.get(key).cloned())
    }

    fn delete(&mut self, key: &str) -> Result<(), IdeError> {
        let previous = self.map.remove(key);
        if let Err(error) = self.persist() {
            if let Some(previous) = previous {
                self.map.insert(key.to_string(), previous);
            }
            return Err(error);
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct SessionOnly {
    map: BTreeMap<String, String>,
    pub reason: Option<String>,
}

impl SessionOnly {
    pub fn with_reason(reason: String) -> Self {
        Self {
            map: BTreeMap::new(),
            reason: Some(reason),
        }
    }
}

impl SecretStore for SessionOnly {
    fn backend_name(&self) -> &'static str {
        "session-only (segredos NÃO serão persistidos)"
    }

    fn set(&mut self, key: &str, value: &str) -> Result<(), IdeError> {
        self.map.insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<String>, IdeError> {
        Ok(self.map.get(key).cloned())
    }

    fn delete(&mut self, key: &str) -> Result<(), IdeError> {
        self.map.remove(key);
        Ok(())
    }
}

#[cfg(unix)]
fn set_0600(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if path.exists() {
        let mut permissions = std::fs::metadata(path)?.permissions();
        permissions.set_mode(0o600);
        std::fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_0600(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

fn data_dir() -> Result<PathBuf, IdeError> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| {
            io_err(
                "segredos data_dir",
                std::io::Error::new(std::io::ErrorKind::NotFound, "HOME/XDG_DATA_HOME ausentes"),
            )
        })?;
    Ok(base.join("phpide"))
}

pub fn open_secret_store() -> Box<dyn SecretStore> {
    if std::process::Command::new("secret-tool")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
    {
        Box::new(KeyringStore::default())
    } else {
        match FileKeyring::open() {
            Ok(store) => Box::new(store),
            Err(error) => Box::new(SessionOnly::with_reason(error.log_line())),
        }
    }
}

fn io_err(operation: &str, error: std::io::Error) -> IdeError {
    let kind = match error.kind() {
        std::io::ErrorKind::PermissionDenied => ErrorKind::PermissionDenied,
        std::io::ErrorKind::NotFound => ErrorKind::NotFound,
        _ => ErrorKind::Io,
    };
    IdeError::new(
        Component::Db,
        operation.to_string(),
        kind,
        format!("falha de E/S em {operation}"),
        "verifique permissões e espaço do armazenamento de segredos",
    )
    .with_detail(error.to_string())
}

static NEXT_DATASOURCE_ID: AtomicU64 = AtomicU64::new(1);
static DATASOURCE_WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct DataSource {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_ref: Option<String>,
}

impl DataSource {
    pub fn new(name: &str, driver: DriverKind, url: &str) -> Result<Self, IdeError> {
        validate_url_without_credentials(driver, url)?;
        let sequence = NEXT_DATASOURCE_ID.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Ok(Self {
            id: format!("ds-{}-{nanos}-{sequence}", std::process::id()),
            name: name.to_string(),
            driver: driver.name().to_string(),
            url: url.to_string(),
            secret_ref: None,
        })
    }
}

impl Serialize for DataSource {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize)]
        struct SafeDataSource<'a> {
            id: &'a str,
            name: &'a str,
            driver: &'a str,
            url: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            secret_ref: &'a Option<String>,
        }
        let kind = DriverKind::from_name(&self.driver).ok_or_else(|| {
            serde::ser::Error::custom(format!("driver desconhecido: {}", self.driver))
        })?;
        validate_url_without_credentials(kind, &self.url).map_err(serde::ser::Error::custom)?;
        SafeDataSource {
            id: &self.id,
            name: &self.name,
            driver: &self.driver,
            url: &self.url,
            secret_ref: &self.secret_ref,
        }
        .serialize(serializer)
    }
}

fn validate_url_without_credentials(kind: DriverKind, url: &str) -> Result<(), IdeError> {
    let has_userinfo = matches!(kind, DriverKind::PostgreSql | DriverKind::MySql)
        && url
            .split_once("://")
            .map(|(_, rest)| rest.split('/').next().unwrap_or(rest).contains('@'))
            .unwrap_or(false);
    if has_userinfo {
        Err(IdeError::new(
            Component::Db,
            "datasource.credentials",
            ErrorKind::InvalidInput,
            "URL com credenciais não é permitida",
            "remova user/password da URL e configure secret_ref no armazenamento seguro",
        ))
    } else {
        Ok(())
    }
}

pub fn resolve_url(ds: &DataSource, secrets: &dyn SecretStore) -> Result<String, IdeError> {
    let kind = DriverKind::from_name(&ds.driver).ok_or_else(|| {
        IdeError::new(
            Component::Db,
            "datasource.driver",
            ErrorKind::InvalidInput,
            "driver desconhecido",
            "selecione um driver suportado",
        )
    })?;
    validate_url_without_credentials(kind, &ds.url)?;
    match ds.secret_ref.as_deref() {
        None => Ok(ds.url.clone()),
        Some(key) => {
            let credential = secrets.get(key)?.ok_or_else(|| {
                IdeError::new(
                    Component::Db,
                    "datasource.resolve",
                    ErrorKind::NotFound,
                    format!("segredo ausente para '{}'", ds.name),
                    "configure a credencial novamente em Settings",
                )
            })?;
            if let Some((scheme, rest)) = ds.url.split_once("://") {
                Ok(format!("{scheme}://{credential}@{rest}"))
            } else {
                Ok(ds.url.clone())
            }
        }
    }
}

pub fn redact(ds: &DataSource) -> String {
    let safe_url = ds
        .url
        .split_once("://")
        .map(|(scheme, rest)| {
            let rest = rest
                .split_once('@')
                .map(|(_, host)| format!("<credenciais-removidas>@{host}"))
                .unwrap_or_else(|| rest.to_string());
            format!("{scheme}://{rest}")
        })
        .unwrap_or_else(|| ds.url.clone());
    format!(
        "DataSource({}, driver={}, url={}, secret={})",
        ds.name,
        ds.driver,
        safe_url,
        if ds.secret_ref.is_some() {
            "<armazenado>"
        } else {
            "nenhum"
        }
    )
}

pub struct DataSourceStore {
    path: PathBuf,
    pub list: Vec<DataSource>,
    pub load_error: Option<String>,
}

impl DataSourceStore {
    pub fn for_project(root: &Path) -> Self {
        let path = root.join(".phpide/datasources.json");
        let mut load_error = None;
        let list = match std::fs::read_to_string(&path) {
            Ok(json) => match serde_json::from_str(&json) {
                Ok(list) => list,
                Err(error) => {
                    load_error = Some(error.to_string());
                    Vec::new()
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                load_error = Some(error.to_string());
                Vec::new()
            }
        };
        Self {
            path,
            list,
            load_error,
        }
    }

    fn read_current(&self) -> Result<Vec<DataSource>, IdeError> {
        if let Some(error) = &self.load_error {
            return Err(IdeError::new(
                Component::Db,
                "datasources reload",
                ErrorKind::InvalidConfig,
                "configuração de data sources inválida; gravação bloqueada",
                "corrija o arquivo manualmente antes de salvar",
            )
            .with_detail(error.clone()));
        }
        match std::fs::read_to_string(&self.path) {
            Ok(json) => serde_json::from_str(&json).map_err(|error| {
                IdeError::new(
                    Component::Db,
                    "datasources parse",
                    ErrorKind::InvalidConfig,
                    "data sources contém JSON inválido",
                    "corrija o arquivo antes de salvar; nada foi substituído",
                )
                .with_detail(error.to_string())
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(io_err("datasources read", error)),
        }
    }

    pub fn save(&mut self, ds: DataSource) -> Result<(), IdeError> {
        let kind = DriverKind::from_name(&ds.driver).ok_or_else(|| {
            IdeError::new(
                Component::Db,
                "datasource.driver",
                ErrorKind::InvalidInput,
                "driver desconhecido",
                "selecione um driver suportado",
            )
        })?;
        validate_url_without_credentials(kind, &ds.url)?;
        let _guard = DATASOURCE_WRITE_LOCK.lock().unwrap();
        let mut current = self.read_current()?;
        if let Some(existing) = current.iter_mut().find(|item| item.id == ds.id) {
            *existing = ds;
        } else {
            current.push(ds);
        }
        self.persist_list(&current)?;
        self.list = current;
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> Result<(), IdeError> {
        let _guard = DATASOURCE_WRITE_LOCK.lock().unwrap();
        let mut current = self.read_current()?;
        current.retain(|item| item.id != id);
        self.persist_list(&current)?;
        self.list = current;
        Ok(())
    }

    fn persist_list(&self, list: &[DataSource]) -> Result<(), IdeError> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|error| io_err("datasources mkdir", error))?;
        }
        let json = serde_json::to_vec_pretty(list).map_err(|error| {
            IdeError::new(
                Component::Db,
                "datasources serialize",
                ErrorKind::InvalidInput,
                "data source inseguro ou inválido",
                "remova credenciais da URL e tente novamente",
            )
            .with_detail(error.to_string())
        })?;
        phpide_core::fsutil::write_atomic(&self.path, &json)
            .map_err(|error| io_err("datasources persist", error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MemSecrets(BTreeMap<String, String>);

    impl SecretStore for MemSecrets {
        fn backend_name(&self) -> &'static str {
            "mem"
        }
        fn set(&mut self, key: &str, value: &str) -> Result<(), IdeError> {
            self.0.insert(key.into(), value.into());
            Ok(())
        }
        fn get(&self, key: &str) -> Result<Option<String>, IdeError> {
            Ok(self.0.get(key).cloned())
        }
        fn delete(&mut self, key: &str) -> Result<(), IdeError> {
            self.0.remove(key);
            Ok(())
        }
    }

    #[test]
    fn datasource_never_serializes_or_resolves_inline_password() {
        assert!(DataSource::new(
            "pg",
            DriverKind::PostgreSql,
            "postgresql://user:pw@localhost/app"
        )
        .is_err());
        let mut ds = DataSource::new(
            "pg",
            DriverKind::PostgreSql,
            "postgresql://localhost:5432/app",
        )
        .unwrap();
        ds.secret_ref = Some("pg-main".into());
        let json = serde_json::to_string(&ds).unwrap();
        assert!(json.contains("secret_ref"));
        let mut secrets = MemSecrets::default();
        secrets.set("pg-main", "user:pw").unwrap();
        assert_eq!(
            resolve_url(&ds, &secrets).unwrap(),
            "postgresql://user:pw@localhost:5432/app"
        );
        assert!(!redact(&ds).contains("user:pw"));
        let unsafe_ds = DataSource {
            url: "postgresql://user:pw@localhost/app".into(),
            ..DataSource::new(
                "unsafe",
                DriverKind::PostgreSql,
                "postgresql://localhost/app",
            )
            .unwrap()
        };
        assert!(serde_json::to_string(&unsafe_ds).is_err());
        assert!(!redact(&unsafe_ds).contains("user:pw"));
    }

    #[test]
    fn ids_are_unique_within_process() {
        let ids: Vec<String> = (0..100)
            .map(|_| {
                DataSource::new("local", DriverKind::Sqlite, "sqlite:///tmp/x.db")
                    .unwrap()
                    .id
            })
            .collect();
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len());
    }

    #[test]
    fn file_keyring_roundtrip_invalid_json_and_failed_persist() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        let mut store = FileKeyring::open_at(path.clone()).unwrap();
        store.set("x", "y").unwrap();
        assert_eq!(store.get("x").unwrap().as_deref(), Some("y"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        store.delete("x").unwrap();

        std::fs::write(&path, "{invalid").unwrap();
        assert!(FileKeyring::open_at(path.clone()).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{invalid");
        assert!(path.with_extension("invalid").exists());

        let mut store = FileKeyring::open_at(path.clone()).unwrap_or_else(|_| {
            std::fs::write(&path, "{}").unwrap();
            FileKeyring::open_at(path.clone()).unwrap()
        });
        std::fs::write(&path, "{}").unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(store.set("x", "not-saved").is_err());
        assert_eq!(store.get("x").unwrap(), None);
    }

    #[test]
    fn keyring_checks_exit_status_and_waits() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("secret-tool");
        std::fs::write(
            &program,
            "#!/bin/sh\ncat >/dev/null\necho 'backend recusou' >&2\nexit 7\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut store = KeyringStore::with_program(program);
        let error = store.set("key", "value").unwrap_err();
        assert!(error.detail.unwrap().contains("backend recusou"));
    }

    #[test]
    fn store_crud_merges_concurrent_instances_and_never_writes_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let mut first = DataSourceStore::for_project(dir.path());
        let mut second = DataSourceStore::for_project(dir.path());
        let a = DataSource::new("a", DriverKind::Sqlite, "sqlite:///tmp/a.db").unwrap();
        let b = DataSource::new("b", DriverKind::Sqlite, "sqlite:///tmp/b.db").unwrap();
        first.save(a.clone()).unwrap();
        second.save(b.clone()).unwrap();
        let reloaded = DataSourceStore::for_project(dir.path());
        assert_eq!(reloaded.list.len(), 2);
        first.remove(&a.id).unwrap();
        let reloaded = DataSourceStore::for_project(dir.path());
        assert_eq!(reloaded.list.len(), 1);
        assert_eq!(reloaded.list[0].id, b.id);
    }

    #[test]
    fn resolve_propagates_missing_secret_and_session_only_is_explicit() {
        let ds = DataSource {
            secret_ref: Some("nope".into()),
            ..DataSource::new("x", DriverKind::MySql, "mysql://h/db").unwrap()
        };
        let secrets = MemSecrets::default();
        assert_eq!(
            resolve_url(&ds, &secrets).unwrap_err().kind,
            ErrorKind::NotFound
        );
        let session = SessionOnly::default();
        assert!(session.backend_name().contains("NÃO"));
    }
}
