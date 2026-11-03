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
use syntect::highlighting::{FontStyle, Theme, ThemeSet};
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

fn dark_theme() -> &'static Theme {
    static TS: OnceLock<Theme> = OnceLock::new();
    TS.get_or_init(|| {
        ThemeSet::load_defaults()
            .themes
            .remove("base16-ocean.dark")
            .unwrap_or_else(|| {
                ThemeSet::load_defaults()
                    .themes
                    .values()
                    .next()
                    .unwrap()
                    .clone()
            })
    })
}

fn php_syntax<'a>(ss: &'a SyntaxSet) -> &'a SyntaxReference {
    ss.find_syntax_by_extension("php")
        .or_else(|| ss.find_syntax_by_name("PHP"))
        .unwrap_or_else(|| ss.find_syntax_plain_text())
}

/// Converte um estilo do syntect para o formato de texto do egui.
/// Bold é ignorado no Estágio 1 (egui não tem peso de fonte por fragmento sem
/// registrar uma família bold — entra no Estágio 2 com fontes dedicadas).
fn to_text_format(style: syntect::highlighting::Style) -> TextFormat {
    let fg = Color32::from_rgb(style.foreground.r, style.foreground.g, style.foreground.b);
    TextFormat {
        font_id: egui::FontId::monospace(14.0),
        color: fg,
        italics: style.font_style.contains(FontStyle::ITALIC),
        underline: if style.font_style.contains(FontStyle::UNDERLINE) {
            Stroke::new(1.0_f32, fg)
        } else {
            Stroke::NONE
        },
        ..Default::default()
    }
}

/// Gera um `LayoutJob` com highlight de PHP para o código-fonte inteiro.
///
/// O pipeline (lexer/grammar -> estilos -> layout) espelha o "annotator" dos IDEs
/// clássicos: aqui o syntect faz o papel do lexer+colorir, e a análise semântica
/// entra no Estágio 3 via LSP/tree-sitter.
pub fn highlight(code: &str, _egui_style: &egui::Style) -> egui::text::LayoutJob {
    let ss = syntax_set();
    let theme = dark_theme();
    let syntax = php_syntax(ss);

    let mut job = egui::text::LayoutJob::default();
    let mut highlighter = HighlightLines::new(syntax, theme);
    for line in LinesWithEndings::from(code) {
        match highlighter.highlight_line(line, ss) {
            Ok(regions) => {
                for (style, text) in regions {
                    let format = to_text_format(style);
                    job.append(text, 0.0, format);
                }
            }
            Err(_) => {
                job.append(line, 0.0, TextFormat::default());
            }
        }
    }
    job
}

/// Trait de linguagem — no Estágio 3 cada linguagem ganha parser, LSP e snippets aqui.
pub trait Language {
    fn name(&self) -> &'static str;
    fn matches(&self, path: &Path) -> bool;
    fn highlight(&self, code: &str, style: &egui::Style) -> egui::text::LayoutJob;
}

pub struct PhpLanguage;

impl Language for PhpLanguage {
    fn name(&self) -> &'static str {
        Php::NAME
    }

    fn matches(&self, path: &Path) -> bool {
        Php::is_php_file(path)
    }

    fn highlight(&self, code: &str, style: &egui::Style) -> egui::text::LayoutJob {
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
    fn symbol_kind_labels() {
        assert_eq!(SymbolKind::Class.label(), "classe");
        assert_eq!(SymbolKind::Function.label(), "função");
    }
}
