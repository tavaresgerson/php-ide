//! Icon set of PHP IDE: Tabler Icons 3.2.0 (outline, MIT) stored under
//! semantic file names in `assets/`, plus the file-type mapping.
//!
//! UI code refers to meanings (`IconId::Run`, `IconId::GitBranch`), never to
//! file names. Each SVG is parsed once into stroke polylines ([`paths`]) that
//! the UI paints with the theme color, so there is no runtime SVG renderer
//! and no network access. `assets/manifest.json` records the Tabler name of
//! every icon; `scripts/update-icons.sh` regenerates the files from npm.

mod vector;
pub use vector::{paths, IconPath};

macro_rules! icons {
    ($($variant:ident => $slug:literal, $tabler:literal, $name:literal;)*) => {
        /// Semantic icon identifiers.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum IconId {
            $($variant,)*
            /// Missing or unknown icon: resolves to the neutral fallback.
            Unknown,
        }

        impl IconId {
            /// Stable slug, equal to the asset file name without extension.
            pub const fn slug(self) -> &'static str {
                match self {
                    $(IconId::$variant => $slug,)*
                    IconId::Unknown => "unknown",
                }
            }

            /// Name of the source icon in Tabler Icons.
            pub const fn tabler_name(self) -> &'static str {
                match self {
                    $(IconId::$variant => $tabler,)*
                    IconId::Unknown => "",
                }
            }

            /// Accessible name (screen readers and default tooltip).
            pub const fn accessible_name(self) -> &'static str {
                match self {
                    $(IconId::$variant => $name,)*
                    IconId::Unknown => "Icon",
                }
            }

            /// Every icon of the catalog (without `Unknown`).
            pub const fn catalog() -> &'static [IconId] {
                &[$(IconId::$variant,)*]
            }
        }

        /// Raw SVG of an icon (embedded; the fallback never fails).
        pub fn svg(id: IconId) -> &'static str {
            match id {
                $(IconId::$variant => include_str!(concat!("../assets/", $slug, ".svg")),)*
                IconId::Unknown => FALLBACK_SVG,
            }
        }
    };
}

