//! Welcome screen: Projects and Customize (Plugins, Learn and Remote
//! Development only with the `experimental` feature); recent projects, Get
//! from VCS, folder browser, settings import and New Project with a
//! composer.json skeleton.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use eframe::egui;
use phpide_core::jobs::{JobContext, JobHandle, JobOutcome};
use phpide_core::{Component, ErrorKind, IdeError};

use crate::design::components::{menu_item, menu_separator, Button, Dialog, IconButton, TextInput};
use crate::design::ThemeId;
use crate::services::desktop::Picked;
use crate::PhpIdeApp;
use phpide_icons::IconId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Projects,
    Customize,
    Plugins,
    Learn,
    RemoteDevelopment,
}

/// Sub-folders of a directory: `(path, looks like a project)`.
type Folders = Vec<(PathBuf, bool)>;

pub struct WelcomeState {
    tab: Tab,
    /// Folder shown by the built-in folder browser.
    browser_dir: PathBuf,
    /// Path typed in the folder browser / New Project location.
    path_input: String,
    new_project_name: String,
    vcs_url: String,
    import_path: String,
    err: String,
    /// Filter of the recent projects list (RF-PRJ-04).
    search: String,
    /// Recent project under the keyboard cursor.
    selected: usize,
    /// Put the keyboard in the search field on the next frame.
    pub focus_search: bool,
    dialog: Option<HomeDialog>,
    /// Native folder chooser running, and what the folder is for.
    picker: Option<(PickFor, JobHandle<Picked>)>,
    /// Sub-folders of `browser_dir`: `(path, looks like a project)`.
    listing: Option<(PathBuf, Folders)>,
    listing_job: Option<(PathBuf, JobHandle<Folders>)>,
    /// Whether each recent project still exists, in settings order.
    recent_exists: Option<(Vec<String>, Vec<bool>)>,
    recent_job: Option<(Vec<String>, JobHandle<Vec<bool>>)>,
    clone_job: Option<(String, JobHandle<PathBuf>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HomeDialog {
    /// Built-in folder browser (no native chooser on this desktop).
    Browse(PickFor),
    NewProject,
    Vcs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickFor {
    /// Open the folder as a project.
    Open,
    /// Location of a new project or a clone.
    Location,
}

impl WelcomeState {
    pub fn new() -> Self {
        let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
        WelcomeState {
            tab: Tab::Projects,
            browser_dir: home.clone(),
            path_input: home.display().to_string(),
            new_project_name: "new-project".to_string(),
            vcs_url: String::new(),
            import_path: String::new(),
            err: String::new(),
            search: String::new(),
            selected: 0,
            focus_search: true,
            dialog: None,
            picker: None,
            listing: None,
            listing_job: None,
            recent_exists: None,
            recent_job: None,
            clone_job: None,
        }
    }
}

impl Default for WelcomeState {
    fn default() -> Self {
        Self::new()
    }
}

/// Starts and collects the Welcome screen's background work: folder
/// listing, recent-project checks and `git clone`.
fn poll_jobs(app: &mut PhpIdeApp, ctx: &egui::Context) -> Option<PathBuf> {
    let state = &mut app.welcome;
    let dir = state.browser_dir.clone();
    let listing_current = matches!(&state.listing, Some((d, _)) if d == &dir);
    let listing_pending = matches!(&state.listing_job, Some((d, _)) if d == &dir);
    let browsing = matches!(state.dialog, Some(HomeDialog::Browse(_)));
    if browsing && !listing_current && !listing_pending {
        let target = dir.clone();
        let ctx = ctx.clone();
        let job = app.jobs.spawn(move |_| {
            let folders = list_folders(&target);
            ctx.request_repaint();
            Ok(folders)
        });
        state.listing_job = Some((dir, job));
    }
    if let Some((dir, job)) = &mut state.listing_job {
        if let Some(outcome) = job.poll() {
            if let JobOutcome::Ok(folders) = outcome {
                state.listing = Some((dir.clone(), folders));
            }
            state.listing_job = None;
        }
    }

    let recents = app.settings.recent_projects.clone();
    let recent_current = matches!(&state.recent_exists, Some((r, _)) if r == &recents);
    let recent_pending = matches!(&state.recent_job, Some((r, _)) if r == &recents);
    if !recent_current && !recent_pending {
        let paths = recents.clone();
        let ctx = ctx.clone();
        let job = app.jobs.spawn(move |_| {
            let exists = paths.iter().map(|p| Path::new(p).is_dir()).collect();
            ctx.request_repaint();
            Ok(exists)
        });
        state.recent_job = Some((recents, job));
    }
    if let Some((paths, job)) = &mut state.recent_job {
        if let Some(outcome) = job.poll() {
            if let JobOutcome::Ok(exists) = outcome {
                state.recent_exists = Some((paths.clone(), exists));
            }
            state.recent_job = None;
        }
    }

    let mut cloned = None;
    if let Some((_, job)) = &mut state.clone_job {
        if let Some(outcome) = job.poll() {
            match outcome {
                JobOutcome::Ok(target) => cloned = Some(target),
                JobOutcome::Err(error) => state.err = error.message,
                JobOutcome::Cancelled => state.err = "clone cancelled".to_string(),
            }
            state.clone_job = None;
        }
    }
    cloned
}

pub fn welcome_screen(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    if let Some(target) = poll_jobs(app, &ctx) {
        app.welcome.dialog = None;
        app.open_project_at(target, &ctx);
        return;
    }
    // A folder dropped on the window opens as a project (RF-PRJ-02).
    let dropped = ctx.input(|i| {
        i.raw
            .dropped_files
            .iter()
            .map(|f| f.path().to_path_buf())
            .find(|p| p.is_dir())
    });
    if let Some(folder) = dropped {
        app.open_project_at(folder, &ctx);
        return;
    }
    egui::CentralPanel::default().show(ui, |ui| {
        let avail = ui.available_size();
        let w = avail.x.min(900.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(avail.y * 0.08);
                ui.set_max_width(w);
                ui.horizontal(|ui| {
                    ui.heading("PHP IDE");
                    if app.project_open {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let name = app
                                .ws
                                .root
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            if Button::new(format!("Back to {name}")).show(ui).clicked() {
                                app.welcome_open = false;
                            }
                        });
                    }
                });
                ui.add_space(18.0);

                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(150.0);
                        let tabs = [
                            (Tab::Projects, "Projects", true),
                            (Tab::Customize, "Customize", true),
                            (Tab::Plugins, "Plugins", crate::scope::EXPERIMENTAL),
                            (Tab::Learn, "Learn", crate::scope::EXPERIMENTAL),
                            (
                                Tab::RemoteDevelopment,
                                "Remote Development",
                                crate::scope::EXPERIMENTAL,
                            ),
                        ];
                        for (tab, label, available) in tabs {
                            if available {
                                ui.selectable_value(&mut app.welcome.tab, tab, label);
                            }
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_max_width(w - 175.0);
                        let mut close = false;
                        match app.welcome.tab {
                            Tab::Projects => projects_tab(app, ui),
                            Tab::Customize => customize_tab(app, ui),
                            Tab::Plugins => plugins_tab(app, ui),
                            Tab::Learn => learn_tab(ui),
                            Tab::RemoteDevelopment => remote_development_tab(app, ui, &mut close),
                        }
                        if close {
                            app.welcome_open = false;
                        }
                    });
                });
            });
        });
    });
    dialogs(app, &ctx);
}

