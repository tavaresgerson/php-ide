//! Suporte à linguagem PHP.
//!
//! Por ora: metadados da linguagem (extensões, comentários) e highlight de sintaxe.
//! Nos próximos estágios entram tree-sitter (estrutura/outline) e LSP (semântica):
//! - Estágio 3: cliente LSP rodando `intelephense` (Node) ou `phpactor` (PHP) como processo externo.
//! - Estágio 3: parser tree-sitter-php para dobramento e navegação de estruturas.

use std::path::Path;
use std::sync::OnceLock;

use eframe::egui::{self, Color32, Stroke, TextFormat};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

pub struct Php;

impl Php {
    pub const NAME: &str = "PHP";
    pub const EXTENSIONS: [&str; 3] = ["php", "phtml", "inc"];
    pub const LINE_COMMENT: &str = "//";
    pub const BLOCK_COMMENT: (&str, &str) = ("/*", "*/");

    pub fn is_php_file(path: &Path) -> bool {
        path.extension()
            .map(|e| {
                let e = e.to_string_lossy().to_ascii_lowercase();
                Self::EXTENSIONS.contains(&e.as_str())
            })
            .unwrap_or(false)
    }
}

fn syntax_set() -> &'static SyntaxSet {
    static SS: OnceLock<SyntaxSet> = OnceLock::new();
    SS.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// Semantic role of a highlighted fragment. The UI maps each role to a
/// color of the active theme, so highlighting follows Dark/Light/High
/// Contrast without a separate syntect theme per palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyntaxRole {
    Plain,
    Keyword,
    Type,
    Function,
    Method,
    Variable,
    Parameter,
    Property,
    String,
    Number,
    Comment,
    Documentation,
    Operator,
    Punctuation,
    Attribute,
    Constant,
    Namespace,
    Tag,
    Invalid,
    Deprecated,
}

impl SyntaxRole {
    pub const COUNT: usize = 20;
    pub const ALL: [SyntaxRole; Self::COUNT] = [
        SyntaxRole::Plain,
        SyntaxRole::Keyword,
        SyntaxRole::Type,
        SyntaxRole::Function,
        SyntaxRole::Method,
        SyntaxRole::Variable,
        SyntaxRole::Parameter,
        SyntaxRole::Property,
        SyntaxRole::String,
        SyntaxRole::Number,
        SyntaxRole::Comment,
        SyntaxRole::Documentation,
        SyntaxRole::Operator,
        SyntaxRole::Punctuation,
        SyntaxRole::Attribute,
        SyntaxRole::Constant,
        SyntaxRole::Namespace,
        SyntaxRole::Tag,
        SyntaxRole::Invalid,
        SyntaxRole::Deprecated,
    ];

    /// Role of an LSP semantic token type, when it has one.
    pub fn from_semantic_token(token_type: &str) -> Option<SyntaxRole> {
        Some(match token_type {
            "class" | "interface" | "trait" | "enum" | "type" | "typeParameter" => SyntaxRole::Type,
            "namespace" => SyntaxRole::Namespace,
            "function" | "macro" => SyntaxRole::Function,
            "method" => SyntaxRole::Method,
            "property" => SyntaxRole::Property,
            "enumMember" => SyntaxRole::Constant,
            "parameter" => SyntaxRole::Parameter,
            "variable" => SyntaxRole::Variable,
            "keyword" | "modifier" => SyntaxRole::Keyword,
            "number" => SyntaxRole::Number,
            "operator" => SyntaxRole::Operator,
            "decorator" => SyntaxRole::Attribute,
            _ => return None,
        })
    }
}

/// Colors and font used to build the layout job.
#[derive(Debug, Clone, PartialEq)]
pub struct SyntaxStyle {
    pub font: egui::FontId,
    pub colors: [Color32; SyntaxRole::COUNT],
}

impl SyntaxStyle {
    /// One color for every role (benchmarks and plain-text fallbacks).
    pub fn uniform(font: egui::FontId, color: Color32) -> Self {
        SyntaxStyle {
            font,
            colors: [color; SyntaxRole::COUNT],
        }
    }

    pub fn color(&self, role: SyntaxRole) -> Color32 {
        self.colors[role as usize]
    }
}