icons! {
    Run => "run", "player-play", "Run";
    Debug => "debug", "bug", "Debug";
    Stop => "stop", "player-stop", "Stop";
    Pause => "pause", "player-pause", "Pause";
    Settings => "settings", "settings", "Settings";
    Close => "close", "x", "Close";
    Warning => "warning", "alert-triangle", "Warning";
    Error => "error", "circle-x", "Error";
    Info => "info", "info-circle", "Information";
    Project => "project", "folders", "Project";
    Folder => "folder", "folder", "Folder";
    File => "file", "file", "File";
    GitBranch => "git-branch", "git-branch", "Git branch";
    Search => "search", "search", "Search";
    Bookmark => "bookmark", "bookmark", "Bookmark";
    Breakpoint => "breakpoint", "circle-dot", "Breakpoint";
    ChevronRight => "chevron-right", "chevron-right", "Expand";
    ChevronDown => "chevron-down", "chevron-down", "Collapse";
    SplitRight => "split-right", "layout-columns", "Split right";
    SplitDown => "split-down", "layout-rows", "Split down";
    Switcher => "switcher", "switch-horizontal", "Tab switcher";
    Eye => "eye", "eye", "Preview";
    Blame => "blame", "history", "Blame";
    Coverage => "coverage", "percentage", "Coverage";
    Hint => "hint", "bulb", "Hint";
    FolderOpen => "folder-open", "folder-open", "Open folder";
    ChevronUp => "chevron-up", "chevron-up", "Up";
    ChevronLeft => "chevron-left", "chevron-left", "Back";
    Terminal => "terminal", "terminal-2", "Terminal";
    Problems => "problems", "alert-circle", "Problems";
    Structure => "structure", "list-tree", "Structure";
    Commit => "commit", "git-commit", "Commit";
    VersionControl => "version-control", "git-pull-request", "Version control";
    Services => "services", "server", "Services";
    Database => "database", "database", "Database";
    Notifications => "notifications", "bell", "Notifications";
    RemoteHost => "remote-host", "cloud", "Remote host";
    Plus => "plus", "plus", "Add";
    Minus => "minus", "minus", "Remove";
    Refresh => "refresh", "refresh", "Refresh";
    Filter => "filter", "filter", "Filter";
    More => "more", "dots", "More";
    Check => "check", "check", "Done";
    Copy => "copy", "copy", "Copy";
    ExternalLink => "external-link", "external-link", "Open externally";
    Undo => "undo", "arrow-back-up", "Undo";
    Redo => "redo", "arrow-forward-up", "Redo";
    Rerun => "rerun", "reload", "Rerun";
    Resume => "resume", "player-track-next", "Resume";
    StepOver => "step-over", "arrow-bar-right", "Step over";
    StepInto => "step-into", "arrow-bar-down", "Step into";
    StepOut => "step-out", "arrow-bar-up", "Step out";
    Pin => "pin", "pin", "Pin";
    Package => "package", "package", "Package";
    Test => "test", "flask", "Test";
    Download => "download", "download", "Download";
    Build => "build", "hammer", "Build";
    SidebarLeft => "sidebar-left", "layout-sidebar", "Left sidebar";
    SidebarRight => "sidebar-right", "layout-sidebar-right", "Right sidebar";
    PanelBottom => "panel-bottom", "layout-bottombar", "Bottom panel";
    Menu => "menu", "menu-2", "Main menu";
    Moon => "moon", "moon", "Dark theme";
    Sun => "sun", "sun", "Light theme";
    Trash => "trash", "trash", "Delete";
    Edit => "edit", "edit", "Edit";
    Save => "save", "device-floppy", "Save";
    Clock => "clock", "clock", "Time";
    Success => "success", "circle-check", "Success";
    Loader => "loader", "loader-2", "Loading";
    Selector => "selector", "selector", "Choose";
    Palette => "palette", "palette", "Theme";
    Keyboard => "keyboard", "keyboard", "Keyboard";
    Command => "command", "command", "Command";
    Home => "home", "home", "Home";
    Locate => "locate", "current-location", "Select opened file";
    CollapseAll => "collapse-all", "fold", "Collapse all";
    ExpandAll => "expand-all", "unfold", "Expand all";
    FilePhp => "file-php", "file-type-php", "PHP file";
    FileJs => "file-js", "file-type-js", "JavaScript file";
    FileTs => "file-ts", "file-type-ts", "TypeScript file";
    FileCss => "file-css", "file-type-css", "CSS file";
    FileHtml => "file-html", "file-type-html", "HTML file";
    FileXml => "file-xml", "file-type-xml", "XML file";
    FileSql => "file-sql", "file-type-sql", "SQL file";
    FileText => "file-text", "file-text", "Text file";
    FileCode => "file-code", "file-code", "Code file";
    FileJson => "file-json", "braces", "JSON file";
    FileMarkdown => "file-markdown", "markdown", "Markdown file";
    FileConfig => "file-config", "file-settings", "Configuration file";
    FileGit => "file-git", "brand-git", "Git file";
    FileLock => "file-lock", "lock", "Lock file";
    FileDocker => "file-docker", "brand-docker", "Docker file";
    FileImage => "file-image", "photo", "Image";
    FileArchive => "file-archive", "file-zip", "Archive";
    FileSvg => "file-svg", "file-type-svg", "SVG file";
}

/// Neutral fallback: a circle with a dot.
const FALLBACK_SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\"><circle cx=\"12\" cy=\"12\" r=\"8\"/><path d=\"M12 12h.01\"/></svg>";

/// Broad file category, used to choose the icon and its tint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    Php,
    Script,
    Style,
    Markup,
    Data,
    Text,
    Config,
    Vcs,
    Image,
    Archive,
    Other,
}

