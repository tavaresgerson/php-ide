//! File types: which language service (highlighting, analysis) a file gets.
//!
//! A type is chosen by name pattern (`*.php`, `Dockerfile`, `.env.*`), then
//! by the first line (`#!/usr/bin/env php`), then by syntect's own
//! extension list. Patterns registered by the user and per-file overrides
//! are applied first by the caller ([`detect_with`]). Types without a
//! syntect grammar use [`SimpleLanguage`]: line comments, keywords,
//! strings, numbers and `key = value` lines, which covers INI, `.env`,
//! TOML, Dockerfile and ignore files.

use crate::{syntax_set, LineSpan, SyntaxRole};

/// How a file type is highlighted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Highlighting {
    /// tree-sitter-php (the editor's own engine).
    Php,
    /// A syntect grammar, by name.
    Syntect(&'static str),
    /// A built-in line-based language.
    Simple(&'static SimpleLanguage),
    /// No highlighting.
    None,
}

/// A registered file type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileType {
    /// Stable id, stored in settings (`"php"`, `"yaml"`).
    pub id: &'static str,
    /// Name shown to the user.
    pub name: &'static str,
    pub highlighting: Highlighting,
    /// Default name patterns: `*.ext`, an exact name, or `prefix*`.
    pub patterns: &'static [&'static str],
}

impl FileType {
    pub fn is_php(&self) -> bool {
        self.highlighting == Highlighting::Php
    }
}

/// Keywords, comments and literals of a language without a grammar.
#[derive(Debug, PartialEq, Eq)]
pub struct SimpleLanguage {
    pub line_comments: &'static [&'static str],
    pub keywords: &'static [&'static str],
    pub case_sensitive: bool,
    /// `key = value` / `key: value` lines: the key is a property.
    pub key_value: bool,
    /// `[section]` lines are types.
    pub sections: bool,
    pub strings: bool,
    pub numbers: bool,
}

const INI: SimpleLanguage = SimpleLanguage {
    line_comments: &["#", ";"],
    keywords: &["true", "false", "on", "off", "yes", "no", "null"],
    case_sensitive: false,
    key_value: true,
    sections: true,
    strings: true,
    numbers: true,
};

const TOML: SimpleLanguage = SimpleLanguage {
    line_comments: &["#"],
    keywords: &["true", "false"],
    case_sensitive: true,
    key_value: true,
    sections: true,
    strings: true,
    numbers: true,
};

const DOTENV: SimpleLanguage = SimpleLanguage {
    line_comments: &["#"],
    keywords: &["true", "false", "null", "export"],
    case_sensitive: false,
    key_value: true,
    sections: false,
    strings: true,
    numbers: true,
};

const DOCKERFILE: SimpleLanguage = SimpleLanguage {
    line_comments: &["#"],
    keywords: &[
        "FROM",
        "AS",
        "RUN",
        "CMD",
        "LABEL",
        "MAINTAINER",
        "EXPOSE",
        "ENV",
        "ADD",
        "COPY",
        "ENTRYPOINT",
        "VOLUME",
        "USER",
        "WORKDIR",
        "ARG",
        "ONBUILD",
        "STOPSIGNAL",
        "HEALTHCHECK",
        "SHELL",
    ],
    case_sensitive: false,
    key_value: false,
    sections: false,
    strings: true,
    numbers: false,
};

const IGNORE: SimpleLanguage = SimpleLanguage {
    line_comments: &["#"],
    keywords: &[],
    case_sensitive: true,
    key_value: false,
    sections: false,
    strings: false,
    numbers: false,
};

