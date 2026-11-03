//! phpide — IDE em Rust focada em PHP.
//!
//! Estágio 6 — shell de IDE com paridade visual à New UI do PhpStorm
//! (REQUISITOS.md, Parte 1): window header unificado, tool windows com
//! atalhos Alt+1..7, status bar com widgets, Search Everywhere (Shift×2),
//! Find Action (Ctrl+Shift+A), Quick Switch Scheme (Ctrl+`), Welcome Screen,
//! editor tabs com preview/pin/Closing Policy e breadcrumbs.

mod completion;
mod editor_view;
mod exec;
mod header;
mod palette;
mod plugins;
mod scheme;
mod search;
mod settings;
mod settings_dialog;
mod statusbar;
mod tabs;
mod themes;
mod tool_panels;
mod toolwindows;
mod vcs;
mod welcome;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use completion::CompletionState;
use eframe::egui;
use egui::widgets::text_edit::TextEditState;
use exec::ExecState;
use notify::Watcher as _;
use palette::{PaletteAction, PaletteCommand, PaletteMode, PaletteState};
use phpide_core::jobs::JobSystem;
use phpide_core::{
    log::{Correlation, Logger},
    Component, Revisioned, Workspace,
};
use phpide_dbgp::{run_with_breakpoints, DebugMsg};
use phpide_lsp::{detect_server, uri_to_path, LspClient, LspDiagnostic, LspStatus};
use phpide_php::{Language, PhpLanguage};
use search::SearchState;
use serde_json::Value;
use themes::ThemeId;
use toolwindows::ToolWindow;
use welcome::WelcomeState;

/// Fechamento pendente que exige confirmação de buffers sujos (NFR-007:
/// "Dirty buffers precisam de confirmação ao fechar").
#[derive(Debug, Clone)]
pub enum PendingClose {
    /// Fechar a aba `i` (do contexto, Ctrl+F4 ou palette).
    Tab(usize),
    /// Fechar as outras abas (Close Others).
    Others(usize),
    /// Fechar todas as abas e abrir outro projeto (Welcome).
    Project(std::path::PathBuf),
    /// Sair da IDE (janela fechada com buffers sujos).
    Exit,
}

fn main() -> eframe::Result<()> {
    // Uso: phpide [projeto] [arquivos...]. Sem projeto, abre a Welcome Screen;
    // arquivos extras são abertos em abas (preview) após carregar o projeto.
    let mut args = std::env::args().skip(1);
    let arg = args.next();
    let files: Vec<PathBuf> = args.map(PathBuf::from).collect();
    let root = arg
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let title = format!("phpide — {}", root.display());
    let loaded = settings::Settings::load();
    let theme = ThemeId::from_name(&loaded.theme);
    // Sem argumento de projeto, abre na Welcome Screen (REQUISITOS 1.5).
    let start_welcome = arg.is_none();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        &title,
        options,
        Box::new(move |cc| {
            themes::apply(&cc.egui_ctx, theme);
            Ok(Box::new(PhpIdeApp::new(root, start_welcome, files)))
        }),
    )
}

struct PhpIdeApp {
    ws: Workspace,
    collapsed: HashSet<PathBuf>,
    selected: Option<PathBuf>,
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
    /// Caminho -> versão já enviada ao servidor.
    lsp_opened: HashMap<PathBuf, u64>,
    pending_def: Option<mpsc::Receiver<Value>>,
    pending_outline: Option<(PathBuf, mpsc::Receiver<Value>)>,
    last_outline_key: Option<(PathBuf, u64)>,
    // Estágio 3.1 — completamento
    completion: CompletionState,
    // Estágio 3.1 — análise estrutural local (tree-sitter); anexada à
    // revisão do documento: edição invalida, análise reanexa (NFR-002).
    syntax: Option<(PathBuf, Revisioned<Vec<phpide_syntax::Block>>)>,
    // Estágio 4 — executor
    exec: ExecState,
    /// Comandos dinâmicos da palette (composer/phpunit), recalculados ao abrir.
    extras: Vec<(String, String)>,
    was_palette_open: bool,
    // Estágio 4c — depuração DBGp
    breakpoints: HashMap<PathBuf, BTreeSet<usize>>,
    debug_rx: Option<mpsc::Receiver<DebugMsg>>,
    /// Saída do processo PHP disparado em modo debug (painel Debug).
    debug_php_rx: Option<mpsc::Receiver<String>>,
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
    git_branch_cache: Option<String>,
    git_refreshed: Instant,
    /// Detecção de Shift×2 (Search Everywhere).
    shift_was_down: bool,
    last_shift_press: Option<Instant>,
    pub zoom_on_ctrl_wheel: bool,
    bookmarks: HashMap<PathBuf, BTreeSet<usize>>,
    terminal_input: String,
    commit_message: String,
}

impl PhpIdeApp {
    fn new(root: PathBuf, start_welcome: bool, files: Vec<PathBuf>) -> Self {
        let (settings, settings_err) = settings::Settings::load_or_default();
        let theme = ThemeId::from_name(&settings.theme);
        let (lsp, lsp_init, lsp_name) = start_lsp(&root);
        let plugins = plugins::discover_plugins(&root, &settings::config_dir());
        let logger = Logger::stderr();
        let session = std::process::id() as u64;
        logger.info(
            Component::Ui,
            &Correlation::session(session),
            "app_start",
            &[("root", &root.display().to_string())],
        );
        let mut app = PhpIdeApp {
            watcher: start_watcher(&root),
            ws: Workspace::open(root),
            collapsed: HashSet::new(),
            selected: None,
            cursor_char: 0,
            palette: PaletteState::new(),
            search: SearchState::new(),
            zoom: settings.zoom,
            change_pending_since: None,
            jobs: JobSystem::new(),
            logger,
            session,
            pending_close: None,
            lsp,
            lsp_init,
            lsp_name,
            lsp_opened: HashMap::new(),
            pending_def: None,
            pending_outline: None,
            last_outline_key: None,
            completion: CompletionState::new(),
            syntax: None,
            exec: ExecState::new(),
            extras: Vec::new(),
            was_palette_open: false,
            breakpoints: HashMap::new(),
            debug_rx: None,
            debug_php_rx: None,
            plugins,
            settings,
            settings_dialog: settings_dialog::SettingsDialogState::default(),
            theme,
            tw: toolwindows::ToolWindows::default(),
            welcome: WelcomeState::new(),
            welcome_open: start_welcome,
            scheme_open: false,
            goto_line_open: false,
            goto_line_input: String::new(),
            debug_listen: false,
            git_branch_cache: None,
            git_refreshed: Instant::now() - Duration::from_secs(60),
            shift_was_down: false,
            last_shift_press: None,
            zoom_on_ctrl_wheel: false,
            bookmarks: HashMap::new(),
            terminal_input: String::new(),
            commit_message: String::new(),
        };
        // Arquivos passados na CLI: abertos em abas (o último fica ativo).
        for f in &files {
            let path = if f.is_absolute() {
                f.clone()
            } else {
                app.ws.root.join(f)
            };
            app.ws.open_file_ex(&path, true);
        }
        if let Some(err) = settings_err {
            // Configuração inválida: defaults em uso, original preservado.
            app.ws.message = err.to_string();
            app.logger.warn(
                Component::Settings,
                &Correlation::session(app.session),
                "settings_recovered",
                &[("error", &err.log_line())],
            );
        }
        app
    }

    // -----------------------------------------------------------------
    // Estágio 6 — shell: projetos, temas, helpers compartilhados
    // -----------------------------------------------------------------

    /// Troca de projeto a partir da UI: com buffers sujos, confirma antes
    /// (NFR-007-02); limpo, carrega direto.
    pub fn request_load_project(&mut self, root: PathBuf, ctx: &egui::Context) {
        if self.ws.buffers.iter().any(|b| b.dirty) {
            self.pending_close = Some(PendingClose::Project(root));
        } else {
            self.load_project(root, ctx);
        }
    }

