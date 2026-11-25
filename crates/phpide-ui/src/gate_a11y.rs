//! Phase 8 gate: accessibility. Every interactive control on the main
//! screens and dialogs has a name a screen reader can announce (AccessKit
//! is what egui exposes to AT-SPI on Linux), and the keyboard alone moves
//! through dialogs and runs the main flows.

use eframe::egui;
use egui::accesskit::Role;
use egui_kittest::kittest::NodeT;
use egui_kittest::Harness;

use crate::commands::Command;
use crate::toolwindows::ToolWindow;
use crate::ui_tests::{harness, project};
use crate::PhpIdeApp;

/// Roles a user acts on.
fn interactive(role: Role) -> bool {
    matches!(
        role,
        Role::Button
            | Role::CheckBox
            | Role::RadioButton
            | Role::Switch
            | Role::TextInput
            | Role::MultilineTextInput
            | Role::SearchInput
            | Role::ComboBox
            | Role::Link
            | Role::Tab
            | Role::MenuItem
            | Role::Slider
            | Role::SpinButton
            | Role::ListBoxOption
            | Role::TreeItem
    )
}

/// Interactive nodes without a name: `"Role at (x, y)"`.
fn unnamed(h: &Harness<'static, PhpIdeApp>) -> Vec<String> {
    let mut out = Vec::new();
    for node in h.root().children_recursive() {
        let ak = node.accesskit_node();
        if !interactive(ak.role()) || ak.is_hidden() {
            continue;
        }
        let name = ak.label().unwrap_or_default();
        let value = ak.value().unwrap_or_default();
        let text_input = matches!(
            ak.role(),
            Role::TextInput | Role::MultilineTextInput | Role::SearchInput
        );
        // A text field may be named by its placeholder or its content.
        let named = !name.trim().is_empty()
            || (text_input
                && (!value.trim().is_empty()
                    || ak.placeholder().is_some_and(|p| !p.trim().is_empty())));
        if !named {
            let rect = node.rect();
            out.push(format!(
                "{:?} at ({:.0}, {:.0}) {:.0}×{:.0}",
                ak.role(),
                rect.min.x,
                rect.min.y,
                rect.width(),
                rect.height()
            ));
        }
    }
    out
}

fn audit(h: &mut Harness<'static, PhpIdeApp>, screen: &str, findings: &mut Vec<String>) {
    h.run_steps(4);
    for item in unnamed(h) {
        findings.push(format!("{screen}: {item}"));
    }
}

