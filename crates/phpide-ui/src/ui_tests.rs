//! Interaction tests: the real app driven through `egui_kittest` with
//! clicks found by their accessible labels.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use crate::PhpIdeApp;

fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("vendor/acme")).unwrap();
    std::fs::write(root.join("vendor/acme/Lib.php"), "<?php\n").unwrap();
    std::fs::write(root.join("src/App.php"), "<?php\necho 'app';\n").unwrap();
    std::fs::write(root.join("composer.json"), "{}").unwrap();
    (dir, root)
}

fn harness(root: &Path, files: Vec<PathBuf>) -> Harness<'static, PhpIdeApp> {
    let root = root.to_path_buf();
    Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_eframe(move |cc| {
            crate::design::install(&cc.egui_ctx, crate::design::Theme::default());
            let mut app = PhpIdeApp::new(root, false, files);
            // No language server in these tests: they exercise the shell.
            if let Some(lsp) = app.lsp.take() {
                lsp.stop();
            }
            app.lsp_init = None;
            app
        })
}

/// Steps the UI until `done` holds; background jobs finish in real time.
fn step_until(
    harness: &mut Harness<'static, PhpIdeApp>,
    what: &str,
    mut done: impl FnMut(&Harness<'static, PhpIdeApp>) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        harness.step();
        if done(harness) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn project_tree_expands_folders_and_opens_files_on_click() {
    let (_dir, root) = project();
    let mut harness = harness(&root, Vec::new());
    step_until(&mut harness, "root folder listing", |h| {
        h.query_by_label("vendor").is_some()
    });
    assert!(
        harness.query_by_label("acme").is_none(),
        "vendor starts collapsed"
    );

    harness.get_by_label("vendor").click();
    step_until(&mut harness, "vendor listing", |h| {
        h.query_by_label("acme").is_some()
    });

    harness.get_by_label("src").click();
    step_until(&mut harness, "src listing", |h| {
        h.query_by_label("App.php").is_some()
    });
    harness.get_by_label("App.php").click();
    step_until(&mut harness, "file opened", |h| {
        h.state()
            .ws
            .active_buffer()
            .and_then(|b| b.path.clone())
            .is_some_and(|p| p.ends_with("src/App.php"))
    });
}

#[test]
fn closing_a_modified_tab_from_the_menu_asks_first() {
    let (_dir, root) = project();
    let file = root.join("src/App.php");
    let mut harness = harness(&root, vec![file.clone()]);
    harness.run_steps(2);
    harness
        .state_mut()
        .ws
        .active_buffer_mut()
        .unwrap()
        .insert(0, "// unsaved\n");

    harness.get_by_label("Menu").click();
    harness.run_steps(2);
    // Submenus carry an arrow in their accessible label.
    harness.get_by_label("File ⏵").click();
    harness.run_steps(2);
    harness.get_by_label_contains("Close Tab").click();
    harness.run_steps(2);

    assert!(
        harness.state().pending_close.is_some(),
        "a confirmation must be pending"
    );
    harness.get_by_label("Cancel").click();
    harness.run_steps(2);
    let app = harness.state();
    assert!(app.pending_close.is_none());
    let buffer = app.ws.active_buffer().expect("tab still open");
    assert!(buffer.dirty);
    assert!(buffer.document.text().starts_with("// unsaved"));
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "<?php\necho 'app';\n",
        "nothing written to disk"
    );
}

#[test]
fn frozen_and_fake_status_items_are_not_shown() {
    let (_dir, root) = project();
    let mut harness = harness(&root, Vec::new());
    harness.run_steps(3);
    let mut hidden = vec!["Processes", "Column"];
    if !crate::scope::EXPERIMENTAL {
        // Deployment is frozen, not fake: it only shows in experimental builds.
        hidden.push("Deploy");
    }
    for label in hidden {
        assert!(
            harness.query_by_label_contains(label).is_none(),
            "{label} must not be in the default build"
        );
    }
    assert!(harness.query_by_label("Ready").is_some());
}

#[test]
fn starting_without_a_project_shows_welcome_and_starts_no_project_work() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_eframe(move |cc| {
            crate::design::install(&cc.egui_ctx, crate::design::Theme::default());
            PhpIdeApp::new(root, true, Vec::new())
        });
    harness.run_steps(3);
    assert!(harness.query_by_label("Projects").is_some());
    let app = harness.state();
    assert!(
        app.lsp.is_none(),
        "no language server before a project is chosen"
    );
    assert!(
        app.watcher.is_none(),
        "no file watcher before a project is chosen"
    );
    assert!(
        app.ws.files.is_empty(),
        "no scan before a project is chosen"
    );
}

