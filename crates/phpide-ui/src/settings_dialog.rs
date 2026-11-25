//! Settings (Ctrl+Alt+S) — árvore de categorias da REQUISITOS 1.6.
//! As páginas de aparência, editor, keymap e PHP alteram preferências reais;
//! as demais já ocupam sua posição oficial e apontam para o módulo responsável.

use eframe::egui;

use crate::design::ThemeId;
use crate::PhpIdeApp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPage {
    Appearance,
    Keymap,
    EditorAppearance,
    EditorFont,
    PhpLanguageLevel,
    SystemSettings,
    EditorTabs,
    ColorScheme,
    FileTypes,
    Named(&'static str),
}

impl SettingsPage {
    fn label(self) -> &'static str {
        match self {
            SettingsPage::Appearance => "Appearance",
            SettingsPage::Keymap => "Keymap",
            SettingsPage::EditorAppearance => "General | Appearance",
            SettingsPage::EditorFont => "Font",
            SettingsPage::PhpLanguageLevel => "Language Level",
            SettingsPage::SystemSettings => "System Settings",
            SettingsPage::EditorTabs => "General | Editor Tabs",
            SettingsPage::ColorScheme => "Color Scheme",
            SettingsPage::FileTypes => "File Types",
            SettingsPage::Named(label) => label,
        }
    }
}

pub struct SettingsDialogState {
    pub open: bool,
    pub page: SettingsPage,
    query: String,
    just_opened: bool,
    /// Color Scheme and Font pages.
    pub editor_pages: crate::settings_editor::EditorPagesState,
    /// Keymap page.
    pub keymap: crate::keymap_page::KeymapState,
}

impl Default for SettingsDialogState {
    fn default() -> Self {
        Self {
            open: false,
            page: SettingsPage::Appearance,
            query: String::new(),
            just_opened: false,
            editor_pages: Default::default(),
            keymap: Default::default(),
        }
    }
}

impl SettingsDialogState {
    pub fn open(&mut self, page: SettingsPage) {
        self.open = true;
        self.page = page;
        self.just_opened = true;
    }
}

/// A page of the Settings tree.
pub struct PageSpec {
    pub page: SettingsPage,
    pub section: &'static str,
    pub label: &'static str,
    /// Words the search also matches (RF-SEL-01: settings are searchable).
    pub keywords: &'static str,
}

/// Every page with real settings, in tree order. Pages of frozen features
/// only exist with the `experimental` feature.
pub const PAGES: &[PageSpec] = &[
    PageSpec {
        page: SettingsPage::Appearance,
        section: "Appearance & Behavior",
        label: "Appearance",
        keywords: "theme dark light high contrast compact density motion navigation bar",
    },
    PageSpec {
        page: SettingsPage::SystemSettings,
        section: "Appearance & Behavior",
        label: "System Settings",
        keywords: "project reopen startup new window current window ask open default directory folder location",
    },
    PageSpec {
        page: SettingsPage::Keymap,
        section: "Keymap",
        label: "Keymap",
        keywords: "shortcuts keys bindings assign shortcut",
    },
    PageSpec {
        page: SettingsPage::EditorAppearance,
        section: "Editor",
        label: "Appearance",
        keywords: "sticky lines inlay hints column selection zoom wheel",
    },
    PageSpec {
        page: SettingsPage::EditorTabs,
        section: "Editor",
        label: "Editor Tabs",
        keywords: "tabs limit closing policy open at end pinned rows",
    },
    PageSpec {
        page: SettingsPage::ColorScheme,
        section: "Editor",
        label: "Color Scheme",
        keywords: "colors syntax highlighting keyword string comment bold italic background language defaults",
    },
    PageSpec {
        page: SettingsPage::EditorFont,
        section: "Editor",
        label: "Font",
        keywords: "font family size line height ligatures zoom monospace",
    },
    PageSpec {
        page: SettingsPage::FileTypes,
        section: "Editor",
        label: "File Types",
        keywords: "file types patterns extensions associations override",
    },
    PageSpec {
        page: SettingsPage::PhpLanguageLevel,
        section: "PHP",
        label: "Language Level",
        keywords: "php version language level composer",
    },
];

