//! Run and Debug tool windows (Phase 6).
//!
//! Run: one tab per session (state icon, pin, close), a toolbar (Rerun,
//! Rerun Failed Tests, Stop, Pause Output, Pin, Clear Finished) and the
//! session's output with clickable `file:line` references. PHPUnit sessions
//! show the live test tree, failure details and the coverage summary.

use eframe::egui;

use crate::design::components::{menu_item, IconButton, ListRow, Tab, TreeRow};
use phpide_dbgp::session::{ContinueCommand, Watch};
use phpide_icons::IconId;
use phpide_run::session::{SessionId, SessionState};
use phpide_run::{RunKind, TestNode, TestStatus};

use crate::run_debug::DebugStatus;
use crate::PhpIdeApp;

/// Icon and colour of a session state.
fn state_icon(state: SessionState, p: &crate::design::tokens::Palette) -> (IconId, egui::Color32) {
    match state {
        SessionState::Starting | SessionState::Running => (IconId::Run, p.success),
        SessionState::PausedOutput => (IconId::Pause, p.warning),
        SessionState::Finished => (IconId::Success, p.text_secondary),
        SessionState::Stopped => (IconId::Stop, p.text_secondary),
        SessionState::Failed => (IconId::Error, p.error),
    }
}

/// The Run tool window.
pub fn run_panel(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let placement = app.tw.placement(crate::toolwindows::ToolWindow::Run);
    crate::toolwindows::dock(
        ui,
        crate::toolwindows::ToolWindow::Run,
        placement,
        260.0,
        |ui| {
            crate::tool_window_header(app, ui, crate::toolwindows::ToolWindow::Run);
            if app.rd.manager.sessions().is_empty() {
                crate::design::components::EmptyState::new(IconId::Run, "Nothing has run yet")
                    .description(
                        "Shift+F10 runs the selected configuration, Ctrl+Shift+F10 the file or test at the caret.",
                    )
                    .show(ui);
                return;
            }
            session_tabs(app, ui);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(30.0);
                    session_toolbar(app, ui);
                });
                ui.separator();
                ui.vertical(|ui| session_content(app, ui));
            });
        },
    );
}

fn session_tabs(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let p = crate::design::current(ui.ctx()).palette;
    let sessions: Vec<(SessionId, String, SessionState, bool)> = app
        .rd
        .manager
        .sessions()
        .iter()
        .map(|s| (s.id, s.title(), s.state, s.pinned))
        .collect();
    if app
        .rd
        .selected
        .is_none_or(|id| !sessions.iter().any(|s| s.0 == id))
    {
        app.rd.selected = sessions.last().map(|s| s.0);
    }
    let mut close = None;
    egui::ScrollArea::horizontal()
        .id_salt("run-tabs")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (id, title, state, pinned) in &sessions {
                    let (icon, color) = state_icon(*state, &p);
                    let response = Tab::new(title)
                        .icon(icon, color)
                        .pinned(*pinned)
                        .active(app.rd.selected == Some(*id))
                        .closable(true)
                        .show(ui);
                    if response.response.clicked() {
                        app.rd.selected = Some(*id);
                    }
                    if response.close_clicked {
                        close = Some(*id);
                    }
                }
            });
        });
    if let Some(id) = close {
        app.rd.manager.stop(id);
        app.rd.manager.close(id);
        app.rd.test_trees.remove(&id);
        if app.rd.selected == Some(id) {
            app.rd.selected = None;
        }
    }
}

fn session_toolbar(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let Some(id) = app.rd.selected else { return };
    let Some(session) = app.rd.manager.session(id) else {
        return;
    };
    let live = session.state.is_live();
    let paused = session.state == SessionState::PausedOutput;
    let pinned = session.pinned;
    let is_test = matches!(session.config.kind, RunKind::PhpUnit { .. });
    let has_failures = app
        .rd
        .test_trees
        .get(&id)
        .is_some_and(|t| !t.failures().is_empty());
    let p = crate::design::current(ui.ctx()).palette;
    if IconButton::new(IconId::Rerun, "Rerun")
        .shortcut("Ctrl+F5")
        .tint(p.success)
        .show(ui)
        .clicked()
    {
        if let Some(new_id) = app.rd.manager.rerun(id) {
            app.rd.selected = Some(new_id);
        }
    }
    if is_test
        && IconButton::new(IconId::Test, "Rerun Failed Tests")
            .enabled(has_failures && !live)
            .tint(p.error)
            .show(ui)
            .clicked()
    {
        rerun_failed(app, id);
    }
    if IconButton::new(IconId::Stop, "Stop")
        .shortcut("Ctrl+F2")
        .enabled(live)
        .tint(p.error)
        .show(ui)
        .clicked()
    {
        app.rd.manager.stop(id);
    }
    if IconButton::new(
        if paused {
            IconId::Resume
        } else {
            IconId::Pause
        },
        "Pause Output",
    )
    .selected(paused)
    .enabled(live)
    .show(ui)
    .clicked()
    {
        if let Some(s) = app.rd.manager.session_mut(id) {
            s.set_output_paused(!paused);
        }
    }
    if IconButton::new(IconId::Pin, "Pin Tab")
        .selected(pinned)
        .show(ui)
        .clicked()
    {
        if let Some(s) = app.rd.manager.session_mut(id) {
            s.pinned = !pinned;
        }
    }
    if IconButton::new(IconId::Trash, "Close Finished Tabs")
        .show(ui)
        .clicked()
    {
        app.rd.manager.reclaim_finished(0);
    }
}

