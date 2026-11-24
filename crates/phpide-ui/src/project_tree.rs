//! Project tool window (Alt+1): the lazily loaded folder tree.
//!
//! Rows come from [`crate::services::files::ProjectFiles`]; only the visible
//! rows are laid out, so large expanded folders stay cheap to draw.
//!
//! PhpStorm behaviour covered here (see `docs/REQUIREMENTS.md`):
//! - follows the active editor tab, or "Select Opened File" on demand;
//! - keyboard navigation, speed search by typing, Ctrl/Shift multi-select;
//! - Expand All / Collapse All, sorting, folders on top, excluded files;
//! - context menu: New, Cut/Copy/Paste, Rename, Delete, Copy Path,
//!   Open In (file manager, terminal), Open in Right Split.

use std::path::{Path, PathBuf};

use eframe::egui;
use phpide_icons::{file_icon, IconId};

use crate::design::components::{
    menu_item, menu_separator, Button, Dialog, IconButton, PanelHeader, TextInput,
    TreeRow as RowWidget,
};
use crate::services::file_ops;
use crate::services::files::{TreeOptions, TreeRow};
use crate::toolwindows::ToolWindow;
use crate::PhpIdeApp;

/// Keyboard focus target of the tree.
pub fn tree_id() -> egui::Id {
    egui::Id::new("project-tree")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateKind {
    File,
    Directory,
    /// `class`, `interface`, `trait` or `enum`.
    Php(&'static str),
}

impl CreateKind {
    fn title(self) -> &'static str {
        match self {
            CreateKind::File => "New File",
            CreateKind::Directory => "New Directory",
            CreateKind::Php("interface") => "New PHP Interface",
            CreateKind::Php("trait") => "New PHP Trait",
            CreateKind::Php("enum") => "New PHP Enum",
            CreateKind::Php(_) => "New PHP Class",
        }
    }
}

pub enum TreeDialog {
    Create {
        dir: PathBuf,
        kind: CreateKind,
        name: String,
        error: Option<String>,
    },
    Rename {
        path: PathBuf,
        name: String,
        error: Option<String>,
    },
    Delete {
        paths: Vec<PathBuf>,
    },
}

#[derive(Default)]
pub struct TreeState {
    /// Selected rows (Ctrl/Shift+click, Shift+arrows).
    pub selection: Vec<PathBuf>,
    /// Row with the keyboard cursor.
    pub lead: Option<PathBuf>,
    anchor: Option<PathBuf>,
    /// Speed search text while typing in the tree (RF-NAV-08).
    pub speed: Option<String>,
    /// Row to scroll into view once it is loaded.
    scroll_to: Option<PathBuf>,
    /// Active file the tree last followed.
    followed: Option<PathBuf>,
    pub dialog: Option<TreeDialog>,
    /// Cut or copied paths (`true` = cut).
    clipboard: Option<(Vec<PathBuf>, bool)>,
    /// "New" popup opened from the keyboard (Alt+Insert) or the + button.
    new_popup: bool,
    new_filter: String,
    new_selected: usize,
    focus_dialog: bool,
}

impl TreeState {
    /// Selects one path and scrolls to it.
    pub fn select_only(&mut self, path: PathBuf) {
        self.selection = vec![path.clone()];
        self.anchor = Some(path.clone());
        self.lead = Some(path.clone());
        self.scroll_to = Some(path);
    }

    fn is_selected(&self, path: &Path) -> bool {
        self.selection.iter().any(|p| p == path)
    }

    /// Folder new items go into: the selected folder, or the folder of the
    /// selected file, or the root.
    fn target_dir(&self, root: &Path) -> PathBuf {
        match &self.lead {
            Some(path) if path.is_dir() => path.clone(),
            Some(path) => path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| root.to_path_buf()),
            None => root.to_path_buf(),
        }
    }
}

/// Index of the first match of the speed search in `label`, ignoring case.
fn speed_match(label: &str, query: &str) -> Option<(usize, usize)> {
    if query.is_empty() {
        return None;
    }
    let lower = label.to_lowercase();
    let byte = lower.find(&query.to_lowercase())?;
    let start = lower[..byte].chars().count();
    Some((start, start + query.chars().count()))
}

pub fn tree_options(app: &PhpIdeApp) -> TreeOptions {
    TreeOptions {
        sort: app.settings.tree_sort.clone(),
        folders_first: app.settings.tree_folders_first,
        show_excluded: app.settings.tree_show_excluded,
    }
}

enum Action {
    Toggle(PathBuf),
    Open {
        path: PathBuf,
        preview: bool,
        focus_editor: bool,
    },
    OpenSplit(PathBuf),
    Create(CreateKind),
    Rename,
    Delete,
    Cut,
    Copy,
    Paste,
    CopyPath(PathKind),
    RevealInFiles,
    OpenTerminal,
    ExpandAll,
    CollapseAll,
    SelectOpened,
    Detach(PathBuf),
}

#[derive(Debug, Clone, Copy)]
enum PathKind {
    Absolute,
    Name,
    Relative,
    Repository,
}

pub fn show(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let placement = app.tw.placement(ToolWindow::Project);
    crate::toolwindows::dock(ui, ToolWindow::Project, placement, 260.0, |ui| {
        let mut actions = Vec::new();
        header(app, ui, &mut actions);
        if let Some(error) = app.files.root_error() {
            ui.colored_label(ui.visuals().error_fg_color, error.to_string());
            return;
        }
        if app.files.root_loading() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak("Loading…");
            });
            return;
        }
        follow_active_file(app, ui.ctx(), false);
        let options = tree_options(app);
        let rows = app.files.visible_rows(&options);
        if rows.is_empty() {
            ui.weak("This folder is empty.");
            return;
        }
        body(app, ui, &rows, &mut actions);
        for action in actions {
            run(app, ui.ctx(), action);
        }
    });
    dialogs(app, ui.ctx());
}

