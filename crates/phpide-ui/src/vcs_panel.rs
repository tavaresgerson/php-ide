//! Version Control tool window (Alt+9): the Log (branches, commit graph,
//! filters, commit details with their changed files) and the Git console
//! (every command the IDE ran, with its output). Also the branch popup of
//! the header.

use std::path::PathBuf;

use eframe::egui;
use phpide_icons::{file_icon, IconId};

use crate::app_vcs::{GitDialog, GitDialogKind};
use crate::design::components::{
    menu_item, menu_separator, menu_submenu, IconButton, Select, Tab, TextInput, TreeRow,
};
use crate::services::vcs::{
    Branch, ChangeKind, CommitDetails, CommitEntry, DiffRequest, LogFilter, Side,
};
use crate::toolwindows::ToolWindow;
use crate::PhpIdeApp;

/// Commits per page of the log.
const PAGE: usize = 300;
/// The tree with no files (the parent of a root commit).
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum VcsTab {
    #[default]
    Log,
    Console,
}

/// One row of the commit graph.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GraphRow {
    /// Lane of the commit's dot.
    pub node: usize,
    /// Lines in the upper half: `(lane at the top, lane at the middle)`.
    pub top: Vec<(usize, usize)>,
    /// Lines in the lower half: `(lane at the middle, lane at the bottom)`.
    pub bottom: Vec<(usize, usize)>,
    pub merge: bool,
}

/// Lanes of a commit graph, newest first (the order of `git log`).
pub(crate) fn graph(entries: &[CommitEntry]) -> Vec<GraphRow> {
    let mut lanes: Vec<Option<String>> = Vec::new();
    let mut rows = Vec::with_capacity(entries.len());
    for entry in entries {
        let hits: Vec<usize> = lanes
            .iter()
            .enumerate()
            .filter(|(_, l)| l.as_deref() == Some(entry.sha.as_str()))
            .map(|(i, _)| i)
            .collect();
        let node = match hits.first() {
            Some(&i) => i,
            None => {
                let free = lanes
                    .iter()
                    .position(Option::is_none)
                    .unwrap_or(lanes.len());
                if free == lanes.len() {
                    lanes.push(None);
                }
                free
            }
        };
        let top: Vec<(usize, usize)> = lanes
            .iter()
            .enumerate()
            .filter_map(|(i, l)| {
                let sha = l.as_deref()?;
                Some(if sha == entry.sha { (i, node) } else { (i, i) })
            })
            .collect();
        for &h in hits.iter().skip(1) {
            lanes[h] = None;
        }
        lanes[node] = entry.parents.first().cloned();
        let mut joins = Vec::new();
        for parent in entry.parents.iter().skip(1) {
            if let Some(k) = lanes
                .iter()
                .position(|l| l.as_deref() == Some(parent.as_str()))
            {
                joins.push(k);
            } else {
                let free = lanes
                    .iter()
                    .position(Option::is_none)
                    .unwrap_or(lanes.len());
                if free == lanes.len() {
                    lanes.push(None);
                }
                lanes[free] = Some(parent.clone());
                joins.push(free);
            }
        }
        let bottom: Vec<(usize, usize)> = lanes
            .iter()
            .enumerate()
            .filter_map(|(i, l)| {
                l.as_ref()?;
                Some(if i == node || joins.contains(&i) {
                    (node, i)
                } else {
                    (i, i)
                })
            })
            .collect();
        while lanes.last().is_some_and(Option::is_none) {
            lanes.pop();
        }
        rows.push(GraphRow {
            node,
            top,
            bottom,
            merge: entry.parents.len() > 1,
        });
    }
    rows
}

/// Filters as typed; a change asks for a new log.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct FilterKey {
    text: String,
    author: String,
    branch: Option<String>,
    path: Option<PathBuf>,
}

#[derive(Default)]
pub(crate) struct LogState {
    pub tab: VcsTab,
    entries: Vec<CommitEntry>,
    graph: Vec<GraphRow>,
    text: String,
    author: String,
    /// Index into the branch options (0 = all, 1 = HEAD).
    branch: usize,
    path: Option<PathBuf>,
    loaded: Option<FilterKey>,
    request: u64,
    loading: bool,
    exhausted: bool,
    error: Option<String>,
    /// Something changed the repository: reload when visible.
    pub stale: bool,
    pub selected: Option<usize>,
    details: Option<CommitDetails>,
    details_request: u64,
    details_for: Option<String>,
    selected_file: Option<usize>,
}

