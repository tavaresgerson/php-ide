//! Git for the Commit, Version Control and diff views, off the UI thread.
//!
//! - A **snapshot** of the repository (changed files, ignored folders,
//!   branches, the operation in progress) is refreshed in a job after every
//!   change and every few seconds; the UI only reads it.
//! - **Quick operations** (stage, unstage, rollback, one hunk, take a side
//!   of a conflict) run one at a time in jobs.
//! - **Commands** that print output the user must see (commit hooks,
//!   fetch, pull, push, checkout, merge) run with streamed output into the
//!   Git console, one at a time, and can be stopped.
//! - **Diffs, log pages and commit details** load on request; results come
//!   back as [`VcsEvent`]s tagged with the requester's id.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use phpide_core::jobs::{JobHandle, JobOutcome, JobSystem};
use phpide_exec::{spawn_stream, CommandSpec, StreamEvent, StreamingChild};

pub use phpide_vcs::commands as git_commands;
pub use phpide_vcs::{
    Branch, ChangeKind, CommitEntry, CommitFile, ConflictSide, DiffLineKind, FileDiff, FileStatus,
    GitCommand, HunkAction, LogFilter, RepoOperation, Side,
};

/// How often the snapshot refreshes on its own.
const SNAPSHOT_REFRESH: Duration = Duration::from_secs(5);
/// Bigger files are not compared line by line.
const MAX_DIFF_BYTES: usize = 4 * 1024 * 1024;

/// Repository state the views read.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// Worktree root; paths below are relative to it.
    pub toplevel: PathBuf,
    pub changes: Vec<FileStatus>,
    /// Ignored files and folders (folders end with `/`).
    pub ignored: Vec<PathBuf>,
    pub branches: Vec<Branch>,
    pub operation: Option<RepoOperation>,
}

impl Snapshot {
    pub fn current_branch(&self) -> Option<&Branch> {
        self.branches.iter().find(|b| b.current && !b.remote)
    }
}

/// How a file is shown: the color of its name in the tree and tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcsStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflict,
    Ignored,
}

impl VcsStatus {
    pub fn of(status: &FileStatus) -> VcsStatus {
        if status.conflict {
            return VcsStatus::Conflict;
        }
        if status.untracked {
            return VcsStatus::Untracked;
        }
        let kinds = [status.staged, status.unstaged];
        if kinds.contains(&Some(ChangeKind::Deleted)) {
            VcsStatus::Deleted
        } else if status.staged == Some(ChangeKind::Added) {
            VcsStatus::Added
        } else if status.staged == Some(ChangeKind::Renamed) {
            VcsStatus::Renamed
        } else {
            VcsStatus::Modified
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            VcsStatus::Modified => "Modified",
            VcsStatus::Added => "Added",
            VcsStatus::Deleted => "Deleted",
            VcsStatus::Renamed => "Renamed",
            VcsStatus::Untracked => "Unversioned",
            VcsStatus::Conflict => "Merge conflict",
            VcsStatus::Ignored => "Ignored",
        }
    }
}

/// A quick operation on the index or the working tree.
#[derive(Debug, Clone)]
pub enum QuickOp {
    Stage(Vec<PathBuf>),
    Unstage(Vec<PathBuf>),
    /// Tracked files back to HEAD; untracked files deleted.
    Rollback {
        tracked: Vec<PathBuf>,
        untracked: Vec<PathBuf>,
    },
    Hunk {
        diff: FileDiff,
        hunk: usize,
        action: HunkAction,
    },
    Accept {
        path: PathBuf,
        side: ConflictSide,
    },
}

impl QuickOp {
    fn describe(&self) -> String {
        let files = |n: usize| {
            if n == 1 {
                "1 file".to_string()
            } else {
                format!("{n} files")
            }
        };
        match self {
            QuickOp::Stage(p) => format!("Staged {}", files(p.len())),
            QuickOp::Unstage(p) => format!("Unstaged {}", files(p.len())),
            QuickOp::Rollback { tracked, untracked } => {
                format!("Rolled back {}", files(tracked.len() + untracked.len()))
            }
            QuickOp::Hunk { action, .. } => match action {
                HunkAction::Stage => "Staged the change".into(),
                HunkAction::Unstage => "Unstaged the change".into(),
                HunkAction::Revert => "Reverted the change".into(),
            },
            QuickOp::Accept { side, .. } => match side {
                ConflictSide::Ours => "Kept your version".into(),
                ConflictSide::Theirs => "Took their version".into(),
            },
        }
    }