/// Frozen pages, listed with the `experimental` feature.
const EXPERIMENTAL_PAGES: &[PageSpec] = &[PageSpec {
    page: SettingsPage::Named("Deployment"),
    section: "Build, Execution, Deployment",
    label: "Deployment",
    keywords: "deployment server sftp ftp",
}];

fn pages() -> impl Iterator<Item = &'static PageSpec> {
    PAGES.iter().chain(
        EXPERIMENTAL_PAGES
            .iter()
            .filter(|_| crate::scope::EXPERIMENTAL),
    )
}

/// Every page (tests walk them all).
#[cfg(test)]
pub fn all_pages() -> Vec<SettingsPage> {
    pages().map(|spec| spec.page).collect()
}

fn spec_of(page: SettingsPage) -> Option<&'static PageSpec> {
    pages().find(|spec| spec.page == page)
}

/// Page whose label (or `Section | Label`) is `label`.
pub fn page_named(label: &str) -> Option<SettingsPage> {
    pages()
        .find(|spec| {
            spec.label.eq_ignore_ascii_case(label)
                || format!("{} | {}", spec.section, spec.label).eq_ignore_ascii_case(label)
        })
        .map(|spec| spec.page)
}

/// Pages matching the search: label, section or keywords.
pub fn search(query: &str) -> Vec<&'static PageSpec> {
    let query = query.trim().to_lowercase();
    pages()
        .filter(|spec| {
            query.is_empty()
                || query.split_whitespace().all(|word| {
                    spec.label.to_lowercase().contains(word)
                        || spec.section.to_lowercase().contains(word)
                        || spec.keywords.contains(word)
                })
        })
        .collect()
}

pub fn show(app: &mut PhpIdeApp, ctx: &egui::Context) {
    use crate::design::components::{Dialog, IconButton, TextInput, TreeRow};
    if !app.settings_dialog.open {
        return;
    }
    let mut close = false;
    let screen = ctx.content_rect();
    let size = egui::vec2(
        (screen.width() - 80.0).clamp(640.0, 1060.0),
        (screen.height() - 80.0).clamp(420.0, 700.0),
    );
    let theme = crate::design::current(ctx);
    let p = theme.palette;
    egui::Modal::new(egui::Id::new("settings-dialog"))
        .backdrop_color(p.shadow)
        .frame(egui::Frame::NONE)
        .show(ctx, |ui| {
            Dialog::frame(ui).show(ui, |ui| {
                ui.set_width(size.x);
                ui.set_height(size.y);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Settings")
                            .size(crate::design::tokens::typography::TITLE)
                            .color(p.text_primary),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if IconButton::new(phpide_icons::IconId::Close, "Close")
                            .shortcut("Esc")
                            .show(ui)
                            .clicked()
                        {
                            close = true;
                        }
                    });
                });
                ui.add_space(8.0);
                ui.horizontal_top(|ui| {
                    // Navigation: search and the page tree.
                    ui.vertical(|ui| {
                        ui.set_width(230.0);
                        let field = TextInput::search(&mut app.settings_dialog.query)
                            .hint("Search settings")
                            .id_salt("settings-search")
                            .show(ui);
                        if app.settings_dialog.just_opened {
                            field.request_focus();
                            app.settings_dialog.just_opened = false;
                        }
                        ui.add_space(6.0);
                        let found = search(&app.settings_dialog.query);
                        egui::ScrollArea::vertical()
                            .id_salt("settings-nav")
                            .max_height(size.y - 90.0)
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                let mut last_section = "";
                                for spec in &found {
                                    let single = spec.section == spec.label;
                                    if spec.section != last_section && !single {
                                        TreeRow::new(spec.section).expanded(Some(true)).show(ui);
                                    }
                                    last_section = spec.section;
                                    let row = TreeRow::new(spec.label)
                                        .depth(usize::from(!single))
                                        .selected(app.settings_dialog.page == spec.page)
                                        .show(ui);
                                    if row.response.clicked() {
                                        app.settings_dialog.page = spec.page;
                                    }
                                }
                                if found.is_empty() {
                                    ui.weak("Nothing found.");
                                }
                            });
                        // Enter opens the first match.
                        if field.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            if let Some(first) = found.first() {
                                app.settings_dialog.page = first.page;
                            }
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("settings-page")
                            .max_height(size.y - 50.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| show_page(app, ui));
                    });
                });
            });
        });
    let recording = app.settings_dialog.keymap.recording;
    if !recording && ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        close = true;
    }
    if close {
        app.settings_dialog.open = false;
        app.persist_settings();
    }
}

