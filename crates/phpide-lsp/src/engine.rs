//! Which PHP language server the IDE starts, and where it comes from.
//!
//! PHPantom ships with the IDE (`<prefix>/lib/phpide/phpantom_lsp`, see
//! `scripts/install-files.sh`); intelephense, phpactor or any other command
//! can be chosen in Settings › PHP › Language Server. Discovery order:
//!
//! 1. the `PHPIDE_LSP` environment variable (a whole command line);
//! 2. the engine chosen in Settings (or the project's own choice);
//! 3. for "Automatic": PHPantom (bundled, then the development copy in
//!    `third_party/phpantom/`, then `phpantom_lsp` on `PATH`), then
//!    intelephense (`third_party/node_modules`, then `PATH`), then phpactor.

use std::path::{Path, PathBuf};

/// The engine chosen in Settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum EngineChoice {
    /// The first engine found, bundled PHPantom first.
    #[default]
    Automatic,
    Phpantom,
    Intelephense,
    Phpactor,
    /// A command line of the user's own.
    Custom(String),
}

impl EngineChoice {
    /// Every choice, in the order Settings lists them (the custom command
    /// is empty).
    pub fn all() -> [EngineChoice; 5] {
        [
            EngineChoice::Automatic,
            EngineChoice::Phpantom,
            EngineChoice::Intelephense,
            EngineChoice::Phpactor,
            EngineChoice::Custom(String::new()),
        ]
    }

    /// Stable id stored in the settings.
    pub fn id(&self) -> &'static str {
        match self {
            EngineChoice::Automatic => "auto",
            EngineChoice::Phpantom => "phpantom",
            EngineChoice::Intelephense => "intelephense",
            EngineChoice::Phpactor => "phpactor",
            EngineChoice::Custom(_) => "custom",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            EngineChoice::Automatic => "Automatic (bundled PHPantom first)",
            EngineChoice::Phpantom => "PHPantom",
            EngineChoice::Intelephense => "intelephense",
            EngineChoice::Phpactor => "phpactor",
            EngineChoice::Custom(_) => "Custom command",
        }
    }

    /// The choice stored as `id` (and `command` for a custom one); unknown
    /// ids fall back to automatic.
    pub fn from_id(id: &str, command: &str) -> Self {
        match id {
            "phpantom" => EngineChoice::Phpantom,
            "intelephense" => EngineChoice::Intelephense,
            "phpactor" => EngineChoice::Phpactor,
            "custom" => EngineChoice::Custom(command.to_string()),
            _ => EngineChoice::Automatic,
        }
    }

    /// The command line of a custom choice.
    pub fn command(&self) -> &str {
        match self {
            EngineChoice::Custom(command) => command,
            _ => "",
        }
    }
}

/// The server that is actually started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineKind {
    Phpantom,
    Intelephense,
    Phpactor,
    Other,
}

impl EngineKind {
    pub fn label(self) -> &'static str {
        match self {
            EngineKind::Phpantom => "PHPantom",
            EngineKind::Intelephense => "intelephense",
            EngineKind::Phpactor => "phpactor",
            EngineKind::Other => "custom server",
        }
    }

    /// The kind a program name suggests (for `PHPIDE_LSP` and custom
    /// commands): it decides which protocol extras are sent.
    pub fn of_program(program: &Path) -> Self {
        let name = program
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if name.starts_with("phpantom") {
            EngineKind::Phpantom
        } else if name.starts_with("intelephense") {
            EngineKind::Intelephense
        } else if name.starts_with("phpactor") {
            EngineKind::Phpactor
        } else {
            EngineKind::Other
        }
    }
}

/// Where the command came from (shown in Settings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The `PHPIDE_LSP` environment variable.
    Environment,
    /// Installed with the IDE.
    Bundled,
    /// `third_party/` of a development checkout.
    Development,
    /// Found on `PATH`.
    Path,
    /// The custom command in Settings.
    Settings,
}

impl Origin {
    pub fn label(self) -> &'static str {
        match self {
            Origin::Environment => "from PHPIDE_LSP",
            Origin::Bundled => "bundled with the IDE",
            Origin::Development => "development copy",
            Origin::Path => "found on PATH",
            Origin::Settings => "custom command",
        }
    }
}

/// A language server command line, ready to spawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerCommand {
    pub kind: EngineKind,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub origin: Origin,
}

impl ServerCommand {
    fn new(kind: EngineKind, program: PathBuf, origin: Origin) -> Self {
        let args = match kind {
            EngineKind::Phpantom | EngineKind::Intelephense => vec!["--stdio".to_string()],
            EngineKind::Phpactor => vec!["language-server".to_string()],
            EngineKind::Other => Vec::new(),
        };
        ServerCommand {
            kind,
            program,
            args,
            origin,
        }
    }

