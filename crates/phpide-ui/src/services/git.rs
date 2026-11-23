//! Git state for the UI, computed off the UI thread.
//!
//! Every `git` invocation runs as a background job; the UI only reads cached
//! values and asks for refreshes. Results from an older request (another
//! file, another revision, another project) are dropped.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use phpide_core::jobs::{JobHandle, JobOutcome, JobSystem};
use phpide_vcs::BlameEntry;

/// How often the branch label is refreshed while the UI is repainting.
const BRANCH_REFRESH: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoStatus {
    /// Detection has not finished yet.
    Unknown,
    NotRepo,
    /// Inside a repository; the label is the branch or `(detached <sha>)`.
    Repo(String),
}

/// Line markers of one file at one buffer revision: `(line, added)`.
pub struct MarkersReply {
    pub path: PathBuf,
    pub revision: u64,
    pub markers: Vec<(u32, bool)>,
}

pub struct BlameReply {
    pub path: PathBuf,
    pub text_hash: u64,
    pub revision: u64,
    pub entries: Vec<BlameEntry>,
}

#[derive(Default)]
pub struct GitEvents {
    pub markers: Option<MarkersReply>,
    pub blame: Option<BlameReply>,
}

type MarkersJob = (PathBuf, u64, JobHandle<Vec<(u32, bool)>>);
type BlameJob = (PathBuf, u64, u64, JobHandle<Vec<BlameEntry>>);

pub struct GitService {
    root: PathBuf,
    status: RepoStatus,
    detect_job: Option<JobHandle<Option<String>>>,
    last_detect: Option<Instant>,
    markers_job: Option<MarkersJob>,
    blame_job: Option<BlameJob>,
    /// Last blame request that failed (untracked file, git error): not
    /// retried until the file content changes.
    blame_failed: Option<(PathBuf, u64)>,
}

impl GitService {
    pub fn new(root: PathBuf) -> Self {
        GitService {
            root,
            status: RepoStatus::Unknown,
            detect_job: None,
            last_detect: None,
            markers_job: None,
            blame_job: None,
            blame_failed: None,
        }
    }

    #[cfg(test)]
    pub fn status(&self) -> &RepoStatus {
        &self.status
    }

    pub fn is_repo(&self) -> bool {
        matches!(self.status, RepoStatus::Repo(_))
    }

    /// Branch label for widgets; `?` while detection is running.
    pub fn branch_label(&self) -> String {
        match &self.status {
            RepoStatus::Repo(label) => label.clone(),
            RepoStatus::Unknown | RepoStatus::NotRepo => "?".to_string(),
        }
    }

    /// Forces a new detection on the next poll.
    pub fn refresh(&mut self) {
        self.last_detect = None;
    }

    /// Asks for the line markers of `path` at buffer `revision`. A request
    /// for the same file and revision already in flight is not repeated.
    pub fn request_markers(
        &mut self,
        jobs: &JobSystem,
        ctx: &egui::Context,
        path: &Path,
        revision: u64,
    ) {
        if !self.is_repo() {
            return;
        }
        if matches!(&self.markers_job, Some((p, r, _)) if p == path && *r == revision) {
            return;
        }
        let root = self.root.clone();
        let file = path.to_path_buf();
        let ctx = ctx.clone();
        let job = jobs.spawn(move |_| {
            let markers = phpide_vcs::markers_for_file(&root, &file)
                .map(|m| {
                    m.iter()
                        .map(|(&line, kind)| (line, *kind == phpide_vcs::LineMarker::Added))
                        .collect()
                })
                .unwrap_or_default();
            ctx.request_repaint();
            Ok(markers)
        });
        self.markers_job = Some((path.to_path_buf(), revision, job));
    }

    /// Asks for `git blame` of the saved file identified by `text_hash`.
    pub fn request_blame(
        &mut self,
        jobs: &JobSystem,
        ctx: &egui::Context,
        path: &Path,
        text_hash: u64,
        revision: u64,
    ) {
        if !self.is_repo() {
            return;
        }
        if matches!(&self.blame_job, Some((p, h, _, _)) if p == path && *h == text_hash) {
            return;
        }
        if matches!(&self.blame_failed, Some((p, h)) if p == path && *h == text_hash) {
            return;
        }
        let root = self.root.clone();
        let file = path.to_path_buf();
        let ctx = ctx.clone();
        let job = jobs.spawn(move |_| {
            let result = phpide_vcs::blame(&root, &file);
            ctx.request_repaint();
            result
        });
        self.blame_job = Some((path.to_path_buf(), text_hash, revision, job));
    }