/// Runs the failed tests of a PHPUnit session again.
fn rerun_failed(app: &mut PhpIdeApp, id: SessionId) {
    let Some(tree) = app.rd.test_trees.get(&id) else {
        return;
    };
    let Some(session) = app.rd.manager.session(id) else {
        return;
    };
    let mut names: Vec<String> = Vec::new();
    collect_failed(&tree.roots, &mut names);
    if names.is_empty() {
        return;
    }
    let mut config = session.config.clone();
    config.id = format!("{}-failed", config.id);
    config.name = format!("Failed in {}", session.config.name);
    if let RunKind::PhpUnit {
        scope,
        runner_options,
        ..
    } = &mut config.kind
    {
        *scope = phpide_run::TestScope::All;
        let alternatives: Vec<String> = names
            .iter()
            .map(|n| phpide_run::phpunit::regex_escape(n))
            .collect();
        runner_options.retain(|o| o != "--filter");
        runner_options.push("--filter".into());
        runner_options.push(format!("::({})( |$)", alternatives.join("|")));
    }
    app.run_config(config, false);
}

fn collect_failed(nodes: &[TestNode], out: &mut Vec<String>) {
    for node in nodes {
        if node.is_suite {
            collect_failed(&node.children, out);
        } else if node.status.is_some_and(|s| s.is_failure()) {
            let method = node
                .name
                .split(" with data set")
                .next()
                .unwrap_or(&node.name);
            if !out.iter().any(|n| n == method) {
                out.push(method.to_string());
            }
        }
    }
}

fn session_content(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let Some(id) = app.rd.selected else {
        return;
    };
    let Some(session) = app.rd.manager.session(id) else {
        return;
    };
    let p = crate::design::current(ui.ctx()).palette;
    let is_test = matches!(session.config.kind, RunKind::PhpUnit { .. });
    let state = session.state;
    let exit = session.exit_code;
    let command = session.command_line();
    ui.horizontal(|ui| {
        ui.add(
            egui::Label::new(egui::RichText::new(command).monospace().weak().small()).truncate(),
        );
        ui.with_layout(
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| match (state.is_live(), exit) {
                (true, _) => {
                    crate::design::components::Spinner::show(ui, 12.0);
                }
                (false, Some(0)) => {
                    ui.colored_label(p.success, "exit code 0");
                }
                (false, Some(code)) => {
                    ui.colored_label(p.error, format!("exit code {code}"));
                }
                (false, None) => {
                    ui.weak(state.as_str());
                }
            },
        );
    });
    if is_test {
        test_view(app, ui, id);
    } else {
        output_view(app, ui, id);
    }
}

