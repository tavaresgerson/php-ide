//! Context menus of the editor: a right click in the text, and one on the
//! gutter (line numbers, breakpoints, folding).
//!
//! The widget reports where the click landed (`EditorOutput::context`) and
//! has already moved the caret there, unless the click fell inside the
//! selection; the items then act on the caret like their shortcuts do.
//! Items a language server provides are hidden when the server does not.

use std::path::{Path, PathBuf};

use eframe::egui;
use phpide_editor::ContextTarget;
use phpide_icons::IconId;

use crate::commands::{self, Command};
use crate::design::components::{menu_check_item, menu_item, menu_separator, menu_submenu};
use crate::PhpIdeApp;

/// What a menu item asks for, run after the menu is drawn.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Act {
    Command(Command),
    /// Cut, Copy or Paste: the platform sends the event to the editor.
    Clipboard(egui::ViewportCommand),
    OpenInFiles(PathBuf),
    OpenInTerminal(PathBuf),
    Breakpoint(usize),
    Bookmark(usize),
}

/// What the menu needs to know about the place it opened on.
pub(crate) struct Context {
    pub target: ContextTarget,
    pub path: Option<PathBuf>,
    pub php: bool,
    pub selection: bool,
    pub readonly: bool,
}

/// Draws the menu for `context`; returns the item clicked, if any.
pub(crate) fn show(app: &PhpIdeApp, ui: &mut egui::Ui, context: &Context) -> Option<Act> {
    ui.set_min_width(260.0);
    match context.target {
        ContextTarget::Text(_) => text_menu(app, ui, context),
        ContextTarget::Gutter(line0) => gutter_menu(app, ui, context, line0),
    }
}

/// A command row: its icon and shortcut come from the registry.
fn command(ui: &mut egui::Ui, id: Command, enabled: bool) -> bool {
    let spec = commands::REGISTRY.iter().find(|s| s.id == id);
    let label = spec.map(|s| s.label).unwrap_or("");
    let icon = commands::icon(id);
    menu_item(ui, icon, label, &commands::shortcut_text(id), enabled).clicked()
}

fn text_menu(app: &PhpIdeApp, ui: &mut egui::Ui, context: &Context) -> Option<Act> {
    let caps = app
        .lsp
        .as_ref()
        .map(|l| l.capabilities())
        .unwrap_or_default();
    let mut chosen = None;
    let mut pick = |act: Act| chosen = Some(act);

    if caps.code_action_provider && command(ui, Command::ShowContextActions, true) {
        pick(Act::Command(Command::ShowContextActions));
    }
    if caps.code_action_provider {
        menu_separator(ui);
    }
    let editable = !context.readonly;
    if menu_item(
        ui,
        Some(IconId::Cut),
        "Cut",
        "Ctrl+X",
        context.selection && editable,
    )
    .clicked()
    {
        pick(Act::Clipboard(egui::ViewportCommand::RequestCut));
    }
    if menu_item(ui, Some(IconId::Copy), "Copy", "Ctrl+C", context.selection).clicked() {
        pick(Act::Clipboard(egui::ViewportCommand::RequestCopy));
    }
    if menu_item(ui, Some(IconId::Paste), "Paste", "Ctrl+V", editable).clicked() {
        pick(Act::Clipboard(egui::ViewportCommand::RequestPaste));
    }
    if context.path.is_some() {
        menu_submenu(ui, None, "Copy Path/Reference", true, |ui| {
            ui.set_min_width(240.0);
            for id in [Command::CopyPath, Command::CopyReference] {
                if command(ui, id, true) {
                    pick(Act::Command(id));
                }
            }
            if app.git.remote().is_some() && command(ui, Command::CopyRepositoryUrl, true) {
                pick(Act::Command(Command::CopyRepositoryUrl));
            }
        });
    }

    // Navigation and refactoring come from the language server.
    let go_to: Vec<Command> = [
        (caps.definition_provider, Command::GotoDefinition),
        (caps.implementation_provider, Command::GotoImplementation),
        (caps.type_definition_provider, Command::GotoTypeDeclaration),
        (caps.references_provider, Command::FindUsages),
    ]
    .into_iter()
    .filter_map(|(offered, id)| offered.then_some(id))
    .collect();
    let refactor = caps.rename_provider;
    if !go_to.is_empty() || refactor || caps.formatting_provider {
        menu_separator(ui);
    }
    if !go_to.is_empty() {
        menu_submenu(ui, None, "Go To", true, |ui| {
            ui.set_min_width(240.0);
            for id in go_to {
                if command(ui, id, true) {
                    pick(Act::Command(id));
                }
            }
        });
    }
    if refactor {
        menu_submenu(ui, None, "Refactor", true, |ui| {
            ui.set_min_width(200.0);
            if command(ui, Command::RenameSymbol, editable) {
                pick(Act::Command(Command::RenameSymbol));
            }
        });
    }
    if caps.formatting_provider && command(ui, Command::ReformatCode, editable) {
        pick(Act::Command(Command::ReformatCode));
    }

    // Running and debugging the file under the caret.
    if context.php {
        menu_separator(ui);
        let name = context
            .path
            .as_deref()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let run = format!("Run '{name}'");
        let debug = format!("Debug '{name}'");
        let shortcut = commands::shortcut_text(Command::RunContext);
        if menu_item(ui, Some(IconId::Run), &run, &shortcut, true).clicked() {
            pick(Act::Command(Command::RunContext));
        }
        let shortcut = commands::shortcut_text(Command::DebugContext);
        if menu_item(ui, Some(IconId::Debug), &debug, &shortcut, true).clicked() {
            pick(Act::Command(Command::DebugContext));
        }
        if command(ui, Command::ToggleBreakpoint, true) {
            pick(Act::Command(Command::ToggleBreakpoint));
        }
    }
    if context.path.is_some() && command(ui, Command::ToggleBookmark, true) {
        pick(Act::Command(Command::ToggleBookmark));
    }

    // Git and the file's folder.
    if let Some(path) = &context.path {
        menu_separator(ui);
        if app.git.is_repo() {
            menu_submenu(ui, Some(IconId::VersionControl), "Git", true, |ui| {
                ui.set_min_width(240.0);
                if blame_item(app, ui) {
                    pick(Act::Command(Command::ToggleBlame));
                }
                for id in [
                    Command::GitShowHistory,
                    Command::GitCompareWithHead,
                    Command::GitRollback,
                ] {
                    if command(ui, id, true) {
                        pick(Act::Command(id));
                    }
                }
            });
        }
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        menu_submenu(ui, Some(IconId::ExternalLink), "Open In", true, |ui| {
            ui.set_min_width(180.0);
            if menu_item(ui, Some(IconId::FolderOpen), "Files", "", true).clicked() {
                pick(Act::OpenInFiles(dir.clone()));
            }
            if menu_item(ui, Some(IconId::Terminal), "Terminal", "", true).clicked() {
                pick(Act::OpenInTerminal(dir.clone()));
            }
        });
    }
    chosen
}

