//! Welcome Screen (REQUISITOS 1.5): Projects / Customize / Plugins / Learn /
//! Remote Development; projetos recentes, Get from VCS, browser de pastas,
//! importação de settings e New Project com esqueleto composer.json.

use std::path::PathBuf;

use eframe::egui;

use crate::themes::ThemeId;
use crate::PhpIdeApp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Projects,
    Customize,
    Plugins,
    Learn,
    RemoteDevelopment,
}

pub struct WelcomeState {
    tab: Tab,
    /// Diretório atual do browser embutido.
    browser_dir: PathBuf,
    /// Entrada de texto para caminho direto / nome de novo projeto.
    path_input: String,
    new_project_name: String,
    vcs_url: String,
    import_path: String,
    err: String,
}

impl WelcomeState {
    pub fn new() -> Self {
        let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
        WelcomeState {
            tab: Tab::Projects,
            browser_dir: home.clone(),
            path_input: home.display().to_string(),
            new_project_name: "novo-projeto".to_string(),
            vcs_url: String::new(),
            import_path: String::new(),
            err: String::new(),
        }
    }
}

impl Default for WelcomeState {
    fn default() -> Self {
        Self::new()
    }
}

pub fn welcome_screen(app: &mut PhpIdeApp, ctx: &egui::Context) {
    let mut close = false;
    egui::CentralPanel::default().show(ctx, |ui| {
        let avail = ui.available_size();
        let w = avail.x.min(860.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(avail.y * 0.12);
                ui.set_max_width(w);
                ui.heading("PHPForge IDE");
                ui.label(egui::RichText::new("Paridade PhpStorm 2026.2 — REQUISITOS.md").weak());
                ui.add_space(18.0);

                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(150.0);
                        for (tab, label) in [
                            (Tab::Projects, "Projects"),
                            (Tab::Customize, "Customize"),
                            (Tab::Plugins, "Plugins"),
                            (Tab::Learn, "Learn"),
                            (Tab::RemoteDevelopment, "Remote Development"),
                        ] {
                            ui.selectable_value(&mut app.welcome.tab, tab, label);
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_max_width(w - 175.0);
                        match app.welcome.tab {
                            Tab::Projects => projects_tab(app, ui, &mut close),
                            Tab::Customize => customize_tab(app, ui),
                            Tab::Plugins => plugins_tab(app, ui),
                            Tab::Learn => learn_tab(ui),
                            Tab::RemoteDevelopment => remote_development_tab(app, ui, &mut close),
                        }
                    });
                });
            });
        });
    });
    if close {
        app.welcome_open = false;
    }
}

