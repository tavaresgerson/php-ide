//! PHP IDE — a PHP-focused IDE written in Rust.
//!
//! Estágio 6 — shell de IDE com paridade visual à New UI do PhpStorm
//! (REQUISITOS.md, Parte 1): window header unificado, tool windows com
//! atalhos Alt+1..7, status bar com widgets, Search Everywhere (Shift×2),
//! Find Action (Ctrl+Shift+A), Quick Switch Scheme (Ctrl+`), Welcome Screen,
//! editor tabs com preview/pin/Closing Policy e breadcrumbs.

mod app_actions;
mod app_completion;
mod app_editor;
mod app_intel;
mod app_lsp;
mod app_panels;
mod app_project;
mod app_recovery;
mod app_vcs;
mod blame_gutter;
mod close;
mod color_scheme;
mod commands;
mod commit_panel;
mod completion;
mod database;
mod design;
mod diff_view;
mod editor_area;
mod editor_menu;
mod exec;
mod fe_servers;
mod header;
mod hierarchy;
mod hints;
mod intelligence;
mod keymap_page;
mod lens;
mod lsp_engine;
mod lsp_features;
mod markdown;
mod notifications;
mod palette;
mod plugins;
mod project_marks;
mod project_tree;
mod quick_preview;
mod recent_locations;
mod routing;
mod run_configs;
mod run_debug;
mod run_panels;
mod scheme;
mod scope;
mod search;
mod services;
mod settings;
mod settings_dialog;
mod settings_editor;
mod settings_lsp;
mod statusbar;
mod switcher;
mod tabs;
mod terminal_panel;
mod tool_panels;
mod toolwindows;
#[cfg(test)]
mod ui_tests;
mod vcs_panel;
mod welcome;
mod workspace_edit;

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

// Items shared between the app_* modules.
pub(crate) use app_editor::{active_path_matches, WsStore};
pub(crate) use app_lsp::short_kind;
pub(crate) use app_panels::tool_window_header;
pub(crate) use app_project::start_lsp;
use commands::Command;
use completion::CompletionState;
use design::ThemeId;
use eframe::egui;
use exec::ExecState;
use notify::Watcher as _;
use palette::{PaletteAction, PaletteMode, PaletteState};
use phpide_core::jobs::JobSystem;
use phpide_core::{
    log::{Correlation, Logger},
    Component, Revisioned, Workspace,
};
use phpide_dbgp::session::ContinueCommand;
use phpide_lsp::{LspClient, LspDiagnostic, LspStatus};
use search::SearchState;
use serde_json::Value;
use toolwindows::ToolWindow;
use welcome::WelcomeState;

/// Cache de semantic tokens: (path, revisão, tokens, spans de byte).
type SemanticCache = (
    PathBuf,
    u64,
    Vec<intelligence::SemanticToken>,
    Vec<(usize, usize, String)>,
);

/// Fechamento pendente que exige confirmação de buffers sujos (NFR-007:
/// "Dirty buffers precisam de confirmação ao fechar").
#[derive(Debug, Clone)]
pub enum PendingClose {
    /// Fechar a aba `i` (do contexto, Ctrl+F4 ou palette).
    Tab(usize),
    /// Close every tab (pinned ones too unless `keep_pinned`).
    All { keep_pinned: bool },
    /// Close the project and go back to the Home screen (RF-PRJ-12).
    Home,
    /// Fechar as outras abas (Close Others).
    Others(usize),
    /// Fechar todas as abas e abrir outro projeto (Welcome).
    Project(std::path::PathBuf),
    /// Sair da IDE (janela fechada com buffers sujos).
    Exit,
}

#[cfg(test)]
thread_local! {
    static DIRTY_BUTTON_RECTS: std::cell::Cell<Option<[egui::Rect; 3]>> =
        const { std::cell::Cell::new(None) };
}

/// Application id: the desktop file, AppStream and Flatpak id, and the
/// Wayland app id that ties windows to the desktop entry.
const APP_ID: &str = "io.github.tavaresgerson.PhpIde";

