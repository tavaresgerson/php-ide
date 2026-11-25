//! Git actions of the window: what the Commit and Version Control tool
//! windows, the diff tabs, the branch popup and the Git commands ask for,
//! routed to the Git service, and what the app does when they finish.

use std::path::{Path, PathBuf};

use eframe::egui;

use crate::design::components::{Button, Checkbox, Dialog, TextInput};
use crate::design::tokens::Palette;
use crate::services::vcs::{
    git_commands, CommandKind, DiffRequest, GitCommand, QuickOp, Side, VcsEvent, VcsStatus,
};
use crate::toolwindows::ToolWindow;
use crate::PhpIdeApp;

/// Name color of a file with Git changes (RF-NAV-25, RF-TAB-12).
pub(crate) fn status_color(p: &Palette, status: VcsStatus) -> egui::Color32 {
    match status {
        VcsStatus::Modified | VcsStatus::Renamed => p.git_modified,
        VcsStatus::Added => p.git_added,
        VcsStatus::Deleted => p.git_deleted,
        VcsStatus::Untracked => p.git_untracked,
        VcsStatus::Conflict => p.git_conflict,
        VcsStatus::Ignored => p.git_ignored,
    }
}

/// A small Git dialog (branch name, confirmation).
pub(crate) enum GitDialogKind {
    NewBranch {
        start: Option<String>,
    },
    RenameBranch {
        old: String,
    },
    DeleteBranch {
        name: String,
    },
    Rollback {
        tracked: Vec<PathBuf>,
        untracked: Vec<PathBuf>,
    },
}

pub(crate) struct GitDialog {
    pub kind: GitDialogKind,
    pub name: String,
    pub checkout: bool,
    pub force: bool,
    focus: bool,
}

impl GitDialog {
    pub(crate) fn new(kind: GitDialogKind) -> Self {
        let name = match &kind {
            GitDialogKind::RenameBranch { old } => old.clone(),
            _ => String::new(),
        };
        GitDialog {
            kind,
            name,
            checkout: true,
            force: false,
            focus: true,
        }
    }
}

impl PhpIdeApp {
    /// Color of a file name by its Git status.
    pub(crate) fn vcs_color(&self, p: &Palette, path: &Path) -> Option<egui::Color32> {
        self.vcs.status_of(path).map(|s| status_color(p, s))
    }

    /// Path relative to the repository root.
    pub(crate) fn repo_relative(&self, path: &Path) -> Option<PathBuf> {
        let top = self.vcs.toplevel()?;
        path.strip_prefix(top).ok().map(Path::to_path_buf)
    }

    pub(crate) fn poll_vcs(&mut self, ctx: &egui::Context) {
        let focused = ctx.input(|i| i.focused);
        let events = self.vcs.poll(&self.jobs, ctx, focused);
        let mut reload_diffs = false;
        for event in events {
            match event {
                VcsEvent::OpDone {
                    summary,
                    changed_disk,
                } => {
                    self.ws.message = summary;
                    if changed_disk {
                        self.reload_after_git(ctx);
                    }
                    reload_diffs = true;
                }
                VcsEvent::OpFailed { message, detail } => {
                    let detail = detail.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
                    self.ws.message = if detail.is_empty() {
                        format!("Git failed: {message}")
                    } else {
                        format!("Git failed: {message} — {}", detail.trim())
                    };
                    reload_diffs = true;
                }
                VcsEvent::CommandFinished {
                    kind,
                    title,
                    ok,
                    summary,
                } => {
                    self.ws.message = if ok {
                        match kind {
                            CommandKind::Commit => {
                                self.commit_ui.committed();
                                format!("Committed: {summary}")
                            }
                            _ if summary.is_empty() => format!("{title}: done"),
                            _ => format!("{title}: {summary}"),
                        }
                    } else if summary.is_empty() {
                        format!("{title} failed")
                    } else {
                        format!("{title} failed: {summary}")
                    };
                    if kind.changes_disk() {
                        self.reload_after_git(ctx);
                    }
                    self.git.refresh();
                    self.vcs_log.stale = true;
                    reload_diffs = true;
                }
                VcsEvent::DiffLoaded { id, data } => self.diffs.loaded(id, data),
                VcsEvent::LogLoaded { id, skip, entries } => self.vcs_log.loaded(id, skip, entries),
                VcsEvent::DetailsLoaded { id, details } => self.vcs_log.details_loaded(id, details),
            }
        }
        if reload_diffs {
            self.reload_diffs(ctx);
        }
    }

