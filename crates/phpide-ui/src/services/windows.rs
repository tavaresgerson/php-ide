//! Project windows across processes (RF-PRJ-10/11/12).
//!
//! Every PHP IDE window is its own process. Each one keeps a small file in
//! a runtime folder (`<pid>.json` with its project) and reads a mailbox
//! (`<pid>.cmd`) where other windows drop "focus" or "close". The session
//! file in the config folder lists the projects to reopen on startup.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Another open window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowEntry {
    pub pid: u32,
    pub root: PathBuf,
}

/// A request from another window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowCommand {
    Focus,
    Close,
}

pub struct WindowRegistry {
    dir: PathBuf,
    session: PathBuf,
    pid: u32,
    root: Option<PathBuf>,
    windows: Vec<WindowEntry>,
    last_poll: Option<Instant>,
}

const POLL_EVERY: Duration = Duration::from_millis(500);

fn runtime_dir() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join(format!("phpide-{}", user_id())));
    base.join("phpide").join("windows")
}

fn user_id() -> String {
    std::env::var("USER").unwrap_or_else(|_| "user".into())
}

/// Whether process `pid` is alive (Linux `/proc`).
fn alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

impl WindowRegistry {
    /// Registry of this process, in the user's runtime folder.
    pub fn new() -> Self {
        Self::with_dirs(
            runtime_dir(),
            crate::settings::config_dir().join("session.json"),
        )
    }

    pub fn with_dirs(dir: PathBuf, session: PathBuf) -> Self {
        WindowRegistry {
            dir,
            session,
            pid: std::process::id(),
            root: None,
            windows: Vec::new(),
            last_poll: None,
        }
    }

    #[cfg(test)]
    fn with_pid(mut self, pid: u32) -> Self {
        self.pid = pid;
        self
    }

    fn entry_path(&self, pid: u32) -> PathBuf {
        self.dir.join(format!("{pid}.json"))
    }

    fn mailbox(&self, pid: u32) -> PathBuf {
        self.dir.join(format!("{pid}.cmd"))
    }

    /// Records the project shown by this window (`None` on Home).
    pub fn set_project(&mut self, root: Option<&Path>) {
        self.root = root.map(Path::to_path_buf);
        let _ = fs::create_dir_all(&self.dir);
        match &self.root {
            Some(root) => {
                let body = serde_json::json!({ "root": root.display().to_string() });
                let _ = fs::write(self.entry_path(self.pid), body.to_string());
                self.add_to_session(root);
            }
            None => {
                let _ = fs::remove_file(self.entry_path(self.pid));
            }
        }
        self.last_poll = None;
    }

    /// The window is closing. The last window keeps its project in the
    /// session so it reopens; others leave it.
    pub fn unregister(&mut self) {
        self.refresh();
        let last = self.windows.is_empty();
        if let Some(root) = self.root.clone() {
            if !last {
                self.remove_from_session(&root);
            }
        }
        let _ = fs::remove_file(self.entry_path(self.pid));
        let _ = fs::remove_file(self.mailbox(self.pid));
    }

    /// Close Project: the project leaves the session.
    pub fn project_closed(&mut self) {
        if let Some(root) = self.root.take() {
            self.remove_from_session(&root);
        }
        let _ = fs::remove_file(self.entry_path(self.pid));
    }

    /// Other windows, oldest first. Refreshed by [`WindowRegistry::poll`].
    pub fn others(&self) -> &[WindowEntry] {
        &self.windows
    }

    /// This window first, then the others, in a stable order (pid).
    pub fn all_windows(&self) -> Vec<WindowEntry> {
        let mut all = self.windows.clone();
        if let Some(root) = &self.root {
            all.push(WindowEntry {
                pid: self.pid,
                root: root.clone(),
            });
        }
        all.sort_by_key(|w| w.pid);
        all
    }

    /// Window after (or before) this one, for Ctrl+Alt+] / [.
    pub fn neighbour(&self, forward: bool) -> Option<WindowEntry> {
        let all = self.all_windows();
        if all.len() < 2 {
            return None;
        }
        let here = all.iter().position(|w| w.pid == self.pid).unwrap_or(0);
        let next = if forward {
            (here + 1) % all.len()
        } else {
            (here + all.len() - 1) % all.len()
        };
        Some(all[next].clone())
    }

