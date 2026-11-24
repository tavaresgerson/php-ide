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

use crate::design::ThemeId;
use crate::PhpIdeApp;

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
    /// Diretório atual do browser embutido.
    browser_dir: PathBuf,
    /// Entrada de texto para caminho direto / nome de novo projeto.
    path_input: String,
    new_project_name: String,
    vcs_url: String,
    import_path: String,
    err: String,
    /// Sub-folders of `browser_dir`: `(path, looks like a project)`.
    listing: Option<(PathBuf, Folders)>,
    listing_job: Option<(PathBuf, JobHandle<Folders>)>,
    /// Whether each recent project still exists, in settings order.
    recent_exists: Option<(Vec<String>, Vec<bool>)>,
    recent_job: Option<(Vec<String>, JobHandle<Vec<bool>>)>,
    clone_job: Option<(String, JobHandle<PathBuf>)>,
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
    if !listing_current && !listing_pending {
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
        app.request_load_project(target, &ctx);
        app.welcome_open = false;
        return;
    }
    let mut close = false;
    egui::CentralPanel::default().show(ui, |ui| {
        let avail = ui.available_size();
        let w = avail.x.min(860.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(avail.y * 0.12);
                ui.set_max_width(w);
                ui.heading("PHP IDE");
                ui.add_space(18.0);

                ui.horizontal(|ui| {
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
                        match app.welcome.tab {
                            Tab::Projects => projects_tab(app, ui, &mut close),
                            Tab::Customize => customize_tab(app, ui),
                            Tab::Plugins => plugins_tab(app, ui),
                            Tab::Learn => learn_tab(ui),
                            Tab::RemoteDevelopment => remote_development_tab(app, ui, &mut close),
                        }
                    });
                });
            });
        });
    });
    if close {
        app.welcome_open = false;
    }
}

fn projects_tab(app: &mut PhpIdeApp, ui: &mut egui::Ui, close: &mut bool) {
    ui.columns(2, |cols| {
        // ---- Coluna esquerda: ações + recentes.
        cols[0].vertical(|ui| {
            ui.label(egui::RichText::new("Actions").strong());
            ui.add_space(6.0);
            if ui.button("Open").clicked() {
                let p = PathBuf::from(&app.welcome.path_input);
                if p.is_dir() {
                    app.request_load_project(p, ui.ctx());
                    *close = true;
                } else {
                    app.welcome.err = format!("folder does not exist: {}", p.display());
                }
            }
            if ui
                .button("New Project")
                .on_hover_text("Create the folder with composer.json and src/")
                .clicked()
            {
                let name = app.welcome.new_project_name.trim().trim_start_matches('/');
                if name.is_empty() || name.contains("..") {
                    app.welcome.err = "invalid project name".to_string();
                } else {
                    let base = PathBuf::from(&app.welcome.path_input);
                    let target = base.join(name);
                    match create_project(&target) {
                        Ok(()) => {
                            app.request_load_project(target, ui.ctx());
                            *close = true;
                        }
                        Err(e) => app.welcome.err = format!("could not create the project: {e}"),
                    }
                }
            }
            ui.add_space(8.0);
            ui.label("Get from VCS — Git repository URL:");
            ui.text_edit_singleline(&mut app.welcome.vcs_url);
            if let Some((url, _)) = &app.welcome.clone_job {
                let url = url.clone();
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("Cloning {url}…"));
                    if ui.button("Cancel").clicked() {
                        if let Some((_, job)) = &app.welcome.clone_job {
                            job.cancel();
                        }
                    }
                });
            } else if ui.button("Get from VCS").clicked() {
                let base = PathBuf::from(&app.welcome.path_input);
                match clone_target(&app.welcome.vcs_url, &base) {
                    Ok(target) => {
                        let url = app.welcome.vcs_url.trim().to_string();
                        let job_url = url.clone();
                        let ctx = ui.ctx().clone();
                        let job = app.jobs.spawn(move |job| {
                            let result = run_clone(job, &job_url, &target);
                            ctx.request_repaint();
                            result
                        });
                        app.welcome.err.clear();
                        app.welcome.clone_job = Some((url, job));
                    }
                    Err(error) => app.welcome.err = error,
                }
            }
            ui.add_space(16.0);

            ui.label(egui::RichText::new("Recent Projects").strong());
            ui.add_space(4.0);
            let recents = app.settings.recent_projects.clone();
            if recents.is_empty() {
                ui.label(egui::RichText::new("No recent projects.").weak());
            }
            let exists_flags = app
                .welcome
                .recent_exists
                .as_ref()
                .filter(|(paths, _)| paths == &recents)
                .map(|(_, flags)| flags.clone());
            for (i, p) in recents.iter().enumerate() {
                // Until the check finishes, assume the folder exists.
                let exists = exists_flags
                    .as_ref()
                    .and_then(|flags| flags.get(i).copied())
                    .unwrap_or(true);
                let label = if exists {
                    p.clone()
                } else {
                    format!("{p}  (missing)")
                };
                if ui.add_enabled(exists, egui::Button::new(label)).clicked() {
                    app.request_load_project(PathBuf::from(p), ui.ctx());
                    *close = true;
                }
            }
        });

        // ---- Coluna direita: browser embutido de pastas.
        cols[1].vertical(|ui| {
            ui.label(egui::RichText::new("Folders").strong());
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Up").on_hover_text("Parent folder").clicked() {
                    if let Some(parent) = app.welcome.browser_dir.parent() {
                        app.welcome.browser_dir = parent.to_path_buf();
                    }
                }
                let dir = app.welcome.browser_dir.display().to_string();
                if ui
                    .button(format!("Open {dir}"))
                    .on_hover_text("Open this folder as a project")
                    .clicked()
                {
                    app.request_load_project(app.welcome.browser_dir.clone(), ui.ctx());
                    *close = true;
                }
            });
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .show(ui, |ui| {
                    let entries = match &app.welcome.listing {
                        Some((dir, folders)) if dir == &app.welcome.browser_dir => folders.clone(),
                        _ => {
                            ui.spinner();
                            Vec::new()
                        }
                    };
                    for (path, is_project) in entries {
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        let label = if is_project {
                            format!("[PHP] {name}/")
                        } else {
                            format!("[DIR] {name}/")
                        };
                        if ui.selectable_label(false, label).clicked() {
                            app.welcome.browser_dir = path.clone();
                            app.welcome.path_input = path.display().to_string();
                        }
                    }
                });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label("Path:");
                ui.text_edit_singleline(&mut app.welcome.path_input);
            });
            ui.horizontal(|ui| {
                ui.label("New project name:");
                ui.text_edit_singleline(&mut app.welcome.new_project_name);
            });
            if !app.welcome.err.is_empty() {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(&app.welcome.err)
                        .color(crate::design::current(ui.ctx()).palette.error),
                );
            }
        });
    });
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