#[test]
fn shortcuts_with_extra_modifiers_run_their_own_command() {
    let (_dir, root) = project();
    let file = root.join("src/App.php");
    let mut harness = harness(&root, vec![file]);
    harness.run_steps(2);
    {
        let buffer = harness.state_mut().ws.active_buffer_mut().unwrap();
        buffer.insert(0, "// one\n");
    }
    let modifiers = |ctrl, shift| egui::Modifiers {
        ctrl,
        shift,
        ..Default::default()
    };
    harness.key_press_modifiers(modifiers(true, false), egui::Key::Z);
    harness.run_steps(2);
    let text =
        |h: &Harness<'static, PhpIdeApp>| h.state().ws.active_buffer().unwrap().document.text();
    assert!(!text(&harness).starts_with("// one"), "Ctrl+Z undoes");
    harness.key_press_modifiers(modifiers(true, true), egui::Key::Z);
    harness.run_steps(2);
    assert!(
        text(&harness).starts_with("// one"),
        "Ctrl+Shift+Z redoes (it used to run Undo)"
    );

    harness.key_press_modifiers(modifiers(true, true), egui::Key::N);
    harness.run_steps(2);
    let app = harness.state();
    assert!(app.palette.open, "Ctrl+Shift+N opens Go to File");
    assert_eq!(app.ws.buffers.len(), 1, "and does not create a new file");
}

fn active_text(h: &Harness<'static, PhpIdeApp>) -> String {
    h.state().ws.active_buffer().unwrap().document.text()
}

#[test]
fn opening_a_file_focuses_the_editor_and_shortcuts_edit_it() {
    let (_dir, root) = project();
    let file = root.join("src/App.php");
    let mut harness = harness(&root, vec![file]);
    harness.run_steps(3);
    assert_eq!(
        harness.query_all_by_label("Editor").count(),
        1,
        "one editor pane"
    );

    // The caret starts at the top: typing lands there without a click.
    for c in ["/", "/", " ", "x"] {
        harness.event(egui::Event::Text(c.into()));
        harness.run_steps(1);
    }
    assert_eq!(active_text(&harness), "// x<?php\necho 'app';\n");

    let ctrl = egui::Modifiers {
        ctrl: true,
        ..Default::default()
    };
    // Ctrl+D duplicates the line, Ctrl+/ toggles its comment.
    harness.key_press_modifiers(ctrl, egui::Key::D);
    harness.run_steps(2);
    assert_eq!(active_text(&harness), "// x<?php\n// x<?php\necho 'app';\n");
    harness.key_press_modifiers(ctrl, egui::Key::Slash);
    harness.run_steps(2);
    assert_eq!(active_text(&harness), "// x<?php\nx<?php\necho 'app';\n");

    // Ctrl+Z walks back through the three groups.
    for _ in 0..3 {
        harness.key_press_modifiers(ctrl, egui::Key::Z);
        harness.run_steps(2);
    }
    assert_eq!(active_text(&harness), "<?php\necho 'app';\n");
    assert!(!harness.state().ws.active_buffer().unwrap().dirty);
}

#[test]
fn split_shows_two_panes_of_one_buffer() {
    let (_dir, root) = project();
    let file = root.join("src/App.php");
    let mut harness = harness(&root, vec![file]);
    harness.run_steps(3);
    let ctx = harness.ctx.clone();
    harness
        .state_mut()
        .dispatch_command(crate::commands::Command::SplitRight, &ctx);
    harness.run_steps(3);
    assert_eq!(harness.query_all_by_label("Editor").count(), 2);

    // Typing in one pane edits the shared buffer.
    harness.event(egui::Event::Text("A".into()));
    harness.run_steps(2);
    assert_eq!(active_text(&harness), "A<?php\necho 'app';\n");
    assert_eq!(harness.state().ws.buffers.len(), 1, "no second buffer");

    harness
        .state_mut()
        .dispatch_command(crate::commands::Command::Unsplit, &ctx);
    harness.run_steps(2);
    assert_eq!(harness.query_all_by_label("Editor").count(), 1);
}