/// Starts the native folder chooser, or the built-in browser when the
/// desktop has none.
fn pick_folder(app: &mut PhpIdeApp, ctx: &egui::Context, purpose: PickFor) {
    if app.welcome.picker.is_some() {
        return;
    }
    let start = match purpose {
        PickFor::Open => app
            .settings
            .recent_projects
            .first()
            .and_then(|p| Path::new(p).parent().map(Path::to_path_buf))
            .unwrap_or_else(|| app.welcome.browser_dir.clone()),
        PickFor::Location => PathBuf::from(&app.welcome.path_input),
    };
    let title = match purpose {
        PickFor::Open => "Open Project",
        PickFor::Location => "Choose a Location",
    };
    let repaint = ctx.clone();
    let job = app.jobs.spawn(move |_| {
        let picked = crate::services::desktop::pick_folder(title, &start);
        repaint.request_repaint();
        Ok(picked)
    });
    app.welcome.picker = Some((purpose, job));
}

/// Collects the native folder chooser's answer (called every frame).
pub fn poll_picker(app: &mut PhpIdeApp, ctx: &egui::Context) {
    let Some((purpose, job)) = &mut app.welcome.picker else {
        return;
    };
    let purpose = *purpose;
    let Some(outcome) = job.poll() else {
        return;
    };
    app.welcome.picker = None;
    match outcome {
        JobOutcome::Ok(Picked::Folder(folder)) => match purpose {
            PickFor::Open => app.open_project_at(folder, ctx),
            PickFor::Location => {
                app.welcome.path_input = folder.display().to_string();
                app.welcome.browser_dir = folder;
            }
        },
        JobOutcome::Ok(Picked::Unavailable) => {
            if purpose == PickFor::Open {
                app.welcome_open = true;
            }
            app.welcome.dialog = Some(HomeDialog::Browse(purpose));
        }
        _ => {}
    }
}

