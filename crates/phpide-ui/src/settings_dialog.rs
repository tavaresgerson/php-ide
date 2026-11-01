//! Settings (Ctrl+Alt+S) — árvore de categorias da REQUISITOS 1.6.
//! As páginas de aparência, editor, keymap e PHP alteram preferências reais;
//! as demais já ocupam sua posição oficial e apontam para o módulo responsável.

use eframe::egui;

use crate::themes::ThemeId;
use crate::PhpIdeApp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPage {
    Appearance,
    Keymap,
    EditorAppearance,
    EditorFont,
    PhpLanguageLevel,
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
            SettingsPage::Named(label) => label,
        }
    }
}

#[derive(Debug)]
pub struct SettingsDialogState {
    pub open: bool,
    pub page: SettingsPage,
    query: String,
    just_opened: bool,
}

impl Default for SettingsDialogState {
    fn default() -> Self {
        Self {
            open: false,
            page: SettingsPage::Appearance,
            query: String::new(),
            just_opened: false,
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

const SECTIONS: &[(&str, &[&str])] = &[
    (
        "Appearance & Behavior",
        &[
            "Appearance",
            "Menus and Toolbars",
            "System Settings",
            "File Colors",
            "Scopes",
            "Notifications",
            "Quick Lists",
            "Path Variables",
        ],
    ),
    ("Keymap", &[]),
    ("Plugins", &[]),
    (
        "Editor",
        &[
            "General | Appearance",
            "General | Breadcrumbs",
            "General | Code Completion",
            "General | Editor Tabs",
            "General | Sticky Lines",
            "General | Smart Keys",
            "General | Postfix Completion",
            "General | Auto Import",
            "Code Style | PHP",
            "Code Style | JavaScript",
            "Code Style | CSS",
            "Code Style | HTML",
            "Code Style | SQL",
            "Code Style | HTTP Request",
            "Inspections",
            "Color Scheme",
            "Font",
            "Inlay Hints",
            "File Types",
            "Code Editing",
            "Live Templates",
        ],
    ),
    (
        "PHP",
        &[
            "Language Level",
            "CLI Interpreter",
            "Composer",
            "CLI Interpreters",
            "Debug",
            "Test Frameworks",
            "Quality Tools",
            "Frameworks | Symfony",
            "Frameworks | Laravel",
            "Frameworks | WordPress",
            "Frameworks | Drupal",
            "Frameworks | Joomla",
            "Blade",
            "Servers",
            "Path Mappings",
        ],
    ),
    (
        "Languages & Frameworks",
        &["JavaScript", "TypeScript", "Node.js", "CSS", "SQL", "Twig"],
    ),
    (
        "Version Control",
        &[
            "Git",
            "GitHub",
            "GitLab",
            "Mercurial",
            "Subversion",
            "Changelists",
            "Commit",
            "Confirmation",
        ],
    ),
    (
        "Build, Execution, Deployment",
        &[
            "Deployment",
            "Debugger",
            "Coverage",
            "Docker",
            "Vagrant",
            "WSL",
        ],
    ),
    (
        "Tools",
        &[
            "Terminal",
            "Web Browsers and Preview",
            "Actions on Save",
            "File Watchers",
            "SSH Configurations",
            "Tasks",
            "HTTP Client",
            "Database",
        ],
    ),
    ("Project Structure", &["Project"]),
    ("Advanced Settings", &[]),
];

fn page_for(label: &'static str) -> SettingsPage {
    match label {
        "Appearance" => SettingsPage::Appearance,
        "Keymap" => SettingsPage::Keymap,
        "General | Appearance" => SettingsPage::EditorAppearance,
        "Font" => SettingsPage::EditorFont,
        "Language Level" => SettingsPage::PhpLanguageLevel,
        other => SettingsPage::Named(other),
    }
}

pub fn show(app: &mut PhpIdeApp, ctx: &egui::Context) {
    if !app.settings_dialog.open {
        return;
    }
    let mut close = false;
    egui::Window::new("Settings")
        .collapsible(false)
        .resizable(true)
        .default_size([920.0, 610.0])
        .min_size([680.0, 420.0])
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Settings");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("X").on_hover_text("Close (Escape)").clicked() {
                        close = true;
                    }
                });
            });
            ui.separator();
            ui.columns(2, |columns| {
                columns[0].set_max_width(285.0);
                let search = egui::TextEdit::singleline(&mut app.settings_dialog.query)
                    .hint_text("Search settings")
                    .desired_width(265.0)
                    .show(&mut columns[0]);
                if app.settings_dialog.just_opened {
                    search.response.request_focus();
                    app.settings_dialog.just_opened = false;
                }
                columns[0].separator();
                egui::ScrollArea::vertical().show(&mut columns[0], |ui| {
                    let query = app.settings_dialog.query.to_lowercase();
                    for (section, children) in SECTIONS {
                        let section_match =
                            query.is_empty() || section.to_lowercase().contains(&query);
                        if children.is_empty() {
                            if section_match {
                                let page = page_for(section);
                                ui.selectable_value(&mut app.settings_dialog.page, page, *section);
                            }
                            continue;
                        }
                        let visible_children: Vec<_> = children
                            .iter()
                            .filter(|child| section_match || child.to_lowercase().contains(&query))
                            .collect();
                        if visible_children.is_empty() {
                            continue;
                        }
                        egui::CollapsingHeader::new(*section)
                            .default_open(
                                section_match && !query.is_empty()
                                    || app.settings_dialog.page.label() == *section,
                            )
                            .show(ui, |ui| {
                                for child in visible_children {
                                    let page = page_for(child);
                                    let label = child.rsplit(" | ").next().unwrap_or(child);
                                    ui.selectable_value(&mut app.settings_dialog.page, page, label);
                                }
                            });
                    }
                });
                columns[1].separator();
                egui::ScrollArea::vertical().show(&mut columns[1], |ui| {
                    show_page(app, ui);
                });
            });
        });
    if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        close = true;
    }
    if close {
        app.settings_dialog.open = false;
        app.persist_settings();
    }
}