/// The output with clickable `file:line` references; stderr in red.
fn output_view(app: &mut PhpIdeApp, ui: &mut egui::Ui, id: SessionId) {
    let Some(session) = app.rd.manager.session(id) else {
        return;
    };
    let p = crate::design::current(ui.ctx()).palette;
    let lines: Vec<String> = session.output().lines().map(String::from).collect();
    let font = egui::FontId::monospace(crate::design::tokens::typography::BODY);
    let row_height = ui.ctx().fonts_mut(|f| f.row_height(&font));
    let mut open: Option<(String, usize)> = None;
    egui::ScrollArea::both()
        .id_salt(("run-output", id))
        .stick_to_bottom(true)
        .auto_shrink([false, false])
        .show_rows(ui, row_height, lines.len(), |ui, range| {
            for line in &lines[range] {
                let (text, color) = match line.strip_prefix("[stderr] ") {
                    Some(rest) => (rest, p.error),
                    None => (line.as_str(), p.text_primary),
                };
                let links = phpide_term::find_links(text);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let chars: Vec<char> = text.chars().collect();
                    let mut at = 0;
                    for link in &links {
                        if link.start > at {
                            let part: String = chars[at..link.start].iter().collect();
                            ui.label(egui::RichText::new(part).font(font.clone()).color(color));
                        }
                        let part: String = chars[link.start..link.end].iter().collect();
                        let response = ui.add(
                            egui::Label::new(
                                egui::RichText::new(part)
                                    .font(font.clone())
                                    .color(p.accent)
                                    .underline(),
                            )
                            .sense(egui::Sense::click()),
                        );
                        if response.clicked() {
                            open = Some((link.path.clone(), link.line.unwrap_or(1)));
                        }
                        at = link.end;
                    }
                    if at < chars.len() || chars.is_empty() {
                        let part: String = chars[at..].iter().collect();
                        ui.label(egui::RichText::new(part).font(font.clone()).color(color));
                    }
                });
            }
        });
    if let Some((path, line)) = open {
        app.open_reported_location(&path, line);
    }
}

/// Live test tree (left) and the selected test's details (right).
fn test_view(app: &mut PhpIdeApp, ui: &mut egui::Ui, id: SessionId) {
    let tree = match app.rd.test_trees.get(&id) {
        Some(tree) => tree.clone(),
        None => match app.rd.test_parsers.get(&id) {
            Some(parser) => parser.snapshot(),
            None => phpide_run::TestTree::default(),
        },
    };
    let p = crate::design::current(ui.ctx()).palette;
    let stats = tree.stats();
    ui.horizontal(|ui| {
        let (tone, summary) = if stats.failed + stats.incomplete > 0 {
            (
                p.error,
                format!(
                    "Tests failed: {}, passed: {}",
                    stats.failed + stats.incomplete,
                    stats.passed
                ),
            )
        } else if stats.total > 0 {
            (p.success, format!("Tests passed: {}", stats.passed))
        } else {
            (p.text_secondary, "Running tests…".to_string())
        };
        ui.colored_label(tone, summary);
        ui.weak(format!(
            "of {} tests · {} ms",
            stats.total,
            stats.duration.as_millis()
        ));
        if stats.skipped > 0 {
            ui.weak(format!("· {} skipped", stats.skipped));
        }
        if let Some(coverage) = &app.rd.coverage {
            ui.weak(format!(
                "· coverage {:.1}%",
                coverage.percent_tenths() as f32 / 10.0
            ));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut show = app.rd.show_passed;
            if IconButton::new(IconId::Success, "Show Passed")
                .selected(show)
                .show(ui)
                .clicked()
            {
                show = !show;
            }
            app.rd.show_passed = show;
        });
    });
    ui.separator();
    let height = ui.available_height();
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width((ui.available_width() * 0.45).max(220.0));
            egui::ScrollArea::vertical()
                .id_salt(("test-tree", id))
                .max_height(height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for root in &tree.roots {
                        test_row(app, ui, root, 0);
                    }
                });
        });
        ui.separator();
        ui.vertical(|ui| {
            egui::ScrollArea::vertical()
                .id_salt(("test-details", id))
                .max_height(height)
                .auto_shrink([false, false])
                .show(ui, |ui| test_details(app, ui, &tree, id));
        });
    });
}

fn status_icon(node: &TestNode, p: &crate::design::tokens::Palette) -> (IconId, egui::Color32) {
    let status = if node.is_suite {
        suite_status(node)
    } else {
        node.status
    };
    match status {
        Some(TestStatus::Passed) => (IconId::Success, p.success),
        Some(TestStatus::Failed) => (IconId::Error, p.error),
        Some(TestStatus::Incomplete) => (IconId::Warning, p.warning),
        Some(TestStatus::Skipped) => (IconId::Minus, p.text_muted),
        Some(TestStatus::Running) | None => (IconId::Loader, p.text_secondary),
    }
}

fn suite_status(node: &TestNode) -> Option<TestStatus> {
    let mut any_running = false;
    let mut any = false;
    let mut worst = TestStatus::Passed;
    let mut stack: Vec<&TestNode> = node.children.iter().collect();
    while let Some(n) = stack.pop() {
        if n.is_suite {
            stack.extend(n.children.iter());
            continue;
        }
        any = true;
        match n.status {
            Some(TestStatus::Failed) => worst = TestStatus::Failed,
            Some(TestStatus::Incomplete) if worst != TestStatus::Failed => {
                worst = TestStatus::Incomplete
            }
            Some(TestStatus::Running) | None => any_running = true,
            _ => {}
        }
    }
    if any_running {
        Some(TestStatus::Running)
    } else {
        any.then_some(worst)
    }
}