    /// A whole command line, split on whitespace; quotes group words.
    pub fn parse(line: &str, origin: Origin) -> Option<Self> {
        let mut words = split_command(line).into_iter();
        let program = PathBuf::from(words.next()?);
        Some(ServerCommand {
            kind: EngineKind::of_program(&program),
            args: words.collect(),
            program,
            origin,
        })
    }

    /// The command line as text.
    pub fn display(&self) -> String {
        let mut out = self.program.display().to_string();
        for arg in &self.args {
            out.push(' ');
            out.push_str(arg);
        }
        out
    }
}

/// Splits a command line on whitespace; double or single quotes keep
/// spaces inside a word.
pub fn split_command(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for c in line.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => word.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                started = true;
            }
            None if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            None => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    words
}

/// Places the discovery looks at; tests replace them.
#[derive(Debug, Clone)]
pub struct Discovery {
    /// `PHPIDE_LSP`.
    pub env_command: Option<String>,
    /// The IDE executable (its `../lib/phpide/` holds the bundled server).
    pub exe: Option<PathBuf>,
    /// Development checkouts whose `third_party/` may hold servers: the
    /// source tree the IDE was built from and the project itself.
    pub checkouts: Vec<PathBuf>,
    /// Directories of `PATH`.
    pub path: Vec<PathBuf>,
}

impl Discovery {
    /// The real environment of this process, for a project at `root`.
    pub fn system(root: &Path) -> Self {
        let source_tree = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        Discovery {
            env_command: std::env::var("PHPIDE_LSP")
                .ok()
                .filter(|c| !c.trim().is_empty()),
            exe: std::env::current_exe().ok(),
            checkouts: vec![source_tree, root.to_path_buf()],
            path: std::env::var_os("PATH")
                .map(|p| std::env::split_paths(&p).collect())
                .unwrap_or_default(),
        }
    }

    /// The PHPantom installed with the IDE.
    pub fn bundled_phpantom(&self) -> Option<PathBuf> {
        let exe = self.exe.as_ref()?.canonicalize().ok();
        let exe = exe.as_ref().or(self.exe.as_ref())?;
        let dir = exe.parent()?;
        let prefix = dir.parent()?;
        [
            prefix.join("lib/phpide/phpantom_lsp"),
            dir.join("phpantom_lsp"),
        ]
        .into_iter()
        .find(|p| is_executable(p))
        .and_then(|p| p.canonicalize().ok())
    }

    fn in_checkouts(&self, rel: &str) -> Option<PathBuf> {
        self.checkouts
            .iter()
            .map(|dir| dir.join(rel))
            .find(|p| is_executable(p))
            .and_then(|p| p.canonicalize().ok())
    }

    fn on_path(&self, name: &str) -> Option<PathBuf> {
        self.path
            .iter()
            .map(|dir| dir.join(name))
            .find(|p| is_executable(p))
    }

    fn phpantom(&self) -> Option<ServerCommand> {
        let found = self
            .bundled_phpantom()
            .map(|p| (p, Origin::Bundled))
            .or_else(|| {
                self.in_checkouts("third_party/phpantom/phpantom_lsp")
                    .map(|p| (p, Origin::Development))
            })
            .or_else(|| self.on_path("phpantom_lsp").map(|p| (p, Origin::Path)))?;
        Some(ServerCommand::new(EngineKind::Phpantom, found.0, found.1))
    }

    fn intelephense(&self) -> Option<ServerCommand> {
        let found = self
            .in_checkouts("third_party/node_modules/.bin/intelephense")
            .map(|p| (p, Origin::Development))
            .or_else(|| self.on_path("intelephense").map(|p| (p, Origin::Path)))?;
        Some(ServerCommand::new(
            EngineKind::Intelephense,
            found.0,
            found.1,
        ))
    }

    fn phpactor(&self) -> Option<ServerCommand> {
        let path = self.on_path("phpactor")?;
        Some(ServerCommand::new(EngineKind::Phpactor, path, Origin::Path))
    }

    /// The server to start for `choice`, or why there is none.
    pub fn resolve(&self, choice: &EngineChoice) -> Result<ServerCommand, String> {
        if let Some(line) = &self.env_command {
            return ServerCommand::parse(line, Origin::Environment)
                .ok_or_else(|| "PHPIDE_LSP is empty".to_string());
        }
        match choice {
            EngineChoice::Automatic => self
                .phpantom()
                .or_else(|| self.intelephense())
                .or_else(|| self.phpactor())
                .ok_or_else(|| {
                    "no PHP language server found: PHPantom is not installed with the IDE \
                     and neither intelephense nor phpactor is on PATH"
                        .to_string()
                }),
            EngineChoice::Phpantom => self.phpantom().ok_or_else(|| {
                "PHPantom not found: it is installed with the IDE packages; in a source \
                 checkout run scripts/fetch-phpantom.sh"
                    .to_string()
            }),
            EngineChoice::Intelephense => self.intelephense().ok_or_else(|| {
                "intelephense not found: install it with npm install -g intelephense".to_string()
            }),
            EngineChoice::Phpactor => self
                .phpactor()
                .ok_or_else(|| "phpactor not found on PATH".to_string()),
            EngineChoice::Custom(line) => ServerCommand::parse(line, Origin::Settings)
                .ok_or_else(|| "the custom language server command is empty".to_string()),
        }
    }
}

