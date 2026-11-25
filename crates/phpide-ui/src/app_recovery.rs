//! Hardening of the window (Phase 8): the log file, the crash notice, and
//! unsaved buffers kept on disk while the IDE runs.
//!
//! - Every two seconds, when the unsaved buffers changed, their text goes
//!   to the project's recovery folder (in a background job). A clean exit
//!   or closing the project empties it.
//! - Opening a project whose recovery folder has buffers (the IDE did not
//!   exit cleanly) offers them back.
//! - A crash report from the last run is shown once at startup.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use phpide_core::diag;
use phpide_core::jobs::JobHandle;
use phpide_core::recovery::{content_hash, RecoveryEntry, RecoveryStore};

use crate::design::components::{Button, Dialog};
use crate::PhpIdeApp;

/// How often unsaved buffers are written to the recovery folder.
const AUTOSAVE_EVERY: Duration = Duration::from_secs(2);

/// The state folder; tests get a folder of their own.
pub(crate) fn state_dir() -> PathBuf {
    #[cfg(test)]
    {
        static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        return DIR
            .get_or_init(|| {
                let dir =
                    std::env::temp_dir().join(format!("phpide-test-state-{}", std::process::id()));
                let _ = std::fs::create_dir_all(&dir);
                dir
            })
            .clone();
    }
    #[allow(unreachable_code)]
    diag::state_dir()
}

/// The recent log lines shared by the log file and the crash hook.
pub(crate) fn recent_lines() -> diag::RecentLines {
    static RECENT: std::sync::OnceLock<diag::RecentLines> = std::sync::OnceLock::new();
    RECENT.get_or_init(diag::RecentLines::default).clone()
}

/// The app's logger: `PHPIDE_LOG=<file>` keeps the old single file;
/// otherwise a rotating file in the state folder.
pub(crate) fn make_logger() -> phpide_core::Logger {
    if std::env::var_os("PHPIDE_LOG").is_some() || cfg!(test) {
        return phpide_core::Logger::stderr();
    }
    phpide_core::Logger::new(Box::new(diag::RotatingFileSink::new(
        &state_dir().join("logs"),
        2 * 1024 * 1024,
        3,
        recent_lines(),
    )))
}

/// Installs the crash hook (the binary only, not tests).
pub(crate) fn install_crash_hook() {
    diag::install_crash_hook(
        state_dir().join("crashes"),
        diag::BuildInfo {
            name: "PHP IDE",
            version: env!("CARGO_PKG_VERSION"),
        },
        recent_lines(),
    );
}

#[derive(Default)]
pub(crate) struct RecoveryState {
    store: Option<RecoveryStore>,
    /// `(buffer id, revision)` of what was written last.
    written: Vec<(u64, u64)>,
    last_tick: Option<Instant>,
    job: Option<JobHandle<()>>,
    /// Buffers found at startup, waiting for the user's answer.
    pub offered: Vec<RecoveryEntry>,
    /// A crash report from the last run.
    pub crash_report: Option<PathBuf>,
    /// The About dialog.
    pub about_open: bool,
}

impl RecoveryState {
    /// Recovery for `root`: what a crash left behind is offered, unless
    /// another window has the project open (its buffers are alive there).
    pub(crate) fn for_project(root: &Path, open_elsewhere: bool) -> Self {
        let store = RecoveryStore::new(&state_dir(), root);
        let offered = if open_elsewhere {
            Vec::new()
        } else {
            store.load()
        };
        RecoveryState {
            store: (!open_elsewhere).then_some(store),
            offered,
            ..Default::default()
        }
    }

    /// Empties the recovery folder (clean exit, project closed).
    pub(crate) fn clear(&mut self) {
        if let Some(store) = &self.store {
            let _ = store.clear();
        }
        self.written.clear();
    }
}

impl PhpIdeApp {
    /// Writes the unsaved buffers to the recovery folder when they changed.
    pub(crate) fn autosave_recovery(&mut self) {
        let state = &mut self.recovery;
        if let Some(job) = &mut state.job {
            if job.poll().is_none() {
                return;
            }
            state.job = None;
        }
        // Not before the user answered about what is there.
        if state.store.is_none() || !state.offered.is_empty() {
            return;
        }
        if state
            .last_tick
            .is_some_and(|at| at.elapsed() < AUTOSAVE_EVERY)
        {
            return;
        }
        state.last_tick = Some(Instant::now());
        let current: Vec<(u64, u64)> = self
            .ws
            .buffers
            .iter()
            .filter(|b| b.dirty)
            .map(|b| (b.id, b.document.revision()))
            .collect();
        if current == state.written {
            return;
        }
        let entries: Vec<RecoveryEntry> = self
            .ws
            .buffers
            .iter()
            .filter(|b| b.dirty)
            .map(|b| RecoveryEntry {
                path: b.path.clone(),
                name: b.name(),
                text: b.document.text(),
                disk_hash: b
                    .path
                    .as_ref()
                    .and_then(|p| std::fs::read(p).ok())
                    .map(|bytes| content_hash(&bytes)),
            })
            .collect();
        state.written = current;
        if state.store.is_none() {
            return;
        }
        let root = self.ws.root.clone();
        state.job = Some(self.jobs.spawn(move |_| {
            let _ = RecoveryStore::new(&state_dir(), &root).save(&entries);
            Ok(())
        }));
    }

