//! Project lifecycle: opening and switching projects, settings, theme,
//! Composer discovery, watcher and background services.

use super::*;

impl PhpIdeApp {
    pub(crate) fn new(root: PathBuf, start_welcome: bool, files: Vec<PathBuf>) -> Self {
        // While the Welcome screen is open there is no project yet: do not
        // scan, watch or start language servers for the current directory
        // (it is often $HOME when launched from a desktop entry).
        let has_project = !start_welcome;
        let fe = if has_project && scope::EXPERIMENTAL {
            fe_servers::detect_and_spawn(&root)
        } else {
            fe_servers::FeServers::empty()
        };
        let db = database::DatabaseState::new(&root);
        let (settings, settings_err) = settings::Settings::load_or_default();
        crate::commands::set_overrides(&settings.shortcuts);
        let theme = ThemeId::from_name(&settings.theme);
        let (lsp, lsp_init, lsp_name) = if has_project {
            start_lsp(&root)
        } else {
            (None, None, String::new())
        };
        let plugins = plugins::discover_plugins(&root, &settings::config_dir());
        let logger = app_recovery::make_logger();
        let session = std::process::id() as u64;
        logger.info(
            Component::Ui,
            &Correlation::session(session),
            "app_start",
            &[("root", &root.display().to_string())],
        );
        let mut app = PhpIdeApp {
            watcher: if has_project {
                start_watcher(&root)
            } else {
                None
            },
            db,
            // The file index is built in the background by `files`.
            ws: Workspace::unscanned(root.clone()),
            tree: Default::default(),
            project_open: !start_welcome,
            open_prompt: None,
            select_in: None,
            windows: if cfg!(test) {
                let dir = std::env::temp_dir().join(format!("phpide-test-{}", std::process::id()));
                services::windows::WindowRegistry::with_dirs(
                    dir.join("windows"),
                    dir.join("session.json"),
                )
            } else {
                services::windows::WindowRegistry::new()
            },
            event_log: Default::default(),
            toasts: Default::default(),
            cursor_char: 0,
            palette: PaletteState::new(),
            search: SearchState::new(),
            zoom: settings.zoom,
            change_pending_since: None,
            jobs: JobSystem::new(),
            logger,
            session,
            pending_close: None,
            lsp,
            lsp_init,
            lsp_name,
            lsp_crash_notified: false,
            lsp_opened: HashMap::new(),
            lsp_mirrors: HashMap::new(),
            lsp_raw_diagnostics: HashMap::new(),
            pending_requests: routing::LspRouting::default(),
            pending_outline: None,
            last_outline_key: None,
            symbol_results: Vec::new(),
            pending_symbols: None,
            last_symbol_query: String::new(),
            semantic: None,
            pending_semantic: None,
            occurrences: None,
            pending_occurrences: None,
            composer: None,
            composer_warned: false,
            composer_mtime: None,
            problems_scope: 0,
            completion: CompletionState::new(),
            snippet: None,
            intel: Default::default(),
            syntax: None,
            exec: ExecState::new(),
            extras: Vec::new(),
            was_palette_open: false,
            rd: run_debug::RunDebugState::default(),
            run_dialog: None,
            plugins,
            settings,
            settings_dialog: settings_dialog::SettingsDialogState::default(),
            theme,
            tw: toolwindows::ToolWindows::default(),
            welcome: WelcomeState::new(),
            welcome_open: start_welcome,
            scheme_open: false,
            goto_line_open: false,
            goto_line_input: String::new(),
            debug_listen: false,
            git: services::git::GitService::new(root.clone()),
            files: services::files::ProjectFiles::new(root.clone()),
            rss_cache: None,
            shift_was_down: false,
            last_shift_press: None,
            shift_alone: false,
            zoom_on_ctrl_wheel: false,
            bookmarks: HashMap::new(),
            editors: editor_area::EditorArea::default(),
            scroll_hook: std::env::var("PHPIDE_SCROLL_TO_LINE")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .map(|line| line.saturating_sub(1)),
            switcher: switcher::MruSwitcher::default(),
            hints: hints::HintStore::default(),
            hints_path: None,
            code_vision: hints::CodeVision::default(),
            blame: blame_gutter::BlameCache::default(),
            blame_revision: None,
            git_markers: None,
            inspections: lens::InspectionsSummary::default(),
            fe,
            refactor_undo: workspace_edit::UndoStack::default(),
            lens_hover: None,
            terminal: Default::default(),
            recovery: Default::default(),
            recent_locations: Default::default(),
            last_place: None,
            library_job: None,
            project_marks: Default::default(),
            vcs: services::vcs::VcsService::new(root.clone()),
            commit_ui: Default::default(),
            diffs: Default::default(),
            vcs_log: Default::default(),
            git_dialog: None,
        };
        if has_project {
            app.windows.set_project(Some(&app.ws.root.clone()));
            app.start_recovery();
        }
        // A crash report from the last run is shown once (tests set it
        // themselves: the folder is shared by every test of the process).
        if !cfg!(test) {
            app.recovery.crash_report =
                phpide_core::diag::unseen_crash_reports(&app_recovery::state_dir().join("crashes"))
                    .into_iter()
                    .next();
        }
        let placements = app.settings.tool_window_placements.clone();
        app.tw.restore_placements(&placements);
        // Screenshot hook: open Settings on a page.
        if let Ok(page) = std::env::var("PHPIDE_SETTINGS_PAGE") {
            if let Some(page) = settings_dialog::page_named(&page) {
                app.settings_dialog.open(page);
            }
        }
        // Closing Policy configurável (UI-ED-003): limite vem do settings.
        app.ws.tab_limit = app.settings.tab_limit.max(1) as usize;
        // Descoberta do projeto Composer (T-PHP-030-01/02): informativa —
        // nunca sobrescreve configuração manual explícita.
        app.refresh_composer(false);
        app.reload_run_configs();
        // Arquivos passados na CLI: abertos em abas (o último fica ativo).
        for f in &files {
            let path = if f.is_absolute() {
                f.clone()
            } else {
                app.ws.root.join(f)
            };
            app.ws.open_file_ex(&path, false);
        }
        if std::env::var_os("PHPIDE_SMOKE_PROJECT_CLOSE").is_some() {
            if let Some(buffer) = app.ws.active_buffer_mut() {
                buffer.insert(0, "// test edit\n");
            }
            app.pending_close = Some(PendingClose::Project(root.join("other")));
        }
        if std::env::var_os("PHPIDE_SMOKE_SAVE_FAILURE").is_some() {
            let active = app.ws.active;
            if let Some(buffer) = app.ws.buffers.get_mut(active) {
                buffer.insert(0, "// edit that must be kept\n");
                if let Some(path) = buffer.path.clone() {
                    let _ = std::fs::remove_file(&path);
                    let _ = std::fs::create_dir(&path);
                }
            }
            app.pending_close = Some(PendingClose::Tab(active));
        }
        if let Ok(url) = std::env::var("PHPIDE_DB_URL") {
            app.db.connect_url_for_smoke(&url);
        }
        if std::env::var("PHPIDE_TOOL_WINDOW").as_deref() == Ok("database") {
            app.tw.set_visible(ToolWindow::Database, true);
        }
        if let Ok(axis) = std::env::var("PHPIDE_SPLIT") {
            if let Some(path) = app
                .ws
                .active_buffer()
                .and_then(|buffer| buffer.path.clone())
            {
                let _ = path;
                let axis = if axis == "down" {
                    editor_area::SplitAxis::Down
                } else {
                    editor_area::SplitAxis::Right
                };
                app.editors.split(&app.ws, axis);
            }
        }
        if let Some(err) = settings_err {
            // Configuração inválida: defaults em uso, original preservado.
            app.ws.message = err.to_string();
            app.logger.warn(
                Component::Settings,
                &Correlation::session(app.session),
                "settings_recovered",
                &[("error", &err.log_line())],
            );
        }
        app
    }