impl PhpIdeApp {
    /// File › Open Project: the folder chooser (RF-PRJ-02).
    pub fn open_project_dialog(&mut self, ctx: &egui::Context) {
        self.welcome.dialog = None;
        self.welcome.err.clear();
        pick_folder(self, ctx, PickFor::Open);
    }
}

/// "Open in this window or a new one?" when a project is already open
/// (RF-PRJ-08).
pub fn open_prompt_dialog(app: &mut PhpIdeApp, ctx: &egui::Context) {
    let Some(target) = app.open_prompt.clone() else {
        return;
    };
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| target.display().to_string());
    let mut choice: Option<&'static str> = None;
    let mut remember = false;
    let remember_id = egui::Id::new("open-prompt-remember");
    ctx.data(|d| remember = d.get_temp(remember_id).unwrap_or(false));
    let shown = Dialog::new("open-prompt", "Open Project")
        .width(460.0)
        .show(
            ctx,
            |ui| {
                ui.label(format!("Where should \"{name}\" open?"));
                ui.weak("Attach adds it to this window's Project view and Go to File.");
                ui.add_space(6.0);
                crate::design::components::Checkbox::new(&mut remember, "Remember my choice")
                    .show(ui);
                ui.weak("Change it later in Settings › Appearance & Behavior › System Settings.");
            },
            |ui| {
                if Button::primary("New Window").show(ui).clicked() {
                    choice = Some("new");
                }
                if Button::new("This Window").show(ui).clicked() {
                    choice = Some("current");
                }
                if Button::new("Attach").show(ui).clicked() {
                    choice = Some("attach");
                }
                if Button::new("Cancel").show(ui).clicked() {
                    choice = Some("cancel");
                }
            },
        );
    ctx.data_mut(|d| d.insert_temp(remember_id, remember));
    if shown.dismissed {
        choice = Some("cancel");
    }
    let Some(choice) = choice else {
        return;
    };
    app.open_prompt = None;
    if remember && choice != "cancel" && choice != "attach" {
        app.settings.open_project_mode = choice.to_string();
        app.persist_settings();
    }
    match choice {
        "new" => app.open_in_new_window(&target),
        "current" => app.request_load_project(target, ctx),
        "attach" => {
            app.attach_project(&target);
            app.welcome_open = false;
        }
        _ => {}
    }
}