fn header(app: &mut PhpIdeApp, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    PanelHeader::new(ToolWindow::Project.title()).show(ui, |ui| {
        if IconButton::new(IconId::Minus, "Hide")
            .shortcut("Shift+Esc")
            .show(ui)
            .clicked()
        {
            app.tw.set_visible(ToolWindow::Project, false);
        }
        let more = IconButton::new(IconId::More, "Options").show(ui);
        egui::Popup::menu(&more)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| options_menu(app, ui));
        if IconButton::new(IconId::CollapseAll, "Collapse All")
            .shortcut("Ctrl+NumPad -")
            .show(ui)
            .clicked()
        {
            actions.push(Action::CollapseAll);
        }
        if IconButton::new(IconId::ExpandAll, "Expand All")
            .shortcut("Ctrl+NumPad +")
            .show(ui)
            .clicked()
        {
            actions.push(Action::ExpandAll);
        }
        if !app.settings.tree_autoscroll_from_source
            && IconButton::new(IconId::Locate, "Select Opened File")
                .shortcut("Alt+F1")
                .show(ui)
                .clicked()
        {
            actions.push(Action::SelectOpened);
        }
        if IconButton::new(IconId::Plus, "New…")
            .shortcut("Alt+Insert")
            .show(ui)
            .clicked()
        {
            app.tree.new_popup = true;
        }
    });
}

fn options_menu(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    ui.set_min_width(240.0);
    let mut changed = false;
    changed |= ui
        .checkbox(
            &mut app.settings.tree_autoscroll_from_source,
            "Always Select Opened File",
        )
        .changed();
    changed |= ui
        .checkbox(
            &mut app.settings.tree_single_click_open,
            "Open Files with Single Click",
        )
        .changed();
    ui.separator();
    ui.label(egui::RichText::new("Sort").weak().small());
    for (value, label) in [
        ("name", "By Name"),
        ("type", "By Type"),
        ("modified", "By Modification Time"),
    ] {
        if ui.radio(app.settings.tree_sort == value, label).clicked() {
            app.settings.tree_sort = value.to_string();
            changed = true;
        }
    }
    changed |= ui
        .checkbox(
            &mut app.settings.tree_folders_first,
            "Folders Always on Top",
        )
        .changed();
    ui.separator();
    changed |= ui
        .checkbox(&mut app.settings.tree_show_excluded, "Show Excluded Files")
        .changed();
    ui.separator();
    crate::app_panels::move_to_menu(app, ui, ToolWindow::Project);
    if menu_item(ui, None, "Hide All Tool Windows", "Ctrl+Shift+F12", true).clicked() {
        app.tw.toggle_hide_all();
        ui.close();
    }
    if changed {
        app.persist_settings();
    }
}

/// Reveals and selects the active file when it changed (or always when
/// `force`, for "Select Opened File").
pub fn follow_active_file(app: &mut PhpIdeApp, ctx: &egui::Context, force: bool) {
    let Some(path) = app.ws.active_buffer().and_then(|b| b.path.clone()) else {
        return;
    };
    if !force
        && (!app.settings.tree_autoscroll_from_source
            || app.tree.followed.as_deref() == Some(path.as_path()))
    {
        return;
    }
    app.tree.followed = Some(path.clone());
    if app.files.root_of(&path).is_none() {
        return;
    }
    app.files.reveal(&app.jobs, ctx, &path);
    app.tree.select_only(path);
}

fn body(app: &mut PhpIdeApp, ui: &mut egui::Ui, rows: &[TreeRow], actions: &mut Vec<Action>) {
    let id = tree_id();
    // Focus target behind the rows: clicks on rows also focus it.
    let background = ui.interact(ui.available_rect_before_wrap(), id, egui::Sense::click());
    if background.clicked() {
        background.request_focus();
    }
    let focused = ui.memory(|m| m.has_focus(id));
    if focused {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    tab: false,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            )
        });
        if !app.tree.new_popup {
            keyboard(app, ui, rows, actions);
        }
    } else if app.tree.speed.is_some() && !app.tree.new_popup {
        app.tree.speed = None;
    }
    if app.tree.new_popup {
        new_popup(app, ui, actions);
    }

    if let Some(query) = &app.tree.speed {
        let matches = rows
            .iter()
            .filter(|r| speed_match(&r.entry.name, query).is_some())
            .count();
        let p = crate::design::current(ui.ctx()).palette;
        let color = if matches == 0 {
            p.error
        } else {
            p.text_primary
        };
        ui.horizontal(|ui| {
            crate::design::components::icon(ui, IconId::Search, 14.0, p.text_muted);
            ui.label(egui::RichText::new(format!("Search for: {query}")).color(color));
        });
    }

    let row_height = crate::design::current(ui.ctx()).metrics.row;
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut scroll = egui::ScrollArea::both().auto_shrink([false; 2]);
    if let Some(target) = &app.tree.scroll_to {
        if let Some(index) = rows.iter().position(|r| &r.entry.path == target) {
            let visible = ui.available_height();
            let offset = (index as f32 * row_height - visible / 2.0 + row_height).max(0.0);
            scroll = scroll.vertical_scroll_offset(offset);
            app.tree.scroll_to = None;
        }
    }
    let query = app.tree.speed.clone();
    scroll.show_rows(ui, row_height, rows.len(), |ui, range| {
        for row in &rows[range] {
            let highlight = query
                .as_deref()
                .and_then(|q| speed_match(&row.entry.name, q));
            draw_row(app, ui, row, focused, highlight, rows, actions);
        }
        // Empty area below the rows: root context menu.
        let rest = ui.available_rect_before_wrap();
        if rest.height() > 0.0 {
            let response = ui.interact(rest, id.with("empty"), egui::Sense::click());
            if response.clicked() {
                ui.memory_mut(|m| m.request_focus(id));
            }
            response.context_menu(|ui| {
                app.tree.lead = None;
                app.tree.selection.clear();
                new_submenu(ui, actions);
            });
        }
    });
}