    /// Touches files on disk (open buffers must reload).
    pub fn changes_disk(&self) -> bool {
        matches!(
            self,
            QuickOp::Rollback { .. }
                | QuickOp::Accept { .. }
                | QuickOp::Hunk {
                    action: HunkAction::Revert,
                    ..
                }
        )
    }
}

/// What kind of command finished (the app reacts to some).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    Commit,
    Push,
    Pull,
    Fetch,
    Checkout,
    Branch,
    Merge,
    Abort,
    Continue,
}

impl CommandKind {
    /// May change files on disk.
    pub fn changes_disk(self) -> bool {
        matches!(
            self,
            CommandKind::Pull
                | CommandKind::Checkout
                | CommandKind::Merge
                | CommandKind::Abort
                | CommandKind::Continue
        )
    }
}

/// One command in the Git console.
#[derive(Debug, Clone)]
pub struct ConsoleEntry {
    pub title: String,
    /// As typed in a terminal.
    pub command: String,
    pub kind: CommandKind,
    /// `(text, from stderr)`.
    pub lines: Vec<(String, bool)>,
    /// `None` while running; `Some(None)` when stopped.
    pub exit: Option<Option<i32>>,
}

impl ConsoleEntry {
    pub fn succeeded(&self) -> bool {
        self.exit == Some(Some(0))
    }

    /// The most telling line of the output (for notifications).
    pub fn summary(&self) -> String {
        let interesting = self.lines.iter().rev().map(|(l, _)| l.trim()).find(|l| {
            let lower = l.to_lowercase();
            lower.starts_with("error")
                || lower.starts_with("fatal")
                || lower.contains("conflict")
                || lower.contains("rejected")
        });
        interesting
            .or_else(|| {
                self.lines
                    .iter()
                    .rev()
                    .map(|(l, _)| l.trim())
                    .find(|l| !l.is_empty())
            })
            .unwrap_or("")
            .to_string()
    }
}

/// What to compare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffRequest {
    /// Relative to the worktree root.
    pub path: PathBuf,
    /// Before a rename.
    pub old_path: Option<PathBuf>,
    pub from: Side,
    pub to: Side,
}

/// Both sides of a file and the hunks between them.
#[derive(Debug, Clone)]
pub struct DiffData {
    pub request: DiffRequest,
    pub toplevel: PathBuf,
    /// `None`: the file does not exist on that side.
    pub old_text: Option<String>,
    pub new_text: Option<String>,
    /// `None` when both sides are equal.
    pub diff: Option<FileDiff>,
    pub binary: bool,
    pub too_large: bool,
}

/// Details of one commit.
#[derive(Debug, Clone)]
pub struct CommitDetails {
    pub sha: String,
    pub message: String,
    pub files: Vec<CommitFile>,
}

/// Something the app has to react to.
#[derive(Debug)]
pub enum VcsEvent {
    /// A quick operation finished.
    OpDone {
        summary: String,
        changed_disk: bool,
    },
    OpFailed {
        message: String,
        detail: String,
    },
    CommandFinished {
        kind: CommandKind,
        title: String,
        ok: bool,
        summary: String,
    },
    DiffLoaded {
        id: u64,
        data: Result<DiffData, String>,
    },
    LogLoaded {
        id: u64,
        skip: usize,
        entries: Result<Vec<CommitEntry>, String>,
    },
    DetailsLoaded {
        id: u64,
        details: Result<CommitDetails, String>,
    },
}

/// What to do after a command, depending on how it went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Followup {
    None,
    /// Commit and Push: push only when the commit went through.
    PushIfOk,
}

type Requested<T> = (u64, JobHandle<T>);