impl LogState {
    pub(crate) fn show_path(&mut self, path: PathBuf) {
        self.path = Some(path);
        self.branch = 0;
        self.selected = None;
    }

    pub(crate) fn loaded(
        &mut self,
        id: u64,
        skip: usize,
        entries: Result<Vec<CommitEntry>, String>,
    ) {
        if id != self.request {
            return;
        }
        self.loading = false;
        match entries {
            Ok(list) => {
                self.exhausted = list.len() < PAGE;
                if skip == 0 {
                    self.entries = list;
                } else {
                    self.entries.extend(list);
                }
                self.graph = graph(&self.entries);
                self.error = None;
                // Keep the selected commit selected after a reload.
                if let Some(sha) = &self.details_for {
                    self.selected = self.entries.iter().position(|e| &e.sha == sha);
                }
            }
            Err(e) => self.error = Some(e),
        }
    }

    pub(crate) fn details_loaded(&mut self, id: u64, details: Result<CommitDetails, String>) {
        if id == self.details_request {
            self.details = details.ok();
            self.selected_file = None;
        }
    }

    #[cfg(test)]
    pub(crate) fn entries(&self) -> &[CommitEntry] {
        &self.entries
    }

    #[cfg(test)]
    pub(crate) fn details(&self) -> Option<&CommitDetails> {
        self.details.as_ref()
    }
}

/// Branch filter options: All, HEAD, then every branch.
fn branch_options(branches: &[Branch]) -> Vec<String> {
    let mut out = vec!["All branches".to_string(), "HEAD".to_string()];
    out.extend(branches.iter().map(|b| b.name.clone()));
    out
}

pub(crate) fn show(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let p = crate::design::current(ui.ctx()).palette;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (tab, label) in [(VcsTab::Log, "Log"), (VcsTab::Console, "Console")] {
            let shown = Tab::new(label)
                .active(app.vcs_log.tab == tab)
                .closable(false)
                .show(ui);
            if shown.response.clicked() {
                app.vcs_log.tab = tab;
            }
        }
        if let Some(title) = app.vcs.running_title() {
            ui.add_space(12.0);
            ui.spinner();
            ui.label(egui::RichText::new(format!("{title}…")).color(p.text_secondary));
        }
    });
    if app.vcs.not_repo() {
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new("The project is not in a Git repository.").color(p.text_muted),
        );
        return;
    }
    match app.vcs_log.tab {
        VcsTab::Log => log_tab(app, ui),
        VcsTab::Console => console_tab(app, ui),
    }
}