fn show_page(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let page = app.settings_dialog.page;
    let (section, label) = spec_of(page).map_or(("", page.label()), |s| (s.section, s.label));
    ui.label(
        egui::RichText::new(label)
            .size(crate::design::tokens::typography::TITLE)
            .strong(),
    );
    if !section.is_empty() && section != label {
        ui.weak(format!("{section} › {label}"));
    }
    ui.separator();
    match page {
        SettingsPage::Appearance => appearance_page(app, ui),
        SettingsPage::Keymap => keymap_page(app, ui),
        SettingsPage::EditorAppearance => editor_appearance_page(app, ui),
        SettingsPage::EditorFont => crate::settings_editor::font_page(app, ui),
        SettingsPage::PhpLanguageLevel => php_page(app, ui),
        SettingsPage::SystemSettings => system_page(app, ui),
        SettingsPage::EditorTabs => tabs_page(app, ui),
        SettingsPage::ColorScheme => crate::settings_editor::color_scheme_page(app, ui),
        SettingsPage::FileTypes => file_types_page(app, ui),
        SettingsPage::Named(name) => {
            ui.label(format!("{name} settings."));
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Not available yet.").weak());
        }
    }
}

/// Project opening and startup (RF-PRJ-08, RF-PRJ-11).
fn system_page(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let mut changed = false;
    ui.label(egui::RichText::new("Project").strong());
    changed |= ui
        .checkbox(
            &mut app.settings.reopen_last_project,
            "Reopen projects on startup",
        )
        .changed();
    ui.add_space(6.0);
    ui.label("Open project in:");
    for (value, label) in [
        ("new", "New window"),
        ("current", "Current window"),
        ("ask", "Ask"),
    ] {
        if ui
            .radio(app.settings.open_project_mode == value, label)
            .clicked()
        {
            app.settings.open_project_mode = value.to_string();
            changed = true;
        }
    }
    ui.add_space(6.0);
    // Where New Project and Get from VCS create folders (RF-PRJ-14).
    let label = ui.label("Default project directory:");
    let field = ui.add(
        egui::TextEdit::singleline(&mut app.settings.projects_dir)
            .hint_text("Home folder")
            .desired_width(320.0),
    );
    let field = field.labelled_by(label.id);
    changed |= field.changed();
    let dir = app.settings.projects_dir.trim();
    if !dir.is_empty() && !std::path::Path::new(dir).is_dir() {
        let p = crate::design::current(ui.ctx()).palette;
        ui.colored_label(p.warning, "This folder does not exist yet.");
    }
    if changed {
        app.persist_settings();
    }
}