pub struct VcsService {
    root: PathBuf,
    snapshot: Option<Snapshot>,
    /// The project is not in a repository (or Git is missing).
    not_repo: bool,
    status_map: HashMap<PathBuf, VcsStatus>,
    /// Folders holding changed files (RF-NAV-26).
    changed_dirs: std::collections::HashSet<PathBuf>,
    snapshot_job: Option<JobHandle<Option<Snapshot>>>,
    snapshot_due: bool,
    last_snapshot: Option<Instant>,
    op_job: Option<(QuickOp, JobHandle<()>)>,
    ops: VecDeque<QuickOp>,
    pub console: Vec<ConsoleEntry>,
    running: Option<(StreamingChild, Followup)>,
    queue: VecDeque<(GitCommand, CommandKind, Followup)>,
    diff_jobs: Vec<Requested<DiffData>>,
    log_jobs: Vec<(u64, usize, JobHandle<Vec<CommitEntry>>)>,
    details_jobs: Vec<Requested<CommitDetails>>,
    events: Vec<VcsEvent>,
}

impl VcsService {
    pub fn new(root: PathBuf) -> Self {
        VcsService {
            root,
            snapshot: None,
            not_repo: false,
            status_map: HashMap::new(),
            changed_dirs: Default::default(),
            snapshot_job: None,
            snapshot_due: true,
            last_snapshot: None,
            op_job: None,
            ops: VecDeque::new(),
            console: Vec::new(),
            running: None,
            queue: VecDeque::new(),
            diff_jobs: Vec::new(),
            log_jobs: Vec::new(),
            details_jobs: Vec::new(),
            events: Vec::new(),
        }
    }

    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.snapshot.as_ref()
    }

    /// The project is known not to be in a Git repository.
    pub fn not_repo(&self) -> bool {
        self.not_repo
    }

    /// Worktree root, once known.
    pub fn toplevel(&self) -> Option<&Path> {
        self.snapshot.as_ref().map(|s| s.toplevel.as_path())
    }

    /// Asks for a fresh snapshot on the next poll.
    pub fn refresh(&mut self) {
        self.snapshot_due = true;
    }

    /// A quick operation or a command is running.
    pub fn busy(&self) -> bool {
        self.op_job.is_some() || !self.ops.is_empty() || self.running.is_some()
    }

    /// Title of the command running now.
    pub fn running_title(&self) -> Option<&str> {
        self.running
            .as_ref()
            .and_then(|_| self.console.last())
            .map(|e| e.title.as_str())
    }

    /// Status of an absolute path (`None` = unchanged).
    pub fn status_of(&self, path: &Path) -> Option<VcsStatus> {
        if let Some(status) = self.status_map.get(path) {
            return Some(*status);
        }
        let snapshot = self.snapshot.as_ref()?;
        let rel = path.strip_prefix(&snapshot.toplevel).ok()?;
        snapshot
            .ignored
            .iter()
            .any(|ignored| {
                let text = ignored.to_string_lossy();
                match text.strip_suffix('/') {
                    Some(dir) => rel.starts_with(dir),
                    None => rel == ignored,
                }
            })
            .then_some(VcsStatus::Ignored)
    }

    /// A folder holds changed files.
    pub fn dir_changed(&self, dir: &Path) -> bool {
        self.changed_dirs.contains(dir)
    }

    pub fn run_op(&mut self, op: QuickOp) {
        self.ops.push_back(op);
    }

    /// Queues a command; it runs after the ones before it.
    pub fn run_command(&mut self, command: GitCommand, kind: CommandKind) {
        self.queue.push_back((command, kind, Followup::None));
    }

    /// Commit, then push when the commit went through.
    pub fn commit_and_push(&mut self, command: GitCommand) {
        self.queue
            .push_back((command, CommandKind::Commit, Followup::PushIfOk));
    }

    /// Stops the running command (and drops the queued ones).
    pub fn stop(&mut self) {
        self.queue.clear();
        if let Some((child, _)) = &self.running {
            child.stop();
        }
    }

    pub fn clear_console(&mut self) {
        if self.running.is_some() {
            let last = self.console.pop();
            self.console.clear();
            self.console.extend(last);
        } else {
            self.console.clear();
        }
    }

    pub fn request_diff(
        &mut self,
        jobs: &JobSystem,
        ctx: &egui::Context,
        id: u64,
        request: DiffRequest,
    ) {
        let Some(toplevel) = self.toplevel().map(Path::to_path_buf) else {
            self.events.push(VcsEvent::DiffLoaded {
                id,
                data: Err("The project is not in a Git repository".into()),
            });
            return;
        };
        self.diff_jobs.retain(|(i, _)| *i != id);
        let ctx = ctx.clone();
        let job = jobs.spawn(move |_| {
            let result = load_diff(&toplevel, request);
            ctx.request_repaint();
            result
        });
        self.diff_jobs.push((id, job));
    }

    pub fn request_log(
        &mut self,
        jobs: &JobSystem,
        ctx: &egui::Context,
        id: u64,
        filter: LogFilter,
    ) {
        let Some(toplevel) = self.toplevel().map(Path::to_path_buf) else {
            return;
        };
        self.log_jobs.retain(|(i, _, _)| *i != id);
        let ctx = ctx.clone();
        let skip = filter.skip;
        let job = jobs.spawn(move |_| {
            let result = phpide_vcs::log(&toplevel, &filter);
            ctx.request_repaint();
            result
        });
        self.log_jobs.push((id, skip, job));
    }

    pub fn request_details(&mut self, jobs: &JobSystem, ctx: &egui::Context, id: u64, sha: String) {
        let Some(toplevel) = self.toplevel().map(Path::to_path_buf) else {
            return;
        };
        self.details_jobs.retain(|(i, _)| *i != id);
        let ctx = ctx.clone();
        let job = jobs.spawn(move |_| {
            let message = phpide_vcs::commit_message(&toplevel, &sha)?;
            let files = phpide_vcs::commit_files(&toplevel, &sha)?;
            ctx.request_repaint();
            Ok(CommitDetails {
                sha,
                message,
                files,
            })
        });
        self.details_jobs.push((id, job));
    }

    /// Message of the last commit (Amend). Runs on the caller's thread: a
    /// single short `git log -1`.
    pub fn last_commit_message(&self) -> Option<String> {
        phpide_vcs::last_commit_message(self.toplevel()?).ok()
    }

    /// Builds a command that needs the repository (commit, push).
    pub fn build<T>(
        &self,
        make: impl FnOnce(&Path) -> Result<T, phpide_core::IdeError>,
    ) -> Result<T, String> {
        let toplevel = self
            .toplevel()
            .ok_or_else(|| "The project is not in a Git repository".to_string())?;
        make(toplevel).map_err(|e| e.message)
    }

    /// Drives everything; call once per frame. Returns what the app must
    /// react to.
    pub fn poll(&mut self, jobs: &JobSystem, ctx: &egui::Context, focused: bool) -> Vec<VcsEvent> {
        self.poll_snapshot(jobs, ctx, focused);
        self.poll_ops(jobs, ctx);
        self.poll_command(ctx);
        self.poll_requests();
        std::mem::take(&mut self.events)
    }

    fn poll_snapshot(&mut self, jobs: &JobSystem, ctx: &egui::Context, focused: bool) {
        if let Some(job) = &mut self.snapshot_job {
            if let Some(outcome) = job.poll() {
                match outcome {
                    JobOutcome::Ok(Some(snapshot)) => {
                        self.status_map = snapshot
                            .changes
                            .iter()
                            .map(|s| (snapshot.toplevel.join(&s.path), VcsStatus::of(s)))
                            .collect();
                        self.changed_dirs = self
                            .status_map
                            .keys()
                            .flat_map(|path| path.ancestors().skip(1))
                            .filter(|dir| dir.starts_with(&snapshot.toplevel))
                            .map(Path::to_path_buf)
                            .collect();
                        self.snapshot = Some(snapshot);
                        self.not_repo = false;
                    }
                    JobOutcome::Ok(None) | JobOutcome::Err(_) => {
                        self.snapshot = None;
                        self.status_map.clear();
                        self.changed_dirs.clear();
                        self.not_repo = true;
                    }
                    JobOutcome::Cancelled => {}
                }
                self.snapshot_job = None;
            }
            return;
        }
        let stale = self
            .last_snapshot
            .is_none_or(|at| at.elapsed() >= SNAPSHOT_REFRESH);
        if !(self.snapshot_due || (focused && stale && !self.not_repo)) {
            return;
        }
        // Not while something is changing the repository.
        if self.op_job.is_some() || self.running.is_some() {
            return;
        }
        self.snapshot_due = false;
        self.last_snapshot = Some(Instant::now());
        let root = self.root.clone();
        let ctx = ctx.clone();
        self.snapshot_job = Some(jobs.spawn(move |_| {
            let result = take_snapshot(&root);
            ctx.request_repaint();
            Ok(result)
        }));
    }

    fn poll_ops(&mut self, jobs: &JobSystem, ctx: &egui::Context) {
        if let Some((op, job)) = &mut self.op_job {
            if let Some(outcome) = job.poll() {
                match outcome {
                    JobOutcome::Ok(()) => self.events.push(VcsEvent::OpDone {
                        summary: op.describe(),
                        changed_disk: op.changes_disk(),
                    }),
                    JobOutcome::Err(e) => self.events.push(VcsEvent::OpFailed {
                        message: e.message.clone(),
                        detail: e.detail.clone().unwrap_or_default(),
                    }),
                    JobOutcome::Cancelled => {}
                }
                self.op_job = None;
                self.snapshot_due = true;
            } else {
                return;
            }
        }
        let Some(op) = self.ops.pop_front() else {
            return;
        };
        let Some(toplevel) = self.toplevel().map(Path::to_path_buf) else {
            self.ops.clear();
            return;
        };
        let ctx = ctx.clone();
        let work = op.clone();
        let job = jobs.spawn(move |_| {
            let result = run_quick(&toplevel, work);
            ctx.request_repaint();
            result
        });
        self.op_job = Some((op, job));
    }

    fn poll_command(&mut self, ctx: &egui::Context) {
        if let Some((child, followup)) = &self.running {
            let followup = *followup;
            let mut exit = None;
            if let Some(entry) = self.console.last_mut() {
                for event in child.drain() {
                    match event {
                        StreamEvent::Line(line) => entry.lines.push((line, false)),
                        StreamEvent::ErrLine(line) => entry.lines.push((line, true)),
                        StreamEvent::Exit(code) => exit = Some(code),
                    }
                }
            }
            if let Some(code) = exit {
                self.running = None;
                self.snapshot_due = true;
                if let Some(entry) = self.console.last_mut() {
                    entry.exit = Some(code);
                    let ok = entry.succeeded();
                    self.events.push(VcsEvent::CommandFinished {
                        kind: entry.kind,
                        title: entry.title.clone(),
                        ok,
                        summary: entry.summary(),
                    });
                    if ok && followup == Followup::PushIfOk {
                        match self.build(phpide_vcs::commands::push) {
                            Ok(push) => {
                                self.queue
                                    .push_front((push, CommandKind::Push, Followup::None))
                            }
                            Err(message) => self.events.push(VcsEvent::OpFailed {
                                message,
                                detail: String::new(),
                            }),
                        }
                    }
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }
        }
        if self.op_job.is_some() {
            return;
        }
        let Some((command, kind, followup)) = self.queue.pop_front() else {
            return;
        };
        let Some(toplevel) = self.toplevel().map(Path::to_path_buf) else {
            self.queue.clear();
            return;
        };
        let mut spec = CommandSpec::new("git", command.full_args()).cwd(&toplevel);
        for (key, value) in &command.env {
            spec = spec.env(key.clone(), value.clone());
        }
        let mut entry = ConsoleEntry {
            title: command.title.clone(),
            command: command.display(),
            kind,
            lines: Vec::new(),
            exit: None,
        };
        match spawn_stream(spec) {
            Ok(child) => {
                self.console.push(entry);
                self.running = Some((child, followup));
                ctx.request_repaint();
            }
            Err(e) => {
                entry.lines.push((e.message.clone(), true));
                entry.exit = Some(None);
                self.console.push(entry);
                self.events.push(VcsEvent::CommandFinished {
                    kind,
                    title: command.title,
                    ok: false,
                    summary: e.message,
                });
            }
        }
        // Keep the console bounded.
        if self.console.len() > 50 {
            self.console.remove(0);
        }
    }

    fn poll_requests(&mut self) {
        let mut i = 0;
        while i < self.diff_jobs.len() {
            if let Some(outcome) = self.diff_jobs[i].1.poll() {
                let id = self.diff_jobs[i].0;
                self.diff_jobs.remove(i);
                let data = match outcome {
                    JobOutcome::Ok(data) => Ok(data),
                    JobOutcome::Err(e) => Err(e.message),
                    JobOutcome::Cancelled => continue,
                };
                self.events.push(VcsEvent::DiffLoaded { id, data });
            } else {
                i += 1;
            }
        }
        let mut i = 0;
        while i < self.log_jobs.len() {
            if let Some(outcome) = self.log_jobs[i].2.poll() {
                let (id, skip, _) = self.log_jobs.remove(i);
                let entries = match outcome {
                    JobOutcome::Ok(entries) => Ok(entries),
                    JobOutcome::Err(e) => Err(e.message),
                    JobOutcome::Cancelled => continue,
                };
                self.events.push(VcsEvent::LogLoaded { id, skip, entries });
            } else {
                i += 1;
            }
        }
        let mut i = 0;
        while i < self.details_jobs.len() {
            if let Some(outcome) = self.details_jobs[i].1.poll() {
                let (id, _) = self.details_jobs.remove(i);
                let details = match outcome {
                    JobOutcome::Ok(details) => Ok(details),
                    JobOutcome::Err(e) => Err(e.message),
                    JobOutcome::Cancelled => continue,
                };
                self.events.push(VcsEvent::DetailsLoaded { id, details });
            } else {
                i += 1;
            }
        }
    }

    /// Waits for every job and command (tests).
    #[cfg(test)]
    pub fn idle(&self) -> bool {
        self.snapshot_job.is_none()
            && self.op_job.is_none()
            && self.ops.is_empty()
            && self.running.is_none()
            && self.queue.is_empty()
            && self.diff_jobs.is_empty()
            && self.log_jobs.is_empty()
            && self.details_jobs.is_empty()
    }
}

