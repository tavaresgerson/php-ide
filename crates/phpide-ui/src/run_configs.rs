//! Run configurations (Phase 6): stored in `.phpide/run-configurations.json`
//! so a team can share them, selected in the header's Run widget, edited in
//! Run › Edit Configurations.
//!
//! Shift+F10 / Shift+F9 run or debug the selected configuration (the
//! current file when none is selected); Ctrl+Shift+F10 / Ctrl+Shift+F9 run
//! the context: the test method or class at the caret in a test, otherwise
//! the current file. Context runs become temporary configurations, like in
//! the JetBrains IDEs, and can be saved.

use std::path::{Path, PathBuf};

use eframe::egui;
use phpide_run::config::{RunConfig, RunConfigFile, RunKind, TestScope};

use crate::design::components::{Button, Checkbox, Dialog, ListRow, Select, TextInput};
use crate::toolwindows::ToolWindow;
use crate::PhpIdeApp;

/// Temporary configurations kept (oldest dropped).
const TEMPORARY: usize = 5;

pub fn file_path(root: &Path) -> PathBuf {
    root.join(".phpide/run-configurations.json")
}

/// The project's saved configurations (empty when there is no file or it
/// cannot be read).
pub fn load(root: &Path) -> Result<Vec<RunConfig>, String> {
    let path = file_path(root);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(Vec::new());
    };
    serde_json::from_str::<RunConfigFile>(&text)
        .map(|file| file.configs)
        .map_err(|e| format!("{} could not be read: {e}", path.display()))
}

pub fn save(root: &Path, configs: &[RunConfig]) -> Result<(), String> {
    let path = file_path(root);
    if configs.is_empty() && !path.exists() {
        return Ok(());
    }
    let mut file = RunConfigFile::new();
    file.configs = configs.to_vec();
    let text = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    phpide_core::fsutil::write_atomic(&path, format!("{text}\n").as_bytes())
        .map_err(|e| format!("could not save {}: {e}", path.display()))
}

/// A short unique id for a new configuration.
pub fn new_id(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{prefix}-{:x}", nanos & 0xffff_ffff_ffff)
}

pub fn kind_label(kind: &RunKind) -> &'static str {
    match kind {
        RunKind::PhpScript { .. } => "PHP Script",
        RunKind::PhpUnit { .. } => "PHPUnit",
        RunKind::ComposerScript { .. } => "Composer Script",
    }
}

/// The test at a line of a PHP file: `(class FQCN, method)`. The method is
/// the `test*` method (or `#[Test]` method) around the line, if any.
pub fn test_at(text: &str, line0: usize) -> Option<(String, Option<String>)> {
    let mut namespace = String::new();
    let mut class = None;
    let mut method = None;
    let mut attribute_test = false;
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if let Some(ns) = line.strip_prefix("namespace ") {
            namespace = ns.trim_end_matches([';', '{', ' ']).trim().to_string();
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        if let Some(at) = words.iter().position(|w| *w == "class") {
            if let Some(name) = words.get(at + 1) {
                let name: String = name
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                let is_test = line.contains("TestCase") || name.ends_with("Test");
                if is_test && !name.is_empty() {
                    class = Some(name);
                }
            }
        }
        if line.starts_with("#[Test") || line.contains("@test") {
            attribute_test = true;
        }
        if let Some(at) = words.iter().position(|w| *w == "function") {
            if let Some(name) = words.get(at + 1) {
                let name: String = name
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if index <= line0 && (name.starts_with("test") || attribute_test) {
                    method = Some(name);
                } else if index <= line0 {
                    method = None;
                }
                attribute_test = false;
            }
        }
        if index >= line0 && class.is_some() {
            // Keep scanning only until the caret's line.
            break;
        }
    }
    let class = class?;
    let fqcn = if namespace.is_empty() {
        class
    } else {
        format!("{namespace}\\{class}")
    };
    Some((fqcn, method))
}

/// Splits command line arguments, honouring quotes.
pub fn split_args(text: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for c in text.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => current.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started || !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            (None, c) => current.push(c),
        }
    }
    if started || !current.is_empty() {
        args.push(current);
    }
    args
}