fn draw_row(
    app: &mut PhpIdeApp,
    ui: &mut egui::Ui,
    row: &TreeRow,
    focused: bool,
    highlight: Option<(usize, usize)>,
    rows: &[TreeRow],
    actions: &mut Vec<Action>,
) {
    let p = crate::design::current(ui.ctx()).palette;
    let entry = &row.entry;
    let (icon, color) = if entry.is_dir {
        let icon = if row.expanded {
            IconId::FolderOpen
        } else {
            IconId::Folder
        };
        (icon, p.folder)
    } else {
        let (icon, kind) = file_icon(&entry.name);
        (icon, p.file_color(kind))
    };
    let cut = app
        .tree
        .clipboard
        .as_ref()
        .is_some_and(|(paths, cut)| *cut && paths.contains(&entry.path));
    let mut widget = RowWidget::new(entry.name.as_str())
        .depth(row.depth)
        .expanded(entry.is_dir.then_some(row.expanded))
        .icon(icon, color)
        .selected(app.tree.is_selected(&entry.path))
        .focused(focused)
        .highlight(highlight);
    if entry.excluded || cut {
        widget = widget.label_color(p.text_muted);
    }
    // Dependency roots read as libraries (RF-NAV-01).
    let library = entry.is_dir
        && entry
            .path
            .parent()
            .is_some_and(|parent| app.files.root_of(parent) == Some(parent))
        && matches!(entry.name.as_str(), "vendor" | "node_modules");
    if library {
        widget = widget
            .icon(IconId::Package, p.text_secondary)
            .detail(if entry.name == "vendor" {
                "Composer packages"
            } else {
                "npm packages"
            });
    }
    // Coverage of the last run with coverage, per file.
    if !entry.is_dir {
        if let Some(file) = app.rd.coverage.as_ref().and_then(|c| c.file(&entry.path)) {
            widget = widget.detail(format!(
                "{}% lines covered",
                (file.percent_tenths() + 5) / 10
            ));
        }
    }
    if row.loading {
        widget = widget.detail("Loading…");
    }
    if let Some(error) = &row.error {
        widget = widget.detail(error.as_str()).label_color(p.error);
    }
    let shown = widget.show(ui);
    let response = shown.response;
    let path = entry.path.clone();
    if shown.toggled {
        actions.push(Action::Toggle(path.clone()));
    } else if response.double_clicked() {
        if entry.is_dir {
            if !app.settings.tree_single_click_open {
                actions.push(Action::Toggle(path.clone()));
            }
        } else {
            actions.push(Action::Open {
                path: path.clone(),
                preview: false,
                focus_editor: true,
            });
        }
    } else if response.clicked() {
        ui.memory_mut(|m| m.request_focus(tree_id()));
        let modifiers = ui.input(|i| i.modifiers);
        click_select(&mut app.tree, rows, &path, modifiers);
        if !modifiers.command && !modifiers.shift && app.settings.tree_single_click_open {
            if entry.is_dir {
                actions.push(Action::Toggle(path.clone()));
            } else {
                actions.push(Action::Open {
                    path: path.clone(),
                    preview: true,
                    focus_editor: false,
                });
            }
        }
    }
    if response.secondary_clicked() && !app.tree.is_selected(&path) {
        app.tree.select_only(path.clone());
        app.tree.scroll_to = None;
    }
    response.context_menu(|ui| context_menu(app, ui, entry.is_dir, actions));
}

fn click_select(tree: &mut TreeState, rows: &[TreeRow], path: &Path, modifiers: egui::Modifiers) {
    if modifiers.shift {
        let anchor = tree.anchor.clone().unwrap_or_else(|| path.to_path_buf());
        let a = rows.iter().position(|r| r.entry.path == anchor);
        let b = rows.iter().position(|r| r.entry.path == path);
        if let (Some(a), Some(b)) = (a, b) {
            let (lo, hi) = (a.min(b), a.max(b));
            tree.selection = rows[lo..=hi].iter().map(|r| r.entry.path.clone()).collect();
        }
    } else if modifiers.command {
        if let Some(i) = tree.selection.iter().position(|p| p == path) {
            tree.selection.remove(i);
        } else {
            tree.selection.push(path.to_path_buf());
        }
        tree.anchor = Some(path.to_path_buf());
    } else {
        tree.selection = vec![path.to_path_buf()];
        tree.anchor = Some(path.to_path_buf());
    }
    tree.lead = Some(path.to_path_buf());
}

fn new_submenu(ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    ui.menu_button("New", |ui| {
        ui.set_min_width(200.0);
        for (kind, icon, label) in [
            (CreateKind::File, IconId::File, "File"),
            (CreateKind::Directory, IconId::Folder, "Directory"),
            (CreateKind::Php("class"), IconId::FilePhp, "PHP Class"),
            (
                CreateKind::Php("interface"),
                IconId::FilePhp,
                "PHP Interface",
            ),
            (CreateKind::Php("trait"), IconId::FilePhp, "PHP Trait"),
            (CreateKind::Php("enum"), IconId::FilePhp, "PHP Enum"),
        ] {
            if menu_item(ui, Some(icon), label, "", true).clicked() {
                actions.push(Action::Create(kind));
                ui.close();
            }
        }
    });
}