fn log_tab(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let theme = crate::design::current(ui.ctx());
    let p = theme.palette;
    let m = theme.metrics;
    let ctx = ui.ctx().clone();
    let branches: Vec<Branch> = app
        .vcs
        .snapshot()
        .map(|s| s.branches.clone())
        .unwrap_or_default();
    let options = branch_options(&branches);
    let log = &mut app.vcs_log;
    if log.branch >= options.len() {
        log.branch = 0;
    }
    // Filters.
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        TextInput::search(&mut log.text)
            .hint("Text or hash")
            .width(200.0)
            .id_salt("log-text")
            .show(ui);
        TextInput::new(&mut log.author)
            .hint("Author")
            .width(130.0)
            .id_salt("log-author")
            .show(ui);
        let refs: Vec<&str> = options.iter().map(String::as_str).collect();
        Select::new("log-branch", &mut log.branch, &refs)
            .width(170.0)
            .show(ui);
        if let Some(path) = &log.path {
            ui.label(egui::RichText::new(format!("Path: {}", path.display())).color(p.accent));
            if IconButton::new(IconId::Close, "Clear Path Filter")
                .show(ui)
                .clicked()
            {
                log.path = None;
            }
        }
        if IconButton::new(IconId::Refresh, "Refresh")
            .show(ui)
            .clicked()
        {
            log.stale = true;
        }
    });
    let key = FilterKey {
        text: log.text.trim().to_string(),
        author: log.author.trim().to_string(),
        branch: match log.branch {
            0 => None,
            1 => Some("HEAD".into()),
            n => options.get(n).cloned(),
        },
        path: log.path.clone(),
    };
    let reload = log.loaded.as_ref() != Some(&key) || log.stale;
    let mut request: Option<LogFilter> = None;
    if reload && !(log.loading && !log.stale && log.loaded.as_ref() == Some(&key)) {
        log.stale = false;
        log.loaded = Some(key.clone());
        log.loading = true;
        log.exhausted = false;
        log.request += 1;
        request = Some(filter_for(&key, 0));
    }
    let request_id = log.request;
    if let Some(filter) = request {
        app.vcs.request_log(&app.jobs, &ctx, request_id, filter);
    }
    let log = &mut app.vcs_log;
    let mut acts: Vec<LogAct> = Vec::new();

    // Details on the right, branches on the left, commits in the middle.
    egui::Panel::right("log-details")
        .resizable(true)
        .default_size(320.0)
        .min_size(220.0)
        .show(ui, |ui| details_view(log, ui, &mut acts));
    egui::Panel::left("log-branches")
        .resizable(true)
        .default_size(190.0)
        .min_size(140.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("log-branch-list")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let mut pick = |ui: &mut egui::Ui,
                                    label: &str,
                                    index: usize,
                                    depth: usize,
                                    branch: Option<&Branch>,
                                    acts: &mut Vec<LogAct>| {
                        let mut row = TreeRow::new(label)
                            .depth(depth)
                            .selected(log.branch == index);
                        if let Some(b) = branch {
                            row = row.icon(
                                IconId::GitBranch,
                                if b.current {
                                    p.accent
                                } else {
                                    p.text_secondary
                                },
                            );
                            let mut detail = String::new();
                            if b.behind > 0 {
                                detail.push_str(&format!("↓{} ", b.behind));
                            }
                            if b.ahead > 0 {
                                detail.push_str(&format!("↑{}", b.ahead));
                            }
                            if !detail.is_empty() {
                                row = row.detail(detail.trim().to_string());
                            }
                        }
                        let response = row.show(ui).response;
                        if response.clicked() {
                            log.branch = index;
                        }
                        if let Some(b) = branch {
                            branch_context_menu(&response, b, acts);
                        }
                    };
                    pick(ui, "All branches", 0, 0, None, &mut acts);
                    pick(ui, "HEAD", 1, 0, None, &mut acts);
                    for (title, remote) in [("Local", false), ("Remote", true)] {
                        let list: Vec<(usize, &Branch)> = branches
                            .iter()
                            .enumerate()
                            .filter(|(_, b)| b.remote == remote)
                            .collect();
                        if list.is_empty() {
                            continue;
                        }
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(title).small().color(p.text_muted));
                        for (i, b) in list {
                            pick(ui, &b.name, i + 2, 1, Some(b), &mut acts);
                        }
                    }
                });
        });
    // Commit table.
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ui, |ui| {
            if let Some(error) = &log.error {
                ui.colored_label(p.error, error);
            }
            if log.entries.is_empty() {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(if log.loading {
                        "Loading…"
                    } else {
                        "No commits"
                    })
                    .color(p.text_muted),
                );
                return;
            }
            let row_h = m.row;
            let lanes = log
                .graph
                .iter()
                .map(|g| g.node.max(g.bottom.iter().map(|b| b.1).max().unwrap_or(0)))
                .max()
                .unwrap_or(0)
                .min(12)
                + 1;
            let graph_w = lanes as f32 * 12.0 + 10.0;
            let total = log.entries.len();
            let mut near_end = false;
            egui::ScrollArea::vertical()
                .id_salt("log-commits")
                .auto_shrink([false, false])
                .show_rows(ui, row_h, total, |ui, range| {
                    near_end = range.end + 20 >= total;
                    // Rows touch: the graph's lines run from row to row.
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let width = ui.available_width();
                    // Narrow tables give the subject the room first.
                    let date_w = if width > 460.0 { 120.0 } else { 0.0 };
                    let author_w = if width > 720.0 {
                        130.0
                    } else if width > 560.0 {
                        90.0
                    } else {
                        0.0
                    };
                    for i in range {
                        let entry = &log.entries[i];
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(width, row_h), egui::Sense::click());
                        response.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::SelectableLabel,
                                true,
                                log.selected == Some(i),
                                &entry.subject,
                            )
                        });
                        let selected = log.selected == Some(i);
                        if selected {
                            ui.painter().rect_filled(rect, 0.0, p.bg_selected);
                        } else if response.hovered() {
                            ui.painter().rect_filled(rect, 0.0, p.bg_hover);
                        }
                        paint_graph(ui, rect, &log.graph[i], graph_w, &p);
                        // Refs, then the subject.
                        let mut x = rect.left() + graph_w;
                        for r in &entry.refs {
                            let (label, color) = if let Some(tag) = r.strip_prefix("tag: ") {
                                (tag.to_string(), p.warning)
                            } else if let Some(head) = r.strip_prefix("HEAD -> ") {
                                (head.to_string(), p.accent)
                            } else if r == "HEAD" {
                                ("HEAD".to_string(), p.accent)
                            } else {
                                (r.clone(), p.success)
                            };
                            let galley = ui.painter().layout_no_wrap(
                                label,
                                egui::FontId::proportional(11.0),
                                color,
                            );
                            let chip = egui::Rect::from_min_size(
                                egui::pos2(x, rect.center().y - galley.size().y / 2.0 - 2.0),
                                galley.size() + egui::vec2(10.0, 4.0),
                            );
                            ui.painter().rect_stroke(
                                chip,
                                3.0,
                                egui::Stroke::new(1.0, color.gamma_multiply(0.7)),
                                egui::StrokeKind::Inside,
                            );
                            ui.painter()
                                .galley(chip.min + egui::vec2(5.0, 2.0), galley, color);
                            x = chip.right() + 4.0;
                        }
                        let subject_right = rect.right() - author_w - date_w;
                        let clip = egui::Rect::from_min_max(
                            egui::pos2(x, rect.top()),
                            egui::pos2(subject_right - 8.0, rect.bottom()),
                        );
                        ui.painter().with_clip_rect(clip).text(
                            egui::pos2(x + 2.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            &entry.subject,
                            egui::FontId::proportional(13.0),
                            p.text_primary,
                        );
                        if author_w > 0.0 {
                            ui.painter()
                                .with_clip_rect(egui::Rect::from_min_max(
                                    egui::pos2(subject_right, rect.top()),
                                    egui::pos2(subject_right + author_w - 8.0, rect.bottom()),
                                ))
                                .text(
                                    egui::pos2(subject_right, rect.center().y),
                                    egui::Align2::LEFT_CENTER,
                                    &entry.author,
                                    egui::FontId::proportional(12.0),
                                    p.text_secondary,
                                );
                        }
                        if date_w > 0.0 {
                            ui.painter().text(
                                egui::pos2(rect.right() - 8.0, rect.center().y),
                                egui::Align2::RIGHT_CENTER,
                                format_time(entry.timestamp),
                                egui::FontId::proportional(12.0),
                                p.text_muted,
                            );
                        }
                        if response.clicked() {
                            acts.push(LogAct::Select(i));
                        }
                        let sha = entry.sha.clone();
                        let short = entry.short.clone();
                        response.context_menu(|ui| {
                            ui.set_min_width(200.0);
                            if menu_item(ui, Some(IconId::Copy), "Copy Revision Number", "", true)
                                .clicked()
                            {
                                acts.push(LogAct::Copy(sha.clone()));
                                ui.close();
                            }
                            if menu_item(ui, Some(IconId::GitBranch), "New Branch…", "", true)
                                .clicked()
                            {
                                acts.push(LogAct::NewBranch(Some(short.clone())));
                                ui.close();
                            }
                        });
                    }
                });
            if near_end && !log.exhausted && !log.loading {
                if let Some(key) = log.loaded.clone() {
                    log.loading = true;
                    log.request += 1;
                    acts.push(LogAct::More(
                        filter_for(&key, log.entries.len()),
                        log.request,
                    ));
                }
            }
        });
    // Keys: Up/Down move the selection.
    if app.vcs_log.selected.is_some() && ui.memory(|mem| mem.focused().is_none()) {
        let (up, down) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
            )
        });
        if let Some(sel) = app.vcs_log.selected {
            if up && sel > 0 {
                acts.push(LogAct::Select(sel - 1));
            } else if down && sel + 1 < app.vcs_log.entries.len() {
                acts.push(LogAct::Select(sel + 1));
            }
        }
    }
    run_log_acts(app, &ctx, acts);
}