    /// Opens the recovered buffers: files get the recovered text (unsaved),
    /// buffers never saved come back as new tabs.
    pub(crate) fn recover_buffers(&mut self) {
        let entries = std::mem::take(&mut self.recovery.offered);
        let mut changed_on_disk = Vec::new();
        for entry in &entries {
            match entry.path.as_ref().filter(|p| p.exists()) {
                Some(path) => {
                    let disk = std::fs::read(path).ok().map(|b| content_hash(&b));
                    if entry.disk_hash.is_some() && disk != entry.disk_hash {
                        changed_on_disk.push(entry.name.clone());
                    }
                    self.open_file_tab(path, false);
                    if let Some(buffer) = self.ws.active_buffer_mut() {
                        buffer.preview = false;
                        buffer.replace_all(&entry.text);
                    }
                }
                None => {
                    self.ws.new_file();
                    if let Some(buffer) = self.ws.buffers.last_mut() {
                        buffer.replace_all(&entry.text);
                    }
                    self.ws.active = self.ws.buffers.len() - 1;
                }
            }
        }
        self.recovery.written.clear();
        self.ws.message = if changed_on_disk.is_empty() {
            format!("Recovered {} unsaved file(s)", entries.len())
        } else {
            format!(
                "Recovered {} unsaved file(s); changed on disk since: {} — compare before saving",
                entries.len(),
                changed_on_disk.join(", ")
            )
        };
    }

    pub(crate) fn discard_recovery(&mut self) {
        self.recovery.offered.clear();
        self.recovery.clear();
    }

    /// Recovery offer, crash notice and About, when open.
    pub(crate) fn recovery_dialogs(&mut self, ctx: &egui::Context) {
        let p = crate::design::current(ctx).palette;
        if !self.recovery.offered.is_empty() {
            let mut recover = false;
            let mut discard = false;
            modal(ctx, "recover-buffers", 460.0, |ui| {
                ui.label(egui::RichText::new("Recover Unsaved Changes").strong());
                ui.add_space(6.0);
                ui.label(
                    "PHP IDE did not close normally. These files had changes that were not saved:",
                );
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .id_salt("recover-list")
                    .max_height(160.0)
                    .show(ui, |ui| {
                        for entry in &self.recovery.offered {
                            let label = entry
                                .path
                                .as_ref()
                                .map(|p| p.display().to_string())
                                .unwrap_or_else(|| format!("{} (never saved)", entry.name));
                            ui.label(egui::RichText::new(label).monospace());
                        }
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if Button::primary("Recover").show(ui).clicked() {
                        recover = true;
                    }
                    if Button::new("Discard").show(ui).clicked() {
                        discard = true;
                    }
                });
            });
            if recover {
                self.recover_buffers();
            } else if discard {
                self.discard_recovery();
            }
            return;
        }
        if let Some(report) = self.recovery.crash_report.clone() {
            let mut close = false;
            let mut open = false;
            modal(ctx, "crash-notice", 460.0, |ui| {
                ui.label(egui::RichText::new("PHP IDE Closed Unexpectedly").strong());
                ui.add_space(6.0);
                ui.label("A crash report was saved on this computer. Nothing was sent anywhere.");
                ui.label(
                    egui::RichText::new(report.display().to_string())
                        .monospace()
                        .color(p.text_secondary),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if Button::primary("Open Report").show(ui).clicked() {
                        open = true;
                    }
                    if Button::new("Close").show(ui).clicked() {
                        close = true;
                    }
                });
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
            });
            if open || close {
                let seen = diag::mark_seen(&report);
                self.recovery.crash_report = None;
                if open && self.project_open {
                    self.open_file_tab(&seen, false);
                }
            }
            return;
        }
        if self.recovery.about_open {
            let mut close = false;
            modal(ctx, "about", 420.0, |ui| {
                ui.label(egui::RichText::new("PHP IDE").heading().strong());
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.add_space(6.0);
                ui.label("An IDE for PHP, written in Rust.");
                ui.label(
                    egui::RichText::new(
                        "MIT OR Apache-2.0 · Tabler Icons (MIT) · Inter and JetBrains Mono (OFL)",
                    )
                    .color(p.text_secondary),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!("Logs: {}", state_dir().join("logs").display()))
                        .small()
                        .color(p.text_muted),
                );
                ui.add_space(8.0);
                if Button::primary("Close").show(ui).clicked()
                    || ui.input(|i| i.key_pressed(egui::Key::Escape))
                {
                    close = true;
                }
            });
            if close {
                self.recovery.about_open = false;
            }
        }
    }

    /// Help › Show Log: the log file in an editor tab.
    pub(crate) fn show_log(&mut self) {
        let file = state_dir().join("logs").join("phpide.log");
        if file.exists() {
            self.open_file_tab(&file, false);
            if let Some(buffer) = self.ws.active_buffer_mut() {
                buffer.readonly = true;
            }
        } else {
            self.ws.message = format!("No log yet at {}", file.display());
        }
    }

    /// Help › Show Log in Files.
    pub(crate) fn show_log_folder(&mut self) {
        let dir = state_dir().join("logs");
        let _ = std::fs::create_dir_all(&dir);
        if let Err(e) = crate::services::desktop::open_folder(&dir) {
            self.ws.message = format!("Could not open the log folder: {e}");
        }
    }
}

fn modal(ctx: &egui::Context, id: &str, width: f32, body: impl FnOnce(&mut egui::Ui)) {
    let p = crate::design::current(ctx).palette;
    let id = egui::Id::new(id);
    egui::Modal::new(id)
        .area(
            egui::Modal::default_area(id)
                .fixed_pos(ctx.content_rect().center() - egui::vec2(width / 2.0, 120.0)),
        )
        .backdrop_color(p.shadow)
        .frame(egui::Frame::NONE)
        .show(ctx, |ui| {
            Dialog::frame(ui).show(ui, |ui| {
                ui.set_width(width);
                body(ui);
            });
        });
}
