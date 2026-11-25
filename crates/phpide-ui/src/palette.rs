//! Search Everywhere (Shift Shift / Ctrl+P), Go to File (Ctrl+Shift+N),
//! Go to Class (Ctrl+N), Go to Symbol, Find Action (Ctrl+Shift+A) and
//! Recent Files (Ctrl+E), one popup with scope tabs (RF-SEL-01..07).
//!
//! Tab and Shift+Tab switch scope. An empty query lists recent files
//! first; `>` forces actions. Actions include Settings pages.

use std::path::{Path, PathBuf};

use eframe::egui;
use phpide_icons::{file_icon, IconId};

use crate::commands::Command;
use crate::settings_dialog::SettingsPage;
use phpide_core::Workspace;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteMode {
    /// Search Everywhere: files, classes, symbols, actions.
    All,
    /// Classes, interfaces, traits and enums (workspace symbols).
    Classes,
    Files,
    /// Every workspace symbol.
    Symbols,
    Actions,
    /// Recent Files (MRU), not one of the tabs.
    Recent,
}

/// Scope tabs, in order.
pub const TABS: [PaletteMode; 5] = [
    PaletteMode::All,
    PaletteMode::Classes,
    PaletteMode::Files,
    PaletteMode::Symbols,
    PaletteMode::Actions,
];

impl PaletteMode {
    pub fn title(self) -> &'static str {
        match self {
            PaletteMode::All => "All",
            PaletteMode::Classes => "Classes",
            PaletteMode::Files => "Files",
            PaletteMode::Symbols => "Symbols",
            PaletteMode::Actions => "Actions",
            PaletteMode::Recent => "Recent Files",
        }
    }

    /// Whether the mode asks the language server for symbols.
    pub fn wants_symbols(self) -> bool {
        matches!(
            self,
            PaletteMode::All | PaletteMode::Classes | PaletteMode::Symbols
        )
    }
}

/// A workspace symbol from the language server.
#[derive(Debug, Clone, PartialEq)]
pub struct SymbolHit {
    pub name: String,
    pub container: String,
    pub path: PathBuf,
    pub line0: usize,
    pub col0: usize,
    /// LSP `SymbolKind`.
    pub kind: u64,
}

impl SymbolHit {
    /// Class, interface, enum or struct (PHP traits arrive as classes or
    /// interfaces, depending on the server).
    pub fn is_class(&self) -> bool {
        matches!(self.kind, 5 | 10 | 11 | 23)
    }

    fn kind_label(&self) -> &'static str {
        match self.kind {
            5 => "class",
            10 => "enum",
            11 => "interface",
            6 => "method",
            12 => "function",
            13 | 14 => "constant",
            7 | 8 => "property",
            _ => "symbol",
        }
    }
}

pub enum PaletteAction {
    Open(PathBuf),
    OpenAt(PathBuf, usize, usize),
    Run(Command),
    /// Comando dinâmico (composer script, phpunit, plugin externo).
    Plugin(String),
    Settings(SettingsPage),
    /// Alt+Enter on an action in Find Action: give it a shortcut.
    AssignShortcut(Command),
}

/// File type groups of the Files filter (RF-SEL-05).
pub const FILE_TYPES: [(&str, &[&str]); 6] = [
    ("PHP", &["php", "phtml", "inc", "phpt"]),
    ("Templates", &["twig", "html", "htm", "tpl", "latte"]),
    (
        "JavaScript and TypeScript",
        &["js", "mjs", "cjs", "ts", "tsx", "jsx", "vue"],
    ),
    ("Styles", &["css", "scss", "sass", "less"]),
    (
        "Configuration",
        &[
            "json", "yml", "yaml", "xml", "neon", "env", "ini", "toml", "dist", "lock",
        ],
    ),
    ("Documentation", &["md", "txt", "rst"]),
];
/// Files of no group above.
pub const OTHER_TYPE: &str = "Other";

/// The type group of a file name.
pub fn file_type(name: &str) -> &'static str {
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    FILE_TYPES
        .iter()
        .find(|(_, exts)| exts.contains(&ext.as_str()))
        .map(|(group, _)| *group)
        .unwrap_or(OTHER_TYPE)
}