/// Every built-in type, most specific patterns first where they overlap
/// (`*.blade.php` before `*.php`).
pub const FILE_TYPES: &[FileType] = &[
    FileType {
        id: "blade",
        name: "Blade",
        highlighting: Highlighting::Syntect("HTML"),
        patterns: &["*.blade.php"],
    },
    FileType {
        id: "php",
        name: "PHP",
        highlighting: Highlighting::Php,
        patterns: &["*.php", "*.phtml", "*.inc", "*.phpt", "*.php8", "artisan"],
    },
    FileType {
        id: "html",
        name: "HTML",
        highlighting: Highlighting::Syntect("HTML"),
        patterns: &["*.html", "*.htm", "*.xhtml", "*.twig", "*.vue", "*.latte"],
    },
    FileType {
        id: "javascript",
        name: "JavaScript",
        highlighting: Highlighting::Syntect("JavaScript"),
        patterns: &["*.js", "*.mjs", "*.cjs", "*.jsx"],
    },
    FileType {
        id: "typescript",
        name: "TypeScript",
        highlighting: Highlighting::Syntect("JavaScript"),
        patterns: &["*.ts", "*.tsx", "*.mts", "*.cts"],
    },
    FileType {
        id: "css",
        name: "CSS",
        highlighting: Highlighting::Syntect("CSS"),
        patterns: &["*.css", "*.scss", "*.sass", "*.less", "*.pcss"],
    },
    FileType {
        id: "json",
        name: "JSON",
        highlighting: Highlighting::Syntect("JSON"),
        patterns: &[
            "*.json",
            "*.jsonc",
            "*.json5",
            "*.webmanifest",
            "composer.lock",
            ".babelrc",
            ".eslintrc",
            ".prettierrc",
        ],
    },
    FileType {
        id: "xml",
        name: "XML",
        highlighting: Highlighting::Syntect("XML"),
        patterns: &[
            "*.xml", "*.xsd", "*.xsl", "*.xslt", "*.svg", "*.wsdl", "*.xliff", "*.xlf",
        ],
    },
    FileType {
        id: "yaml",
        name: "YAML",
        highlighting: Highlighting::Syntect("YAML"),
        patterns: &["*.yml", "*.yaml", "*.neon"],
    },
    FileType {
        id: "markdown",
        name: "Markdown",
        highlighting: Highlighting::Syntect("Markdown"),
        patterns: &["*.md", "*.markdown", "*.mdx"],
    },
    FileType {
        id: "sql",
        name: "SQL",
        highlighting: Highlighting::Syntect("SQL"),
        patterns: &["*.sql", "*.ddl", "*.dml"],
    },
    FileType {
        id: "shell",
        name: "Shell Script",
        highlighting: Highlighting::Syntect("Bourne Again Shell (bash)"),
        patterns: &[
            "*.sh",
            "*.bash",
            "*.zsh",
            ".bashrc",
            ".bash_profile",
            ".profile",
            ".zshrc",
        ],
    },
    FileType {
        id: "makefile",
        name: "Makefile",
        highlighting: Highlighting::Syntect("Makefile"),
        patterns: &["Makefile", "makefile", "GNUmakefile", "*.mk"],
    },
    FileType {
        id: "dockerfile",
        name: "Dockerfile",
        highlighting: Highlighting::Simple(&DOCKERFILE),
        patterns: &[
            "Dockerfile",
            "Containerfile",
            "*.dockerfile",
            "Dockerfile.*",
        ],
    },
    FileType {
        id: "dotenv",
        name: ".env",
        highlighting: Highlighting::Simple(&DOTENV),
        patterns: &[".env", ".env.*", "*.env"],
    },
    FileType {
        id: "ini",
        name: "INI / Config",
        highlighting: Highlighting::Simple(&INI),
        patterns: &[
            "*.ini",
            "*.conf",
            "*.cfg",
            "*.properties",
            ".editorconfig",
            ".htaccess",
            ".npmrc",
            "php.ini-*",
        ],
    },
    FileType {
        id: "toml",
        name: "TOML",
        highlighting: Highlighting::Simple(&TOML),
        patterns: &["*.toml", "Cargo.lock"],
    },
    FileType {
        id: "ignore",
        name: "Ignore List",
        highlighting: Highlighting::Simple(&IGNORE),
        patterns: &[
            ".gitignore",
            ".dockerignore",
            ".gitattributes",
            ".npmignore",
            ".eslintignore",
            ".prettierignore",
            "*.gitignore",
        ],
    },
    FileType {
        id: "text",
        name: "Plain Text",
        highlighting: Highlighting::None,
        patterns: &[
            "*.txt",
            "*.log",
            "*.text",
            "LICENSE",
            "LICENCE",
            "README",
            "AUTHORS",
            "CHANGELOG",
        ],
    },
];