/// Tab opening and closing policy (RF-TAB-01, RF-TAB-03).
fn tabs_page(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let mut changed = false;
    ui.label(egui::RichText::new("Opening Policy").strong());
    changed |= ui
        .checkbox(
            &mut app.settings.tabs_open_at_end,
            "Open new tabs at the end",
        )
        .changed();
    ui.add_space(8.0);
    ui.label(egui::RichText::new("Appearance").strong());
    changed |= ui
        .checkbox(
            &mut app.settings.tabs_multiple_rows,
            "Show tabs in multiple rows",
        )
        .changed();
    if ui
        .checkbox(&mut app.settings.tabs_pinned_left, "Show pinned tabs first")
        .changed()
    {
        if app.settings.tabs_pinned_left {
            app.ws.keep_pinned_first();
        }
        changed = true;
    }
    ui.horizontal(|ui| {
        ui.label("Tab placement:");
        for option in ["Top", "None"] {
            changed |= ui
                .selectable_value(&mut app.settings.tab_placement, option.to_string(), option)
                .changed();
        }
    });
    ui.horizontal(|ui| {
        ui.label("Close button:");
        for option in ["Right", "Left", "None"] {
            changed |= ui
                .selectable_value(
                    &mut app.settings.tab_close_button,
                    option.to_string(),
                    option,
                )
                .changed();
        }
    });
    changed |= ui
        .checkbox(
            &mut app.settings.tabs_alphabetical,
            "Sort tabs alphabetically",
        )
        .changed();
    ui.add_space(8.0);
    ui.label(egui::RichText::new("Closing Policy").strong());
    ui.horizontal(|ui| {
        ui.label("When the active tab closes, activate:");
        for option in phpide_core::AfterClose::ALL {
            changed |= ui
                .selectable_value(
                    &mut app.settings.tab_after_close,
                    option.label().to_string(),
                    option.label(),
                )
                .changed();
        }
    });
    ui.horizontal(|ui| {
        let label = ui.label("Tab limit:");
        let mut limit = app.settings.tab_limit;
        if ui
            .add(egui::DragValue::new(&mut limit).range(1..=99))
            .labelled_by(label.id)
            .changed()
        {
            app.settings.tab_limit = limit;
            app.ws.tab_limit = limit as usize;
            changed = true;
        }
    });
    ui.label(
        egui::RichText::new(
            "Over the limit, the unmodified tab opened longest ago closes. Pinned tabs stay.",
        )
        .weak(),
    );
    if changed {
        app.ws.open_tabs_at_end = app.settings.tabs_open_at_end;
        app.persist_settings();
    }
}

/// Recognized file types and the user's patterns (RF-HL-03).
fn file_types_page(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical()
        .max_height(260.0)
        .show(ui, |ui| {
            egui::Grid::new("file-types").striped(true).show(ui, |ui| {
                ui.label(egui::RichText::new("Type").strong());
                ui.label(egui::RichText::new("Patterns").strong());
                ui.end_row();
                for file_type in phpide_php::filetype::FILE_TYPES {
                    ui.label(file_type.name);
                    let mut patterns: Vec<String> =
                        file_type.patterns.iter().map(|p| p.to_string()).collect();
                    for (pattern, id) in &app.settings.file_type_patterns {
                        if id == file_type.id {
                            patterns.push(format!("{pattern} (yours)"));
                        }
                    }
                    ui.label(egui::RichText::new(patterns.join(", ")).weak());
                    ui.end_row();
                }
            });
        });
    ui.add_space(8.0);
    ui.label(egui::RichText::new("Your patterns").strong());
    let mut remove = None;
    for (i, (pattern, id)) in app.settings.file_type_patterns.iter().enumerate() {
        ui.horizontal(|ui| {
            let name = phpide_php::filetype::by_id(id).map_or(id.as_str(), |t| t.name);
            ui.label(format!("{pattern} → {name}"));
            if ui.small_button("Remove").clicked() {
                remove = Some(i);
            }
        });
    }
    if app.settings.file_type_patterns.is_empty() {
        ui.weak("None. Pick a type from the status bar or the notice of an unknown file.");
    }
    if let Some(i) = remove {
        app.settings.file_type_patterns.remove(i);
        app.persist_settings();
    }
    if !app.settings.file_type_overrides.is_empty() {
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Files with a forced type").strong());
        let mut clear = None;
        for (i, (path, id)) in app.settings.file_type_overrides.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("{path} → {id}"));
                if ui.small_button("Revert").clicked() {
                    clear = Some(i);
                }
            });
        }
        if let Some(i) = clear {
            app.settings.file_type_overrides.remove(i);
            app.persist_settings();
        }
    }
}

