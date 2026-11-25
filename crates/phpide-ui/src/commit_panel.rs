//! Commit tool window (Alt+0), in the staging-area model: Merge Conflicts,
//! Staged, Changes and Unversioned files; stage and unstage per file (the
//! diff tabs do it per hunk); rollback; the message with Amend; Commit and
//! Commit and Push; the output of the commit (hooks included) right below.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use eframe::egui;
use phpide_icons::{file_icon, IconId};

use crate::app_vcs::{status_color, GitDialog, GitDialogKind};
use crate::design::components::{Button, Checkbox, IconButton, TreeRow};
use crate::services::vcs::{
    ChangeKind, ConflictSide, DiffRequest, FileStatus, QuickOp, Side, VcsStatus,
};
use crate::PhpIdeApp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Group {
    Conflicts,
    Staged,
    Changes,
    Unversioned,
}

impl Group {
    fn title(self) -> &'static str {
        match self {
            Group::Conflicts => "Merge Conflicts",
            Group::Staged => "Staged",
            Group::Changes => "Changes",
            Group::Unversioned => "Unversioned Files",
        }
    }
}

#[derive(Default)]
pub(crate) struct CommitState {
    pub message: String,
    pub amend: bool,
    /// The message before Amend filled in the last one.
    message_before_amend: Option<String>,
    pub selected: BTreeSet<(Group, PathBuf)>,
    collapsed: HashSet<Group>,
    /// Console entry of the last commit, shown under the buttons.
    output: Option<usize>,
    pub focus_message: bool,
}

impl CommitState {
    pub(crate) fn watch_output(&mut self, console_index: usize) {
        self.output = Some(console_index);
    }

    /// A commit went through: start over.
    pub(crate) fn committed(&mut self) {
        self.message.clear();
        self.amend = false;
        self.message_before_amend = None;
        self.selected.clear();
    }
}

/// The files of each group, in order.
pub(crate) fn groups(changes: &[FileStatus]) -> Vec<(Group, Vec<&FileStatus>)> {
    let mut out: Vec<(Group, Vec<&FileStatus>)> = Vec::new();
    for group in [
        Group::Conflicts,
        Group::Staged,
        Group::Changes,
        Group::Unversioned,
    ] {
        let mut files: Vec<&FileStatus> = changes
            .iter()
            .filter(|s| match group {
                Group::Conflicts => s.conflict,
                Group::Staged => !s.conflict && s.staged.is_some(),
                Group::Changes => !s.conflict && s.unstaged.is_some(),
                Group::Unversioned => s.untracked,
            })
            .collect();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        if !files.is_empty() {
            out.push((group, files));
        }
    }
    out
}

/// Status of a file as shown in one group.
fn shown_status(group: Group, status: &FileStatus) -> VcsStatus {
    let kind = match group {
        Group::Conflicts => return VcsStatus::Conflict,
        Group::Unversioned => return VcsStatus::Untracked,
        Group::Staged => status.staged,
        Group::Changes => status.unstaged,
    };
    match kind {
        Some(ChangeKind::Added) => VcsStatus::Added,
        Some(ChangeKind::Deleted) => VcsStatus::Deleted,
        Some(ChangeKind::Renamed) => VcsStatus::Renamed,
        _ => VcsStatus::Modified,
    }
}

/// What to compare for a file of a group.
fn diff_request(group: Group, status: &FileStatus) -> DiffRequest {
    let (from, to) = match group {
        Group::Staged => (Side::Head, Side::Index),
        Group::Conflicts => (Side::Head, Side::WorkTree),
        Group::Changes | Group::Unversioned => (Side::Index, Side::WorkTree),
    };
    DiffRequest {
        path: status.path.clone(),
        old_path: (group == Group::Staged)
            .then(|| status.orig_path.clone())
            .flatten(),
        from,
        to,
    }
}

enum Act {
    Diff(Group, FileStatus),
    Open(PathBuf),
    Stage(Vec<PathBuf>),
    Unstage(Vec<PathBuf>),
    Rollback(Vec<(Group, PathBuf)>),
    Accept(PathBuf, ConflictSide),
    Select(Group, PathBuf, bool),
    ToggleGroup(Group),
    Commit(bool),
    Refresh,
    Operation(bool),
    CopyPath(PathBuf),
    /// The row with keyboard focus this frame.
    Focused(Group, PathBuf),
}

