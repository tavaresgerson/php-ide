//! User changes to the editor color scheme (RF-HL-10, RF-HL-11).
//!
//! Each theme has its own scheme. "Language Defaults" changes a role for
//! every language; a language section changes it for files of that type
//! only, and an element left on "inherit" follows Language Defaults. Saved
//! in the settings as
//! `{ "<theme>": { "defaults" | "<file type id>": { "<role>": {...} } } }`.

use eframe::egui::Color32;
use phpide_php::{RoleAttributes, SyntaxRole, SyntaxStyle};
use serde_json::{json, Map, Value};

use crate::design::tokens::{hex_color_string, parse_hex_color};

/// Scope of the language defaults.
pub const DEFAULTS: &str = "defaults";

/// Every role shown in the Color Scheme page, with its settings key.
pub const ROLES: &[(SyntaxRole, &str, &str)] = &[
    (SyntaxRole::Keyword, "keyword", "Keyword"),
    (SyntaxRole::String, "string", "String"),
    (SyntaxRole::Number, "number", "Number"),
    (SyntaxRole::Comment, "comment", "Line comment"),
    (SyntaxRole::Documentation, "doc", "Doc comment"),
    (SyntaxRole::Variable, "variable", "Variable"),
    (SyntaxRole::Parameter, "parameter", "Parameter"),
    (SyntaxRole::Property, "property", "Property / key"),
    (SyntaxRole::Function, "function", "Function"),
    (SyntaxRole::Method, "method", "Method"),
    (SyntaxRole::Type, "type", "Class / type"),
    (SyntaxRole::Constant, "constant", "Constant"),
    (SyntaxRole::Namespace, "namespace", "Namespace"),
    (SyntaxRole::Attribute, "attribute", "Attribute / annotation"),
    (SyntaxRole::Operator, "operator", "Operator"),
    (
        SyntaxRole::Punctuation,
        "punctuation",
        "Braces and punctuation",
    ),
    (SyntaxRole::Tag, "tag", "Markup tag"),
    (SyntaxRole::Invalid, "invalid", "Invalid code"),
    (SyntaxRole::Deprecated, "deprecated", "Deprecated symbol"),
    (SyntaxRole::Plain, "plain", "Default text"),
];

/// Languages with a section of their own: `(file type id, name)`.
pub fn languages() -> Vec<(&'static str, &'static str)> {
    phpide_php::filetype::FILE_TYPES
        .iter()
        .filter(|t| t.highlighting != phpide_php::filetype::Highlighting::None)
        .map(|t| (t.id, t.name))
        .collect()
}

fn key_of(role: SyntaxRole) -> &'static str {
    ROLES
        .iter()
        .find(|(r, _, _)| *r == role)
        .map(|(_, key, _)| *key)
        .unwrap_or("plain")
}

/// Text decoration of an element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Underline,
    Strikethrough,
}

/// One element as the user set it. `None` colors mean "the default".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Element {
    pub foreground: Option<Color32>,
    pub background: Option<Color32>,
    pub bold: bool,
    pub italic: bool,
    pub effect: Option<(Effect, Option<Color32>)>,
}

impl Element {
    /// The element as the resolved style shows it.
    pub fn of(style: &SyntaxStyle, role: SyntaxRole) -> Element {
        let attrs = style.attributes(role);
        let effect = attrs
            .underline
            .map(|c| (Effect::Underline, Some(c)))
            .or(attrs
                .strikethrough
                .map(|c| (Effect::Strikethrough, Some(c))));
        Element {
            foreground: Some(style.color(role)),
            background: attrs.background,
            bold: attrs.bold,
            italic: attrs.italic,
            effect,
        }
    }

    fn to_json(self) -> Value {
        let color = |c: Option<Color32>| c.map(hex_color_string);
        json!({
            "fg": color(self.foreground),
            "bg": color(self.background),
            "bold": self.bold,
            "italic": self.italic,
            "effect": self.effect.map(|(e, _)| match e {
                Effect::Underline => "underline",
                Effect::Strikethrough => "strikethrough",
            }),
            "effect_color": self.effect.and_then(|(_, c)| c).map(hex_color_string),
        })
    }