/// Files outside the project index: `vendor/`, `node_modules/` and the
/// folders marked Excluded (RF-SEL-05 "include non-project items").
pub fn scan_libraries(
    root: &Path,
    excluded: &[PathBuf],
    cancelled: impl Fn() -> bool,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = vec![root.join("vendor"), root.join("node_modules")];
    stack.extend(excluded.iter().cloned());
    while let Some(dir) = stack.pop() {
        if cancelled() || out.len() >= 200_000 {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            match entry.file_type() {
                Ok(t) if t.is_dir() => stack.push(entry.path()),
                Ok(_) => out.push(entry.path()),
                Err(_) => {}
            }
        }
    }
    out.sort();
    out
}

pub struct PaletteState {
    pub open: bool,
    /// File type groups hidden by the Files filter.
    pub hidden_types: Vec<&'static str>,
    /// Also search `vendor/`, `node_modules/` and excluded folders.
    pub include_non_project: bool,
    /// Those files, once scanned (`None`: not yet).
    pub libraries: Option<std::sync::Arc<Vec<PathBuf>>>,
    pub mode: PaletteMode,
    pub query: String,
    pub selected: usize,
    pub just_opened: bool,
    /// Recent Files shows only edited files (Ctrl+E pressed twice, RF-SEL-07).
    pub edited_only: bool,
}

impl PaletteState {
    pub fn new() -> Self {
        PaletteState {
            open: false,
            hidden_types: Vec::new(),
            include_non_project: false,
            libraries: None,
            mode: PaletteMode::All,
            query: String::new(),
            selected: 0,
            just_opened: false,
            edited_only: false,
        }
    }

    pub fn open_with(&mut self, mode: PaletteMode) {
        if self.open && mode == PaletteMode::Recent && self.mode == PaletteMode::Recent {
            self.edited_only = !self.edited_only;
            self.selected = 0;
            return;
        }
        self.edited_only = false;
        self.open = true;
        self.mode = mode;
        self.query.clear();
        self.selected = 0;
        self.just_opened = true;
    }
}

#[derive(Clone)]
struct Entry {
    icon: IconId,
    kind: Option<phpide_icons::FileKind>,
    label: String,
    detail: String,
    hint: String,
    action: ActionKind,
}

#[derive(Clone)]
enum ActionKind {
    Open(PathBuf),
    OpenAt(PathBuf, usize, usize),
    Run(Command),
    Plugin(String),
    Settings(SettingsPage),
}

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
                || hay[h].is_uppercase() && hay[h - 1].is_lowercase();
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

/// Files ranked by name first (so `UC` finds `UserController.php`), then
/// by path.
fn project_files(
    ws: &Workspace,
    query: &str,
    cap: usize,
    keep: impl Fn(&Path) -> bool,
) -> Vec<(i32, PathBuf)> {
    let mut files: Vec<(i32, PathBuf)> = ws
        .files
        .iter()
        .filter(|e| !e.is_dir && keep(&e.path))
        .filter_map(|e| {
            let name = e.path.file_name()?.to_string_lossy();
            if let Some(score) = fuzzy_score(query, &name) {
                return Some((score + 20, e.path.clone()));
            }
            let rel = relative(ws, &e.path);
            fuzzy_score(query, &rel).map(|s| (s, e.path.clone()))
        })
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    files.truncate(cap);
    files
}