/// Window icon, from the same artwork as the desktop icon.
fn app_icon() -> Option<egui::IconData> {
    let bytes = include_bytes!("../assets/app-icon.png");
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).ok()?;
    let rgba = image.to_rgba8();
    Some(egui::IconData {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

fn main() -> eframe::Result<()> {
    // Uso: phpide [projeto] [arquivos...]. Sem projeto, abre a Welcome Screen;
    // arquivos extras são abertos em abas (preview) após carregar o projeto.
    app_recovery::install_crash_hook();
    let mut args = std::env::args().skip(1).peekable();
    match args.peek().map(String::as_str) {
        Some("--version" | "-V") => {
            println!("PHP IDE {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("--help" | "-h") => {
            println!(
                "PHP IDE {}\n\nUsage: phpide [PROJECT_DIR] [FILES...]\n\n\
                 Without a project the Welcome screen opens; FILES open in tabs.\n\n\
                 Options:\n  -h, --help     Show this help\n  -V, --version  Show the version\n  \
                 --gallery      Show every UI component\n\n\
                 Logs: {}",
                env!("CARGO_PKG_VERSION"),
                phpide_core::diag::log_dir().display()
            );
            return Ok(());
        }
        _ => {}
    }
    if args.peek().map(String::as_str) == Some("--gallery") {
        return eframe::run_native(
            "PHP IDE — Components",
            eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default()
                    .with_inner_size([1280.0, 900.0])
                    .with_app_id(APP_ID),
                ..Default::default()
            },
            Box::new(|cc| Ok(Box::new(design::gallery::GalleryApp::new(&cc.egui_ctx)))),
        );
    }
    let mut arg = args.next();
    let files: Vec<PathBuf> = args.map(PathBuf::from).collect();
    let loaded = settings::Settings::load();
    // Reopen the projects that were open at exit (RF-PRJ-11): this window
    // takes the first, each other one gets its own window.
    if arg.is_none() && loaded.reopen_last_project {
        let mut projects: Vec<PathBuf> = services::windows::WindowRegistry::new()
            .session()
            .into_iter()
            .filter(|p| p.is_dir())
            .collect();
        if projects.is_empty() && Path::new(&loaded.last_project).is_dir() {
            projects.push(PathBuf::from(&loaded.last_project));
        }
        if let Some(first) = projects.first() {
            arg = Some(first.display().to_string());
            if let Ok(exe) = std::env::current_exe() {
                for other in &projects[1..] {
                    let _ = std::process::Command::new(&exe).arg(other).spawn();
                }
            }
        }
    }
    let root = arg
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let title = if arg.is_some() {
        format!("PHP IDE — {}", root.display())
    } else {
        "PHP IDE".to_string()
    };
    let theme = settings_theme(&loaded);
    // Sem argumento de projeto, abre na Welcome Screen (REQUISITOS 1.5).
    let start_welcome = arg.is_none();

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([640.0, 480.0])
        .with_app_id(APP_ID);
    if let Some(icon) = app_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        &title,
        options,
        Box::new(move |cc| {
            design::install(&cc.egui_ctx, theme);
            Ok(Box::new(PhpIdeApp::new(root, start_welcome, files)))
        }),
    )
}

/// Design theme described by the settings (theme id, density, editor font).
fn settings_theme(settings: &settings::Settings) -> design::Theme {
    let density = if settings.compact_mode {
        design::Density::Compact
    } else {
        design::Density::Comfortable
    };
    let mut theme = design::Theme::new(ThemeId::from_name(&settings.theme), density);
    theme.editor_font_size = settings.editor_font_size;
    theme.editor_font_family = settings.editor_font_family.clone();
    theme.editor_line_height = settings.editor_line_height;
    theme.color_changes = std::sync::Arc::new(settings.color_scheme.clone());
    theme.reduced_motion = settings.reduced_motion;
    theme
}

type GitMarkerCache = (PathBuf, u64, Vec<(u32, bool)>);

struct PhpIdeApp {
    ws: Workspace,
    db: database::DatabaseState,
    tree: project_tree::TreeState,
    /// A project is loaded (false on the Home screen before the first one
    /// and after Close Project).
    project_open: bool,
    /// Asking how to open a second project (RF-PRJ-08).
    open_prompt: Option<PathBuf>,
    /// Select In popup (Alt+F1): selected row, when open.
    select_in: Option<usize>,
    /// Other PHP IDE windows and the session to reopen (RF-PRJ-10/11).
    windows: services::windows::WindowRegistry,
    /// Event Log behind the Notifications tool window (RF-UI-05).
    event_log: notifications::EventLog,
    toasts: design::components::Toasts,
    cursor_char: usize,
    palette: PaletteState,
    search: SearchState,
    zoom: f32,
    watcher: Option<(notify::RecommendedWatcher, mpsc::Receiver<()>)>,
    change_pending_since: Option<Instant>,
    // NFR-001 — jobs da sessão (busca, workers; shutdown ao trocar/fechar).
    jobs: JobSystem,
    // NFR-005 — log correlacionado por sessão/job/documento.
    logger: Logger,
    session: u64,
    // NFR-007 — confirmação de buffers sujos ao fechar.
    pending_close: Option<PendingClose>,
    // Estágio 3 — LSP
    lsp: Option<LspClient>,
    lsp_init: Option<mpsc::Receiver<Value>>,
    lsp_name: String,
    /// The command of the running server (Settings › PHP › Language Server).
    lsp_server: Option<phpide_lsp::engine::ServerCommand>,
    /// Why no PHP server runs (not found, could not start).
    lsp_start_error: Option<String>,
    /// Restarts after crashes, with backoff.
    lsp_supervisor: lsp_engine::Supervisor,
    /// PHPantom's file filters as last sent (initialize or
    /// didChangeConfiguration); `Null` for other servers.
    lsp_settings_sent: Value,
    /// Attached projects sent as workspace folders.
    lsp_folders: Vec<PathBuf>,
    /// Crash já sinalizado à UI (evita mensagem repetida a cada frame).
    lsp_crash_notified: bool,
    /// Caminho -> revisão já enviada ao servidor.
    lsp_opened: HashMap<PathBuf, u64>,
    /// The server's copy of each open document, for ranged `didChange`.
    lsp_mirrors: HashMap<PathBuf, phpide_lsp::sync::Mirror>,
    /// Diagnostics as published, by file: (first line, last line, raw), for
    /// code action contexts.
    lsp_raw_diagnostics: HashMap<PathBuf, Vec<(usize, usize, Value)>>,
    pending_requests: routing::LspRouting,
    pending_outline: Option<(PathBuf, mpsc::Receiver<Value>)>,
    last_outline_key: Option<(PathBuf, u64)>,
    /// The active PHP file `ws.outline` describes (Structure tool window,
    /// File Structure), and whether its entries came from the language
    /// server; otherwise they come from the syntax tree.
    outline_path: Option<PathBuf>,
    outline_from_server: bool,
    /// Revision of the syntax-tree outline last copied into `ws.outline`.
    outline_local_revision: Option<u64>,
    /// Go to Symbol: resultados da última consulta workspace/symbol.
    symbol_results: Vec<palette::SymbolHit>,
    pending_symbols: Option<mpsc::Receiver<Value>>,
    last_symbol_query: String,
    /// Semantic tokens por (path, revisão) — T-PHP-005-01. Os spans de byte
    /// são derivados uma vez por revisão (barato por frame na pintura).
    semantic: Option<SemanticCache>,
    pending_semantic: Option<(PathBuf, u64, mpsc::Receiver<Value>)>,
    /// Ocorrências por (path, revisão, palavra) — T-PHP-005-02: mudar o
    /// cursor para outra palavra na mesma revisão recalcula.
    occurrences: Option<(PathBuf, u64, String, Vec<intelligence::CharRange>)>,
    pending_occurrences: Option<mpsc::Receiver<Value>>,

    /// Descoberta do projeto Composer (T-PHP-030-01/02).
    composer: Option<phpide_php::composer::ComposerInfo>,
    composer_warned: bool,
    /// mtime do composer.json na última descoberta (detecção de mudança
    /// externa pelo watcher, T-PHP-030-03).
    composer_mtime: Option<std::time::SystemTime>,
    /// Escopo do painel Problems (T-PHP-002-02): 0 = projeto, 1 = arquivo.
    problems_scope: u8,
    // Estágio 3.1 — completamento
    completion: CompletionState,
    /// Tab stops of the last snippet inserted by completion.
    snippet: Option<completion::SnippetSession>,
    /// Documentation, parameter info, rename, context actions, formatting.
    intel: app_intel::IntelState,
    // Estágio 3.1 — análise estrutural local (tree-sitter); anexada à
    // revisão do documento: edição invalida, análise reanexa (NFR-002).
    syntax: Option<(PathBuf, Revisioned<Vec<phpide_syntax::Block>>)>,
    // Estágio 4 — executor
    exec: ExecState,
    /// Comandos dinâmicos da palette (composer/phpunit), recalculados ao abrir.
    extras: Vec<(String, String)>,
    was_palette_open: bool,
    // Estágio 8 — Run/Debug/Test (RUN-001/002, RUN-DBG-001/002/003,
    // RUN-TST-001/002/003): sessões, engine DBGp e cobertura.
    rd: run_debug::RunDebugState,
    /// Run › Edit Configurations, when open.
    run_dialog: Option<run_configs::EditDialog>,
    // Estágio 5 — plugins
    plugins: Vec<plugins::PluginCommand>,
    // Estágio 6 — shell de IDE (REQUISITOS Parte 1)
    settings: settings::Settings,
    settings_dialog: settings_dialog::SettingsDialogState,
    theme: ThemeId,
    tw: toolwindows::ToolWindows,
    welcome: WelcomeState,
    pub welcome_open: bool,
    pub scheme_open: bool,
    pub goto_line_open: bool,
    goto_line_input: String,
    pub debug_listen: bool,
    git: services::git::GitService,
    files: services::files::ProjectFiles,
    rss_cache: Option<(Instant, u64)>,
    /// Detecção de Shift×2 (Search Everywhere).
    shift_was_down: bool,
    last_shift_press: Option<Instant>,
    /// Shift is held with no other key or modifier (a tap so far).
    shift_alone: bool,
    pub zoom_on_ctrl_wheel: bool,
    bookmarks: HashMap<PathBuf, BTreeSet<usize>>,
    /// Terminal tool window sessions.
    terminal: terminal_panel::TerminalState,
    /// Unsaved buffers on disk, crash notice, About (Phase 8).
    recovery: app_recovery::RecoveryState,
    /// Recent Locations popup and the caret place last seen (RF-SEL-08).
    recent_locations: recent_locations::RecentLocations,
    last_place: Option<(PathBuf, usize, usize)>,
    /// Scan of vendor/ and excluded folders for Search Everywhere.
    library_job: Option<phpide_core::jobs::JobHandle<Vec<PathBuf>>>,
    /// Folders marked as sources, tests or excluded (RF-NAV-19).
    project_marks: project_marks::ProjectMarks,
    /// Git state for the Commit, Version Control and diff views.
    vcs: services::vcs::VcsService,
    commit_ui: commit_panel::CommitState,
    diffs: diff_view::DiffTabs,
    vcs_log: vcs_panel::LogState,
    git_dialog: Option<app_vcs::GitDialog>,
    /// Editor views, split pane and per-buffer syntax state.
    editors: editor_area::EditorArea,
    /// Line to jump to on the first frame (`PHPIDE_SCROLL_TO_LINE`, demos).
    scroll_hook: Option<usize>,
    /// Switcher Ctrl+Tab (MRU).
    switcher: switcher::MruSwitcher,
    /// Inlay hints por revisão (UI-ED-005).
    hints: hints::HintStore,
    hints_path: Option<PathBuf>,
    /// Code Vision (lenses) por arquivo/revisão.
    code_vision: hints::CodeVision,
    /// Code lenses of the active file (server).
    code_lens: lsp_features::CodeLensState,
    /// Extend/Shrink Selection.
    selection_ranges: lsp_features::SelectionRanges,
    /// Document links of the active file.
    document_links: lsp_features::LinkState,
    /// Type and call hierarchies (Hierarchy tool window).
    hierarchy: hierarchy::HierarchyState,
    /// Gutter de blame (UI-ED-007) com cache por revisão salva.
    blame: blame_gutter::BlameCache,
    blame_revision: Option<(PathBuf, u64)>,
    /// Cache de markers Git para a scrollbar (UI-ED-006-01).
    git_markers: Option<GitMarkerCache>,
    /// Resumo de inspeções do arquivo ativo (widget UI-ED-006-03).
    inspections: lens::InspectionsSummary,
    /// Language Servers de frontend (FE-001/FE-002): TS, HTML, CSS.
    fe: fe_servers::FeServers,
    /// Undo coeso de refatorações/auto-import (PHP-021-01/PHP-003-05).
    refactor_undo: workspace_edit::UndoStack,
    /// Popup do lens preview (linha sob a scrollbar).
    lens_hover: Option<(usize, egui::Pos2, std::time::Instant)>,
}

impl eframe::App for PhpIdeApp {
    /// Encerramento da IDE: cancela workers, para processos e encerra o
    /// LSP (NFR-001: "O encerramento da IDE deve terminar processos-filhos
    /// gerenciados por ela").
    fn on_exit(&mut self) {
        // A clean exit: unsaved buffers were saved or discarded.
        self.recovery.clear();
        self.windows.unregister();
        let report = self.jobs.shutdown(Duration::from_secs(2));
        self.search.cancel();
        self.exec.stop_if_running();
        // NFR-001-04: encerra sessões de execução e a engine DBGp.
        self.rd.shutdown();
        if let Some(lsp) = self.lsp.take() {
            lsp.stop();
        }
        self.db.shutdown();
        self.logger.info(
            Component::Ui,
            &Correlation::session(self.session),
            "app_exit",
            &[
                ("cancelled_jobs", &report.cancelled.to_string()),
                ("unresponsive", &report.unresponsive.to_string()),
            ],
        );
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame_ui(ui);
    }
}

impl PhpIdeApp {
    /// One frame of the whole window (also rendered by snapshot tests).
    pub(crate) fn frame_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = &ui.ctx().clone();
        // Screenshot do próprio app para documentação (README): com
        // PHPIDE_SCREENSHOT_TO=<arquivo.ppm> definido, pede o frame N
        // (PHPIDE_SCREENSHOT_AFTER, default 8) ao egui e grava PPM (RGBA→RGB;
        // converta para PNG com o ImageMagick) antes de sair. Só ativo com o
        // env — zero efeito em uso normal.
        if std::env::var_os("PHPIDE_SCREENSHOT_TO").is_some() {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static FRAMES: AtomicUsize = AtomicUsize::new(0);
            let n = FRAMES.fetch_add(1, Ordering::Relaxed);
            let after: usize = std::env::var("PHPIDE_SCREENSHOT_AFTER")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8);
            if n < after {
                // Keep frames coming even when nothing animates.
                ctx.request_repaint();
            }
            if n == after {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                // O frame do screenshot só é produzido num redraw seguinte.
                ctx.request_repaint();
            }
            let shot = ctx.input(|i| {
                i.events.iter().find_map(|ev| match ev {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(image) = shot {
                let path = std::env::var("PHPIDE_SHOT_OUTPUT")
                    .unwrap_or_else(|_| std::env::var("PHPIDE_SCREENSHOT_TO").unwrap());
                let (w, h) = (image.width(), image.height());
                let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
                for px in image.pixels.iter() {
                    ppm.extend_from_slice(&[px.r(), px.g(), px.b()]);
                }
                if std::fs::write(&path, &ppm).is_ok() {
                    eprintln!("screenshot saved: {path} ({w}x{h})");
                }
                std::process::exit(0);
            }
        }
        // Fechamento da janela com buffers sujos: cancela o close e abre a
        // confirmação (NFR-007-02). O modal reenvia ViewportCommand::Close.
        if ctx.input(|i| i.viewport().close_requested()) {
            if matches!(self.pending_close, Some(PendingClose::Exit)) {
                // Confirmação já resolvida como "sair": deixa fechar.
            } else if self.any_dirty() {
                self.pending_close = Some(PendingClose::Exit);
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
        self.poll_watcher(ctx);
        if !self.welcome_open {
            self.poll_project_services(ctx);
            self.poll_vcs(ctx);
        }
        if !self.terminal.focused {
            self.detect_double_shift(ctx);
        }
        if !self.palette.open && !self.settings_dialog.open {
            self.handle_shortcuts(ctx);
        }
        self.poll_search();
        self.poll_lsp();
        self.handle_lsp_refreshes();
        self.poll_code_lenses();
        self.poll_document_links();
        self.poll_selection_ranges();
        self.poll_hierarchy();
        self.poll_fe();
        self.poll_syntax();
        self.poll_routed_requests();
        self.poll_hints();
        self.poll_completion();
        self.terminal.poll();
        // Set again by the terminal view if it still has the keyboard.
        self.terminal.focused = false;
        self.poll_intel();
        self.poll_symbol_query();
        self.exec.poll();
        self.db.poll();
        // Estágio 8: sessões de execução + engine DBGp (eventos, testes,
        // cobertura) e sync do widget de listener na status bar.
        self.rd.poll();
        // Once Xdebug connects, the "waiting" hint is stale.
        if self.rd.status != run_debug::DebugStatus::Listening
            && self.ws.message.starts_with("Debug: waiting for Xdebug")
        {
            self.ws.message.clear();
        }
        self.debug_listen = self.rd.is_listening();
        self.debug_keys(ctx);
        self.ctrl_wheel_zoom(ctx);

        if self.palette.open && !self.was_palette_open {
            self.extras = self.palette_extras();
        }
        if !self.palette.open && self.was_palette_open {
            self.editors.request_focus();
        }
        self.was_palette_open = self.palette.open;

        self.poll_libraries(ctx);
        if let Some(action) = palette::show(
            ctx,
            &self.ws,
            &mut self.palette,
            &self.extras,
            &self.symbol_results,
        ) {
            match action {
                PaletteAction::Open(path) => self.open_file_tab(&path, false),
                PaletteAction::OpenAt(path, line0, col0) => {
                    self.ws.open_file_at(&path, line0, col0);
                    self.editors.request_focus();
                }
                PaletteAction::Settings(page) => self.settings_dialog.open(page),
                PaletteAction::AssignShortcut(cmd) => {
                    self.settings_dialog
                        .open(settings_dialog::SettingsPage::Keymap);
                    self.settings_dialog.keymap.assign(cmd);
                }
                PaletteAction::Run(cmd) => self.dispatch_command(cmd, ctx),
                PaletteAction::Plugin(id) => self.run_plugin(&id),
            }
        }
        match self.windows.poll() {
            Some(services::windows::WindowCommand::Focus) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            Some(services::windows::WindowCommand::Close) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            None => {}
        }
        // Keep reading the window mailbox while idle.
        ctx.request_repaint_after(Duration::from_millis(600));
        self.event_log.observe(&self.ws.message, &mut self.toasts);
        if self.tw.is_visible(ToolWindow::Notifications) {
            self.event_log.mark_read();
        }
        self.toasts.show(ctx);
        self.completion_popup(ctx);
        self.context_actions_popup(ctx);
        self.file_structure_popup(ctx);
        self.snippet_keys(ctx);
        self.signature_popup(ctx);
        self.hover_popup(ctx);
        self.rename_dialog(ctx);
        self.edit_configurations_dialog(ctx);
        self.git_dialog_ui(ctx);
        self.recovery_dialogs(ctx);
        quick_preview::show(self, ctx);
        self.track_location();
        recent_locations::show(self, ctx);
        self.autosave_recovery();
        run_panels::evaluate_dialog(self, ctx);
        run_panels::breakpoints_dialog(self, ctx);
        self.language_server_log(ctx);
        project_tree::select_in_popup(self, ctx);
        welcome::poll_picker(self, ctx);
        welcome::open_prompt_dialog(self, ctx);
        scheme::scheme_popup(self, ctx);
        self.goto_line_popup(ctx);
        settings_dialog::show(self, ctx);
        if std::env::var("PHPIDE_SMOKE_SAVE_ACTION").as_deref() == Ok("save") {
            static TRIGGER: std::sync::OnceLock<()> = std::sync::OnceLock::new();
            if TRIGGER.get().is_none() {
                self.confirm_save_close(ctx);
                let _ = TRIGGER.set(());
            }
        }

        ctx.set_zoom_factor(self.zoom);
        let compact = self.settings.compact_mode;
        ctx.global_style_mut(|style| {
            style.spacing.item_spacing = if compact {
                egui::vec2(5.0, 3.0)
            } else {
                egui::vec2(8.0, 5.0)
            };
            style.spacing.button_padding = if compact {
                egui::vec2(5.0, 1.0)
            } else {
                egui::vec2(7.0, 3.0)
            };
        });

        if self.welcome_open {
            welcome::welcome_screen(self, ui);
            self.dirty_close_modal(ctx);
            if self.watcher.is_some() {
                ctx.request_repaint_after(Duration::from_millis(150));
            }
            return;
        }

        header::header(self, ui);
        // Tab placement: none hides the strip (RF-TAB-05).
        if self.settings.tab_placement != "None" {
            tabs::tab_strip(self, ui);
        }
        self.ws.after_close = phpide_core::AfterClose::from_label(&self.settings.tab_after_close);
        self.ws.note_active();
        statusbar::navigation_bar_top(self, ui);
        // Bottom panels are laid out from the window edge inwards: status, bar, content.
        statusbar::status_bar(self, ui);
        self.tw.bottom_bar(ui, compact);

        self.tw.bar(ui, toolwindows::Anchor::Left, compact);
        if self.tw.is_visible(ToolWindow::Project) {
            self.file_tree(ui);
        }
        for tw in [ToolWindow::Bookmarks, ToolWindow::Services] {
            if self.tw.is_visible(tw) {
                tool_panels::show(self, ui, tw);
            }
        }

        self.tw.bar(ui, toolwindows::Anchor::Right, compact);
        if self.tw.is_visible(ToolWindow::Structure) {
            self.outline_panel(ui);
        }
        if self.tw.is_visible(ToolWindow::Hierarchy) {
            self.hierarchy_panel(ui);
        }
        if self.tw.is_visible(ToolWindow::RemoteHost) {
            tool_panels::show(self, ui, ToolWindow::RemoteHost);
        }

        if self.tw.is_visible(ToolWindow::Find) {
            self.search_panel(ui);
        }
        if self.tw.is_visible(ToolWindow::Problems) {
            self.problems_panel(ui);
        }
        if self.tw.is_visible(ToolWindow::Run) || self.tw.is_visible(ToolWindow::Debug) {
            self.exec_panel(ui);
        }
        for tw in [
            ToolWindow::VersionControl,
            ToolWindow::Commit,
            ToolWindow::Terminal,
            ToolWindow::Database,
            ToolWindow::Notifications,
        ] {
            if self.tw.is_visible(tw) {
                tool_panels::show(self, ui, tw);
            }
        }
        self.editor(ui);
        self.dirty_close_modal(ctx);

        if self.watcher.is_some() {
            // O watcher dispara entre frames; mantém a UI viva para aplicar mudanças.
            ctx.request_repaint_after(Duration::from_millis(150));
        }
    }
}

// Frontend language servers are frozen until v1.0 (see `scope`).

#[cfg(test)]
mod gate_a11y;
#[cfg(test)]
mod gate_git;
#[cfg(test)]
mod gate_hardening;
#[cfg(test)]
mod gate_lsp;
#[cfg(test)]
mod gate_menus;
#[cfg(test)]
mod gate_parity;
#[cfg(test)]
mod gate_phpantom;
#[cfg(test)]
mod gate_project_view;
#[cfg(test)]
mod gate_run;
#[cfg(test)]
mod gate_ui_ed;

// Frontend language servers are frozen until v1.0 (see `scope`).
#[cfg(all(test, feature = "experimental"))]
mod gate_fe;

#[cfg(test)]
mod gate_tests;

#[cfg(test)]
mod editor_snapshots;