fn test_row(app: &mut PhpIdeApp, ui: &mut egui::Ui, node: &TestNode, depth: usize) {
    let p = crate::design::current(ui.ctx()).palette;
    let status = if node.is_suite {
        suite_status(node)
    } else {
        node.status
    };
    if !app.rd.show_passed && status == Some(TestStatus::Passed) {
        return;
    }
    let key = format!("{depth}:{}", node.name);
    let collapsed = app.rd.collapsed_tests.contains(&key);
    let (icon, color) = status_icon(node, &p);
    let mut row = TreeRow::new(short_name(&node.name))
        .depth(depth)
        .icon(icon, color)
        .selected(app.rd.selected_test.as_deref() == Some(key.as_str()));
    if node.is_suite {
        row = row.expanded(Some(!collapsed));
    }
    if let Some(duration) = node.duration {
        row = row.detail(format!("{} ms", duration.as_millis()));
    }
    let response = row.show(ui);
    if response.response.clicked() {
        app.rd.selected_test = Some(key.clone());
        if node.is_suite && response.toggled {
            if collapsed {
                app.rd.collapsed_tests.remove(&key);
            } else {
                app.rd.collapsed_tests.insert(key.clone());
            }
        }
    }
    if response.response.double_clicked() {
        if let Some((file, line)) = &node.location {
            app.ws.open_file_at(file, line.saturating_sub(1), 0);
            app.editors.request_focus();
        }
    }
    if node.is_suite && !collapsed {
        for child in &node.children {
            test_row(app, ui, child, depth + 1);
        }
    }
}

/// `Tests\Unit\UserTest` → `UserTest`.
fn short_name(name: &str) -> String {
    name.rsplit('\\').next().unwrap_or(name).to_string()
}

fn find_node<'a>(nodes: &'a [TestNode], key: &str, depth: usize) -> Option<&'a TestNode> {
    for node in nodes {
        if format!("{depth}:{}", node.name) == key {
            return Some(node);
        }
        if let Some(found) = find_node(&node.children, key, depth + 1) {
            return Some(found);
        }
    }
    None
}

fn test_details(
    app: &mut PhpIdeApp,
    ui: &mut egui::Ui,
    tree: &phpide_run::TestTree,
    id: SessionId,
) {
    let p = crate::design::current(ui.ctx()).palette;
    let node = app
        .rd
        .selected_test
        .as_ref()
        .and_then(|key| find_node(&tree.roots, key, 0))
        .cloned()
        .or_else(|| tree.failures().first().map(|n| (*n).clone()));
    let Some(node) = node else {
        // No test selected: the raw output.
        output_view(app, ui, id);
        return;
    };
    ui.label(egui::RichText::new(&node.name).strong());
    if let Some(message) = &node.message {
        ui.colored_label(p.error, message);
    }
    let mut open = None;
    if let Some(details) = &node.details {
        ui.add_space(4.0);
        for line in details.lines() {
            let links = phpide_term::find_links(line);
            match links.first() {
                Some(link) => {
                    if ui.link(egui::RichText::new(line).monospace()).clicked() {
                        open = Some((link.path.clone(), link.line.unwrap_or(1)));
                    }
                }
                None => {
                    ui.label(egui::RichText::new(line).monospace());
                }
            }
        }
    }
    if let Some((file, line)) = &node.location {
        ui.add_space(4.0);
        if ui
            .link(format!("{}:{line}", file.display()))
            .on_hover_text("Go to the test")
            .clicked()
        {
            open = Some((file.display().to_string(), *line));
        }
    }
    if node.message.is_none() && node.details.is_none() && !node.is_suite {
        ui.weak(match node.status {
            Some(TestStatus::Passed) => "Passed.",
            Some(TestStatus::Skipped) => "Skipped.",
            _ => "Running…",
        });
    }
    if let Some((path, line)) = open {
        app.open_reported_location(&path, line);
    }
}

// ---------------------------------------------------------------------------
// Debug tool window
// ---------------------------------------------------------------------------

