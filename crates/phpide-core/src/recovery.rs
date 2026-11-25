//! Unsaved buffers kept on disk while the IDE runs, so a crash or a killed
//! process does not lose them. One folder per project under
//! `<state>/recovery/`; a clean exit empties it.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::fsutil::write_atomic;

/// One unsaved buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryEntry {
    /// The file it edits; `None` for a buffer never saved.
    pub path: Option<PathBuf>,
    /// Tab name (for untitled buffers).
    pub name: String,
    pub text: String,
    /// Hash of the file on disk when the buffer was last loaded or saved,
    /// to tell whether the file changed since.
    pub disk_hash: Option<u64>,
}

/// The recovery folder of one project.
pub struct RecoveryStore {
    dir: PathBuf,
}

impl RecoveryStore {
    /// `state` is the state folder; `project` the project root.
    pub fn new(state: &Path, project: &Path) -> Self {
        RecoveryStore {
            dir: state.join("recovery").join(project_key(project)),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Replaces what is stored with `entries` (empty clears it).
    pub fn save(&self, entries: &[RecoveryEntry]) -> std::io::Result<()> {
        if entries.is_empty() {
            return self.clear();
        }
        fs::create_dir_all(&self.dir)?;
        let mut index = Vec::new();
        for (i, entry) in entries.iter().enumerate() {
            let file = format!("buffer-{i}.txt");
            write_atomic(&self.dir.join(&file), entry.text.as_bytes())?;
            index.push(json!({
                "path": entry.path.as_ref().map(|p| p.to_string_lossy().into_owned()),
                "name": entry.name,
                "file": file,
                "disk_hash": entry.disk_hash.map(|h| h.to_string()),
            }));
        }
        write_atomic(
            &self.dir.join("index.json"),
            serde_json::to_string_pretty(&json!({ "version": 1, "buffers": index }))
                .unwrap_or_default()
                .as_bytes(),
        )?;
        // Files of buffers that are gone.
        if let Ok(list) = fs::read_dir(&self.dir) {
            for item in list.flatten() {
                let name = item.file_name().to_string_lossy().into_owned();
                if let Some(n) = name
                    .strip_prefix("buffer-")
                    .and_then(|r| r.strip_suffix(".txt"))
                    .and_then(|n| n.parse::<usize>().ok())
                {
                    if n >= entries.len() {
                        let _ = fs::remove_file(item.path());
                    }
                }
            }
        }
        Ok(())
    }

    /// What was stored (empty when nothing, or when it cannot be read).
    pub fn load(&self) -> Vec<RecoveryEntry> {
        let Ok(text) = fs::read_to_string(self.dir.join("index.json")) else {
            return Vec::new();
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            return Vec::new();
        };
        value["buffers"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|b| {
                        let text = fs::read_to_string(self.dir.join(b["file"].as_str()?)).ok()?;
                        Some(RecoveryEntry {
                            path: b["path"].as_str().map(PathBuf::from),
                            name: b["name"].as_str().unwrap_or("Untitled").to_string(),
                            text,
                            disk_hash: b["disk_hash"].as_str().and_then(|h| h.parse().ok()),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn clear(&self) -> std::io::Result<()> {
        match fs::remove_dir_all(&self.dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

/// Folder name for a project: readable prefix plus a hash of the path.
fn project_key(project: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    project.hash(&mut hasher);
    let name: String = project
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "root".into())
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(40)
        .collect();
    format!("{name}-{:016x}", hasher.finish())
}

/// Hash of file contents, to compare with [`RecoveryEntry::disk_hash`].
pub fn content_hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_loads_shrinks_and_clears() {
        let state = std::env::temp_dir().join(format!("phpide-recovery-{}", std::process::id()));
        let _ = fs::remove_dir_all(&state);
        let store = RecoveryStore::new(&state, Path::new("/home/ana/shop"));
        assert!(store.load().is_empty());
        let a = RecoveryEntry {
            path: Some("/home/ana/shop/a.php".into()),
            name: "a.php".into(),
            text: "<?php\necho 'é';\n".into(),
            disk_hash: Some(content_hash(b"old")),
        };
        let b = RecoveryEntry {
            path: None,
            name: "Untitled-1".into(),
            text: "notes".into(),
            disk_hash: None,
        };
        store.save(&[a.clone(), b.clone()]).unwrap();
        assert_eq!(store.load(), [a.clone(), b]);
        store.save(std::slice::from_ref(&a)).unwrap();
        assert_eq!(store.load(), [a]);
        assert!(!store.dir().join("buffer-1.txt").exists());
        // Another project has its own folder.
        assert!(RecoveryStore::new(&state, Path::new("/home/ana/blog"))
            .load()
            .is_empty());
        store.save(&[]).unwrap();
        assert!(!store.dir().exists());
        let _ = fs::remove_dir_all(&state);
    }
}