    /// Files changed on disk under Git's hands: reread the tree and the
    /// open (saved) buffers.
    fn reload_after_git(&mut self, ctx: &egui::Context) {
        self.files.refresh(&self.jobs, ctx);
        let paths: Vec<PathBuf> = self
            .ws
            .buffers
            .iter()
            .filter_map(|b| b.path.clone())
            .collect();
        self.ws.reload_clean_buffers(&paths);
    }

    /// Re-requests every open diff (the sides may have changed).
    pub(crate) fn reload_diffs(&mut self, ctx: &egui::Context) {
        for (id, request) in self.diffs.requests() {
            self.vcs.request_diff(&self.jobs, ctx, id, request);
        }
    }

    /// A quick Git operation; files on disk are saved first when it writes
    /// them.
    pub(crate) fn git_quick(&mut self, op: QuickOp) {
        if op.changes_disk() {
            self.save_all_quietly();
        }
        self.vcs.run_op(op);
    }

    /// Runs a command after saving the open files (Git reads them).
    pub(crate) fn git_command(&mut self, command: Result<GitCommand, String>, kind: CommandKind) {
        match command {
            Ok(command) => {
                self.save_all_quietly();
                self.vcs.run_command(command, kind);
            }
            Err(message) => self.ws.message = format!("Git: {message}"),
        }
    }

    /// Commits the staged changes (and pushes when `push`).
    pub(crate) fn git_commit(&mut self, push: bool) {
        let message = self.commit_ui.message.clone();
        let amend = self.commit_ui.amend;
        match self
            .vcs
            .build(|top| git_commands::commit(top, &message, amend))
        {
            Ok(command) => {
                self.save_all_quietly();
                self.commit_ui.watch_output(self.vcs.console.len());
                if push {
                    self.vcs.commit_and_push(command);
                } else {
                    self.vcs.run_command(command, CommandKind::Commit);
                }
            }
            Err(message) => self.ws.message = format!("Commit failed: {message}"),
        }
    }

    pub(crate) fn git_push(&mut self) {
        let command = self.vcs.build(git_commands::push);
        self.git_command(command, CommandKind::Push);
    }

    pub(crate) fn git_update(&mut self) {
        let command = self.vcs.build(|top| Ok(git_commands::pull(top)));
        self.git_command(command, CommandKind::Pull);
    }

    pub(crate) fn git_fetch(&mut self) {
        let command = self.vcs.build(|top| Ok(git_commands::fetch(top)));
        self.git_command(command, CommandKind::Fetch);
    }

    pub(crate) fn git_checkout(&mut self, name: &str, remote: bool) {
        self.git_command(
            Ok(git_commands::checkout(name, remote)),
            CommandKind::Checkout,
        );
    }

    pub(crate) fn git_merge(&mut self, name: &str) {
        self.git_command(Ok(git_commands::merge(name)), CommandKind::Merge);
    }

    /// Abort or continue the merge/rebase in progress.
    pub(crate) fn git_operation(&mut self, abort: bool) {
        let Some(op) = self.vcs.snapshot().and_then(|s| s.operation) else {
            return;
        };
        if abort {
            self.git_command(Ok(git_commands::abort(op)), CommandKind::Abort);
        } else {
            self.git_command(Ok(git_commands::continue_op(op)), CommandKind::Continue);
        }
    }