fn filter_for(key: &FilterKey, skip: usize) -> LogFilter {
    LogFilter {
        max_count: PAGE,
        skip,
        text: (!key.text.is_empty()).then(|| key.text.clone()),
        author: (!key.author.is_empty()).then(|| key.author.clone()),
        all: key.branch.is_none(),
        rev: key.branch.clone(),
        path: key.path.clone(),
        without_files: true,
    }
}

enum LogAct {
    Select(usize),
    More(LogFilter, u64),
    Copy(String),
    NewBranch(Option<String>),
    FileDiff(usize),
    OpenFile(PathBuf),
    Checkout(String, bool),
    Merge(String),
    Rename(String),
    Delete(String),
    Push,
    Update,
}

fn branch_context_menu(response: &egui::Response, b: &Branch, acts: &mut Vec<LogAct>) {
    response.context_menu(|ui| {
        ui.set_min_width(220.0);
        branch_actions(ui, b, acts);
    });
}

/// Actions of one branch (context menu and branch popup).
fn branch_actions(ui: &mut egui::Ui, b: &Branch, acts: &mut Vec<LogAct>) {
    let mut item = |ui: &mut egui::Ui, icon: Option<IconId>, label: &str, act: LogAct| {
        if menu_item(ui, icon, label, "", true).clicked() {
            acts.push(act);
            ui.close();
        }
    };
    if b.current {
        item(ui, Some(IconId::Update), "Update", LogAct::Update);
        item(ui, Some(IconId::Push), "Push…", LogAct::Push);
    } else {
        item(
            ui,
            Some(IconId::GitBranch),
            "Checkout",
            LogAct::Checkout(b.name.clone(), b.remote),
        );
    }
    item(
        ui,
        Some(IconId::Plus),
        format!("New Branch from '{}'…", b.name).as_str(),
        LogAct::NewBranch(Some(b.name.clone())),
    );
    if !b.current {
        item(
            ui,
            Some(IconId::Merge),
            format!("Merge '{}' into Current", b.name).as_str(),
            LogAct::Merge(b.name.clone()),
        );
    }
    if !b.remote {
        menu_separator(ui);
        item(
            ui,
            Some(IconId::Edit),
            "Rename…",
            LogAct::Rename(b.name.clone()),
        );
        if !b.current {
            item(
                ui,
                Some(IconId::Trash),
                "Delete",
                LogAct::Delete(b.name.clone()),
            );
        }
    }
}