fn projects_tab(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let theme = crate::design::current(&ctx);
    let p = theme.palette;
    ui.horizontal(|ui| {
        let search_width = (ui.available_width() - 330.0).max(160.0);
        let search = TextInput::search(&mut app.welcome.search)
            .hint("Search projects")
            .width(search_width)
            .id_salt("home-search")
            .show(ui);
        // Keyboard: type to filter, ↑/↓ to pick, Enter to open.
        if app.welcome.focus_search && app.welcome.dialog.is_none() {
            search.request_focus();
            app.welcome.focus_search = false;
        }
        if search.has_focus() {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    search.id,
                    egui::EventFilter {
                        vertical_arrows: true,
                        ..Default::default()
                    },
                )
            });
        }
        if search.changed() {
            app.welcome.selected = 0;
        }
        ui.add_space(8.0);
        if Button::new("Get from VCS").show(ui).clicked() {
            app.welcome.err.clear();
            app.welcome.dialog = Some(HomeDialog::Vcs);
        }
        if Button::new("New Project").show(ui).clicked() {
            app.welcome.err.clear();
            app.welcome.dialog = Some(HomeDialog::NewProject);
        }
        let picking = app.welcome.picker.is_some();
        if Button::primary(if picking { "Opening…" } else { "Open" })
            .enabled(!picking)
            .show(ui)
            .clicked()
        {
            app.welcome.err.clear();
            pick_folder(app, &ctx, PickFor::Open);
        }
    });
    ui.add_space(12.0);
    if !app.welcome.err.is_empty() && app.welcome.dialog.is_none() {
        ui.colored_label(p.error, &app.welcome.err);
        ui.add_space(8.0);
    }

    let recents = app.settings.recent_projects.clone();
    let exists_flags = app
        .welcome
        .recent_exists
        .as_ref()
        .filter(|(paths, _)| paths == &recents)
        .map(|(_, flags)| flags.clone());
    let query = app.welcome.search.trim().to_lowercase();
    let shown: Vec<(usize, &String)> = recents
        .iter()
        .enumerate()
        .filter(|(_, path)| query.is_empty() || path.to_lowercase().contains(&query))
        .collect();
    if recents.is_empty() {
        crate::design::components::EmptyState::new(IconId::Folder, "No recent projects")
            .description("Open a folder or drop it on this window to start.")
            .show(ui);
        return;
    }
    if shown.is_empty() {
        ui.weak("No project matches the search.");
        return;
    }
    let keyboard = app.welcome.dialog.is_none() && app.open_prompt.is_none();
    if keyboard {
        ui.input(|i| {
            if i.key_pressed(egui::Key::ArrowDown) {
                app.welcome.selected = (app.welcome.selected + 1).min(shown.len() - 1);
            }
            if i.key_pressed(egui::Key::ArrowUp) {
                app.welcome.selected = app.welcome.selected.saturating_sub(1);
            }
        });
    }
    app.welcome.selected = app.welcome.selected.min(shown.len() - 1);
    let enter = keyboard && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let mut action: Option<RecentAction> = None;
    for (row, (i, path)) in shown.into_iter().enumerate() {
        let exists = exists_flags
            .as_ref()
            .and_then(|flags| flags.get(i).copied())
            .unwrap_or(true);
        let name = Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        let current = app.project_open && Path::new(path) == app.ws.root;
        let selected = row == app.welcome.selected;
        if let Some(a) = recent_row(ui, path, &name, exists, current, selected) {
            action = Some(a);
        }
        if selected && enter && exists {
            action = Some(RecentAction::Open(path.clone()));
        }
    }
    match action {
        Some(RecentAction::Open(path)) => app.open_project_at(PathBuf::from(path), &ctx),
        Some(RecentAction::NewWindow(path)) => app.open_in_new_window(Path::new(&path)),
        Some(RecentAction::Remove(path)) => {
            app.settings.forget_project(&path);
            app.persist_settings();
        }
        Some(RecentAction::Copy(path)) => ctx.copy_text(path),
        Some(RecentAction::ShowInFiles(path)) => {
            if let Err(e) = crate::services::desktop::open_folder(Path::new(&path)) {
                app.welcome.err = e;
            }
        }
        None => {}
    }
}

enum RecentAction {
    Open(String),
    NewWindow(String),
    Remove(String),
    Copy(String),
    ShowInFiles(String),
}