    fn from_json(value: &Value) -> Element {
        let color = |key: &str| value[key].as_str().and_then(parse_hex_color);
        let effect = match value["effect"].as_str() {
            Some("underline") => Some((Effect::Underline, color("effect_color"))),
            Some("strikethrough") => Some((Effect::Strikethrough, color("effect_color"))),
            _ => None,
        };
        Element {
            foreground: color("fg"),
            background: color("bg"),
            bold: value["bold"].as_bool().unwrap_or(false),
            italic: value["italic"].as_bool().unwrap_or(false),
            effect,
        }
    }

    fn apply(self, style: &mut SyntaxStyle, role: SyntaxRole) {
        let i = role as usize;
        if let Some(fg) = self.foreground {
            style.colors[i] = fg;
        }
        let text = style.colors[i];
        style.attributes[i] = RoleAttributes {
            background: self.background,
            bold: self.bold,
            italic: self.italic,
            underline: match self.effect {
                Some((Effect::Underline, color)) => Some(color.unwrap_or(text)),
                _ => None,
            },
            strikethrough: match self.effect {
                Some((Effect::Strikethrough, color)) => Some(color.unwrap_or(text)),
                _ => None,
            },
        };
    }
}

/// The user's changes for one theme, read from / written to the settings.
pub struct SchemeChanges<'a> {
    value: &'a mut Value,
    theme: String,
}

impl<'a> SchemeChanges<'a> {
    pub fn new(value: &'a mut Value, theme: &str) -> Self {
        if !value.is_object() {
            *value = Value::Object(Map::new());
        }
        SchemeChanges {
            value,
            theme: theme.to_string(),
        }
    }

    /// The user's element for `scope` (`DEFAULTS` or a language), if any.
    pub fn get(&self, scope: &str, role: SyntaxRole) -> Option<Element> {
        let element = &self.value[&self.theme][scope][key_of(role)];
        element.is_object().then(|| Element::from_json(element))
    }

    pub fn set(&mut self, scope: &str, role: SyntaxRole, element: Option<Element>) {
        let theme = self
            .value
            .as_object_mut()
            .expect("object")
            .entry(self.theme.clone())
            .or_insert_with(|| Value::Object(Map::new()));
        let scope_map = theme
            .as_object_mut()
            .expect("object")
            .entry(scope.to_string())
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .expect("object");
        match element {
            Some(element) => {
                scope_map.insert(key_of(role).to_string(), element.to_json());
            }
            None => {
                scope_map.remove(key_of(role));
            }
        }
    }

    /// Drops every change of this theme.
    pub fn reset(&mut self) {
        if let Some(map) = self.value.as_object_mut() {
            map.remove(&self.theme);
        }
    }
}

// Named schemes (RF-HL-12). A scheme is built on one theme; the theme's
// own scheme is keyed by the theme name. Duplicates live next to it:
// `"_schemes": { "<name>": "<base theme>" }`, and the scheme each theme
// uses is `"_active": { "<theme>": "<name>" }`.
const SCHEMES: &str = "_schemes";
const ACTIVE: &str = "_active";
/// Format tag of an exported scheme file.
pub const EXPORT_FORMAT: &str = "phpide-color-scheme";

fn object<'a>(value: &'a mut Value, key: &str) -> &'a mut Map<String, Value> {
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    let entry = value
        .as_object_mut()
        .expect("object")
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    entry.as_object_mut().expect("object")
}