pub fn join_args(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            if a.is_empty() || a.contains(char::is_whitespace) {
                format!("\"{a}\"")
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Run › Edit Configurations.
pub struct EditDialog {
    pub(crate) configs: Vec<RunConfig>,
    /// The list of configuration types to add is shown.
    adding: bool,
    selected: Option<usize>,
    error: Option<String>,
}

impl PhpIdeApp {
    /// Reads the project's configurations (project open).
    pub(crate) fn reload_run_configs(&mut self) {
        match load(&self.ws.root) {
            Ok(configs) => self.rd.configs = configs,
            Err(error) => {
                self.rd.configs.clear();
                self.ws.message = error;
            }
        }
        self.rd.temporary.clear();
        self.rd.selected_config_id = None;
    }

    fn persist_run_configs(&mut self) {
        if let Err(error) = save(&self.ws.root, &self.rd.configs) {
            self.ws.message = error;
        }
    }

    /// The selected configuration (saved or temporary).
    pub(crate) fn selected_run_config(&self) -> Option<&RunConfig> {
        let id = self.rd.selected_config_id.as_ref()?;
        self.rd
            .configs
            .iter()
            .chain(self.rd.temporary.iter())
            .find(|c| &c.id == id)
    }

    /// Shift+F10 / Shift+F9.
    pub(crate) fn run_selected(&mut self, debug: bool) {
        match self.selected_run_config().cloned() {
            Some(config) => self.run_config(config, debug),
            None => self.run_context(debug),
        }
    }

    /// Ctrl+Shift+F10 / Ctrl+Shift+F9: the test at the caret, or the file.
    pub(crate) fn run_context(&mut self, debug: bool) {
        self.save_active();
        let Some(buf) = self.ws.active_buffer() else {
            self.ws.message = "open a PHP file to run it".to_string();
            return;
        };
        let Some(path) = buf.path.clone() else {
            self.ws.message = "save the file before running it".to_string();
            return;
        };
        let rel = path
            .strip_prefix(&self.ws.root)
            .unwrap_or(&path)
            .to_path_buf();
        let line0 = buf
            .document
            .rope()
            .char_to_line(self.cursor_char.min(buf.document.len_chars()));
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let test = test_at(&buf.document.text(), line0);
        let config = match test {
            Some((class, method)) => {
                let short = class.rsplit('\\').next().unwrap_or(&class).to_string();
                let (name, scope) = match method {
                    Some(method) => (
                        format!("{short}::{method}"),
                        TestScope::Method { class, method },
                    ),
                    None => (
                        short,
                        TestScope::Class {
                            class,
                            file: Some(rel.clone()),
                        },
                    ),
                };
                RunConfig::new(
                    format!("temp-test-{name}"),
                    name,
                    RunKind::PhpUnit {
                        scope,
                        config: None,
                        bootstrap: None,
                        runner_options: Vec::new(),
                        coverage: false,
                    },
                )
            }
            None => RunConfig::new(
                format!("temp-php-{}", rel.display()),
                file_name,
                RunKind::PhpScript {
                    file: rel,
                    args: Vec::new(),
                    interpreter_options: Vec::new(),
                    break_at_first_line: false,
                },
            ),
        };
        self.add_temporary(config.clone());
        self.run_config(config, debug);
    }

    fn add_temporary(&mut self, config: RunConfig) {
        self.rd.temporary.retain(|c| c.id != config.id);
        if !self.rd.configs.iter().any(|c| c.id == config.id) {
            self.rd.temporary.push(config.clone());
            if self.rd.temporary.len() > TEMPORARY {
                self.rd.temporary.remove(0);
            }
        }
        self.rd.selected_config_id = Some(config.id);
    }

    /// Promotes a temporary configuration to a saved one.
    pub(crate) fn save_temporary(&mut self, id: &str) {
        if let Some(at) = self.rd.temporary.iter().position(|c| c.id == id) {
            let mut config = self.rd.temporary.remove(at);
            config.id = new_id("config");
            config.store_as_project_file = true;
            self.rd.selected_config_id = Some(config.id.clone());
            self.rd.configs.push(config);
            self.persist_run_configs();
        }
    }

    /// Runs (or debugs) a configuration in the Run (Debug) tool window.
    pub(crate) fn run_config(&mut self, mut config: RunConfig, debug: bool) {
        self.save_all_quietly();
        let php = match crate::exec::resolve_php(&self.settings) {
            Ok(program) => program,
            Err(e) => {
                self.ws.message = e.to_string();
                return;
            }
        };
        let mut ctx = crate::exec::build_context(&self.settings, &php);
        if ctx.composer.is_none() {
            ctx.composer = crate::exec::resolve_composer(&self.settings).ok();
        }
        // A running instance is stopped first unless several are allowed.
        if !config.allow_multiple_instances {
            for id in self.rd.manager.live_for(&config.id) {
                self.rd.manager.stop(id);
            }
        }
        if debug {
            if let RunKind::PhpUnit { coverage, .. } = &mut config.kind {
                *coverage = false;
            }
            match self.rd.start_debug_config(&self.ws.root, &ctx, config) {
                Ok(()) => {
                    self.tw.set_visible(ToolWindow::Debug, true);
                    self.ws.message = format!(
                        "Debug: waiting for Xdebug on 127.0.0.1:{} (IDE key phpide)",
                        crate::run_debug::RunDebugState::debug_port()
                    );
                }
                Err(e) => self.ws.message = e,
            }
            return;
        }
        match phpide_run::build_command(&config, &self.ws.root, &ctx) {
            Ok(built) => {
                self.tw.set_visible(ToolWindow::Run, true);
                let id = self.rd.manager.start(config, built.spec, built.plan);
                self.rd.selected = Some(id);
            }
            Err(e) => self.ws.message = format!("{} — {}", e.message, e.action),
        }
    }

    /// Runs the tests with coverage (Run › Run with Coverage).
    pub(crate) fn run_with_coverage(&mut self) {
        let config = match self.selected_run_config().cloned() {
            Some(config) if matches!(config.kind, RunKind::PhpUnit { .. }) => config,
            _ => RunConfig::new(
                "temp-tests-coverage",
                "All tests",
                RunKind::PhpUnit {
                    scope: TestScope::All,
                    config: None,
                    bootstrap: None,
                    runner_options: Vec::new(),
                    coverage: true,
                },
            ),
        };
        let mut config = config;
        if let RunKind::PhpUnit { coverage, .. } = &mut config.kind {
            *coverage = true;
        }
        self.run_config(config, false);
    }

    fn save_all_quietly(&mut self) {
        for buffer in &mut self.ws.buffers {
            if buffer.dirty && buffer.path.is_some() {
                let _ = buffer.save();
            }
        }
    }

    pub(crate) fn open_edit_configurations(&mut self) {
        let selected = self
            .rd
            .selected_config_id
            .as_ref()
            .and_then(|id| self.rd.configs.iter().position(|c| &c.id == id))
            .or((!self.rd.configs.is_empty()).then_some(0));
        self.run_dialog = Some(EditDialog {
            adding: self.rd.configs.is_empty(),
            configs: self.rd.configs.clone(),
            selected,
            error: None,
        });
    }

    pub(crate) fn edit_configurations_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.run_dialog.as_mut() else {
            return;
        };
        let root = self.ws.root.clone();
        let composer_scripts: Vec<String> = crate::exec::composer_scripts(&root)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        let p = crate::design::current(ctx).palette;
        let mut close = false;
        let mut apply = false;
        let id = egui::Id::new("edit-run-configurations");
        egui::Modal::new(id)
            .area(
                egui::Modal::default_area(id)
                    .fixed_pos(ctx.content_rect().center() - egui::vec2(424.0, 300.0)),
            )
            .backdrop_color(p.shadow)
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                Dialog::frame(ui)
                    .inner_margin(egui::Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(820.0);
                        ui.label(egui::RichText::new("Run/Debug Configurations").strong());
                        ui.add_space(8.0);
                        ui.horizontal_top(|ui| {
                            // A fixed height: the vertical separator fills the
                            // available height, which would otherwise grow with
                            // the area's remembered size every frame.
                            ui.set_min_height(440.0);
                            ui.set_max_height(440.0);
                            // List.
                            ui.vertical(|ui| {
                                ui.set_width(240.0);
                                ui.horizontal(|ui| {
                                    if crate::design::components::IconButton::new(
                                        phpide_icons::IconId::Plus,
                                        "Add New Configuration",
                                    )
                                    .selected(dialog.adding)
                                    .show(ui)
                                    .clicked()
                                    {
                                        dialog.adding = !dialog.adding;
                                    }
                                    let has = dialog.selected.is_some();
                                    if crate::design::components::IconButton::new(
                                        phpide_icons::IconId::Minus,
                                        "Remove Configuration",
                                    )
                                    .enabled(has)
                                    .show(ui)
                                    .clicked()
                                    {
                                        if let Some(at) = dialog.selected {
                                            dialog.configs.remove(at);
                                            dialog.selected = (!dialog.configs.is_empty())
                                                .then(|| at.min(dialog.configs.len() - 1));
                                        }
                                    }
                                    if crate::design::components::IconButton::new(
                                        phpide_icons::IconId::Copy,
                                        "Copy Configuration",
                                    )
                                    .enabled(has)
                                    .show(ui)
                                    .clicked()
                                    {
                                        if let Some(at) = dialog.selected {
                                            let mut copy = dialog.configs[at].clone();
                                            copy.id = new_id("config");
                                            copy.name = format!("{} (copy)", copy.name);
                                            dialog.configs.push(copy);
                                            dialog.selected = Some(dialog.configs.len() - 1);
                                        }
                                    }
                                });
                                if dialog.adding {
                                    ui.label(egui::RichText::new("Choose the type").weak().small());
                                    for (label, which) in [
                                        ("PHP Script", "php"),
                                        ("PHPUnit", "phpunit"),
                                        ("Composer Script", "composer"),
                                    ] {
                                        let kind = new_kind(which);
                                        if ListRow::new(label)
                                            .icon(kind_icon(&kind), p.accent)
                                            .show(ui)
                                            .clicked()
                                        {
                                            let mut config = RunConfig::new(
                                                new_id("config"),
                                                format!("Unnamed {label}"),
                                                kind,
                                            );
                                            config.store_as_project_file = true;
                                            if let RunKind::ComposerScript { script } =
                                                &mut config.kind
                                            {
                                                *script = composer_scripts
                                                    .first()
                                                    .cloned()
                                                    .unwrap_or_default();
                                            }
                                            dialog.configs.push(config);
                                            dialog.selected = Some(dialog.configs.len() - 1);
                                            dialog.adding = false;
                                        }
                                    }
                                    ui.separator();
                                }
                                egui::ScrollArea::vertical()
                                    .id_salt("run-config-list")
                                    .max_height(360.0)
                                    .show(ui, |ui| {
                                        if dialog.configs.is_empty() {
                                            ui.weak("No configurations. Add one with +.");
                                        }
                                        for (index, config) in dialog.configs.iter().enumerate() {
                                            let row = ListRow::new(&config.name)
                                                .icon(kind_icon(&config.kind), p.accent)
                                                .detail(kind_label(&config.kind))
                                                .selected(dialog.selected == Some(index))
                                                .show(ui);
                                            if row.clicked() {
                                                dialog.selected = Some(index);
                                            }
                                        }
                                    });
                            });
                            ui.separator();
                            // Form.
                            ui.vertical(|ui| {
                                ui.set_width(540.0);
                                match dialog.selected {
                                    Some(at) => {
                                        config_form(ui, &mut dialog.configs[at], &composer_scripts);
                                        let issues = dialog.configs[at].validate(&root);
                                        for issue in issues {
                                            let color = match issue.severity {
                                                phpide_run::config::IssueSeverity::Error => p.error,
                                                phpide_run::config::IssueSeverity::Warning => {
                                                    p.warning
                                                }
                                            };
                                            ui.colored_label(
                                                color,
                                                format!("{}: {}", issue.field, issue.message),
                                            );
                                        }
                                    }
                                    None => {
                                        ui.weak("Select a configuration or add one.");
                                    }
                                }
                            });
                        });
                        if let Some(error) = &dialog.error {
                            ui.colored_label(p.error, error);
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if Button::primary("OK").show(ui).clicked() {
                                apply = true;
                                close = true;
                            }
                            if Button::new("Cancel").show(ui).clicked() {
                                close = true;
                            }
                            if Button::new("Apply").show(ui).clicked() {
                                apply = true;
                            }
                        });
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            close = true;
                        }
                    });
            });
        if apply {
            let dialog = self.run_dialog.as_ref().unwrap();
            self.rd.configs = dialog.configs.clone();
            if let Some(at) = dialog.selected {
                self.rd.selected_config_id = Some(self.rd.configs[at].id.clone());
            }
            self.persist_run_configs();
        }
        if close {
            self.run_dialog = None;
        }
    }
}

