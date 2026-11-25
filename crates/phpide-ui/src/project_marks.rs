//! Folders marked as sources, tests or excluded (RF-NAV-19).
//!
//! Stored in `.phpide/project.json` next to the run configurations. Without
//! marks of its own, a project takes them from `composer.json`: `autoload`
//! roots are sources, `autoload-dev` roots are tests.

use std::path::{Path, PathBuf};

use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Sources,
    Tests,
    Excluded,
}

impl Mark {
    pub fn label(self) -> &'static str {
        match self {
            Mark::Sources => "Sources Root",
            Mark::Tests => "Test Sources Root",
            Mark::Excluded => "Excluded",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectMarks {
    pub sources: Vec<PathBuf>,
    pub tests: Vec<PathBuf>,
    pub excluded: Vec<PathBuf>,
    /// The marks came from composer.json, not from the user.
    pub from_composer: bool,
}

impl ProjectMarks {
    fn file(root: &Path) -> PathBuf {
        root.join(".phpide").join("project.json")
    }

    /// Marks of `root`: the user's, or composer.json's.
    pub fn load(root: &Path, composer: Option<&phpide_php::composer::ComposerInfo>) -> Self {
        if let Ok(text) = std::fs::read_to_string(Self::file(root)) {
            let parsed = serde_json::from_str::<serde_json::Value>(&text).ok();
            // A file holding only other settings has no marks of the user's.
            let has_marks = parsed.as_ref().is_some_and(|v| {
                ["sources", "tests", "excluded"]
                    .iter()
                    .any(|key| v.get(*key).is_some())
            });
            if let Some(v) = parsed.filter(|_| has_marks) {
                let list = |key: &str| -> Vec<PathBuf> {
                    v[key]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x.as_str())
                                .map(|rel| root.join(rel))
                                .collect()
                        })
                        .unwrap_or_default()
                };
                return ProjectMarks {
                    sources: list("sources"),
                    tests: list("tests"),
                    excluded: list("excluded"),
                    from_composer: false,
                };
            }
        }
        let Some(info) = composer else {
            return ProjectMarks::default();
        };
        let dirs = |roots: &[(String, PathBuf)]| -> Vec<PathBuf> {
            let mut out: Vec<PathBuf> = roots
                .iter()
                .map(|(_, dir)| {
                    if dir.is_absolute() {
                        dir.clone()
                    } else {
                        root.join(dir)
                    }
                })
                .map(|dir| PathBuf::from(dir.to_string_lossy().trim_end_matches('/')))
                .collect();
            out.dedup();
            out
        };
        ProjectMarks {
            sources: dirs(&info.source_roots),
            tests: dirs(&info.test_roots),
            excluded: Vec::new(),
            from_composer: true,
        }
    }

    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        let rel = |list: &[PathBuf]| -> Vec<String> {
            list.iter()
                .filter_map(|p| p.strip_prefix(root).ok())
                .map(|p| p.to_string_lossy().into_owned())
                .collect()
        };
        let file = Self::file(root);
        // Other settings share the file (the project's language server).
        let mut value = std::fs::read_to_string(&file)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .filter(serde_json::Value::is_object)
            .unwrap_or_else(|| json!({}));
        value["sources"] = json!(rel(&self.sources));
        value["tests"] = json!(rel(&self.tests));
        value["excluded"] = json!(rel(&self.excluded));
        let text = serde_json::to_string_pretty(&value).unwrap_or_default();
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        phpide_core::fsutil::write_atomic(&file, text.as_bytes())
    }

    /// The mark set on `dir` itself.
    pub fn mark_of(&self, dir: &Path) -> Option<Mark> {
        if self.excluded.iter().any(|d| d == dir) {
            Some(Mark::Excluded)
        } else if self.tests.iter().any(|d| d == dir) {
            Some(Mark::Tests)
        } else if self.sources.iter().any(|d| d == dir) {
            Some(Mark::Sources)
        } else {
            None
        }
    }

    /// The mark that applies to `path` (the nearest marked ancestor).
    pub fn applies_to(&self, path: &Path) -> Option<Mark> {
        path.ancestors().find_map(|dir| self.mark_of(dir))
    }

    /// Sets (or clears, with `None`) the mark of `dir`. The marks become
    /// the user's own from then on.
    pub fn set(&mut self, dir: &Path, mark: Option<Mark>) {
        for list in [&mut self.sources, &mut self.tests, &mut self.excluded] {
            list.retain(|d| d != dir);
        }
        match mark {
            Some(Mark::Sources) => self.sources.push(dir.to_path_buf()),
            Some(Mark::Tests) => self.tests.push(dir.to_path_buf()),
            Some(Mark::Excluded) => self.excluded.push(dir.to_path_buf()),
            None => {}
        }
        self.from_composer = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_come_from_composer_until_the_user_sets_them() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let info = phpide_php::composer::ComposerInfo {
            root: root.to_path_buf(),
            name: None,
            php_level: None,
            source_roots: vec![("App\\\\".into(), PathBuf::from("src/"))],
            test_roots: vec![("Tests\\\\".into(), PathBuf::from("tests/"))],
            vendor_present: false,
            packages: Vec::new(),
            warnings: Vec::new(),
        };
        let mut marks = ProjectMarks::load(root, Some(&info));
        assert!(marks.from_composer);
        assert_eq!(
            marks.applies_to(&root.join("src/Http/A.php")),
            Some(Mark::Sources)
        );
        assert_eq!(
            marks.applies_to(&root.join("tests/ATest.php")),
            Some(Mark::Tests)
        );
        marks.set(&root.join("storage"), Some(Mark::Excluded));
        marks.set(&root.join("tests"), None);
        marks.save(root).unwrap();
        let again = ProjectMarks::load(root, Some(&info));
        assert!(!again.from_composer);
        assert_eq!(again.mark_of(&root.join("storage")), Some(Mark::Excluded));
        assert_eq!(again.applies_to(&root.join("tests/ATest.php")), None);
        assert_eq!(
            again.applies_to(&root.join("src/x.php")),
            Some(Mark::Sources)
        );
    }
}