fn context_menu(app: &mut PhpIdeApp, ui: &mut egui::Ui, is_dir: bool, actions: &mut Vec<Action>) {
    ui.set_min_width(240.0);
    let single = app.tree.selection.len() == 1;
    new_submenu(ui, actions);
    menu_separator(ui);
    fn item(
        actions: &mut Vec<Action>,
        ui: &mut egui::Ui,
        label: &str,
        shortcut: &str,
        enabled: bool,
        action: Action,
    ) {
        if menu_item(ui, None, label, shortcut, enabled).clicked() {
            actions.push(action);
            ui.close();
        }
    }
    item(actions, ui, "Cut", "Ctrl+X", true, Action::Cut);
    item(actions, ui, "Copy", "Ctrl+C", true, Action::Copy);
    item(
        actions,
        ui,
        "Paste",
        "Ctrl+V",
        app.tree.clipboard.is_some(),
        Action::Paste,
    );
    ui.menu_button("Copy Path/Reference…", |ui| {
        ui.set_min_width(220.0);
        for (kind, label, shortcut) in [
            (PathKind::Absolute, "Absolute Path", "Ctrl+Shift+C"),
            (PathKind::Name, "File Name", ""),
            (
                PathKind::Relative,
                "Path From Project Root",
                "Ctrl+Alt+Shift+C",
            ),
            (PathKind::Repository, "Repository URL", ""),
        ] {
            if menu_item(ui, None, label, shortcut, true).clicked() {
                actions.push(Action::CopyPath(kind));
                ui.close();
            }
        }
    });
    menu_separator(ui);
    item(actions, ui, "Rename…", "Shift+F6", single, Action::Rename);
    item(actions, ui, "Delete…", "Delete", true, Action::Delete);
    menu_separator(ui);
    if !is_dir {
        item(
            actions,
            ui,
            "Open in Right Split",
            "Shift+Enter",
            single,
            Action::OpenSplit(app.tree.lead.clone().unwrap_or_default()),
        );
    }
    if !is_dir && single {
        if let Some(path) = app.tree.lead.clone() {
            ui.menu_button("Override File Type", |ui| {
                ui.set_min_width(200.0);
                let current = app.file_type_override(&path).map(str::to_string);
                if ui
                    .selectable_label(current.is_none(), "Auto-detect")
                    .clicked()
                {
                    app.set_file_type_override(&path, None);
                    ui.close();
                }
                ui.separator();
                for file_type in phpide_php::filetype::FILE_TYPES {
                    let selected = current.as_deref() == Some(file_type.id);
                    if ui.selectable_label(selected, file_type.name).clicked() {
                        app.set_file_type_override(&path, Some(file_type.id));
                        ui.close();
                    }
                }
            });
        }
    }
    ui.menu_button("Open In", |ui| {
        if menu_item(ui, None, "Files", "", single).clicked() {
            actions.push(Action::RevealInFiles);
            ui.close();
        }
        if menu_item(ui, None, "Terminal", "", single).clicked() {
            actions.push(Action::OpenTerminal);
            ui.close();
        }
    });
    if let Some(lead) = app.tree.lead.clone() {
        if app.files.attached().contains(&lead) {
            menu_separator(ui);
            item(
                actions,
                ui,
                "Detach Project",
                "",
                true,
                Action::Detach(lead),
            );
        }
    }
    if is_dir {
        menu_separator(ui);
        item(
            actions,
            ui,
            "Expand All",
            "Ctrl+NumPad +",
            single,
            Action::ExpandAll,
        );
        item(
            actions,
            ui,
            "Collapse All",
            "Ctrl+NumPad -",
            true,
            Action::CollapseAll,
        );
    }
}

const NEW_KINDS: [(CreateKind, IconId, &str); 6] = [
    (CreateKind::File, IconId::File, "File"),
    (CreateKind::Directory, IconId::Folder, "Directory"),
    (CreateKind::Php("class"), IconId::FilePhp, "PHP Class"),
    (
        CreateKind::Php("interface"),
        IconId::FilePhp,
        "PHP Interface",
    ),
    (CreateKind::Php("trait"), IconId::FilePhp, "PHP Trait"),
    (CreateKind::Php("enum"), IconId::FilePhp, "PHP Enum"),
];

/// The New popup (Alt+Insert, +): typing filters it (RF-NAV-15).
fn new_popup(app: &mut PhpIdeApp, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    use crate::design::components::ListRow;
    let mut open = true;
    let query = app.tree.new_filter.to_lowercase();
    let shown: Vec<(CreateKind, IconId, &str)> = NEW_KINDS
        .into_iter()
        .filter(|(_, _, label)| {
            query.is_empty() || crate::palette::fuzzy_score(&query, label).is_some()
        })
        .collect();
    if app.tree.new_selected >= shown.len() {
        app.tree.new_selected = shown.len().saturating_sub(1);
    }
    let p = crate::design::current(ui.ctx()).palette;
    let area = egui::Area::new(egui::Id::new("tree-new"))
        .order(egui::Order::Foreground)
        .fixed_pos(ui.max_rect().left_top() + egui::vec2(24.0, 48.0))
        .show(ui.ctx(), |ui| {
            Dialog::frame(ui).show(ui, |ui| {
                ui.set_width(240.0);
                ui.label(egui::RichText::new("New").strong());
                let input = TextInput::new(&mut app.tree.new_filter)
                    .hint("Type to filter")
                    .id_salt("tree-new-filter")
                    .show(ui);
                if !input.has_focus() {
                    input.request_focus();
                }
                ui.memory_mut(|m| {
                    m.set_focus_lock_filter(
                        input.id,
                        egui::EventFilter {
                            vertical_arrows: true,
                            escape: true,
                            ..Default::default()
                        },
                    )
                });
                ui.add_space(4.0);
                for (n, (kind, icon, label)) in shown.iter().enumerate() {
                    let row = ListRow::new(*label)
                        .icon(*icon, p.text_secondary)
                        .selected(n == app.tree.new_selected)
                        .show(ui);
                    if row.clicked() {
                        actions.push(Action::Create(*kind));
                        open = false;
                    }
                }
            });
        });
    ui.input(|i| {
        if i.key_pressed(egui::Key::ArrowDown) && !shown.is_empty() {
            app.tree.new_selected = (app.tree.new_selected + 1) % shown.len();
        }
        if i.key_pressed(egui::Key::ArrowUp) && !shown.is_empty() {
            app.tree.new_selected = (app.tree.new_selected + shown.len() - 1) % shown.len();
        }
        if i.key_pressed(egui::Key::Enter) {
            if let Some((kind, _, _)) = shown.get(app.tree.new_selected) {
                actions.push(Action::Create(*kind));
            }
            open = false;
        }
        if i.key_pressed(egui::Key::Escape) {
            open = false;
        }
    });
    if ui.input(|i| i.pointer.any_pressed()) && !area.response.contains_pointer() {
        open = false;
    }
    if !open {
        app.tree.new_popup = false;
        app.tree.new_filter.clear();
        app.tree.new_selected = 0;
        if app.tree.dialog.is_none() {
            ui.memory_mut(|m| m.request_focus(tree_id()));
        }
    }
}