/// The server for `choice` in this process's environment.
pub fn resolve(choice: &EngineChoice, root: &Path) -> Result<ServerCommand, String> {
    Discovery::system(root).resolve(choice)
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn executable(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    fn empty(dir: &Path) -> Discovery {
        Discovery {
            env_command: None,
            exe: Some(dir.join("prefix/bin/phpide")),
            checkouts: vec![dir.join("checkout")],
            path: vec![dir.join("path")],
        }
    }

    #[test]
    fn bundled_phpantom_comes_first() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        executable(&d.join("prefix/lib/phpide/phpantom_lsp"));
        executable(&d.join("checkout/third_party/phpantom/phpantom_lsp"));
        executable(&d.join("path/intelephense"));
        let found = empty(d).resolve(&EngineChoice::Automatic).unwrap();
        assert_eq!(found.kind, EngineKind::Phpantom);
        assert_eq!(found.origin, Origin::Bundled);
        assert_eq!(found.args, ["--stdio"]);
        assert!(found.program.ends_with("lib/phpide/phpantom_lsp"));
    }

    #[test]
    fn automatic_falls_back_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let discovery = empty(d);
        assert!(discovery.resolve(&EngineChoice::Automatic).is_err());

        executable(&d.join("path/phpactor"));
        let found = discovery.resolve(&EngineChoice::Automatic).unwrap();
        assert_eq!(
            (found.kind, found.args.clone()),
            (EngineKind::Phpactor, vec!["language-server".to_string()])
        );

        executable(&d.join("checkout/third_party/node_modules/.bin/intelephense"));
        let found = discovery.resolve(&EngineChoice::Automatic).unwrap();
        assert_eq!(
            (found.kind, found.origin),
            (EngineKind::Intelephense, Origin::Development)
        );

        executable(&d.join("path/phpantom_lsp"));
        let found = discovery.resolve(&EngineChoice::Automatic).unwrap();
        assert_eq!(
            (found.kind, found.origin),
            (EngineKind::Phpantom, Origin::Path)
        );

        executable(&d.join("checkout/third_party/phpantom/phpantom_lsp"));
        let found = discovery.resolve(&EngineChoice::Automatic).unwrap();
        assert_eq!(found.origin, Origin::Development);
    }

    #[test]
    fn a_chosen_engine_does_not_fall_back() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        executable(&d.join("prefix/lib/phpide/phpantom_lsp"));
        let discovery = empty(d);
        let err = discovery.resolve(&EngineChoice::Intelephense).unwrap_err();
        assert!(err.contains("intelephense"), "{err}");
        assert_eq!(
            discovery.resolve(&EngineChoice::Phpantom).unwrap().origin,
            Origin::Bundled
        );
    }

    #[test]
    fn environment_and_custom_commands() {
        let dir = tempfile::tempdir().unwrap();
        let mut discovery = empty(dir.path());
        let custom = EngineChoice::Custom("'/opt/my server/phpactor' language-server -vvv".into());
        let found = discovery.resolve(&custom).unwrap();
        assert_eq!(found.program, PathBuf::from("/opt/my server/phpactor"));
        assert_eq!(found.args, ["language-server", "-vvv"]);
        assert_eq!(
            (found.kind, found.origin),
            (EngineKind::Phpactor, Origin::Settings)
        );
        assert!(discovery
            .resolve(&EngineChoice::Custom("  ".into()))
            .is_err());

        discovery.env_command = Some("phpantom_lsp --stdio".into());
        let found = discovery.resolve(&EngineChoice::Intelephense).unwrap();
        assert_eq!(
            (found.kind, found.origin),
            (EngineKind::Phpantom, Origin::Environment)
        );
    }

    #[test]
    fn choices_round_trip_through_their_ids() {
        for choice in EngineChoice::all() {
            assert_eq!(EngineChoice::from_id(choice.id(), choice.command()), choice);
        }
        assert_eq!(EngineChoice::from_id("?", ""), EngineChoice::Automatic);
        assert_eq!(split_command(r#"a "b c" 'd' e"#), ["a", "b c", "d", "e"]);
    }
}