/// One recent project: name over its path, a remove button on hover and a
/// context menu (RF-PRJ-03, RF-PRJ-05).
fn recent_row(
    ui: &mut egui::Ui,
    path: &str,
    name: &str,
    exists: bool,
    current: bool,
    selected: bool,
) -> Option<RecentAction> {
    let theme = crate::design::current(ui.ctx());
    let p = theme.palette;
    let height = 44.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, exists, format!("Open {name}"))
    });
    let remove_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 20.0, rect.center().y),
        egui::vec2(24.0, 24.0),
    );
    let remove = ui.interact(
        remove_rect,
        response.id.with("remove"),
        egui::Sense::click(),
    );
    remove.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("Remove {name} from Recent Projects"),
        )
    });
    let hovered = response.hovered() || remove.hovered();
    if ui.is_rect_visible(rect) {
        if selected {
            ui.painter().rect_filled(rect, 6.0, p.bg_selected);
        } else if hovered {
            ui.painter().rect_filled(rect, 6.0, p.bg_hover);
        }
        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 22.0, rect.center().y),
            egui::vec2(20.0, 20.0),
        );
        crate::design::components::paint_icon(
            ui.painter(),
            icon_rect,
            if exists {
                IconId::Project
            } else {
                IconId::Warning
            },
            if exists { p.accent } else { p.warning },
        );
        let x = rect.left() + 44.0;
        let title = if current {
            format!("{name}  (open)")
        } else if exists {
            name.to_string()
        } else {
            format!("{name}  (missing)")
        };
        ui.painter().text(
            egui::pos2(x, rect.top() + 7.0),
            egui::Align2::LEFT_TOP,
            title,
            egui::FontId::proportional(14.0),
            if exists { p.text_primary } else { p.text_muted },
        );
        ui.painter().text(
            egui::pos2(x, rect.bottom() - 7.0),
            egui::Align2::LEFT_BOTTOM,
            path,
            egui::FontId::proportional(12.0),
            p.text_muted,
        );
        if hovered {
            crate::design::components::paint_icon(
                ui.painter(),
                remove_rect.shrink(4.0),
                IconId::Close,
                if remove.hovered() {
                    p.text_primary
                } else {
                    p.text_muted
                },
            );
        }
    }
    let mut action = None;
    if remove.clicked() {
        action = Some(RecentAction::Remove(path.to_string()));
    } else if response.clicked() && exists {
        action = Some(RecentAction::Open(path.to_string()));
    }
    response.context_menu(|ui| {
        ui.set_min_width(240.0);
        if menu_item(ui, None, "Open", "", exists).clicked() {
            action = Some(RecentAction::Open(path.to_string()));
            ui.close();
        }
        if menu_item(ui, None, "Open in New Window", "", exists).clicked() {
            action = Some(RecentAction::NewWindow(path.to_string()));
            ui.close();
        }
        if menu_item(ui, None, "Show in Files", "", exists).clicked() {
            action = Some(RecentAction::ShowInFiles(path.to_string()));
            ui.close();
        }
        if menu_item(ui, None, "Copy Path", "", true).clicked() {
            action = Some(RecentAction::Copy(path.to_string()));
            ui.close();
        }
        menu_separator(ui);
        if menu_item(ui, None, "Remove from Recent Projects", "", true).clicked() {
            action = Some(RecentAction::Remove(path.to_string()));
            ui.close();
        }
    });
    action
}