    // -----------------------------------------------------------------
    // Estágio 6 — shell: projetos, temas, helpers compartilhados
    // -----------------------------------------------------------------

    /// Troca de projeto a partir da UI: com buffers sujos, confirma antes
    /// (NFR-007-02); limpo, carrega direto.
    pub fn request_load_project(&mut self, root: PathBuf, ctx: &egui::Context) {
        if self.ws.buffers.iter().any(|b| b.dirty) {
            self.pending_close = Some(PendingClose::Project(root));
        } else {
            self.load_project(root, ctx);
        }
    }

    /// Carrega outro projeto no lugar (Welcome Screen / Project widget).
    /// NFR-001: troca de projeto cancela workers da sessão anterior, para
    /// a execução em andamento e encerra o LSP antigo.
    pub fn load_project(&mut self, root: PathBuf, ctx: &egui::Context) {
        self.settings.touch_project(&root);
        self.persist_settings();
        // The previous project's buffers were saved or discarded.
        self.recovery.clear();
        self.palette.libraries = None;
        // Isola o trabalho do projeto anterior (NFR-001-04).
        let report = self.jobs.shutdown(Duration::from_secs(2));
        self.search.cancel();
        self.exec.stop_if_running();
        // NFR-001-04: encerra sessões de execução e a engine DBGp.
        self.rd.shutdown();
        self.logger.info(
            Component::Ui,
            &Correlation::session(self.session),
            "project_switch",
            &[
                ("cancelled_jobs", &report.cancelled.to_string()),
                ("unresponsive", &report.unresponsive.to_string()),
            ],
        );
        if let Some(old) = self.lsp.take() {
            old.stop();
        }
        self.watcher = start_watcher(&root);
        let (lsp, lsp_init, lsp_name) = start_lsp(&root);
        self.lsp = lsp;
        self.lsp_init = lsp_init;
        self.lsp_name = lsp_name;
        self.lsp_opened.clear();
        self.lsp_mirrors.clear();
        self.lsp_raw_diagnostics.clear();
        self.pending_requests.clear();
        self.pending_outline = None;
        self.last_outline_key = None;
        self.syntax = None;
        self.db.reset_project(&root);
        self.ws = Workspace::unscanned(root.clone());
        self.files = services::files::ProjectFiles::new(root.clone());
        self.reload_run_configs();
        // Shells of the previous project end with it.
        self.terminal = Default::default();
        self.git = services::git::GitService::new(root.clone());
        self.reset_vcs(&root);
        self.ws.tab_limit = self.settings.tab_limit.max(1) as usize;
        self.refresh_composer(false);
        self.tree = Default::default();
        self.project_open = true;
        self.windows.set_project(Some(&root));
        self.start_recovery();
        self.ws.open_tabs_at_end = self.settings.tabs_open_at_end;
        self.welcome_open = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "PHP IDE — {}",
            root.display()
        )));
        self.ws.message = format!("opened project: {}", root.display());
    }

    pub fn persist_settings(&mut self) {
        self.settings.zoom = self.zoom;
        self.settings.theme = self.theme.name().to_string();
        // Interaction tests drive real app instances: they must not write
        // temporary projects into the user's settings.
        if !cfg!(test) {
            self.settings.save();
        }
    }

    /// Opens `root` as asked by the "open project" setting: in this
    /// window, in a new one, or after asking (RF-PRJ-08).
    pub fn open_project_at(&mut self, root: PathBuf, ctx: &egui::Context) {
        if !self.project_open || root == self.ws.root && self.project_open {
            self.request_load_project(root, ctx);
            return;
        }
        // Already open in another window: bring that one forward.
        if let Some(window) = self.windows.others().iter().find(|w| w.root == root) {
            self.windows
                .send(window.pid, services::windows::WindowCommand::Focus);
            self.welcome_open = false;
            return;
        }
        match self.settings.open_project_mode.as_str() {
            "current" => self.request_load_project(root, ctx),
            "new" => self.open_in_new_window(&root),
            _ => self.open_prompt = Some(root),
        }
    }

    /// Adds `root` to this window's project tree and file index (RF-PRJ-08).
    pub fn attach_project(&mut self, root: &Path) {
        let main = self.ws.root.display().to_string();
        let attached = root.display().to_string();
        if !self
            .settings
            .attached_projects
            .iter()
            .any(|(m, a)| *m == main && *a == attached)
        {
            self.settings.attached_projects.push((main, attached));
            self.persist_settings();
        }
        self.tw.set_visible(ToolWindow::Project, true);
        self.ws.message = format!("Attached {}", root.display());
    }

    /// Removes an attached project from this window.
    pub fn detach_project(&mut self, root: &Path) {
        let main = self.ws.root.display().to_string();
        let attached = root.display().to_string();
        self.settings
            .attached_projects
            .retain(|(m, a)| !(*m == main && *a == attached));
        self.persist_settings();
        self.ws.message = format!("Detached {}", root.display());
    }

    /// Starts another PHP IDE process on `root`.
    pub fn open_in_new_window(&mut self, root: &Path) {
        let exe = std::env::current_exe();
        let spawned = exe.and_then(|exe| {
            std::process::Command::new(exe)
                .arg(root)
                .stdin(std::process::Stdio::null())
                .spawn()
        });
        self.ws.message = match spawned {
            Ok(_) => {
                self.settings.touch_project(root);
                // This window keeps its own project as the one to reopen.
                self.settings.last_project = self.ws.root.display().to_string();
                self.persist_settings();
                format!("Opened {} in a new window", root.display())
            }
            Err(e) => format!("could not open a new window: {e}"),
        };
        self.welcome_open = false;
    }

    /// Close Project: stops the project's services and shows Home
    /// (RF-PRJ-12). Unsaved files were handled by the caller.
    /// Recovery for the open project (what a crash left is offered).
    fn start_recovery(&mut self) {
        self.windows.refresh();
        let root = self.ws.root.clone();
        let elsewhere = self.windows.others().iter().any(|w| w.root == root);
        let crash = self.recovery.crash_report.take();
        self.recovery = app_recovery::RecoveryState::for_project(&root, elsewhere);
        self.recovery.crash_report = crash;
    }

    pub fn close_project(&mut self, ctx: &egui::Context) {
        self.recovery.clear();
        self.recovery = Default::default();
        let _ = self.jobs.shutdown(Duration::from_secs(2));
        self.search.cancel();
        self.exec.stop_if_running();
        self.rd.shutdown();
        self.terminal = Default::default();
        if let Some(old) = self.lsp.take() {
            old.stop();
        }
        self.lsp_init = None;
        self.lsp_opened.clear();
        self.lsp_mirrors.clear();
        self.lsp_raw_diagnostics.clear();
        self.watcher = None;
        let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
        self.ws = Workspace::unscanned(home.clone());
        self.files = services::files::ProjectFiles::new(home.clone());
        self.git = services::git::GitService::new(home.clone());
        self.reset_vcs(&home);
        self.tree = Default::default();
        self.editors = editor_area::EditorArea::default();
        self.project_open = false;
        self.welcome_open = true;
        self.welcome.focus_search = true;
        self.windows.project_closed();
        self.settings.last_project.clear();
        self.persist_settings();
        ctx.send_viewport_cmd(egui::ViewportCommand::Title("PHP IDE".into()));
    }

    pub fn apply_theme(&mut self, theme: ThemeId, ctx: &egui::Context) {
        self.theme = theme;
        self.persist_settings();
        self.apply_design(ctx);
    }

    /// Re-applies theme, density and editor font from the settings.
    pub fn apply_design(&mut self, ctx: &egui::Context) {
        crate::design::install(ctx, crate::settings_theme(&self.settings));
    }

    pub fn open_palette(&mut self, mode: PaletteMode) {
        self.palette.open_with(mode);
    }

    /// Folder marks of the project (its own, or composer.json's).
    /// Scans the non-project files Search Everywhere may include
    /// (RF-SEL-05), once per project, when the option is on.
    pub(crate) fn poll_libraries(&mut self, ctx: &egui::Context) {
        let wanted = self.palette.open && self.palette.include_non_project;
        if wanted && self.palette.libraries.is_none() && self.library_job.is_none() {
            let root = self.ws.root.clone();
            let excluded = self.project_marks.excluded.clone();
            let ctx = ctx.clone();
            self.library_job = Some(self.jobs.spawn(move |job| {
                let files = palette::scan_libraries(&root, &excluded, || job.is_cancelled());
                ctx.request_repaint();
                Ok(files)
            }));
        }
        if let Some(job) = &mut self.library_job {
            if let Some(outcome) = job.poll() {
                if let phpide_core::jobs::JobOutcome::Ok(files) = outcome {
                    self.palette.libraries = Some(std::sync::Arc::new(files));
                }
                self.library_job = None;
            }
        }
    }

    pub(crate) fn reload_marks(&mut self) {
        self.palette.libraries = None;
        self.project_marks =
            crate::project_marks::ProjectMarks::load(&self.ws.root, self.composer.as_ref());
        let excluded = self.project_marks.excluded.clone();
        self.files.set_excluded_dirs(excluded);
    }

    /// Re-executa a descoberta Composer (T-PHP-030-03): chamada na abertura,
    /// na troca de projeto e quando o watcher detecta mudança no
    /// composer.json. `from_watcher` evita mensagens no startup silencioso.
    pub fn refresh_composer(&mut self, from_watcher: bool) {
        match phpide_php::composer::discover(&self.ws.root) {
            Ok(info) => {
                let had_vendor = self.composer.as_ref().map(|c| c.vendor_present);
                self.composer = info.clone();
                self.reload_marks();
                if let Some(info) = info {
                    if from_watcher {
                        self.ws.message = format!(
                            "composer.json updated: {} (PHP {})",
                            info.name.as_deref().unwrap_or("project"),
                            info.php_level.as_deref().unwrap_or("?")
                        );
                    }
                    // Vendor ausente: avisar uma vez por sessão de descoberta.
                    if !info.vendor_present && (had_vendor != Some(false) || from_watcher) {
                        self.composer_warned = true;
                        self.ws.message = format!(
                            "vendor/ is missing in {} — run `composer install`",
                            self.ws.root.display()
                        );
                        self.logger.warn(
                            Component::Php,
                            &Correlation::session(self.session),
                            "composer_vendor_missing",
                            &[("root", &self.ws.root.display().to_string())],
                        );
                    }
                }
            }
            Err(e) => {
                self.composer = None;
                self.reload_marks();
                if from_watcher || !self.composer_warned {
                    self.composer_warned = true;
                    self.ws.message = format!("{} — action: {}", e.message, e.action);
                    self.logger.error(
                        Component::Php,
                        &Correlation::session(self.session),
                        "composer_invalid",
                        &[("error", &e.message)],
                    );
                }
            }
        }
    }

    pub fn git_branch(&self) -> String {
        self.git.branch_label()
    }

    pub fn git_refresh(&mut self) {
        self.git_markers = None;
        self.git.refresh();
    }

    /// Process RSS in MB for the status bar, read at most every 2 seconds.
    pub fn rss_mb(&mut self) -> u64 {
        if let Some((at, mb)) = self.rss_cache {
            if at.elapsed() < Duration::from_secs(2) {
                return mb;
            }
        }
        let mb = Self::read_rss_mb();
        self.rss_cache = Some((Instant::now(), mb));
        mb
    }

    pub(crate) fn read_rss_mb() -> u64 {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| {
                s.lines().find(|l| l.starts_with("VmRSS:")).and_then(|l| {
                    l.split_whitespace()
                        .nth(1)
                        .and_then(|v| v.parse::<u64>().ok())
                })
            })
            .map(|kb| kb / 1024)
            .unwrap_or(0)
    }

    /// Git views start over with another project.
    fn reset_vcs(&mut self, root: &Path) {
        self.vcs = services::vcs::VcsService::new(root.to_path_buf());
        self.commit_ui = Default::default();
        self.diffs = Default::default();
        self.vcs_log = Default::default();
        self.git_dialog = None;
    }

    pub(crate) fn poll_watcher(&mut self, ctx: &egui::Context) {
        if let Some((_, rx)) = &self.watcher {
            let mut any = false;
            while rx.try_recv().is_ok() {
                any = true;
            }
            if any {
                self.change_pending_since = Some(Instant::now());
                self.vcs.refresh();
            }
        }
        if let Some(since) = self.change_pending_since {
            if since.elapsed() > Duration::from_millis(300) {
                self.change_pending_since = None;
                self.files.refresh(&self.jobs, ctx);
                // Mudança externa no composer.json → re-descoberta
                // (T-PHP-030-03: alteração externa solicita reload).
                let composer_file = self.ws.root.join("composer.json");
                let mtime = std::fs::metadata(&composer_file)
                    .and_then(|m| m.modified())
                    .ok();
                if mtime.is_some() && mtime != self.composer_mtime {
                    self.composer_mtime = mtime;
                    self.refresh_composer(true);
                }
                let changed: Vec<PathBuf> = self
                    .ws
                    .buffers
                    .iter()
                    .filter_map(|b| b.path.clone())
                    .collect();
                let reloaded = self.ws.reload_clean_buffers(&changed);
                if !reloaded.is_empty() {
                    self.ws.message = format!("{} file(s) reloaded from disk", reloaded.len());
                }
            }
        }
    }

    /// Stores a finished `git blame` and derives the "code author" lenses
    /// of the active file from it.
    pub(crate) fn apply_blame(&mut self, reply: services::git::BlameReply) {
        let key = reply.path.to_string_lossy().into_owned();
        self.blame
            .load(&key, reply.text_hash, reply.entries.clone());
        self.blame_revision = Some((reply.path.clone(), reply.revision));
        let active = self.ws.active_buffer().and_then(|b| b.path.clone());
        if active.as_deref() != Some(reply.path.as_path()) {
            return;
        }
        let Some((_, cache)) = &self.syntax else {
            return;
        };
        let Some(buf) = self.ws.active_buffer() else {
            return;
        };
        let Some(blocks) = cache.get(&buf.document) else {
            return;
        };
        let entries = reply.entries;
        for b in blocks.iter().filter(|b| {
            matches!(
                b.kind.as_str(),
                "class_declaration" | "method_declaration" | "function_declaration"
            )
        }) {
            self.code_vision
                .set_code_author(b.start_line0, b.start_line0, b.end_line0, |line| {
                    entries
                        .iter()
                        .find(|e| line >= e.final_start as usize && line <= e.final_end() as usize)
                        .map(|e| (e.author.as_str(), e.timestamp))
                });
        }
    }

    /// Results of the file index and Git jobs.
    pub(crate) fn poll_project_services(&mut self, ctx: &egui::Context) {
        let attached = self.settings.attached_to(&self.ws.root);
        self.files.set_attached(&self.jobs, ctx, attached);
        if let Some(index) = self.files.poll(&self.jobs, ctx) {
            self.ws.files = index;
        }
        let events = self.git.poll(&self.jobs, ctx);
        if let Some(reply) = events.markers {
            self.git_markers = Some((reply.path, reply.revision, reply.markers));
        }
        if let Some(reply) = events.blame {
            self.apply_blame(reply);
        }
    }

    pub(crate) fn poll_search(&mut self) {
        let job_before = self.search.current_job();
        let was_busy = self.search.busy();
        self.search.poll();
        if was_busy && !self.search.busy() {
            if let Some(job) = job_before {
                self.logger.info(
                    Component::Search,
                    &Correlation::session(self.session).with_job(job),
                    "search_finished",
                    &[("results", &self.search.results.len().to_string())],
                );
            }
        }
    }

    pub(crate) fn palette_extras(&self) -> Vec<(String, String)> {
        let mut v = vec![
            (
                "Run: Run Current File (Shift+F10)".to_string(),
                "php:run".to_string(),
            ),
            (
                "Run: Debug Current File (Shift+F9)".to_string(),
                "debug:run".to_string(),
            ),
        ];
        for (name, desc) in exec::composer_scripts(&self.ws.root) {
            let label = if desc.is_empty() {
                format!("Composer: {name}")
            } else {
                format!("Composer: {name} — {desc}")
            };
            v.push((label, format!("composer:{name}")));
        }
        if self.ws.root.join("vendor/bin/phpunit").exists() {
            v.push((
                "Tests: PHPUnit (vendor/bin/phpunit)".to_string(),
                "phpunit".to_string(),
            ));
        }
        for p in &self.plugins {
            v.push((p.label.clone(), p.id.clone()));
        }
        v
    }

    pub(crate) fn run_plugin(&mut self, id: &str) {
        let root = self.ws.root.clone();
        match id {
            "php:run" => self.run_current_php(),
            "phpunit" => {
                let bin = root.join("vendor/bin/phpunit");
                self.exec
                    .run("PHPUnit".into(), bin.display().to_string(), vec![], root);
            }
            "debug:run" => self.debug_run(),
            _ => {
                if let Some(script) = id.strip_prefix("composer:") {
                    let composer = match exec::resolve_composer(&self.settings) {
                        Ok(program) => program,
                        Err(e) => {
                            self.ws.message = e.to_string();
                            return;
                        }
                    };
                    let script = script.to_string();
                    self.exec.run(
                        format!("composer {script}"),
                        composer,
                        vec!["run-script".into(), script],
                        root,
                    );
                } else if let Some(plugin_id) = id.strip_prefix("plugin:") {
                    self.run_manifest_plugin(plugin_id);
                } else {
                    self.ws.message = format!("unknown command: {id}");
                }
            }
        }
    }

    pub(crate) fn run_manifest_plugin(&mut self, id: &str) {
        let wanted = format!("plugin:{id}");
        let Some(p) = self
            .plugins
            .iter()
            .find(|p| p.id == wanted)
            .or_else(|| self.plugins.iter().find(|p| p.id == id))
        else {
            self.ws.message = format!("plugin not found: {id}");
            return;
        };
        let (label, cmd, args) = (p.label.clone(), p.command.clone(), p.args.clone());
        let root = self.ws.root.clone();
        self.exec.run(label, cmd, args, root);
    }

    pub(crate) fn set_zoom(&mut self, zoom: f32) {
        self.zoom = settings::clamp_zoom(zoom);
        self.persist_settings();
    }
}