    /// Carrega outro projeto no lugar (Welcome Screen / Project widget).
    /// NFR-001: troca de projeto cancela workers da sessão anterior, para
    /// a execução em andamento e encerra o LSP antigo.
    pub fn load_project(&mut self, root: PathBuf, ctx: &egui::Context) {
        self.settings.touch_project(&root);
        self.persist_settings();
        // Isola o trabalho do projeto anterior (NFR-001-04).
        let report = self.jobs.shutdown(Duration::from_secs(2));
        self.search.cancel();
        self.exec.stop_if_running();
        self.logger.info(
            Component::Ui,
            &Correlation::session(self.session),
            "project_switch",
            &[
                ("cancelled_jobs", &report.cancelled.to_string()),
                ("unresponsive", &report.unresponsive.to_string()),
            ],
        );
        if let Some(old) = self.lsp.take() {
            old.stop();
        }
        self.watcher = start_watcher(&root);
        let (lsp, lsp_init, lsp_name) = start_lsp(&root);
        self.lsp = lsp;
        self.lsp_init = lsp_init;
        self.lsp_name = lsp_name;
        self.lsp_opened.clear();
        self.pending_def = None;
        self.pending_outline = None;
        self.last_outline_key = None;
        self.syntax = None;
        self.ws = Workspace::open(root.clone());
        self.collapsed.clear();
        self.selected = None;
        self.git_branch_cache = None;
        self.git_refreshed = Instant::now() - Duration::from_secs(60);
        self.welcome_open = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "phpide — {}",
            root.display()
        )));
        self.ws.message = format!("projeto aberto: {}", root.display());
    }

    pub fn persist_settings(&mut self) {
        self.settings.zoom = self.zoom;
        self.settings.theme = self.theme.name().to_string();
        self.settings.save();
    }

    pub fn apply_theme(&mut self, theme: ThemeId, ctx: &egui::Context) {
        self.theme = theme;
        themes::apply(ctx, theme);
        self.persist_settings();
    }

    pub fn open_palette(&mut self, mode: PaletteMode) {
        self.palette.open_with(mode);
    }

    pub fn git_branch(&mut self) -> String {
        if self.git_refreshed.elapsed() > Duration::from_secs(2) {
            self.git_branch_cache = vcs::current_branch(&self.ws.root);
            self.git_refreshed = Instant::now();
        }
        self.git_branch_cache
            .clone()
            .unwrap_or_else(|| "?".to_string())
    }

    pub fn git_refresh(&mut self) {
        self.git_branch_cache = vcs::current_branch(&self.ws.root);
        self.git_refreshed = Instant::now();
    }

    /// RSS do processo em MB (widget de memória da status bar).
    pub fn rss_mb(&self) -> u64 {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| {
                s.lines().find(|l| l.starts_with("VmRSS:")).and_then(|l| {
                    l.split_whitespace()
                        .nth(1)
                        .and_then(|v| v.parse::<u64>().ok())
                })
            })
            .map(|kb| kb / 1024)
            .unwrap_or(0)
    }

    /// F2 / Shift+F2: salta para o próximo/anterior problema vigente
    /// (revisão atual; diagnóstico obsoleto não entra na ordem).
    pub fn goto_problem(&mut self, dir: i32) {
        let diags: Vec<phpide_core::Diagnostic> = self.ws.current_diagnostics().cloned().collect();
        if diags.is_empty() {
            self.ws.message = "nenhum problema detectado".to_string();
            return;
        }
        let cur_line = self
            .ws
            .active_buffer()
            .map(|b| {
                b.document
                    .line_col(self.cursor_char.min(b.document.len_chars()))
                    .0
            })
            .unwrap_or(0);
        let idx = match dir >= 0 {
            true => diags
                .iter()
                .position(|d| (d.line, d.col) > (cur_line, 0))
                .unwrap_or(0),
            false => diags
                .iter()
                .rposition(|d| d.line < cur_line)
                .unwrap_or(diags.len() - 1),
        };
        let d = diags[idx].clone();
        self.ws
            .open_file_at(&d.path, d.line.saturating_sub(1), d.col.saturating_sub(1));
    }

    pub fn stop_execution(&mut self) {
        if self.exec.running {
            self.exec.stop();
        }
        if let Some(rx) = self.debug_rx.take() {
            drop(rx);
            self.ws.message = "sessão de debug encerrada".to_string();
        }
    }

    fn has_collapsed_ancestor(&self, path: &Path) -> bool {
        path.ancestors().any(|a| self.collapsed.contains(a))
    }

    /// Reduz a edição "texto inteiro" do TextEdit a uma operação mínima na rope.
    fn diff_edit(old: &str, new: &str) -> Option<(usize, usize, String)> {
        if old == new {
            return None;
        }
        let old: Vec<char> = old.chars().collect();
        let new: Vec<char> = new.chars().collect();
        let mut start = 0;
        while start < old.len() && start < new.len() && old[start] == new[start] {
            start += 1;
        }
        let mut old_end = old.len();
        let mut new_end = new.len();
        while old_end > start && new_end > start && old[old_end - 1] == new[new_end - 1] {
            old_end -= 1;
            new_end -= 1;
        }
        Some((start, old_end - start, new[start..new_end].iter().collect()))
    }

    fn dispatch_command(&mut self, cmd: PaletteCommand, ctx: &egui::Context) {
        use toolwindows::ToolWindow as Tw;
        match cmd {
            PaletteCommand::Save => self.save_active(),
            PaletteCommand::NewFile => self.ws.new_file(),
            PaletteCommand::CloseTab => {
                self.request_close_active();
            }
            PaletteCommand::CloseOthers => {
                let active = self.ws.active;
                self.request_close_others(active);
            }
            PaletteCommand::PinTab => {
                if let Some(buf) = self.ws.active_buffer_mut() {
                    buf.pinned = !buf.pinned;
                }
            }
            PaletteCommand::RefreshTree => self.ws.refresh_files(),
            PaletteCommand::ToggleTheme => {
                let next = if self.theme.is_dark() {
                    ThemeId::IslandsLight
                } else {
                    ThemeId::IslandsDark
                };
                self.apply_theme(next, ctx);
            }
            PaletteCommand::ZoomIn => self.set_zoom(self.zoom * 1.1),
            PaletteCommand::ZoomOut => self.set_zoom(self.zoom / 1.1),
            PaletteCommand::ZoomReset => self.set_zoom(1.0),
            PaletteCommand::ToggleProject => self.tw.toggle(Tw::Project),
            PaletteCommand::ToggleFind => self.tw.toggle(Tw::Find),
            PaletteCommand::ToggleRun => self.tw.toggle(Tw::Run),
            PaletteCommand::ToggleDebug => self.tw.toggle(Tw::Debug),
            PaletteCommand::ToggleProblems => self.tw.toggle(Tw::Problems),
            PaletteCommand::ToggleStructure => self.tw.toggle(Tw::Structure),
            PaletteCommand::HideAllToolWindows => self.tw.toggle_hide_all(),
            PaletteCommand::NavigateBack => {
                if !self.ws.navigate_back() {
                    self.ws.message = "sem histórico de navegação".to_string();
                }
            }
            PaletteCommand::NavigateForward => {
                if !self.ws.navigate_forward() {
                    self.ws.message = "sem histórico de navegação".to_string();
                }
            }
            PaletteCommand::NextProblem => self.goto_problem(1),
            PaletteCommand::PrevProblem => self.goto_problem(-1),
            PaletteCommand::OpenWelcome => self.welcome_open = true,
            PaletteCommand::OpenScheme => self.scheme_open = true,
            PaletteCommand::OpenSettings => {
                self.settings_dialog
                    .open(settings_dialog::SettingsPage::Appearance);
            }
            PaletteCommand::GotoDefinition => self.request_definition(),
            PaletteCommand::RunCurrent => self.run_current_php(),
            PaletteCommand::DebugCurrent => self.debug_run(),
            PaletteCommand::GotoLine => self.goto_line_open = true,
        }
        let _ = ctx;
    }

    /// Fecha a aba ativa; com edição pendente, pede confirmação (NFR-007-02).
    pub fn request_close_active(&mut self) {
        let active = self.ws.active;
        self.request_close_tab(active);
    }

    /// Fecha a aba `i`; buffer sujo exige confirmação (salvar/descartar/cancelar).
    pub fn request_close_tab(&mut self, i: usize) {
        let dirty = self.ws.buffers.get(i).map(|b| b.dirty).unwrap_or(false);
        if dirty {
            self.pending_close = Some(PendingClose::Tab(i));
        } else {
            self.ws.close_tab(i);
        }
    }

    /// Close Others: confirma se alguma vítima estiver suja.
    pub fn request_close_others(&mut self, keep: usize) {
        let any_dirty = self
            .ws
            .buffers
            .iter()
            .enumerate()
            .any(|(i, b)| i != keep && b.dirty);
        if any_dirty {
            self.pending_close = Some(PendingClose::Others(keep));
        } else {
            self.ws.close_other_tabs(keep);
        }
    }

    fn save_active(&mut self) {
        let active = self.ws.active;
        self.save_buffer_at(active);
    }

    /// Salva um buffer específico: escrita atômica (NFR-007-01), erro com
    /// componente/operação/ação (NFR-005), didSave no LSP.
    fn save_buffer_at(&mut self, i: usize) -> bool {
        let Some(buf) = self.ws.buffers.get_mut(i) else {
            return false;
        };
        if buf.readonly {
            self.ws.message =
                "arquivo em modo somente leitura (destrave na status bar)".to_string();
            return false;
        }
        let path = buf.path.clone();
        // Line endings: converte \n -> \r\n ao salvar quando o widget for CRLF.
        if self.settings.line_ending == "CRLF" {
            let text = buf
                .document
                .text()
                .replace("\r\n", "\n")
                .replace('\n', "\r\n");
            buf.document = phpide_core::Document::new(&text);
        }
        let corr = Correlation::session(self.session).with_document(
            path.clone()
                .unwrap_or_else(|| PathBuf::from("<sem-titulo>")),
            buf.document.revision(),
        );
        match buf.try_save() {
            Ok(()) => {
                self.ws.message = "salvo".to_string();
                self.logger
                    .info(Component::Core, &corr, "buffer_saved", &[]);
            }
            Err(e) => {
                // Buffer permanece sujo e intacto; mensagem curta + ação.
                self.ws.message = e.to_string();
                self.logger.error(
                    Component::Core,
                    &corr,
                    "buffer_save_failed",
                    &[("error", &e.log_line())],
                );
                return false;
            }
        }
        if let (Some(lsp), Some(path)) = (&self.lsp, path) {
            if phpide_php::Php::is_php_file(&path) {
                let text = self
                    .ws
                    .buffers
                    .get(i)
                    .map(|b| b.document.text())
                    .unwrap_or_default();
                lsp.did_save(&path, &text);
            }
        }
        true
    }

    /// Salva todos os buffers sujos (confirmação "Salvar Tudo").
    fn save_all_dirty(&mut self) {
        for i in 0..self.ws.buffers.len() {
            if self.ws.buffers[i].dirty {
                self.save_buffer_at(i);
            }
        }
    }

    fn any_dirty(&self) -> bool {
        self.ws.buffers.iter().any(|b| b.dirty)
    }
}