fn projects_tab(app: &mut PhpIdeApp, ui: &mut egui::Ui, close: &mut bool) {
    ui.columns(2, |cols| {
        // ---- Coluna esquerda: ações + recentes.
        cols[0].vertical(|ui| {
            ui.label(egui::RichText::new("Ações").strong());
            ui.add_space(6.0);
            if ui.button("Open — navegar pastas").clicked() {
                let p = PathBuf::from(&app.welcome.path_input);
                if p.is_dir() {
                    app.load_project(p, ui.ctx());
                    *close = true;
                } else {
                    app.welcome.err = format!("diretório não existe: {}", p.display());
                }
            }
            if ui
                .button("New Project — criar pasta + composer.json")
                .clicked()
            {
                let name = app.welcome.new_project_name.trim().trim_start_matches('/');
                if name.is_empty() || name.contains("..") {
                    app.welcome.err = "nome de projeto inválido".to_string();
                } else {
                    let base = PathBuf::from(&app.welcome.path_input);
                    let target = base.join(name);
                    match create_project(&target) {
                        Ok(()) => {
                            app.load_project(target, ui.ctx());
                            *close = true;
                        }
                        Err(e) => app.welcome.err = format!("falha ao criar projeto: {e}"),
                    }
                }
            }
            ui.add_space(8.0);
            ui.label("Get from VCS — URL do repositório Git:");
            ui.text_edit_singleline(&mut app.welcome.vcs_url);
            if ui.button("Get from VCS").clicked() {
                match clone_from_vcs(&app.welcome.vcs_url, PathBuf::from(&app.welcome.path_input)) {
                    Ok(target) => {
                        app.load_project(target, ui.ctx());
                        *close = true;
                    }
                    Err(error) => app.welcome.err = error,
                }
            }
            ui.add_space(16.0);

            ui.label(egui::RichText::new("Recent Projects").strong());
            ui.add_space(4.0);
            let recents = app.settings.recent_projects.clone();
            if recents.is_empty() {
                ui.label(egui::RichText::new("Nenhum projeto aberto ainda.").weak());
            }
            for p in &recents {
                let exists = std::path::Path::new(p).is_dir();
                let label = if exists {
                    p.clone()
                } else {
                    format!("{p}  (não existe)")
                };
                if ui.add_enabled(exists, egui::Button::new(label)).clicked() {
                    app.load_project(PathBuf::from(p), ui.ctx());
                    *close = true;
                }
            }
        });

        // ---- Coluna direita: browser embutido de pastas.
        cols[1].vertical(|ui| {
            ui.label(egui::RichText::new("Browser de pastas").strong());
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Up").on_hover_text("Subir um nível").clicked() {
                    if let Some(parent) = app.welcome.browser_dir.parent() {
                        app.welcome.browser_dir = parent.to_path_buf();
                    }
                }
                let dir = app.welcome.browser_dir.display().to_string();
                if ui
                    .button(format!("Open {dir}"))
                    .on_hover_text("Abrir este diretório como projeto")
                    .clicked()
                {
                    app.load_project(app.welcome.browser_dir.clone(), ui.ctx());
                    *close = true;
                }
            });
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .show(ui, |ui| {
                    let mut entries: Vec<PathBuf> = Vec::new();
                    if let Ok(rd) = std::fs::read_dir(&app.welcome.browser_dir) {
                        for e in rd.flatten() {
                            let path = e.path();
                            if path.is_dir() {
                                entries.push(path);
                            }
                        }
                    }
                    entries.sort();
                    for path in entries {
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        let is_project =
                            path.join("composer.json").exists() || (path.join(".git")).exists();
                        let label = if is_project {
                            format!("[PHP] {name}/")
                        } else {
                            format!("[DIR] {name}/")
                        };
                        if ui.selectable_label(false, label).clicked() {
                            app.welcome.browser_dir = path.clone();
                            app.welcome.path_input = path.display().to_string();
                        }
                    }
                });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label("Caminho:");
                ui.text_edit_singleline(&mut app.welcome.path_input);
            });
            ui.horizontal(|ui| {
                ui.label("Novo projeto:");
                ui.text_edit_singleline(&mut app.welcome.new_project_name);
            });
            if !app.welcome.err.is_empty() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(&app.welcome.err).color(egui::Color32::RED));
            }
        });
    });
}

fn customize_tab(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("Theme").strong());
    ui.add_space(4.0);
    for theme in ThemeId::ALL {
        let sel = ThemeId::from_name(&app.settings.theme) == theme;
        if ui.selectable_label(sel, theme.label()).clicked() {
            app.apply_theme(theme, ui.ctx());
        }
    }
    ui.add_space(14.0);
    ui.label(egui::RichText::new("Accessibility").strong());
    ui.add_space(4.0);
    ui.checkbox(&mut app.zoom_on_ctrl_wheel, "Zoom com Ctrl+wheel");
    ui.add_space(14.0);
    ui.label(egui::RichText::new("Keymap").strong());
    for keymap in [
        "Default",
        "Windows",
        "macOS",
        "Eclipse",
        "NetBeans",
        "Sublime",
        "Visual Studio",
    ] {
        if ui
            .selectable_label(app.settings.keymap == keymap, keymap)
            .clicked()
        {
            app.settings.keymap = keymap.to_string();
            app.persist_settings();
        }
    }
    ui.add_space(14.0);
    ui.label(egui::RichText::new("Import Settings").strong());
    ui.label("Caminho de um settings.json exportado pelo PHPForge:");
    ui.text_edit_singleline(&mut app.welcome.import_path);
    if ui.button("Import Settings...").clicked() {
        match import_settings(&app.welcome.import_path) {
            Ok(()) => {
                app.settings = crate::settings::Settings::load();
                app.zoom = app.settings.zoom;
                let theme = ThemeId::from_name(&app.settings.theme);
                app.apply_theme(theme, ui.ctx());
                app.welcome.err = "configurações importadas".to_string();
            }
            Err(error) => app.welcome.err = error,
        }
    }
}