/// The type with this id.
pub fn by_id(id: &str) -> Option<&'static FileType> {
    FILE_TYPES.iter().find(|t| t.id == id)
}

/// The plain-text type.
pub fn plain_text() -> &'static FileType {
    by_id("text").expect("plain text type")
}

/// Whether `pattern` (`*.ext`, `name`, `prefix*`, `*.a.b`) matches `name`.
pub fn pattern_matches(pattern: &str, name: &str) -> bool {
    let pattern = pattern.to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    match (pattern.strip_prefix('*'), pattern.strip_suffix('*')) {
        (Some(suffix), _) => name.ends_with(suffix) && name.len() > suffix.len(),
        (None, Some(prefix)) => name.starts_with(prefix) && name.len() > prefix.len(),
        (None, None) => name == pattern,
    }
}

/// How specific a matching pattern is: longer literal parts win, exact
/// names win over wildcards.
fn specificity(pattern: &str) -> usize {
    let literal = pattern.trim_matches('*').len();
    if pattern.contains('*') {
        literal
    } else {
        literal + 1000
    }
}

/// The result of detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detected {
    /// A registered type.
    Known(&'static FileType),
    /// Recognized by syntect only (its grammar name).
    Grammar(String),
    /// Nothing matched: the user may want to pick a type.
    Unknown,
}

impl Detected {
    pub fn name(&self) -> &str {
        match self {
            Detected::Known(t) => t.name,
            Detected::Grammar(name) => name,
            Detected::Unknown => "Plain Text",
        }
    }

    pub fn is_php(&self) -> bool {
        matches!(self, Detected::Known(t) if t.is_php())
    }
}

