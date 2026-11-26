//! File templates of Project › New (Alt+Insert): a new file of a type,
//! already holding its basic structure (an HTML page with `<head>` and
//! `<body>`, a PHP file with `declare(strict_types=1)`, a `composer.json`…).
//!
//! Templates of a type ask for a name and add the extension; templates of a
//! well-known file (`composer.json`, `Dockerfile`) create it at once.

use phpide_icons::IconId;

/// What a template gets to fill its text.
pub struct TemplateContext<'a> {
    /// File name without the extension (`index` for `index.html`).
    pub name: &'a str,
    /// PSR-4 namespace of the folder, when Composer maps it.
    pub namespace: Option<&'a str>,
    /// Project folder name (package names).
    pub project: &'a str,
}

pub struct FileTemplate {
    pub id: &'static str,
    pub label: &'static str,
    pub icon: IconId,
    /// Extension added to the name typed (`html`), or `None` for files with
    /// a fixed name.
    pub extension: Option<&'static str>,
    /// The fixed file name (`composer.json`).
    pub fixed_name: Option<&'static str>,
    pub body: fn(&TemplateContext) -> String,
}

impl FileTemplate {
    /// The file name for what the user typed: the extension is added when
    /// missing (`index` → `index.html`; `about.htm` stays).
    pub fn file_name(&self, typed: &str) -> String {
        let typed = typed.trim();
        if let Some(fixed) = self.fixed_name {
            return fixed.to_string();
        }
        match self.extension {
            Some(ext) => {
                let lower = typed.to_lowercase();
                let has = lower.ends_with(&format!(".{ext}"))
                    || self
                        .aliases()
                        .iter()
                        .any(|a| lower.ends_with(&format!(".{a}")));
                if has {
                    typed.to_string()
                } else {
                    format!("{typed}.{ext}")
                }
            }
            None => typed.to_string(),
        }
    }

    fn aliases(&self) -> &'static [&'static str] {
        match self.id {
            "html" => &["htm", "phtml"],
            "yaml" => &["yml"],
            "markdown" => &["markdown"],
            "javascript" => &["mjs", "cjs"],
            _ => &[],
        }
    }

    /// The name without the extension, for the text of the file.
    pub fn stem(file_name: &str) -> &str {
        let base = file_name.rsplit('/').next().unwrap_or(file_name);
        base.split('.')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(base)
    }
}

fn php_file(_: &TemplateContext) -> String {
    "<?php\n\ndeclare(strict_types=1);\n\n".to_string()
}

fn phpunit_test(c: &TemplateContext) -> String {
    let namespace = c
        .namespace
        .map(|n| format!("namespace {n};\n\n"))
        .unwrap_or_default();
    let subject = c.name.strip_suffix("Test").unwrap_or(c.name);
    format!(
        "<?php\n\ndeclare(strict_types=1);\n\n{namespace}use PHPUnit\\Framework\\TestCase;\n\nfinal class {name} extends TestCase\n{{\n    public function test{subject}(): void\n    {{\n        self::assertTrue(true);\n    }}\n}}\n",
        name = c.name,
    )
}

fn html(c: &TemplateContext) -> String {
    format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n    <meta charset=\"UTF-8\">\n    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n    <title>{}</title>\n</head>\n<body>\n\n</body>\n</html>\n",
        c.name
    )
}

fn javascript(_: &TemplateContext) -> String {
    "'use strict';\n\n".to_string()
}

fn empty(_: &TemplateContext) -> String {
    String::new()
}

fn css(_: &TemplateContext) -> String {
    "/* Styles */\n\n".to_string()
}

fn json(_: &TemplateContext) -> String {
    "{\n}\n".to_string()
}

fn xml(_: &TemplateContext) -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n".to_string()
}

fn yaml(_: &TemplateContext) -> String {
    "# YAML\n".to_string()
}

fn markdown(c: &TemplateContext) -> String {
    format!("# {}\n\n", c.name)
}

fn sql(_: &TemplateContext) -> String {
    "-- SQL\n\n".to_string()
}

fn package_name(project: &str) -> String {
    let name: String = project
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let name = name.trim_matches('-').to_string();
    if name.is_empty() {
        "app".into()
    } else {
        name
    }
}