fn run_log_acts(app: &mut PhpIdeApp, ctx: &egui::Context, acts: Vec<LogAct>) {
    for act in acts {
        match act {
            LogAct::Select(i) => {
                let Some(entry) = app.vcs_log.entries.get(i) else {
                    continue;
                };
                app.vcs_log.selected = Some(i);
                if app.vcs_log.details_for.as_deref() != Some(entry.sha.as_str()) {
                    let sha = entry.sha.clone();
                    app.vcs_log.details_for = Some(sha.clone());
                    app.vcs_log.details = None;
                    app.vcs_log.details_request += 1;
                    let id = app.vcs_log.details_request;
                    app.vcs.request_details(&app.jobs, ctx, id, sha);
                }
            }
            LogAct::More(filter, id) => app.vcs.request_log(&app.jobs, ctx, id, filter),
            LogAct::Copy(text) => ctx.copy_text(text),
            LogAct::NewBranch(start) => {
                app.git_dialog = Some(GitDialog::new(GitDialogKind::NewBranch { start }))
            }
            LogAct::FileDiff(index) => {
                let (Some(details), Some(i)) = (&app.vcs_log.details, app.vcs_log.selected) else {
                    continue;
                };
                let Some(file) = details.files.get(index) else {
                    continue;
                };
                let entry = &app.vcs_log.entries[i];
                let parent = entry
                    .parents
                    .first()
                    .cloned()
                    .unwrap_or_else(|| EMPTY_TREE.to_string());
                let path = file.renamed_to.clone().unwrap_or_else(|| file.path.clone());
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let title = format!("{name} ({})", entry.short);
                let request = DiffRequest {
                    old_path: file.renamed_to.as_ref().map(|_| file.path.clone()),
                    path,
                    from: Side::Rev(parent),
                    to: Side::Rev(entry.sha.clone()),
                };
                app.open_diff(ctx, request, title);
            }
            LogAct::OpenFile(path) => {
                if let Some(top) = app.vcs.toplevel() {
                    let abs = top.join(path);
                    if abs.exists() {
                        app.open_file_tab(&abs, false);
                    } else {
                        app.ws.message = "The file is no longer in the project".into();
                    }
                }
            }
            LogAct::Checkout(name, remote) => app.git_checkout(&name, remote),
            LogAct::Merge(name) => app.git_merge(&name),
            LogAct::Rename(old) => {
                app.git_dialog = Some(GitDialog::new(GitDialogKind::RenameBranch { old }))
            }
            LogAct::Delete(name) => {
                app.git_dialog = Some(GitDialog::new(GitDialogKind::DeleteBranch { name }))
            }
            LogAct::Push => app.git_push(),
            LogAct::Update => app.git_update(),
        }
    }
}