fn show_page(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let page = app.settings_dialog.page;
    ui.heading(page.label().rsplit(" | ").next().unwrap_or(page.label()));
    ui.label(
        egui::RichText::new(match page {
            SettingsPage::EditorAppearance => "Editor | General | Appearance",
            SettingsPage::EditorFont => "Editor | Font",
            SettingsPage::PhpLanguageLevel => "PHP | Language Level",
            _ => page.label(),
        })
        .weak(),
    );
    ui.separator();
    match page {
        SettingsPage::Appearance => appearance_page(app, ui),
        SettingsPage::Keymap => keymap_page(app, ui),
        SettingsPage::EditorAppearance => editor_appearance_page(app, ui),
        SettingsPage::EditorFont => editor_font_page(app, ui),
        SettingsPage::PhpLanguageLevel => php_page(app, ui),
        SettingsPage::Named(name) => {
            ui.label(format!("Configurações de {name}."));
            ui.add_space(8.0);
            ui.label(egui::RichText::new(
                "A categoria já está mapeada na árvore oficial. A integração funcional será entregue com o módulo correspondente do roadmap.",
            ).weak());
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
    changed |= ui
        .checkbox(&mut app.settings.compact_mode, "Compact Mode")
        .changed();
    ui.horizontal(|ui| {
        ui.label("Navigation Bar:");
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
            });
    });
    ui.label("UI font: Inter");
    ui.label(egui::RichText::new("New UI é o padrão; os temas afetam a shell e o editor.").weak());
    if changed {
        app.persist_settings();
    }
}

fn keymap_page(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    ui.label("Keymap padrão compatível com PhpStorm. Outros mapas ficam disponíveis para seleção e persistência.");
    let mut changed = false;
    for keymap in [
        "Default",
        "Windows",
        "macOS",
        "Emacs",
        "Eclipse",
        "NetBeans",
        "Sublime",
        "Visual Studio",
    ] {
        changed |= ui
            .selectable_value(&mut app.settings.keymap, keymap.to_string(), keymap)
            .changed();
    }
    if changed {
        app.persist_settings();
    }
}

fn editor_appearance_page(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let mut changed = false;
    changed |= ui
        .checkbox(&mut app.settings.sticky_lines, "Sticky Lines (máx. 5)")
        .changed();
    changed |= ui
        .checkbox(&mut app.settings.inlay_hints, "Inlay Hints")
        .changed();
    changed |= ui
        .checkbox(&mut app.zoom_on_ctrl_wheel, "Zoom com Ctrl+wheel")
        .changed();
    changed |= ui
        .checkbox(&mut app.settings.column_selection, "Column Selection Mode")
        .changed();
    ui.add_space(8.0);
    ui.label("Breadcrumbs estruturais, highlight de ocorrências, indent guides e separadores PHP seguem as preferências do editor.");
    if changed {
        app.persist_settings();
    }
}

fn editor_font_page(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    ui.label("Fonte do editor: Monospace");
    ui.horizontal(|ui| {
        ui.label("IDE zoom:");
        let mut zoom = app.zoom;
        if ui
            .add(egui::Slider::new(&mut zoom, 0.5..=3.0).suffix("×"))
            .changed()
        {
            app.set_zoom(zoom);
        }
    });
    ui.label("Atalhos: Alt+Shift+. aumenta · Alt+Shift+, reduz · Ctrl+0 restaura");
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
    ui.label(
        egui::RichText::new("A sincronização com composer.json pertence ao módulo Composer.")
            .weak(),
    );
    if changed {
        app.persist_settings();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_contains_required_top_level_sections() {
        let names: Vec<_> = SECTIONS.iter().map(|section| section.0).collect();
        for required in [
            "Appearance & Behavior",
            "Keymap",
            "Plugins",
            "Editor",
            "PHP",
            "Languages & Frameworks",
            "Version Control",
            "Build, Execution, Deployment",
            "Tools",
            "Advanced Settings",
        ] {
            assert!(names.contains(&required), "seção ausente: {required}");
        }
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