/// Icon and category for a file name (case-insensitive extension match).
pub fn file_icon(file_name: &str) -> (IconId, FileKind) {
    let lower = file_name.to_ascii_lowercase();
    match lower.as_str() {
        "composer.lock" | "package-lock.json" | "yarn.lock" | "pnpm-lock.yaml" => {
            return (IconId::FileLock, FileKind::Config)
        }
        ".gitignore" | ".gitattributes" | ".gitmodules" | ".gitkeep" => {
            return (IconId::FileGit, FileKind::Vcs)
        }
        "dockerfile" | "docker-compose.yml" | "docker-compose.yaml" | "compose.yaml" => {
            return (IconId::FileDocker, FileKind::Config)
        }
        "artisan" => return (IconId::FilePhp, FileKind::Php),
        _ => {}
    }
    if lower.starts_with(".env") || lower.ends_with(".dist") && lower.contains("phpunit") {
        return (IconId::FileConfig, FileKind::Config);
    }
    if lower.ends_with(".blade.php") {
        return (IconId::FileHtml, FileKind::Markup);
    }
    let extension = lower.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("");
    match extension {
        "php" | "phtml" | "inc" | "phpt" => (IconId::FilePhp, FileKind::Php),
        "js" | "mjs" | "cjs" | "jsx" => (IconId::FileJs, FileKind::Script),
        "ts" | "mts" | "cts" | "tsx" => (IconId::FileTs, FileKind::Script),
        "css" | "scss" | "sass" | "less" => (IconId::FileCss, FileKind::Style),
        "html" | "htm" | "twig" | "vue" => (IconId::FileHtml, FileKind::Markup),
        "xml" | "xsd" | "xsl" => (IconId::FileXml, FileKind::Markup),
        "svg" => (IconId::FileSvg, FileKind::Image),
        "sql" => (IconId::FileSql, FileKind::Data),
        "json" => (IconId::FileJson, FileKind::Data),
        "csv" | "tsv" => (IconId::FileText, FileKind::Data),
        "md" | "markdown" => (IconId::FileMarkdown, FileKind::Text),
        "txt" | "log" | "rst" => (IconId::FileText, FileKind::Text),
        "yml" | "yaml" | "ini" | "toml" | "neon" | "conf" | "env" => {
            (IconId::FileConfig, FileKind::Config)
        }
        "lock" => (IconId::FileLock, FileKind::Config),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "ico" | "bmp" | "avif" => {
            (IconId::FileImage, FileKind::Image)
        }
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" | "phar" => {
            (IconId::FileArchive, FileKind::Archive)
        }
        "sh" | "bash" | "zsh" | "py" | "rb" | "go" | "rs" | "java" | "c" | "h" | "cpp" => {
            (IconId::FileCode, FileKind::Script)
        }
        _ => (IconId::File, FileKind::Other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_assets_are_tabler_svgs_listed_in_the_manifest() {
        let manifest = include_str!("../assets/manifest.json");
        assert!(manifest.contains("\"version\": \"3.2.0\""));
        assert!(manifest.contains("\"license\": \"MIT\""));
        for id in IconId::catalog() {
            let source = svg(*id);
            assert!(source.starts_with("<svg"), "{}", id.slug());
            assert!(source.contains("viewBox=\"0 0 24 24\""), "{}", id.slug());
            assert!(source.contains("stroke=\"currentColor\""), "{}", id.slug());
            let entry = format!(
                "\"name\": \"{}\",\n      \"tabler\": \"{}\"",
                id.slug(),
                id.tabler_name()
            );
            assert!(manifest.contains(&entry), "manifest misses {entry}");
            assert!(!id.accessible_name().is_empty());
        }
    }

    #[test]
    fn slugs_are_unique() {
        let mut slugs: Vec<_> = IconId::catalog().iter().map(|id| id.slug()).collect();
        slugs.sort_unstable();
        let total = slugs.len();
        slugs.dedup();
        assert_eq!(slugs.len(), total);
    }

    #[test]
    fn file_icons_by_name() {
        assert_eq!(file_icon("Index.PHP"), (IconId::FilePhp, FileKind::Php));
        assert_eq!(file_icon("app.blade.php").0, IconId::FileHtml);
        assert_eq!(file_icon("composer.json").0, IconId::FileJson);
        assert_eq!(file_icon("composer.lock").0, IconId::FileLock);
        assert_eq!(file_icon(".env.local").0, IconId::FileConfig);
        assert_eq!(file_icon("phpunit.xml.dist").0, IconId::FileConfig);
        assert_eq!(file_icon(".gitignore").1, FileKind::Vcs);
        assert_eq!(file_icon("README").0, IconId::File);
        assert_eq!(file_icon("logo.svg").0, IconId::FileSvg);
    }
}