    /// Opens a diff tab (or focuses the one already showing it).
    pub(crate) fn open_diff(&mut self, ctx: &egui::Context, request: DiffRequest, title: String) {
        let active_buffer = self.ws.active_buffer().map(|b| b.id);
        let (id, new) = self.diffs.open(request.clone(), title, active_buffer);
        if new {
            self.vcs.request_diff(&self.jobs, ctx, id, request);
        }
    }

    /// Compare with HEAD for the active file.
    pub(crate) fn compare_active_with_head(&mut self, ctx: &egui::Context) {
        let Some(path) = self.ws.active_buffer().and_then(|b| b.path.clone()) else {
            self.ws.message = "Open a file to compare it".into();
            return;
        };
        let Some(rel) = self.repo_relative(&path) else {
            self.ws.message = "The file is not in a Git repository".into();
            return;
        };
        self.save_all_quietly();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.open_diff(
            ctx,
            DiffRequest {
                path: rel,
                old_path: None,
                from: Side::Head,
                to: Side::WorkTree,
            },
            format!("{name} (HEAD)"),
        );
    }

    /// Show History for the active file: the log filtered by its path.
    pub(crate) fn show_file_history(&mut self) {
        let Some(path) = self.ws.active_buffer().and_then(|b| b.path.clone()) else {
            self.ws.message = "Open a file to see its history".into();
            return;
        };
        let Some(rel) = self.repo_relative(&path) else {
            self.ws.message = "The file is not in a Git repository".into();
            return;
        };
        self.vcs_log.show_path(rel);
        self.vcs_log.tab = crate::vcs_panel::VcsTab::Log;
        self.tw.set_visible(ToolWindow::VersionControl, true);
    }

    /// Rollback of the active file (when no Commit selection asks).
    pub(crate) fn rollback_active(&mut self) {
        let Some(path) = self.ws.active_buffer().and_then(|b| b.path.clone()) else {
            return;
        };
        let Some(rel) = self.repo_relative(&path) else {
            return;
        };
        let untracked = self.vcs.status_of(&path) == Some(VcsStatus::Untracked);
        if self.vcs.status_of(&path).is_none() {
            self.ws.message = "No changes to roll back".into();
            return;
        }
        self.git_dialog = Some(GitDialog::new(GitDialogKind::Rollback {
            tracked: if untracked {
                Vec::new()
            } else {
                vec![rel.clone()]
            },
            untracked: if untracked { vec![rel] } else { Vec::new() },
        }));
    }