/// Dialogs of the Home screen: folder browser, New Project, Get from VCS.
fn dialogs(app: &mut PhpIdeApp, ctx: &egui::Context) {
    let Some(dialog) = app.welcome.dialog else {
        return;
    };
    let mut close = false;
    let error = app.welcome.err.clone();
    let error_color = crate::design::current(ctx).palette.error;
    match dialog {
        HomeDialog::Browse(purpose) => {
            let mut chosen: Option<PathBuf> = None;
            let mut confirm = false;
            let title = if purpose == PickFor::Open {
                "Open Project"
            } else {
                "Choose a Location"
            };
            let shown = Dialog::new("home-browse", title).width(520.0).show(
                ctx,
                |ui| {
                    ui.horizontal(|ui| {
                        if IconButton::new(IconId::ChevronUp, "Parent folder")
                            .show(ui)
                            .clicked()
                        {
                            if let Some(parent) = app.welcome.browser_dir.parent() {
                                app.welcome.browser_dir = parent.to_path_buf();
                                app.welcome.path_input =
                                    app.welcome.browser_dir.display().to_string();
                            }
                        }
                        let response = TextInput::new(&mut app.welcome.path_input)
                            .id_salt("home-browse-path")
                            .show(ui);
                        if response.lost_focus() {
                            let typed = PathBuf::from(&app.welcome.path_input);
                            if typed.is_dir() {
                                app.welcome.browser_dir = typed;
                            }
                        }
                    });
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical()
                        .max_height(300.0)
                        .show(ui, |ui| {
                            let entries = match &app.welcome.listing {
                                Some((dir, folders)) if dir == &app.welcome.browser_dir => {
                                    folders.clone()
                                }
                                _ => {
                                    ui.spinner();
                                    Vec::new()
                                }
                            };
                            let theme = crate::design::current(ui.ctx());
                            for (path, is_project) in entries {
                                let name = path.file_name().unwrap_or_default().to_string_lossy();
                                let row = crate::design::components::ListRow::new(name)
                                    .icon(
                                        if is_project {
                                            IconId::Project
                                        } else {
                                            IconId::Folder
                                        },
                                        if is_project {
                                            theme.palette.accent
                                        } else {
                                            theme.palette.folder
                                        },
                                    )
                                    .show(ui);
                                if row.double_clicked() && purpose == PickFor::Open && is_project {
                                    chosen = Some(path.clone());
                                } else if row.clicked() || row.double_clicked() {
                                    app.welcome.browser_dir = path.clone();
                                    app.welcome.path_input = path.display().to_string();
                                }
                            }
                        });
                    if !error.is_empty() {
                        ui.colored_label(error_color, &error);
                    }
                },
                |ui| {
                    let label = if purpose == PickFor::Open {
                        "Open"
                    } else {
                        "Choose"
                    };
                    if Button::primary(label).show(ui).clicked() {
                        confirm = true;
                    }
                    if Button::new("Cancel").show(ui).clicked() {
                        close = true;
                    }
                },
            );
            close |= shown.dismissed;
            if confirm {
                chosen = Some(PathBuf::from(app.welcome.path_input.trim()));
            }
            if let Some(dir) = chosen {
                if dir.is_dir() {
                    close = true;
                    match purpose {
                        PickFor::Open => app.open_project_at(dir, ctx),
                        PickFor::Location => {
                            app.welcome.browser_dir = dir;
                            app.welcome.dialog = Some(HomeDialog::NewProject);
                            return;
                        }
                    }
                } else {
                    app.welcome.err = format!("folder does not exist: {}", dir.display());
                }
            }
        }
        HomeDialog::NewProject => {
            let mut create = false;
            let shown = Dialog::new("home-new", "New Project").width(480.0).show(
                ctx,
                |ui| {
                    ui.label("Location");
                    ui.horizontal(|ui| {
                        TextInput::new(&mut app.welcome.path_input)
                            .width((ui.available_width() - 100.0).max(120.0))
                            .id_salt("home-new-location")
                            .show(ui);
                        if Button::new("Browse…").show(ui).clicked() {
                            pick_folder(app, ui.ctx(), PickFor::Location);
                        }
                    });
                    ui.add_space(6.0);
                    ui.label("Name");
                    TextInput::new(&mut app.welcome.new_project_name)
                        .id_salt("home-new-name")
                        .show(ui);
                    ui.add_space(4.0);
                    ui.weak(
                        "Creates composer.json (PSR-4 App\\ → src/), src/index.php and .gitignore.",
                    );
                    if !error.is_empty() {
                        ui.colored_label(error_color, &error);
                    }
                },
                |ui| {
                    if Button::primary("Create").show(ui).clicked() {
                        create = true;
                    }
                    if Button::new("Cancel").show(ui).clicked() {
                        close = true;
                    }
                },
            );
            close |= shown.dismissed;
            if create {
                let name = app.welcome.new_project_name.trim().trim_start_matches('/');
                if name.is_empty() || name.contains("..") || name.contains('/') {
                    app.welcome.err = "invalid project name".to_string();
                } else {
                    let target = PathBuf::from(&app.welcome.path_input).join(name);
                    if target.exists() {
                        app.welcome.err = format!("{} already exists", target.display());
                    } else {
                        match create_project(&target) {
                            Ok(()) => {
                                close = true;
                                app.open_project_at(target, ctx);
                            }
                            Err(e) => {
                                app.welcome.err = format!("could not create the project: {e}")
                            }
                        }
                    }
                }
            }
        }
        HomeDialog::Vcs => {
            let mut clone = false;
            let mut stop = false;
            let cloning = app.welcome.clone_job.is_some();
            let shown = Dialog::new("home-vcs", "Get from Version Control")
                .width(520.0)
                .show(
                    ctx,
                    |ui| {
                        ui.label("Repository URL");
                        TextInput::new(&mut app.welcome.vcs_url)
                            .hint("https://github.com/owner/repo.git")
                            .id_salt("home-vcs-url")
                            .show(ui);
                        ui.add_space(6.0);
                        ui.label("Directory");
                        ui.horizontal(|ui| {
                            TextInput::new(&mut app.welcome.path_input)
                                .width((ui.available_width() - 100.0).max(120.0))
                                .id_salt("home-vcs-dir")
                                .show(ui);
                            if Button::new("Browse…").show(ui).clicked() {
                                pick_folder(app, ui.ctx(), PickFor::Location);
                            }
                        });
                        if let Some((url, _)) = &app.welcome.clone_job {
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(format!("Cloning {url}…"));
                            });
                        }
                        if !error.is_empty() {
                            ui.colored_label(error_color, &error);
                        }
                    },
                    |ui| {
                        if cloning {
                            if Button::new("Stop").show(ui).clicked() {
                                stop = true;
                            }
                        } else {
                            if Button::primary("Clone").show(ui).clicked() {
                                clone = true;
                            }
                            if Button::new("Cancel").show(ui).clicked() {
                                close = true;
                            }
                        }
                    },
                );
            close |= shown.dismissed && !cloning;
            if stop {
                if let Some((_, job)) = &app.welcome.clone_job {
                    job.cancel();
                }
            }
            if clone {
                let base = PathBuf::from(&app.welcome.path_input);
                match clone_target(&app.welcome.vcs_url, &base) {
                    Ok(target) => {
                        let url = app.welcome.vcs_url.trim().to_string();
                        let job_url = url.clone();
                        let repaint = ctx.clone();
                        let job = app.jobs.spawn(move |job| {
                            let result = run_clone(job, &job_url, &target);
                            repaint.request_repaint();
                            result
                        });
                        app.welcome.err.clear();
                        app.welcome.clone_job = Some((url, job));
                    }
                    Err(error) => app.welcome.err = error,
                }
            }
        }
    }
    if close {
        app.welcome.dialog = None;
        app.welcome.err.clear();
    }
}