fn start_lsp(root: &Path) -> (Option<LspClient>, Option<mpsc::Receiver<Value>>, String) {
    match detect_server(root) {
        Some((cmd, args)) => {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            match LspClient::spawn(&cmd, &refs, root) {
                Ok((client, rx)) => {
                    eprintln!("[lsp] servidor: {cmd} {}", args.join(" "));
                    (Some(client), Some(rx), cmd)
                }
                Err(e) => {
                    eprintln!("[lsp] falha ao iniciar: {e}");
                    (None, None, String::new())
                }
            }
        }
        None => {
            eprintln!("[lsp] nenhum servidor encontrado (intelephense/phpactor)");
            (None, None, String::new())
        }
    }
}

fn start_watcher(root: &Path) -> Option<(notify::RecommendedWatcher, mpsc::Receiver<()>)> {
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            let relevant = event
                .paths
                .iter()
                .any(|p| p.is_dir() || phpide_php::Php::is_php_file(p));
            if relevant {
                let _ = tx.send(());
            }
        }
    })
    .ok()?;
    watcher.watch(root, notify::RecursiveMode::Recursive).ok()?;
    Some((watcher, rx))
}

impl eframe::App for PhpIdeApp {
    /// Encerramento da IDE: cancela workers, para processos e encerra o
    /// LSP (NFR-001: "O encerramento da IDE deve terminar processos-filhos
    /// gerenciados por ela").
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let report = self.jobs.shutdown(Duration::from_secs(2));
        self.search.cancel();
        self.exec.stop_if_running();
        if let Some(lsp) = self.lsp.take() {
            lsp.stop();
        }
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

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
        self.poll_watcher();
        self.detect_double_shift(ctx);
        if !self.palette.open && !self.settings_dialog.open {
            self.handle_shortcuts(ctx);
        }
        self.poll_search();
        self.poll_lsp();
        self.poll_syntax();
        self.exec.poll();
        self.poll_debug();
        self.ctrl_wheel_zoom(ctx);

        if self.palette.open && !self.was_palette_open {
            self.extras = self.palette_extras();
        }
        self.was_palette_open = self.palette.open;

        if let Some(action) = palette::show(ctx, &self.ws, &mut self.palette, &self.extras) {
            match action {
                PaletteAction::Open(path) => self.ws.open_file(&path),
                PaletteAction::Run(cmd) => self.dispatch_command(cmd, ctx),
                PaletteAction::Plugin(id) => self.run_plugin(&id),
            }
        }
        self.completion_popup(ctx);
        scheme::scheme_popup(self, ctx);
        self.goto_line_popup(ctx);
        settings_dialog::show(self, ctx);
        self.dirty_close_modal(ctx);

        ctx.set_zoom_factor(self.zoom);
        let compact = self.settings.compact_mode;
        ctx.style_mut(|style| {
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
            welcome::welcome_screen(self, ctx);
            if self.watcher.is_some() {
                ctx.request_repaint_after(Duration::from_millis(150));
            }
            return;
        }

        header::header(self, ctx);
        tabs::tab_strip(self, ctx);
        statusbar::navigation_bar_top(self, ctx);
        // Painéis bottom são registrados da borda para dentro: status, barra e conteúdo.
        statusbar::status_bar(self, ctx);
        self.tw.bottom_bar(ctx, compact);

        self.tw.bar(ctx, toolwindows::Anchor::Left, compact);
        if self.tw.is_visible(ToolWindow::Project) {
            self.file_tree(ctx);
        }
        for tw in [ToolWindow::Bookmarks, ToolWindow::Services] {
            if self.tw.is_visible(tw) {
                tool_panels::show(self, ctx, tw);
            }
        }

        self.tw.bar(ctx, toolwindows::Anchor::Right, compact);
        if self.tw.is_visible(ToolWindow::Structure) {
            self.outline_panel(ctx);
        }
        if self.tw.is_visible(ToolWindow::RemoteHost) {
            tool_panels::show(self, ctx, ToolWindow::RemoteHost);
        }

        if self.tw.is_visible(ToolWindow::Find) {
            self.search_panel(ctx);
        }
        if self.tw.is_visible(ToolWindow::Problems) {
            self.problems_panel(ctx);
        }
        if self.tw.is_visible(ToolWindow::Run) || self.tw.is_visible(ToolWindow::Debug) {
            self.exec_panel(ctx);
        }
        for tw in [
            ToolWindow::VersionControl,
            ToolWindow::Commit,
            ToolWindow::Terminal,
            ToolWindow::Database,
            ToolWindow::Notifications,
        ] {
            if self.tw.is_visible(tw) {
                tool_panels::show(self, ctx, tw);
            }
        }
        self.editor(ctx);

        if self.watcher.is_some() {
            // O watcher dispara entre frames; mantém a UI viva para aplicar mudanças.
            ctx.request_repaint_after(Duration::from_millis(150));
        }
    }
}

impl PhpIdeApp {
    fn poll_watcher(&mut self) {
        if let Some((_, rx)) = &self.watcher {
            let mut any = false;
            while rx.try_recv().is_ok() {
                any = true;
            }
            if any {
                self.change_pending_since = Some(Instant::now());
            }
        }
        if let Some(since) = self.change_pending_since {
            if since.elapsed() > Duration::from_millis(300) {
                self.change_pending_since = None;
                self.ws.refresh_files();
                let changed: Vec<PathBuf> = self
                    .ws
                    .buffers
                    .iter()
                    .filter_map(|b| b.path.clone())
                    .collect();
                let reloaded = self.ws.reload_clean_buffers(&changed);
                if !reloaded.is_empty() {
                    self.ws.message =
                        format!("{} arquivo(s) recarregados do disco", reloaded.len());
                }
            }
        }
    }

    fn poll_search(&mut self) {
        let job_before = self.search.current_job();
        let was_busy = self.search.busy();
        self.search.poll();
        if was_busy && !self.search.busy() {
            if let Some(job) = job_before {
                self.logger.info(
                    Component::Search,
                    &Correlation::session(self.session).with_job(job),
                    "search_finished",
                    &[("results", &self.search.results.len().to_string())],
                );
            }
        }
    }

    /// Shift×2 abre o Search Everywhere (edge da tecla Shift, janela de 400ms).
    fn detect_double_shift(&mut self, ctx: &egui::Context) {
        let down = ctx.input(|i| i.modifiers.shift);
        if down && !self.shift_was_down && !self.palette.open {
            let now = Instant::now();
            if let Some(last) = self.last_shift_press {
                if now.duration_since(last) < Duration::from_millis(400) {
                    self.open_palette(PaletteMode::All);
                    self.last_shift_press = None;
                } else {
                    self.last_shift_press = Some(now);
                }
            } else {
                self.last_shift_press = Some(now);
            }
        }
        self.shift_was_down = down;
    }