fn details_view(log: &mut LogState, ui: &mut egui::Ui, acts: &mut Vec<LogAct>) {
    let p = crate::design::current(ui.ctx()).palette;
    let Some(i) = log.selected else {
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Select a commit to see its changes").color(p.text_muted));
        return;
    };
    let Some(entry) = log.entries.get(i) else {
        return;
    };
    egui::ScrollArea::vertical()
        .id_salt("log-details-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(6.0);
            match &log.details {
                Some(details) if details.sha == entry.sha => {
                    ui.add(
                        egui::Label::new(egui::RichText::new(&details.message).strong())
                            .selectable(true),
                    );
                }
                _ => {
                    ui.label(egui::RichText::new(&entry.subject).strong());
                }
            }
            ui.add_space(4.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!(
                        "{} · {} <{}> · {}",
                        entry.short,
                        entry.author,
                        entry.email,
                        format_time(entry.timestamp)
                    ))
                    .small()
                    .color(p.text_secondary),
                )
                .selectable(true),
            );
            ui.add_space(8.0);
            let Some(details) = log.details.as_ref().filter(|d| d.sha == entry.sha) else {
                ui.spinner();
                return;
            };
            ui.label(
                egui::RichText::new(format!(
                    "{} file{} changed",
                    details.files.len(),
                    if details.files.len() == 1 { "" } else { "s" }
                ))
                .small()
                .color(p.text_muted),
            );
            ui.spacing_mut().item_spacing.y = 0.0;
            for (k, file) in details.files.iter().enumerate() {
                let path = file.renamed_to.as_ref().unwrap_or(&file.path);
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let (icon, kind) = file_icon(&name);
                let color = match file.kind {
                    ChangeKind::Added => p.git_added,
                    ChangeKind::Deleted => p.git_deleted,
                    _ => p.git_modified,
                };
                let folder = path
                    .parent()
                    .map(|d| d.display().to_string())
                    .unwrap_or_default();
                let response = TreeRow::new(&name)
                    .icon(icon, p.file_color(kind))
                    .label_color(color)
                    .detail(folder)
                    .selected(log.selected_file == Some(k))
                    .show(ui)
                    .response
                    .on_hover_text(path.display().to_string());
                if response.clicked() {
                    log.selected_file = Some(k);
                    response.request_focus();
                }
                let enter = response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if response.double_clicked() || enter {
                    acts.push(LogAct::FileDiff(k));
                }
                let path = path.clone();
                response.context_menu(|ui| {
                    ui.set_min_width(180.0);
                    if menu_item(ui, Some(IconId::Diff), "Show Diff", "", true).clicked() {
                        acts.push(LogAct::FileDiff(k));
                        ui.close();
                    }
                    if menu_item(ui, Some(IconId::File), "Open File", "", true).clicked() {
                        acts.push(LogAct::OpenFile(path.clone()));
                        ui.close();
                    }
                });
            }
        });
}