pub(crate) fn start_lsp(root: &Path) -> (Option<LspClient>, Option<mpsc::Receiver<Value>>, String) {
    // The built-in fixture server, for tests, screenshots and demos.
    if std::env::var_os("PHPIDE_FIXTURE_LSP").is_some() {
        return match phpide_lsp::fixture::start(root) {
            Ok((client, rx)) => (Some(client), Some(rx), "fixture-php".to_string()),
            Err(_) => (None, None, String::new()),
        };
    }
    match detect_server(root) {
        Some((cmd, args)) => {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            match LspClient::spawn(&cmd, &refs, root) {
                Ok((client, rx)) => {
                    eprintln!("[lsp] server: {cmd} {}", args.join(" "));
                    let name = Path::new(&cmd)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or(cmd);
                    (Some(client), Some(rx), name)
                }
                Err(e) => {
                    eprintln!("[lsp] could not start: {e}");
                    (None, None, String::new())
                }
            }
        }
        None => {
            eprintln!("[lsp] no server found (intelephense/phpactor)");
            (None, None, String::new())
        }
    }
}

pub(crate) fn start_watcher(
    root: &Path,
) -> Option<(notify::RecommendedWatcher, mpsc::Receiver<()>)> {
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            // Reading a file is not a change (and the IDE's own reads would
            // otherwise trigger endless reloads); Git's metadata is not
            // project content.
            if matches!(event.kind, notify::EventKind::Access(_)) {
                return;
            }
            let relevant = event.paths.iter().any(|p| {
                !p.components().any(|c| c.as_os_str() == ".git")
                    && (p.is_dir() || phpide_php::Php::is_php_file(p))
            });
            if relevant {
                let _ = tx.send(());
            }
        }
    })
    .ok()?;
    watcher.watch(root, notify::RecursiveMode::Recursive).ok()?;
    Some((watcher, rx))
}