fn customize_tab(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("Theme").strong());
    ui.add_space(4.0);
    for theme in ThemeId::ALL {
        let sel = ThemeId::from_name(&app.settings.theme) == theme;
        if ui.selectable_label(sel, theme.label()).clicked() {
            app.apply_theme(theme, ui.ctx());
        }
    }
    ui.add_space(14.0);
    ui.label(egui::RichText::new("Accessibility").strong());
    ui.add_space(4.0);
    ui.checkbox(&mut app.zoom_on_ctrl_wheel, "Zoom with Ctrl+mouse wheel");
    ui.add_space(14.0);
    ui.label(egui::RichText::new("Import Settings").strong());
    ui.label("Path to a settings.json exported by PHP IDE:");
    ui.text_edit_singleline(&mut app.welcome.import_path);
    if ui.button("Import Settings...").clicked() {
        match import_settings(&app.welcome.import_path) {
            Ok(()) => {
                app.settings = crate::settings::Settings::load();
                app.zoom = app.settings.zoom;
                let theme = ThemeId::from_name(&app.settings.theme);
                app.apply_theme(theme, ui.ctx());
                app.welcome.err = "settings imported".to_string();
            }
            Err(error) => app.welcome.err = error,
        }
    }
}

fn plugins_tab(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("Installed").strong());
    if app.plugins.is_empty() {
        ui.label("No plugins installed in .phpide/plugins or in the global plugins folder.");
    } else {
        for plugin in &app.plugins {
            ui.horizontal(|ui| {
                ui.label("OK");
                ui.label(&plugin.label);
                ui.label(egui::RichText::new(&plugin.id).weak().small());
            });
        }
    }
    ui.add_space(16.0);
}

fn learn_tab(ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("Learn PHP IDE").strong());
    ui.add_space(8.0);
    for (shortcut, action) in [
        ("Shift×2", "Search Everywhere"),
        ("Ctrl+Shift+N", "Go to File"),
        ("Ctrl+Shift+A", "Find Action"),
        ("Ctrl+Alt+S", "Settings"),
        ("Alt+1…0", "Tool windows"),
        ("Ctrl+Alt+3", "Database"),
        ("Alt+F12", "Terminal"),
        ("Ctrl+`", "Quick Switch Scheme"),
    ] {
        ui.horizontal(|ui| {
            ui.monospace(shortcut);
            ui.label(action);
        });
    }
}