/// Detects the type of `file_name` with its `first_line`. User patterns
/// (`pattern → type id`) are tried before the built-in ones.
pub fn detect_with(file_name: &str, first_line: &str, user: &[(String, String)]) -> Detected {
    let mut best: Option<(usize, &'static FileType)> = None;
    for (pattern, id) in user {
        if let Some(file_type) = by_id(id) {
            if pattern_matches(pattern, file_name) {
                // User patterns beat built-in ones of equal specificity.
                let score = specificity(pattern) + 1;
                if best.is_none_or(|(s, _)| score > s) {
                    best = Some((score, file_type));
                }
            }
        }
    }
    for file_type in FILE_TYPES {
        for pattern in file_type.patterns {
            if pattern_matches(pattern, file_name) {
                let score = specificity(pattern);
                if best.is_none_or(|(s, _)| score > s) {
                    best = Some((score, file_type));
                }
            }
        }
    }
    if let Some((_, file_type)) = best {
        return Detected::Known(file_type);
    }
    // `phpunit.xml.dist`, `.env.dist`, `phpstan.neon.dist`: the real type
    // is under the template suffix.
    for suffix in [".dist", ".example", ".sample", ".template", ".orig", ".bak"] {
        if let Some(stem) = file_name.strip_suffix(suffix) {
            if !stem.is_empty() {
                match detect_with(stem, first_line, user) {
                    Detected::Unknown => {}
                    found => return found,
                }
            }
        }
    }
    if let Some(found) = detect_by_first_line(first_line) {
        return found;
    }
    let ss = syntax_set();
    let extension = file_name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    if !extension.is_empty() {
        if let Some(syntax) = ss.find_syntax_by_extension(extension) {
            if syntax.name != "Plain Text" {
                return Detected::Grammar(syntax.name.clone());
            }
        }
    }
    Detected::Unknown
}

/// [`detect_with`] without user patterns.
pub fn detect(file_name: &str, first_line: &str) -> Detected {
    detect_with(file_name, first_line, &[])
}

fn detect_by_first_line(first_line: &str) -> Option<Detected> {
    let line = first_line.trim();
    if line.starts_with("<?php") {
        return by_id("php").map(Detected::Known);
    }
    if let Some(command) = line.strip_prefix("#!") {
        let program = command
            .split_whitespace()
            .filter(|word| *word != "/usr/bin/env" && !word.starts_with('-'))
            .map(|word| word.rsplit('/').next().unwrap_or(word))
            .next()
            .unwrap_or("");
        let id = match program {
            p if p.starts_with("php") => "php",
            "sh" | "bash" | "zsh" | "dash" | "ksh" => "shell",
            "node" | "nodejs" | "deno" | "bun" => "javascript",
            _ => "",
        };
        if let Some(file_type) = by_id(id) {
            return Some(Detected::Known(file_type));
        }
    }
    let ss = syntax_set();
    ss.find_syntax_by_first_line(first_line)
        .filter(|s| s.name != "Plain Text")
        .map(|s| Detected::Grammar(s.name.clone()))
}

impl SimpleLanguage {
    /// Spans of one line (char columns). Lines are independent: no
    /// construct spans lines in these languages.
    pub fn highlight_line(&self, line: &str) -> Vec<LineSpan> {
        let chars: Vec<char> = line.trim_end_matches(['\n', '\r']).chars().collect();
        let mut spans = Vec::new();
        let first = chars.iter().position(|c| !c.is_whitespace());
        let Some(first) = first else {
            return spans;
        };
        let rest: String = chars[first..].iter().collect();
        for prefix in self.line_comments {
            if rest.starts_with(prefix) {
                spans.push((first, chars.len(), SyntaxRole::Comment));
                return spans;
            }
        }
        if self.sections && chars[first] == '[' {
            spans.push((first, chars.len(), SyntaxRole::Type));
            return spans;
        }
        let mut i = first;
        if self.key_value {
            let separator = chars[first..]
                .iter()
                .position(|&c| c == '=' || c == ':')
                .map(|p| p + first);
            if let Some(sep) = separator {
                let key_end = chars[..sep]
                    .iter()
                    .rposition(|c| !c.is_whitespace())
                    .map_or(first, |p| p + 1);
                // `export KEY=value` in .env files.
                let mut key_start = first;
                let head: String = chars[first..key_end].iter().collect();
                if let Some(after) = head.strip_prefix("export ") {
                    spans.push((first, first + 6, SyntaxRole::Keyword));
                    key_start = key_end - after.trim_start().chars().count();
                }
                if key_end > key_start {
                    spans.push((key_start, key_end, SyntaxRole::Property));
                }
                spans.push((sep, sep + 1, SyntaxRole::Operator));
                i = sep + 1;
            }
        }
        while i < chars.len() {
            let c = chars[i];
            if self.strings && (c == '"' || c == '\'') {
                let mut j = i + 1;
                while j < chars.len() && chars[j] != c {
                    if chars[j] == '\\' {
                        j += 1;
                    }
                    j += 1;
                }
                let end = (j + 1).min(chars.len());
                spans.push((i, end, SyntaxRole::String));
                i = end;
                continue;
            }
            if self
                .line_comments
                .iter()
                .any(|p| p.starts_with(c) && (i == 0 || chars[i - 1].is_whitespace()))
            {
                spans.push((i, chars.len(), SyntaxRole::Comment));
                break;
            }
            if c.is_alphanumeric() || c == '_' || c == '.' || c == '-' {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_alphanumeric() || matches!(chars[i], '_' | '.' | '-'))
                {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                let is_keyword = self.keywords.iter().any(|k| {
                    if self.case_sensitive {
                        *k == word
                    } else {
                        k.eq_ignore_ascii_case(&word)
                    }
                });
                if is_keyword {
                    spans.push((start, i, SyntaxRole::Keyword));
                } else if self.numbers
                    && word.chars().next().is_some_and(|c| c.is_ascii_digit())
                    && word.parse::<f64>().is_ok()
                {
                    spans.push((start, i, SyntaxRole::Number));
                }
                continue;
            }
            i += 1;
        }
        spans
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> String {
        match detect(name, "") {
            Detected::Known(t) => t.id.to_string(),
            Detected::Grammar(g) => format!("grammar:{g}"),
            Detected::Unknown => "unknown".into(),
        }
    }

    #[test]
    fn names_map_to_types() {
        assert_eq!(id("User.php"), "php");
        assert_eq!(id("welcome.blade.php"), "blade");
        assert_eq!(id("artisan"), "php");
        assert_eq!(id("app.ts"), "typescript");
        assert_eq!(id("style.scss"), "css");
        assert_eq!(id("composer.lock"), "json");
        assert_eq!(id("phpunit.xml.dist"), "xml");
        assert_eq!(id("phpstan.neon.dist"), "yaml");
        assert_eq!(id(".env"), "dotenv");
        assert_eq!(id(".env.local"), "dotenv");
        assert_eq!(id(".env.example"), "dotenv");
        assert_eq!(id("Dockerfile"), "dockerfile");
        assert_eq!(id("Dockerfile.prod"), "dockerfile");
        assert_eq!(id(".gitignore"), "ignore");
        assert_eq!(id("php.ini-development"), "ini");
        assert_eq!(id("Cargo.toml"), "toml");
        assert_eq!(id("LICENSE"), "text");
        assert_eq!(id("script.py"), "grammar:Python");
        assert_eq!(id("data.unknownext"), "unknown");
    }

    #[test]
    fn first_line_detects_scripts_without_extension() {
        assert!(detect("bin/console", "#!/usr/bin/env php").is_php());
        assert!(detect("console", "<?php").is_php());
        assert_eq!(detect("deploy", "#!/bin/bash").name(), "Shell Script");
    }

    #[test]
    fn user_patterns_win() {
        let user = vec![("*.tpl".to_string(), "html".to_string())];
        assert_eq!(detect_with("page.tpl", "", &user).name(), "HTML");
        let user = vec![("*.php".to_string(), "text".to_string())];
        assert_eq!(detect_with("a.php", "", &user).name(), "Plain Text");
    }

    #[test]
    fn simple_languages_highlight_keys_comments_and_keywords() {
        let spans = DOTENV.highlight_line("APP_DEBUG=true # local\n");
        assert_eq!(
            spans,
            vec![
                (0, 9, SyntaxRole::Property),
                (9, 10, SyntaxRole::Operator),
                (10, 14, SyntaxRole::Keyword),
                (15, 22, SyntaxRole::Comment),
            ]
        );
        let spans = DOTENV.highlight_line("export DB_URL=\"mysql://x\"");
        assert_eq!(spans[0], (0, 6, SyntaxRole::Keyword));
        assert_eq!(spans[1], (7, 13, SyntaxRole::Property));
        assert_eq!(spans[3].2, SyntaxRole::String);
        assert_eq!(
            INI.highlight_line("[database]"),
            vec![(0, 10, SyntaxRole::Type)]
        );
        assert_eq!(
            INI.highlight_line("; note"),
            vec![(0, 6, SyntaxRole::Comment)]
        );
        let spans = DOCKERFILE.highlight_line("FROM php:8.3-fpm AS base");
        assert_eq!(spans[0], (0, 4, SyntaxRole::Keyword));
        assert!(spans.contains(&(17, 19, SyntaxRole::Keyword)));
        let spans = TOML.highlight_line("version = 3");
        assert!(spans.contains(&(10, 11, SyntaxRole::Number)));
    }
}
