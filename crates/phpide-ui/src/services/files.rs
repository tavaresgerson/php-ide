//! Project files for the UI, read off the UI thread.
//!
//! Two views of the same folder:
//! - the **index**: every file of the project (dependency folders excluded),
//!   used by Go to File, Search Everywhere and Find in Files;
//! - the **tree**: what the Project tool window shows. Each folder is read
//!   only when it is expanded, so `vendor/` and `node_modules/` are visible
//!   and cost nothing until opened, and there is no depth limit.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use eframe::egui;
use phpide_core::jobs::{JobHandle, JobOutcome, JobSystem};
use phpide_core::{FileEntry, IdeError};

/// Folders never shown in the tree (tool metadata, not project content).
const HIDDEN_IN_TREE: [&str; 2] = [".git", ".idea"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}

/// One visible row of the tree, in display order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    pub entry: TreeEntry,
    pub depth: usize,
    pub expanded: bool,
    pub loading: bool,
    pub error: Option<String>,
}

pub struct ProjectFiles {
    root: PathBuf,
    loaded: HashMap<PathBuf, Vec<TreeEntry>>,
    pending: HashMap<PathBuf, JobHandle<Vec<TreeEntry>>>,
    failed: HashMap<PathBuf, String>,
    expanded: HashSet<PathBuf>,
    index_job: Option<JobHandle<Vec<FileEntry>>>,
    /// A refresh was asked while the index job was running.
    index_stale: bool,
    /// The first read (root folder + index) has been requested.
    started: bool,
}

impl ProjectFiles {
    pub fn new(root: PathBuf) -> Self {
        let mut expanded = HashSet::new();
        expanded.insert(root.clone());
        ProjectFiles {
            root,
            loaded: HashMap::new(),
            pending: HashMap::new(),
            failed: HashMap::new(),
            expanded,
            index_job: None,
            index_stale: false,
            started: false,
        }
    }

    /// True while the search index is being built.
    pub fn indexing(&self) -> bool {
        self.index_job.is_some()
    }

    /// Re-reads the index and every folder already loaded (file watcher,
    /// "Reload Project Tree").
    pub fn refresh(&mut self, jobs: &JobSystem, ctx: &egui::Context) {
        self.started = true;
        self.request_index(jobs, ctx);
        let dirs: Vec<PathBuf> = self.loaded.keys().cloned().collect();
        for dir in dirs {
            self.request_dir(jobs, ctx, &dir);
        }
        if !self.loaded.contains_key(&self.root) {
            let root = self.root.clone();
            self.request_dir(jobs, ctx, &root);
        }
    }

    /// Expands or collapses `dir`, reading it the first time it opens.
    pub fn toggle(&mut self, jobs: &JobSystem, ctx: &egui::Context, dir: &Path) {
        if !self.expanded.remove(dir) {
            self.expanded.insert(dir.to_path_buf());
            if !self.loaded.contains_key(dir) {
                self.request_dir(jobs, ctx, dir);
            }
        }
    }

    /// Expands every ancestor of `path` (reveal the active file).
    pub fn reveal(&mut self, jobs: &JobSystem, ctx: &egui::Context, path: &Path) {
        let mut dirs: Vec<PathBuf> = path
            .ancestors()
            .skip(1)
            .take_while(|dir| dir.starts_with(&self.root))
            .map(Path::to_path_buf)
            .collect();
        dirs.reverse();
        for dir in dirs {
            if self.expanded.insert(dir.clone()) && !self.loaded.contains_key(&dir) {
                self.request_dir(jobs, ctx, &dir);
            }
        }
    }

    /// Rows to draw, in order. Only expanded and loaded folders contribute.
    pub fn visible_rows(&self) -> Vec<TreeRow> {
        let mut rows = Vec::new();
        self.push_children(&self.root, 0, &mut rows);
        rows
    }

    fn push_children(&self, dir: &Path, depth: usize, rows: &mut Vec<TreeRow>) {
        let Some(children) = self.loaded.get(dir) else {
            return;
        };
        for child in children {
            let expanded = child.is_dir && self.expanded.contains(&child.path);
            rows.push(TreeRow {
                entry: child.clone(),
                depth,
                expanded,
                loading: expanded && self.pending.contains_key(&child.path),
                error: self.failed.get(&child.path).cloned(),
            });
            if expanded {
                self.push_children(&child.path, depth + 1, rows);
            }
        }
    }

    pub fn root_loading(&self) -> bool {
        !self.loaded.contains_key(&self.root) && self.pending.contains_key(&self.root)
    }

    pub fn root_error(&self) -> Option<&str> {
        self.failed.get(&self.root).map(String::as_str)
    }

    /// Drives the jobs: call once per frame. Returns a new index when one
    /// finished.
    pub fn poll(&mut self, jobs: &JobSystem, ctx: &egui::Context) -> Option<Vec<FileEntry>> {
        if !self.started {
            self.started = true;
            self.refresh(jobs, ctx);
        }
        let mut finished = Vec::new();
        for (dir, job) in self.pending.iter_mut() {
            if let Some(outcome) = job.poll() {
                finished.push((dir.clone(), outcome));
            }
        }
        for (dir, outcome) in finished {
            self.pending.remove(&dir);
            match outcome {
                JobOutcome::Ok(entries) => {
                    self.failed.remove(&dir);
                    self.loaded.insert(dir, entries);
                }
                JobOutcome::Err(error) => {
                    self.failed.insert(dir, error.message);
                }
                JobOutcome::Cancelled => {}
            }
        }

        let mut index = None;
        if let Some(job) = &mut self.index_job {
            if let Some(outcome) = job.poll() {
                if let JobOutcome::Ok(files) = outcome {
                    index = Some(files);
                }
                self.index_job = None;
                if self.index_stale {
                    self.index_stale = false;
                    self.request_index(jobs, ctx);
                }
            }
        }
        index
    }