fn appearance_page(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("Theme").strong());
    for theme in ThemeId::ALL {
        if ui
            .selectable_label(app.theme == theme, theme.label())
            .clicked()
        {
            app.apply_theme(theme, ui.ctx());
        }
    }
    ui.add_space(12.0);
    let mut changed = false;
    let mut restyle = false;
    restyle |= ui
        .checkbox(&mut app.settings.compact_mode, "Compact Mode")
        .changed();
    restyle |= ui
        .checkbox(&mut app.settings.reduced_motion, "Reduce Motion")
        .on_hover_text("Turns off non-essential animations")
        .changed();
    ui.horizontal(|ui| {
        let label = ui.label("Navigation Bar:");
        egui::ComboBox::from_id_salt("settings-navigation-bar")
            .selected_text(&app.settings.navigation_bar)
            .show_ui(ui, |ui| {
                for option in ["Bottom", "Top", "Hidden"] {
                    changed |= ui
                        .selectable_value(
                            &mut app.settings.navigation_bar,
                            option.to_string(),
                            option,
                        )
                        .changed();
                }
            })
            .response
            .labelled_by(label.id);
    });
    ui.label("UI font: Inter");
    ui.label(egui::RichText::new("Themes apply to the window and the editor.").weak());
    if changed || restyle {
        app.persist_settings();
    }
    if restyle {
        app.apply_design(ui.ctx());
    }
}

fn keymap_page(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    crate::keymap_page::show(app, ui);
}

fn editor_appearance_page(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let mut changed = false;
    changed |= ui
        .checkbox(&mut app.settings.sticky_lines, "Sticky lines (up to 5)")
        .changed();
    changed |= ui
        .checkbox(&mut app.settings.inlay_hints, "Inlay Hints")
        .changed();
    changed |= ui
        .checkbox(&mut app.zoom_on_ctrl_wheel, "Zoom with Ctrl+mouse wheel")
        .changed();
    changed |= ui
        .checkbox(&mut app.settings.column_selection, "Column Selection Mode")
        .changed();
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label("Line separator for new files:");
        for label in ["LF", "CRLF"] {
            changed |= ui
                .selectable_value(&mut app.settings.line_ending, label.to_string(), label)
                .changed();
        }
    });
    ui.label(
        egui::RichText::new(
            "Existing files keep their own separator; change it from the status bar.",
        )
        .weak(),
    );
    ui.add_space(8.0);
    ui.label("Breadcrumbs and occurrence highlighting follow these editor settings.");
    if changed {
        app.persist_settings();
    }
}

fn php_page(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    ui.label("PHP Language Level");
    let mut changed = false;
    for level in ["5.3", "7.0", "7.4", "8.0", "8.1", "8.2", "8.3", "8.4"] {
        changed |= ui
            .selectable_value(
                &mut app.settings.php_level,
                level.to_string(),
                format!("PHP {level}"),
            )
            .changed();
    }
    ui.label(egui::RichText::new("Detected from composer.json when available.").weak());
    if changed {
        app.persist_settings();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_is_real_and_searchable() {
        for spec in PAGES {
            assert!(
                !matches!(spec.page, SettingsPage::Named(_)),
                "{} has no content",
                spec.label
            );
        }
        let labels = |q: &str| search(q).iter().map(|s| s.label).collect::<Vec<_>>();
        assert_eq!(labels("ligatures"), ["Font"]);
        assert_eq!(labels("bold"), ["Color Scheme"]);
        assert!(labels("tabs").contains(&"Editor Tabs"));
        assert_eq!(page_named("Color Scheme"), Some(SettingsPage::ColorScheme));
        assert_eq!(page_named("Editor | Font"), Some(SettingsPage::EditorFont));
    }

    #[test]
    fn page_open_restores_search_focus() {
        let mut state = SettingsDialogState::default();
        state.open(SettingsPage::Named("Deployment"));
        assert!(state.open);
        assert!(state.just_opened);
        assert_eq!(state.page, SettingsPage::Named("Deployment"));
    }
}