fn keyboard(app: &mut PhpIdeApp, ui: &mut egui::Ui, rows: &[TreeRow], actions: &mut Vec<Action>) {
    let lead_index = app
        .tree
        .lead
        .as_ref()
        .and_then(|lead| rows.iter().position(|r| &r.entry.path == lead));
    let events = ui.input(|i| i.events.clone());
    let mut moved: Option<(usize, bool)> = None;
    for event in events {
        match event {
            egui::Event::Text(text) => {
                let query = app.tree.speed.get_or_insert_with(String::new);
                query.push_str(&text);
                let query = query.clone();
                // Jump to the first match from the current row on.
                let start = lead_index.unwrap_or(0);
                if let Some(i) = (start..rows.len())
                    .chain(0..start)
                    .find(|&i| speed_match(&rows[i].entry.name, &query).is_some())
                {
                    moved = Some((i, false));
                }
            }
            egui::Event::Copy => actions.push(Action::Copy),
            egui::Event::Cut => actions.push(Action::Cut),
            egui::Event::Paste(_) => actions.push(Action::Paste),
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                let current = lead_index.unwrap_or(0);
                let speed = app.tree.speed.clone();
                let step = |from: usize, down: bool| -> Option<usize> {
                    match &speed {
                        Some(q) if !q.is_empty() => {
                            let found = if down {
                                (from + 1..rows.len())
                                    .find(|&i| speed_match(&rows[i].entry.name, q).is_some())
                            } else {
                                (0..from)
                                    .rev()
                                    .find(|&i| speed_match(&rows[i].entry.name, q).is_some())
                            };
                            found.or(Some(from))
                        }
                        _ if down => Some((from + 1).min(rows.len().saturating_sub(1))),
                        _ => Some(from.saturating_sub(1)),
                    }
                };
                match key {
                    egui::Key::ArrowDown => {
                        let to = if lead_index.is_none() {
                            Some(0)
                        } else {
                            step(current, true)
                        };
                        moved = to.map(|i| (i, modifiers.shift));
                    }
                    egui::Key::ArrowUp => {
                        moved = step(current, false).map(|i| (i, modifiers.shift));
                    }
                    egui::Key::Home => moved = Some((0, modifiers.shift)),
                    egui::Key::End => moved = Some((rows.len().saturating_sub(1), modifiers.shift)),
                    egui::Key::PageDown => {
                        moved = Some((
                            (current + 20).min(rows.len().saturating_sub(1)),
                            modifiers.shift,
                        ))
                    }
                    egui::Key::PageUp => {
                        moved = Some((current.saturating_sub(20), modifiers.shift))
                    }
                    egui::Key::ArrowRight => {
                        if let Some(row) = lead_index.map(|i| &rows[i]) {
                            if row.entry.is_dir && !row.expanded {
                                actions.push(Action::Toggle(row.entry.path.clone()));
                            } else if row.entry.is_dir && current + 1 < rows.len() {
                                moved = Some((current + 1, false));
                            }
                        }
                    }
                    egui::Key::ArrowLeft => {
                        if let Some(row) = lead_index.map(|i| &rows[i]) {
                            if row.entry.is_dir && row.expanded {
                                actions.push(Action::Toggle(row.entry.path.clone()));
                            } else if let Some(parent) =
                                (0..current).rev().find(|&i| rows[i].depth + 1 == row.depth)
                            {
                                moved = Some((parent, false));
                            }
                        }
                    }
                    egui::Key::Enter => {
                        if let Some(row) = lead_index.map(|i| &rows[i]) {
                            if row.entry.is_dir {
                                actions.push(Action::Toggle(row.entry.path.clone()));
                            } else if modifiers.shift {
                                actions.push(Action::OpenSplit(row.entry.path.clone()));
                            } else {
                                actions.push(Action::Open {
                                    path: row.entry.path.clone(),
                                    preview: false,
                                    focus_editor: true,
                                });
                            }
                        }
                        app.tree.speed = None;
                    }
                    egui::Key::F4 => {
                        if let Some(row) = lead_index.map(|i| &rows[i]).filter(|r| !r.entry.is_dir)
                        {
                            actions.push(Action::Open {
                                path: row.entry.path.clone(),
                                preview: false,
                                focus_editor: true,
                            });
                        }
                    }
                    egui::Key::Delete => actions.push(Action::Delete),
                    egui::Key::F6 if modifiers.shift => actions.push(Action::Rename),
                    egui::Key::Insert if modifiers.alt => app.tree.new_popup = true,
                    egui::Key::C if modifiers.command && modifiers.shift && modifiers.alt => {
                        actions.push(Action::CopyPath(PathKind::Relative))
                    }
                    egui::Key::C if modifiers.command && modifiers.shift => {
                        actions.push(Action::CopyPath(PathKind::Absolute))
                    }
                    egui::Key::F if modifiers.command => {
                        app.tree.speed.get_or_insert_with(String::new);
                    }
                    egui::Key::Plus | egui::Key::Equals if modifiers.command => {
                        actions.push(Action::ExpandAll)
                    }
                    egui::Key::Minus if modifiers.command => actions.push(Action::CollapseAll),
                    egui::Key::Backspace => {
                        if let Some(query) = &mut app.tree.speed {
                            query.pop();
                        }
                    }
                    egui::Key::Escape => {
                        if app.tree.speed.is_some() {
                            app.tree.speed = None;
                        } else {
                            // Esc returns focus to the editor (RF-UI-02).
                            ui.memory_mut(|m| m.surrender_focus(tree_id()));
                            app.editors.request_focus();
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    if let Some((index, extend)) = moved {
        let Some(row) = rows.get(index) else {
            return;
        };
        let path = row.entry.path.clone();
        if extend {
            let modifiers = egui::Modifiers::SHIFT;
            click_select(&mut app.tree, rows, &path, modifiers);
        } else {
            app.tree.selection = vec![path.clone()];
            app.tree.anchor = Some(path.clone());
            app.tree.lead = Some(path.clone());
        }
        app.tree.scroll_to = Some(path);
    }
}

fn run(app: &mut PhpIdeApp, ctx: &egui::Context, action: Action) {
    let root = app.files.root().to_path_buf();
    match action {
        Action::Toggle(dir) => {
            app.files.toggle(&app.jobs, ctx, &dir);
        }
        Action::Open {
            path,
            preview,
            focus_editor,
        } => {
            app.open_file_from_tree(&path, preview);
            if focus_editor {
                app.editors.request_focus();
            }
        }
        Action::OpenSplit(path) => {
            app.open_file_from_tree(&path, false);
            app.editors
                .split(&app.ws, crate::editor_area::SplitAxis::Right);
        }
        Action::Create(kind) => {
            app.tree.new_popup = false;
            app.tree.dialog = Some(TreeDialog::Create {
                dir: app.tree.target_dir(&root),
                kind,
                name: String::new(),
                error: None,
            });
            app.tree.focus_dialog = true;
        }
        Action::Rename => {
            if let [path] = app.tree.selection.as_slice() {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                app.tree.dialog = Some(TreeDialog::Rename {
                    path: path.clone(),
                    name,
                    error: None,
                });
                app.tree.focus_dialog = true;
            }
        }
        Action::Delete => {
            let paths: Vec<PathBuf> = app
                .tree
                .selection
                .iter()
                .filter(|p| **p != root)
                .cloned()
                .collect();
            if !paths.is_empty() {
                app.tree.dialog = Some(TreeDialog::Delete { paths });
            }
        }
        Action::Cut | Action::Copy => {
            let cut = matches!(action, Action::Cut);
            let paths = app.tree.selection.clone();
            if !paths.is_empty() {
                let text = paths
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                ctx.copy_text(text);
                app.tree.clipboard = Some((paths, cut));
            }
        }
        Action::Paste => paste(app, ctx, &root),
        Action::CopyPath(kind) => {
            let text = app
                .tree
                .selection
                .iter()
                .map(|p| match kind {
                    PathKind::Absolute => p.display().to_string(),
                    PathKind::Name => p
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    PathKind::Relative => p.strip_prefix(&root).unwrap_or(p).display().to_string(),
                    PathKind::Repository => app
                        .git
                        .remote()
                        .and_then(|r| r.file_url(p, None))
                        .unwrap_or_default(),
                })
                .collect::<Vec<_>>()
                .join("\n");
            if !text.is_empty() {
                app.ws.message = format!("Copied {text}");
                ctx.copy_text(text);
            }
        }
        Action::RevealInFiles => {
            if let Some(path) = app.tree.lead.clone() {
                let dir = if path.is_dir() {
                    path
                } else {
                    path.parent().map(Path::to_path_buf).unwrap_or(root)
                };
                app.ws.message = match crate::services::desktop::open_folder(&dir) {
                    Ok(()) => format!("Opened {}", dir.display()),
                    Err(e) => e,
                };
            }
        }
        Action::OpenTerminal => {
            if let Some(path) = app.tree.lead.clone() {
                let dir = if path.is_dir() {
                    path
                } else {
                    path.parent().map(Path::to_path_buf).unwrap_or(root)
                };
                app.ws.message = match crate::services::desktop::open_terminal(&dir) {
                    Ok(name) => format!("Opened {name} in {}", dir.display()),
                    Err(e) => e,
                };
            }
        }
        Action::ExpandAll => {
            let dir = app.tree.lead.clone().filter(|p| p.is_dir()).unwrap_or(root);
            app.files.expand_all(&app.jobs, ctx, &dir);
        }
        Action::CollapseAll => {
            app.files.collapse_all();
            if let Some(lead) = &app.tree.lead {
                // Keep the cursor on a visible row.
                let top = lead
                    .strip_prefix(&root)
                    .ok()
                    .and_then(|rel| rel.components().next())
                    .map(|first| root.join(first));
                if let Some(top) = top {
                    app.tree.select_only(top);
                }
            }
        }
        Action::Detach(root) => app.detach_project(&root),
        Action::SelectOpened => {
            app.tw.set_visible(ToolWindow::Project, true);
            follow_active_file(app, ctx, true);
            ctx.memory_mut(|m| m.request_focus(tree_id()));
        }
    }
}

fn paste(app: &mut PhpIdeApp, ctx: &egui::Context, root: &Path) {
    let Some((paths, cut)) = app.tree.clipboard.clone() else {
        return;
    };
    let target = app.tree.target_dir(root);
    let mut last = None;
    let mut errors = Vec::new();
    for path in &paths {
        let result = if cut {
            file_ops::move_into(path, &target)
        } else {
            file_ops::copy_into(path, &target)
        };
        match result {
            Ok(new_path) => {
                if cut {
                    app.on_path_moved(path, &new_path);
                    if let Some(parent) = path.parent() {
                        app.files.reload_dir(&app.jobs, ctx, parent);
                    }
                }
                last = Some(new_path);
            }
            Err(e) => errors.push(e),
        }
    }
    if cut {
        app.tree.clipboard = None;
    }
    app.files.reload_dir(&app.jobs, ctx, &target);
    if !app.files.is_expanded(&target) {
        app.files.expand(&app.jobs, ctx, &target);
    }
    if let Some(path) = last {
        app.tree.select_only(path);
    }
    app.ws.message = if errors.is_empty() {
        format!(
            "{} {} item(s) into {}",
            if cut { "Moved" } else { "Copied" },
            paths.len(),
            target.display()
        )
    } else {
        errors.join("; ")
    };
}

fn dialogs(app: &mut PhpIdeApp, ctx: &egui::Context) {
    let Some(dialog) = app.tree.dialog.take() else {
        return;
    };
    let focus = std::mem::take(&mut app.tree.focus_dialog);
    let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
    let mut keep = true;
    let next = match dialog {
        TreeDialog::Create {
            dir,
            kind,
            mut name,
            mut error,
        } => {
            let mut confirm = false;
            let shown = Dialog::new("tree-create", kind.title()).width(380.0).show(
                ctx,
                |ui| {
                    let relative = dir
                        .strip_prefix(app.files.root())
                        .unwrap_or(&dir)
                        .display()
                        .to_string();
                    ui.label(
                        egui::RichText::new(if relative.is_empty() {
                            "In the project root".to_string()
                        } else {
                            format!("In {relative}")
                        })
                        .weak(),
                    );
                    let hint = match kind {
                        CreateKind::File => "name.php or folder/name.php",
                        CreateKind::Directory => "name or parent/child",
                        CreateKind::Php(_) => "ClassName",
                    };
                    let response = TextInput::new(&mut name)
                        .hint(hint)
                        .invalid(error.is_some())
                        .id_salt("tree-create-name")
                        .show(ui);
                    if focus {
                        response.request_focus();
                    }
                    if response.changed() {
                        error = None;
                    }
                    if let Some(e) = &error {
                        ui.colored_label(crate::design::current(ui.ctx()).palette.error, e);
                    }
                },
                |ui| {
                    if Button::primary("Create").show(ui).clicked() {
                        confirm = true;
                    }
                    if Button::new("Cancel").show(ui).clicked() {
                        keep = false;
                    }
                },
            );
            if shown.dismissed {
                keep = false;
            }
            if keep && (confirm || enter) {
                match create(app, ctx, &dir, kind, &name) {
                    Ok(()) => keep = false,
                    Err(e) => error = Some(e),
                }
            }
            TreeDialog::Create {
                dir,
                kind,
                name,
                error,
            }
        }
        TreeDialog::Rename {
            path,
            mut name,
            mut error,
        } => {
            let mut confirm = false;
            let title = if path.is_dir() {
                "Rename Directory"
            } else {
                "Rename File"
            };
            let shown = Dialog::new("tree-rename", title).width(380.0).show(
                ctx,
                |ui| {
                    let response = TextInput::new(&mut name)
                        .invalid(error.is_some())
                        .id_salt("tree-rename-name")
                        .show(ui);
                    if focus {
                        response.request_focus();
                        // Select the name without its extension, as PhpStorm.
                        let stem = match name.rsplit_once('.') {
                            Some((stem, _)) if !stem.is_empty() && !path.is_dir() => {
                                stem.chars().count()
                            }
                            _ => name.chars().count(),
                        };
                        let mut state =
                            egui::TextEdit::load_state(ui.ctx(), response.id).unwrap_or_default();
                        state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::two(
                                egui::text::CCursor::new(0),
                                egui::text::CCursor::new(stem),
                            )));
                        state.store(ui.ctx(), response.id);
                    }
                    if response.changed() {
                        error = None;
                    }
                    if let Some(e) = &error {
                        ui.colored_label(crate::design::current(ui.ctx()).palette.error, e);
                    }
                },
                |ui| {
                    if Button::primary("Rename").show(ui).clicked() {
                        confirm = true;
                    }
                    if Button::new("Cancel").show(ui).clicked() {
                        keep = false;
                    }
                },
            );
            if shown.dismissed {
                keep = false;
            }
            if keep && (confirm || enter) {
                match file_ops::rename(&path, &name) {
                    Ok(new_path) => {
                        app.on_path_moved(&path, &new_path);
                        if let Some(parent) = new_path.parent() {
                            app.files.reload_dir(&app.jobs, ctx, parent);
                        }
                        app.tree.select_only(new_path);
                        keep = false;
                    }
                    Err(e) => error = Some(e),
                }
            }
            TreeDialog::Rename { path, name, error }
        }
        TreeDialog::Delete { paths } => {
            let mut confirm = false;
            let title = if paths.len() == 1 {
                "Delete"
            } else {
                "Delete Items"
            };
            let shown = Dialog::new("tree-delete", title).width(420.0).show(
                ctx,
                |ui| {
                    let what = if paths.len() == 1 {
                        format!(
                            "Delete \"{}\"?",
                            paths[0]
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default()
                        )
                    } else {
                        format!("Delete {} items?", paths.len())
                    };
                    ui.label(what);
                    ui.label(
                        egui::RichText::new("Items go to the Trash when the desktop has one.")
                            .weak(),
                    );
                },
                |ui| {
                    if Button::primary("Delete").show(ui).clicked() {
                        confirm = true;
                    }
                    if Button::new("Cancel").show(ui).clicked() {
                        keep = false;
                    }
                },
            );
            if shown.dismissed {
                keep = false;
            }
            if keep && (confirm || enter) {
                delete(app, ctx, &paths);
                keep = false;
            }
            TreeDialog::Delete { paths }
        }
    };
    if keep {
        app.tree.dialog = Some(next);
    } else {
        ctx.memory_mut(|m| m.request_focus(tree_id()));
    }
}

fn create(
    app: &mut PhpIdeApp,
    ctx: &egui::Context,
    dir: &Path,
    kind: CreateKind,
    name: &str,
) -> Result<(), String> {
    let name = name.trim();
    let path = match kind {
        CreateKind::File => file_ops::create_file(dir, name, "")?,
        CreateKind::Directory => file_ops::create_dir(dir, name)?,
        CreateKind::Php(keyword) => {
            let class = name.trim_end_matches(".php");
            if class.is_empty()
                || !class
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
                || !class.chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                return Err("Enter a valid PHP name".into());
            }
            let namespace = file_ops::psr4_namespace(app.files.root(), dir);
            let keyword = if keyword == "class" {
                "final class"
            } else {
                keyword
            };
            let source = file_ops::php_class_source(keyword, class, namespace.as_deref());
            file_ops::create_file(dir, &format!("{class}.php"), &source)?
        }
    };
    let parent = path.parent().unwrap_or(dir).to_path_buf();
    app.files.reload_dir(&app.jobs, ctx, dir);
    app.files.reload_dir(&app.jobs, ctx, &parent);
    app.files.reveal(&app.jobs, ctx, &path);
    app.tree.select_only(path.clone());
    if !path.is_dir() {
        app.open_file_from_tree(&path, false);
        app.editors.request_focus();
    }
    Ok(())
}

fn delete(app: &mut PhpIdeApp, ctx: &egui::Context, paths: &[PathBuf]) {
    let mut trashed = 0;
    let mut errors = Vec::new();
    for path in paths {
        // Tests never touch the desktop Trash.
        match file_ops::delete(path, !cfg!(test)) {
            Ok(to_trash) => {
                trashed += usize::from(to_trash);
                app.on_path_deleted(path);
                if let Some(parent) = path.parent() {
                    app.files.reload_dir(&app.jobs, ctx, parent);
                }
            }
            Err(e) => errors.push(format!("{}: {e}", path.display())),
        }
    }
    app.tree.selection.retain(|p| !paths.contains(p));
    app.tree.lead = None;
    app.ws.message = if !errors.is_empty() {
        errors.join("; ")
    } else if trashed == paths.len() {
        format!("Moved {} item(s) to the Trash", paths.len())
    } else {
        format!("Deleted {} item(s)", paths.len())
    };
}

impl PhpIdeApp {
    /// Opens a file selected in the tree (preview tab on single click).
    pub(crate) fn open_file_from_tree(&mut self, path: &Path, preview: bool) {
        self.open_file_tab(path, preview);
        self.tree.followed = Some(path.to_path_buf());
    }

    /// Open tabs follow a renamed or moved file or folder.
    pub(crate) fn on_path_moved(&mut self, from: &Path, to: &Path) {
        for buffer in &mut self.ws.buffers {
            if let Some(path) = &buffer.path {
                if let Ok(rest) = path.strip_prefix(from) {
                    buffer.path = Some(if rest.as_os_str().is_empty() {
                        to.to_path_buf()
                    } else {
                        to.join(rest)
                    });
                }
            }
        }
        self.ws.recent_files.retain(|p| !p.starts_with(from));
    }

    /// Clean tabs of deleted files close; modified ones stay open so their
    /// text can still be saved elsewhere.
    pub(crate) fn on_path_deleted(&mut self, deleted: &Path) {
        let mut i = 0;
        while i < self.ws.buffers.len() {
            let buffer = &self.ws.buffers[i];
            let gone = buffer
                .path
                .as_deref()
                .is_some_and(|p| p.starts_with(deleted));
            if gone && !buffer.dirty {
                self.ws.close_tab(i);
            } else {
                i += 1;
            }
        }
        self.ws.recent_files.retain(|p| !p.starts_with(deleted));
    }
}

/// Select In (Alt+F1): where to show the active file (RF-SEL-10).
pub fn select_in_popup(app: &mut PhpIdeApp, ctx: &egui::Context) {
    use crate::design::components::ListRow;
    let Some(mut selected) = app.select_in else {
        return;
    };
    let Some(path) = app.ws.active_buffer().and_then(|b| b.path.clone()) else {
        app.select_in = None;
        return;
    };
    let targets = [
        (IconId::Project, "Project View"),
        (IconId::Folder, "Files (file manager)"),
        (IconId::Terminal, "Terminal"),
    ];
    let mut chosen = None;
    let mut close = false;
    ctx.input(|i| {
        for (n, key) in [egui::Key::Num1, egui::Key::Num2, egui::Key::Num3]
            .into_iter()
            .enumerate()
        {
            if i.key_pressed(key) {
                chosen = Some(n);
            }
        }
        if i.key_pressed(egui::Key::ArrowDown) {
            selected = (selected + 1) % targets.len();
        }
        if i.key_pressed(egui::Key::ArrowUp) {
            selected = (selected + targets.len() - 1) % targets.len();
        }
        if i.key_pressed(egui::Key::Enter) {
            chosen = Some(selected);
        }
        if i.key_pressed(egui::Key::Escape) {
            close = true;
        }
    });
    let p = crate::design::current(ctx).palette;
    let area = egui::Area::new(egui::Id::new("select-in"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 120.0))
        .show(ctx, |ui| {
            crate::design::components::Dialog::frame(ui).show(ui, |ui| {
                ui.set_width(300.0);
                // Holds the keyboard so digits do not reach the editor.
                let focus = ui.interact(
                    ui.max_rect(),
                    egui::Id::new("select-in-focus"),
                    egui::Sense::focusable_noninteractive(),
                );
                if !focus.has_focus() {
                    focus.request_focus();
                }
                ui.label(egui::RichText::new("Select In").strong());
                ui.add_space(4.0);
                for (n, (icon, label)) in targets.iter().enumerate() {
                    let row = ListRow::new(format!("{}. {label}", n + 1))
                        .icon(*icon, p.text_secondary)
                        .selected(n == selected)
                        .show(ui);
                    if row.clicked() {
                        chosen = Some(n);
                    }
                }
            });
        });
    if ctx.input(|i| i.pointer.any_pressed()) && !area.response.contains_pointer() {
        close = true;
    }
    app.select_in = (!close && chosen.is_none()).then_some(selected);
    if app.select_in.is_none() && chosen != Some(0) {
        app.editors.request_focus();
    }
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    match chosen {
        Some(0) => {
            app.tw.set_visible(ToolWindow::Project, true);
            follow_active_file(app, ctx, true);
            ctx.memory_mut(|m| m.request_focus(tree_id()));
        }
        Some(1) => {
            if let Err(e) = crate::services::desktop::open_folder(&dir) {
                app.ws.message = e;
            }
        }
        Some(2) => {
            app.ws.message = match crate::services::desktop::open_terminal(&dir) {
                Ok(name) => format!("Opened {name} in {}", dir.display()),
                Err(e) => e,
            };
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_search_matches_ignore_case_and_count_chars() {
        assert_eq!(speed_match("UserController.php", "contr"), Some((4, 9)));
        assert_eq!(speed_match("Ação.php", "ção"), Some((1, 4)));
        assert_eq!(speed_match("a.php", "zz"), None);
        assert_eq!(speed_match("a.php", ""), None);
    }
}