    /// The Git dialog, when one is open.
    pub(crate) fn git_dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.git_dialog.as_mut() else {
            return;
        };
        let p = crate::design::current(ctx).palette;
        let mut close = false;
        let mut confirm = false;
        let (title, action) = match &dialog.kind {
            GitDialogKind::NewBranch { .. } => ("Create New Branch".to_string(), "Create"),
            GitDialogKind::RenameBranch { old } => (format!("Rename Branch '{old}'"), "Rename"),
            GitDialogKind::DeleteBranch { name } => (format!("Delete Branch '{name}'"), "Delete"),
            GitDialogKind::Rollback { .. } => ("Rollback Changes".to_string(), "Rollback"),
        };
        let id = egui::Id::new("git-dialog");
        egui::Modal::new(id)
            .area(
                egui::Modal::default_area(id)
                    .fixed_pos(ctx.content_rect().center() - egui::vec2(210.0, 110.0)),
            )
            .backdrop_color(p.shadow)
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                Dialog::frame(ui).show(ui, |ui| {
                    ui.set_width(420.0);
                    ui.label(egui::RichText::new(&title).strong());
                    ui.add_space(8.0);
                    match &dialog.kind {
                        GitDialogKind::NewBranch { start } => {
                            if let Some(start) = start {
                                ui.label(egui::RichText::new(format!("From {start}")).weak());
                            }
                            let field = TextInput::new(&mut dialog.name)
                                .hint("Branch name")
                                .width(400.0)
                                .id_salt("git-branch-name")
                                .show(ui);
                            if std::mem::take(&mut dialog.focus) {
                                field.request_focus();
                            }
                            Checkbox::new(&mut dialog.checkout, "Checkout branch").show(ui);
                        }
                        GitDialogKind::RenameBranch { .. } => {
                            let field = TextInput::new(&mut dialog.name)
                                .hint("New name")
                                .width(400.0)
                                .id_salt("git-branch-name")
                                .show(ui);
                            if std::mem::take(&mut dialog.focus) {
                                field.request_focus();
                            }
                        }
                        GitDialogKind::DeleteBranch { .. } => {
                            ui.label("The branch is deleted locally; commits that are not on another branch become unreachable.");
                            Checkbox::new(&mut dialog.force, "Delete even if it is not merged").show(ui);
                        }
                        GitDialogKind::Rollback { tracked, untracked } => {
                            let count = tracked.len() + untracked.len();
                            ui.label(if count == 1 {
                                "The changes to this file will be lost:".to_string()
                            } else {
                                format!("The changes to these {count} files will be lost:")
                            });
                            egui::ScrollArea::vertical()
                                .id_salt("rollback-files")
                                .max_height(140.0)
                                .show(ui, |ui| {
                                    for path in tracked.iter().chain(untracked) {
                                        ui.label(egui::RichText::new(path.display().to_string()).monospace());
                                    }
                                });
                            if !untracked.is_empty() {
                                ui.label(
                                    egui::RichText::new("Unversioned files go to the Trash.")
                                        .color(p.text_muted),
                                );
                            }
                        }
                    }
                    let needs_name = matches!(
                        dialog.kind,
                        GitDialogKind::NewBranch { .. } | GitDialogKind::RenameBranch { .. }
                    );
                    let name_ok = !needs_name || valid_branch_name(dialog.name.trim());
                    if needs_name && !dialog.name.trim().is_empty() && !name_ok {
                        ui.colored_label(p.error, "Not a valid branch name");
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if Button::primary(action).enabled(name_ok && (!needs_name || !dialog.name.trim().is_empty())).show(ui).clicked() {
                            confirm = true;
                        }
                        if Button::new("Cancel").show(ui).clicked() {
                            close = true;
                        }
                    });
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        close = true;
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Enter))
                        && name_ok
                        && (!needs_name || !dialog.name.trim().is_empty())
                    {
                        confirm = true;
                    }
                });
            });
        if confirm {
            let dialog = self.git_dialog.take().unwrap();
            let name = dialog.name.trim().to_string();
            match dialog.kind {
                GitDialogKind::NewBranch { start } => self.git_command(
                    Ok(git_commands::new_branch(
                        &name,
                        start.as_deref(),
                        dialog.checkout,
                    )),
                    CommandKind::Branch,
                ),
                GitDialogKind::RenameBranch { old } => self.git_command(
                    Ok(git_commands::rename_branch(&old, &name)),
                    CommandKind::Branch,
                ),
                GitDialogKind::DeleteBranch { name } => self.git_command(
                    Ok(git_commands::delete_branch(&name, dialog.force)),
                    CommandKind::Branch,
                ),
                GitDialogKind::Rollback { tracked, untracked } => {
                    self.git_quick(QuickOp::Rollback { tracked, untracked })
                }
            }
        } else if close {
            self.git_dialog = None;
        }
    }
}

/// The rules of `git check-ref-format --branch`, enough to catch typos.
pub(crate) fn valid_branch_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.starts_with('/')
        && !name.ends_with('/')
        && !name.ends_with(".lock")
        && !name.ends_with('.')
        && !name.contains("..")
        && !name.contains("//")
        && !name.contains("@{")
        && name != "@"
        && !name
            .chars()
            .any(|c| c.is_control() || " ~^:?*[\\".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_names_follow_git_rules() {
        for ok in ["main", "feature/login", "fix-12", "release/1.2"] {
            assert!(valid_branch_name(ok), "{ok}");
        }
        for bad in [
            "", "-x", "a..b", "a b", "x.lock", "a~1", "a:b", "x/", "@", "a@{1}",
        ] {
            assert!(!valid_branch_name(bad), "{bad}");
        }
    }
}