fn paint_graph(
    ui: &egui::Ui,
    rect: egui::Rect,
    row: &GraphRow,
    width: f32,
    p: &crate::design::tokens::Palette,
) {
    let colors = [
        p.accent,
        p.success,
        p.warning,
        p.info,
        p.git_untracked,
        p.git_added,
        p.error,
    ];
    let x = |lane: usize| rect.left() + 10.0 + lane.min(12) as f32 * 12.0;
    let clip = egui::Rect::from_min_size(rect.min, egui::vec2(width, rect.height()));
    let painter = ui.painter().with_clip_rect(clip);
    let mid = rect.center().y;
    for &(a, b) in &row.top {
        painter.line_segment(
            [egui::pos2(x(a), rect.top()), egui::pos2(x(b), mid)],
            egui::Stroke::new(1.6, colors[a % colors.len()]),
        );
    }
    for &(a, b) in &row.bottom {
        painter.line_segment(
            [egui::pos2(x(a), mid), egui::pos2(x(b), rect.bottom())],
            egui::Stroke::new(1.6, colors[b % colors.len()]),
        );
    }
    let color = colors[row.node % colors.len()];
    let center = egui::pos2(x(row.node), mid);
    if row.merge {
        painter.circle(center, 3.8, p.bg_panel, egui::Stroke::new(1.6, color));
    } else {
        painter.circle_filled(center, 3.8, color);
    }
}

/// `2026-11-24 14:05` in local time.
pub(crate) fn format_time(epoch: i64) -> String {
    #[cfg(unix)]
    {
        let t = epoch as libc::time_t;
        // SAFETY: `localtime_r` writes into the `tm` we own; both pointers are valid.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        if unsafe { !libc::localtime_r(&t, &mut tm).is_null() } {
            return format!(
                "{:04}-{:02}-{:02} {:02}:{:02}",
                tm.tm_year + 1900,
                tm.tm_mon + 1,
                tm.tm_mday,
                tm.tm_hour,
                tm.tm_min
            );
        }
    }
    let days = epoch.div_euclid(86_400);
    format!("{days} days since 1970")
}

fn console_tab(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let p = crate::design::current(ui.ctx()).palette;
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        let running = app.vcs.running_title().is_some();
        if IconButton::new(IconId::Stop, "Stop")
            .enabled(running)
            .tint(p.error)
            .show(ui)
            .clicked()
        {
            app.vcs.stop();
        }
        if IconButton::new(IconId::Trash, "Clear All")
            .show(ui)
            .clicked()
        {
            app.vcs.clear_console();
        }
    });
    egui::ScrollArea::vertical()
        .id_salt("git-console")
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            if app.vcs.console.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "Commands the IDE runs for you appear here with their output.",
                    )
                    .color(p.text_muted),
                );
            }
            for entry in &app.vcs.console {
                let (mark, color) = match entry.exit {
                    None => ("…", p.text_secondary),
                    Some(Some(0)) => ("✓", p.success),
                    Some(Some(code)) => {
                        let _ = code;
                        ("✗", p.error)
                    }
                    Some(None) => ("■", p.warning),
                };
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(mark).color(color).strong());
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&entry.command)
                                .monospace()
                                .color(p.accent),
                        )
                        .selectable(true),
                    );
                });
                for (line, _) in &entry.lines {
                    let lower = line.to_lowercase();
                    let color = if lower.starts_with("error")
                        || lower.starts_with("fatal")
                        || lower.contains("conflict")
                        || lower.contains("rejected")
                    {
                        p.error
                    } else {
                        p.text_secondary
                    };
                    ui.add(
                        egui::Label::new(egui::RichText::new(line).monospace().color(color))
                            .selectable(true),
                    );
                }
                ui.add_space(6.0);
            }
        });
}