fn type_shown(state: &PaletteState, path: &Path) -> bool {
    if state.hidden_types.is_empty() {
        return true;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    !state.hidden_types.contains(&file_type(&name))
}

fn relative(ws: &Workspace, path: &Path) -> String {
    path.strip_prefix(&ws.root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn file_entry(ws: &Workspace, path: &Path, hint: &str) -> Entry {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (icon, kind) = file_icon(&name);
    let detail = relative(ws, path);
    let detail = detail
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_default();
    Entry {
        icon,
        kind: Some(kind),
        label: name,
        detail,
        hint: hint.to_string(),
        action: ActionKind::Open(path.to_path_buf()),
    }
}

fn symbol_entry(ws: &Workspace, hit: &SymbolHit) -> Entry {
    let place = format!("{}:{}", relative(ws, &hit.path), hit.line0 + 1);
    Entry {
        icon: if hit.is_class() {
            IconId::Structure
        } else {
            IconId::Command
        },
        kind: None,
        label: hit.name.clone(),
        detail: if hit.container.is_empty() {
            place
        } else {
            format!("{} · {place}", hit.container)
        },
        hint: hit.kind_label().to_string(),
        action: ActionKind::OpenAt(hit.path.clone(), hit.line0, hit.col0),
    }
}

fn collect_entries(
    ws: &Workspace,
    state: &PaletteState,
    extra: &[(String, String)],
    symbols: &[SymbolHit],
) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut query = state.query.trim_start();
    let mut mode = state.mode;
    if let Some(rest) = query.strip_prefix('>') {
        mode = PaletteMode::Actions;
        query = rest.trim_start();
    }
    let query = query.trim();

    // Recent files first when nothing is typed (RF-SEL-01).
    if mode == PaletteMode::Recent
        || query.is_empty() && matches!(mode, PaletteMode::All | PaletteMode::Files)
    {
        let list = if mode == PaletteMode::Recent && state.edited_only {
            &ws.edited_files
        } else {
            &ws.recent_files
        };
        for path in list {
            if !type_shown(state, path) {
                continue;
            }
            if query.is_empty() || fuzzy_score(query, &relative(ws, path)).is_some() {
                out.push(file_entry(ws, path, "recent"));
            }
        }
        if mode == PaletteMode::Recent || mode == PaletteMode::Files {
            return out;
        }
    }

    if !query.is_empty()
        && matches!(
            mode,
            PaletteMode::All | PaletteMode::Classes | PaletteMode::Symbols
        )
    {
        let mut hits: Vec<(i32, &SymbolHit)> = symbols
            .iter()
            .filter(|h| mode != PaletteMode::Classes && mode != PaletteMode::All || h.is_class())
            .filter_map(|h| fuzzy_score(query, &h.name).map(|s| (s, h)))
            .collect();
        hits.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
        let cap = if mode == PaletteMode::All { 8 } else { 60 };
        out.extend(hits.into_iter().take(cap).map(|(_, h)| symbol_entry(ws, h)));
    }

    if !query.is_empty() && matches!(mode, PaletteMode::All | PaletteMode::Files) {
        let cap = if mode == PaletteMode::All { 12 } else { 60 };
        let mut found: Vec<(i32, PathBuf, &str)> =
            project_files(ws, query, usize::MAX, |p| type_shown(state, p))
                .into_iter()
                .map(|(score, path)| (score, path, ""))
                .collect();
        if state.include_non_project {
            if let Some(libraries) = &state.libraries {
                for path in libraries.iter().filter(|p| type_shown(state, p)) {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy())
                        .unwrap_or_default();
                    let score = fuzzy_score(query, &name)
                        .map(|s| s + 20)
                        .or_else(|| fuzzy_score(query, &relative(ws, path)));
                    if let Some(score) = score {
                        // Project files first at equal scores.
                        found.push((score - 1, path.clone(), "non-project"));
                    }
                }
            }
        }
        found.sort_by_key(|f| std::cmp::Reverse(f.0));
        for (_, path, hint) in found.into_iter().take(cap) {
            out.push(file_entry(ws, &path, hint));
        }
    }

    if matches!(mode, PaletteMode::All | PaletteMode::Actions)
        && (!query.is_empty() || mode == PaletteMode::Actions)
    {
        let mut actions: Vec<(i32, Entry)> = Vec::new();
        for spec in crate::commands::REGISTRY {
            if !crate::commands::is_available(spec.id) {
                continue;
            }
            let label = spec.palette_label();
            if let Some(score) =
                fuzzy_score(query, &label).or_else(|| fuzzy_score(query, spec.label))
            {
                actions.push((
                    score,
                    Entry {
                        icon: IconId::Command,
                        kind: None,
                        label,
                        detail: String::new(),
                        hint: crate::commands::shortcut_text(spec.id),
                        action: ActionKind::Run(spec.id),
                    },
                ));
            }
        }
        // Settings pages, searched by name and keywords.
        for spec in crate::settings_dialog::search(query) {
            // A page named like the query beats loose subsequence matches.
            let named = spec.label.to_lowercase().contains(&query.to_lowercase());
            actions.push((
                if query.is_empty() {
                    0
                } else if named {
                    1000
                } else {
                    20
                },
                Entry {
                    icon: IconId::Settings,
                    kind: None,
                    label: format!("Settings: {}", spec.label),
                    detail: spec.section.to_string(),
                    hint: String::new(),
                    action: ActionKind::Settings(spec.page),
                },
            ));
        }
        for (label, id) in extra {
            if let Some(score) = fuzzy_score(query, label) {
                actions.push((
                    score,
                    Entry {
                        icon: IconId::Run,
                        kind: None,
                        label: label.clone(),
                        detail: "external".to_string(),
                        hint: String::new(),
                        action: ActionKind::Plugin(id.clone()),
                    },
                ));
            }
        }
        if !query.is_empty() {
            actions.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
        }
        let cap = if mode == PaletteMode::All { 10 } else { 200 };
        out.extend(actions.into_iter().take(cap).map(|(_, e)| e));
    }
    out
}

/// Draws the popup and returns the chosen action (Enter or click).
pub fn show(
    ctx: &egui::Context,
    ws: &Workspace,
    state: &mut PaletteState,
    extra: &[(String, String)],
    symbols: &[SymbolHit],
) -> Option<PaletteAction> {
    use crate::design::components::{ListRow, TextInput};
    if !state.open {
        return None;
    }
    let theme = crate::design::current(ctx);
    let p = theme.palette;
    let mut action: Option<PaletteAction> = None;
    let mut close = false;

    // Tab / Shift+Tab switch scope before the text field sees the key.
    if state.mode != PaletteMode::Recent {
        // Shift+Tab first: a plain Tab pattern also matches it.
        let (back, tab) = ctx.input_mut(|i| {
            let back = i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab);
            (back, i.consume_key(egui::Modifiers::NONE, egui::Key::Tab))
        });
        if tab || back {
            let here = TABS.iter().position(|m| *m == state.mode).unwrap_or(0);
            let next = if tab {
                (here + 1) % TABS.len()
            } else {
                (here + TABS.len() - 1) % TABS.len()
            };
            state.mode = TABS[next];
            state.selected = 0;
            state.just_opened = true;
        }
    }

    let area = egui::Area::new(egui::Id::new("palette"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 56.0))
        .show(ctx, |ui| {
            crate::design::components::Dialog::frame(ui).show(ui, |ui| {
                ui.set_width(640.0);
                if state.mode == PaletteMode::Recent {
                    ui.label(
                        egui::RichText::new(if state.edited_only {
                            "Recently Edited Files"
                        } else {
                            "Recent Files"
                        })
                        .strong(),
                    );
                    if !state.edited_only {
                        ui.weak("Press Ctrl+E again to show edited files only.");
                    }
                } else {
                    ui.horizontal(|ui| {
                        for mode in TABS {
                            let selected = state.mode == mode;
                            let text = egui::RichText::new(mode.title()).color(if selected {
                                p.text_primary
                            } else {
                                p.text_secondary
                            });
                            if ui.selectable_label(selected, text).clicked() {
                                state.mode = mode;
                                state.selected = 0;
                                state.just_opened = true;
                            }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if matches!(state.mode, PaletteMode::All | PaletteMode::Files) {
                                file_filters(ui, state);
                            } else if state.mode == PaletteMode::Actions {
                                ui.weak("Alt+Enter assigns a shortcut");
                            } else {
                                ui.weak("Tab switches");
                            }
                        });
                    });
                }
                ui.add_space(4.0);
                let hint = match state.mode {
                    PaletteMode::Recent => "Filter recent files",
                    PaletteMode::Actions => "Type an action or a setting",
                    PaletteMode::Classes => "Type a class name",
                    PaletteMode::Symbols => "Type a symbol name",
                    PaletteMode::Files => "Type a file name (CamelCase works: UsCo)",
                    PaletteMode::All => {
                        "Search files, classes, symbols and actions (> for actions)"
                    }
                };
                let edit = TextInput::search(&mut state.query)
                    .hint(hint)
                    .id_salt("palette-query")
                    .show(ui);
                if state.just_opened {
                    edit.request_focus();
                    state.just_opened = false;
                }
                // Tab switches scope instead of moving the focus away.
                if edit.has_focus() {
                    ui.memory_mut(|m| {
                        m.set_focus_lock_filter(
                            edit.id,
                            egui::EventFilter {
                                tab: true,
                                vertical_arrows: true,
                                ..Default::default()
                            },
                        )
                    });
                }
                if edit.changed() {
                    state.selected = 0;
                }

                let entries = collect_entries(ws, state, extra, symbols);
                if state.selected >= entries.len() {
                    state.selected = entries.len().saturating_sub(1);
                }
                let mut moved = false;
                ctx.input(|i| {
                    if i.key_pressed(egui::Key::ArrowDown) && !entries.is_empty() {
                        state.selected = (state.selected + 1).min(entries.len() - 1);
                        moved = true;
                    }
                    if i.key_pressed(egui::Key::ArrowUp) {
                        state.selected = state.selected.saturating_sub(1);
                        moved = true;
                    }
                    if i.key_pressed(egui::Key::PageDown) && !entries.is_empty() {
                        state.selected = (state.selected + 10).min(entries.len() - 1);
                        moved = true;
                    }
                    if i.key_pressed(egui::Key::PageUp) {
                        state.selected = state.selected.saturating_sub(10);
                        moved = true;
                    }
                    if i.key_pressed(egui::Key::Escape) {
                        close = true;
                    }
                });
                let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
                let alt_enter = enter && ctx.input(|i| i.modifiers.alt);

                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .id_salt("palette-results")
                    .max_height(380.0)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        if entries.is_empty() {
                            ui.weak(if state.query.trim().is_empty() {
                                "Nothing here yet."
                            } else if state.mode.wants_symbols() && symbols.is_empty() {
                                "No match. Classes and symbols need a running language server."
                            } else {
                                "Nothing found."
                            });
                        }
                        for (i, entry) in entries.iter().enumerate() {
                            let selected = i == state.selected;
                            let color = match entry.kind {
                                Some(kind) => p.file_color(kind),
                                None => p.text_secondary,
                            };
                            let mut row = ListRow::new(entry.label.clone())
                                .icon(entry.icon, color)
                                .selected(selected);
                            if !entry.detail.is_empty() {
                                row = row.detail(entry.detail.clone());
                            }
                            if !entry.hint.is_empty() {
                                row = row.trailing(entry.hint.clone());
                            }
                            let response = row.show(ui);
                            if selected && moved {
                                response.scroll_to_me(None);
                            }
                            if selected && alt_enter {
                                if let ActionKind::Run(c) = &entry.action {
                                    action = Some(PaletteAction::AssignShortcut(*c));
                                    close = true;
                                    continue;
                                }
                            }
                            if response.clicked() || (selected && enter) {
                                action = Some(match &entry.action {
                                    ActionKind::Open(p) => PaletteAction::Open(p.clone()),
                                    ActionKind::OpenAt(p, l, c) => {
                                        PaletteAction::OpenAt(p.clone(), *l, *c)
                                    }
                                    ActionKind::Run(c) => PaletteAction::Run(*c),
                                    ActionKind::Plugin(id) => PaletteAction::Plugin(id.clone()),
                                    ActionKind::Settings(page) => PaletteAction::Settings(*page),
                                });
                                close = true;
                            }
                        }
                    });
            });
        });
    // A click outside closes the popup, as in PhpStorm.
    let clicked_outside = ctx.input(|i| i.pointer.any_pressed())
        && !area.response.contains_pointer()
        && !state.just_opened;
    if clicked_outside {
        close = true;
    }

    if action.is_some() && enter_used(ctx) {
        // The Enter that chose the entry must not also press a button of
        // whatever opens next (a dialog's first row, say).
        ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    }
    if close {
        state.open = false;
        state.query.clear();
        state.selected = 0;
    }
    action
}