fn remote_development_tab(app: &mut PhpIdeApp, ui: &mut egui::Ui, close: &mut bool) {
    ui.label(egui::RichText::new("Remote Development").strong());
    ui.label("Open a project mounted through WSL, SSHFS or a Dev Container.");
    ui.text_edit_singleline(&mut app.welcome.path_input);
    if ui.button("Open Remote Project").clicked() {
        let path = PathBuf::from(&app.welcome.path_input);
        if path.is_dir() {
            app.request_load_project(path, ui.ctx());
            *close = true;
        } else {
            app.welcome.err = "the mounted path is not accessible".to_string();
        }
    }
}

fn import_settings(source: &str) -> Result<(), String> {
    let source = PathBuf::from(source.trim());
    let text = std::fs::read_to_string(&source)
        .map_err(|error| format!("could not read {}: {error}", source.display()))?;
    serde_json::from_str::<serde_json::Value>(&text)
        .map_err(|error| format!("invalid settings.json: {error}"))?;
    let dir = crate::settings::config_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("could not create the config folder: {error}"))?;
    std::fs::write(dir.join("settings.json"), text)
        .map_err(|error| format!("could not import settings: {error}"))
}

/// Destination folder for cloning `url` into `base`.
fn clone_target(url: &str, base: &Path) -> Result<PathBuf, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("enter the repository URL".to_string());
    }
    let name = url
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("project")
        .trim_end_matches(".git");
    if name.is_empty() || name.contains("..") {
        return Err("could not determine the project name".to_string());
    }
    let target = base.join(name);
    if target.exists() {
        return Err(format!("folder already exists: {}", target.display()));
    }
    Ok(target)
}

/// Runs `git clone` in a background job; cancelling the job kills git.
fn run_clone(job: &JobContext, url: &str, target: &Path) -> Result<PathBuf, IdeError> {
    let error = |kind, message: String| {
        IdeError::new(
            Component::Git,
            "clone",
            kind,
            message,
            "check the URL and your access to the repository",
        )
    };
    let mut child = Command::new("git")
        .args(["clone", "--quiet", "--", url])
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| error(ErrorKind::SpawnFailed, format!("could not run git: {e}")))?;
    loop {
        if job.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_dir_all(target);
            return Err(error(ErrorKind::Cancelled, "clone cancelled".to_string()));
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(target.to_path_buf()),
            Ok(Some(status)) => {
                return Err(error(
                    ErrorKind::Other,
                    format!("git clone failed with {status}"),
                ))
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(error(ErrorKind::Other, format!("git clone failed: {e}"))),
        }
    }
}

/// Sub-folders of `dir`, sorted, each flagged when it looks like a project.
fn list_folders(dir: &Path) -> Folders {
    let mut folders: Folders = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|path| path.is_dir())
                .map(|path| {
                    let is_project =
                        path.join("composer.json").exists() || path.join(".git").exists();
                    (path, is_project)
                })
                .collect()
        })
        .unwrap_or_default();
    folders.sort();
    folders
}

/// Creates a minimal PHP project skeleton (New Project).
fn create_project(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir.join("src"))?;
    std::fs::write(
        dir.join("composer.json"),
        "{\n    \"name\": \"app/project\",\n    \"require\": { \"php\": \">=8.1\" },\n    \"autoload\": { \"psr-4\": { \"App\\\\\": \"src/\" } }\n}\n",
    )?;
    std::fs::write(
        dir.join("src/index.php"),
        "<?php\n\nrequire __DIR__ . '/../vendor/autoload.php';\n\necho 'Hello from PHP IDE!' . PHP_EOL;\n",
    )?;
    std::fs::write(dir.join(".gitignore"), "/vendor/\n/.idea/\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_project_scaffold() {
        let dir = std::env::temp_dir().join(format!("phpide-newproj-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        create_project(&dir).unwrap();
        assert!(dir.join("composer.json").exists());
        assert!(dir.join("src/index.php").exists());
        let composer = std::fs::read_to_string(dir.join("composer.json")).unwrap();
        assert!(composer.contains("psr-4"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