    fn ctrl_wheel_zoom(&mut self, ctx: &egui::Context) {
        if !self.zoom_on_ctrl_wheel {
            return;
        }
        let scroll = ctx.input(|i| {
            if i.modifiers.ctrl {
                i.raw_scroll_delta.y
            } else {
                0.0
            }
        });
        if scroll.abs() > 0.0 {
            self.set_zoom(self.zoom * (1.0 + scroll * 0.001));
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let mut save = false;
        let mut undo = false;
        let mut redo = false;
        let mut def = false;
        let mut complete = false;
        let mut block_start = false;
        let mut block_end = false;
        let mut run_php = false;
        let mut debug_php = false;
        let mut bp_toggle = false;
        ctx.input_mut(|i| {
            use egui::{Key, Modifiers as M};
            let shift = M::SHIFT;
            let ctrl = M::CTRL;
            let alt = M::ALT;
            if i.consume_key(ctrl, Key::S) {
                save = true;
            }
            if i.consume_key(ctrl, Key::N) {
                self.ws.new_file();
            }
            if i.consume_key(ctrl, Key::Z) {
                undo = true;
            }
            if i.consume_key(ctrl | shift, Key::Z) {
                redo = true;
            }
            // Search Everywhere / Go to File / Find Action / Recent Files.
            if i.consume_key(ctrl, Key::P) {
                self.open_palette(PaletteMode::All);
            }
            if i.consume_key(ctrl | shift, Key::N) {
                self.open_palette(PaletteMode::Files);
            }
            if i.consume_key(ctrl, Key::E) {
                self.open_palette(PaletteMode::Recent);
            }
            if i.consume_key(ctrl | shift, Key::A) {
                self.open_palette(PaletteMode::Actions);
            }
            // Tool windows (REQUISITOS 1.2): Alt+0…9 + Terminal/Database.
            for digit in 0u8..=9 {
                let key = match digit {
                    0 => Key::Num0,
                    1 => Key::Num1,
                    2 => Key::Num2,
                    3 => Key::Num3,
                    4 => Key::Num4,
                    5 => Key::Num5,
                    6 => Key::Num6,
                    7 => Key::Num7,
                    8 => Key::Num8,
                    _ => Key::Num9,
                };
                if let Some(tw) = ToolWindow::from_digit(digit) {
                    if i.consume_key(alt, key) {
                        self.tw.toggle(tw);
                    }
                }
            }
            if i.consume_key(alt, Key::F12) {
                self.tw.toggle(ToolWindow::Terminal);
            }
            if i.consume_key(ctrl | alt, Key::Num3) {
                self.tw.toggle(ToolWindow::Database);
            }
            if i.consume_key(shift, Key::Escape) {
                self.tw.hide_active();
            }
            if i.consume_key(ctrl | shift, Key::F12) {
                self.tw.toggle_hide_all();
            }
            if i.consume_key(ctrl | alt, Key::S) {
                self.settings_dialog
                    .open(settings_dialog::SettingsPage::Appearance);
            }
            if i.consume_key(alt | shift, Key::Insert) {
                self.settings.column_selection = !self.settings.column_selection;
                self.persist_settings();
            }
            if i.consume_key(alt, Key::Home) {
                if self.settings.navigation_bar == "Hidden" {
                    self.settings.navigation_bar = "Bottom".to_string();
                }
                self.ws.message = "Navigation Bar".to_string();
            }
            if i.consume_key(ctrl, Key::F4) {
                self.request_close_active();
            }
            if i.consume_key(alt, Key::ArrowLeft) {
                if !self.ws.navigate_back() {
                    self.ws.message = "sem histórico de navegação".to_string();
                }
            }
            if i.consume_key(alt, Key::ArrowRight) {
                if !self.ws.navigate_forward() {
                    self.ws.message = "sem histórico de navegação".to_string();
                }
            }
            if i.consume_key(alt, Key::Enter) {
                // Placeholder de intention actions: abre o quick-fix do problema na linha.
                self.goto_problem(1);
            }
            if i.consume_key(ctrl, Key::F) && i.modifiers.shift {
                self.tw.toggle(ToolWindow::Find);
            }
            if i.consume_key(ctrl | shift, Key::M) {
                self.tw.toggle(ToolWindow::Problems);
            }
            if i.consume_key(ctrl, Key::Equals) {
                self.set_zoom(self.zoom * 1.1);
            }
            if i.consume_key(ctrl, Key::Minus) {
                self.set_zoom(self.zoom / 1.1);
            }
            if i.consume_key(ctrl, Key::Num0) {
                self.set_zoom(1.0);
            }
            if i.consume_key(alt | shift, Key::Period) {
                self.set_zoom(self.zoom * 1.1);
            }
            if i.consume_key(alt | shift, Key::Comma) {
                self.set_zoom(self.zoom / 1.1);
            }
            if i.consume_key(ctrl, Key::Tab) {
                self.open_palette(PaletteMode::Recent);
            }
            if i.consume_key(ctrl, Key::Backtick) {
                self.scheme_open = true;
            }
            if i.consume_key(ctrl, Key::B) {
                def = true;
            }
            if i.consume_key(egui::Modifiers::NONE, Key::F12) {
                def = true;
            }
            if i.consume_key(ctrl, Key::Space) {
                complete = true;
            }
            if i.consume_key(ctrl, Key::R) {
                run_php = true;
            }
            if i.consume_key(ctrl, Key::D) {
                debug_php = true;
            }
            if i.consume_key(ctrl, Key::F2) {
                self.stop_execution();
            }
            if i.consume_key(egui::Modifiers::NONE, Key::F9) {
                bp_toggle = true;
            }
            if i.consume_key(egui::Modifiers::NONE, Key::F11) {
                self.toggle_bookmark();
            }
            if i.consume_key(ctrl, Key::F12) {
                self.tw.toggle(ToolWindow::Structure);
            }
            if i.consume_key(egui::Modifiers::NONE, Key::F2) {
                self.goto_problem(1);
            }
            if i.consume_key(shift, Key::F2) {
                self.goto_problem(-1);
            }
            if i.consume_key(ctrl, Key::OpenBracket) {
                block_start = true;
            }
            if i.consume_key(ctrl, Key::CloseBracket) {
                block_end = true;
            }
        });
        if save {
            self.save_active();
        }
        if undo {
            if let Some(buf) = self.ws.active_buffer_mut() {
                if !buf.readonly {
                    buf.undo();
                }
            }
        }
        if redo {
            if let Some(buf) = self.ws.active_buffer_mut() {
                if !buf.readonly {
                    buf.redo();
                }
            }
        }
        if def {
            self.request_definition();
        }
        if complete {
            self.trigger_completion();
        }
        if block_start {
            self.jump_block(true);
        }
        if block_end {
            self.jump_block(false);
        }
        if run_php {
            self.run_current_php();
        }
        if debug_php {
            self.debug_run();
        }
        if bp_toggle {
            self.toggle_breakpoint();
        }
    }

    fn file_tree(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("tree")
            .resizable(true)
            .default_width(220.0)
            .show(ctx, |ui| {
                tool_window_header(self, ui, ToolWindow::Project);
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let entries: Vec<phpide_core::FileEntry> = self
                        .ws
                        .files
                        .iter()
                        .filter(|e| {
                            !self.has_collapsed_ancestor(e.path.parent().unwrap_or(&e.path))
                        })
                        .cloned()
                        .collect();
                    for entry in entries {
                        let indent = 14.0 * entry.depth as f32;
                        ui.horizontal(|ui| {
                            ui.add_space(indent);
                            let name = entry
                                .path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_else(|| entry.path.display().to_string());
                            let label = if entry.is_dir {
                                if self.collapsed.contains(&entry.path) {
                                    format!("> {name}/")
                                } else {
                                    format!("v {name}/")
                                }
                            } else {
                                name
                            };
                            let is_php = phpide_php::Php::is_php_file(&entry.path);
                            let text = if is_php {
                                egui::RichText::new(label).color(egui::Color32::LIGHT_BLUE)
                            } else {
                                egui::RichText::new(label)
                            };
                            let resp = ui.selectable_label(
                                self.selected.as_deref() == Some(entry.path.as_path()),
                                text,
                            );
                            if resp.clicked() {
                                self.selected = Some(entry.path.clone());
                                if entry.is_dir {
                                    if !self.collapsed.insert(entry.path.clone()) {
                                        self.collapsed.remove(&entry.path);
                                    }
                                } else {
                                    self.ws.open_file(&entry.path);
                                }
                            }
                        });
                    }
                });
            });
    }

    fn search_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("search")
            .resizable(true)
            .show(ctx, |ui| {
                tool_window_header(self, ui, ToolWindow::Find);
                ui.separator();
                ui.horizontal(|ui| {
                    let edit = egui::TextEdit::singleline(&mut self.search.query)
                        .hint_text("texto (Enter para buscar)")
                        .desired_width(280.0)
                        .show(ui);
                    if edit.response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let files: Vec<PathBuf> = self
                            .ws
                            .files
                            .iter()
                            .filter(|e| !e.is_dir && search::is_searchable(&e.path))
                            .map(|e| e.path.clone())
                            .collect();
                        self.search.start(files, &self.jobs);
                    }
                    ui.checkbox(&mut self.search.case_sensitive, "Aa");
                    if ui.button("Buscar").clicked() {
                        let files: Vec<PathBuf> = self
                            .ws
                            .files
                            .iter()
                            .filter(|e| !e.is_dir && search::is_searchable(&e.path))
                            .map(|e| e.path.clone())
                            .collect();
                        self.search.start(files, &self.jobs);
                    }
                    if self.search.busy() {
                        ui.spinner();
                    }
                    ui.label(&self.search.status);
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        let hits: Vec<(PathBuf, usize, String)> = self
                            .search
                            .results
                            .iter()
                            .map(|h| (h.path.clone(), h.line, h.preview.clone()))
                            .collect();
                        for (path, line, preview) in hits {
                            let rel = path
                                .strip_prefix(&self.ws.root)
                                .unwrap_or(&path)
                                .display()
                                .to_string();
                            let label = format!("{rel}:{line}:  {preview}");
                            if ui.selectable_label(false, label).clicked() {
                                self.ws.open_file_at(&path, line.saturating_sub(1), 0);
                            }
                        }
                    });
            });
    }

    fn problems_panel(&mut self, ctx: &egui::Context) {
        let diags: Vec<phpide_core::Diagnostic> = self.ws.current_diagnostics().cloned().collect();
        egui::TopBottomPanel::bottom("problems")
            .resizable(true)
            .show(ctx, |ui| {
                tool_window_header(self, ui, ToolWindow::Problems);
                ui.separator();
                ui.label(format!("{} problemas", diags.len()));
                ui.separator();
                if diags.is_empty() {
                    ui.label("Sem problemas.");
                } else {
                    egui::ScrollArea::vertical()
                        .max_height(200.0)
                        .show(ui, |ui| {
                            for d in &diags {
                                let color = match d.severity {
                                    phpide_core::Severity::Error
                                    | phpide_core::Severity::Warning => egui::Color32::RED,
                                    _ => egui::Color32::YELLOW,
                                };
                                let rel = d
                                    .path
                                    .strip_prefix(&self.ws.root)
                                    .unwrap_or(&d.path)
                                    .display()
                                    .to_string();
                                let label = format!(
                                    "[{}] {rel}:{}:{}  {}",
                                    d.severity.marker(),
                                    d.line,
                                    d.col,
                                    d.message.lines().next().unwrap_or("")
                                );
                                if ui
                                    .selectable_label(
                                        false,
                                        egui::RichText::new(label).color(color),
                                    )
                                    .clicked()
                                {
                                    self.ws.open_file_at(
                                        &d.path,
                                        d.line.saturating_sub(1),
                                        d.col.saturating_sub(1),
                                    );
                                }
                            }
                        });
                }
            });
    }

    fn exec_panel(&mut self, ctx: &egui::Context) {
        let debug_active = self.debug_rx.is_some();
        let title = if debug_active { "Debug" } else { "Run" };
        egui::TopBottomPanel::bottom("exec")
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(title).strong());
                    if self.exec.running {
                        ui.spinner();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("X").on_hover_text("Hide (Shift+Esc)").clicked() {
                            if debug_active {
                                self.tw.set_visible(ToolWindow::Debug, false);
                            } else {
                                self.tw.set_visible(ToolWindow::Run, false);
                            }
                        }
                        if ui.button("Clear").on_hover_text("Limpar saída").clicked() {
                            self.exec.output.clear();
                        }
                        if ui.button("Stop").on_hover_text("Stop (Ctrl+F2)").clicked() {
                            self.stop_execution();
                        }
                    });
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(180.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(egui::RichText::new(&self.exec.output).monospace())
                                .selectable(true),
                        );
                    });
            });
    }

    /// Modal de confirmação para buffers sujos (NFR-007-02): aba, Close
    /// Others, troca de projeto e fechamento da IDE. Salvar grava tudo o que
    /// for necessário; Descartar fecha sem salvar; Cancelar volta ao fluxo.
    fn dirty_close_modal(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_close.clone() else {
            return;
        };
        let (title, body) = match &pending {
            PendingClose::Tab(i) => {
                let name = self
                    .ws
                    .buffers
                    .get(*i)
                    .map(|b| b.name())
                    .unwrap_or_else(|| "aba".to_string());
                (
                    "Fechar aba".to_string(),
                    format!("\"{name}\" tem alterações não salvas."),
                )
            }
            PendingClose::Others(_) => (
                "Fechar outras abas".to_string(),
                "Outras abas têm alterações não salvas.".to_string(),
            ),
            PendingClose::Project(path) => (
                "Trocar de projeto".to_string(),
                format!(
                    "Há alterações não salvas.\nAbrir \"{}\" mesmo assim?",
                    path.display()
                ),
            ),
            PendingClose::Exit => (
                "Sair da IDE".to_string(),
                "Há alterações não salvas. Sair mesmo assim?".to_string(),
            ),
        };
        let mut action: Option<&'static str> = None;
        egui::Window::new("dirty-close")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .frame(
                egui::Frame::window(&ctx.style())
                    .shadow(egui::epaint::Shadow::default())
                    .inner_margin(egui::Margin::same(12)),
            )
            .show(ctx, |ui| {
                ui.label(egui::RichText::new(title).strong());
                ui.add_space(4.0);
                ui.label(body);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Salvar").clicked() {
                        action = Some("save");
                    }
                    if ui.button("Não Salvar").clicked() {
                        action = Some("discard");
                    }
                    if ui.button("Cancelar").clicked() {
                        action = Some("cancel");
                    }
                });
            });
        match action {
            Some("save") => {
                match pending {
                    PendingClose::Tab(i) => {
                        if self.save_buffer_at(i) {
                            self.ws.close_tab(i);
                        }
                    }
                    PendingClose::Others(keep) => {
                        self.save_all_dirty();
                        self.ws.close_other_tabs(keep);
                    }
                    PendingClose::Project(path) => {
                        self.save_all_dirty();
                        self.pending_close = None;
                        self.load_project(path, ctx);
                        return;
                    }
                    PendingClose::Exit => {
                        self.save_all_dirty();
                        self.pending_close = None;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        return;
                    }
                }
                self.pending_close = None;
            }
            Some("discard") => {
                match pending {
                    PendingClose::Tab(i) => {
                        self.ws.close_tab(i);
                    }
                    PendingClose::Others(keep) => {
                        self.ws.close_other_tabs(keep);
                    }
                    PendingClose::Project(path) => {
                        self.pending_close = None;
                        self.load_project(path, ctx);
                        return;
                    }
                    PendingClose::Exit => {
                        // Descartar: fecha marcando os buffers como limpos
                        // (a confirmação já cobriu a perda explícita).
                        for buf in &mut self.ws.buffers {
                            buf.dirty = false;
                        }
                        self.pending_close = None;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        return;
                    }
                }
                self.pending_close = None;
            }
            _ => {}
        }
    }

    fn goto_line_popup(&mut self, ctx: &egui::Context) {
        if !self.goto_line_open {
            return;
        }
        let mut close = false;
        egui::Window::new("goto-line")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .frame(
                egui::Frame::window(&ctx.style())
                    .shadow(egui::epaint::Shadow::default())
                    .inner_margin(egui::Margin::same(10)),
            )
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("Go to Line").strong().small());
                ui.add_space(4.0);
                let edit = egui::TextEdit::singleline(&mut self.goto_line_input)
                    .hint_text("número da linha")
                    .desired_width(180.0)
                    .show(ui);
                edit.response.request_focus();
                ui.ctx().input(|i| {
                    if i.key_pressed(egui::Key::Enter) {
                        if let Ok(line) = self.goto_line_input.trim().parse::<usize>() {
                            if let Some(buf) = self.ws.active_buffer_mut() {
                                let line0 = line
                                    .saturating_sub(1)
                                    .min(buf.document.line_count().saturating_sub(1));
                                buf.request_cursor(line0, 0);
                            }
                        }
                        close = true;
                    }
                    if i.key_pressed(egui::Key::Escape) {
                        close = true;
                    }
                });
            });
        if close {
            self.goto_line_open = false;
            self.goto_line_input.clear();
        }
    }

    // -----------------------------------------------------------------
    // Estágio 3: integração LSP (poll por frame, nunca bloqueia a UI)
    // -----------------------------------------------------------------

    fn poll_lsp(&mut self) {
        let Some(lsp) = &self.lsp else { return };

        // 1. Handshake do initialize.
        if let Some(rx) = &self.lsp_init {
            match rx.try_recv() {
                Ok(result) => {
                    self.lsp_init = None;
                    if result.get("capabilities").is_some() {
                        *lsp.shared.status.lock().unwrap() =
                            LspStatus::Running(self.lsp_name.clone());
                        lsp.notify("initialized", Value::Null);
                        // Diagnósticos passam a ser do LSP; reanálise local do zero.
                        self.ws.clear_diagnostics();
                        self.syntax = None;
                        self.ws.message = format!("LSP conectado: {}", self.lsp_name);
                    } else {
                        *lsp.shared.status.lock().unwrap() =
                            LspStatus::Failed(format!("initialize rejeitado: {result}"));
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.lsp_init = None;
                    *lsp.shared.status.lock().unwrap() =
                        LspStatus::Failed("sem resposta do initialize".into());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if !matches!(lsp.status(), LspStatus::Running(_)) {
            return;
        }

        // 2. Sincronização de documentos: didOpen / didChange / didClose.
        let mut opened: Vec<(PathBuf, String, u64)> = Vec::new();
        let mut changed: Vec<(PathBuf, String, u64)> = Vec::new();
        for buf in &self.ws.buffers {
            let Some(path) = buf.path.clone() else {
                continue;
            };
            if !phpide_php::Php::is_php_file(&path) {
                continue;
            }
            let version = buf.document.version;
            match self.lsp_opened.get(&path) {
                None => opened.push((path, buf.document.text(), version)),
                Some(v) if *v != version => changed.push((path, buf.document.text(), version)),
                _ => {}
            }
        }
        for (path, text, version) in opened {
            lsp.did_open(&path, &text, version);
            self.lsp_opened.insert(path, version);
        }
        for (path, text, version) in changed {
            lsp.did_change(&path, &text, version);
            self.lsp_opened.insert(path, version);
        }
        let open_paths: HashSet<PathBuf> = self
            .ws
            .buffers
            .iter()
            .filter_map(|b| b.path.clone())
            .collect();
        let closed: Vec<PathBuf> = self
            .lsp_opened
            .keys()
            .filter(|p| !open_paths.contains(*p))
            .cloned()
            .collect();
        for path in closed {
            lsp.did_close(&path);
            self.lsp_opened.remove(&path);
            // Arquivo fechado: sem revisão viva; conjunto vazio remove as entradas.
            self.ws.set_diagnostics(&path, 0, Vec::new());
        }

        // 3. Diagnósticos publicados -> painel de problemas (anexados à
        // revisão do buffer: edição posterior invalida até nova publicação).
        let published: Vec<(PathBuf, Vec<LspDiagnostic>)> =
            lsp.shared.diagnostics.lock().unwrap().drain().collect();
        for (path, items) in published {
            let revision = self
                .ws
                .buffers
                .iter()
                .find(|b| b.path.as_deref() == Some(path.as_path()))
                .map(|b| b.document.revision())
                .unwrap_or(u64::MAX);
            let diags: Vec<phpide_core::Diagnostic> = items
                .iter()
                .map(|d| phpide_core::Diagnostic {
                    path: path.clone(),
                    line: d.line0 + 1,
                    col: d.col0 + 1,
                    message: d.message.clone(),
                    severity: match d.severity {
                        1 => phpide_core::Severity::Error,
                        2 => phpide_core::Severity::Warning,
                        3 => phpide_core::Severity::Information,
                        _ => phpide_core::Severity::Hint,
                    },
                    source: d.source.clone(),
                })
                .collect();
            self.ws.set_diagnostics(&path, revision, diags);
        }

        // 4. Definição pendente (F12/Ctrl+B).
        if let Some(rx) = &self.pending_def {
            if let Ok(result) = rx.try_recv() {
                self.pending_def = None;
                let loc = if let Some(arr) = result.as_array() {
                    arr.first()
                } else if result.is_object() {
                    Some(&result)
                } else {
                    None
                };
                if let Some(loc) = loc {
                    let uri = loc["uri"].as_str();
                    let line = loc["range"]["start"]["line"].as_u64();
                    if let (Some(uri), Some(line)) = (uri, line) {
                        let col = loc["range"]["start"]["character"].as_u64().unwrap_or(0);
                        if let Some(path) = uri_to_path(uri) {
                            self.ws.open_file_at(&path, line as usize, col as usize);
                        }
                    }
                }
            }
        }

        // 5. Outline pendente (só aplica se o arquivo ainda for o ativo).
        if let Some((req_path, rx)) = &self.pending_outline {
            let mut done = false;
            if let Ok(result) = rx.try_recv() {
                done = true;
                let active_path = self
                    .ws
                    .active_buffer()
                    .and_then(|b| b.path.clone())
                    .unwrap_or_default();
                if *req_path == active_path {
                    let mut entries = Vec::new();
                    collect_symbols(&result, 0, &mut entries);
                    self.ws.outline = entries;
                }
            }
            if done {
                self.pending_outline = None;
            }
        }

        // 6. Solicita outline quando o arquivo ativo muda de versão.
        if self.pending_outline.is_none() {
            if let Some(buf) = self.ws.active_buffer() {
                if let Some(path) = buf.path.clone() {
                    if phpide_php::Php::is_php_file(&path) {
                        let key = (path.clone(), buf.document.version);
                        if self.last_outline_key.as_ref() != Some(&key) {
                            self.last_outline_key = Some(key);
                            let rx = lsp.document_symbol(&path);
                            self.pending_outline = Some((path, rx));
                        }
                    }
                }
            }
        }
    }

    fn request_definition(&mut self) {
        let Some(lsp) = &self.lsp else { return };
        if !matches!(lsp.status(), LspStatus::Running(_)) {
            self.ws.message = "LSP ainda não está pronto".to_string();
            return;
        }
        let Some(buf) = self.ws.active_buffer() else {
            return;
        };
        let Some(path) = buf.path.clone() else { return };
        let (line, col) = buf
            .document
            .line_col(self.cursor_char.min(buf.document.len_chars()));
        let rx = lsp.definition(&path, line.saturating_sub(1), col.saturating_sub(1));
        self.pending_def = Some(rx);
    }

    fn outline_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("outline")
            .resizable(true)
            .default_width(210.0)
            .show(ctx, |ui| {
                tool_window_header(self, ui, ToolWindow::Structure);
                ui.separator();
                if self.ws.outline.is_empty() {
                    ui.label("—");
                } else {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let entries = self.ws.outline.clone();
                        for e in entries {
                            let jump = ui
                                .horizontal(|ui| {
                                    ui.add_space(12.0 * e.depth as f32);
                                    ui.selectable_label(false, format!("{}  {}", e.kind, e.name))
                                        .clicked()
                                })
                                .inner;
                            if jump {
                                let path = self.ws.active_buffer().and_then(|b| b.path.clone());
                                if let Some(path) = path {
                                    self.ws.open_file_at(&path, e.line0, 0);
                                }
                            }
                        }
                    });
                }
            });
    }

    fn lsp_label(&self) -> String {
        match &self.lsp {
            None => "LSP: off".to_string(),
            Some(lsp) => match lsp.status() {
                LspStatus::Starting => "LSP: conectando...".to_string(),
                LspStatus::Running(name) => format!("LSP: {name}"),
                LspStatus::Failed(e) => format!("LSP: falhou ({e})"),
            },
        }
    }

    fn trigger_completion(&mut self) {
        let Some(lsp) = &self.lsp else {
            self.ws.message = "LSP indisponível".to_string();
            return;
        };
        if !matches!(lsp.status(), LspStatus::Running(_)) {
            self.ws.message = "LSP ainda não está pronto".to_string();
            return;
        }
        let Some(buf) = self.ws.active_buffer() else {
            return;
        };
        let Some(path) = buf.path.clone() else { return };
        let cursor = self.cursor_char.min(buf.document.len_chars());
        let chars: Vec<char> = buf.document.text().chars().collect();
        let (prefix, word_start) = completion::word_at(&chars, cursor);
        let (line, col) = buf.document.line_col(cursor);
        self.completion.begin(
            lsp,
            &path,
            line.saturating_sub(1),
            col.saturating_sub(1),
            word_start,
            prefix,
        );
    }

    fn completion_popup(&mut self, ctx: &egui::Context) {
        if !self.completion.open {
            return;
        }
        self.completion.poll();
        if !self.completion.open {
            return;
        }
        let items = self.completion.filtered();
        if items.is_empty() && !self.completion.loading() {
            self.completion.close();
            return;
        }
        let mut accept: Option<String> = None;
        let mut close = false;

        egui::Window::new("completion")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -56.0))
            .frame(
                egui::Frame::window(&ctx.style())
                    .shadow(egui::epaint::Shadow::default())
                    .inner_margin(egui::Margin::same(8)),
            )
            .show(ctx, |ui| {
                let edit = egui::TextEdit::singleline(&mut self.completion.query)
                    .hint_text("filtrar...")
                    .desired_width(360.0)
                    .show(ui);
                if self.completion.just_opened {
                    edit.response.request_focus();
                    self.completion.just_opened = false;
                }
                ctx.input(|i| {
                    if !items.is_empty() {
                        if i.key_pressed(egui::Key::ArrowDown) {
                            self.completion.selected =
                                (self.completion.selected + 1).min(items.len() - 1);
                        }
                        if i.key_pressed(egui::Key::ArrowUp) {
                            self.completion.selected = self.completion.selected.saturating_sub(1);
                        }
                        if i.key_pressed(egui::Key::Enter) {
                            accept = Some(
                                items[self.completion.selected.min(items.len() - 1)]
                                    .label
                                    .clone(),
                            );
                        }
                    }
                    if i.key_pressed(egui::Key::Escape) {
                        close = true;
                    }
                });
                ui.separator();
                if self.completion.loading() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("consultando servidor...");
                    });
                }
                egui::ScrollArea::vertical()
                    .max_height(230.0)
                    .show(ui, |ui| {
                        for (i, item) in items.iter().enumerate() {
                            let sel = i == self.completion.selected;
                            let row = ui.selectable_label(
                                sel,
                                format!("{}   {} — {}", item.label, item.kind_label(), item.detail),
                            );
                            if row.clicked() {
                                accept = Some(item.label.clone());
                            }
                        }
                    });
            });

        if close {
            self.completion.close();
        }
        if let Some(label) = accept {
            self.accept_completion(ctx, &label);
        }
    }

    fn accept_completion(&mut self, ctx: &egui::Context, label: &str) {
        if let Some(buf) = self.ws.active_buffer_mut() {
            if buf.readonly {
                self.completion.close();
                return;
            }
            let cursor = self.cursor_char.min(buf.document.len_chars());
            let start = self.completion.word_start.min(cursor);
            if start < cursor {
                buf.delete(start, cursor);
            }
            buf.insert(start, label);
            self.cursor_char = start + label.chars().count();
            let id = egui::Id::new(("editor", self.ws.active));
            let mut st = TextEditState::load(ctx, id).unwrap_or_default();
            let cc = egui::text::CCursor {
                index: self.cursor_char,
                prefer_next_row: true,
            };
            st.cursor.set_char_range(Some(egui::text::CCursorRange {
                primary: cc,
                secondary: cc,
            }));
            st.store(ctx, id);
        }
        self.completion.close();
    }

    /// Análise estrutural do buffer ativo (tree-sitter). Diagnósticos locais só
    /// entram quando o LSP está fora; os blocos alimentam Ctrl+[ / Ctrl+] e os
    /// breadcrumbs sempre.
    fn poll_syntax(&mut self) {
        let lsp_running = self
            .lsp
            .as_ref()
            .map(|l| matches!(l.status(), LspStatus::Running(_)))
            .unwrap_or(false);
        let Some(buf) = self.ws.active_buffer() else {
            return;
        };
        let Some(path) = buf.path.clone() else { return };
        if !phpide_php::Php::is_php_file(&path) {
            return;
        }
        let version = buf.document.revision();
        let fresh = matches!(&self.syntax, Some((p, cache)) if p == &path
            && cache.revision() == version
            && cache.is_current(&buf.document));
        if fresh {
            return;
        }
        let report = phpide_syntax::analyze(&buf.document.text());
        self.syntax = Some((
            path.clone(),
            Revisioned::new(version, report.blocks.clone()),
        ));
        if !lsp_running {
            let diags: Vec<phpide_core::Diagnostic> = report
                .errors
                .iter()
                .map(|e| phpide_core::Diagnostic {
                    path: path.clone(),
                    line: e.line0 + 1,
                    col: e.col0 + 1,
                    message: e.message.clone(),
                    severity: phpide_core::Severity::Error,
                    source: "tree-sitter".to_string(),
                })
                .collect();
            self.ws.set_diagnostics(&path, version, diags);
        }
    }

    /// Salta para o início/fim do bloco estrutural que contém a linha do cursor.
    fn jump_block(&mut self, to_start: bool) {
        let target = {
            let Some(buf) = self.ws.active_buffer_mut() else {
                return;
            };
            if !buf
                .path
                .as_deref()
                .map(phpide_php::Php::is_php_file)
                .unwrap_or(false)
            {
                return;
            }
            let line0 = buf
                .document
                .line_col(self.cursor_char.min(buf.document.len_chars()))
                .0
                .saturating_sub(1);
            phpide_syntax::enclosing_block(&buf.document.text(), line0).map(|b| {
                if to_start {
                    b.start_line0
                } else {
                    b.end_line0
                }
            })
        };
        match target {
            Some(line0) => {
                if let Some(buf) = self.ws.active_buffer_mut() {
                    buf.request_cursor(line0, 0);
                }
            }
            None => self.ws.message = "sem bloco estrutural nesta linha".to_string(),
        }
    }

    // -----------------------------------------------------------------
    // Estágio 4 — executor: php / composer / phpunit
    // -----------------------------------------------------------------

    fn palette_extras(&self) -> Vec<(String, String)> {
        let mut v = vec![
            (
                "Executar: Rodar arquivo atual (Ctrl+R)".to_string(),
                "php:run".to_string(),
            ),
            (
                "Executar: Debug arquivo atual (Ctrl+D)".to_string(),
                "debug:run".to_string(),
            ),
        ];
        for (name, desc) in exec::composer_scripts(&self.ws.root) {
            let label = if desc.is_empty() {
                format!("Composer: {name}")
            } else {
                format!("Composer: {name} — {desc}")
            };
            v.push((label, format!("composer:{name}")));
        }
        if self.ws.root.join("vendor/bin/phpunit").exists() {
            v.push((
                "Testes: PHPUnit (vendor/bin/phpunit)".to_string(),
                "phpunit".to_string(),
            ));
        }
        for p in &self.plugins {
            v.push((p.label.clone(), p.id.clone()));
        }
        v
    }

    fn run_plugin(&mut self, id: &str) {
        let root = self.ws.root.clone();
        match id {
            "php:run" => self.run_current_php(),
            "phpunit" => {
                let bin = root.join("vendor/bin/phpunit");
                self.exec
                    .run("PHPUnit".into(), bin.display().to_string(), vec![], root);
            }
            "debug:run" => self.debug_run(),
            _ => {
                if let Some(script) = id.strip_prefix("composer:") {
                    let composer = match exec::resolve_composer(&self.settings) {
                        Ok(program) => program,
                        Err(e) => {
                            self.ws.message = e.to_string();
                            return;
                        }
                    };
                    let script = script.to_string();
                    self.exec.run(
                        format!("composer {script}"),
                        composer,
                        vec!["run-script".into(), script],
                        root,
                    );
                } else if let Some(plugin_id) = id.strip_prefix("plugin:") {
                    self.run_manifest_plugin(plugin_id);
                } else {
                    self.ws.message = format!("comando desconhecido: {id}");
                }
            }
        }
    }

    fn run_current_php(&mut self) {
        if self.exec.running {
            self.ws.message = "já há uma execução em andamento".to_string();
            return;
        }
        self.save_active();
        let Some(buf) = self.ws.active_buffer() else {
            return;
        };
        let Some(path) = buf.path.clone() else {
            self.ws.message = "salve o arquivo antes de executar".to_string();
            return;
        };
        // NFR-003-01: PHP por configuração (env/settings) antes do PATH;
        // ausência vira mensagem acionável (NFR-005), nunca panic.
        let php = match exec::resolve_php(&self.settings) {
            Ok(program) => program,
            Err(e) => {
                self.ws.message = e.to_string();
                return;
            }
        };
        let cwd = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.ws.root.clone());
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        self.tw.set_visible(ToolWindow::Run, true);
        self.exec.run(format!("php {file}"), php, vec![file], cwd);
    }

    /// Liga/desliga breakpoint na linha do cursor (F9).
    fn toggle_breakpoint(&mut self) {
        let Some(buf) = self.ws.active_buffer() else {
            return;
        };
        let Some(path) = buf.path.clone() else { return };
        if !phpide_php::Php::is_php_file(&path) {
            return;
        }
        let line = buf
            .document
            .line_col(self.cursor_char.min(buf.document.len_chars()))
            .0;
        self.toggle_breakpoint_at(line);
    }

    /// Alterna breakpoint na linha informada (1-based) — usado por F9 e pelo
    /// clique no gutter (Estágio 6.2).
    fn toggle_breakpoint_at(&mut self, line: usize) {
        let Some(path) = self.ws.active_buffer().and_then(|b| b.path.clone()) else {
            return;
        };
        let set = self.breakpoints.entry(path.clone()).or_default();
        let action = if set.remove(&line) {
            "removido"
        } else {
            set.insert(line);
            "adicionado"
        };
        if set.is_empty() {
            self.breakpoints.remove(&path);
        }
        let total: usize = self.breakpoints.values().map(|s| s.len()).sum();
        self.ws.message = format!("breakpoint {action} (Ln {line}) — total {total}");
    }

    /// Adiciona/remove um bookmark na linha atual (F11 / Bookmarks tool window).
    pub fn toggle_bookmark(&mut self) {
        let Some(buf) = self.ws.active_buffer() else {
            return;
        };
        let Some(path) = buf.path.clone() else { return };
        let line = buf
            .document
            .line_col(self.cursor_char.min(buf.document.len_chars()))
            .0;
        let entries = self.bookmarks.entry(path.clone()).or_default();
        let action = if entries.remove(&line) {
            "removido"
        } else {
            entries.insert(line);
            "adicionado"
        };
        if entries.is_empty() {
            self.bookmarks.remove(&path);
        }
        self.ws.message = format!("bookmark {action} (Ln {line})");
    }

    /// Debug do arquivo atual: sobe o listener DBGp e dispara o PHP com
    /// XDEBUG_CONFIG apontando para a IDE (zero-config quando ☎ está ligado).
    fn debug_run(&mut self) {
        if self.debug_rx.is_some() {
            self.ws.message = "já há uma sessão de debug aguardando".to_string();
            return;
        }
        self.save_active();
        let Some(buf) = self.ws.active_buffer() else {
            return;
        };
        let Some(path) = buf.path.clone() else {
            self.ws.message = "salve o arquivo antes de debugar".to_string();
            return;
        };
        let port: u16 = std::env::var("PHPIDE_DBGP_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(phpide_dbgp::DEFAULT_PORT);
        let bps: Vec<(PathBuf, u32)> = self
            .breakpoints
            .iter()
            .flat_map(|(p, lines)| lines.iter().map(move |&l| (p.clone(), l as u32)))
            .collect();
        let (tx, rx) = mpsc::channel();
        self.debug_rx = Some(rx);
        let (php_tx, php_rx) = mpsc::channel();
        self.debug_php_rx = Some(php_rx);
        self.tw.set_visible(ToolWindow::Debug, true);
        self.exec.title = "Debug".to_string();
        self.exec.output =
            format!("escutando xdebug em 127.0.0.1:{port} e executando com XDEBUG_CONFIG...\n\n");
        self.exec.running = true;
        std::thread::spawn(move || {
            let _ = run_with_breakpoints(port, Duration::from_secs(300), &bps, tx);
        });
        // Dispara o PHP com o XDEBUG_CONFIG conectando na IDE; stdout/stderr
        // vão para o painel Debug via canal dedicado.
        let php = match exec::resolve_php(&self.settings) {
            Ok(program) => program,
            Err(e) => {
                self.ws.message = e.to_string();
                self.exec.running = false;
                self.debug_rx = None;
                self.debug_php_rx = None;
                return;
            }
        };
        let cwd = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.ws.root.clone());
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let xdebug_config = format!("client_host=127.0.0.1 client_port={port}");
        std::thread::spawn(move || {
            let result = std::process::Command::new(&php)
                .arg(&file)
                .current_dir(&cwd)
                .env("XDEBUG_CONFIG", &xdebug_config)
                .env("XDEBUG_SESSION", "PHPIDE")
                .output();
            match result {
                Ok(out) => {
                    for line in String::from_utf8_lossy(&out.stdout).lines() {
                        let _ = php_tx.send(line.to_string());
                    }
                    for line in String::from_utf8_lossy(&out.stderr).lines() {
                        let _ = php_tx.send(format!("[stderr] {line}"));
                    }
                    let _ = php_tx.send(format!("[php exit {}]", out.status.code().unwrap_or(-1)));
                }
                Err(e) => {
                    let _ = php_tx.send(format!("[debug] falha ao rodar php: {e}"));
                }
            }
        });
    }

    fn poll_debug(&mut self) {
        if let Some(rx) = &self.debug_php_rx {
            let mut lines = Vec::new();
            while let Ok(l) = rx.try_recv() {
                lines.push(l);
            }
            for l in lines {
                self.exec.push_line(&l);
            }
        }
        if let Some(rx) = &self.debug_rx {
            let mut done = false;
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    DebugMsg::Line(l) => self.exec.push_line(&l),
                    DebugMsg::Done => done = true,
                }
            }
            if done {
                self.debug_rx = None;
                self.exec.running = false;
                self.debug_php_rx = None;
            }
        }
    }

    fn run_manifest_plugin(&mut self, id: &str) {
        let wanted = format!("plugin:{id}");
        let Some(p) = self
            .plugins
            .iter()
            .find(|p| p.id == wanted)
            .or_else(|| self.plugins.iter().find(|p| p.id == id))
        else {
            self.ws.message = format!("plugin não encontrado: {id}");
            return;
        };
        let (label, cmd, args) = (p.label.clone(), p.command.clone(), p.args.clone());
        let root = self.ws.root.clone();
        self.exec.run(label, cmd, args, root);
    }

    fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom.clamp(0.5, 3.0);
        self.persist_settings();
    }

    /// Breadcrumbs (REQUISITOS 1.4): trilha estrutural sob o cursor, clicável.
    fn breadcrumbs(&mut self, ui: &mut egui::Ui) {
        let Some(buf) = self.ws.active_buffer() else {
            return;
        };
        let Some(path) = buf.path.clone() else { return };
        if !phpide_php::Php::is_php_file(&path) {
            return;
        }
        let line0 = buf
            .document
            .line_col(self.cursor_char.min(buf.document.len_chars()))
            .0
            .saturating_sub(1);
        let Some((sp, cache)) = &self.syntax else {
            return;
        };
        if sp != &path {
            return;
        }
        // Blocos vigentes na revisão atual (cache inválido ⇒ sem trilha).
        let blocks = match self.ws.active_buffer() {
            Some(buf) => cache.get(&buf.document).cloned().unwrap_or_default(),
            None => Vec::new(),
        };
        let trail: Vec<(String, usize)> = blocks
            .iter()
            .filter(|b| {
                let end = if b.kind == "namespace_definition" {
                    usize::MAX
                } else {
                    b.end_line0
                };
                b.start_line0 <= line0 && end >= line0
            })
            .map(|b| {
                let label = if b.name.is_empty() {
                    b.kind.clone()
                } else {
                    format!("{} {}", short_kind(&b.kind), b.name)
                };
                (label, b.start_line0)
            })
            .collect();
        if trail.is_empty() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            for (i, (label, line)) in trail.iter().enumerate() {
                if i > 0 {
                    ui.label(egui::RichText::new("/").weak());
                }
                if ui.link(label).clicked() {
                    if let Some(buf) = self.ws.active_buffer_mut() {
                        buf.request_cursor(*line, 0);
                    }
                }
            }
        });
        ui.separator();
    }

    /// Pinta a superfície do editor sobre o viewport (Estágio 6.2): gutter,
    /// sticky lines e faixas de inspeção — usando a geometria exata da galley.
    fn paint_editor_surface(
        &mut self,
        ui: &egui::Ui,
        output: &egui::text_edit::TextEditOutput,
        _offset: egui::Vec2,
    ) {
        use phpide_core::Severity;
        let Some(path) = self.ws.active_buffer().and_then(|b| b.path.clone()) else {
            return;
        };
        // Snapshot: empréstimo imutável termina antes das mutações abaixo.
        let (total_lines, cursor_line1, text, blocks) = {
            let Some(buf) = self.ws.active_buffer() else {
                return;
            };
            let blocks: Vec<phpide_syntax::Block> = self
                .syntax
                .as_ref()
                .filter(|(p, _)| p == &path)
                .and_then(|(_, cache)| cache.get(&buf.document).cloned())
                .unwrap_or_default();
            (
                buf.document.line_count(),
                buf.document
                    .line_col(self.cursor_char.min(buf.document.len_chars()))
                    .0,
                buf.document.text(),
                blocks,
            )
        };
        let Some(first_row) = output.galley.rows.first() else {
            return;
        };
        let row_height = first_row.rect.height();
        // Espaço de tela egui: galley_pos já tem a rolagem fisicamente
        // aplicada; o viewport visível é o clip do próprio painel.
        let viewport = ui.clip_rect();
        let geom = editor_view::EditorGeometry {
            galley_pos: output.galley_pos,
            viewport,
            row_height,
        };
        let bps = self.breakpoints.get(&path).cloned().unwrap_or_default();
        let bms = self.bookmarks.get(&path).cloned().unwrap_or_default();
        let action = editor_view::paint_gutter(
            ui,
            &geom,
            total_lines,
            cursor_line1,
            &bps,
            &bms,
            output.galley.rows.len(),
        );
        if let Some(line1) = action.toggle_breakpoint {
            self.toggle_breakpoint_at(line1);
        }
        // Sticky lines: só para PHP com análise estrutural do arquivo ativo.
        if !blocks.is_empty() {
            let doc_lines: Vec<String> = text.lines().map(str::to_string).collect();
            if let Some(line0) = editor_view::paint_sticky_lines(ui, &geom, &doc_lines, &blocks) {
                if let Some(buf) = self.ws.active_buffer_mut() {
                    buf.request_cursor(line0, 0);
                }
            }
        }
        // Faixas de inspeção na scrollbar (diagnósticos do arquivo ativo).
        let diags: Vec<(u32, Severity)> = self
            .ws
            .diagnostics_for(&path)
            .map(|d| (d.line as u32, d.severity))
            .collect();
        editor_view::paint_scrollbar_stripes(ui, &geom, &diags, total_lines);
    }

    fn editor(&mut self, ctx: &egui::Context) {
        let mut want_completion = false;
        // Hook de demonstração/teste: rola o editor para uma linha no startup
        // (permite capturar sticky lines/gutter sem simulação de input).
        let scroll_hook: Option<f32> = std::env::var("PHPIDE_SCROLL_TO_LINE")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .map(|line| (line - 1.0).max(0.0));
        egui::CentralPanel::default().show(ctx, |ui| {
            self.breadcrumbs(ui);
            let active = self.ws.active;
            if self.ws.active_buffer().is_none() {
                ui.centered_and_justified(|ui| {
                    ui.label("Nenhum arquivo aberto.");
                });
                return;
            }

            // Salto pendente (palette, busca, problemas): posiciona cursor.
            let pending = self
                .ws
                .active_buffer_mut()
                .and_then(|buf| buf.take_pending_cursor());
            if let Some((line0, col0)) = pending {
                let char_at = self
                    .ws
                    .active_buffer()
                    .map(|buf| {
                        (buf.document.line_start(line0) + col0).min(buf.document.len_chars())
                    })
                    .unwrap_or(0);
                let id = egui::Id::new(("editor", active));
                let mut st = TextEditState::load(ui.ctx(), id).unwrap_or_default();
                let cc = egui::text::CCursor {
                    index: char_at,
                    prefer_next_row: true,
                };
                st.cursor.set_char_range(Some(egui::text::CCursorRange {
                    primary: cc,
                    secondary: cc,
                }));
                st.store(ui.ctx(), id);
                self.cursor_char = char_at;
            }

            // Snapshot do estado necessário ao widget (borrow termina aqui).
            let (old, readonly) = {
                let buf = self.ws.active_buffer().unwrap();
                (buf.document.text(), buf.readonly)
            };
            let mut edited = old.clone();
            let mut layouter = |ui: &egui::Ui, text: &str, wrap_width: f32| {
                let mut job = PhpLanguage.highlight(text, ui.style());
                job.wrap.max_width = wrap_width;
                ui.ctx().fonts(|fonts| fonts.layout_job(job))
            };
            // O gutter é pintado por cima do viewport; o texto ganha um
            // recuo à esquerda para nunca ficar sob ele.
            let scroll_area = egui::ScrollArea::vertical().auto_shrink([false; 2]);
            let scroll_area = match scroll_hook {
                Some(line) => scroll_area.vertical_scroll_offset(line * 18.0),
                None => scroll_area,
            };
            let scroll = scroll_area.show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.add_space(editor_view::GUTTER_WIDTH);
                    egui::TextEdit::multiline(&mut edited)
                        .id(egui::Id::new(("editor", active)))
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(40)
                        .lock_focus(true)
                        .interactive(!readonly)
                        .layouter(&mut layouter)
                        .show(ui)
                })
                .inner
            });
            let output = scroll.inner;

            if let Some(cursor) = output.cursor_range {
                self.cursor_char = cursor.primary.ccursor.index;
            }

            // ---- Estágio 6.2: superfície do editor (gutter/sticky/stripes) ----
            self.paint_editor_surface(ui, &output, scroll.state.offset);

            if edited != old {
                if let Some((at, removed, inserted)) = Self::diff_edit(&old, &edited) {
                    let mut edited_buf = self.ws.active_buffer_mut();
                    if let Some(buf) = edited_buf.take() {
                        if removed > 0 {
                            buf.delete(at, at + removed);
                        }
                        if !inserted.is_empty() {
                            buf.insert(at, &inserted);
                            // Gatilhos de completamento: $, -> e \ (namespace)
                            if matches!(inserted.chars().last(), Some('$' | '>' | '\\')) {
                                want_completion = true;
                            }
                        }
                    }
                }
            }
        });
        if want_completion && !self.completion.open {
            self.trigger_completion();
        }
    }
}