/// TextMate scope selectors per role. syntect resolves the most specific
/// match, so `comment.block.documentation` beats `comment`.
const ROLE_SCOPES: &[(SyntaxRole, &str)] = &[
    (SyntaxRole::Comment, "comment, punctuation.definition.comment"),
    (SyntaxRole::Documentation, "comment.block.documentation, comment.block.documentation punctuation.definition.comment"),
    (SyntaxRole::Keyword, "keyword, storage.type, storage.modifier, constant.language, variable.language, variable.language punctuation.definition.variable, punctuation.section.embedded, support.function.construct"),
    (SyntaxRole::Operator, "keyword.operator, punctuation.accessor"),
    (SyntaxRole::String, "string, string punctuation.definition.string"),
    (SyntaxRole::Constant, "constant.character.escape, constant.other, support.constant, variable.other.constant, entity.name.constant"),
    (SyntaxRole::Number, "constant.numeric"),
    (SyntaxRole::Variable, "variable, variable.other, punctuation.definition.variable"),
    (SyntaxRole::Parameter, "variable.parameter, variable.parameter punctuation.definition.variable"),
    (SyntaxRole::Property, "variable.other.property, variable.other.member"),
    (SyntaxRole::Function, "entity.name.function, support.function, variable.function"),
    (SyntaxRole::Method, "meta.method-call variable.function, meta.function-call.method"),
    (SyntaxRole::Type, "entity.name.class, entity.name.type, support.class, entity.other.inherited-class, support.type"),
    (SyntaxRole::Namespace, "entity.name.namespace, support.other.namespace, meta.path"),
    (SyntaxRole::Attribute, "meta.attribute, entity.other.attribute-name"),
    (SyntaxRole::Tag, "entity.name.tag"),
    (SyntaxRole::Punctuation, "punctuation.separator, punctuation.terminator, punctuation.section.block, punctuation.section.group, punctuation.section.brackets"),
    (SyntaxRole::Invalid, "invalid"),
    (SyntaxRole::Deprecated, "invalid.deprecated"),
];

/// A syntect theme whose foreground red channel encodes the [`SyntaxRole`].
fn role_theme() -> &'static Theme {
    use std::str::FromStr;
    use syntect::highlighting::{Color, ScopeSelectors, StyleModifier, ThemeItem, ThemeSettings};
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(|| {
        let encode = |role: SyntaxRole| Color {
            r: role as u8,
            g: 0,
            b: 0,
            a: 0xFF,
        };
        let scopes = ROLE_SCOPES
            .iter()
            .map(|(role, selectors)| ThemeItem {
                scope: ScopeSelectors::from_str(selectors).expect("valid scope selectors"),
                style: StyleModifier {
                    foreground: Some(encode(*role)),
                    background: None,
                    font_style: matches!(role, SyntaxRole::Comment | SyntaxRole::Documentation)
                        .then_some(FontStyle::ITALIC)
                        .or(Some(FontStyle::empty())),
                },
            })
            .collect();
        Theme {
            name: Some("phpide-roles".into()),
            author: None,
            settings: ThemeSettings {
                foreground: Some(encode(SyntaxRole::Plain)),
                ..ThemeSettings::default()
            },
            scopes,
        }
    })
}

fn php_syntax(ss: &SyntaxSet) -> &SyntaxReference {
    ss.find_syntax_by_extension("php")
        .or_else(|| ss.find_syntax_by_name("PHP"))
        .unwrap_or_else(|| ss.find_syntax_plain_text())
}

/// Role of a syntect style produced with [`role_theme`].
fn role_of(style: syntect::highlighting::Style) -> SyntaxRole {
    SyntaxRole::ALL
        .get(style.foreground.r as usize)
        .copied()
        .unwrap_or(SyntaxRole::Plain)
}

fn to_text_format(style: syntect::highlighting::Style, syntax: &SyntaxStyle) -> TextFormat {
    let role = role_of(style);
    let color = syntax.color(role);
    TextFormat {
        font_id: syntax.font.clone(),
        color,
        italics: style.font_style.contains(FontStyle::ITALIC),
        underline: if role == SyntaxRole::Invalid {
            Stroke::new(1.0_f32, color)
        } else {
            Stroke::NONE
        },
        strikethrough: if role == SyntaxRole::Deprecated {
            Stroke::new(1.0_f32, color)
        } else {
            Stroke::NONE
        },
        ..Default::default()
    }
}

/// Builds a highlighted `LayoutJob` for the whole source.
pub fn highlight(code: &str, syntax: &SyntaxStyle) -> egui::text::LayoutJob {
    let ss = syntax_set();
    let mut job = egui::text::LayoutJob::default();
    let mut highlighter = HighlightLines::new(php_syntax(ss), role_theme());
    for line in LinesWithEndings::from(code) {
        match highlighter.highlight_line(line, ss) {
            Ok(regions) => {
                for (style, text) in regions {
                    job.append(text, 0.0, to_text_format(style, syntax));
                }
            }
            Err(_) => {
                job.append(
                    line,
                    0.0,
                    TextFormat::simple(syntax.font.clone(), syntax.color(SyntaxRole::Plain)),
                );
            }
        }
    }
    job
}