pub(crate) fn show(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let p = crate::design::current(ui.ctx()).palette;
    let m = crate::design::current(ui.ctx()).metrics;
    if app.vcs.not_repo() {
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new("The project is not in a Git repository.").color(p.text_muted),
        );
        return;
    }
    let Some(snapshot) = app.vcs.snapshot().cloned() else {
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.spinner();
            ui.label("Reading the repository…");
        });
        return;
    };
    let mut acts = Vec::new();
    let groups = groups(&snapshot.changes);
    let all_selected: Vec<(Group, PathBuf)> = app.commit_ui.selected.iter().cloned().collect();

    // Toolbar.
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        if IconButton::new(IconId::Refresh, "Refresh")
            .show(ui)
            .clicked()
        {
            acts.push(Act::Refresh);
        }
        let any = !all_selected.is_empty();
        if IconButton::new(IconId::Undo, "Rollback…")
            .shortcut("Ctrl+Alt+Z")
            .enabled(any)
            .show(ui)
            .clicked()
        {
            acts.push(Act::Rollback(all_selected.clone()));
        }
        let single = all_selected.first().cloned();
        if IconButton::new(IconId::Diff, "Show Diff")
            .shortcut("Ctrl+D")
            .enabled(single.is_some())
            .show(ui)
            .clicked()
        {
            if let Some((group, path)) = &single {
                if let Some(status) = snapshot.changes.iter().find(|s| &s.path == path) {
                    acts.push(Act::Diff(*group, status.clone()));
                }
            }
        }
        if app.vcs.busy() {
            ui.spinner();
        }
    });

    // A merge or rebase stopped half-way.
    if let Some(op) = snapshot.operation {
        egui::Frame::new()
            .fill(p.warning.gamma_multiply(0.12))
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let conflicts = snapshot.changes.iter().any(|s| s.conflict);
                    let text = if conflicts {
                        format!("{} — resolve the conflicts, then commit.", op.label())
                    } else if op == crate::services::vcs::RepoOperation::Merge {
                        format!("{} — commit to conclude the merge.", op.label())
                    } else {
                        format!("{} stopped.", op.label())
                    };
                    ui.label(text);
                    if op != crate::services::vcs::RepoOperation::Merge
                        && Button::new("Continue")
                            .enabled(!conflicts)
                            .show(ui)
                            .clicked()
                    {
                        acts.push(Act::Operation(false));
                    }
                    if Button::new("Abort").show(ui).clicked() {
                        acts.push(Act::Operation(true));
                    }
                });
            });
    }

    // Bottom part first: message, buttons and output keep their place.
    let staged_count = snapshot
        .changes
        .iter()
        .filter(|s| !s.conflict && s.staged.is_some())
        .count();
    let conflicts = snapshot.changes.iter().filter(|s| s.conflict).count();
    egui::Panel::bottom("commit-message")
        .resizable(true)
        .default_size(200.0)
        .min_size(140.0)
        .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 6)))
        .show(ui, |ui| {
            let amend_before = app.commit_ui.amend;
            ui.horizontal(|ui| {
                Checkbox::new(&mut app.commit_ui.amend, "Amend").show(ui);
                ui.label(
                    egui::RichText::new(match (staged_count, conflicts) {
                        (_, c) if c > 0 => {
                            format!("{c} conflict{} to resolve", if c == 1 { "" } else { "s" })
                        }
                        (0, _) => "Nothing staged".to_string(),
                        (1, _) => "1 file staged".to_string(),
                        (n, _) => format!("{n} files staged"),
                    })
                    .color(p.text_muted),
                );
            });
            if app.commit_ui.amend != amend_before {
                if app.commit_ui.amend {
                    // Amend starts from the last commit's message.
                    app.commit_ui.message_before_amend = Some(app.commit_ui.message.clone());
                    if let Some(last) = app.vcs.last_commit_message() {
                        app.commit_ui.message = last;
                    }
                } else if let Some(before) = app.commit_ui.message_before_amend.take() {
                    app.commit_ui.message = before;
                }
            }
            let can_commit = !app.commit_ui.message.trim().is_empty()
                && conflicts == 0
                && (staged_count > 0 || app.commit_ui.amend || snapshot.operation.is_some())
                && !app.vcs.busy();
            // Ctrl+Enter commits; taken before the field sees it as a newline.
            let message_id = ui.make_persistent_id("commit-message");
            if can_commit
                && ui.memory(|m| m.has_focus(message_id))
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter))
            {
                acts.push(Act::Commit(false));
            }
            let message = egui::TextEdit::multiline(&mut app.commit_ui.message)
                .id(message_id)
                .hint_text("Commit Message")
                .desired_rows(4)
                .desired_width(f32::INFINITY)
                .show(ui)
                .response;
            message.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Commit Message")
            });
            if std::mem::take(&mut app.commit_ui.focus_message) {
                message.request_focus();
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if Button::primary(if app.commit_ui.amend {
                    "Amend Commit"
                } else {
                    "Commit"
                })
                .enabled(can_commit)
                .show(ui)
                .on_hover_text("Ctrl+Enter")
                .clicked()
                {
                    acts.push(Act::Commit(false));
                }
                if Button::new("Commit and Push…")
                    .enabled(can_commit)
                    .show(ui)
                    .clicked()
                {
                    acts.push(Act::Commit(true));
                }
            });
            // Output of the last commit: hooks print here.
            if let Some(entry) = app.commit_ui.output.and_then(|i| app.vcs.console.get(i)) {
                ui.add_space(6.0);
                let (icon, color, state) = match entry.exit {
                    None => (IconId::Loader, p.text_secondary, "running…"),
                    Some(Some(0)) => (IconId::Success, p.success, "done"),
                    Some(_) => (IconId::Error, p.error, "failed"),
                };
                let mut close = false;
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(m.icon, m.icon), egui::Sense::hover());
                    crate::design::components::paint_icon(ui.painter(), rect, icon, color);
                    ui.label(egui::RichText::new(format!("{} — {state}", entry.title)).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if IconButton::new(IconId::Close, "Hide Output")
                            .show(ui)
                            .clicked()
                        {
                            close = true;
                        }
                    });
                });
                egui::ScrollArea::vertical()
                    .id_salt("commit-output")
                    .max_height(140.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        if entry.lines.is_empty() && entry.exit.is_some() {
                            ui.label(egui::RichText::new("(no output)").color(p.text_muted));
                        }
                        for (line, _err) in &entry.lines {
                            let lower = line.to_lowercase();
                            let color = if lower.contains("error")
                                || lower.contains("fatal")
                                || lower.contains("failed")
                            {
                                p.error
                            } else {
                                p.text_secondary
                            };
                            ui.label(egui::RichText::new(line).monospace().color(color));
                        }
                    });
                if close {
                    app.commit_ui.output = None;
                }
            }
        });

    // The changes.
    egui::ScrollArea::vertical()
        .id_salt("commit-changes")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if groups.is_empty() {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("No changes").color(p.text_muted));
                });
            }
            for (group, files) in &groups {
                let collapsed = app.commit_ui.collapsed.contains(group);
                let header = TreeRow::new(format!("{}  {}", group.title(), files.len()))
                    .expanded(Some(!collapsed))
                    .show(ui);
                let header_rect = header.response.rect;
                if header.toggled || header.response.clicked() {
                    acts.push(Act::ToggleGroup(*group));
                }
                // Group action on the header's right end.
                let paths: Vec<PathBuf> = files.iter().map(|s| s.path.clone()).collect();
                let (icon, label) = match group {
                    Group::Staged => (IconId::Minus, "Unstage All"),
                    Group::Conflicts => (IconId::Plus, "Mark All Resolved"),
                    _ => (IconId::Plus, "Stage All"),
                };
                let button_rect = egui::Rect::from_min_size(
                    egui::pos2(header_rect.right() - m.icon_button - 4.0, header_rect.top()),
                    egui::vec2(m.icon_button, header_rect.height()),
                );
                if *group != Group::Conflicts {
                    let clicked = ui
                        .scope_builder(egui::UiBuilder::new().max_rect(button_rect), |ui| {
                            IconButton::new(icon, label).show(ui).clicked()
                        })
                        .inner;
                    if clicked {
                        acts.push(if *group == Group::Staged {
                            Act::Unstage(paths.clone())
                        } else {
                            Act::Stage(paths.clone())
                        });
                    }
                }
                if collapsed {
                    continue;
                }
                for status in files {
                    file_row(app, ui, &snapshot.toplevel, *group, status, &mut acts);
                }
            }
        });

    // Keys on the list.
    let focused_row = acts.iter().find_map(|a| match a {
        Act::Focused(g, p) => Some((*g, p.clone())),
        _ => None,
    });
    if let Some((focus_group, focus_path)) = focused_row {
        let (enter, diff, space) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::D) && i.modifiers.command,
                i.key_pressed(egui::Key::Space),
            )
        });
        let target = if app
            .commit_ui
            .selected
            .contains(&(focus_group, focus_path.clone()))
        {
            Some((focus_group, focus_path))
        } else {
            app.commit_ui.selected.iter().next().cloned()
        };
        if let Some((group, path)) = target {
            let status = snapshot.changes.iter().find(|s| s.path == path).cloned();
            if let Some(status) = status {
                if enter || diff {
                    acts.push(Act::Diff(group, status));
                } else if space {
                    let paths: Vec<PathBuf> = app
                        .commit_ui
                        .selected
                        .iter()
                        .map(|(_, p)| p.clone())
                        .collect();
                    acts.push(if group == Group::Staged {
                        Act::Unstage(paths)
                    } else {
                        Act::Stage(paths)
                    });
                }
            }
        }
    }

    let ctx = ui.ctx().clone();
    for act in acts {
        match act {
            Act::Diff(group, status) => {
                let name = file_name(&status.path);
                let title = match group {
                    Group::Staged => format!("{name} (Staged)"),
                    _ => name,
                };
                app.save_all_quietly();
                app.open_diff(&ctx, diff_request(group, &status), title);
            }
            Act::Open(path) => {
                app.open_file_tab(&path, false);
            }
            Act::Stage(paths) => app.git_quick(QuickOp::Stage(paths)),
            Act::Unstage(paths) => app.git_quick(QuickOp::Unstage(paths)),
            Act::Rollback(items) => {
                let mut tracked = Vec::new();
                let mut untracked = Vec::new();
                for (group, path) in items {
                    let list = if group == Group::Unversioned {
                        &mut untracked
                    } else {
                        &mut tracked
                    };
                    if !list.contains(&path) {
                        list.push(path);
                    }
                }
                if !tracked.is_empty() || !untracked.is_empty() {
                    app.git_dialog = Some(GitDialog::new(GitDialogKind::Rollback {
                        tracked,
                        untracked,
                    }));
                }
            }
            Act::Accept(path, side) => app.git_quick(QuickOp::Accept { path, side }),
            Act::Select(group, path, add) => {
                if !add {
                    app.commit_ui.selected.clear();
                }
                let key = (group, path);
                if add && app.commit_ui.selected.contains(&key) {
                    app.commit_ui.selected.remove(&key);
                } else {
                    app.commit_ui.selected.insert(key);
                }
            }
            Act::ToggleGroup(group) => {
                if !app.commit_ui.collapsed.remove(&group) {
                    app.commit_ui.collapsed.insert(group);
                }
            }
            Act::Commit(push) => app.git_commit(push),
            Act::Refresh => app.vcs.refresh(),
            Act::Operation(abort) => app.git_operation(abort),
            Act::CopyPath(path) => ctx.copy_text(path.display().to_string()),
            Act::Focused(..) => {}
        }
    }
    // Forget selections of files that left their group.
    let present: HashSet<(Group, PathBuf)> = groups
        .iter()
        .flat_map(|(g, files)| files.iter().map(move |s| (*g, s.path.clone())))
        .collect();
    app.commit_ui.selected.retain(|k| present.contains(k));
}