    fn request_index(&mut self, jobs: &JobSystem, ctx: &egui::Context) {
        if self.index_job.is_some() {
            self.index_stale = true;
            return;
        }
        let root = self.root.clone();
        let ctx = ctx.clone();
        self.index_job = Some(jobs.spawn(move |_| {
            let files = phpide_core::scan(&root);
            ctx.request_repaint();
            Ok(files)
        }));
    }

    fn request_dir(&mut self, jobs: &JobSystem, ctx: &egui::Context, dir: &Path) {
        if self.pending.contains_key(dir) {
            return;
        }
        let target = dir.to_path_buf();
        let ctx = ctx.clone();
        let job = jobs.spawn(move |_| {
            let result = read_dir_sorted(&target).map_err(|error| {
                IdeError::from_io(
                    phpide_core::Component::Ui,
                    "read_dir",
                    &target.display().to_string(),
                    &error,
                )
            });
            ctx.request_repaint();
            result
        });
        self.pending.insert(dir.to_path_buf(), job);
    }
}

/// Folder contents for the tree: folders first, then files, each sorted by
/// name without regard to case.
pub fn read_dir_sorted(dir: &Path) -> io::Result<Vec<TreeEntry>> {
    let mut entries = Vec::new();
    for item in fs::read_dir(dir)? {
        let item = item?;
        let name = item.file_name().to_string_lossy().into_owned();
        if HIDDEN_IN_TREE.contains(&name.as_str()) {
            continue;
        }
        let path = item.path();
        let file_type = item.file_type()?;
        let is_dir = if file_type.is_symlink() {
            fs::metadata(&path).map(|m| m.is_dir()).unwrap_or(false)
        } else {
            file_type.is_dir()
        };
        entries.push(TreeEntry { path, name, is_dir });
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn settle(files: &mut ProjectFiles, jobs: &JobSystem, ctx: &egui::Context) -> Option<usize> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut index = None;
        loop {
            if let Some(found) = files.poll(jobs, ctx) {
                index = Some(found.len());
            }
            if files.pending.is_empty() && files.index_job.is_none() {
                return index;
            }
            assert!(Instant::now() < deadline, "file jobs did not finish");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut deep = root.join("src");
        for level in 0..9 {
            deep = deep.join(format!("level{level}"));
        }
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("Deep.php"), "<?php").unwrap();
        fs::create_dir_all(root.join("vendor/acme")).unwrap();
        fs::write(root.join("vendor/acme/Lib.php"), "<?php").unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("composer.json"), "{}").unwrap();
        fs::write(root.join(".env"), "A=1").unwrap();
        dir
    }

    #[test]
    fn tree_loads_folders_on_expand_and_shows_vendor_collapsed() {
        let dir = project();
        let root = dir.path().to_path_buf();
        let jobs = JobSystem::new();
        let ctx = egui::Context::default();
        let mut files = ProjectFiles::new(root.clone());
        files.refresh(&jobs, &ctx);
        settle(&mut files, &jobs, &ctx);

        let names: Vec<String> = files
            .visible_rows()
            .iter()
            .map(|row| row.entry.name.clone())
            .collect();
        assert_eq!(names, vec!["src", "vendor", ".env", "composer.json"]);
        assert!(files.visible_rows().iter().all(|row| !row.expanded));

        files.toggle(&jobs, &ctx, &root.join("vendor"));
        assert!(
            files.visible_rows()[1].loading,
            "reading happens in background"
        );
        settle(&mut files, &jobs, &ctx);
        let names: Vec<String> = files
            .visible_rows()
            .iter()
            .map(|row| row.entry.name.clone())
            .collect();
        assert_eq!(
            names,
            vec!["src", "vendor", "acme", ".env", "composer.json"]
        );
    }

    #[test]
    fn deep_files_are_indexed_and_revealed_without_depth_limit() {
        let dir = project();
        let root = dir.path().to_path_buf();
        let jobs = JobSystem::new();
        let ctx = egui::Context::default();
        let mut files = ProjectFiles::new(root.clone());
        files.refresh(&jobs, &ctx);
        let indexed = settle(&mut files, &jobs, &ctx).expect("index");
        assert!(indexed >= 12, "deep folders indexed: {indexed}");

        let mut deep = root.join("src");
        for level in 0..9 {
            deep = deep.join(format!("level{level}"));
        }
        files.reveal(&jobs, &ctx, &deep.join("Deep.php"));
        settle(&mut files, &jobs, &ctx);
        let rows = files.visible_rows();
        let last = rows
            .iter()
            .find(|row| row.entry.name == "Deep.php")
            .expect("revealed");
        assert_eq!(last.depth, 10);
    }

    #[test]
    fn missing_folder_reports_an_error_instead_of_an_empty_tree() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("gone");
        let jobs = JobSystem::new();
        let ctx = egui::Context::default();
        let mut files = ProjectFiles::new(missing);
        files.refresh(&jobs, &ctx);
        settle(&mut files, &jobs, &ctx);
        assert!(files.root_error().is_some());
    }
}