/// Role of every highlighted fragment, in order (tests and tooling).
pub fn highlight_roles(code: &str) -> Vec<(String, SyntaxRole)> {
    let ss = syntax_set();
    let mut highlighter = HighlightLines::new(php_syntax(ss), role_theme());
    let mut out = Vec::new();
    for line in LinesWithEndings::from(code) {
        if let Ok(regions) = highlighter.highlight_line(line, ss) {
            out.extend(
                regions
                    .into_iter()
                    .map(|(style, text)| (text.to_string(), role_of(style))),
            );
        }
    }
    out
}

/// Trait de linguagem — no Estágio 3 cada linguagem ganha parser, LSP e snippets aqui.
pub trait Language {
    fn name(&self) -> &'static str;
    fn matches(&self, path: &Path) -> bool;
    fn highlight(&self, code: &str, style: &SyntaxStyle) -> egui::text::LayoutJob;
}

pub struct PhpLanguage;

impl Language for PhpLanguage {
    fn name(&self) -> &'static str {
        Php::NAME
    }

    fn matches(&self, path: &Path) -> bool {
        Php::is_php_file(path)
    }

    fn highlight(&self, code: &str, style: &SyntaxStyle) -> egui::text::LayoutJob {
        highlight(code, style)
    }
}

/// Estrutura que um outline provider devolve no Estágio 3 (classe/função + posição).
#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    /// Linha 0-based.
    pub line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Class,
    Interface,
    Trait,
    Enum,
    Function,
    Method,
    Property,
    Constant,
}

impl SymbolKind {
    pub fn label(self) -> &'static str {
        match self {
            SymbolKind::Class => "classe",
            SymbolKind::Interface => "interface",
            SymbolKind::Trait => "trait",
            SymbolKind::Enum => "enum",
            SymbolKind::Function => "função",
            SymbolKind::Method => "método",
            SymbolKind::Property => "propriedade",
            SymbolKind::Constant => "constante",
        }
    }
}

pub mod composer;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_php_files() {
        assert!(Php::is_php_file(Path::new("src/index.php")));
        assert!(Php::is_php_file(Path::new("view.phtml")));
        assert!(!Php::is_php_file(Path::new("README.md")));
        assert!(!Php::is_php_file(Path::new("Makefile")));
    }

    #[test]
    fn scopes_map_to_roles() {
        let roles = highlight_roles(
            "<?php\n/** Doc */\n// note\nnamespace App;\nfinal class User extends Model {\n    public function name(int $id): string { return \"x\" . 42 . $this->id; }\n}\n",
        );
        let role = |text: &str| {
            roles
                .iter()
                .find(|(t, _)| t.trim() == text)
                .map(|(_, r)| *r)
                .unwrap_or_else(|| panic!("no fragment {text:?} in {roles:?}"))
        };
        assert_eq!(role("//"), SyntaxRole::Comment);
        assert_eq!(role("/*"), SyntaxRole::Comment);
        assert_eq!(role("->"), SyntaxRole::Operator);
        assert_eq!(role("this"), SyntaxRole::Keyword);
        assert_eq!(role("class"), SyntaxRole::Keyword);
        assert_eq!(role("User"), SyntaxRole::Type);
        assert_eq!(role("name"), SyntaxRole::Function);
        assert_eq!(role("42"), SyntaxRole::Number);
        assert_eq!(role("return"), SyntaxRole::Keyword);
        assert!(roles.iter().any(|(_, r)| *r == SyntaxRole::String));
        let dollar_before_id = roles.windows(2).find(|w| w[0].0 == "$" && w[1].0 == "id");
        assert_eq!(
            dollar_before_id.map(|w| w[0].1),
            Some(SyntaxRole::Parameter)
        );
    }

    #[test]
    fn highlight_uses_the_style_colors() {
        let mut style = SyntaxStyle::uniform(egui::FontId::monospace(13.0), Color32::WHITE);
        style.colors[SyntaxRole::Keyword as usize] = Color32::RED;
        let job = highlight("<?php return 1;", &style);
        assert!(job.sections.iter().any(|s| s.format.color == Color32::RED));
        assert!(job.sections.iter().all(|s| s.format.font_id.size == 13.0));
    }

    #[test]
    fn symbol_kind_labels() {
        assert_eq!(SymbolKind::Class.label(), "classe");
        assert_eq!(SymbolKind::Function.label(), "função");
    }
}