/// Header padrão das tool windows: título + Options (...) com Hide.
fn tool_window_header(app: &mut PhpIdeApp, ui: &mut egui::Ui, tw: ToolWindow) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tw.title()).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("X").on_hover_text("Hide (Shift+Esc)").clicked() {
                app.tw.set_visible(tw, false);
            }
            ui.menu_button("...", |ui| {
                ui.set_min_width(180.0);
                ui.label(egui::RichText::new(tw.title()).weak().small());
                ui.separator();
                if ui.button("Hide\tShift+Esc").clicked() {
                    app.tw.set_visible(tw, false);
                    ui.close_menu();
                }
                if ui.button("Hide All Tool Windows\tCtrl+Shift+F12").clicked() {
                    app.tw.toggle_hide_all();
                    ui.close_menu();
                }
            })
            .response
            .on_hover_text("Options");
        });
    });
}

/// Rótulo curto de kind para breadcrumbs.
fn short_kind(kind: &str) -> &'static str {
    match kind {
        "namespace_definition" => "ns",
        "class_declaration" => "class",
        "interface_declaration" => "interface",
        "trait_declaration" => "trait",
        "enum_declaration" => "enum",
        "function_definition" => "fn",
        "method_declaration" => "method",
        "if_statement" => "if",
        "foreach_statement" => "foreach",
        "while_statement" => "while",
        "for_statement" => "for",
        "switch_statement" => "switch",
        _ => "block",
    }
}

