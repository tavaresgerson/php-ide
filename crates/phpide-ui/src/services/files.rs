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

/// Excluded folders: tool metadata, not project content. Shown only with
/// "Show Excluded Files" (RF-NAV-10).
const EXCLUDED: [&str; 3] = [".git", ".idea", ".phpide"];

/// Most folders "Expand All" opens under one folder, so it cannot walk all
/// of `vendor/` by accident.
const EXPAND_ALL_LIMIT: usize = 400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub excluded: bool,
    pub modified: Option<std::time::SystemTime>,
}

/// How the tree orders and filters entries (RF-NAV-10, RF-NAV-14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeOptions {
    /// "name", "type" or "modified" (newest first).
    pub sort: String,
    pub folders_first: bool,
    pub show_excluded: bool,
}

impl Default for TreeOptions {
    fn default() -> Self {
        TreeOptions {
            sort: "name".into(),
            folders_first: true,
            show_excluded: false,
        }
    }
}

fn extension_of(name: &str) -> String {
    name.rsplit_once('.')
        .filter(|(stem, _)| !stem.is_empty())
        .map(|(_, ext)| ext.to_lowercase())
        .unwrap_or_default()
}

/// Orders folder contents for display. Folders keep name order whatever
/// the sort, as in PhpStorm.
pub fn sort_entries(entries: &mut [TreeEntry], options: &TreeOptions) {
    entries.sort_by(|a, b| {
        let group = if options.folders_first {
            b.is_dir.cmp(&a.is_dir)
        } else {
            std::cmp::Ordering::Equal
        };
        let by_name = || a.name.to_lowercase().cmp(&b.name.to_lowercase());
        group.then_with(|| {
            if a.is_dir && b.is_dir {
                return by_name();
            }
            match options.sort.as_str() {
                "type" => extension_of(&a.name)
                    .cmp(&extension_of(&b.name))
                    .then_with(by_name),
                "modified" => b.modified.cmp(&a.modified).then_with(by_name),
                _ => by_name(),
            }
        })
    });
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
    /// Folders whose subfolders expand as they load ("Expand All").
    expanding: HashSet<PathBuf>,
    /// Projects attached to this one, shown as extra roots (RF-PRJ-08).
    attached: Vec<PathBuf>,
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
            expanding: HashSet::new(),
            attached: Vec::new(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Attached project roots.
    pub fn attached(&self) -> &[PathBuf] {
        &self.attached
    }

    /// Replaces the attached roots; new ones are read and indexed.
    pub fn set_attached(&mut self, jobs: &JobSystem, ctx: &egui::Context, roots: Vec<PathBuf>) {
        if roots == self.attached {
            return;
        }
        for root in &roots {
            if !self.attached.contains(root) {
                self.expanded.insert(root.clone());
                self.request_dir(jobs, ctx, root);
            }
        }
        self.attached = roots;
        self.request_index(jobs, ctx);
    }

    /// The root (main or attached) that contains `path`.
    pub fn root_of(&self, path: &Path) -> Option<&Path> {
        std::iter::once(&self.root)
            .chain(self.attached.iter())
            .filter(|r| path.starts_with(r))
            .max_by_key(|r| r.components().count())
            .map(PathBuf::as_path)
    }

    pub fn is_expanded(&self, dir: &Path) -> bool {
        self.expanded.contains(dir)
    }

    /// Expands `dir` (no-op when open).
    pub fn expand(&mut self, jobs: &JobSystem, ctx: &egui::Context, dir: &Path) {
        if !self.expanded.contains(dir) {
            self.toggle(jobs, ctx, dir);
        }
    }

    /// Collapses every folder (RF-NAV-07).
    pub fn collapse_all(&mut self) {
        self.expanding.clear();
        let root = self.root.clone();
        let only_root = self.attached.is_empty();
        self.expanded.retain(|dir| only_root && *dir == root);
        if !only_root {
            // Root rows stay visible; the main one stays loaded.
            self.expanded.insert(root);
        }
    }

    /// Expands `dir` and its subfolders as they load, up to a limit, and
    /// skipping excluded and dependency folders (RF-NAV-07).
    pub fn expand_all(&mut self, jobs: &JobSystem, ctx: &egui::Context, dir: &Path) {
        self.expanding.insert(dir.to_path_buf());
        self.expand(jobs, ctx, dir);
        self.continue_expand_all(jobs, ctx);
    }

    fn continue_expand_all(&mut self, jobs: &JobSystem, ctx: &egui::Context) {
        if self.expanding.is_empty() {
            return;
        }
        let mut opened = 0;
        let mut queue: Vec<PathBuf> = self.expanding.iter().cloned().collect();
        while let Some(dir) = queue.pop() {
            let Some(children) = self.loaded.get(&dir) else {
                continue;
            };
            let subdirs: Vec<PathBuf> = children
                .iter()
                .filter(|c| c.is_dir && !c.excluded)
                .filter(|c| !matches!(c.name.as_str(), "vendor" | "node_modules"))
                .map(|c| c.path.clone())
                .collect();
            for sub in subdirs {
                if self.expanded.len() >= EXPAND_ALL_LIMIT {
                    self.expanding.clear();
                    return;
                }
                if self.expanded.insert(sub.clone()) {
                    opened += 1;
                    if !self.loaded.contains_key(&sub) {
                        self.request_dir(jobs, ctx, &sub);
                    }
                }
                queue.push(sub);
            }
        }
        if opened == 0 && self.pending.is_empty() {
            self.expanding.clear();
        }
    }

    /// Loaded contents of `dir` (asks for them when missing).
    pub fn children(
        &mut self,
        jobs: &JobSystem,
        ctx: &egui::Context,
        dir: &Path,
    ) -> Option<Vec<TreeEntry>> {
        match self.loaded.get(dir) {
            Some(children) => Some(children.clone()),
            None => {
                self.request_dir(jobs, ctx, dir);
                None
            }
        }
    }

    /// Re-reads one folder (after a file operation in it).
    pub fn reload_dir(&mut self, jobs: &JobSystem, ctx: &egui::Context, dir: &Path) {
        self.request_dir(jobs, ctx, dir);
        self.request_index(jobs, ctx);
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
        let roots: Vec<PathBuf> = std::iter::once(self.root.clone())
            .chain(self.attached.iter().cloned())
            .collect();
        for root in roots {
            if !self.loaded.contains_key(&root) {
                self.request_dir(jobs, ctx, &root);
            }
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
        let Some(root) = self.root_of(path).map(Path::to_path_buf) else {
            return;
        };
        let mut dirs: Vec<PathBuf> = path
            .ancestors()
            .skip(1)
            .take_while(|dir| dir.starts_with(&root))
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
    /// With attached projects, every root is a row of its own.
    pub fn visible_rows(&self, options: &TreeOptions) -> Vec<TreeRow> {
        let mut rows = Vec::new();
        if self.attached.is_empty() {
            self.push_children(&self.root, 0, options, &mut rows);
            return rows;
        }
        for root in std::iter::once(&self.root).chain(self.attached.iter()) {
            let expanded = self.expanded.contains(root);
            rows.push(TreeRow {
                entry: TreeEntry {
                    path: root.clone(),
                    name: root
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| root.display().to_string()),
                    is_dir: true,
                    excluded: false,
                    modified: None,
                },
                depth: 0,
                expanded,
                loading: expanded && self.pending.contains_key(root),
                error: self.failed.get(root).cloned(),
            });
            if expanded {
                self.push_children(root, 1, options, &mut rows);
            }
        }
        rows
    }

    fn push_children(
        &self,
        dir: &Path,
        depth: usize,
        options: &TreeOptions,
        rows: &mut Vec<TreeRow>,
    ) {
        let Some(children) = self.loaded.get(dir) else {
            return;
        };
        let mut children: Vec<TreeEntry> = children
            .iter()
            .filter(|c| options.show_excluded || !c.excluded)
            .cloned()
            .collect();
        sort_entries(&mut children, options);
        for child in &children {
            let expanded = child.is_dir && self.expanded.contains(&child.path);
            rows.push(TreeRow {
                entry: child.clone(),
                depth,
                expanded,
                loading: expanded && self.pending.contains_key(&child.path),
                error: self.failed.get(&child.path).cloned(),
            });
            if expanded {
                self.push_children(&child.path, depth + 1, options, rows);
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
                    self.continue_expand_all(jobs, ctx);
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
        let roots: Vec<PathBuf> = std::iter::once(self.root.clone())
            .chain(self.attached.iter().cloned())
            .collect();
        let ctx = ctx.clone();
        self.index_job = Some(jobs.spawn(move |_| {
            let files = roots
                .iter()
                .flat_map(|root| phpide_core::scan(root))
                .collect();
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

/// Folder contents for the tree, in name order (display order is
/// [`sort_entries`]).
pub fn read_dir_sorted(dir: &Path) -> io::Result<Vec<TreeEntry>> {
    let mut entries = Vec::new();
    for item in fs::read_dir(dir)? {
        let item = item?;
        let name = item.file_name().to_string_lossy().into_owned();
        let path = item.path();
        let file_type = item.file_type()?;
        let metadata = if file_type.is_symlink() {
            fs::metadata(&path).ok()
        } else {
            item.metadata().ok()
        };
        let is_dir = metadata.as_ref().is_some_and(|m| m.is_dir());
        let modified = metadata.and_then(|m| m.modified().ok());
        let excluded = EXCLUDED.contains(&name.as_str());
        entries.push(TreeEntry {
            path,
            name,
            is_dir,
            excluded,
            modified,
        });
    }
    sort_entries(&mut entries, &TreeOptions::default());
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
            .visible_rows(&TreeOptions::default())
            .iter()
            .map(|row| row.entry.name.clone())
            .collect();
        assert_eq!(names, vec!["src", "vendor", ".env", "composer.json"]);
        assert!(files
            .visible_rows(&TreeOptions::default())
            .iter()
            .all(|row| !row.expanded));

        files.toggle(&jobs, &ctx, &root.join("vendor"));
        assert!(
            files.visible_rows(&TreeOptions::default())[1].loading,
            "reading happens in background"
        );
        settle(&mut files, &jobs, &ctx);
        let names: Vec<String> = files
            .visible_rows(&TreeOptions::default())
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
        let rows = files.visible_rows(&TreeOptions::default());
        let last = rows
            .iter()
            .find(|row| row.entry.name == "Deep.php")
            .expect("revealed");
        assert_eq!(last.depth, 10);
    }

    #[test]
    fn options_sort_filter_and_expand_all() {
        let dir = project();
        let root = dir.path().to_path_buf();
        fs::write(root.join("b.txt"), "").unwrap();
        fs::write(root.join("a.php"), "").unwrap();
        let jobs = JobSystem::new();
        let ctx = egui::Context::default();
        let mut files = ProjectFiles::new(root.clone());
        files.refresh(&jobs, &ctx);
        settle(&mut files, &jobs, &ctx);
        let names = |files: &ProjectFiles, options: &TreeOptions| -> Vec<String> {
            files
                .visible_rows(options)
                .iter()
                .map(|r| r.entry.name.clone())
                .collect()
        };
        let by_type = TreeOptions {
            sort: "type".into(),
            ..TreeOptions::default()
        };
        assert_eq!(
            names(&files, &by_type),
            vec!["src", "vendor", ".env", "composer.json", "a.php", "b.txt"]
        );
        let with_excluded = TreeOptions {
            show_excluded: true,
            ..TreeOptions::default()
        };
        assert_eq!(names(&files, &with_excluded)[0], ".git");
        let mixed = TreeOptions {
            folders_first: false,
            ..TreeOptions::default()
        };
        assert_eq!(names(&files, &mixed)[0], ".env");

        files.expand_all(&jobs, &ctx, &root);
        settle(&mut files, &jobs, &ctx);
        assert!(files.is_expanded(&root.join("src/level0/level1")));
        assert!(
            !files.is_expanded(&root.join("vendor")),
            "dependencies stay closed"
        );
        files.collapse_all();
        assert!(files
            .visible_rows(&TreeOptions::default())
            .iter()
            .all(|r| !r.expanded));
    }

    #[test]
    fn attached_projects_are_extra_roots_and_are_indexed() {
        let dir = project();
        let other = tempfile::tempdir().unwrap();
        fs::write(other.path().join("lib.php"), "<?php").unwrap();
        let root = dir.path().to_path_buf();
        let jobs = JobSystem::new();
        let ctx = egui::Context::default();
        let mut files = ProjectFiles::new(root.clone());
        files.refresh(&jobs, &ctx);
        settle(&mut files, &jobs, &ctx);
        files.set_attached(&jobs, &ctx, vec![other.path().to_path_buf()]);
        let index = settle(&mut files, &jobs, &ctx).expect("index");
        assert!(index > 12);
        let rows = files.visible_rows(&TreeOptions::default());
        let roots: Vec<&TreeRow> = rows.iter().filter(|r| r.depth == 0).collect();
        assert_eq!(roots.len(), 2);
        assert!(rows
            .iter()
            .any(|r| r.entry.name == "lib.php" && r.depth == 1));
        assert_eq!(
            files.root_of(&other.path().join("lib.php")),
            Some(other.path())
        );
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