/// The Debug tool window: stepping toolbar, frames, variables and watches.
pub fn debug_panel(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let placement = app.tw.placement(crate::toolwindows::ToolWindow::Debug);
    crate::toolwindows::dock(
        ui,
        crate::toolwindows::ToolWindow::Debug,
        placement,
        280.0,
        |ui| {
            crate::tool_window_header(app, ui, crate::toolwindows::ToolWindow::Debug);
            debug_toolbar(app, ui);
            ui.separator();
            if !app.rd.status.is_active() && app.rd.frames.is_empty() {
                crate::design::components::EmptyState::new(IconId::Debug, "No debug session")
                    .description(
                        format!("Shift+F9 debugs the selected configuration. Xdebug connects to port {} with the IDE key phpide.", crate::run_debug::RunDebugState::debug_port()),
                    )
                    .show(ui);
                return;
            }
            let height = ui.available_height();
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(280.0);
                    frames_list(app, ui, height);
                });
                ui.separator();
                ui.vertical(|ui| variables_tree(app, ui, height));
            });
        },
    );
}

fn debug_toolbar(app: &mut PhpIdeApp, ui: &mut egui::Ui) {
    let p = crate::design::current(ui.ctx()).palette;
    let paused = app.rd.status == DebugStatus::Paused;
    let active = app.rd.status.is_active();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let last_stop = app.rd.last_stop.is_some();
        let mut command = None;
        if tool(
            ui,
            IconId::Resume,
            "Resume Program",
            "F9",
            paused,
            Some(p.success),
        ) {
            command = Some(ContinueCommand::Resume);
        }
        if tool(ui, IconId::Stop, "Stop", "Ctrl+F2", active, Some(p.error)) {
            app.rd.stop();
        }
        if tool(ui, IconId::Close, "Detach", "", active, None) {
            app.rd.detach();
        }
        ui.separator();
        if tool(
            ui,
            IconId::Locate,
            "Show Execution Point",
            "Alt+F10",
            last_stop,
            None,
        ) {
            show_execution_point(app);
        }
        if tool(ui, IconId::StepOver, "Step Over", "F8", paused, None) {
            command = Some(ContinueCommand::StepOver);
        }
        if tool(ui, IconId::StepInto, "Step Into", "F7", paused, None) {
            command = Some(ContinueCommand::StepInto);
        }
        if tool(ui, IconId::StepOut, "Step Out", "Shift+F8", paused, None) {
            command = Some(ContinueCommand::StepOut);
        }
        if let Some(command) = command {
            app.rd.continue_debug(command);
        }
        ui.separator();
        if tool(
            ui,
            IconId::Command,
            "Evaluate Expression",
            "Alt+F8",
            paused,
            None,
        ) {
            app.rd.evaluate_open = true;
        }
        let muted = app.rd.bp_muted();
        if IconButton::new(IconId::Breakpoint, "Mute Breakpoints")
            .selected(muted)
            .show(ui)
            .clicked()
        {
            if let Ok(mut store) = app.rd.bp_store.lock() {
                store.muted = !muted;
            }
            app.rd.breakpoints_dirty = true;
        }
        if IconButton::new(IconId::Structure, "View Breakpoints")
            .shortcut("Ctrl+Shift+F8")
            .show(ui)
            .clicked()
        {
            app.rd.breakpoints_dialog = Some(None);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let status = match (app.rd.status, &app.rd.last_stop) {
                (DebugStatus::Paused, Some((file, line))) => format!(
                    "Paused at {}:{line}",
                    file.file_name()
                        .map(|n| n.to_string_lossy())
                        .unwrap_or_default()
                ),
                (DebugStatus::Listening, _) => format!(
                    "Waiting for Xdebug on port {}",
                    crate::run_debug::RunDebugState::debug_port()
                ),
                (DebugStatus::Running, _) => "Running".to_string(),
                _ => "Finished".to_string(),
            };
            ui.weak(status);
        });
    });
}

fn tool(
    ui: &mut egui::Ui,
    icon: IconId,
    label: &str,
    key: &str,
    enabled: bool,
    tint: Option<egui::Color32>,
) -> bool {
    let mut button = IconButton::new(icon, label).shortcut(key).enabled(enabled);
    if let Some(tint) = tint {
        button = button.tint(tint);
    }
    button.show(ui).clicked()
}

/// Alt+F10 — mostra o ponto de execução atual no editor.
pub fn show_execution_point(app: &mut PhpIdeApp) {
    if let Some((file, line)) = app.rd.last_stop.clone() {
        app.ws
            .open_file_at(&file, line.saturating_sub(1) as usize, 0);
        app.editors.request_focus();
    }
}