fn new_kind(which: &str) -> RunKind {
    match which {
        "phpunit" => RunKind::PhpUnit {
            scope: TestScope::All,
            config: None,
            bootstrap: None,
            runner_options: Vec::new(),
            coverage: false,
        },
        "composer" => RunKind::ComposerScript {
            script: String::new(),
        },
        _ => RunKind::PhpScript {
            file: PathBuf::new(),
            args: Vec::new(),
            interpreter_options: Vec::new(),
            break_at_first_line: false,
        },
    }
}

pub fn kind_icon(kind: &RunKind) -> phpide_icons::IconId {
    match kind {
        RunKind::PhpScript { .. } => phpide_icons::IconId::FilePhp,
        RunKind::PhpUnit { .. } => phpide_icons::IconId::Test,
        RunKind::ComposerScript { .. } => phpide_icons::IconId::Package,
    }
}

/// One labelled text field bound to a string.
fn field(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str, salt: &str) -> bool {
    ui.horizontal(|ui| {
        ui.add_sized([150.0, 20.0], egui::Label::new(label));
        TextInput::new(value)
            .hint(hint)
            .width(370.0)
            .id_salt(salt)
            .show(ui)
            .changed()
    })
    .inner
}

fn path_field(ui: &mut egui::Ui, label: &str, value: &mut Option<PathBuf>, hint: &str, salt: &str) {
    let mut text = value
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    if field(ui, label, &mut text, hint, salt) {
        *value = (!text.trim().is_empty()).then(|| PathBuf::from(text.trim()));
    }
}