fn take_snapshot(root: &Path) -> Option<Snapshot> {
    let toplevel = phpide_vcs::worktree_root(root)?;
    let changes = phpide_vcs::status(&toplevel).ok()?;
    Some(Snapshot {
        ignored: phpide_vcs::ignored(&toplevel).unwrap_or_default(),
        branches: phpide_vcs::branches(&toplevel).unwrap_or_default(),
        operation: phpide_vcs::operation(&toplevel).ok().flatten(),
        changes,
        toplevel,
    })
}

fn run_quick(toplevel: &Path, op: QuickOp) -> Result<(), phpide_core::IdeError> {
    match op {
        QuickOp::Stage(paths) => phpide_vcs::stage_paths(toplevel, &paths),
        QuickOp::Unstage(paths) => phpide_vcs::unstage_paths(toplevel, &paths),
        QuickOp::Rollback { tracked, untracked } => {
            phpide_vcs::rollback(toplevel, &tracked)?;
            for path in untracked {
                let abs = toplevel.join(&path);
                // To the Trash when there is one, so a rollback can be undone.
                super::file_ops::delete(&abs, true).map_err(|e| {
                    phpide_core::IdeError::new(
                        phpide_core::Component::Git,
                        "rollback",
                        phpide_core::ErrorKind::Io,
                        format!("could not delete {}", path.display()),
                        "check the file permissions",
                    )
                    .with_detail(e.to_string())
                })?;
            }
            Ok(())
        }
        QuickOp::Hunk { diff, hunk, action } => {
            phpide_vcs::apply_hunk(toplevel, &diff, hunk, action)
        }
        QuickOp::Accept { path, side } => phpide_vcs::accept_side(toplevel, &path, side),
    }
}