fn frames_list(app: &mut PhpIdeApp, ui: &mut egui::Ui, height: f32) {
    ui.label(egui::RichText::new("Frames").strong().small());
    let p = crate::design::current(ui.ctx()).palette;
    egui::ScrollArea::vertical()
        .id_salt("debug-frames")
        .max_height(height - 20.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let frames = app.rd.frames.clone();
            for (i, frame) in frames.iter().enumerate() {
                let location = frame
                    .file
                    .as_ref()
                    .and_then(|f| f.file_name())
                    .map(|n| format!("{}:{}", n.to_string_lossy(), frame.line))
                    .unwrap_or_else(|| format!("?:{}", frame.line));
                let row = ListRow::new(&frame.name)
                    .icon(
                        if i == 0 {
                            IconId::Run
                        } else {
                            IconId::ChevronRight
                        },
                        if i == 0 { p.success } else { p.text_secondary },
                    )
                    .trailing(location)
                    .selected(app.rd.selected_frame == i)
                    .show(ui);
                if row.clicked() {
                    app.rd.select_frame(i);
                    if let Some(file) = &frame.file {
                        app.ws
                            .open_file_at(file, frame.line.saturating_sub(1) as usize, 0);
                    }
                }
            }
            if frames.is_empty() {
                ui.weak(if app.rd.status == DebugStatus::Running {
                    "Running…"
                } else {
                    "Waiting for a breakpoint…"
                });
            }
        });
}

fn variables_tree(app: &mut PhpIdeApp, ui: &mut egui::Ui, height: f32) {
    let p = crate::design::current(ui.ctx()).palette;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Variables").strong().small());
        let response = crate::design::components::TextInput::new(&mut app.rd.new_watch)
            .hint("Add watch: expression, Enter")
            .id_salt("debug-new-watch")
            .width(260.0)
            .show(ui);
        if response.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && !app.rd.new_watch.trim().is_empty()
        {
            let next_id = app.rd.watches.iter().map(|w| w.id).max().unwrap_or(0) + 1;
            app.rd.watches.push(Watch {
                id: next_id,
                expression: app.rd.new_watch.trim().to_string(),
            });
            app.rd.new_watch.clear();
            app.rd.refresh_watches();
        }
    });
    egui::ScrollArea::vertical()
        .id_salt("debug-variables")
        .max_height(height - 24.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            // Watches first, as in the JetBrains debugger.
            let watches = app.rd.watches.clone();
            let mut remove = None;
            for (i, watch) in watches.iter().enumerate() {
                let value = app
                    .rd
                    .watch_values
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| "—".to_string());
                let response = TreeRow::new(format!("{} = {value}", watch.expression))
                    .icon(IconId::Eye, p.info)
                    .show(ui);
                response.response.context_menu(|ui| {
                    ui.set_min_width(180.0);
                    if menu_item(ui, Some(IconId::Trash), "Remove Watch", "", true).clicked() {
                        remove = Some(i);
                        ui.close();
                    }
                });
            }
            if let Some(i) = remove {
                app.rd.watches.remove(i);
                app.rd.watch_values.clear();
                app.rd.refresh_watches();
            }
            let variables = app.rd.variables.clone();
            for variable in &variables {
                variable_row(app, ui, variable, 0);
            }
            if variables.is_empty() && watches.is_empty() {
                ui.weak("Variables appear when the program pauses.");
            }
        });
}

/// A variable and, when expanded, its children (asked for lazily).
fn variable_row(
    app: &mut PhpIdeApp,
    ui: &mut egui::Ui,
    variable: &phpide_dbgp::session::Variable,
    depth: usize,
) {
    let p = crate::design::current(ui.ctx()).palette;
    let expandable = variable.num_children > 0;
    let expanded = app.rd.expanded.contains(&variable.fullname);
    let value = match (&variable.value, expandable) {
        (Some(value), _) if variable.type_name == "string" => format!("\"{value}\""),
        (Some(value), _) => value.clone(),
        (None, true) => format!("{} [{}]", variable.type_name, variable.num_children),
        (None, false) => variable.type_name.clone(),
    };
    let mut row = TreeRow::new(format!("{} = {value}", variable.name))
        .depth(depth)
        .icon(variable_icon(&variable.type_name), p.text_secondary)
        .detail(variable.type_name.clone());
    if expandable {
        row = row.expanded(Some(expanded));
    }
    let response = row.show(ui);
    if expandable && (response.toggled || response.response.double_clicked()) {
        app.rd.toggle_variable(&variable.fullname);
    }
    let copy_value = variable.value.clone().unwrap_or_default();
    let fullname = variable.fullname.clone();
    response.response.context_menu(|ui| {
        ui.set_min_width(180.0);
        if menu_item(ui, Some(IconId::Copy), "Copy Value", "", true).clicked() {
            ui.ctx().copy_text(copy_value.clone());
            ui.close();
        }
        if menu_item(ui, Some(IconId::Eye), "Add to Watches", "", true).clicked() {
            let next_id = app.rd.watches.iter().map(|w| w.id).max().unwrap_or(0) + 1;
            app.rd.watches.push(Watch {
                id: next_id,
                expression: fullname.clone(),
            });
            app.rd.refresh_watches();
            ui.close();
        }
    });
    if expandable && expanded {
        match app.rd.children.get(&variable.fullname).cloned() {
            Some(children) => {
                for child in &children {
                    variable_row(app, ui, child, depth + 1);
                }
            }
            None => {
                TreeRow::new("Loading…").depth(depth + 1).show(ui);
            }
        }
    }
}