/// The user's own schemes: `(name, base theme name)`, sorted by name.
pub fn custom_schemes(value: &Value) -> Vec<(String, String)> {
    let mut list: Vec<(String, String)> = value[SCHEMES]
        .as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(name, base)| Some((name.clone(), base.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default();
    list.sort();
    list
}

/// Key of the scheme `theme` uses: one of the user's built on it, or the
/// theme's own.
pub fn active_scheme(value: &Value, theme: &str) -> String {
    let chosen = value[ACTIVE][theme].as_str().unwrap_or(theme);
    if value[SCHEMES][chosen].as_str() == Some(theme) {
        chosen.to_string()
    } else {
        theme.to_string()
    }
}

/// Uses `scheme` (built on `theme`) for that theme.
pub fn set_active(value: &mut Value, theme: &str, scheme: &str) {
    object(value, ACTIVE).insert(theme.to_string(), json!(scheme));
}

/// Why `name` cannot name a new scheme, if it cannot.
pub fn invalid_name(value: &Value, name: &str) -> Option<&'static str> {
    let name = name.trim();
    if name.is_empty() {
        return Some("Enter a name");
    }
    if name.starts_with('_') {
        return Some("A name cannot start with _");
    }
    let builtin = crate::design::ThemeId::ALL
        .iter()
        .any(|t| t.name().eq_ignore_ascii_case(name) || t.label().eq_ignore_ascii_case(name));
    if builtin || value[SCHEMES].get(name).is_some() {
        return Some("A scheme with this name exists");
    }
    None
}

/// Copies scheme `from` (a theme name or one of the user's) as `name`,
/// built on `base`, and makes it the scheme of that theme.
pub fn duplicate(value: &mut Value, from: &str, name: &str, base: &str) {
    let changes = value[from].clone();
    let changes = if changes.is_object() {
        changes
    } else {
        json!({})
    };
    object(value, SCHEMES).insert(name.to_string(), json!(base));
    value
        .as_object_mut()
        .expect("object")
        .insert(name.to_string(), changes);
    set_active(value, base, name);
}

pub fn rename(value: &mut Value, old: &str, new: &str) {
    let Some(base) = value[SCHEMES][old].as_str().map(str::to_string) else {
        return;
    };
    let changes = value
        .as_object_mut()
        .and_then(|m| m.remove(old))
        .unwrap_or(json!({}));
    object(value, SCHEMES).remove(old);
    object(value, SCHEMES).insert(new.to_string(), json!(base));
    value
        .as_object_mut()
        .expect("object")
        .insert(new.to_string(), changes);
    if value[ACTIVE][&base].as_str() == Some(old) {
        set_active(value, &base, new);
    }
}

/// Deletes one of the user's schemes; its theme goes back to its own.
pub fn delete(value: &mut Value, name: &str) {
    let base = value[SCHEMES][name].as_str().map(str::to_string);
    object(value, SCHEMES).remove(name);
    if let Some(map) = value.as_object_mut() {
        map.remove(name);
    }
    if let Some(base) = base {
        if value[ACTIVE][&base].as_str() == Some(name) {
            object(value, ACTIVE).remove(&base);
        }
    }
}

/// A scheme as a file: its name, base theme and changes.
pub fn export(value: &Value, key: &str, name: &str, base: &str) -> Value {
    let changes = if value[key].is_object() {
        value[key].clone()
    } else {
        json!({})
    };
    json!({ "format": EXPORT_FORMAT, "version": 1, "name": name, "base": base, "changes": changes })
}

/// Adds an exported scheme; a taken name gets a number. Returns the name
/// and the base theme.
pub fn import(value: &mut Value, file: &Value) -> Result<(String, String), String> {
    if file["format"].as_str() != Some(EXPORT_FORMAT) {
        return Err("not a PHP IDE color scheme file".to_string());
    }
    let base = file["base"].as_str().unwrap_or("dark");
    let base = crate::design::ThemeId::from_name(base).name().to_string();
    let wanted = file["name"].as_str().unwrap_or("Imported").trim();
    let wanted = if wanted.is_empty() || wanted.starts_with('_') {
        "Imported"
    } else {
        wanted
    };
    let mut name = wanted.to_string();
    let mut n = 2;
    while invalid_name(value, &name).is_some() {
        name = format!("{wanted} ({n})");
        n += 1;
    }
    let changes = if file["changes"].is_object() {
        file["changes"].clone()
    } else {
        json!({})
    };
    object(value, SCHEMES).insert(name.clone(), json!(base));
    value
        .as_object_mut()
        .expect("object")
        .insert(name.clone(), changes);
    set_active(value, &base, &name);
    Ok((name, base))
}

/// The style of a language: the theme, then the user's Language Defaults,
/// then the user's changes for that language.
pub fn resolve(
    base: &SyntaxStyle,
    changes: &Value,
    theme: &str,
    language: Option<&str>,
) -> SyntaxStyle {
    let mut style = base.clone();
    let scheme = &changes[&active_scheme(changes, theme)];
    if !scheme.is_object() {
        return style;
    }
    for scope in [Some(DEFAULTS), language] {
        let Some(scope) = scope else {
            continue;
        };
        for (role, key, _) in ROLES {
            let element = &scheme[scope][*key];
            if element.is_object() {
                Element::from_json(element).apply(&mut style, *role);
            }
        }
    }
    style
}

/// Code shown in the preview of the Color Scheme page.
pub fn sample(language: &str) -> (&'static str, &'static str) {
    match language {
        "html" | "blade" => (
            "sample.html",
            "<!-- Navigation -->\n<nav class=\"menu\" id=\"main\">\n    <a href=\"/home\">Home</a>\n</nav>\n",
        ),
        "javascript" | "typescript" => (
            "sample.js",
            "// Fetch the orders\nconst limit = 20;\nasync function load(id) {\n    return fetch(`/api/${id}`).then(r => r.json());\n}\n",
        ),
        "css" => (
            "sample.css",
            "/* Layout */\n.menu > a:hover {\n    color: #3b6fd8;\n    margin: 0 4px;\n}\n",
        ),
        "json" => (
            "sample.json",
            "{\n    \"name\": \"app/shop\",\n    \"require\": { \"php\": \">=8.2\" },\n    \"stable\": true\n}\n",
        ),
        "yaml" => (
            "sample.yaml",
            "# Services\nparameters:\n    locale: 'en'\n    retries: 3\n",
        ),
        "markdown" => (
            "sample.md",
            "# Shop\n\nRun `composer install`, then **start** the server.\n\n- [Docs](https://example.org)\n",
        ),
        "sql" => (
            "sample.sql",
            "-- Recent orders\nSELECT id, total FROM orders\nWHERE created_at > NOW() - INTERVAL 7 DAY\nORDER BY total DESC;\n",
        ),
        "dotenv" => (
            ".env",
            "# Application\nAPP_ENV=local\nAPP_DEBUG=true\nDB_PASSWORD='secret'\n",
        ),
        "ini" | "toml" => (
            "sample.ini",
            "; PHP settings\n[opcache]\nopcache.enable=1\nmemory_limit = \"512M\"\n",
        ),
        "dockerfile" => (
            "Dockerfile",
            "# Runtime\nFROM php:8.3-fpm AS base\nWORKDIR /app\nCMD [\"php-fpm\"]\n",
        ),
        "xml" => (
            "sample.xml",
            "<?xml version=\"1.0\"?>\n<!-- Test suites -->\n<phpunit colors=\"true\">\n    <testsuite name=\"unit\"/>\n</phpunit>\n",
        ),
        "shell" => (
            "sample.sh",
            "#!/bin/bash\n# Deploy\nfor host in web1 web2; do\n    echo \"Deploying $host\"\ndone\n",
        ),
        _ => (
            "Sample.php",
            "<?php\n\nnamespace App\\Billing;\n\n/** Builds invoices. @deprecated */\n#[Service]\nfinal class Invoice extends Document\n{\n    private const RATE = 0.15;\n\n    public function total(int $count, string $label = 'net'): float\n    {\n        // Tax included\n        return $this->amount * self::RATE + strlen($label) / $count;\n    }\n}\n",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::FontId;

    #[test]
    fn schemes_are_duplicated_renamed_exported_imported_and_deleted() {
        let mut value = json!({});
        SchemeChanges::new(&mut value, "dark").set(
            DEFAULTS,
            SyntaxRole::Keyword,
            Some(Element {
                foreground: Some(Color32::RED),
                background: None,
                bold: false,
                italic: false,
                effect: None,
            }),
        );
        assert_eq!(active_scheme(&value, "dark"), "dark");
        assert!(invalid_name(&value, "Dark").is_some());
        duplicate(&mut value, "dark", "Mine", "dark");
        assert_eq!(active_scheme(&value, "dark"), "Mine");
        assert_eq!(custom_schemes(&value), vec![("Mine".into(), "dark".into())]);
        // The copy starts with the theme's changes and changes on its own.
        SchemeChanges::new(&mut value, "Mine").set(DEFAULTS, SyntaxRole::Keyword, None);
        assert!(SchemeChanges::new(&mut value, "dark")
            .get(DEFAULTS, SyntaxRole::Keyword)
            .is_some());
        rename(&mut value, "Mine", "Ours");
        assert_eq!(active_scheme(&value, "dark"), "Ours");
        let file = export(&value, "Ours", "Ours", "dark");
        let (name, base) = import(&mut value, &file).unwrap();
        assert_eq!((name.as_str(), base.as_str()), ("Ours (2)", "dark"));
        assert_eq!(active_scheme(&value, "dark"), "Ours (2)");
        assert!(import(&mut value, &json!({"name": "x"})).is_err());
        delete(&mut value, "Ours (2)");
        assert_eq!(active_scheme(&value, "dark"), "dark");
        assert_eq!(custom_schemes(&value).len(), 1);
        // A scheme does not leak into a theme it is not built on.
        set_active(&mut value, "light", "Ours");
        assert_eq!(active_scheme(&value, "light"), "light");
    }

    #[test]
    fn languages_inherit_defaults_and_override_them() {
        let base = SyntaxStyle::uniform(FontId::monospace(13.0), Color32::GRAY);
        let mut value = Value::Null;
        let red = parse_hex_color("#FF0000").unwrap();
        let blue = parse_hex_color("#0000FF").unwrap();
        {
            let mut changes = SchemeChanges::new(&mut value, "dark");
            changes.set(
                DEFAULTS,
                SyntaxRole::Keyword,
                Some(Element {
                    foreground: Some(red),
                    background: None,
                    bold: true,
                    italic: false,
                    effect: None,
                }),
            );
            changes.set(
                "php",
                SyntaxRole::Keyword,
                Some(Element {
                    foreground: Some(blue),
                    background: None,
                    bold: false,
                    italic: true,
                    effect: Some((Effect::Underline, None)),
                }),
            );
            assert!(changes.get("php", SyntaxRole::Keyword).is_some());
            assert!(changes.get("css", SyntaxRole::Keyword).is_none());
        }
        let css = resolve(&base, &value, "dark", Some("css"));
        assert_eq!(css.color(SyntaxRole::Keyword), red);
        assert!(css.attributes(SyntaxRole::Keyword).bold);
        let php = resolve(&base, &value, "dark", Some("php"));
        assert_eq!(php.color(SyntaxRole::Keyword), blue);
        let attrs = php.attributes(SyntaxRole::Keyword);
        assert!(attrs.italic && !attrs.bold);
        assert_eq!(
            attrs.underline,
            Some(blue),
            "the effect takes the text color"
        );
        let light = resolve(&base, &value, "light", Some("php"));
        assert_eq!(light, base, "each theme has its own scheme");

        // Back to inherit; reset clears the theme.
        SchemeChanges::new(&mut value, "dark").set("php", SyntaxRole::Keyword, None);
        assert_eq!(
            resolve(&base, &value, "dark", Some("php")).color(SyntaxRole::Keyword),
            red
        );
        SchemeChanges::new(&mut value, "dark").reset();
        assert_eq!(resolve(&base, &value, "dark", Some("php")), base);
    }
}