fn composer_json(c: &TemplateContext) -> String {
    format!(
        "{{\n    \"name\": \"vendor/{}\",\n    \"description\": \"\",\n    \"type\": \"project\",\n    \"require\": {{\n        \"php\": \"^8.2\"\n    }},\n    \"require-dev\": {{\n        \"phpunit/phpunit\": \"^11.0\"\n    }},\n    \"autoload\": {{\n        \"psr-4\": {{\n            \"App\\\\\": \"src/\"\n        }}\n    }},\n    \"autoload-dev\": {{\n        \"psr-4\": {{\n            \"Tests\\\\\": \"tests/\"\n        }}\n    }}\n}}\n",
        package_name(c.project)
    )
}

fn package_json(c: &TemplateContext) -> String {
    format!(
        "{{\n  \"name\": \"{}\",\n  \"version\": \"1.0.0\",\n  \"private\": true,\n  \"scripts\": {{\n  }},\n  \"dependencies\": {{\n  }},\n  \"devDependencies\": {{\n  }}\n}}\n",
        package_name(c.project)
    )
}

fn tsconfig(_: &TemplateContext) -> String {
    "{\n  \"compilerOptions\": {\n    \"target\": \"ES2022\",\n    \"module\": \"ESNext\",\n    \"moduleResolution\": \"Bundler\",\n    \"strict\": true,\n    \"esModuleInterop\": true,\n    \"skipLibCheck\": true,\n    \"outDir\": \"dist\"\n  },\n  \"include\": [\"src\"]\n}\n".to_string()
}

fn phpunit_xml(_: &TemplateContext) -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<phpunit xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"\n         xsi:noNamespaceSchemaLocation=\"vendor/phpunit/phpunit/phpunit.xsd\"\n         bootstrap=\"vendor/autoload.php\"\n         colors=\"true\">\n    <testsuites>\n        <testsuite name=\"Tests\">\n            <directory>tests</directory>\n        </testsuite>\n    </testsuites>\n    <source>\n        <include>\n            <directory>src</directory>\n        </include>\n    </source>\n</phpunit>\n".to_string()
}

fn dockerfile(_: &TemplateContext) -> String {
    "FROM php:8.3-cli\n\nRUN apt-get update \\\n    && apt-get install -y --no-install-recommends git unzip \\\n    && rm -rf /var/lib/apt/lists/*\n\nCOPY --from=composer:2 /usr/bin/composer /usr/bin/composer\n\nWORKDIR /app\nCOPY . .\nRUN composer install --no-interaction --prefer-dist\n\nCMD [\"php\", \"-S\", \"0.0.0.0:8000\", \"-t\", \"public\"]\n".to_string()
}

fn compose(_: &TemplateContext) -> String {
    "services:\n  app:\n    build: .\n    ports:\n      - \"8000:8000\"\n    volumes:\n      - .:/app\n".to_string()
}

fn editorconfig(_: &TemplateContext) -> String {
    "root = true\n\n[*]\ncharset = utf-8\nend_of_line = lf\ninsert_final_newline = true\nindent_style = space\nindent_size = 4\ntrim_trailing_whitespace = true\n\n[*.{js,ts,json,yml,yaml}]\nindent_size = 2\n\n[*.md]\ntrim_trailing_whitespace = false\n".to_string()
}

fn gitignore(_: &TemplateContext) -> String {
    "/vendor/\n/node_modules/\n.env\n.phpunit.result.cache\n.phpunit.cache/\n.idea/\n.phpide/\n"
        .to_string()
}

fn env_file(_: &TemplateContext) -> String {
    "APP_ENV=local\nAPP_DEBUG=true\n".to_string()
}

const fn typed(
    id: &'static str,
    label: &'static str,
    icon: IconId,
    extension: &'static str,
    body: fn(&TemplateContext) -> String,
) -> FileTemplate {
    FileTemplate {
        id,
        label,
        icon,
        extension: Some(extension),
        fixed_name: None,
        body,
    }
}

const fn fixed(
    id: &'static str,
    label: &'static str,
    icon: IconId,
    name: &'static str,
    body: fn(&TemplateContext) -> String,
) -> FileTemplate {
    FileTemplate {
        id,
        label,
        icon,
        extension: None,
        fixed_name: Some(name),
        body,
    }
}

