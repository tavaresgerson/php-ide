//! Command palette / Search Everywhere (Estágio 2 → 6): `Ctrl+P` ou `Shift×2`
//! abrem tudo, `Ctrl+E` Recent Files, `Ctrl+Shift+A` Find Action (modo ações).
//! Fuzzy match por subsequência; prefixo `>` força o modo ações (convenção
//! JetBrains dentro do Search Everywhere).

use std::path::PathBuf;

use eframe::egui;
use phpide_core::Workspace;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteMode {
    /// Search Everywhere: arquivos + ações + extras.
    All,
    /// Só arquivos do projeto.
    Files,
    /// Find Action: só comandos da IDE + extras.
    Actions,
    /// Recent Files (MRU).
    Recent,
}

impl PaletteMode {
    pub fn title(self) -> &'static str {
        match self {
            PaletteMode::All => "Search Everywhere",
            PaletteMode::Files => "Go to File",
            PaletteMode::Actions => "Find Action",
            PaletteMode::Recent => "Recent Files",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteCommand {
    Save,
    NewFile,
    CloseTab,
    CloseOthers,
    PinTab,
    RefreshTree,
    ToggleTheme,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ToggleProject,
    ToggleFind,
    ToggleRun,
    ToggleDebug,
    ToggleProblems,
    ToggleStructure,
    HideAllToolWindows,
    NavigateBack,
    NavigateForward,
    NextProblem,
    PrevProblem,
    OpenWelcome,
    OpenScheme,
    GotoDefinition,
    RunCurrent,
    DebugCurrent,
    GotoLine,
}

pub enum PaletteAction {
    Open(PathBuf),
    Run(PaletteCommand),
    /// Comando dinâmico (composer script, phpunit, plugin externo).
    Plugin(String),
}

pub struct PaletteState {
    pub open: bool,
    pub mode: PaletteMode,
    pub query: String,
    pub selected: usize,
    pub just_opened: bool,
}

impl PaletteState {
    pub fn new() -> Self {
        PaletteState {
            open: false,
            mode: PaletteMode::All,
            query: String::new(),
            selected: 0,
            just_opened: false,
        }
    }

    pub fn open_with(&mut self, mode: PaletteMode) {
        self.open = true;
        self.mode = mode;
        self.query.clear();
        self.selected = 0;
        self.just_opened = true;
    }
}

struct Entry {
    label: String,
    detail: String,
    hint: String,
    action: PaletteActionKind,
}

enum PaletteActionKind {
    Open(PathBuf),
    Run(PaletteCommand),
    Plugin(String),
}

/// Comandos estáticos da IDE, com prefixo de categoria estilo JetBrains.
const COMMANDS: &[(&str, PaletteCommand)] = &[
    ("Arquivo: Salvar", PaletteCommand::Save),
    ("Arquivo: Novo arquivo", PaletteCommand::NewFile),
    ("Arquivo: Fechar aba", PaletteCommand::CloseTab),
    ("Arquivo: Fechar outras abas", PaletteCommand::CloseOthers),
    ("Arquivo: Fixar/despinar aba", PaletteCommand::PinTab),
    ("Arquivo: Recarregar árvore", PaletteCommand::RefreshTree),
    ("Ver: Toggle Theme", PaletteCommand::ToggleTheme),
    ("Ver: Zoom in", PaletteCommand::ZoomIn),
    ("Ver: Zoom out", PaletteCommand::ZoomOut),
    ("Ver: Zoom reset", PaletteCommand::ZoomReset),
    ("Ver: Project Tool Window", PaletteCommand::ToggleProject),
    ("Ver: Find Tool Window", PaletteCommand::ToggleFind),
    ("Ver: Run Tool Window", PaletteCommand::ToggleRun),
    ("Ver: Debug Tool Window", PaletteCommand::ToggleDebug),
    ("Ver: Problems Tool Window", PaletteCommand::ToggleProblems),
    ("Ver: Structure Tool Window", PaletteCommand::ToggleStructure),
    ("Ver: Hide All Tool Windows", PaletteCommand::HideAllToolWindows),
    ("Navegar: Back", PaletteCommand::NavigateBack),
    ("Navegar: Forward", PaletteCommand::NavigateForward),
    ("Navegar: Ir para definição", PaletteCommand::GotoDefinition),
    ("Navegar: Ir para linha", PaletteCommand::GotoLine),
    ("Navegar: Próximo problema", PaletteCommand::NextProblem),
    ("Navegar: Problema anterior", PaletteCommand::PrevProblem),
    ("Executar: Rodar arquivo atual", PaletteCommand::RunCurrent),
    ("Executar: Debug arquivo atual", PaletteCommand::DebugCurrent),
    ("Ajuda: Welcome Screen", PaletteCommand::OpenWelcome),
    ("Ajuda: Quick Switch Scheme", PaletteCommand::OpenScheme),
];

/// Fuzzy match por subsequência: retorna pontuação maior = melhor.
/// Bônus: início da string, após separador, e caracteres consecutivos.
pub fn fuzzy_score(needle: &str, hay: &str) -> Option<i32> {
    let needle: Vec<char> = needle.chars().collect();
    let hay: Vec<char> = hay.chars().collect();
    if needle.is_empty() {
        return Some(0);
    }
    let mut score = 0i32;
    let mut h = 0usize;
    let mut run = 0i32;
    for (i, &n) in needle.iter().enumerate() {
        let n_lower = n.to_ascii_lowercase();
        let mut found = false;
        while h < hay.len() {
            let hc = hay[h].to_ascii_lowercase();
            let boundary = h == 0
                || matches!(hay[h - 1], '/' | '\\' | '_' | '.' | '-')
                || hay[h - 1].is_ascii_uppercase() && hay[h].is_ascii_lowercase();
            if hc == n_lower {
                score += 1;
                if h == 0 || boundary {
                    score += 4;
                }
                if i > 0 {
                    run += 1;
                    score += run * 2;
                } else {
                    run = 0;
                }
                found = true;
                h += 1;
                break;
            }
            run = 0;
            h += 1;
        }
        if !found {
            return None;
        }
    }
    Some(score)
}

fn project_files(ws: &Workspace, query: &str, cap: usize) -> Vec<(i32, PathBuf)> {
    let mut files: Vec<(i32, PathBuf)> = ws
        .files
        .iter()
        .filter(|e| !e.is_dir)
        .filter_map(|e| {
            let rel = e
                .path
                .strip_prefix(&ws.root)
                .unwrap_or(&e.path)
                .to_string_lossy()
                .into_owned();
            fuzzy_score(query, &rel).map(|s| (s, e.path.clone()))
        })
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files.truncate(cap);
    files
}

fn collect_entries(ws: &Workspace, state: &PaletteState, extra: &[(String, String)]) -> Vec<Entry> {
    let mut out = Vec::new();
    // Convenção JetBrains: `>` dentro do Search Everywhere força ações.
    let mut query = state.query.trim_start();
    let mut mode = state.mode;
    if query.starts_with('>') {
        mode = PaletteMode::Actions;
        query = query[1..].trim_start();
    }

    let want_actions = matches!(mode, PaletteMode::All | PaletteMode::Actions);
    let want_files = matches!(mode, PaletteMode::All | PaletteMode::Files);

    if want_actions {
        for (label, cmd) in COMMANDS {
            if query.is_empty() || fuzzy_score(query, label).is_some() {
                out.push(Entry {
                    label: label.to_string(),
                    detail: "comando".to_string(),
                    hint: String::new(),
                    action: PaletteActionKind::Run(*cmd),
                });
            }
        }
        for (label, id) in extra {
            if query.is_empty() || fuzzy_score(query, label).is_some() {
                out.push(Entry {
                    label: label.clone(),
                    detail: "externo".to_string(),
                    hint: String::new(),
                    action: PaletteActionKind::Plugin(id.clone()),
                });
            }
        }
    }

    if want_files {
        match mode {
            PaletteMode::Recent => {
                for path in &ws.recent_files {
                    let rel = path
                        .strip_prefix(&ws.root)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .into_owned();
                    if query.is_empty() || fuzzy_score(query, &rel).is_some() {
                        out.push(Entry {
                            label: path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                            detail: rel,
                            hint: "recente".to_string(),
                            action: PaletteActionKind::Open(path.clone()),
                        });
                    }
                }
            }
            _ => {
                for (_, path) in project_files(ws, query, 20) {
                    out.push(Entry {
                        label: path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        detail: path.display().to_string(),
                        hint: String::new(),
                        action: PaletteActionKind::Open(path),
                    });
                }
            }
        }
    }
    out
}

/// Desenha a palette e devolve a ação escolhida (Enter/clique).
pub fn show(
    ctx: &egui::Context,
    ws: &Workspace,
    state: &mut PaletteState,
    extra: &[(String, String)],
) -> Option<PaletteAction> {
    if !state.open {
        return None;
    }
    let mut action: Option<PaletteAction> = None;
    let mut close = false;

    egui::Window::new("palette")
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .movable(false)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 40.0))
        .frame(
            egui::Frame::window(&ctx.style())
                .shadow(egui::epaint::Shadow::default())
                .inner_margin(egui::Margin::same(8)),
        )
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(state.mode.title())
                    .strong()
                    .small()
                    .weak(),
            );
            let edit = egui::TextEdit::singleline(&mut state.query)
                .hint_text("Digite para buscar ( > = ações )")
                .desired_width(460.0)
                .show(ui);
            if state.just_opened {
                edit.response.request_focus();
                state.just_opened = false;
            }

            let entries = collect_entries(ws, state, extra);
            if state.selected >= entries.len() {
                state.selected = entries.len().saturating_sub(1);
            }

            ctx.input(|i| {
                if i.key_pressed(egui::Key::ArrowDown) && !entries.is_empty() {
                    state.selected = (state.selected + 1).min(entries.len() - 1);
                }
                if i.key_pressed(egui::Key::ArrowUp) {
                    state.selected = state.selected.saturating_sub(1);
                }
                if i.key_pressed(egui::Key::PageDown) && !entries.is_empty() {
                    state.selected = (state.selected + 10).min(entries.len() - 1);
                }
                if i.key_pressed(egui::Key::PageUp) {
                    state.selected = state.selected.saturating_sub(10);
                }
                if i.key_pressed(egui::Key::Escape) {
                    close = true;
                }
            });

            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for (i, entry) in entries.iter().enumerate() {
                        let selected = i == state.selected;
                        let label = if entry.hint.is_empty() {
                            format!("{}   —   {}", entry.label, entry.detail)
                        } else {
                            format!("{}   —   {} [{}]", entry.label, entry.detail, entry.hint)
                        };
                        let row = ui.selectable_label(selected, label);
                        if row.clicked() || (selected && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                            action = Some(match &entry.action {
                                PaletteActionKind::Open(p) => PaletteAction::Open(p.clone()),
                                PaletteActionKind::Run(c) => PaletteAction::Run(*c),
                                PaletteActionKind::Plugin(id) => PaletteAction::Plugin(id.clone()),
                            });
                            close = true;
                        }
                    }
                });
        });

    if close {
        state.open = false;
        state.query.clear();
        state.selected = 0;
    }
    action
}