/// Converte a resposta de documentSymbol (SymbolInformation plano ou
/// DocumentSymbol hierárquico) em entradas de outline com profundidade.
fn collect_symbols(value: &Value, depth: usize, out: &mut Vec<phpide_core::OutlineEntry>) {
    let Some(arr) = value.as_array() else { return };
    for item in arr {
        let name = item["name"].as_str().unwrap_or("?").to_string();
        let kind = symbol_kind_label(item["kind"].as_u64().unwrap_or(0));
        if item.get("location").is_some() {
            // SymbolInformation: { name, kind, location: { uri, range } }
            if let Some(line) = item["location"]["range"]["start"]["line"].as_u64() {
                out.push(phpide_core::OutlineEntry {
                    name,
                    kind,
                    line0: line as usize,
                    depth,
                });
            }
        } else if let Some(line) = item["range"]["start"]["line"].as_u64() {
            // DocumentSymbol: { name, kind, range, selectionRange, children }
            out.push(phpide_core::OutlineEntry {
                name,
                kind,
                line0: line as usize,
                depth,
            });
            if let Some(children) = item.get("children") {
                collect_symbols(children, depth + 1, out);
            }
        }
    }
}

fn symbol_kind_label(kind: u64) -> String {
    match kind {
        3 => "namespace",
        5 => "classe",
        6 => "método",
        7 => "propriedade",
        10 => "enum",
        11 => "interface",
        12 => "função",
        13 => "variável",
        14 => "constante",
        26 => "trait",
        _ => "símbolo",
    }
    .to_string()
}