fn gutter_menu(app: &PhpIdeApp, ui: &mut egui::Ui, context: &Context, line0: usize) -> Option<Act> {
    let mut chosen = None;
    let has_path = context.path.is_some();
    if context.php
        && menu_item(
            ui,
            Some(IconId::Breakpoint),
            &format!("Toggle Breakpoint at Line {}", line0 + 1),
            "",
            true,
        )
        .clicked()
    {
        chosen = Some(Act::Breakpoint(line0));
    }
    if has_path
        && menu_item(
            ui,
            Some(IconId::Bookmark),
            &format!("Toggle Bookmark at Line {}", line0 + 1),
            "",
            true,
        )
        .clicked()
    {
        chosen = Some(Act::Bookmark(line0));
    }
    if has_path && app.git.is_repo() {
        menu_separator(ui);
        if blame_item(app, ui) {
            chosen = Some(Act::Command(Command::ToggleBlame));
        }
    }
    chosen
}

/// Annotate with Git Blame, checked while the blame gutter is on.
fn blame_item(app: &PhpIdeApp, ui: &mut egui::Ui) -> bool {
    menu_check_item(
        ui,
        Some(IconId::Blame),
        "Annotate with Git Blame",
        "",
        app.settings.blame_gutter,
    )
    .clicked()
}

impl PhpIdeApp {
    /// Runs what an editor context menu item asked for.
    pub(crate) fn run_editor_act(&mut self, act: Act, path: Option<&Path>, ctx: &egui::Context) {
        match act {
            Act::Command(id) => self.dispatch_command(id, ctx),
            Act::Clipboard(request) => {
                // The event reaches the focused editor on the next frame.
                self.editors.request_focus();
                ctx.send_viewport_cmd(request);
            }
            Act::OpenInFiles(dir) => {
                self.ws.message = match crate::services::desktop::open_folder(&dir) {
                    Ok(()) => format!("Opened {}", dir.display()),
                    Err(e) => e,
                };
            }
            Act::OpenInTerminal(dir) => {
                self.ws.message = match crate::services::desktop::open_terminal(&dir) {
                    Ok(name) => format!("Opened {name} in {}", dir.display()),
                    Err(e) => e,
                };
            }
            Act::Breakpoint(line0) => {
                if let Some(path) = path {
                    let added = self.rd.toggle_breakpoint(path.to_path_buf(), line0 + 1);
                    self.ws.message = format!(
                        "breakpoint {} (Ln {})",
                        if added { "added" } else { "removed" },
                        line0 + 1
                    );
                }
            }
            Act::Bookmark(line0) => {
                if let Some(buffer) = self.ws.active_buffer() {
                    let rope = buffer.document.rope();
                    self.cursor_char =
                        rope.line_to_char(line0.min(rope.len_lines().saturating_sub(1)));
                }
                self.toggle_bookmark();
            }
        }
    }
}