    /// Asks window `pid` to do something.
    pub fn send(&self, pid: u32, command: WindowCommand) {
        let text = match command {
            WindowCommand::Focus => "focus",
            WindowCommand::Close => "close",
        };
        let _ = fs::create_dir_all(&self.dir);
        let _ = fs::write(self.mailbox(pid), text);
    }

    /// Rereads the other windows (dead ones are removed).
    pub fn refresh(&mut self) {
        let mut windows = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|e| e != "json") {
                    continue;
                }
                let Some(pid) = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.parse::<u32>().ok())
                else {
                    continue;
                };
                if pid == self.pid {
                    continue;
                }
                if !alive(pid) {
                    let _ = fs::remove_file(&path);
                    let _ = fs::remove_file(self.mailbox(pid));
                    continue;
                }
                let root = fs::read_to_string(&path)
                    .ok()
                    .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                    .and_then(|v| v["root"].as_str().map(PathBuf::from));
                if let Some(root) = root {
                    windows.push(WindowEntry { pid, root });
                }
            }
        }
        windows.sort_by_key(|w| w.pid);
        self.windows = windows;
    }

    /// Call every frame: refreshes the window list twice a second and
    /// returns a command left for this window.
    pub fn poll(&mut self) -> Option<WindowCommand> {
        if self.last_poll.is_some_and(|t| t.elapsed() < POLL_EVERY) {
            return None;
        }
        self.last_poll = Some(Instant::now());
        self.refresh();
        let mailbox = self.mailbox(self.pid);
        let text = fs::read_to_string(&mailbox).ok()?;
        let _ = fs::remove_file(&mailbox);
        match text.trim() {
            "focus" => Some(WindowCommand::Focus),
            "close" => Some(WindowCommand::Close),
            _ => None,
        }
    }

    /// Projects to reopen on startup, in order.
    pub fn session(&self) -> Vec<PathBuf> {
        fs::read_to_string(&self.session)
            .ok()
            .and_then(|t| serde_json::from_str::<Vec<String>>(&t).ok())
            .unwrap_or_default()
            .into_iter()
            .map(PathBuf::from)
            .collect()
    }

    fn write_session(&self, projects: &[PathBuf]) {
        if cfg!(test) && !self.session.starts_with(std::env::temp_dir()) {
            return;
        }
        if let Some(dir) = self.session.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let list: Vec<String> = projects.iter().map(|p| p.display().to_string()).collect();
        let _ = fs::write(
            &self.session,
            serde_json::to_string_pretty(&list).unwrap_or_default(),
        );
    }

    fn add_to_session(&self, root: &Path) {
        let mut projects = self.session();
        if !projects.iter().any(|p| p == root) {
            projects.push(root.to_path_buf());
            self.write_session(&projects);
        }
    }

    fn remove_from_session(&self, root: &Path) {
        let mut projects = self.session();
        projects.retain(|p| p != root);
        self.write_session(&projects);
    }
}

impl Default for WindowRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_see_each_other_exchange_commands_and_keep_the_session() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("windows");
        let session = dir.path().join("session.json");
        // Two "windows": this process and the parent (alive for sure).
        let parent = std::os::unix::process::parent_id();
        let mut a = WindowRegistry::with_dirs(runtime.clone(), session.clone());
        let mut b = WindowRegistry::with_dirs(runtime.clone(), session.clone()).with_pid(parent);
        a.set_project(Some(Path::new("/p/shop")));
        b.set_project(Some(Path::new("/p/blog")));
        a.refresh();
        assert_eq!(a.others().len(), 1);
        assert_eq!(a.others()[0].root, PathBuf::from("/p/blog"));
        assert_eq!(
            a.session(),
            vec![PathBuf::from("/p/shop"), PathBuf::from("/p/blog")]
        );
        assert!(a.neighbour(true).is_some());

        a.send(parent, WindowCommand::Focus);
        assert_eq!(b.poll(), Some(WindowCommand::Focus));
        assert_eq!(b.poll(), None, "a command is read once");

        // A dead window disappears.
        fs::write(runtime.join("999999999.json"), r#"{"root":"/gone"}"#).unwrap();
        a.refresh();
        assert_eq!(a.others().len(), 1);

        // Closing a window while another is open drops it from the session;
        // the last one stays to be reopened.
        b.unregister();
        assert_eq!(a.session(), vec![PathBuf::from("/p/shop")]);
        a.unregister();
        assert_eq!(a.session(), vec![PathBuf::from("/p/shop")]);
        a.set_project(Some(Path::new("/p/shop")));
        a.project_closed();
        assert!(a.session().is_empty());
    }
}
