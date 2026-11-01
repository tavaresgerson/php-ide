//! Plugins por script (Estágio 5).
//!
//! Um plugin é uma pasta com um `plugin.json`:
//!
//! ```json
//! {
//!   "name": "pint",
//!   "commands": [
//!     { "id": "fmt", "label": "Formatar com Pint",
//!       "command": "vendor/bin/pint", "args": ["--dirty"] }
//!   ]
//! }
//! ```
//!
//! Pastas varridas: `<projeto>/.phpide/plugins/*/plugin.json` e
//! `~/.config/phpide/plugins/*/plugin.json`. Comandos aparecem na palette
//! (Ctrl+P) como "Plugin <nome>: <label>" e rodam no painel Execução.
//! Plugins WASM (estilo Lapce) ficam para o futuro — scripts cobrem a maior
//! parte das automações de projeto.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PluginCommand {
    /// Identificador único: "plugin:<nome>:<cmd>".
    pub id: String,
    pub label: String,
    pub command: String,
    pub args: Vec<String>,
}

pub fn discover_plugins(project: &Path, config_dir: &Path) -> Vec<PluginCommand> {
    let mut out = Vec::new();
    let bases = [
        project.join(".phpide/plugins"),
        PathBuf::from(config_dir).join("plugins"),
    ];
    for base in bases {
        let Ok(entries) = std::fs::read_dir(&base) else { continue };
        let mut dirs: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        dirs.sort();
        for dir in dirs {
            let Ok(text) = std::fs::read_to_string(dir.join("plugin.json")) else {
                continue;
            };
            let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            let name = json["name"].as_str().unwrap_or("plugin").to_string();
            let Some(cmds) = json["commands"].as_array() else { continue };
            for c in cmds {
                let (Some(cid), Some(command)) = (c["id"].as_str(), c["command"].as_str()) else {
                    continue;
                };
                let label = c["label"].as_str().unwrap_or(cid).to_string();
                let args = c["args"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                out.push(PluginCommand {
                    id: format!("plugin:{name}:{cid}"),
                    label: format!("Plugin {name}: {label}"),
                    command: command.to_string(),
                    args,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_manifest_plugin() {
        let root = std::env::temp_dir().join(format!("phpide-plugins-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".phpide/plugins/pint")).unwrap();
        std::fs::write(
            root.join(".phpide/plugins/pint/plugin.json"),
            r#"{ "name": "pint", "commands": [ { "id": "fmt", "label": "Formatar", "command": "vendor/bin/pint", "args": ["--dirty"] } ] }"#,
        )
        .unwrap();
        let found = discover_plugins(&root, &root.join("sem-config"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "plugin:pint:fmt");
        assert_eq!(found[0].label, "Plugin pint: Formatar");
        assert_eq!(found[0].args, vec!["--dirty"]);
        std::fs::remove_dir_all(&root).ok();
    }
}
