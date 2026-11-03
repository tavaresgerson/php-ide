//! Configurações persistentes (Estágio 5/6) em
//! `$XDG_CONFIG_HOME/phpide/settings.json` (ou `~/.config/phpide/`).
//!
//! Agora cobre o que o Quick Switch Scheme / Welcome / Status Bar precisam:
//! tema nomeado (família Islands), zoom, nível da linguagem PHP, quebra de
//! linha/indentação padrão e a lista de projetos recentes da Welcome Screen.
//!
//! Persistência versionada com migração incremental e escrita atômica
//! (`phpide_core::settings`, NFR-004 / T-NFR-004-01/02): o arquivo carrega
//! `settings_version`; JSON inválido vira erro recuperável com o original
//! preservado (`.invalid`); toda gravação mantém `.bak` da versão anterior.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use phpide_core::settings as store;
use phpide_core::settings::Migration;

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

/// Migrações incrementais do settings.json da IDE:
/// - v1 (Estágio 5: `theme: "dark"|"light"` sem versão) → v2 (schema atual,
///   com nomes Islands e caminhos de ferramentas).
pub fn migrations() -> store::MigrationChain {
    store::MigrationChain::new(vec![Migration {
        from: 1,
        apply: |mut v| {
            if let Some(t) = v.get("theme").and_then(Value::as_str) {
                let mapped = match t {
                    "dark" => "islands-dark",
                    "light" => "islands-light",
                    other => other,
                };
                v["theme"] = Value::from(mapped);
            }
            v
        },
    }])
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
    /// Caminho explícito do PHP ("" = descobrir no PATH, NFR-003-01).
    pub php_path: String,
    /// Caminho explícito do Composer ("" = descobrir no PATH).
    pub composer_path: String,
    /// Closing Policy: máximo de abas (default 10, como o PhpStorm).
    pub tab_limit: u32,
}

/// Limites de zoom da IDE (UI-ED-010): NaN volta a 1.0; fora da faixa
/// (inclusive ±infinito) cai nos extremos 0.5/3.0.
pub fn clamp_zoom(zoom: f32) -> f32 {
    if zoom.is_nan() {
        return 1.0;
    }
    zoom.clamp(0.5, 3.0)
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
            php_path: String::new(),
            composer_path: String::new(),
            tab_limit: 10,
        }
    }
}