    /// Drives the jobs: call once per frame.
    pub fn poll(&mut self, jobs: &JobSystem, ctx: &egui::Context) -> GitEvents {
        let mut events = GitEvents::default();
        self.poll_detect(jobs, ctx);

        if let Some((path, revision, job)) = &mut self.markers_job {
            if let Some(outcome) = job.poll() {
                if let JobOutcome::Ok(markers) = outcome {
                    events.markers = Some(MarkersReply {
                        path: path.clone(),
                        revision: *revision,
                        markers,
                    });
                }
                self.markers_job = None;
            }
        }

        if let Some((path, text_hash, revision, job)) = &mut self.blame_job {
            if let Some(outcome) = job.poll() {
                match outcome {
                    JobOutcome::Ok(entries) => {
                        events.blame = Some(BlameReply {
                            path: path.clone(),
                            text_hash: *text_hash,
                            revision: *revision,
                            entries,
                        });
                    }
                    JobOutcome::Err(_) => {
                        self.blame_failed = Some((path.clone(), *text_hash));
                    }
                    JobOutcome::Cancelled => {}
                }
                self.blame_job = None;
            }
        }
        events
    }

    fn poll_detect(&mut self, jobs: &JobSystem, ctx: &egui::Context) {
        if let Some(job) = &mut self.detect_job {
            if let Some(outcome) = job.poll() {
                self.status = match outcome {
                    JobOutcome::Ok(Some(label)) => RepoStatus::Repo(label),
                    JobOutcome::Ok(None) | JobOutcome::Err(_) => RepoStatus::NotRepo,
                    JobOutcome::Cancelled => self.status.clone(),
                };
                self.detect_job = None;
            }
            return;
        }
        let due = self
            .last_detect
            .map(|at| at.elapsed() >= BRANCH_REFRESH)
            .unwrap_or(true);
        if !due {
            return;
        }
        self.last_detect = Some(Instant::now());
        let root = self.root.clone();
        let ctx = ctx.clone();
        self.detect_job = Some(jobs.spawn(move |_| {
            let label = phpide_vcs::detect(&root).ok().map(|state| state.label());
            ctx.request_repaint();
            Ok(label)
        }));
    }

    #[cfg(test)]
    fn wait_idle(&mut self, jobs: &JobSystem, ctx: &egui::Context) -> GitEvents {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut events = GitEvents::default();
        loop {
            let got = self.poll(jobs, ctx);
            events.markers = got.markers.or(events.markers);
            events.blame = got.blame.or(events.blame);
            let busy =
                self.detect_job.is_some() || self.markers_job.is_some() || self.blame_job.is_some();
            if !busy && self.status != RepoStatus::Unknown {
                return events;
            }
            assert!(Instant::now() < deadline, "git jobs did not finish");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(ok, "git {args:?}");
    }

    fn have_git() -> bool {
        Command::new("git").arg("--version").output().is_ok()
    }

    #[test]
    fn not_a_repository_is_detected_without_blocking_the_caller() {
        let dir = tempfile::tempdir().unwrap();
        let jobs = JobSystem::new();
        let ctx = egui::Context::default();
        let mut git = GitService::new(dir.path().to_path_buf());
        let t0 = Instant::now();
        git.poll(&jobs, &ctx);
        assert!(
            t0.elapsed() < Duration::from_millis(50),
            "poll must not wait"
        );
        assert_eq!(git.status(), &RepoStatus::Unknown);
        git.wait_idle(&jobs, &ctx);
        if have_git() {
            assert_eq!(git.status(), &RepoStatus::NotRepo);
        }
        assert_eq!(git.branch_label(), "?");
    }

    #[test]
    fn branch_markers_and_blame_arrive_from_background_jobs() {
        if !have_git() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("a.php"), "<?php\necho 1;\n").unwrap();
        git(root, &["add", "a.php"]);
        git(root, &["commit", "-q", "-m", "init"]);
        std::fs::write(root.join("a.php"), "<?php\necho 1;\necho 2;\n").unwrap();

        let jobs = JobSystem::new();
        let ctx = egui::Context::default();
        let mut git = GitService::new(root.to_path_buf());
        git.wait_idle(&jobs, &ctx);
        assert_eq!(git.status(), &RepoStatus::Repo("main".to_string()));

        let file = root.join("a.php");
        git.request_markers(&jobs, &ctx, &file, 7);
        let markers = git.wait_idle(&jobs, &ctx).markers.expect("markers");
        assert_eq!(markers.revision, 7);
        assert!(
            markers.markers.contains(&(3, true)),
            "{:?}",
            markers.markers
        );

        git.request_blame(&jobs, &ctx, &file, 42, 7);
        let blame = git.wait_idle(&jobs, &ctx).blame.expect("blame");
        assert_eq!(blame.text_hash, 42);
        assert!(!blame.entries.is_empty());
    }

    #[test]
    fn failed_blame_is_not_retried_for_the_same_content() {
        if !have_git() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("untracked.php"), "<?php\n").unwrap();
        let jobs = JobSystem::new();
        let ctx = egui::Context::default();
        let mut git = GitService::new(root.to_path_buf());
        git.wait_idle(&jobs, &ctx);
        let file = root.join("untracked.php");
        git.request_blame(&jobs, &ctx, &file, 1, 1);
        assert!(git.wait_idle(&jobs, &ctx).blame.is_none());
        git.request_blame(&jobs, &ctx, &file, 1, 1);
        assert!(
            git.blame_job.is_none(),
            "same content must not be blamed again"
        );
        git.request_blame(&jobs, &ctx, &file, 2, 2);
        assert!(git.blame_job.is_some(), "new content is blamed again");
    }
}