/// The branch popup of the header (and Git › Branches…).
pub(crate) fn branches_popup(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let p = crate::design::current(ui.ctx()).palette;
    ui.set_min_width(300.0);
    let mut acts = Vec::new();
    let snapshot = app.vcs.snapshot().cloned();
    if menu_item(ui, Some(IconId::Update), "Update Project…", "Ctrl+T", true).clicked() {
        acts.push(LogAct::Update);
        ui.close();
    }
    if menu_item(ui, Some(IconId::Commit), "Commit…", "Ctrl+K", true).clicked() {
        app.commit_ui.focus_message = true;
        app.tw.set_visible(ToolWindow::Commit, true);
        ui.close();
    }
    if menu_item(ui, Some(IconId::Push), "Push…", "Ctrl+Shift+K", true).clicked() {
        acts.push(LogAct::Push);
        ui.close();
    }
    if menu_item(ui, Some(IconId::Download), "Fetch", "", true).clicked() {
        app.git_fetch();
        ui.close();
    }
    if let Some(op) = snapshot.as_ref().and_then(|s| s.operation) {
        menu_separator(ui);
        if menu_item(
            ui,
            Some(IconId::Stop),
            &format!("Abort {}", op.subcommand()),
            "",
            true,
        )
        .clicked()
        {
            app.git_operation(true);
            ui.close();
        }
    }
    menu_separator(ui);
    if menu_item(ui, Some(IconId::Plus), "New Branch…", "", true).clicked() {
        acts.push(LogAct::NewBranch(None));
        ui.close();
    }
    let filter_id = egui::Id::new("branch-popup-filter");
    let mut filter: String = ui.data(|d| d.get_temp(filter_id)).unwrap_or_default();
    TextInput::search(&mut filter)
        .hint("Search branches")
        .width(280.0)
        .id_salt("branch-popup-search")
        .show(ui);
    ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
    let needle = filter.to_lowercase();
    let branches = snapshot.map(|s| s.branches).unwrap_or_default();
    for (title, remote) in [("Local", false), ("Remote", true)] {
        let list: Vec<&Branch> = branches
            .iter()
            .filter(|b| b.remote == remote && b.name.to_lowercase().contains(&needle))
            .collect();
        if list.is_empty() {
            continue;
        }
        ui.label(egui::RichText::new(title).small().color(p.text_muted));
        for b in list {
            let mut label = b.name.clone();
            if b.behind > 0 {
                label.push_str(&format!("  ↓{}", b.behind));
            }
            if b.ahead > 0 {
                label.push_str(&format!("  ↑{}", b.ahead));
            }
            // The current branch carries a check mark instead of the branch icon.
            let icon = if b.current {
                IconId::Check
            } else {
                IconId::GitBranch
            };
            menu_submenu(ui, Some(icon), &label, true, |ui| {
                branch_actions(ui, b, &mut acts)
            });
        }
    }
    let ctx = ui.ctx().clone();
    run_log_acts(app, &ctx, acts);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(sha: &str, parents: &[&str]) -> CommitEntry {
        CommitEntry {
            sha: sha.into(),
            short: sha.into(),
            author: String::new(),
            email: String::new(),
            timestamp: 0,
            subject: String::new(),
            files: Vec::new(),
            parents: parents.iter().map(|p| p.to_string()).collect(),
            refs: Vec::new(),
        }
    }

    #[test]
    fn graph_lanes_for_a_branch_and_its_merge() {
        // m merges f into b; f and b both come from a.
        let log = [
            commit("m", &["b", "f"]),
            commit("f", &["a"]),
            commit("b", &["a"]),
            commit("a", &[]),
        ];
        let rows = graph(&log);
        assert_eq!(rows[0].node, 0);
        assert!(rows[0].merge);
        // The merge opens a second lane for f.
        assert_eq!(rows[0].bottom, [(0, 0), (0, 1)]);
        assert_eq!(rows[1].node, 1);
        assert_eq!(rows[2].node, 0);
        // a closes both lanes.
        assert_eq!(rows[3].node, 0);
        assert_eq!(rows[3].top, [(0, 0), (1, 0)]);
        assert!(rows[3].bottom.is_empty());
    }

    #[test]
    fn linear_history_stays_in_one_lane() {
        let log = [commit("c", &["b"]), commit("b", &["a"]), commit("a", &[])];
        assert!(graph(&log).iter().all(|r| r.node == 0));
        assert_eq!(format_time(0).len(), 16);
    }
}