/// Templates of a file type, in menu order.
pub const TYPED: &[FileTemplate] = &[
    typed("php", "PHP File", IconId::FilePhp, "php", php_file),
    typed("phpunit", "PHPUnit Test", IconId::Test, "php", phpunit_test),
    typed("html", "HTML File", IconId::FileHtml, "html", html),
    typed(
        "javascript",
        "JavaScript File",
        IconId::FileJs,
        "js",
        javascript,
    ),
    typed("typescript", "TypeScript File", IconId::FileTs, "ts", empty),
    typed("css", "Stylesheet", IconId::FileCss, "css", css),
    typed("json", "JSON File", IconId::FileJson, "json", json),
    typed("xml", "XML File", IconId::FileXml, "xml", xml),
    typed("yaml", "YAML File", IconId::FileConfig, "yaml", yaml),
    typed(
        "markdown",
        "Markdown File",
        IconId::FileMarkdown,
        "md",
        markdown,
    ),
    typed("sql", "SQL File", IconId::FileSql, "sql", sql),
];

/// Templates of well-known files, in menu order.
pub const FIXED: &[FileTemplate] = &[
    fixed(
        "composer",
        "composer.json",
        IconId::FileJson,
        "composer.json",
        composer_json,
    ),
    fixed(
        "package",
        "package.json",
        IconId::FileJs,
        "package.json",
        package_json,
    ),
    fixed(
        "tsconfig",
        "tsconfig.json",
        IconId::FileTs,
        "tsconfig.json",
        tsconfig,
    ),
    fixed(
        "phpunit-xml",
        "phpunit.xml",
        IconId::Test,
        "phpunit.xml",
        phpunit_xml,
    ),
    fixed(
        "dockerfile",
        "Dockerfile",
        IconId::FileDocker,
        "Dockerfile",
        dockerfile,
    ),
    fixed(
        "compose",
        "Docker Compose File",
        IconId::FileDocker,
        "compose.yaml",
        compose,
    ),
    fixed(
        "editorconfig",
        "EditorConfig File",
        IconId::FileConfig,
        ".editorconfig",
        editorconfig,
    ),
    fixed(
        ".gitignore",
        ".gitignore File",
        IconId::FileGit,
        ".gitignore",
        gitignore,
    ),
    fixed("env", ".env File", IconId::FileConfig, ".env", env_file),
];

pub fn by_id(id: &str) -> Option<&'static FileTemplate> {
    TYPED.iter().chain(FIXED).find(|t| t.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(name: &str) -> TemplateContext<'_> {
        TemplateContext {
            name,
            namespace: Some("App\\Tests"),
            project: "My Shop",
        }
    }

    #[test]
    fn names_get_their_extension_once() {
        let html = by_id("html").unwrap();
        assert_eq!(html.file_name("index"), "index.html");
        assert_eq!(html.file_name("index.html"), "index.html");
        assert_eq!(html.file_name("legacy.htm"), "legacy.htm");
        assert_eq!(html.file_name("views/home"), "views/home.html");
        assert_eq!(by_id("yaml").unwrap().file_name("ci.yml"), "ci.yml");
        assert_eq!(by_id("composer").unwrap().file_name("x"), "composer.json");
        assert_eq!(FileTemplate::stem("views/home.blade.php"), "home");
    }

    #[test]
    fn templates_hold_their_basic_structure() {
        let page = (by_id("html").unwrap().body)(&ctx("about"));
        for part in [
            "<!DOCTYPE html>",
            "<html lang=\"en\">",
            "<head>",
            "<meta charset=\"UTF-8\">",
            "<title>about</title>",
            "<body>",
            "</html>",
        ] {
            assert!(page.contains(part), "{part} in {page}");
        }
        let test = (by_id("phpunit").unwrap().body)(&ctx("CartTest"));
        assert!(test.contains("namespace App\\Tests;"));
        assert!(test.contains("final class CartTest extends TestCase"));
        assert!(test.contains("public function testCart(): void"));
        let composer = (by_id("composer").unwrap().body)(&ctx(""));
        let parsed: serde_json::Value = serde_json::from_str(&composer).unwrap();
        assert_eq!(parsed["name"], "vendor/my-shop");
        assert_eq!(parsed["autoload"]["psr-4"]["App\\"], "src/");
        for id in ["package", "tsconfig", "json"] {
            let text = (by_id(id).unwrap().body)(&ctx("x"));
            serde_json::from_str::<serde_json::Value>(&text)
                .unwrap_or_else(|e| panic!("{id}: {e}"));
        }
        assert!((by_id("php").unwrap().body)(&ctx("x")).starts_with("<?php\n"));
        assert_eq!(
            (by_id("markdown").unwrap().body)(&ctx("Notes")),
            "# Notes\n\n"
        );
    }

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<&str> = TYPED.iter().chain(FIXED).map(|t| t.id).collect();
        ids.sort();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before);
    }
}