fn row_id(group: Group, path: &Path) -> egui::Id {
    egui::Id::new(("commit-row", group, path))
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn file_row(
    app: &PhpIdeApp,
    ui: &mut egui::Ui,
    toplevel: &Path,
    group: Group,
    status: &FileStatus,
    acts: &mut Vec<Act>,
) {
    let theme = crate::design::current(ui.ctx());
    let p = theme.palette;
    let m = theme.metrics;
    let name = file_name(&status.path);
    let (icon, kind) = file_icon(&name);
    let shown = shown_status(group, status);
    let folder = status
        .path
        .parent()
        .map(|d| d.display().to_string())
        .unwrap_or_default();
    let detail = match &status.orig_path {
        Some(old) if group == Group::Staged => format!("{folder}  ← {}", old.display()),
        _ => folder,
    };
    let selected = app
        .commit_ui
        .selected
        .contains(&(group, status.path.clone()));
    let row = ui.push_id(row_id(group, &status.path), |ui| {
        TreeRow::new(&name)
            .depth(1)
            .icon(icon, p.file_color(kind))
            .label_color(status_color(&p, shown))
            .detail(detail)
            .selected(selected)
            .show(ui)
    });
    let response =
        row.inner
            .response
            .on_hover_text(format!("{} — {}", status.path.display(), shown.label()));
    if response.clicked() {
        let add = ui.input(|i| i.modifiers.command);
        acts.push(Act::Select(group, status.path.clone(), add));
        response.request_focus();
    }
    if response.has_focus() {
        acts.push(Act::Focused(group, status.path.clone()));
    }
    if response.gained_focus() && !selected {
        // Tab onto a row selects it: Space and Enter act on it.
        acts.push(Act::Select(group, status.path.clone(), false));
    }
    if response.double_clicked() {
        if group == Group::Conflicts {
            acts.push(Act::Open(toplevel.join(&status.path)));
        } else {
            acts.push(Act::Diff(group, status.clone()));
        }
    }
    // Stage / unstage on hover.
    if group != Group::Conflicts && (response.hovered() || selected) {
        let rect = egui::Rect::from_min_size(
            egui::pos2(
                response.rect.right() - m.icon_button - 4.0,
                response.rect.top(),
            ),
            egui::vec2(m.icon_button, response.rect.height()),
        );
        let (icon, label) = if group == Group::Staged {
            (IconId::Minus, "Unstage")
        } else {
            (IconId::Plus, "Stage")
        };
        let clicked = ui
            .scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                IconButton::new(icon, format!("{label} {name}"))
                    .show(ui)
                    .clicked()
            })
            .inner;
        if clicked {
            acts.push(if group == Group::Staged {
                Act::Unstage(vec![status.path.clone()])
            } else {
                Act::Stage(vec![status.path.clone()])
            });
        }
    }
    let path = status.path.clone();
    let abs = toplevel.join(&status.path);
    response.context_menu(|ui| {
        ui.set_min_width(200.0);
        if group == Group::Conflicts {
            if ui.button("Accept Yours").clicked() {
                acts.push(Act::Accept(path.clone(), ConflictSide::Ours));
                ui.close();
            }
            if ui.button("Accept Theirs").clicked() {
                acts.push(Act::Accept(path.clone(), ConflictSide::Theirs));
                ui.close();
            }
            if ui.button("Mark Resolved").clicked() {
                acts.push(Act::Stage(vec![path.clone()]));
                ui.close();
            }
            ui.separator();
        } else {
            if ui.button("Show Diff").clicked() {
                acts.push(Act::Diff(group, status.clone()));
                ui.close();
            }
            let stage_label = if group == Group::Staged {
                "Unstage"
            } else {
                "Stage"
            };
            if ui.button(stage_label).clicked() {
                acts.push(if group == Group::Staged {
                    Act::Unstage(vec![path.clone()])
                } else {
                    Act::Stage(vec![path.clone()])
                });
                ui.close();
            }
            if ui.button("Rollback…").clicked() {
                acts.push(Act::Rollback(vec![(group, path.clone())]));
                ui.close();
            }
        }
        if abs.exists() && ui.button("Jump to Source").clicked() {
            acts.push(Act::Open(abs.clone()));
            ui.close();
        }
        if ui.button("Copy Path").clicked() {
            acts.push(Act::CopyPath(abs.clone()));
            ui.close();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(path: &str, staged: Option<ChangeKind>, unstaged: Option<ChangeKind>) -> FileStatus {
        FileStatus {
            path: path.into(),
            staged,
            unstaged,
            untracked: false,
            conflict: false,
            orig_path: None,
        }
    }

    #[test]
    fn a_file_can_be_staged_and_changed_at_once() {
        let mut list = vec![
            status(
                "b.php",
                Some(ChangeKind::Modified),
                Some(ChangeKind::Modified),
            ),
            status("a.php", None, Some(ChangeKind::Deleted)),
            status("n.php", Some(ChangeKind::Added), None),
        ];
        let mut new = status("u.txt", None, None);
        new.untracked = true;
        list.push(new);
        let mut conflict = status("c.php", Some(ChangeKind::Other), Some(ChangeKind::Other));
        conflict.conflict = true;
        list.push(conflict);
        let groups = groups(&list);
        let names: Vec<(Group, Vec<&str>)> = groups
            .iter()
            .map(|(g, f)| (*g, f.iter().map(|s| s.path.to_str().unwrap()).collect()))
            .collect();
        assert_eq!(
            names,
            [
                (Group::Conflicts, vec!["c.php"]),
                (Group::Staged, vec!["b.php", "n.php"]),
                (Group::Changes, vec!["a.php", "b.php"]),
                (Group::Unversioned, vec!["u.txt"]),
            ]
        );
        assert_eq!(shown_status(Group::Changes, &list[1]), VcsStatus::Deleted);
        assert_eq!(shown_status(Group::Staged, &list[2]), VcsStatus::Added);
        assert_eq!(diff_request(Group::Staged, &list[0]).from, Side::Head);
        assert_eq!(diff_request(Group::Changes, &list[0]).to, Side::WorkTree);
    }
}
