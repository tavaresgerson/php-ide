//! Settings › PHP › Language Server: which engine analyses PHP, the one in
//! use, and its configuration file.

use eframe::egui;
use phpide_icons::IconId;
use phpide_lsp::engine::{EngineChoice, EngineKind};

use crate::design::components::Button;
use crate::lsp_engine;
use crate::PhpIdeApp;

/// Edits in progress on the page.
#[derive(Debug, Default)]
pub struct LspPageState {
    /// The custom command being typed (applied with Apply or Enter).
    pub command: Option<String>,
}

pub fn page(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let root = app.ws.root.clone();
    let project_choice = lsp_engine::project_choice(&root);
    let per_project = project_choice.is_some();
    let current = project_choice
        .clone()
        .unwrap_or_else(|| app.settings.lsp_choice());
    let mut chosen: Option<EngineChoice> = None;
    let mut scope_changed: Option<bool> = None;

    ui.label(egui::RichText::new("Engine").strong());
    for choice in EngineChoice::all() {
        let selected = current.id() == choice.id();
        if ui.radio(selected, choice.label()).clicked() && !selected {
            chosen = Some(match choice {
                EngineChoice::Custom(_) => EngineChoice::Custom(app.settings.lsp_command.clone()),
                other => other,
            });
        }
    }
    if let EngineChoice::Custom(command) = &current {
        let draft = app
            .settings_dialog
            .lsp
            .command
            .get_or_insert_with(|| command.clone());
        ui.horizontal(|ui| {
            let label = ui.label("Command:");
            let field = ui
                .add(
                    egui::TextEdit::singleline(draft)
                        .hint_text("phpactor language-server")
                        .desired_width(320.0),
                )
                .labelled_by(label.id);
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (Button::new("Apply").show(ui).clicked() || enter) && *draft != *command {
                chosen = Some(EngineChoice::Custom(draft.trim().to_string()));
            }
        });
        ui.label(
            egui::RichText::new("The program and its arguments; quote paths with spaces.").weak(),
        );
    } else {
        app.settings_dialog.lsp.command = None;
    }
    ui.add_space(6.0);
    let mut project_only = per_project;
    if app.project_open
        && ui
            .checkbox(&mut project_only, "Use this choice for this project only")
            .on_hover_text("Stored in .phpide/project.json; other projects keep the global choice")
            .changed()
    {
        scope_changed = Some(project_only);
    }
    if std::env::var_os("PHPIDE_LSP").is_some() {
        ui.label(egui::RichText::new("PHPIDE_LSP is set: it wins over this choice.").weak());
    }

    ui.add_space(12.0);
    ui.label(egui::RichText::new("In use").strong());
    in_use(app, ui);

    // Apply after drawing: the choice restarts the server.
    if let Some(project_only) = scope_changed {
        let result = if project_only {
            lsp_engine::set_project_choice(&root, Some(&current))
        } else {
            lsp_engine::set_project_choice(&root, None)
        };
        match result {
            Ok(()) => {
                // Leaving the project choice falls back to the global one.
                if !project_only && current != app.settings.lsp_choice() {
                    app.restart_lsp();
                }
            }
            Err(e) => app.ws.message = format!("could not save .phpide/project.json: {e}"),
        }
    }
    if let Some(choice) = chosen {
        app.settings_dialog.lsp.command = None;
        if per_project {
            if let Err(e) = lsp_engine::set_project_choice(&root, Some(&choice)) {
                app.ws.message = format!("could not save .phpide/project.json: {e}");
                return;
            }
        } else {
            app.settings.set_lsp_choice(&choice);
            app.persist_settings();
        }
        if app.project_open {
            app.restart_lsp();
        }
    }
}

/// The server that runs now: name, version, program, and its actions.
fn in_use(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let caps = app.lsp.as_ref().map(|l| l.capabilities());
    match (&app.lsp_server, &app.lsp_start_error) {
        (_, Some(error)) => {
            let p = crate::design::current(ui.ctx()).palette;
            ui.colored_label(p.error, error);
        }
        (Some(server), None) => {
            let version = caps
                .as_ref()
                .map(|c| c.server_version.clone())
                .filter(|v| !v.is_empty())
                .map(|v| format!(" {v}"))
                .unwrap_or_default();
            ui.label(format!("{}{version}", app.lsp_name));
            ui.label(
                egui::RichText::new(format!("{} ({})", server.display(), server.origin.label()))
                    .monospace()
                    .small(),
            );
        }
        (None, None) if app.lsp.is_some() => {
            ui.label(app.lsp_name.clone());
        }
        (None, None) => {
            ui.label(egui::RichText::new("No project is open.").weak());
        }
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if Button::new("Restart")
            .icon(IconId::Refresh)
            .enabled(app.project_open)
            .show(ui)
            .clicked()
        {
            app.restart_lsp();
        }
        if Button::new("Show Log")
            .icon(IconId::FileText)
            .enabled(app.lsp.is_some())
            .show(ui)
            .clicked()
        {
            app.intel.log_open = true;
        }
        let phpantom = phpantom_program(app);
        if Button::new("Open .phpantom.toml")
            .icon(IconId::Settings)
            .enabled(app.project_open && phpantom.is_some())
            .show(ui)
            .on_hover_text("PHPantom's settings for this project; created when missing")
            .clicked()
        {
            if let Some(program) = phpantom {
                app.open_phpantom_config(&program);
            }
        }
    });
}

/// The PHPantom binary: the running one, else the one discovery finds.
fn phpantom_program(app: &PhpIdeApp) -> Option<std::path::PathBuf> {
    if let Some(server) = &app.lsp_server {
        if server.kind == EngineKind::Phpantom {
            return Some(server.program.clone());
        }
    }
    let root = app.ws.root.as_path();
    phpide_lsp::engine::Discovery {
        env_command: None,
        ..phpide_lsp::engine::Discovery::system(root)
    }
    .resolve(&EngineChoice::Phpantom)
    .ok()
    .map(|s| s.program)
}

impl PhpIdeApp {
    /// Opens the project's `.phpantom.toml`, created by `phpantom_lsp init
    /// --yes` when it is missing. The IDE never writes the file otherwise.
    pub(crate) fn open_phpantom_config(&mut self, program: &std::path::Path) {
        let file = self.ws.root.join(".phpantom.toml");
        if !file.exists() {
            let made = std::process::Command::new(program)
                .args(["init", "--yes"])
                .current_dir(&self.ws.root)
                .stdin(std::process::Stdio::null())
                .output();
            match made {
                Ok(out) if out.status.success() && file.exists() => {}
                Ok(out) => {
                    let err = String::from_utf8_lossy(&out.stderr);
                    self.ws.message = format!("could not create .phpantom.toml: {}", err.trim());
                    return;
                }
                Err(e) => {
                    self.ws.message = format!("could not run {}: {e}", program.display());
                    return;
                }
            }
        }
        self.settings_dialog.open = false;
        self.open_file_tab(&file, false);
    }
}