fn config_form(ui: &mut egui::Ui, config: &mut RunConfig, composer_scripts: &[String]) {
    ui.spacing_mut().item_spacing.y = 6.0;
    field(
        ui,
        "Name",
        &mut config.name,
        "Configuration name",
        "rc-name",
    );
    ui.horizontal(|ui| {
        ui.add_sized([150.0, 20.0], egui::Label::new("Type"));
        ui.weak(kind_label(&config.kind));
    });
    ui.separator();
    match &mut config.kind {
        RunKind::PhpScript {
            file,
            args,
            interpreter_options,
            break_at_first_line,
        } => {
            let mut text = file.display().to_string();
            if field(ui, "File", &mut text, "public/index.php", "rc-file") {
                *file = PathBuf::from(text.trim());
            }
            let mut text = join_args(args);
            if field(ui, "Arguments", &mut text, "--verbose input.csv", "rc-args") {
                *args = split_args(&text);
            }
            let mut text = join_args(interpreter_options);
            if field(
                ui,
                "Interpreter options",
                &mut text,
                "-d memory_limit=1G",
                "rc-iopts",
            ) {
                *interpreter_options = split_args(&text);
            }
            Checkbox::new(break_at_first_line, "Break at first line (debug)").show(ui);
        }
        RunKind::PhpUnit {
            scope,
            config: phpunit_config,
            runner_options,
            coverage,
            ..
        } => {
            let scopes = ["All tests", "Directory", "Class", "Method"];
            let mut which = match scope {
                TestScope::All => 0,
                TestScope::Directory { .. } => 1,
                TestScope::Class { .. } => 2,
                TestScope::Method { .. } | TestScope::DataSet { .. } => 3,
            };
            let before = which;
            ui.horizontal(|ui| {
                ui.add_sized([150.0, 20.0], egui::Label::new("Test scope"));
                Select::new("rc-scope", &mut which, &scopes)
                    .width(200.0)
                    .show(ui);
            });
            if which != before {
                *scope = match which {
                    1 => TestScope::Directory {
                        path: PathBuf::from("tests"),
                    },
                    2 => TestScope::Class {
                        class: String::new(),
                        file: None,
                    },
                    3 => TestScope::Method {
                        class: String::new(),
                        method: String::new(),
                    },
                    _ => TestScope::All,
                };
            }
            match scope {
                TestScope::Directory { path } => {
                    let mut text = path.display().to_string();
                    if field(ui, "Directory", &mut text, "tests/Unit", "rc-dir") {
                        *path = PathBuf::from(text.trim());
                    }
                }
                TestScope::Class { class, file } => {
                    field(ui, "Class", class, "Tests\\Unit\\UserTest", "rc-class");
                    path_field(ui, "File", file, "tests/Unit/UserTest.php", "rc-class-file");
                }
                TestScope::Method { class, method } => {
                    field(ui, "Class", class, "Tests\\Unit\\UserTest", "rc-mclass");
                    field(ui, "Method", method, "testItWorks", "rc-method");
                }
                TestScope::DataSet {
                    class,
                    method,
                    data_set,
                } => {
                    field(ui, "Class", class, "", "rc-dclass");
                    field(ui, "Method", method, "", "rc-dmethod");
                    field(ui, "Data set", data_set, "", "rc-dset");
                }
                TestScope::All => {}
            }
            path_field(
                ui,
                "Configuration file",
                phpunit_config,
                "phpunit.xml (detected)",
                "rc-pcfg",
            );
            let mut text = join_args(runner_options);
            if field(
                ui,
                "Test runner options",
                &mut text,
                "--group fast",
                "rc-ropts",
            ) {
                *runner_options = split_args(&text);
            }
            Checkbox::new(coverage, "Collect coverage (Clover)").show(ui);
        }
        RunKind::ComposerScript { script } => {
            if composer_scripts.is_empty() {
                field(ui, "Script", script, "test", "rc-script");
            } else {
                let mut which = composer_scripts
                    .iter()
                    .position(|s| s == script)
                    .unwrap_or(0);
                ui.horizontal(|ui| {
                    ui.add_sized([150.0, 20.0], egui::Label::new("Script"));
                    let options: Vec<&str> = composer_scripts.iter().map(String::as_str).collect();
                    Select::new("rc-composer", &mut which, &options)
                        .width(240.0)
                        .show(ui);
                });
                *script = composer_scripts[which.min(composer_scripts.len() - 1)].clone();
            }
        }
    }
    ui.separator();
    path_field(
        ui,
        "Working directory",
        &mut config.cwd,
        "project root",
        "rc-cwd",
    );
    let mut env = config
        .env
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("; ");
    if field(
        ui,
        "Environment",
        &mut env,
        "APP_ENV=testing; DEBUG=1",
        "rc-env",
    ) {
        config.env = env
            .split(';')
            .filter_map(|pair| {
                let (k, v) = pair.split_once('=')?;
                let k = k.trim();
                (!k.is_empty()).then(|| (k.to_string(), v.trim().to_string()))
            })
            .collect();
    }
    let mut interpreter = config.interpreter.clone().unwrap_or_default();
    if field(
        ui,
        "PHP interpreter",
        &mut interpreter,
        "from Settings › PHP",
        "rc-php",
    ) {
        config.interpreter =
            (!interpreter.trim().is_empty()).then(|| interpreter.trim().to_string());
    }
    Checkbox::new(
        &mut config.allow_multiple_instances,
        "Allow multiple instances",
    )
    .show(ui);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_finds_class_and_method() {
        let text = "<?php\nnamespace Tests\\Unit;\n\nuse PHPUnit\\Framework\\TestCase;\n\nfinal class UserTest extends TestCase\n{\n    public function testName(): void\n    {\n        $this->assertTrue(true);\n    }\n\n    #[Test]\n    public function itWorks(): void\n    {\n    }\n}\n";
        assert_eq!(
            test_at(text, 9),
            Some(("Tests\\Unit\\UserTest".into(), Some("testName".into())))
        );
        assert_eq!(
            test_at(text, 14),
            Some(("Tests\\Unit\\UserTest".into(), Some("itWorks".into())))
        );
        assert_eq!(
            test_at(text, 5),
            Some(("Tests\\Unit\\UserTest".into(), None))
        );
        assert_eq!(test_at("<?php\necho 1;\n", 1), None);
    }

    #[test]
    fn arguments_round_trip() {
        let args = split_args("a \"b c\" 'd' ''");
        assert_eq!(args, vec!["a", "b c", "d", ""]);
        assert_eq!(split_args(&join_args(&args)), args);
    }

    #[test]
    fn configurations_persist_in_the_project() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).unwrap().is_empty());
        let config = RunConfig::new(
            "c1",
            "Serve",
            RunKind::PhpScript {
                file: PathBuf::from("public/index.php"),
                args: vec!["--port".into(), "8000".into()],
                interpreter_options: Vec::new(),
                break_at_first_line: false,
            },
        );
        save(dir.path(), std::slice::from_ref(&config)).unwrap();
        assert_eq!(load(dir.path()).unwrap(), vec![config]);
    }
}