fn load_diff(toplevel: &Path, request: DiffRequest) -> Result<DiffData, phpide_core::IdeError> {
    let old_path = request.old_path.as_deref().unwrap_or(&request.path);
    let old = phpide_vcs::file_at(toplevel, &request.from, old_path)?;
    let new = phpide_vcs::file_at(toplevel, &request.to, &request.path)?;
    let too_large = [&old, &new]
        .iter()
        .any(|side| side.as_ref().is_some_and(|b| b.len() > MAX_DIFF_BYTES));
    let binary = [&old, &new].iter().any(|side| {
        side.as_ref()
            .is_some_and(|b| b[..b.len().min(8000)].contains(&0))
    });
    let diff = if too_large || binary {
        None
    } else {
        phpide_vcs::diff_between(
            toplevel,
            &request.from,
            &request.to,
            &request.path,
            request.old_path.as_deref(),
        )?
    };
    let text = |bytes: Option<Vec<u8>>| bytes.map(|b| String::from_utf8_lossy(&b).into_owned());
    Ok(DiffData {
        toplevel: toplevel.to_path_buf(),
        old_text: if too_large || binary {
            old.map(|_| String::new())
        } else {
            text(old)
        },
        new_text: if too_large || binary {
            new.map(|_| String::new())
        } else {
            text(new)
        },
        diff,
        binary,
        too_large,
        request,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn wait(service: &mut VcsService, jobs: &JobSystem, ctx: &egui::Context) -> Vec<VcsEvent> {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut events = Vec::new();
        loop {
            events.extend(service.poll(jobs, ctx, false));
            if service.idle() && !service.snapshot_due {
                return events;
            }
            assert!(Instant::now() < deadline, "git jobs did not finish");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn snapshot_ops_commands_and_diffs_run_in_the_background() {
        if std::process::Command::new("git")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "Ana"]);
        git(&root, &["config", "user.email", "ana@example.com"]);
        std::fs::write(root.join(".gitignore"), "vendor/\n").unwrap();
        std::fs::write(root.join("a.php"), "one\ntwo\n").unwrap();
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "base"]);
        std::fs::write(root.join("a.php"), "one\n2\n").unwrap();
        std::fs::write(root.join("b.php"), "new\n").unwrap();
        std::fs::create_dir_all(root.join("vendor/x")).unwrap();

        let jobs = JobSystem::new();
        let ctx = egui::Context::default();
        let mut vcs = VcsService::new(root.clone());
        wait(&mut vcs, &jobs, &ctx);
        let top = vcs.toplevel().unwrap().to_path_buf();
        assert_eq!(vcs.status_of(&top.join("a.php")), Some(VcsStatus::Modified));
        assert_eq!(
            vcs.status_of(&top.join("b.php")),
            Some(VcsStatus::Untracked)
        );
        std::fs::write(root.join("vendor/x/y.php"), "").unwrap();
        vcs.refresh();
        wait(&mut vcs, &jobs, &ctx);
        assert_eq!(
            vcs.status_of(&top.join("vendor/x/y.php")),
            Some(VcsStatus::Ignored)
        );
        assert_eq!(
            vcs.snapshot().unwrap().current_branch().unwrap().name,
            "main"
        );

        vcs.request_diff(
            &jobs,
            &ctx,
            7,
            DiffRequest {
                path: "a.php".into(),
                old_path: None,
                from: Side::Index,
                to: Side::WorkTree,
            },
        );
        let events = wait(&mut vcs, &jobs, &ctx);
        let data = events
            .into_iter()
            .find_map(|e| match e {
                VcsEvent::DiffLoaded { id: 7, data } => Some(data.unwrap()),
                _ => None,
            })
            .unwrap();
        assert_eq!(data.old_text.as_deref(), Some("one\ntwo\n"));
        assert_eq!(data.diff.unwrap().hunks.len(), 1);

        vcs.run_op(QuickOp::Stage(vec!["a.php".into(), "b.php".into()]));
        let events = wait(&mut vcs, &jobs, &ctx);
        assert!(events
            .iter()
            .any(|e| matches!(e, VcsEvent::OpDone { summary, .. } if summary == "Staged 2 files")));
        let staged = vcs
            .snapshot()
            .unwrap()
            .changes
            .iter()
            .filter(|c| c.staged.is_some())
            .count();
        assert_eq!(staged, 2);

        let commit = vcs
            .build(|t| phpide_vcs::commands::commit(t, "Second", false))
            .unwrap();
        vcs.run_command(commit, CommandKind::Commit);
        let events = wait(&mut vcs, &jobs, &ctx);
        assert!(events.iter().any(|e| matches!(
            e,
            VcsEvent::CommandFinished {
                kind: CommandKind::Commit,
                ok: true,
                ..
            }
        )));
        assert!(vcs
            .console
            .last()
            .unwrap()
            .command
            .starts_with("git commit"));
        assert!(vcs.snapshot().unwrap().changes.is_empty());

        vcs.request_log(
            &jobs,
            &ctx,
            1,
            LogFilter {
                max_count: 10,
                ..Default::default()
            },
        );
        let events = wait(&mut vcs, &jobs, &ctx);
        assert!(events.iter().any(
            |e| matches!(e, VcsEvent::LogLoaded { id: 1, entries: Ok(list), .. } if list.len() == 2)
        ));
    }
}