fn plugins_tab(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("Installed").strong());
    if app.plugins.is_empty() {
        ui.label("Nenhum plugin instalado em .phpide/plugins ou no diretório global.");
    } else {
        for plugin in &app.plugins {
            ui.horizontal(|ui| {
                ui.label("OK");
                ui.label(&plugin.label);
                ui.label(egui::RichText::new(&plugin.id).weak().small());
            });
        }
    }
    ui.add_space(16.0);
    ui.label(egui::RichText::new("Marketplace").strong());
    ui.label(egui::RichText::new("O catálogo remoto será conectado ao gerenciador de plugins; plugins locais por manifesto já são descobertos pela IDE.").weak());
}

fn learn_tab(ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("Learn PHPForge IDE").strong());
    ui.add_space(8.0);
    for (shortcut, action) in [
        ("Shift×2", "Search Everywhere"),
        ("Ctrl+Shift+N", "Go to File"),
        ("Ctrl+Shift+A", "Find Action"),
        ("Ctrl+Alt+S", "Settings"),
        ("Alt+1…0", "Tool windows"),
        ("Ctrl+Alt+3", "Database"),
        ("Alt+F12", "Terminal"),
        ("Ctrl+`", "Quick Switch Scheme"),
    ] {
        ui.horizontal(|ui| {
            ui.monospace(shortcut);
            ui.label(action);
        });
    }
}

fn remote_development_tab(app: &mut PhpIdeApp, ui: &mut egui::Ui, close: &mut bool) {
    ui.label(egui::RichText::new("Remote Development").strong());
    ui.label("Abra um projeto disponível por mount WSL, SSHFS ou Dev Container. O caminho é tratado pelo workspace local da IDE.");
    ui.text_edit_singleline(&mut app.welcome.path_input);
    if ui.button("Open Remote Project").clicked() {
        let path = PathBuf::from(&app.welcome.path_input);
        if path.is_dir() {
            app.load_project(path, ui.ctx());
            *close = true;
        } else {
            app.welcome.err = "o caminho remoto/montado não está acessível".to_string();
        }
    }
    ui.add_space(8.0);
    ui.label("WSL · SSH Configurations · Dev Containers");
}

fn import_settings(source: &str) -> Result<(), String> {
    let source = PathBuf::from(source.trim());
    let text = std::fs::read_to_string(&source)
        .map_err(|error| format!("falha ao ler {}: {error}", source.display()))?;
    serde_json::from_str::<serde_json::Value>(&text)
        .map_err(|error| format!("settings.json inválido: {error}"))?;
    let dir = crate::settings::config_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("falha ao criar configuração: {error}"))?;
    std::fs::write(dir.join("settings.json"), text)
        .map_err(|error| format!("falha ao importar configurações: {error}"))
}

fn clone_from_vcs(url: &str, base: PathBuf) -> Result<PathBuf, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("informe a URL do repositório".to_string());
    }
    let name = url
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("projeto")
        .trim_end_matches(".git");
    if name.is_empty() || name.contains("..") {
        return Err("não foi possível determinar o nome do projeto".to_string());
    }
    let target = base.join(name);
    if target.exists() {
        return Err(format!("o diretório já existe: {}", target.display()));
    }
    let status = std::process::Command::new("git")
        .args(["clone", "--", url])
        .arg(&target)
        .status()
        .map_err(|error| format!("falha ao executar git: {error}"))?;
    if status.success() {
        Ok(target)
    } else {
        Err(format!("git clone terminou com status {status}"))
    }
}

/// Cria esqueleto mínimo de projeto PHP (New Project).
fn create_project(dir: &PathBuf) -> std::io::Result<()> {
    std::fs::create_dir_all(dir.join("src"))?;
    std::fs::write(
        dir.join("composer.json"),
        "{\n    \"name\": \"meu/projeto\",\n    \"require\": { \"php\": \">=8.1\" },\n    \"autoload\": { \"psr-4\": { \"Meu\\\\Projeto\\\\\": \"src/\" } }\n}\n",
    )?;
    std::fs::write(
        dir.join("src/index.php"),
        "<?php\n\nrequire __DIR__ . '/../vendor/autoload.php';\n\necho 'Olá, PHPForge!\\n';\n",
    )?;
    std::fs::write(dir.join(".gitignore"), "/vendor/\n/.idea/\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_project_scaffold() {
        let dir = std::env::temp_dir().join(format!("phpide-newproj-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        create_project(&dir).unwrap();
        assert!(dir.join("composer.json").exists());
        assert!(dir.join("src/index.php").exists());
        let composer = std::fs::read_to_string(dir.join("composer.json")).unwrap();
        assert!(composer.contains("psr-4"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