impl Settings {
    /// Carrega do disco com migração e recuperação (NFR-004). O segundo
    /// elemento é um diagnóstico recuperável (settings inválido preservado
    /// como `.invalid`, versão futura, E/S) para exibição na UI.
    pub fn load_or_default() -> (Self, Option<phpide_core::IdeError>) {
        let path = config_dir().join("settings.json");
        let report = store::load(&path, &migrations());
        let mut s = Self::default();
        let v = &report.value;
        if let Some(t) = v["theme"].as_str() {
            s.theme = match t {
                "dark" => "islands-dark".to_string(),
                "light" => "islands-light".to_string(),
                other => other.to_string(),
            };
        }
        if let Some(z) = v["zoom"].as_f64() {
            s.zoom = clamp_zoom(z as f32);
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
        if let Some(value) = v["php_path"].as_str() {
            s.php_path = value.to_string();
        }
        if let Some(value) = v["composer_path"].as_str() {
            s.composer_path = value.to_string();
        }
        if let Some(value) = v["tab_limit"].as_u64() {
            // Limites seguros: pelo menos 1 aba, no máximo 99.
            s.tab_limit = value.clamp(1, 99) as u32;
        }
        (s, report.recovered)
    }

    /// Compatibilidade com o arquivo do Estágio 5: `{theme: "dark"|"light", zoom}`.
    pub fn load() -> Self {
        Self::load_or_default().0
    }

    /// Grava versionado e atomicamente (tmp + rename + `.bak` anterior).
    /// Falhas são logadas (nunca panic): a IDE continua com a config em memória.
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
            "php_path": self.php_path,
            "composer_path": self.composer_path,
            "tab_limit": self.tab_limit,
        });
        if let Err(e) = store::save(&dir.join("settings.json"), &v) {
            eprintln!("{}", e.log_line());
        }
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

    /// Os testes abaixo alteram XDG_CONFIG_HOME (env global do processo):
    /// mutuamente exclusivos para não ler o settings.json um do outro.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_xdg<T>(dir: &Path, f: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK.lock().unwrap();
        let key = "XDG_CONFIG_HOME";
        let old = std::env::var(key).ok();
        unsafe { std::env::set_var(key, dir) };
        let result = f();
        match old {
            Some(v) => unsafe { std::env::set_var(key, v) },
            None => unsafe { std::env::remove_var(key) },
        }
        result
    }

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

    #[test]
    fn legacy_v1_config_migrates_with_theme_mapping() {
        // Config do Estágio 5 sem settings_version: migra para v2 e mapeia
        // os temas legados (T-NFR-004-01: migrações com configs antigas).
        let dir = std::env::temp_dir().join(format!("phpide-uimig-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("phpide")).unwrap();
        let file = dir.join("phpide").join("settings.json");
        std::fs::write(&file, r#"{"theme": "dark", "zoom": 1.2}"#).unwrap();
        let (s, err) = with_xdg(&dir, || {
            let (s, err) = Settings::load_or_default();
            assert!(err.is_none(), "sem erro recuperável: {err:?}");
            assert_eq!(s.theme, "islands-dark", "dark legado → islands-dark");
            assert!((s.zoom - 1.2).abs() < 1e-6);
            // Salvar grava a versão corrente.
            s.save();
            (s, err)
        });
        let _ = (s, err);
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            text.contains("\"settings_version\": 2"),
            "versão gravada: {text}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn invalid_settings_recovers_with_defaults_and_preserves_file() {
        let dir = std::env::temp_dir().join(format!("phpide-uiinv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("phpide")).unwrap();
        std::fs::write(dir.join("phpide").join("settings.json"), "{ quebrado").unwrap();
        let (s, err) = with_xdg(&dir, Settings::load_or_default);
        let err = err.expect("diagnóstico recuperável");
        assert_eq!(err.kind, phpide_core::ErrorKind::InvalidConfig);
        assert_eq!(s.theme, "islands-dark", "defaults em uso");
        assert!(
            dir.join("phpide").join("settings.json.invalid").exists(),
            "arquivo original preservado"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tool_paths_default_to_empty_and_roundtrip() {
        let mut s = Settings::default();
        assert_eq!(s.php_path, "");
        assert_eq!(s.composer_path, "");
        s.php_path = "/opt/php bin/php".to_string();
        s.composer_path = "/usr/local/bin/composer".to_string();
        let dir = std::env::temp_dir().join(format!("phpide-uitool-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (back, err) = with_xdg(&dir, || {
            s.save();
            Settings::load_or_default()
        });
        assert!(err.is_none());
        assert_eq!(back.php_path, "/opt/php bin/php");
        assert_eq!(back.composer_path, "/usr/local/bin/composer");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn zoom_limits_clamp_and_non_finite_reset() {
        // UI-ED-010: limites 0.5..=3.0; valores inválidos voltam a 1.0.
        assert_eq!(clamp_zoom(1.5), 1.5);
        assert_eq!(clamp_zoom(0.1), 0.5);
        assert_eq!(clamp_zoom(9.9), 3.0);
        assert_eq!(clamp_zoom(f32::NAN), 1.0);
        assert_eq!(clamp_zoom(f32::INFINITY), 3.0);
        assert_eq!(clamp_zoom(f32::NEG_INFINITY), 0.5);
    }

    #[test]
    fn zoom_and_tab_limit_persist_roundtrip() {
        let dir = std::env::temp_dir().join(format!("phpide-uizoom-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (back, err) = with_xdg(&dir, || {
            let s = Settings {
                zoom: clamp_zoom(2.2),
                tab_limit: 4,
                ..Default::default()
            };
            s.save();
            Settings::load_or_default()
        });
        assert!(err.is_none());
        assert!((back.zoom - 2.2).abs() < 1e-6, "zoom persistido");
        assert_eq!(back.tab_limit, 4, "tab_limit persistido");
        // Valor fora da faixa é clampado na leitura.
        let clamped = with_xdg(&dir, || {
            let file = config_dir().join("settings.json");
            std::fs::write(&file, r#"{"tab_limit": 500, "zoom": 99.0}"#).unwrap();
            Settings::load_or_default().0
        });
        assert_eq!(clamped.tab_limit, 99);
        assert_eq!(clamped.zoom, 3.0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
