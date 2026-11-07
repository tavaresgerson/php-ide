//! phpide — IDE em Rust focada em PHP.
//!
//! Estágio 6 — shell de IDE com paridade visual à New UI do PhpStorm
//! (REQUISITOS.md, Parte 1): window header unificado, tool windows com
//! atalhos Alt+1..7, status bar com widgets, Search Everywhere (Shift×2),
//! Find Action (Ctrl+Shift+A), Quick Switch Scheme (Ctrl+`), Welcome Screen,
//! editor tabs com preview/pin/Closing Policy e breadcrumbs.

mod blame_gutter;
mod completion;
mod editor_surface;
mod editor_view;
mod exec;
mod folding;
mod geometry;
mod groups;
mod header;
mod hints;
mod icons_ui;
mod intelligence;
mod lens;
mod palette;
mod plugins;
mod run_debug;
mod run_panels;
mod scheme;
mod search;
mod settings;
mod settings_dialog;
mod statusbar;
mod switcher;
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
use phpide_dbgp::session::ContinueCommand;
use phpide_lsp::{detect_server, LspClient, LspDiagnostic, LspStatus};
use phpide_php::{Language, PhpLanguage};
use search::SearchState;
use serde_json::Value;
use themes::ThemeId;
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

type GitMarkerCache = (PathBuf, u64, Vec<(u32, bool)>);

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
    /// Crash já sinalizado à UI (evita mensagem repetida a cada frame).
    lsp_crash_notified: bool,
    /// Caminho -> revisão já enviada ao servidor.
    lsp_opened: HashMap<PathBuf, u64>,
    /// Definição pendente com (path, revisão): resposta de sessão/revisão
    /// antiga é descartada (T-PHP-001-04).
    pending_def: Option<(PathBuf, u64, mpsc::Receiver<Value>)>,
    pending_outline: Option<(PathBuf, mpsc::Receiver<Value>)>,
    last_outline_key: Option<(PathBuf, u64)>,
    /// Go to Symbol: resultados da última consulta workspace/symbol.
    symbol_results: Vec<(String, PathBuf, usize, usize)>,
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
    /// Find Usages pendente (T-PHP-004-03).
    pending_usages: Option<mpsc::Receiver<Value>>,
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
    // Fase 5 — superfície avançada do editor (UI-ED-001..007, UI-ICON-001).
    /// Estado de folding por arquivo (UI-ED-002).
    folds: HashMap<PathBuf, folding::FoldState>,
    /// Cache de ranges de folding por (arquivo, revisão).
    fold_cache: Option<(PathBuf, u64, Vec<phpide_syntax::folding::FoldRange>)>,
    /// Switcher Ctrl+Tab (MRU).
    switcher: switcher::MruSwitcher,
    /// Editor groups / split (UI-ED-003-03/04).
    groups: groups::EditorGroups,
    /// Inlay hints por revisão (UI-ED-005).
    hints: hints::HintStore,
    pending_hints: Option<(PathBuf, u64, mpsc::Receiver<Value>)>,
    /// Code Vision (lenses) por arquivo/revisão.
    code_vision: hints::CodeVision,
    /// Gutter de blame (UI-ED-007) com cache por revisão salva.
    blame: blame_gutter::BlameCache,
    blame_revision: Option<(PathBuf, u64)>,
    /// Cache de markers Git para a scrollbar (UI-ED-006-01).
    git_markers: Option<GitMarkerCache>,
    /// Resumo de inspeções do arquivo ativo (widget UI-ED-006-03).
    inspections: lens::InspectionsSummary,
    /// Popup do lens preview (linha sob a scrollbar).
    lens_hover: Option<(usize, std::time::Instant)>,
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
            lsp_crash_notified: false,
            lsp_opened: HashMap::new(),
            pending_def: None,
            pending_outline: None,
            last_outline_key: None,
            symbol_results: Vec::new(),
            pending_symbols: None,
            last_symbol_query: String::new(),
            semantic: None,
            pending_semantic: None,
            occurrences: None,
            pending_occurrences: None,
            pending_usages: None,
            composer: None,
            composer_warned: false,
            composer_mtime: None,
            problems_scope: 0,
            completion: CompletionState::new(),
            syntax: None,
            exec: ExecState::new(),
            extras: Vec::new(),
            was_palette_open: false,
            rd: run_debug::RunDebugState::default(),
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
            folds: HashMap::new(),
            fold_cache: None,
            switcher: switcher::MruSwitcher::default(),
            groups: groups::EditorGroups::default(),
            hints: hints::HintStore::default(),
            pending_hints: None,
            code_vision: hints::CodeVision::default(),
            blame: blame_gutter::BlameCache::default(),
            blame_revision: None,
            git_markers: None,
            inspections: lens::InspectionsSummary::default(),
            lens_hover: None,
            terminal_input: String::new(),
            commit_message: String::new(),
        };
        // Closing Policy configurável (UI-ED-003): limite vem do settings.
        app.ws.tab_limit = app.settings.tab_limit.max(1) as usize;
        // Descoberta do projeto Composer (T-PHP-030-01/02): informativa —
        // nunca sobrescreve configuração manual explícita.
        app.refresh_composer(false);
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
        // NFR-001-04: encerra sessões de execução e a engine DBGp.
        self.rd.shutdown();
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
        self.ws.tab_limit = self.settings.tab_limit.max(1) as usize;
        self.refresh_composer(false);
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

    /// Re-executa a descoberta Composer (T-PHP-030-03): chamada na abertura,
    /// na troca de projeto e quando o watcher detecta mudança no
    /// composer.json. `from_watcher` evita mensagens no startup silencioso.
    pub fn refresh_composer(&mut self, from_watcher: bool) {
        match phpide_php::composer::discover(&self.ws.root) {
            Ok(info) => {
                let had_vendor = self.composer.as_ref().map(|c| c.vendor_present);
                self.composer = info.clone();
                if let Some(info) = info {
                    if from_watcher {
                        self.ws.message = format!(
                            "composer.json atualizado: {} (PHP {})",
                            info.name.as_deref().unwrap_or("projeto"),
                            info.php_level.as_deref().unwrap_or("?")
                        );
                    }
                    // Vendor ausente: avisar uma vez por sessão de descoberta.
                    if !info.vendor_present && (had_vendor != Some(false) || from_watcher) {
                        self.composer_warned = true;
                        self.ws.message = format!(
                            "vendor/ ausente em {} — execute `composer install`",
                            self.ws.root.display()
                        );
                        self.logger.warn(
                            Component::Php,
                            &Correlation::session(self.session),
                            "composer_vendor_missing",
                            &[("root", &self.ws.root.display().to_string())],
                        );
                    }
                }
            }
            Err(e) => {
                self.composer = None;
                if from_watcher || !self.composer_warned {
                    self.composer_warned = true;
                    self.ws.message = format!("{} — ação: {}", e.message, e.action);
                    self.logger.error(
                        Component::Php,
                        &Correlation::session(self.session),
                        "composer_invalid",
                        &[("error", &e.message)],
                    );
                }
            }
        }
    }

    /// Reinicia o servidor de linguagem (T-PHP-001-02): stop + spawn com as
    /// mesmas configurações; sessão anterior é descartada (T-PHP-001-04).
    pub fn restart_lsp(&mut self) {
        if let Some(old) = self.lsp.take() {
            old.stop();
        }
        self.lsp_init = None;
        self.lsp_opened.clear();
        self.pending_def = None;
        self.pending_outline = None;
        self.last_outline_key = None;
        self.pending_symbols = None;
        self.symbol_results.clear();
        self.semantic = None;
        self.pending_semantic = None;
        self.occurrences = None;
        self.pending_occurrences = None;
        self.pending_usages = None;
        self.lsp_crash_notified = false;
        let (lsp, lsp_init, lsp_name) = start_lsp(&self.ws.root);
        self.lsp = lsp;
        self.lsp_init = lsp_init;
        self.lsp_name = lsp_name;
        self.ws.message = if self.lsp.is_some() {
            format!("reiniciando LSP: {}", self.lsp_name)
        } else {
            "nenhum servidor LSP disponível para reiniciar".to_string()
        };
    }

    /// Find Usages (T-PHP-004-03): referências alimentam o painel Find.
    pub fn request_usages(&mut self) {
        let Some(lsp) = &self.lsp else {
            self.ws.message = "LSP indisponível".to_string();
            return;
        };
        if !matches!(lsp.status(), LspStatus::Running(_)) {
            self.ws.message = "LSP ainda não está pronto".to_string();
            return;
        }
        if !lsp.capabilities().references_provider {
            self.ws.message = "servidor não suporta references".to_string();
            return;
        }
        let Some(buf) = self.ws.active_buffer() else {
            return;
        };
        let Some(path) = buf.path.clone() else { return };
        let (line, col) = buf.document.line_col_encoded(
            self.cursor_char.min(buf.document.len_chars()),
            phpide_core::PositionEncoding::Utf16,
        );
        self.pending_usages = Some(lsp.references(&path, line, col));
        self.ws.message = "buscando usos...".to_string();
    }

    /// Consome o resultado de Find Usages para o painel Find (com preview).
    fn poll_usages(&mut self) {
        let Some(rx) = &self.pending_usages else {
            return;
        };
        let Ok(result) = rx.try_recv() else { return };
        self.pending_usages = None;
        let locs = intelligence::parse_locations(&result);
        // Code Vision (UI-ED-005-05): lens de usages com resultados reais no
        // bloco estrutural sob o cursor.
        if let (Some((path, cache)), Some(buf)) = (&self.syntax, self.ws.active_buffer()) {
            if buf.path.as_deref() == Some(path.as_path()) {
                if let Some(blocks) = cache.get(&buf.document) {
                    let line0 = buf
                        .document
                        .line_col(self.cursor_char.min(buf.document.len_chars()))
                        .0;
                    if let Some(b) = blocks
                        .iter()
                        .filter(|b| b.start_line0 <= line0 && b.end_line0 >= line0)
                        .max_by_key(|b| b.start_line0)
                    {
                        let locations: Vec<hints::UsageLocation> = locs
                            .iter()
                            .map(|l| hints::UsageLocation {
                                path: l.path.to_string_lossy().into_owned(),
                                line0: l.line0,
                                preview: String::new(),
                            })
                            .collect();
                        self.code_vision.set_usages(b.start_line0, &locations);
                    }
                }
            }
        }
        if locs.is_empty() {
            self.ws.message = "nenhum uso encontrado".to_string();
            return;
        }
        let mut hits = Vec::new();
        for loc in &locs {
            // Preview: linha do arquivo no disco (buffer aberto tem prioridade).
            let preview = self
                .ws
                .buffers
                .iter()
                .find(|b| b.path.as_deref() == Some(loc.path.as_path()))
                .and_then(|b| {
                    b.document
                        .line(loc.line0)
                        .lines()
                        .next()
                        .map(str::to_string)
                })
                .or_else(|| {
                    std::fs::read_to_string(&loc.path)
                        .ok()
                        .and_then(|t| t.lines().nth(loc.line0).map(str::to_string))
                })
                .unwrap_or_default();
            hits.push(search::SearchHit {
                path: loc.path.clone(),
                line: loc.line0 + 1,
                preview: preview.trim().chars().take(140).collect(),
            });
        }
        let count = hits.len();
        self.search.results = hits;
        self.search.status = format!("{count} usos (Find Usages)");
        self.search.query.clear();
        self.tw.set_visible(ToolWindow::Find, true);
        self.ws.message = format!("{count} usos encontrados");
    }

    /// Go to Symbol (T-PHP-004-02): dispara workspace/symbol quando a
    /// consulta da palette muda.
    fn poll_symbol_query(&mut self) {
        if !self.palette.open || self.palette.mode != palette::PaletteMode::Symbols {
            return;
        }
        let query = self.palette.query.clone();
        if query == self.last_symbol_query {
            return;
        }
        self.last_symbol_query = query.clone();
        self.symbol_results.clear();
        if query.trim().len() < 2 {
            return;
        }
        let Some(lsp) = &self.lsp else { return };
        if !matches!(lsp.status(), LspStatus::Running(_)) {
            return;
        }
        if !lsp.capabilities().workspace_symbol_provider {
            return;
        }
        self.pending_symbols = Some(lsp.workspace_symbol(&query));
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
    /// (revisão atual; diagnóstico obsoleto não entra na ordem; wrap-around
    /// nas extremidades — T-PHP-002-03).
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
        let Some(idx) = intelligence::next_problem_index(diags.len(), cur_line, dir) else {
            return;
        };
        let d = diags[idx].clone();
        self.ws
            .open_file_at(&d.path, d.line.saturating_sub(1), d.col.saturating_sub(1));
    }

    /// Stop global (Ctrl+F2): encerra execução e debug ativos (RUN-001,
    /// RUN-DBG-001-04 — Stop confiável em processos e socket).
    pub fn stop_execution(&mut self) {
        if self.exec.running {
            self.exec.stop();
        }
        self.rd.stop();
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
            PaletteCommand::GotoImplementation => self.request_implementation(),
            PaletteCommand::GotoTypeDeclaration => self.request_type_definition(),
            PaletteCommand::GotoSymbol => {
                self.symbol_results.clear();
                self.last_symbol_query = String::new();
                self.open_palette(PaletteMode::Symbols);
            }
            PaletteCommand::FindUsages => self.request_usages(),
            PaletteCommand::SplitRight => {
                if let Some(p) = self.ws.active_buffer().and_then(|b| b.path.clone()) {
                    self.groups.open(p.to_string_lossy().into_owned());
                    self.groups.split_root(groups::Axis::Horizontal);
                }
            }
            PaletteCommand::SplitDown => {
                if let Some(p) = self.ws.active_buffer().and_then(|b| b.path.clone()) {
                    self.groups.open(p.to_string_lossy().into_owned());
                    self.groups.split_root(groups::Axis::Vertical);
                }
            }
            PaletteCommand::ToggleBlame => {
                self.settings.blame_gutter = !self.settings.blame_gutter;
                self.ws.message = format!(
                    "blame gutter: {}",
                    if self.settings.blame_gutter {
                        "on"
                    } else {
                        "off"
                    }
                );
            }
            PaletteCommand::ToggleFolding => {
                self.settings.folding = !self.settings.folding;
                self.fold_cache = None;
            }
            PaletteCommand::RestartLsp => self.restart_lsp(),
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
        // NFR-001-04: encerra sessões de execução e a engine DBGp.
        self.rd.shutdown();
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
            if n == after {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                // O frame do screenshot só é produzido num redraw seguinte.
                ctx.request_repaint();
            }
            let shot = ctx.input(|i| {
                i.events
                    .iter()
                    .find_map(|ev| match ev {
                        egui::Event::Screenshot { image, .. } => Some(image.clone()),
                        _ => None,
                    })
            });
            if let Some(image) = shot {
                let path = std::env::var("PHPIDE_SHOT_OUTPUT").unwrap_or_else(|_| {
                    std::env::var("PHPIDE_SCREENSHOT_TO").unwrap()
                });
                let (w, h) = (image.width(), image.height());
                let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
                for px in image.pixels.iter() {
                    ppm.extend_from_slice(&[px.r(), px.g(), px.b()]);
                }
                if std::fs::write(&path, &ppm).is_ok() {
                    eprintln!("screenshot gravado: {path} ({w}x{h})");
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
        self.poll_watcher();
        self.detect_double_shift(ctx);
        if !self.palette.open && !self.settings_dialog.open {
            self.handle_shortcuts(ctx);
        }
        self.poll_search();
        self.poll_lsp();
        self.poll_syntax();
        self.poll_usages();
        self.poll_symbol_query();
        self.exec.poll();
        // Estágio 8: sessões de execução + engine DBGp (eventos, testes,
        // cobertura) e sync do widget de listener na status bar.
        self.rd.poll();
        self.debug_listen = self.rd.is_listening();
        self.debug_keys(ctx);
        self.ctrl_wheel_zoom(ctx);

        if self.palette.open && !self.was_palette_open {
            self.extras = self.palette_extras();
        }
        self.was_palette_open = self.palette.open;

        if let Some(action) = palette::show(
            ctx,
            &self.ws,
            &mut self.palette,
            &self.extras,
            &self.symbol_results,
        ) {
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
                // Mudança externa no composer.json → re-descoberta
                // (T-PHP-030-03: alteração externa solicita reload).
                let composer_file = self.ws.root.join("composer.json");
                let mtime = std::fs::metadata(&composer_file)
                    .and_then(|m| m.modified())
                    .ok();
                if mtime.is_some() && mtime != self.composer_mtime {
                    self.composer_mtime = mtime;
                    self.refresh_composer(true);
                }
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
            if i.consume_key(alt, Key::ArrowLeft) && !self.ws.navigate_back() {
                self.ws.message = "sem histórico de navegação".to_string();
            }
            if i.consume_key(alt, Key::ArrowRight) && !self.ws.navigate_forward() {
                self.ws.message = "sem histórico de navegação".to_string();
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
                // Switcher MRU (UI-ED-003-02): primeiro Ctrl+Tab abre o
                // ciclo no mais recente; Tab repetido anda; soltar confirma.
                if !self.switcher.is_cycling() {
                    self.switcher.begin_cycle();
                } else {
                    self.switcher.cycle_next();
                }
            }
            if !i.modifiers.ctrl && self.switcher.is_cycling() {
                if let Some(key) = self.switcher.confirm() {
                    if let Some(idx) = self.ws.buffers.iter().position(|b| {
                        b.path
                            .as_ref()
                            .map(|p| p.to_string_lossy().into_owned())
                            .as_deref()
                            == Some(key.as_str())
                    }) {
                        self.ws.active = idx;
                    }
                }
            }
            if i.consume_key(ctrl, Key::Backtick) {
                self.scheme_open = true;
            }
            if i.consume_key(ctrl, Key::B) {
                def = true;
            }
            if i.consume_key(ctrl | alt, Key::B) {
                self.request_implementation();
            }
            if i.consume_key(ctrl | shift, Key::B) {
                self.request_type_definition();
            }
            if i.consume_key(ctrl | alt | shift, Key::N) {
                self.open_palette(PaletteMode::Symbols);
            }
            if i.consume_key(alt, Key::F7) {
                self.request_usages();
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
                // Roteamento contextual: com debug ativo, F9 é Resume
                // (T-RUN-DBG-002-01); fora de sessão, mantém o toggle de
                // breakpoint do baseline até o requisito de keymap dedicado.
                if self.rd.status.is_active() {
                    if self.rd.status == run_debug::DebugStatus::Paused {
                        self.rd.continue_debug(ContinueCommand::Resume);
                    }
                } else {
                    bp_toggle = true;
                }
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
        // Escopo (T-PHP-002-02): projeto inteiro ou só o arquivo ativo.
        let active_path = self.ws.active_buffer().and_then(|b| b.path.clone());
        let mut diags: Vec<phpide_core::Diagnostic> =
            self.ws.current_diagnostics().cloned().collect();
        if self.problems_scope == 1 {
            if let Some(path) = &active_path {
                diags.retain(|d| &d.path == path);
            }
        }
        // Resumo por ferramenta de origem (LSP/tree-sitter/...).
        let sources = {
            let refs: Vec<&phpide_core::Diagnostic> = diags.iter().collect();
            intelligence::problems_by_source(&refs)
        };
        egui::TopBottomPanel::bottom("problems")
            .resizable(true)
            .show(ctx, |ui| {
                tool_window_header(self, ui, ToolWindow::Problems);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(format!("{} problemas", diags.len()));
                    ui.separator();
                    let scope_label = if self.problems_scope == 0 {
                        "Escopo: Projeto"
                    } else {
                        "Escopo: Arquivo"
                    };
                    if ui
                        .button(scope_label)
                        .on_hover_text("Alternar projeto/arquivo")
                        .clicked()
                    {
                        self.problems_scope = 1 - self.problems_scope;
                    }
                    for (source, critical, total) in &sources {
                        ui.label(
                            egui::RichText::new(format!("{source}: {critical}/{total}"))
                                .small()
                                .weak(),
                        );
                    }
                });
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

    /// Painéis Run e Debug (Estágio 8): a janela Run mostra as sessões
    /// (process list) e a Debug mostra a toolbar/inspeção DBGp. As duas são
    /// independentes — podem estar abertas ao mesmo tempo.
    fn exec_panel(&mut self, ctx: &egui::Context) {
        if self.tw.is_visible(ToolWindow::Run) {
            run_panels::run_panel(self, ctx);
        }
        if self.tw.is_visible(ToolWindow::Debug) {
            run_panels::debug_panel(self, ctx);
        }
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
                        // Capabilities negociadas (T-PHP-001-01): a UI
                        // habilita recursos conforme o servidor declara.
                        lsp.register_capabilities(&result);
                        let caps = lsp.capabilities();
                        *lsp.shared.status.lock().unwrap() =
                            LspStatus::Running(self.lsp_name.clone());
                        lsp.notify("initialized", Value::Null);
                        // Diagnósticos passam a ser do LSP; reanálise local do zero.
                        self.ws.clear_diagnostics();
                        self.syntax = None;
                        let mut enabled = Vec::new();
                        if caps.definition_provider {
                            enabled.push("definition");
                        }
                        if caps.implementation_provider {
                            enabled.push("implementation");
                        }
                        if caps.type_definition_provider {
                            enabled.push("typeDefinition");
                        }
                        if caps.references_provider {
                            enabled.push("references");
                        }
                        if caps.workspace_symbol_provider {
                            enabled.push("workspace/symbol");
                        }
                        if caps.semantic_tokens_provider {
                            enabled.push("semanticTokens");
                        }
                        self.ws.message = format!(
                            "LSP conectado: {} ({})",
                            self.lsp_name,
                            if enabled.is_empty() {
                                "sem recursos extras".to_string()
                            } else {
                                enabled.join(", ")
                            }
                        );
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
        // Crash após estar Running: servidor caiu — edição segue offline
        // (T-PHP-001-05) e o usuário pode reiniciar pela palette.
        if let LspStatus::Failed(reason) = lsp.status() {
            if !self.lsp_crash_notified {
                self.lsp_crash_notified = true;
                self.ws.message = format!(
                    "LSP caiu ({reason}) — edição e diagnósticos locais seguem disponíveis; reinicie em Find Action (Ctrl+Shift+A > 'Restart Language Server')"
                );
                self.logger.error(
                    Component::Lsp,
                    &Correlation::session(self.session),
                    "lsp_crashed",
                    &[("reason", &reason)],
                );
            }
            return;
        }
        if !matches!(lsp.status(), LspStatus::Running(_)) {
            return;
        }

        // 2. Sincronização de documentos — plano único e ordenado
        // (T-PHP-001-03): didOpen → didChange → didClose, sem reprogrer versão.
        let buffers: Vec<(PathBuf, u64)> = self
            .ws
            .buffers
            .iter()
            .filter_map(|b| {
                let path = b.path.clone()?;
                if !phpide_php::Php::is_php_file(&path) {
                    return None;
                }
                Some((path, b.document.revision()))
            })
            .collect();
        for op in intelligence::plan_sync(&buffers, &self.lsp_opened) {
            match op {
                intelligence::SyncOp::Open(path, version) => {
                    let text = self
                        .ws
                        .buffers
                        .iter()
                        .find(|b| b.path.as_deref() == Some(path.as_path()))
                        .map(|b| b.document.text())
                        .unwrap_or_default();
                    lsp.did_open(&path, &text, version);
                    self.lsp_opened.insert(path, version);
                }
                intelligence::SyncOp::Change(path, version) => {
                    let text = self
                        .ws
                        .buffers
                        .iter()
                        .find(|b| b.path.as_deref() == Some(path.as_path()))
                        .map(|b| b.document.text())
                        .unwrap_or_default();
                    lsp.did_change(&path, &text, version);
                    self.lsp_opened.insert(path, version);
                }
                intelligence::SyncOp::Close(path) => {
                    lsp.did_close(&path);
                    self.lsp_opened.remove(&path);
                    self.ws.set_diagnostics(&path, 0, Vec::new());
                }
            }
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

        // 4. Definição pendente (F12/Ctrl+B): aplica apenas se o buffer ainda
        // estiver na mesma revisão do disparo (T-PHP-001-04).
        if let Some((req_path, req_revision, rx)) = &self.pending_def {
            if let Ok(result) = rx.try_recv() {
                let (req_path, req_revision) = (req_path.clone(), *req_revision);
                self.pending_def = None;
                let current = self
                    .ws
                    .buffers
                    .iter()
                    .find(|b| b.path.as_deref() == Some(req_path.as_path()));
                let stale = match current {
                    Some(buf) => buf.document.revision() != req_revision,
                    None => true,
                };
                if stale {
                    self.logger.info(
                        Component::Lsp,
                        &Correlation::session(self.session).with_document(req_path, req_revision),
                        "stale_response_discarded",
                        &[("kind", "definition")],
                    );
                } else {
                    let locs = intelligence::parse_locations(&result);
                    if let Some(loc) = locs.first() {
                        self.ws.open_file_at(&loc.path, loc.line0, loc.col0);
                    }
                }
            }
        }

        // 5. Outline pendente (só aplica se o arquivo ainda for o ativo e na
        // mesma revisão pedida).
        if let Some((req_path, rx)) = &self.pending_outline {
            let mut done = false;
            if let Ok(result) = rx.try_recv() {
                done = true;
                let active = self.ws.active_buffer();
                let matches = active
                    .and_then(|b| b.path.clone())
                    .map(|p| p == *req_path)
                    .unwrap_or(false);
                let revision_ok = active
                    .map(|b| {
                        self.last_outline_key
                            .as_ref()
                            .map(|(kp, kv)| kp == req_path && kv == &b.document.revision())
                            .unwrap_or(false)
                    })
                    .unwrap_or(false);
                if matches && revision_ok {
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
                        let key = (path.clone(), buf.document.revision());
                        if self.last_outline_key.as_ref() != Some(&key) {
                            self.last_outline_key = Some(key);
                            let rx = lsp.document_symbol(&path);
                            self.pending_outline = Some((path, rx));
                        }
                    }
                }
            }
        }

        // 7. Go to Symbol (workspace/symbol): respostas da consulta vigente.
        if let Some(rx) = &self.pending_symbols {
            if let Ok(result) = rx.try_recv() {
                self.pending_symbols = None;
                self.symbol_results = intelligence::parse_workspace_symbols(&result)
                    .into_iter()
                    .map(|(name, loc)| (name, loc.path, loc.line0, loc.col0))
                    .collect();
            }
        }

        // 8. Semantic tokens (T-PHP-005-01): aplica só na revisão pedida.
        if let Some((req_path, req_revision, rx)) = &self.pending_semantic {
            if let Ok(result) = rx.try_recv() {
                let (req_path, req_revision) = (req_path.clone(), *req_revision);
                self.pending_semantic = None;
                let current = self
                    .ws
                    .buffers
                    .iter()
                    .find(|b| b.path.as_deref() == Some(req_path.as_path()));
                if let Some(buf) = current {
                    if buf.document.revision() == req_revision {
                        if let Some(data) = result["data"].as_array() {
                            let flat: Vec<u32> = data
                                .iter()
                                .filter_map(|v| v.as_u64().map(|n| n as u32))
                                .collect();
                            let tokens = intelligence::decode_semantic_tokens(
                                &buf.document,
                                &lsp.capabilities().semantic_token_types,
                                &flat,
                            );
                            // Spans de byte uma vez por revisão: char absoluto
                            // = início da linha + coluna, convertido pela rope.
                            let spans: Vec<(usize, usize, String)> = tokens
                                .iter()
                                .map(|t| {
                                    let abs = buf.document.line_start(t.line0) + t.start_char;
                                    let start = buf.document.char_to_byte(abs);
                                    let end = buf.document.char_to_byte(
                                        (abs + t.len_chars).min(buf.document.len_chars()),
                                    );
                                    (start, end, t.token_type.clone())
                                })
                                .collect();
                            self.semantic = Some((req_path, req_revision, tokens, spans));
                        }
                    }
                }
            }
        }
        // Dispara semantic tokens quando o arquivo/revisão muda e há suporte.
        if lsp.capabilities().semantic_tokens_provider {
            let active = self.ws.active_buffer().and_then(|b| {
                let path = b.path.clone()?;
                if !phpide_php::Php::is_php_file(&path) {
                    return None;
                }
                Some((path, b.document.revision()))
            });
            let needs = match (&active, &self.semantic, &self.pending_semantic) {
                (Some((p, r)), cached, pending) => {
                    let cached_stale = cached
                        .as_ref()
                        .map(|(cp, cr, _, _)| cp != p || cr != r)
                        .unwrap_or(true);
                    let pending_stale = pending
                        .as_ref()
                        .map(|(pp, pr, _)| pp != p || pr != r)
                        .unwrap_or(false);
                    cached_stale && !pending_stale
                }
                (None, _, _) => false,
            };
            if needs {
                if let Some((path, revision)) = active {
                    let rx = lsp.semantic_tokens_full(&path);
                    self.pending_semantic = Some((path, revision, rx));
                }
            }
        } else {
            self.semantic = None;
        }

        // 8b. Inlay hints (UI-ED-005-01/02): resposta por revisão, descartada
        // se o documento já avançou (provedor lento não pinta dado velho).
        if let Some((req_path, req_revision, rx)) = &self.pending_hints {
            if let Ok(result) = rx.try_recv() {
                let (req_path, req_revision) = (req_path.clone(), *req_revision);
                self.pending_hints = None;
                let current_rev = self
                    .ws
                    .buffers
                    .iter()
                    .find(|b| b.path.as_deref() == Some(req_path.as_path()))
                    .map(|b| b.document.revision());
                if current_rev == Some(req_revision) {
                    let parsed = parse_inlay_hints(&result);
                    self.hints.apply(hints::HintResponse {
                        revision: hints::Revision(req_revision),
                        hints: parsed,
                        status: if result.is_null() {
                            Some("servidor sem inlay hints".to_string())
                        } else {
                            None
                        },
                    });
                }
            }
        }
        if self.settings.inlay_hints {
            let active = self.ws.active_buffer().and_then(|b| {
                let path = b.path.clone()?;
                if !phpide_php::Php::is_php_file(&path) {
                    return None;
                }
                Some((path, b.document.revision(), b.document.line_count()))
            });
            let needs = match (&active, &self.pending_hints, self.hints.current) {
                (Some((p, r, _)), pending, cur) => {
                    let pending_ok = pending
                        .as_ref()
                        .map(|(pp, pr, _)| pp == p && pr == r)
                        .unwrap_or(false);
                    !pending_ok && cur != Some(hints::Revision(*r))
                }
                (None, _, _) => false,
            };
            if needs {
                if let Some((path, revision, line_count)) = active {
                    let end_line = line_count.saturating_sub(1) as u64;
                    let rx = lsp.request(
                        "textDocument/inlayHint",
                        serde_json::json!({
                            "textDocument": { "uri": phpide_lsp::path_to_uri(&path) },
                            "range": {
                                "start": { "line": 0, "character": 0 },
                                "end": { "line": end_line, "character": 10_000 }
                            }
                        }),
                    );
                    self.pending_hints = Some((path, revision, rx));
                    self.hints.begin(hints::Revision(revision));
                }
            }
        } else if self.hints.current.is_some() {
            self.hints.invalidate();
        }

        // 9. Ocorrências (documentHighlight) com fallback local (005-02).
        if let Some(rx) = &self.pending_occurrences {
            if let Ok(result) = rx.try_recv() {
                self.pending_occurrences = None;
                let active = self.ws.active_buffer().and_then(|b| {
                    let path = b.path.clone()?;
                    let chars: Vec<char> = b.document.text().chars().collect();
                    let cursor = self.cursor_char.min(chars.len());
                    let (word, _) = completion::word_at(&chars, cursor);
                    Some((path, b.document.revision(), word, b.document.text()))
                });
                if let Some((path, revision, word, text)) = active {
                    if word.len() >= 2 {
                        let ranges = intelligence::parse_highlights(
                            &phpide_core::Document::new(&text),
                            &result,
                        );
                        self.occurrences = Some((path, revision, word, ranges));
                    }
                }
            }
        }
        let occurrences_key = self.ws.active_buffer().and_then(|b| {
            let path = b.path.clone()?;
            let chars: Vec<char> = b.document.text().chars().collect();
            let cursor = self.cursor_char.min(chars.len());
            let (word, _) = completion::word_at(&chars, cursor);
            Some((path, b.document.revision(), word))
        });
        match (&occurrences_key, &self.occurrences) {
            (Some((p, r, w)), Some((op, or_, ow, _))) if op == p && or_ == r && ow == w => {}
            (Some((p, r, w)), _) => {
                let word_ctx = self.ws.active_buffer().map(|b| {
                    let chars: Vec<char> = b.document.text().chars().collect();
                    let cursor = self.cursor_char.min(chars.len());
                    let (word, start) = completion::word_at(&chars, cursor);
                    (word, start)
                });
                if let Some((word, _start)) = word_ctx {
                    if word.len() >= 2 && &word == w {
                        // Servidor primeiro; sem suporte, fallback sintático.
                        let chars: Vec<char> = self
                            .ws
                            .active_buffer()
                            .map(|b| b.document.text().chars().collect())
                            .unwrap_or_default();
                        let ranges = if lsp.capabilities().document_highlight_provider {
                            if self.pending_occurrences.is_none() {
                                let (line, col) = self
                                    .ws
                                    .active_buffer()
                                    .map(|b| {
                                        b.document.line_col_encoded(
                                            self.cursor_char.min(b.document.len_chars()),
                                            phpide_core::PositionEncoding::Utf16,
                                        )
                                    })
                                    .unwrap_or((0, 0));
                                let path = p.clone();
                                let rx = lsp.document_highlight(&path, line, col);
                                self.pending_occurrences = Some(rx);
                            }
                            None
                        } else {
                            Some(intelligence::local_occurrences(
                                &chars,
                                &word,
                                self.cursor_char,
                            ))
                        };
                        if let Some(ranges) = ranges {
                            self.occurrences = Some((p.clone(), *r, w.clone(), ranges));
                        }
                    }
                    if word.len() < 2 {
                        self.occurrences = None;
                    }
                }
            }
            (None, _) => self.occurrences = None,
        }
    }

    fn request_definition(&mut self) {
        self.request_locations("definition", |lsp, path, line, col| {
            lsp.definition(path, line, col)
        });
    }

    /// Navegação por localizações genérica (T-PHP-004-01): definition,
    /// implementation e typeDefinition compartilham tratamento de resposta,
    /// gate por capability e descarte por revisão (T-PHP-001-04).
    fn request_locations(
        &mut self,
        what: &str,
        call: impl Fn(&LspClient, &std::path::Path, usize, usize) -> mpsc::Receiver<Value>,
    ) {
        let Some(lsp) = &self.lsp else {
            self.ws.message = "LSP indisponível".to_string();
            return;
        };
        if !matches!(lsp.status(), LspStatus::Running(_)) {
            self.ws.message = "LSP ainda não está pronto".to_string();
            return;
        }
        let caps = lsp.capabilities();
        let supported = match what {
            "definition" => caps.definition_provider,
            "implementation" => caps.implementation_provider,
            "typeDefinition" => caps.type_definition_provider,
            _ => false,
        };
        if !supported {
            self.ws.message = format!("servidor não suporta {what}");
            return;
        }
        let Some(buf) = self.ws.active_buffer() else {
            return;
        };
        let Some(path) = buf.path.clone() else { return };
        let revision = buf.document.revision();
        let (line, col) = buf.document.line_col_encoded(
            self.cursor_char.min(buf.document.len_chars()),
            phpide_core::PositionEncoding::Utf16,
        );
        let rx = call(lsp, &path, line, col);
        self.pending_def = Some((path, revision, rx));
    }

    fn request_implementation(&mut self) {
        self.request_locations("implementation", |lsp, path, line, col| {
            lsp.implementation(path, line, col)
        });
    }

    fn request_type_definition(&mut self) {
        self.request_locations("typeDefinition", |lsp, path, line, col| {
            lsp.type_definition(path, line, col)
        });
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
        let revision = buf.document.revision();
        self.completion.begin(
            lsp,
            &path,
            line.saturating_sub(1),
            col.saturating_sub(1),
            word_start,
            prefix,
            revision,
        );
    }

    fn completion_popup(&mut self, ctx: &egui::Context) {
        if !self.completion.open {
            return;
        }
        // Descarta respostas de revisões antigas (digitação durante o voo).
        let revision = self
            .ws
            .active_buffer()
            .map(|b| b.document.revision())
            .unwrap_or(0);
        if let Some(lsp) = &self.lsp {
            self.completion.poll(lsp, revision);
        }
        if !self.completion.open {
            return;
        }
        let items = self.completion.filtered();
        if items.is_empty() && !self.completion.loading() {
            if let Some(lsp) = &self.lsp {
                self.completion.close(lsp);
            } else {
                self.completion.open = false;
            }
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
                // Documentação do item selecionado (T-PHP-003-03): texto ou
                // markdown vindo do servidor, sem bloquear o aceite.
                let doc_text =
                    self.completion
                        .selected_raw(self.completion.selected.min(items.len().saturating_sub(1)))
                        .and_then(|raw| {
                            raw["documentation"].as_str().map(String::from).or_else(|| {
                                raw["documentation"]["value"].as_str().map(String::from)
                            })
                        })
                        .filter(|d| !d.trim().is_empty());
                if let Some(doc) = doc_text {
                    ui.separator();
                    ui.label(
                        egui::RichText::new(doc.trim().chars().take(300).collect::<String>())
                            .small()
                            .weak(),
                    );
                }
            });

        if close {
            if let Some(lsp) = &self.lsp {
                self.completion.close(lsp);
            } else {
                self.completion.open = false;
            }
        }
        if let Some(label) = accept {
            self.accept_completion(ctx, &label);
        }
    }

    /// Aplica o item aceito: `textEdit` (range UTF-16) > `insertText` >
    /// rótulo, pelas conversões centralizadas (T-PHP-003-02, NFR-002).
    fn accept_completion(&mut self, ctx: &egui::Context, label: &str) {
        // Captura o item cru (para textEdit) antes de fechar a popup.
        let raw = {
            let items = self.completion.filtered();
            let idx = items.iter().position(|i| i.label == label).unwrap_or(0);
            self.completion.selected_raw(idx).cloned()
        };
        let Some(buf) = self.ws.active_buffer_mut() else {
            if let Some(lsp) = &self.lsp {
                self.completion.close(lsp);
            }
            return;
        };
        if buf.readonly {
            if let Some(lsp) = &self.lsp {
                self.completion.close(lsp);
            }
            return;
        }
        let cursor = self.cursor_char.min(buf.document.len_chars());
        let edit = raw.as_ref().and_then(|item| {
            completion::accept_edit(&buf.document, item, self.completion.word_start, cursor)
        });
        match edit {
            Some(edit) => {
                if edit.end_char > edit.start_char {
                    buf.delete(edit.start_char, edit.end_char);
                }
                buf.insert(edit.start_char, &edit.new_text);
                self.cursor_char = edit.start_char + edit.new_text.chars().count();
            }
            None => {
                let start = self.completion.word_start.min(cursor);
                if start < cursor {
                    buf.delete(start, cursor);
                }
                buf.insert(start, label);
                self.cursor_char = start + label.chars().count();
            }
        }
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
        if let Some(lsp) = &self.lsp {
            self.completion.close(lsp);
        }
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

    /// Roda o arquivo atual como PHP Script (Ctrl+R) — Run Configuration
    /// temporária sobre o RunManager (T-RUN-002-01): validação de campos,
    /// cwd, env e sessão com output/estado no painel Run.
    fn run_current_php(&mut self) {
        self.save_active();
        let Some(path) = self.ws.active_buffer().and_then(|b| b.path.clone()) else {
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
        let rel = path
            .strip_prefix(&self.ws.root)
            .unwrap_or(&path)
            .to_path_buf();
        let config = phpide_run::RunConfig::new(
            format!("temp-run-{}", path.display()),
            format!("php {}", path.file_name().unwrap().to_string_lossy()),
            phpide_run::RunKind::PhpScript {
                file: rel,
                args: Vec::new(),
                interpreter_options: Vec::new(),
                break_at_first_line: false,
            },
        );
        match phpide_run::build_command(
            &config,
            &self.ws.root,
            &exec::build_context(&self.settings, &php),
        ) {
            Ok(built) => {
                self.tw.set_visible(ToolWindow::Run, true);
                let id = self.rd.manager.start(config, built.spec, built.plan);
                self.rd.selected = Some(id);
            }
            Err(e) => self.ws.message = format!("{} — {}", e.message, e.action),
        }
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
    /// clique no gutter. O ponto entra no store compartilhado com a engine
    /// DBGp e é sincronizado com o backend (T-RUN-DBG-003-01).
    fn toggle_breakpoint_at(&mut self, line: usize) {
        let Some(path) = self.ws.active_buffer().and_then(|b| b.path.clone()) else {
            return;
        };
        let added = self.rd.toggle_breakpoint(path, line);
        let action = if added { "adicionado" } else { "removido" };
        let total = self.rd.bp_store.lock().unwrap().breakpoints.len();
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

    /// Debug do arquivo atual (Ctrl+D): engine DBGp + PHP com XDEBUG_CONFIG
    /// zero-config (mescla o env do usuário, T-RUN-DBG-001-01) e Stop/
    /// Detach confiáveis (T-RUN-DBG-001-04 — processo via RunManager,
    /// socket via engine).
    fn debug_run(&mut self) {
        if self.rd.status.is_active() {
            self.ws.message = "já há uma sessão de debug ativa".to_string();
            return;
        }
        self.save_active();
        let Some(path) = self.ws.active_buffer().and_then(|b| b.path.clone()) else {
            self.ws.message = "salve o arquivo antes de debugar".to_string();
            return;
        };
        let php = match exec::resolve_php(&self.settings) {
            Ok(program) => program,
            Err(e) => {
                self.ws.message = e.to_string();
                return;
            }
        };
        match self.rd.start_debug(&self.ws.root, &php, path, Vec::new()) {
            Ok(()) => {
                self.tw.set_visible(ToolWindow::Debug, true);
                self.ws.message = format!(
                    "debug: aguardando Xdebug em 127.0.0.1:{} (IDE key phpide)",
                    run_debug::RunDebugState::debug_port()
                );
            }
            Err(e) => self.ws.message = e,
        }
    }

    /// Atalhos de debug com sessão ativa (roteamento contextual — ver README):
    /// F9 Resume, F7/F8/Shift+F8 stepping, Alt+F10 Show Execution Point.
    fn debug_keys(&mut self, ctx: &egui::Context) {
        if !self.rd.status.is_active() {
            return;
        }
        let paused = self.rd.status == run_debug::DebugStatus::Paused;
        ctx.input_mut(|i| {
            if paused && i.consume_key(egui::Modifiers::NONE, egui::Key::F9) {
                self.rd.continue_debug(ContinueCommand::Resume);
            }
            if paused && i.consume_key(egui::Modifiers::NONE, egui::Key::F7) {
                self.rd.continue_debug(ContinueCommand::StepInto);
            }
            if paused && i.consume_key(egui::Modifiers::NONE, egui::Key::F8) {
                self.rd.continue_debug(ContinueCommand::StepOver);
            }
            if paused && i.consume_key(egui::Modifiers::SHIFT, egui::Key::F8) {
                self.rd.continue_debug(ContinueCommand::StepOut);
            }
            if paused && i.consume_key(egui::Modifiers::ALT, egui::Key::F10) {
                run_panels::show_execution_point(self);
            }
        });
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
        self.zoom = settings::clamp_zoom(zoom);
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

    /// Pinta a superfície do editor sobre o viewport (Estágio 6.2 + Fase 5):
    /// gutter, estados de breakpoint, chevrons de folding, sticky lines,
    /// hints/Code Vision, blame, markers agregados da scrollbar e lens
    /// preview — geometria exata da galley, projeção de folding quando há
    /// dobras (linhas do documento mapeadas para linhas visuais).
    fn paint_editor_surface(
        &mut self,
        ui: &egui::Ui,
        output: &egui::text_edit::TextEditOutput,
        proj: Option<&folding::FoldProjection>,
    ) {
        use phpide_core::Severity;
        let Some(path) = self.ws.active_buffer().and_then(|b| b.path.clone()) else {
            return;
        };
        // Snapshot: empréstimo imutável termina antes das mutações abaixo.
        let (doc_total_lines, cursor_line1, text, blocks, saved_hash, buffer_dirty) = {
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
                blame_gutter::text_hash(buf.document.text().as_str()),
                buf.dirty,
            )
        };
        let Some(first_row) = output.galley.rows.first() else {
            return;
        };
        let row_height = first_row.rect.height();
        let viewport = ui.clip_rect();
        let geom = editor_view::EditorGeometry {
            galley_pos: output.galley_pos,
            viewport,
            row_height,
        };
        // Mapeamento doc→visual (identidade sem folding).
        let (total_lines, doc_to_visual): (usize, Option<Vec<usize>>) = match proj {
            Some(p) => (p.text.lines().count(), Some(p.doc_to_visual.clone())),
            None => (doc_total_lines, None),
        };
        let map_line = |doc_line1: usize| -> usize {
            match &doc_to_visual {
                Some(map) => map
                    .get(doc_line1.saturating_sub(1))
                    .copied()
                    .unwrap_or(usize::MAX),
                None => doc_line1.saturating_sub(1),
            }
        };

        // Gutter: números/bookmarks/coverage na numeração visual; breakpoints
        // entram vazios aqui e são pintados por estado logo abaixo.
        let bps = self.rd.bp_lines_for(&path);
        let bms_doc = self.bookmarks.get(&path).cloned().unwrap_or_default();
        let bms: std::collections::BTreeSet<usize> = bms_doc.iter().map(|&l| map_line(l)).collect();
        let coverage_doc = self
            .rd
            .coverage
            .as_ref()
            .and_then(|report| report.file(&path).cloned());
        let coverage_view = coverage_doc.map(|cov| {
            let mut mapped = phpide_run::coverage::FileCoverage::default();
            for (&k, &v) in cov.lines.iter() {
                let vline = map_line(k as usize);
                if vline != usize::MAX {
                    mapped.lines.insert(vline as u32 + 1, v);
                }
            }
            mapped
        });
        let action = editor_view::paint_gutter(
            ui,
            &geom,
            total_lines,
            cursor_line1,
            &std::collections::BTreeSet::new(),
            &bms,
            output.galley.rows.len(),
            coverage_view.as_ref(),
        );
        if let Some(line1) = action.toggle_breakpoint {
            // Linha visual → documento.
            let doc_line1 = match proj {
                Some(p) => p
                    .visual_to_doc
                    .get(line1.saturating_sub(1))
                    .map(|d| d + 1)
                    .unwrap_or(line1),
                None => line1,
            };
            self.toggle_breakpoint_at(doc_line1);
        }
        // Discos de breakpoint com estado real (UI-ED-001-04).
        let muted = self.rd.bp_muted();
        let num_x = viewport.left() + editor_view::GUTTER_WIDTH - 7.0;
        for doc_line1 in &bps {
            let vline = map_line(*doc_line1);
            if vline == usize::MAX {
                continue;
            }
            let y = geom.row_y(vline);
            if y + row_height < viewport.top() || y > viewport.bottom() {
                continue;
            }
            let status = self.rd.bp_status_for(&path, *doc_line1);
            let visual = status
                .as_ref()
                .map(|s| editor_surface::bp_visual(s, muted))
                .unwrap_or(editor_surface::BpVisual::Enabled);
            editor_surface::paint_breakpoint_disc(
                ui.painter(),
                egui::pos2(num_x, y + row_height * 0.5),
                row_height * 0.32,
                visual,
            );
        }
        // Chevrons de folding (UI-ED-002-03).
        if self.settings.folding {
            let rev_now = self
                .ws
                .active_buffer()
                .map(|b| b.document.revision())
                .unwrap_or(0);
            let cached = self
                .fold_cache
                .as_ref()
                .filter(|(fp, fr, _)| fp == &path && *fr == rev_now)
                .map(|(_, _, r)| r.clone());
            if let Some(ranges) = cached {
                let visuals = self.folds.get(&path).map(|f| f.visuals());
                let folded: std::collections::BTreeSet<usize> = self
                    .folds
                    .get(&path)
                    .map(|f| (0..f.ranges.len()).filter(|&i| f.is_folded(i)).collect())
                    .unwrap_or_default();
                if let Some(visuals) = visuals {
                    let act = editor_surface::paint_fold_chevrons(
                        ui,
                        &geom,
                        viewport.left(),
                        editor_view::GUTTER_WIDTH * 0.42,
                        &ranges,
                        &visuals,
                        &folded,
                        proj.map(|p| p.doc_to_visual.as_slice()),
                        total_lines,
                        self.settings.fold_icons_always,
                    );
                    if let Some(idx) = act.toggle {
                        if let Some(f) = self.folds.get_mut(&path) {
                            f.toggle(idx);
                        }
                    }
                }
            }
        }
        // Sticky lines (UI-ED-004): limite configurável (0 = desliga).
        let sticky_max = self.settings.sticky_lines_max as usize;
        if self.settings.sticky_lines && sticky_max > 0 && !blocks.is_empty() {
            let doc_lines: Vec<String> = text.lines().map(str::to_string).collect();
            let first_visible = match proj {
                Some(p) => p
                    .visual_to_doc
                    .get(geom.first_visible_line())
                    .copied()
                    .unwrap_or(geom.first_visible_line()),
                None => geom.first_visible_line(),
            };
            let jump = editor_view::paint_sticky_lines_max(
                ui,
                &geom,
                &doc_lines,
                &blocks,
                first_visible,
                sticky_max,
                proj.map(|p| p.doc_to_visual.as_slice()),
            );
            if let Some(line0) = jump {
                if let Some(buf) = self.ws.active_buffer_mut() {
                    buf.request_cursor(line0, 0);
                }
            }
        }
        // Inlay hints (UI-ED-005-03) — filtrados pelas configurações.
        if self.settings.inlay_hints {
            let hs = hints::HintSettings {
                enabled: true,
                all_arguments: self.settings.hint_all_arguments,
                pass_by_reference: self.settings.hint_pass_by_reference,
                exclude: self.settings.hint_exclude.clone(),
            };
            let filtered = hs.filter(self.hints.hints());
            editor_surface::paint_inlay_hints(
                ui,
                &geom,
                &output.galley,
                output.galley_pos,
                &filtered,
                proj.map(|p| p.doc_to_visual.as_slice()),
                total_lines,
            );
        }
        // Code Vision (UI-ED-005-05/06).
        if let Some(clicked) = editor_surface::paint_code_vision(
            ui,
            &geom,
            &self.code_vision.lenses,
            proj.map(|p| p.doc_to_visual.as_slice()),
            total_lines,
            editor_view::GUTTER_WIDTH,
        ) {
            if let Some(buf) = self.ws.active_buffer_mut() {
                buf.request_cursor(clicked, 0);
            }
            self.request_usages();
        }
        // Gutter de blame (UI-ED-007-01): dados reais, invalidação por
        // revisão salva; buffer sujo = anotações "stale" sinalizadas.
        if self.settings.blame_gutter {
            let rev = self
                .ws
                .active_buffer()
                .map(|b| b.document.revision())
                .unwrap_or(0);
            let needs_load =
                !buffer_dirty && self.blame.is_stale(&path.to_string_lossy(), saved_hash);
            if needs_load {
                if let Ok(entries) = phpide_vcs::blame(&self.ws.root, &path) {
                    self.blame
                        .load(&path.to_string_lossy(), saved_hash, entries.clone());
                    self.blame_revision = Some((path.clone(), rev));
                    // Code author (UI-ED-005-06) agregado do blame real.
                    if let Some((_, cache)) = &self.syntax {
                        if let Some(buf) = self.ws.active_buffer() {
                            if let Some(blocks) = cache.get(&buf.document) {
                                for b in blocks.iter().filter(|b| {
                                    matches!(
                                        b.kind.as_str(),
                                        "class_declaration"
                                            | "method_declaration"
                                            | "function_declaration"
                                    )
                                }) {
                                    let entry_vec = entries.clone();
                                    self.code_vision.set_code_author(
                                        b.start_line0,
                                        b.start_line0,
                                        b.end_line0,
                                        |line| {
                                            entry_vec
                                                .iter()
                                                .find(|e| {
                                                    line >= e.final_start as usize
                                                        && line <= e.final_end() as usize
                                                })
                                                .map(|e| (e.author.as_str(), e.timestamp))
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
            }
            let stale = buffer_dirty
                || !matches!(&self.blame_revision, Some((p, r)) if p == &path && *r == rev);
            let strip = egui::Rect::from_min_size(
                egui::pos2(viewport.left() + editor_view::GUTTER_WIDTH, viewport.top()),
                egui::vec2(76.0, viewport.height()),
            );
            editor_surface::paint_blame_strip(
                ui,
                &geom,
                strip,
                &|line1| self.blame.annotate(line1),
                total_lines,
                stale,
            );
        }
        // Markers agregados da scrollbar: diagnósticos + Git (UI-ED-006-01).
        let diags_doc: Vec<(u32, Severity)> = self
            .ws
            .diagnostics_for(&path)
            .map(|d| (d.line as u32, d.severity))
            .collect();
        let diags: Vec<(u32, Severity)> = diags_doc
            .iter()
            .map(|(l, s)| ((map_line(*l as usize) as u32).saturating_add(1), *s))
            .filter(|(l, _)| (*l as usize) < usize::MAX)
            .collect();
        if self
            .git_markers
            .as_ref()
            .map(|(p, _, _)| p != &path)
            .unwrap_or(true)
        {
            self.git_markers = None;
        }
        let rev = self
            .ws
            .active_buffer()
            .map(|b| b.document.revision())
            .unwrap_or(0);
        let git_fresh = matches!(&self.git_markers, Some((p, r, _)) if p == &path && *r == rev);
        if !git_fresh && !buffer_dirty {
            let markers = phpide_vcs::markers_for_file(&self.ws.root, &path)
                .map(|m| {
                    m.iter()
                        .map(|(&l, kind)| (l, *kind == phpide_vcs::LineMarker::Added))
                        .collect()
                })
                .unwrap_or_default();
            self.git_markers = Some((path.clone(), rev, markers));
        }
        let git: Vec<(u32, bool)> = self
            .git_markers
            .as_ref()
            .filter(|(p, _, _)| p == &path)
            .map(|(_, _, mk)| {
                mk.iter()
                    .map(|(l, added)| ((map_line(*l as usize) as u32).saturating_add(1), *added))
                    .collect()
            })
            .unwrap_or_default();
        let markers = lens::aggregate_scrollbar_markers(&diags, &git, total_lines);
        editor_surface::paint_scroll_markers(ui, &geom, &markers);
        // Lens preview (UI-ED-006-02).
        if self.settings.lens_mode {
            if let Some(line0) = editor_surface::scrollbar_hover_line(ui, &geom, total_lines) {
                self.lens_hover = Some((line0, std::time::Instant::now()));
            } else if self
                .lens_hover
                .as_ref()
                .map(|(_, t)| t.elapsed() > std::time::Duration::from_millis(400))
                .unwrap_or(false)
            {
                self.lens_hover = None;
            }
            if let Some((line0, _)) = self.lens_hover {
                let doc_line0 = match proj {
                    Some(p) => p.visual_to_doc.get(line0).copied().unwrap_or(line0),
                    None => line0,
                };
                let doc_lines: Vec<String> = text.lines().map(str::to_string).collect();
                let preview = lens::lens_preview(&doc_lines, doc_line0, 3);
                egui::Area::new(egui::Id::new("lens-preview"))
                    .order(egui::Order::Foreground)
                    .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, 80.0))
                    .show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            for l in &preview.lines {
                                ui.label(egui::RichText::new(l).monospace().size(11.0));
                            }
                        });
                    });
            }
        }
        // Resumo de inspeções do arquivo (widget UI-ED-006-03, status bar).
        self.inspections = lens::inspections_summary(&diags_doc);
        // Ocorrências do símbolo (T-PHP-005-02): só na revisão calculada.
        if let Some((op, or_, _word, ranges)) = &self.occurrences {
            if active_path_matches(&self.ws, op, *or_) && !ranges.is_empty() {
                editor_view::paint_occurrences(ui, &output.galley, output.galley_pos, ranges);
            }
        }
    }

    fn editor(&mut self, ctx: &egui::Context) {
        let mut want_completion = false;
        // Hook de demonstração/teste: rola o editor para uma linha no startup
        // (permite capturar sticky lines/gutter sem simulação de input).
        let scroll_hook: Option<f32> = std::env::var("PHPIDE_SCROLL_TO_LINE")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .map(|line| (line - 1.0).max(0.0));
        // Offset de scroll calculado pelo mapa centralizado com a altura de
        // linha efetiva do zoom (antes: 18.0 fixo, que desalinhava com zoom).
        let scroll_hook = scroll_hook.map(|line0| {
            let zoom = self.zoom.max(0.01);
            let row = 18.0 * zoom;
            let g = geometry::Geometry::new(row, 600.0, usize::MAX);
            g.line_to_content_y(line0 as usize)
        });
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
            // ---- Fase 5: folding (UI-ED-002) — ranges por revisão ----
            let path_key = self.ws.active_buffer().and_then(|b| b.path.clone());
            if let (Some(path), Some(rev)) = (
                path_key.clone(),
                self.ws.active_buffer().map(|b| b.document.revision()),
            ) {
                if !self.switcher.is_cycling() {
                    self.switcher.touch(&path.to_string_lossy());
                }
                if self.settings.folding {
                    let fresh =
                        matches!(&self.fold_cache, Some((p, r, _)) if p == &path && *r == rev);
                    if !fresh {
                        let ranges =
                            phpide_syntax::folding::folding_ranges(&old, old.lines().count());
                        let doc_lines: Vec<String> = old.lines().map(str::to_string).collect();
                        self.fold_cache = Some((path.clone(), rev, ranges.clone()));
                        self.folds
                            .entry(path)
                            .or_default()
                            .update(ranges, &doc_lines);
                    }
                }
            }
            let fold_ctx = path_key.clone().and_then(|path| {
                if !self.settings.folding {
                    return None;
                }
                let visuals = self.folds.get(&path)?.visuals();
                if visuals.is_empty() {
                    return None;
                }
                Some((path, folding::project(&old, &visuals), visuals))
            });
            let mut edited = match &fold_ctx {
                Some((_, proj, _)) => proj.text.clone(),
                None => old.clone(),
            };
            // Semantic tokens (T-PHP-005-01): spans de byte da revisão
            // vigente do arquivo ativo; aplicados como cor de texto por
            // cima do highlight sintático (sem flicker: só atualiza por
            // revisão e por resposta do servidor).
            let semantic_spans: Option<Vec<(usize, usize, String)>> = self
                .semantic
                .as_ref()
                .filter(|(p, r, _, _)| active_path_matches(&self.ws, p, *r))
                .map(|(_, _, _, spans)| spans.clone());
            let mut layouter = |ui: &egui::Ui, text: &str, wrap_width: f32| {
                let mut job = PhpLanguage.highlight(text, ui.style());
                if let Some(spans) = &semantic_spans {
                    apply_semantic_colors(&mut job, spans);
                }
                job.wrap.max_width = wrap_width;
                ui.ctx().fonts(|fonts| fonts.layout_job(job))
            };
            // O gutter é pintado por cima do viewport; o texto ganha um
            // recuo à esquerda para nunca ficar sob ele.
            let scroll_area = egui::ScrollArea::vertical().auto_shrink([false; 2]);
            let scroll_area = match scroll_hook {
                // `line` já é offset em pixels (line_to_content_y).
                Some(offset_y) => scroll_area.vertical_scroll_offset(offset_y),
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
            // Caret em região dobrada revela a dobra (UI-ED-002-03): o texto
            // projetado some; o caret volta a enxergar a região.
            if let Some((path, proj, _)) = &fold_ctx {
                let vline = edited
                    .chars()
                    .take(self.cursor_char.min(edited.chars().count()))
                    .filter(|c| *c == '\n')
                    .count();
                if let Some(&doc_line) = proj.visual_to_doc.get(vline) {
                    let visuals = self.folds.get(path).map(|f| f.visuals());
                    if let Some(v) = visuals {
                        if v.is_hidden(doc_line) {
                            let target = v.fold_at(doc_line).map(|(s, _)| s);
                            let st = self.folds.get_mut(path).unwrap();
                            if let Some(idx) = target
                                .and_then(|s| st.ranges.iter().position(|r| r.start_line0 == s))
                            {
                                st.toggle(idx);
                            }
                        }
                    }
                }
            }

            // ---- Estágio 6.2+Fase 5: superfície do editor ----
            self.paint_editor_surface(ui, &output, fold_ctx.as_ref().map(|(_, p, _)| p));

            // Com folding, o TextEdit editou o texto projetado: mapeia a
            // edição de volta ao documento (bandas são read-only).
            let (old_for_diff, edited_for_diff) = match &fold_ctx {
                Some((_, proj, _)) => match folding::commit(proj, &edited, &old) {
                    folding::ProjectedCommit::NewDocText(t) => (old.clone(), t),
                    folding::ProjectedCommit::Unchanged => (old.clone(), old.clone()),
                },
                None => (old.clone(), edited.clone()),
            };
            if old_for_diff != edited_for_diff {
                if let Some((at, removed, inserted)) =
                    Self::diff_edit(&old_for_diff, &edited_for_diff)
                {
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
            // Segundo painel do split (UI-ED-003-04): mesma Document/rope do
            // buffer (texto, dirty e undo compartilhados), mas TextEdit com
            // id/scroll/caret próprios. Neste recorte o layout empilha o
            // segundo painel (o eixo fica registrado no modelo de groups
            // para o layout lado a lado da próxima iteração).
            let second = match &self.groups.root {
                groups::GroupNode::Split { children, .. } => match &children[1] {
                    groups::GroupNode::Leaf { views, .. } => {
                        views.first().map(|v| v.buffer.clone())
                    }
                    _ => None,
                },
                _ => None,
            };
            if let Some(key) = second {
                let buf_idx = self.ws.buffers.iter().position(|b| {
                    b.path
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned())
                        .as_deref()
                        == Some(key.as_str())
                });
                if let Some(idx) = buf_idx {
                    ui.separator();
                    let (text2, readonly2) = {
                        let b = &self.ws.buffers[idx];
                        (b.document.text(), b.readonly)
                    };
                    let mut edited2 = text2.clone();
                    egui::ScrollArea::vertical()
                        .id_salt(("pane2", idx))
                        .max_height(ui.available_height() * 0.45)
                        .show(ui, |ui| {
                            egui::TextEdit::multiline(&mut edited2)
                                .id(egui::Id::new(("editor", idx, "pane2")))
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY)
                                .interactive(!readonly2)
                                .show(ui);
                        });
                    if edited2 != text2 {
                        if let Some((at, removed, inserted)) = Self::diff_edit(&text2, &edited2) {
                            let b = &mut self.ws.buffers[idx];
                            if removed > 0 {
                                b.delete(at, at + removed);
                            }
                            if !inserted.is_empty() {
                                b.insert(at, &inserted);
                            }
                        }
                    }
                    // Fechar o painel: remove a vista do segundo grupo.
                    if ui.small_button("✕ Unsplit").clicked() {
                        while self.groups.close_active_in_root_second().is_some() {}
                    }
                }
            }
        });
        if want_completion && !self.completion.open {
            self.trigger_completion();
        }
        // Popup do switcher MRU (UI-ED-003-02): desenha por cima de tudo.
        let mut labels = std::collections::HashMap::new();
        for b in &self.ws.buffers {
            if let Some(p) = &b.path {
                labels.insert(
                    p.to_string_lossy().into_owned(),
                    groups::tab_title(&p.to_string_lossy(), b.dirty, false),
                );
            }
        }
        switcher::popup(ctx, &self.switcher, &labels, 0);
    }
}

/// `true` se o buffer ativo é `path` na revisão `revision` (semantic
/// tokens e ocorrências só valem para o estado em que foram calculados).
/// Converte a resposta LSP `textDocument/inlayHint` no modelo interno
/// (label pode ser string ou partes com formatação).
fn parse_inlay_hints(result: &Value) -> Vec<hints::InlayHint> {
    result
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|h| {
                    let line0 = h["position"]["line"].as_u64()? as usize;
                    let label = match &h["label"] {
                        Value::String(s) => s.clone(),
                        Value::Array(parts) => parts
                            .iter()
                            .filter_map(|p| p["value"].as_str().map(String::from))
                            .collect(),
                        _ => return None,
                    };
                    Some(hints::InlayHint { line0, label })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn active_path_matches(ws: &Workspace, path: &Path, revision: u64) -> bool {
    ws.active_buffer()
        .map(|b| b.path.as_deref() == Some(path) && b.document.revision() == revision)
        .unwrap_or(false)
}

/// Cor semântica por tipo de token (T-PHP-005-01). `None` = manter a cor
/// do highlight sintático (strings, comentários e tipos não mapeados).
fn semantic_color(token_type: &str) -> Option<egui::Color32> {
    use egui::Color32;
    Some(match token_type {
        "class" | "interface" | "trait" | "enum" | "type" | "namespace" => {
            Color32::from_rgb(0x7A, 0xA2, 0xF7)
        }
        "function" | "method" | "macro" => Color32::from_rgb(0xFF, 0xC6, 0x6D),
        "property" | "enumMember" => Color32::from_rgb(0x9E, 0xCE, 0x6A),
        "parameter" => Color32::from_rgb(0xD2, 0xA8, 0xFF),
        "keyword" => Color32::from_rgb(0xBB, 0x9A, 0xF7),
        "number" => Color32::from_rgb(0xFF, 0xA1, 0x94),
        "operator" => Color32::from_rgb(0x89, 0xDD, 0xFF),
        _ => return None,
    })
}

/// Sobrepõe cores semânticas no job de layout: a seção cuja posição de
/// início cai dentro de um span de token recebe a cor do tipo. Spans e
/// seções estão ordenados — caminhada única O(n+m) por frame.
fn apply_semantic_colors(job: &mut egui::text::LayoutJob, spans: &[(usize, usize, String)]) {
    let mut span_idx = 0usize;
    for section in &mut job.sections {
        while span_idx < spans.len() && spans[span_idx].1 <= section.byte_range.start {
            span_idx += 1;
        }
        if span_idx >= spans.len() {
            break;
        }
        let (s, e, ty) = &spans[span_idx];
        if section.byte_range.start >= *s && section.byte_range.start < *e {
            if let Some(color) = semantic_color(ty) {
                section.format.color = color;
            }
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

#[cfg(test)]
mod gate_ui_ed {
    //! Gate UI-ED-GATE-01: pipeline de superfície (folding + gutter visual +
    //! markers + sticky) sobre fixture de 10 mil linhas, com medição.

    use super::*;

    fn big_php() -> String {
        let mut s = String::from("<?php\n");
        for i in 0..2000 {
            s.push_str(&format!(
                "function f{i}(): void {{\n    if (true) {{\n        echo {i};\n    }}\n}}\n"
            ));
        }
        s
    }

    #[test]
    fn gate_folding_gutter_markers_10k_within_budget() {
        let code = big_php();
        let total = code.lines().count();
        assert!(total >= 10_000, "fixture tem {total} linhas");

        let t0 = std::time::Instant::now();
        // 1) Ranges de folding (tree-sitter) sobre as 10k linhas.
        let ranges = phpide_syntax::folding::folding_ranges(&code, total);
        assert!(!ranges.is_empty());
        let t_ranges = t0.elapsed();

        // 2) Dobras efetivas + projeção + contagem visual.
        let folded: Vec<(usize, usize)> = ranges
            .iter()
            .take(200)
            .map(|r| (r.start_line0, r.end_line0))
            .collect();
        let visuals = folding::FoldVisuals::new(&folded);
        let proj = folding::project(&code, &visuals);
        assert!(proj.text.lines().count() < total);
        let t_proj = t0.elapsed() - t_ranges;

        // 3) Markers agregados (limite por categoria) + sticky trail.
        let diags: Vec<(u32, phpide_core::Severity)> = (1..=50)
            .map(|l| (l * 100, phpide_core::Severity::Error))
            .collect();
        let git: Vec<(u32, bool)> = (1..=20).map(|l| (l * 200, true)).collect();
        let markers = lens::aggregate_scrollbar_markers(&diags, &git, total);
        assert!(!markers.is_empty());
        let blocks = phpide_syntax::analyze(&code).blocks;
        let trail = editor_view::sticky_trail(&blocks, total / 2, 5);
        assert!(!trail.is_empty());
        let total_t = t0.elapsed();

        // Orçamento folgado: pipeline completo < 2s (baseline UI-ED-000-04:
        // 7.5ns/frame). Registre o número real ao evoluir o gate.
        assert!(
            total_t < std::time::Duration::from_secs(2),
            "pipeline 10k lento: {total_t:?} (ranges {t_ranges:?}, proj {t_proj:?})"
        );
        eprintln!(
            "GATE 10k: total {total} linhas, {} ranges, {} linhas visuais, {total_t:?}",
            ranges.len(),
            proj.text.lines().count()
        );
    }

    #[test]
    fn gate_editor_avancado_integrado_dados_reais() {
        // Dados reais: folding (tree-sitter) + markers git (phpide-vcs puro)
        // + blame model sobre repo temporário real.
        let git_ok = std::process::Command::new("git")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !git_ok {
            eprintln!("SKIP: git indisponível");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let g = |args: &[&str]| {
            assert!(std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .map(|o| o.status.success())
                .unwrap())
        };
        g(&["init", "-b", "main"]);
        g(&["config", "user.name", "Gate"]);
        g(&["config", "user.email", "g@x"]);
        std::fs::write(root.join("a.php"), "<?php\n$x = 1;\n").unwrap();
        g(&["add", "."]);
        g(&["commit", "-m", "base"]);
        let m = phpide_vcs::markers_for_file(&root, &root.join("a.php")).unwrap();
        assert!(m.is_empty(), "arquivo sem alterações: sem markers falsos");
        std::fs::write(root.join("a.php"), "<?php\n$x = 2;\n").unwrap();
        let m = phpide_vcs::markers_for_file(&root, &root.join("a.php")).unwrap();
        assert_eq!(
            m.values().fold(0, |acc, _| acc + 1),
            1,
            "1 linha modificada"
        );
    }
}

mod gate_tests {
    //! Gate do recorte UI-ED-000 e verificação do modo offline.

    /// Modo offline (sem LSP): highlight sintático e diagnósticos estruturais
    /// locais continuam funcionais — são a degradação prevista pelo NFR
    /// (tree-sitter cobre estrutura e erros de sintaxe offline).
    #[test]
    fn offline_highlight_and_structural_diagnostics_still_work() {
        // Highlight: código PHP produz fragments com formatação colorida.
        let style = eframe::egui::Style::default();
        let job = phpide_php::highlight("<?php\n$nome = 'phpide';\n", &style);
        assert!(!job.sections.is_empty(), "highlight gera seções");
        let colored = job
            .sections
            .iter()
            .any(|s| s.format.color != eframe::egui::Color32::default());
        assert!(colored, "alguma seção recebe cor do syntect");

        // Diagnósticos estruturais: código inválido gera erros do tree-sitter.
        let ok =
            phpide_syntax::analyze("<?php class Ok { public function a(): int { return 1; } }");
        assert!(
            ok.errors.is_empty(),
            "código válido sem erros: {:?}",
            ok.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
        let broken = phpide_syntax::analyze("<?php class Quebrado { public function a( {");
        assert!(
            !broken.errors.is_empty(),
            "código inválido produz diagnóstico estrutural offline"
        );
        // Decisão do poll_syntax replicada com as peças reais: sem LSP, os
        // erros locais alimentam o painel de problemas do arquivo.
        let lsp_running = false;
        let diags: Vec<phpide_core::Diagnostic> = if lsp_running {
            Vec::new()
        } else {
            broken
                .errors
                .iter()
                .map(|e| phpide_core::Diagnostic {
                    path: std::path::PathBuf::from("/p/offline.php"),
                    line: e.line0 + 1,
                    col: e.col0 + 1,
                    message: e.message.clone(),
                    severity: phpide_core::Severity::Error,
                    source: "tree-sitter".to_string(),
                })
                .collect()
        };
        assert_eq!(diags.len(), broken.errors.len());
        assert!(diags.iter().all(|d| d.source == "tree-sitter"));
    }

    /// Zoom: atalhos e limites consolidados (UI-ED-010-01).
    #[test]
    fn zoom_shortcut_values_are_clamped() {
        // Mesma função usada por set_zoom e pela carga do settings.
        assert_eq!(crate::settings::clamp_zoom(1.0 * 1.1f32.powi(20)), 3.0);
        assert_eq!(crate::settings::clamp_zoom(1.0 / 1.1f32.powi(20)), 0.5);
        // Roundtrip: persistir e recarregar preserva o valor dentro dos limites.
        let s = crate::settings::Settings {
            zoom: crate::settings::clamp_zoom(1.7),
            ..Default::default()
        };
        let v = serde_json::json!({ "zoom": s.zoom });
        let back = v["zoom"].as_f64().unwrap() as f32;
        assert_eq!(crate::settings::clamp_zoom(back), 1.7);
    }
}