/// The Files filter: a type menu and "Include non-project items".
fn file_filters(ui: &mut egui::Ui, state: &mut PaletteState) {
    let hidden = state.hidden_types.len();
    let label = if hidden == 0 {
        "All file types".to_string()
    } else {
        format!("File types ({} hidden)", hidden)
    };
    let button = crate::design::components::ToolbarWidget::new(label)
        .icon(IconId::Filter)
        .tooltip("Filter by file type")
        .show(ui);
    egui::Popup::menu(&button)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(220.0);
            let groups = FILE_TYPES.iter().map(|(g, _)| *g).chain([OTHER_TYPE]);
            for group in groups {
                let mut shown = !state.hidden_types.contains(&group);
                if ui.checkbox(&mut shown, group).changed() {
                    state.hidden_types.retain(|g| *g != group);
                    if !shown {
                        state.hidden_types.push(group);
                    }
                    state.selected = 0;
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("All").clicked() {
                    state.hidden_types.clear();
                }
                if ui.button("None").clicked() {
                    state.hidden_types = FILE_TYPES
                        .iter()
                        .map(|(g, _)| *g)
                        .chain([OTHER_TYPE])
                        .collect();
                }
            });
        });
    if ui
        .checkbox(&mut state.include_non_project, "Include non-project items")
        .on_hover_text("Also search vendor/, node_modules/ and excluded folders")
        .changed()
    {
        state.selected = 0;
    }
}