#[test]
fn every_control_has_a_name_on_the_main_screens() {
    let (_dir, root) = project();
    let mut findings = Vec::new();

    // Home (no project).
    let mut home = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_eframe(|cc| {
            crate::design::install(&cc.egui_ctx, crate::design::Theme::default());
            PhpIdeApp::new(std::path::PathBuf::new(), true, Vec::new())
        });
    audit(&mut home, "Home", &mut findings);
    drop(home);

    let mut h = harness(&root, vec![root.join("src/App.php")]);
    audit(&mut h, "Editor", &mut findings);
    let ctx = h.ctx.clone();
    for tw in [
        ToolWindow::Find,
        ToolWindow::Run,
        ToolWindow::Debug,
        ToolWindow::Problems,
        ToolWindow::Structure,
        ToolWindow::Commit,
        ToolWindow::VersionControl,
        ToolWindow::Terminal,
        ToolWindow::Bookmarks,
        ToolWindow::Notifications,
    ] {
        h.state_mut().tw.set_visible(tw, true);
        audit(&mut h, tw.title(), &mut findings);
        h.state_mut().tw.set_visible(tw, false);
    }
    // Dialogs and popups.
    for (command, name) in [
        (Command::OpenSettings, "Settings"),
        (Command::FindAction, "Find Action"),
        (Command::GotoFile, "Go to File"),
        (Command::EditConfigurations, "Run/Debug Configurations"),
        (Command::GitNewBranch, "New Branch"),
        (Command::About, "About"),
    ] {
        h.state_mut().dispatch_command(command, &ctx);
        audit(&mut h, name, &mut findings);
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
        // Dialogs without Escape handling close here.
        h.state_mut().git_dialog = None;
        h.state_mut().recovery.about_open = false;
        h.state_mut().run_dialog = None;
        h.state_mut().settings_dialog.open = false;
    }
    // Every page of Settings.
    h.state_mut().dispatch_command(Command::OpenSettings, &ctx);
    for page in crate::settings_dialog::all_pages() {
        h.state_mut().settings_dialog.page = page;
        audit(&mut h, &format!("Settings › {page:?}"), &mut findings);
    }
    assert!(
        findings.is_empty(),
        "unnamed controls:\n{}",
        findings.join("\n")
    );
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A project in a Git repository with one modified and one new file.
fn git_project() -> Option<(tempfile::TempDir, std::path::PathBuf)> {
    if std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_err()
    {
        return None;
    }
    let (dir, root) = project();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.name", "Ana"]);
    git(&root, &["config", "user.email", "ana@example.com"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "Initial import"]);
    std::fs::write(root.join("src/App.php"), "<?php\necho 'changed';\n").unwrap();
    Some((dir, root))
}

fn settle(h: &mut Harness<'static, PhpIdeApp>, what: &str, done: impl Fn(&PhpIdeApp) -> bool) {
    crate::ui_tests::step_until(h, what, |h| done(h.state()));
}

#[test]
fn git_views_name_every_control() {
    let Some((_dir, root)) = git_project() else {
        return;
    };
    let mut findings = Vec::new();
    let mut h = harness(&root, Vec::new());
    settle(&mut h, "the repository", |app| {
        app.vcs.snapshot().is_some_and(|s| !s.changes.is_empty())
    });
    h.state_mut().tw.set_visible(ToolWindow::Commit, true);
    audit(&mut h, "Commit", &mut findings);
    let ctx = h.ctx.clone();
    h.state_mut().open_diff(
        &ctx,
        crate::services::vcs::DiffRequest {
            path: "src/App.php".into(),
            old_path: None,
            from: crate::services::vcs::Side::Index,
            to: crate::services::vcs::Side::WorkTree,
        },
        "App.php".into(),
    );
    settle(&mut h, "the diff", |app| {
        app.diffs.active_tab().is_some_and(|t| t.data.is_some())
    });
    audit(&mut h, "Diff", &mut findings);
    h.state_mut()
        .tw
        .set_visible(ToolWindow::VersionControl, true);
    settle(&mut h, "the log", |app| !app.vcs_log.entries().is_empty());
    audit(&mut h, "Log", &mut findings);
    h.state_mut().dispatch_command(Command::GitBranches, &ctx);
    audit(&mut h, "Branches", &mut findings);
    assert!(
        findings.is_empty(),
        "unnamed controls:\n{}",
        findings.join("\n")
    );
}

/// Keyboard only: Ctrl+K, Tab past the buttons to the changed file, Space
/// stages it, back to the message (Ctrl+K), type it, Ctrl+Enter commits.
#[test]
fn commit_with_the_keyboard_only() {
    let Some((_dir, root)) = git_project() else {
        return;
    };
    let mut h = harness(&root, Vec::new());
    h.state_mut().tw.set_visible(ToolWindow::Project, false);
    settle(&mut h, "the repository", |app| {
        app.vcs.snapshot().is_some_and(|s| !s.changes.is_empty())
    });
    h.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::K);
    h.run_steps(3);
    // From the message, past the buttons, to the file row.
    let mut reached = false;
    for _ in 0..12 {
        h.key_press(egui::Key::Tab);
        h.run_steps(2);
        if !h.state().commit_ui.selected.is_empty() {
            reached = true;
            break;
        }
    }
    assert!(reached, "Tab never reached the changed file");
    h.key_press(egui::Key::Space);
    settle(&mut h, "the file staged", |app| {
        !app.vcs.busy()
            && app
                .vcs
                .snapshot()
                .is_some_and(|s| s.changes.iter().any(|c| c.staged.is_some()))
    });
    // Back to the message.
    h.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::K);
    h.run_steps(3);
    for c in "Change the greeting".chars() {
        h.event(egui::Event::Text(c.to_string()));
    }
    h.run_steps(1);
    h.key_press_modifiers(
        egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
        egui::Key::Enter,
    );
    settle(&mut h, "the commit", |app| {
        !app.vcs.busy() && app.vcs.console.last().is_some_and(|e| e.exit.is_some())
    });
    let log = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["log", "-1", "--format=%s"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&log.stdout).trim(),
        "Change the greeting"
    );
}

/// Tab walks a dialog's controls and stays inside it.
#[test]
fn tab_moves_through_the_settings_dialog() {
    let (_dir, root) = project();
    let mut h = harness(&root, Vec::new());
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::ALT, egui::Key::S);
    h.run_steps(3);
    assert!(h.state().settings_dialog.open);
    let mut seen = std::collections::HashSet::new();
    for _ in 0..15 {
        h.key_press(egui::Key::Tab);
        h.run_steps(2);
        if let Some(id) = h.ctx.memory(|m| m.focused()) {
            seen.insert(id);
        }
        assert!(h.state().settings_dialog.open, "Tab closed the dialog");
    }
    assert!(seen.len() >= 5, "Tab reached only {} controls", seen.len());
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(!h.state().settings_dialog.open);
}
