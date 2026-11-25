//! The PHP language server of a project: which engine (Settings, with an
//! override in `.phpide/project.json`), what it is told at startup and when
//! it is restarted after a crash.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use phpide_lsp::engine::EngineChoice;
use serde_json::{json, Value};

use crate::project_marks::ProjectMarks;

fn project_file(root: &Path) -> PathBuf {
    root.join(".phpide").join("project.json")
}

/// The engine `.phpide/project.json` chooses for this project, if any.
pub fn project_choice(root: &Path) -> Option<EngineChoice> {
    let text = std::fs::read_to_string(project_file(root)).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let server = value.get("language_server")?;
    let id = server["engine"].as_str()?;
    Some(EngineChoice::from_id(
        id,
        server["command"].as_str().unwrap_or_default(),
    ))
}

/// Sets (or, with `None`, removes) the project's own engine, keeping the
/// rest of `.phpide/project.json`.
pub fn set_project_choice(root: &Path, choice: Option<&EngineChoice>) -> std::io::Result<()> {
    let file = project_file(root);
    let mut value: Value = std::fs::read_to_string(&file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    match choice {
        Some(choice) => {
            value["language_server"] =
                json!({ "engine": choice.id(), "command": choice.command() });
        }
        None => {
            if let Some(object) = value.as_object_mut() {
                object.remove("language_server");
            }
        }
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(&value).unwrap_or_default();
    phpide_core::fsutil::write_atomic(&file, text.as_bytes())
}

/// The engine in force for `root`: the project's own, else the global one.
pub fn effective_choice(settings: &crate::settings::Settings, root: &Path) -> EngineChoice {
    project_choice(root).unwrap_or_else(|| settings.lsp_choice())
}

/// PHPantom's file filters (`initializationOptions.indexing` and
/// `workspace/didChangeConfiguration`): folders marked Excluded, as
/// gitignore patterns anchored at the root, and the extensions File Types
/// maps to PHP.
pub fn indexing_settings(
    root: &Path,
    marks: &ProjectMarks,
    file_type_patterns: &[(String, String)],
) -> Value {
    let mut exclude: Vec<String> = marks
        .excluded
        .iter()
        .filter_map(|dir| dir.strip_prefix(root).ok())
        .filter(|rel| !rel.as_os_str().is_empty())
        .map(|rel| format!("/{}/", rel.to_string_lossy().trim_end_matches('/')))
        .collect();
    exclude.sort();
    exclude.dedup();
    let mut extensions: Vec<String> = file_type_patterns
        .iter()
        .filter(|(_, id)| id == "php")
        .filter_map(|(pattern, _)| {
            let ext = pattern.strip_prefix("*.")?;
            let plain =
                !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            (plain && ext != "php").then(|| ext.to_lowercase())
        })
        .collect();
    extensions.sort();
    extensions.dedup();
    json!({ "indexing": { "exclude": exclude, "extensions": extensions } })
}

/// Restarts after a crash, waiting longer each time: at most
/// [`MAX_RESTARTS`] in [`WINDOW`], then it gives up until the user restarts
/// the server.
#[derive(Debug, Default)]
pub struct Supervisor {
    restarts: Vec<Instant>,
    next_at: Option<Instant>,
    gave_up: bool,
}

pub const MAX_RESTARTS: usize = 3;
pub const WINDOW: Duration = Duration::from_secs(5 * 60);

impl Supervisor {
    /// The server stopped by itself at `now`. Returns the wait before the
    /// restart, or `None` once the restarts in the window are used up.
    pub fn crashed(&mut self, now: Instant) -> Option<Duration> {
        self.restarts
            .retain(|at| now.saturating_duration_since(*at) < WINDOW);
        if self.restarts.len() >= MAX_RESTARTS {
            self.gave_up = true;
            self.next_at = None;
            return None;
        }
        let wait = Duration::from_secs(1 << self.restarts.len());
        self.next_at = Some(now + wait);
        Some(wait)
    }

    /// Whether a scheduled restart is due; it is then counted.
    pub fn due(&mut self, now: Instant) -> bool {
        match self.next_at {
            Some(at) if at <= now => {
                self.next_at = None;
                self.restarts.push(now);
                true
            }
            _ => false,
        }
    }

    pub fn waiting(&self) -> bool {
        self.next_at.is_some()
    }

    pub fn gave_up(&self) -> bool {
        self.gave_up
    }

    /// A restart asked by the user starts the count again.
    pub fn reset(&mut self) {
        *self = Supervisor::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_choice_overrides_and_keeps_the_marks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut marks = ProjectMarks::default();
        marks.set(
            &root.join("build"),
            Some(crate::project_marks::Mark::Excluded),
        );
        marks.save(root).unwrap();
        assert_eq!(project_choice(root), None);

        set_project_choice(
            root,
            Some(&EngineChoice::Custom("phpactor language-server".into())),
        )
        .unwrap();
        assert_eq!(
            project_choice(root),
            Some(EngineChoice::Custom("phpactor language-server".into()))
        );
        // The marks are still there, and saving them keeps the choice.
        let again = ProjectMarks::load(root, None);
        assert_eq!(again.excluded, [root.join("build")]);
        again.save(root).unwrap();
        assert!(project_choice(root).is_some());

        set_project_choice(root, None).unwrap();
        assert_eq!(project_choice(root), None);
        assert_eq!(
            ProjectMarks::load(root, None).excluded,
            [root.join("build")]
        );
    }

    #[test]
    fn indexing_settings_from_marks_and_file_types() {
        let root = Path::new("/p");
        let mut marks = ProjectMarks::default();
        marks.set(
            Path::new("/p/generated/cache"),
            Some(crate::project_marks::Mark::Excluded),
        );
        marks.set(
            Path::new("/elsewhere"),
            Some(crate::project_marks::Mark::Excluded),
        );
        marks.set(
            Path::new("/p/src"),
            Some(crate::project_marks::Mark::Sources),
        );
        let patterns = vec![
            ("*.module".to_string(), "php".to_string()),
            ("*.inc".to_string(), "php".to_string()),
            ("*.tpl".to_string(), "html".to_string()),
            ("Makefile.*".to_string(), "php".to_string()),
        ];
        assert_eq!(
            indexing_settings(root, &marks, &patterns),
            json!({ "indexing": {
                "exclude": ["/generated/cache/"],
                "extensions": ["inc", "module"]
            } })
        );
    }

    #[test]
    fn restarts_back_off_then_give_up() {
        let mut s = Supervisor::default();
        let t0 = Instant::now();
        assert_eq!(s.crashed(t0), Some(Duration::from_secs(1)));
        assert!(!s.due(t0));
        assert!(s.due(t0 + Duration::from_secs(1)));
        assert_eq!(
            s.crashed(t0 + Duration::from_secs(2)),
            Some(Duration::from_secs(2))
        );
        assert!(s.due(t0 + Duration::from_secs(4)));
        assert_eq!(
            s.crashed(t0 + Duration::from_secs(5)),
            Some(Duration::from_secs(4))
        );
        assert!(s.due(t0 + Duration::from_secs(9)));
        // Three restarts in five minutes: the fourth crash is final.
        assert_eq!(s.crashed(t0 + Duration::from_secs(10)), None);
        assert!(s.gave_up());
        // Once the window has passed, crashes are restarted again.
        let mut s = Supervisor::default();
        for i in 0..3 {
            s.crashed(t0 + Duration::from_secs(i));
            assert!(s.due(t0 + Duration::from_secs(i + 10)));
        }
        assert!(s.crashed(t0 + WINDOW + Duration::from_secs(60)).is_some());
        s.reset();
        assert!(!s.gave_up() && !s.waiting());
    }
}