fn enter_used(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.key_pressed(egui::Key::Enter))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camel_case_and_snake_case_queries_match_names() {
        assert!(fuzzy_score("UsCo", "UserController.php").is_some());
        let camel = fuzzy_score("UC", "UserController.php").unwrap();
        let middle = fuzzy_score("UC", "ubuntu_cache.php").unwrap();
        assert!(camel >= middle, "word starts rank higher");
        assert!(fuzzy_score("use_rep", "user_repository.php").is_some());
        assert!(fuzzy_score("xyz", "UserController.php").is_none());
    }

    #[test]
    fn empty_search_lists_recent_files_and_settings_are_actions() {
        let mut ws = Workspace::unscanned(PathBuf::from("/p"));
        ws.recent_files = vec![PathBuf::from("/p/src/A.php"), PathBuf::from("/p/B.php")];
        let mut state = PaletteState::new();
        state.open_with(PaletteMode::All);
        let entries = collect_entries(&ws, &state, &[], &[]);
        assert_eq!(entries[0].label, "A.php");
        assert_eq!(entries[1].label, "B.php");
        state.mode = PaletteMode::Actions;
        state.query = "color scheme".into();
        let entries = collect_entries(&ws, &state, &[], &[]);
        assert!(entries
            .iter()
            .any(|e| matches!(e.action, ActionKind::Settings(SettingsPage::ColorScheme))));
        // Classes keep only class-like symbols.
        let symbols = vec![
            SymbolHit {
                name: "Invoice".into(),
                container: "App".into(),
                path: "/p/src/Invoice.php".into(),
                line0: 4,
                col0: 0,
                kind: 5,
            },
            SymbolHit {
                name: "invoiceTotal".into(),
                container: String::new(),
                path: "/p/src/f.php".into(),
                line0: 1,
                col0: 0,
                kind: 12,
            },
        ];
        state.mode = PaletteMode::Classes;
        state.query = "inv".into();
        let entries = collect_entries(&ws, &state, &[], &symbols);
        assert_eq!(entries.len(), 1);
        state.mode = PaletteMode::Symbols;
        assert_eq!(collect_entries(&ws, &state, &[], &symbols).len(), 2);
    }

    #[test]
    fn files_filter_by_type_and_include_non_project_items() {
        let mut ws = Workspace::unscanned(PathBuf::from("/p"));
        for name in ["src/User.php", "assets/user.js", "user.json"] {
            ws.files.push(phpide_core::FileEntry {
                path: PathBuf::from("/p").join(name),
                depth: 1,
                is_dir: false,
            });
        }
        let mut state = PaletteState::new();
        state.open_with(PaletteMode::Files);
        state.query = "user".into();
        let labels = |state: &PaletteState| -> Vec<String> {
            collect_entries(&ws, state, &[], &[])
                .into_iter()
                .map(|e| e.label)
                .collect()
        };
        assert_eq!(labels(&state).len(), 3);
        state.hidden_types = vec!["JavaScript and TypeScript", "Configuration"];
        assert_eq!(labels(&state), ["User.php"]);
        state.include_non_project = true;
        state.libraries = Some(std::sync::Arc::new(vec![PathBuf::from(
            "/p/vendor/acme/src/UserFactory.php",
        )]));
        let entries = collect_entries(&ws, &state, &[], &[]);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].label, "User.php");
        assert_eq!(entries[1].hint, "non-project");
        assert_eq!(file_type("base.html.twig"), "Templates");
        assert_eq!(file_type("Makefile"), OTHER_TYPE);
    }
}
