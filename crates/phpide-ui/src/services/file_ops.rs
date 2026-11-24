//! File operations of the Project tool window: create, rename, delete,
//! copy and move (RF-NAV-15, RF-NAV-16). Small and synchronous: one folder
//! entry at a time, so they run on the UI thread like a save does.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Checks a new file or folder name typed by the user.
pub fn validate_name(name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Enter a name".into());
    }
    if name == "." || name == ".." {
        return Err("Invalid name".into());
    }
    if name.contains('\0') {
        return Err("The name contains an invalid character".into());
    }
    if name.split('/').any(|part| part.is_empty() || part == "..") {
        return Err("Invalid path".into());
    }
    Ok(())
}

/// Creates `dir/name` with `contents`. `name` may contain `/` to create
/// intermediate folders (`Http/Controller.php`), as in PhpStorm.
pub fn create_file(dir: &Path, name: &str, contents: &str) -> Result<PathBuf, String> {
    validate_name(name)?;
    let path = dir.join(name.trim());
    if path.exists() {
        return Err(format!("{} already exists", display_name(&path)));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(&path, contents).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Creates the folder `dir/name` (and its parents).
pub fn create_dir(dir: &Path, name: &str) -> Result<PathBuf, String> {
    validate_name(name)?;
    let path = dir.join(name.trim());
    if path.exists() {
        return Err(format!("{} already exists", display_name(&path)));
    }
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Renames `path` inside its folder.
pub fn rename(path: &Path, new_name: &str) -> Result<PathBuf, String> {
    validate_name(new_name)?;
    if new_name.contains('/') {
        return Err("Use Move to change the folder".into());
    }
    let target = path.with_file_name(new_name.trim());
    if target == path {
        return Ok(target);
    }
    // A case-only rename on a case-insensitive file system "exists".
    let case_only = target
        .to_string_lossy()
        .eq_ignore_ascii_case(&path.to_string_lossy());
    if target.exists() && !case_only {
        return Err(format!("{} already exists", display_name(&target)));
    }
    fs::rename(path, &target).map_err(|e| e.to_string())?;
    Ok(target)
}

/// Moves `path` to the desktop Trash (`gio trash`) when `use_trash`, or
/// deletes it when asked or when no Trash is available. Returns whether it
/// went to the Trash.
pub fn delete(path: &Path, use_trash: bool) -> io::Result<bool> {
    let trashed = use_trash
        && std::process::Command::new("gio")
            .arg("trash")
            .arg(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
    if trashed {
        return Ok(true);
    }
    if path.is_dir() && !path.is_symlink() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(false)
}

/// A free name for `name` inside `dir`: `name`, then `name copy`,
/// `name copy 2`… (the extension is kept).
pub fn free_name(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_string(), format!(".{ext}")),
        _ => (name.to_string(), String::new()),
    };
    (1..)
        .map(|n| {
            let suffix = if n == 1 {
                " copy".to_string()
            } else {
                format!(" copy {n}")
            };
            dir.join(format!("{stem}{suffix}{ext}"))
        })
        .find(|candidate| !candidate.exists())
        .expect("an unused name")
}

fn copy_recursive(from: &Path, to: &Path) -> io::Result<()> {
    if from.is_dir() {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(from, to).map(|_| ())
    }
}

/// Copies `from` into the folder `dir` under a free name.
pub fn copy_into(from: &Path, dir: &Path) -> Result<PathBuf, String> {
    if dir.starts_with(from) {
        return Err("Cannot copy a folder into itself".into());
    }
    let name = from
        .file_name()
        .ok_or("Nothing to copy")?
        .to_string_lossy()
        .into_owned();
    let target = free_name(dir, &name);
    copy_recursive(from, &target).map_err(|e| e.to_string())?;
    Ok(target)
}

/// Moves `from` into the folder `dir` (keeps the name; fails if taken).
pub fn move_into(from: &Path, dir: &Path) -> Result<PathBuf, String> {
    if dir.starts_with(from) {
        return Err("Cannot move a folder into itself".into());
    }
    let name = from.file_name().ok_or("Nothing to move")?;
    let target = dir.join(name);
    if target == from {
        return Ok(target);
    }
    if target.exists() {
        return Err(format!("{} already exists", display_name(&target)));
    }
    if fs::rename(from, &target).is_err() {
        // Another file system: copy then delete.
        copy_recursive(from, &target).map_err(|e| e.to_string())?;
        if from.is_dir() {
            fs::remove_dir_all(from).map_err(|e| e.to_string())?;
        } else {
            fs::remove_file(from).map_err(|e| e.to_string())?;
        }
    }
    Ok(target)
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// PSR-4 namespace for a class file in `dir`, from the project's
/// `composer.json` autoload roots (`App\\` → `src/`).
pub fn psr4_namespace(project: &Path, dir: &Path) -> Option<String> {
    let text = fs::read_to_string(project.join("composer.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let mut best: Option<(usize, String)> = None;
    for section in ["autoload", "autoload-dev"] {
        let Some(map) = json[section]["psr-4"].as_object() else {
            continue;
        };
        for (prefix, paths) in map {
            let paths: Vec<&str> = match paths {
                serde_json::Value::String(p) => vec![p.as_str()],
                serde_json::Value::Array(a) => a.iter().filter_map(|v| v.as_str()).collect(),
                _ => continue,
            };
            for root in paths {
                let root_dir = project.join(root.trim_end_matches('/'));
                if let Ok(rest) = dir.strip_prefix(&root_dir) {
                    let depth = root_dir.components().count();
                    if best.as_ref().is_some_and(|(d, _)| *d >= depth) {
                        continue;
                    }
                    let mut namespace = prefix.trim_end_matches('\\').to_string();
                    for part in rest.components() {
                        if !namespace.is_empty() {
                            namespace.push('\\');
                        }
                        namespace.push_str(&part.as_os_str().to_string_lossy());
                    }
                    best = Some((depth, namespace));
                }
            }
        }
    }
    best.map(|(_, namespace)| namespace)
}

/// Source of a new PHP class, interface, trait or enum.
pub fn php_class_source(kind: &str, name: &str, namespace: Option<&str>) -> String {
    let mut out = String::from("<?php\n\ndeclare(strict_types=1);\n\n");
    if let Some(namespace) = namespace.filter(|n| !n.is_empty()) {
        out.push_str(&format!("namespace {namespace};\n\n"));
    }
    out.push_str(&format!("{kind} {name}\n{{\n}}\n"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_rename_copy_move_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = create_file(root, "src/Http/Kernel.php", "<?php").unwrap();
        assert!(file.ends_with("src/Http/Kernel.php"));
        assert!(create_file(root, "src/Http/Kernel.php", "").is_err());
        assert!(create_file(root, "../escape.php", "").is_err());
        assert!(create_file(root, " ", "").is_err());
        let folder = create_dir(root, "tests").unwrap();

        let renamed = rename(&file, "HttpKernel.php").unwrap();
        assert!(renamed.exists() && !file.exists());
        assert!(rename(&renamed, "a/b.php").is_err());

        let copy = copy_into(&renamed, renamed.parent().unwrap()).unwrap();
        assert!(copy.ends_with("HttpKernel copy.php"));
        let copy2 = copy_into(&renamed, renamed.parent().unwrap()).unwrap();
        assert!(copy2.ends_with("HttpKernel copy 2.php"));
        assert!(copy_into(&root.join("src"), &root.join("src/Http")).is_err());

        let moved = move_into(&copy, &folder).unwrap();
        assert!(moved.exists() && !copy.exists());

        delete(&moved, false).unwrap();
        assert!(!moved.exists());
        delete(&root.join("src"), false).unwrap();
        assert!(!root.join("src").exists());
    }

    #[test]
    fn namespace_from_composer_psr4() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(
            root.join("composer.json"),
            r#"{"autoload":{"psr-4":{"App\\":"src/"}},"autoload-dev":{"psr-4":{"Tests\\":["tests/"]}}}"#,
        )
        .unwrap();
        assert_eq!(
            psr4_namespace(root, &root.join("src/Http/Controllers")).as_deref(),
            Some("App\\Http\\Controllers")
        );
        assert_eq!(
            psr4_namespace(root, &root.join("tests")).as_deref(),
            Some("Tests")
        );
        assert_eq!(psr4_namespace(root, &root.join("public")), None);
        let source = php_class_source("final class", "User", Some("App\\Models"));
        assert!(source.contains("namespace App\\Models;"));
        assert!(source.contains("final class User\n{"));
    }
}
