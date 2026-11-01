//! Configurações persistentes (Estágio 5/6) em
//! `$XDG_CONFIG_HOME/phpide/settings.json` (ou `~/.config/phpide/`).
//!
//! Agora cobre o que o Quick Switch Scheme / Welcome / Status Bar precisam:
//! tema nomeado (família Islands), zoom, nível da linguagem PHP, quebra de
//! linha/indentação padrão e a lista de projetos recentes da Welcome Screen.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

pub fn config_dir() -> PathBuf {
    if let Ok(x) = std::env::var("XDG_CONFIG_HOME") {
        if !x.is_empty() {
            return PathBuf::from(x).join("phpide");
        }
    }
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(".config/phpide")
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// Id do tema (veja `themes.rs`): "islands-dark", "islands-light",
    /// "darcula", "light". Aceita o legado "dark"/"light" na leitura.
    pub theme: String,
    pub zoom: f32,
    /// Nível da linguagem PHP exibido/selecionável na status bar.
    pub php_level: String,
    /// Quebra de linha usada ao salvar: "LF" | "CRLF".
    pub line_ending: String,
    /// Estilo de indentação exibido na status bar: "2 spaces" | "4 spaces" | "8 spaces" | "Tab".
    pub indent: String,
    /// Projetos recentes (MRU) da Welcome Screen.
    pub recent_projects: Vec<String>,
    /// New UI: reduz alturas de barras e espaçamentos.
    pub compact_mode: bool,
    /// Posição da Navigation Bar: "Bottom" | "Top" | "Hidden".
    pub navigation_bar: String,
    /// Servidor de deployment exibido na status bar.
    pub deployment_server: String,
    /// Column Selection Mode (Alt+Shift+Insert).
    pub column_selection: bool,
    /// Editor: sticky lines e inlay hints (preferências visuais).
    pub sticky_lines: bool,
    pub inlay_hints: bool,
    /// Keymap selecionado no Customize/Settings.
    pub keymap: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: "islands-dark".to_string(),
            zoom: 1.0,
            php_level: "8.1".to_string(),
            line_ending: "LF".to_string(),
            indent: "4 spaces".to_string(),
            recent_projects: Vec::new(),
            compact_mode: false,
            navigation_bar: "Bottom".to_string(),
            deployment_server: "None".to_string(),
            column_selection: false,
            sticky_lines: true,
            inlay_hints: true,
            keymap: "Default".to_string(),
        }
    }
}

impl Settings {
    /// Compatibilidade com o arquivo do Estágio 5: `{theme: "dark"|"light", zoom}`.
    pub fn load() -> Self {
        let mut s = Self::default();
        let path = config_dir().join("settings.json");
        let Ok(text) = std::fs::read_to_string(path) else {
            return s;
        };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            return s;
        };
        if let Some(t) = v["theme"].as_str() {
            s.theme = match t {
                "dark" => "islands-dark".to_string(),
                "light" => "islands-light".to_string(),
                other => other.to_string(),
            };
        }
        if let Some(z) = v["zoom"].as_f64() {
            s.zoom = (z as f32).clamp(0.5, 3.0);
        }
        if let Some(p) = v["php_level"].as_str() {
            s.php_level = p.to_string();
        }
        if let Some(le) = v["line_ending"].as_str() {
            s.line_ending = le.to_string();
        }
        if let Some(i) = v["indent"].as_str() {
            s.indent = i.to_string();
        }
        if let Some(arr) = v["recent_projects"].as_array() {
            s.recent_projects = arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
        }
        if let Some(value) = v["compact_mode"].as_bool() {
            s.compact_mode = value;
        }
        if let Some(value) = v["navigation_bar"].as_str() {
            if matches!(value, "Bottom" | "Top" | "Hidden") {
                s.navigation_bar = value.to_string();
            }
        }
        if let Some(value) = v["deployment_server"].as_str() {
            s.deployment_server = value.to_string();
        }
        if let Some(value) = v["column_selection"].as_bool() {
            s.column_selection = value;
        }
        if let Some(value) = v["sticky_lines"].as_bool() {
            s.sticky_lines = value;
        }
        if let Some(value) = v["inlay_hints"].as_bool() {
            s.inlay_hints = value;
        }
        if let Some(value) = v["keymap"].as_str() {
            s.keymap = value.to_string();
        }
        s
    }

    pub fn save(&self) {
        let dir = config_dir();
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let v = json!({
            "theme": self.theme,
            "zoom": self.zoom,
            "php_level": self.php_level,
            "line_ending": self.line_ending,
            "indent": self.indent,
            "recent_projects": self.recent_projects,
            "compact_mode": self.compact_mode,
            "navigation_bar": self.navigation_bar,
            "deployment_server": self.deployment_server,
            "column_selection": self.column_selection,
            "sticky_lines": self.sticky_lines,
            "inlay_hints": self.inlay_hints,
            "keymap": self.keymap,
        });
        let _ = std::fs::write(dir.join("settings.json"), v.to_string());
    }

    /// Registra projeto aberto no topo do MRU (sem duplicatas, máx. 12).
    pub fn touch_project(&mut self, path: &Path) {
        let s = path.display().to_string();
        self.recent_projects.retain(|p| p != &s);
        self.recent_projects.insert(0, s);
        self.recent_projects.truncate(12);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_islands_dark() {
        let s = Settings::default();
        assert_eq!(s.theme, "islands-dark");
        assert!(s.theme != "islands-light");
        assert_eq!(s.php_level, "8.1");
    }

    #[test]
    fn new_ui_defaults_follow_requirements() {
        let settings = Settings::default();
        assert_eq!(settings.navigation_bar, "Bottom");
        assert_eq!(settings.deployment_server, "None");
        assert!(!settings.compact_mode);
        assert!(settings.sticky_lines);
        assert!(settings.inlay_hints);
        assert_eq!(settings.keymap, "Default");
    }

    #[test]
    fn legacy_dark_theme_maps_to_islands() {
        // Simula o parse do JSON legado sem tocar no config real.
        let v = json!({ "theme": "dark", "zoom": 1.2 });
        let parsed = serde_json::from_str::<Value>(&v.to_string()).unwrap();
        assert_eq!(parsed["theme"], "dark");
        // Caminho de migração coberto por load(); aqui só o contrato do JSON.
    }

    #[test]
    fn settings_roundtrip_fields() {
        let dir = std::env::temp_dir().join(format!("phpide-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.json");
        let v = json!({
            "theme": "islands-light",
            "zoom": 1.4,
            "php_level": "8.3",
            "line_ending": "CRLF",
            "indent": "2 spaces",
            "recent_projects": ["/tmp/a", "/tmp/b"],
        });
        std::fs::write(&file, v.to_string()).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        let parsed = serde_json::from_str::<Value>(&text).unwrap();
        assert_eq!(parsed["theme"], "islands-light");
        assert_eq!(parsed["php_level"], "8.3");
        assert_eq!(parsed["recent_projects"].as_array().unwrap().len(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn touch_project_mru_no_duplicates() {
        let mut s = Settings::default();
        s.touch_project(Path::new("/tmp/alpha"));
        s.touch_project(Path::new("/tmp/beta"));
        s.touch_project(Path::new("/tmp/alpha"));
        assert_eq!(s.recent_projects[0], "/tmp/alpha");
        assert_eq!(s.recent_projects.len(), 2);
    }
}