fn variable_icon(type_name: &str) -> IconId {
    match type_name {
        "array" => IconId::FileJson,
        "object" => IconId::Package,
        "string" => IconId::FileText,
        "null" | "uninitialized" => IconId::Minus,
        _ => IconId::Hint,
    }
}

/// Evaluate Expression (Alt+F8).
pub fn evaluate_dialog(app: &mut PhpIdeApp, ctx: &egui::Context) {
    if !app.rd.evaluate_open {
        return;
    }
    let p = crate::design::current(ctx).palette;
    let mut close = false;
    egui::Modal::new(egui::Id::new("evaluate-expression"))
        .backdrop_color(p.shadow)
        .frame(egui::Frame::NONE)
        .show(ctx, |ui| {
            crate::design::components::Dialog::frame(ui)
                .inner_margin(egui::Margin::same(14))
                .show(ui, |ui| {
                    ui.set_width(520.0);
                    ui.label(egui::RichText::new("Evaluate Expression").strong());
                    ui.add_space(6.0);
                    let response =
                        crate::design::components::TextInput::new(&mut app.rd.eval_input)
                            .hint("Expression in the selected frame ($user->name, count($items))")
                            .id_salt("evaluate-input")
                            .width(500.0)
                            .show(ui);
                    response.request_focus();
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let expression = app.rd.eval_input.clone();
                        app.rd.evaluate(&expression);
                    }
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical()
                        .id_salt("evaluate-result")
                        .max_height(280.0)
                        .show(ui, |ui| match app.rd.eval_result.clone() {
                            Some(variable) => variable_row(app, ui, &variable, 0),
                            None if !app.rd.eval_output.is_empty() => {
                                ui.label(egui::RichText::new(&app.rd.eval_output).monospace());
                            }
                            None => {
                                ui.weak("Enter evaluates; the result appears here.");
                            }
                        });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if crate::design::components::Button::primary("Evaluate")
                            .show(ui)
                            .clicked()
                        {
                            let expression = app.rd.eval_input.clone();
                            app.rd.evaluate(&expression);
                        }
                        if crate::design::components::Button::new("Close")
                            .show(ui)
                            .clicked()
                        {
                            close = true;
                        }
                    });
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        close = true;
                    }
                });
        });
    if close {
        app.rd.evaluate_open = false;
    }
}

/// Height of the list and details area of View Breakpoints.
const BREAKPOINTS_BODY_HEIGHT: f32 = 320.0;

/// View Breakpoints (Ctrl+Shift+F8): enable, condition, remove, go to.
pub fn breakpoints_dialog(app: &mut PhpIdeApp, ctx: &egui::Context) {
    let Some(selected) = app.rd.breakpoints_dialog else {
        return;
    };
    let p = crate::design::current(ctx).palette;
    let breakpoints = app.rd.bp_store.lock().unwrap().breakpoints.clone();
    let mut selected = selected.or(breakpoints.first().map(|b| b.id));
    let mut close = false;
    let mut changed = false;
    let mut goto = None;
    egui::Modal::new(egui::Id::new("view-breakpoints"))
        .backdrop_color(p.shadow)
        .frame(egui::Frame::NONE)
        .show(ctx, |ui| {
            crate::design::components::Dialog::frame(ui)
                .inner_margin(egui::Margin::same(14))
                .show(ui, |ui| {
                    ui.set_width(720.0);
                    ui.label(egui::RichText::new("Breakpoints").strong());
                    ui.add_space(6.0);
                    ui.horizontal_top(|ui| {
                        // A fixed height: `horizontal_top` takes all the height
                        // left in the modal, the vertical separator fills it and
                        // the modal remembers that size, so without a cap the
                        // dialog grew every frame past the window edges.
                        ui.set_min_height(BREAKPOINTS_BODY_HEIGHT);
                        ui.set_max_height(BREAKPOINTS_BODY_HEIGHT);
                        ui.vertical(|ui| {
                            ui.set_width(320.0);
                            egui::ScrollArea::vertical()
                                .id_salt("breakpoint-list")
                                .max_height(BREAKPOINTS_BODY_HEIGHT)
                                .show(ui, |ui| {
                                    if breakpoints.is_empty() {
                                        ui.weak("No breakpoints. F9 or a click on the line numbers adds one.");
                                    }
                                    for bp in &breakpoints {
                                        let name = bp
                                            .file
                                            .as_ref()
                                            .and_then(|f| f.file_name())
                                            .map(|n| format!("{}:{}", n.to_string_lossy(), bp.line))
                                            .unwrap_or_else(|| format!("line {}", bp.line));
                                        let color = match &bp.status {
                                            phpide_dbgp::BreakpointStatus::Verified => p.debug_breakpoint,
                                            phpide_dbgp::BreakpointStatus::Invalid(_) => p.error,
                                            phpide_dbgp::BreakpointStatus::Disabled => p.text_muted,
                                            _ => p.debug_breakpoint,
                                        };
                                        let detail = match &bp.kind {
                                            phpide_dbgp::BreakpointKind::Conditional(c) => c.clone(),
                                            _ => bp.status.as_str().to_string(),
                                        };
                                        let row = ListRow::new(name)
                                            .icon(IconId::Breakpoint, color)
                                            .detail(detail)
                                            .selected(selected == Some(bp.id))
                                            .show(ui);
                                        if row.clicked() {
                                            selected = Some(bp.id);
                                        }
                                        if row.double_clicked() {
                                            goto = bp.file.clone().map(|f| (f, bp.line));
                                        }
                                    }
                                });
                        });
                        ui.separator();
                        ui.vertical(|ui| {
                            let Some(bp) = breakpoints.iter().find(|b| Some(b.id) == selected) else {
                                return;
                            };
                            let mut store = app.rd.bp_store.lock().unwrap();
                            let Some(live) = store.get_mut(bp.id) else { return };
                            let mut enabled = live.status != phpide_dbgp::BreakpointStatus::Disabled;
                            if crate::design::components::Checkbox::new(&mut enabled, "Enabled").show(ui).changed() {
                                live.status = if enabled {
                                    phpide_dbgp::BreakpointStatus::Pending
                                } else {
                                    phpide_dbgp::BreakpointStatus::Disabled
                                };
                                changed = true;
                            }
                            ui.add_space(4.0);
                            ui.label("Condition (PHP, stops when true)");
                            let mut condition = match &live.kind {
                                phpide_dbgp::BreakpointKind::Conditional(c) => c.clone(),
                                _ => String::new(),
                            };
                            let before = condition.clone();
                            crate::design::components::TextInput::new(&mut condition)
                                .hint("$user->id === 42")
                                .id_salt(("bp-condition", bp.id))
                                .width(340.0)
                                .show(ui);
                            if condition != before {
                                live.kind = if condition.trim().is_empty() {
                                    phpide_dbgp::BreakpointKind::Line
                                } else {
                                    phpide_dbgp::BreakpointKind::Conditional(condition)
                                };
                                if live.status != phpide_dbgp::BreakpointStatus::Disabled {
                                    live.status = phpide_dbgp::BreakpointStatus::Pending;
                                }
                                changed = true;
                            }
                            if let phpide_dbgp::BreakpointStatus::Invalid(message) = &live.status {
                                ui.colored_label(p.error, message);
                            }
                            ui.weak(format!("Hits in this session: {}", live.hit_count));
                            ui.add_space(6.0);
                            let (id, file, line) = (live.id, live.file.clone(), live.line);
                            let mut remove = false;
                            ui.horizontal(|ui| {
                                if crate::design::components::Button::new("Go to Source").show(ui).clicked() {
                                    goto = file.map(|f| (f, line));
                                }
                                if crate::design::components::Button::new("Remove").show(ui).clicked() {
                                    remove = true;
                                }
                            });
                            if remove {
                                store.remove(id);
                                changed = true;
                            }
                        });
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if crate::design::components::Button::primary("Done").show(ui).clicked() {
                            close = true;
                        }
                        if crate::design::components::Button::new("Remove All")
                            .enabled(!breakpoints.is_empty())
                            .show(ui)
                            .clicked()
                        {
                            app.rd.bp_store.lock().unwrap().breakpoints.clear();
                            changed = true;
                        }
                    });
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        close = true;
                    }
                });
        });
    if changed {
        app.rd.breakpoints_dirty = true;
    }
    if let Some((file, line)) = goto {
        app.ws
            .open_file_at(&file, line.saturating_sub(1) as usize, 0);
        close = true;
    }
    app.rd.breakpoints_dialog = if close { None } else { Some(selected) };
}
