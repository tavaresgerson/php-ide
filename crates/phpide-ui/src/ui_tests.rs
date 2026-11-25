//! Interaction tests: the real app driven through `egui_kittest` with
//! clicks found by their accessible labels.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use crate::PhpIdeApp;

pub(crate) fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("vendor/acme")).unwrap();
    std::fs::write(root.join("vendor/acme/Lib.php"), "<?php\n").unwrap();
    std::fs::write(root.join("src/App.php"), "<?php\necho 'app';\n").unwrap();
    std::fs::write(root.join("composer.json"), "{}").unwrap();
    (dir, root)
}

pub(crate) fn harness(root: &Path, files: Vec<PathBuf>) -> Harness<'static, PhpIdeApp> {
    let root = root.to_path_buf();
    Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        // Frames 1/60 s apart, so two clicks in a row are a double click.
        .with_step_dt(1.0 / 60.0)
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
pub(crate) fn step_until(
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

    harness.get_by_label("Main menu").click();
    harness.run_steps(2);
    harness.get_by_label("File").click();
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

fn tree_rows(h: &Harness<'static, PhpIdeApp>) -> Vec<String> {
    let app = h.state();
    let options = crate::project_tree::tree_options(app);
    app.files
        .visible_rows(&options)
        .iter()
        .map(|r| format!("{}{}", "  ".repeat(r.depth), r.entry.name))
        .collect()
}

fn focus_tree(h: &mut Harness<'static, PhpIdeApp>) {
    h.ctx
        .memory_mut(|m| m.request_focus(crate::project_tree::tree_id()));
    h.run_steps(2);
}

#[test]
fn project_tree_follows_the_active_file_and_navigates_by_keyboard() {
    let (_dir, root) = project();
    let file = root.join("src/App.php");
    let mut harness = harness(&root, vec![file.clone()]);
    step_until(&mut harness, "the tree to reveal the open file", |h| {
        tree_rows(h).contains(&"  App.php".to_string())
    });
    assert_eq!(harness.state().tree.selection, vec![file.clone()]);

    // Speed search: typing jumps to the first match; Enter opens it.
    focus_tree(&mut harness);
    for c in ["c", "o", "m", "p"] {
        harness.event(egui::Event::Text(c.into()));
        harness.run_steps(1);
    }
    assert_eq!(harness.state().tree.lead, Some(root.join("composer.json")));
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    let app = harness.state();
    assert_eq!(app.ws.buffers.len(), 2);
    assert_eq!(
        app.ws.active_buffer().unwrap().path.as_deref(),
        Some(root.join("composer.json").as_path())
    );
    assert!(
        !app.ws.active_buffer().unwrap().preview,
        "Enter opens a permanent tab"
    );
    assert!(app.tree.speed.is_none());

    // Arrows: up to `vendor`, Right expands it, Left collapses it.
    focus_tree(&mut harness);
    harness.state_mut().tree.select_only(root.join("vendor"));
    harness.key_press(egui::Key::ArrowRight);
    step_until(&mut harness, "vendor to expand", |h| {
        tree_rows(h).contains(&"  acme".to_string())
    });
    harness.key_press(egui::Key::ArrowLeft);
    harness.run_steps(2);
    assert!(!tree_rows(&harness).contains(&"  acme".to_string()));
    harness.key_press(egui::Key::ArrowUp);
    harness.run_steps(1);
    assert_eq!(
        harness.state().tree.lead,
        Some(file),
        "the row above `vendor` is the last child of the open `src`"
    );
}

#[test]
fn project_tree_creates_renames_and_deletes_files() {
    let (_dir, root) = project();
    let mut harness = harness(&root, Vec::new());
    step_until(&mut harness, "the tree", |h| !tree_rows(h).is_empty());
    focus_tree(&mut harness);
    harness.state_mut().tree.select_only(root.join("src"));
    harness.run_steps(1);

    // Alt+Insert → PHP Class → name → Enter.
    harness.key_press_modifiers(
        egui::Modifiers {
            alt: true,
            ..Default::default()
        },
        egui::Key::Insert,
    );
    harness.run_steps(2);
    harness.get_by_label("PHP Class").click();
    harness.run_steps(3);
    for c in "Invoice".chars() {
        harness.event(egui::Event::Text(c.to_string()));
    }
    harness.run_steps(1);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    let created = root.join("src/Invoice.php");
    let source = std::fs::read_to_string(&created).expect("class file created");
    assert!(source.contains("final class Invoice"), "{source}");
    assert_eq!(
        harness.state().ws.active_buffer().unwrap().path.as_deref(),
        Some(created.as_path()),
        "the new file opens"
    );

    // Shift+F6 renames; the open tab follows.
    focus_tree(&mut harness);
    harness.state_mut().tree.select_only(created.clone());
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F6);
    harness.run_steps(3);
    // The name without the extension is preselected.
    for c in "Bill".chars() {
        harness.event(egui::Event::Text(c.to_string()));
    }
    harness.run_steps(1);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    let renamed = root.join("src/Bill.php");

    assert!(renamed.exists() && !created.exists());
    assert_eq!(
        harness.state().ws.active_buffer().unwrap().path.as_deref(),
        Some(renamed.as_path())
    );

    // Delete asks first, then closes the clean tab.
    focus_tree(&mut harness);
    harness.state_mut().tree.select_only(renamed.clone());
    harness.key_press(egui::Key::Delete);
    harness.run_steps(3);
    assert!(renamed.exists(), "nothing deleted before confirming");
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    assert!(!renamed.exists());
    assert!(harness.state().ws.buffers.is_empty());
}

#[test]
fn tabs_open_beside_the_active_one_switch_with_alt_arrows_and_reopen() {
    let (_dir, root) = project();
    std::fs::write(root.join("src/B.php"), "<?php\n").unwrap();
    let mut harness = harness(
        &root,
        vec![root.join("src/App.php"), root.join("composer.json")],
    );
    harness.run_steps(2);
    let names = |h: &Harness<'static, PhpIdeApp>| -> Vec<String> {
        h.state().ws.buffers.iter().map(|b| b.name()).collect()
    };
    assert_eq!(names(&harness), ["App.php", "composer.json"]);

    let alt = egui::Modifiers {
        alt: true,
        ..Default::default()
    };
    harness.key_press_modifiers(alt, egui::Key::ArrowLeft);
    harness.run_steps(1);
    assert_eq!(
        harness.state().ws.active,
        0,
        "Alt+Left selects the previous tab"
    );

    harness
        .state_mut()
        .open_file_tab(&root.join("src/B.php"), false);
    harness.run_steps(1);
    assert_eq!(names(&harness), ["App.php", "B.php", "composer.json"]);

    // Ctrl+F4 closes; Ctrl+Shift+T brings it back.
    harness.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::F4);
    harness.run_steps(1);
    assert_eq!(names(&harness), ["App.php", "composer.json"]);
    harness.key_press_modifiers(
        egui::Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        },
        egui::Key::T,
    );
    harness.run_steps(1);
    assert!(names(&harness).contains(&"B.php".to_string()));

    // A preview tab becomes permanent on double click.
    harness
        .state_mut()
        .open_file_tab(&root.join("vendor/acme/Lib.php"), true);
    harness.run_steps(2);
    assert!(harness.state().ws.active_buffer().unwrap().preview);
    // The tab, not the Run widget, the tree row or the breadcrumb with the
    // same name: the topmost label below the window header.
    let pos = harness
        .get_all_by_label("Lib.php")
        .map(|n| n.rect())
        .filter(|r| r.top() >= 39.0)
        .min_by(|a, b| a.top().total_cmp(&b.top()))
        .unwrap()
        .center();
    harness.event(egui::Event::PointerMoved(pos));
    for pressed in [true, false, true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        harness.step();
    }
    assert!(!harness.state().ws.active_buffer().unwrap().preview);
}

pub(crate) fn home_harness(root: &Path, recents: Vec<String>) -> Harness<'static, PhpIdeApp> {
    let root = root.to_path_buf();
    Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_eframe(move |cc| {
            crate::design::install(&cc.egui_ctx, crate::design::Theme::default());
            let mut app = PhpIdeApp::new(root, true, Vec::new());
            app.settings.recent_projects = recents;
            app
        })
}

#[test]
fn home_filters_removes_and_opens_recent_projects() {
    let (_dir, root) = project();
    let other = root.join("vendor/acme");
    let recents = vec![root.display().to_string(), other.display().to_string()];
    let mut harness = home_harness(&root, recents);
    harness.run_steps(3);
    let root_name = root.file_name().unwrap().to_string_lossy().into_owned();
    assert!(harness
        .query_by_label(&format!("Open {root_name}"))
        .is_some());
    assert!(harness.query_by_label("Open acme").is_some());

    // The search keeps matching projects only.
    harness
        .get_by_role(egui::accesskit::Role::TextInput)
        .click();
    harness.run_steps(1);
    for c in "acme".chars() {
        harness.event(egui::Event::Text(c.to_string()));
    }
    harness.run_steps(2);
    assert!(harness
        .query_by_label(&format!("Open {root_name}"))
        .is_none());
    assert!(harness.query_by_label("Open acme").is_some());

    // Remove from the list: the folder stays on disk.
    harness
        .get_by_label("Remove acme from Recent Projects")
        .click();
    harness.run_steps(2);
    assert_eq!(harness.state().settings.recent_projects.len(), 1);
    assert!(other.exists());

    harness.get_by_label("Clear").click();
    harness.run_steps(2);
    harness.get_by_label(&format!("Open {root_name}")).click();
    harness.run_steps(3);
    let app = harness.state();
    assert!(app.project_open && !app.welcome_open);
    assert_eq!(app.ws.root, root);
}

#[test]
fn opening_a_second_project_asks_where() {
    let (_dir, root) = project();
    let other = root.join("vendor/acme");
    let mut harness = harness(&root, Vec::new());
    harness.run_steps(2);
    let ctx = harness.ctx.clone();
    harness.state_mut().open_project_at(other.clone(), &ctx);
    harness.run_steps(2);
    assert!(harness.query_by_label("New Window").is_some());
    harness.get_by_label("This Window").click();
    harness.run_steps(3);
    assert_eq!(harness.state().ws.root, other);
    assert!(harness.state().open_prompt.is_none());

    // Close Project goes back to Home.
    harness
        .state_mut()
        .dispatch_command(crate::commands::Command::CloseProject, &ctx);
    harness.run_steps(2);
    let app = harness.state();
    assert!(app.welcome_open && !app.project_open);
}

#[test]
fn attached_project_joins_the_tree_and_go_to_file() {
    let (_dir, root) = project();
    let other = tempfile::tempdir().unwrap();
    std::fs::write(other.path().join("Helper.php"), "<?php\n").unwrap();
    let mut harness = harness(&root, Vec::new());
    harness.run_steps(2);
    let ctx = harness.ctx.clone();
    harness
        .state_mut()
        .open_project_at(other.path().to_path_buf(), &ctx);
    harness.run_steps(2);
    harness.get_by_label("Attach").click();
    let other_name = other
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    step_until(&mut harness, "the attached root in the tree", |h| {
        tree_rows(h).contains(&other_name) && tree_rows(h).contains(&"  Helper.php".to_string())
    });
    assert_eq!(
        harness.state().ws.root,
        root,
        "the window keeps its project"
    );
    step_until(
        &mut harness,
        "the index to include the attached files",
        |h| {
            h.state()
                .ws
                .files
                .iter()
                .any(|f| f.path.ends_with("Helper.php"))
        },
    );
}

#[test]
fn search_everywhere_tabs_recent_files_and_settings() {
    let (_dir, root) = project();
    let file = root.join("src/App.php");
    let mut harness = harness(&root, vec![file]);
    harness.run_steps(2);
    // Double Shift: two lone taps.
    for _ in 0..2 {
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::SHIFT));
        harness.run_steps(1);
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        harness.run_steps(1);
    }
    assert!(harness.state().palette.open);
    assert!(
        harness.query_all_by_label("App.php").count() >= 3,
        "recent files show before typing (tab, tree, palette)"
    );
    harness.key_press(egui::Key::Tab);
    harness.run_steps(1);
    assert_eq!(
        harness.state().palette.mode,
        crate::palette::PaletteMode::Classes
    );
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Tab);
    harness.run_steps(1);
    assert_eq!(
        harness.state().palette.mode,
        crate::palette::PaletteMode::All
    );
    for c in ">color scheme".chars() {
        harness.event(egui::Event::Text(c.to_string()));
    }
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    let app = harness.state();
    assert!(!app.palette.open);
    assert!(app.settings_dialog.open);
    assert_eq!(
        app.settings_dialog.page,
        crate::settings_dialog::SettingsPage::ColorScheme
    );
}

#[test]
fn select_in_offers_targets_and_reveals_the_file() {
    let (_dir, root) = project();
    let file = root.join("src/App.php");
    let mut harness = harness(&root, vec![file.clone()]);
    harness.run_steps(2);
    harness.state_mut().settings.tree_autoscroll_from_source = false;
    harness.state_mut().tree.selection.clear();
    harness.key_press_modifiers(
        egui::Modifiers {
            alt: true,
            ..Default::default()
        },
        egui::Key::F1,
    );
    harness.run_steps(2);
    assert!(harness.query_by_label("2. Files (file manager)").is_some());
    harness.key_press(egui::Key::Num1);
    harness.run_steps(3);
    assert!(harness.state().select_in.is_none());
    assert_eq!(harness.state().tree.selection, vec![file]);
}

/// Phase 4 gate: open project → edit → save → close, keyboard only.
#[test]
fn keyboard_only_open_edit_save_close() {
    let (_dir, root) = project();
    let mut harness = home_harness(&root, vec![root.display().to_string()]);
    harness.run_steps(3);
    // Home: the search field has the keyboard; Enter opens the first project.
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    assert!(
        harness.state().project_open,
        "Enter opens the selected project"
    );
    step_until(&mut harness, "the file index", |h| {
        !h.state().ws.files.is_empty()
    });

    // Go to File, type, Enter.
    harness.key_press_modifiers(
        egui::Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        },
        egui::Key::N,
    );
    harness.run_steps(2);
    for c in "App".chars() {
        harness.event(egui::Event::Text(c.to_string()));
    }
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    let file = root.join("src/App.php");
    assert_eq!(
        harness.state().ws.active_buffer().unwrap().path.as_deref(),
        Some(file.as_path())
    );

    // The editor has the keyboard: type and save.
    for c in "// ok\n".chars() {
        if c == '\n' {
            harness.key_press(egui::Key::Enter);
        } else {
            harness.event(egui::Event::Text(c.to_string()));
        }
        harness.run_steps(1);
    }
    harness.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::S);
    harness.run_steps(2);
    let saved = std::fs::read_to_string(&file).unwrap();
    assert!(saved.starts_with("// ok\n"), "{saved:?}");

    // Esc from a tool window gives the keyboard back to the editor.
    harness.key_press_modifiers(
        egui::Modifiers {
            alt: true,
            ..Default::default()
        },
        egui::Key::Num6,
    );
    harness.run_steps(2);
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(harness.state().editors.editor_focused);

    // Find Action → Close Project.
    harness.key_press_modifiers(
        egui::Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        },
        egui::Key::A,
    );
    harness.run_steps(2);
    for c in "close project".chars() {
        harness.event(egui::Event::Text(c.to_string()));
    }
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    let app = harness.state();
    assert!(app.welcome_open && !app.project_open);
}

/// RF-UI-05: failures pop up as a toast and stay in the Event Log.
#[test]
fn failures_become_toasts_and_land_in_the_event_log() {
    let (_dir, root) = project();
    let mut harness = harness(&root, Vec::new());
    harness.run_steps(2);
    harness.state_mut().ws.message = "Could not rename: permission denied".to_string();
    harness.run_steps(2);
    assert_eq!(harness.state().toasts.len(), 1);
    assert_eq!(harness.state().event_log.unread(), 1);
    assert!(harness.query_by_label("Notifications (1 new)").is_some());

    harness.get_by_label("Notifications (1 new)").click();
    harness.run_steps(2);
    assert!(harness
        .state()
        .tw
        .is_visible(crate::toolwindows::ToolWindow::Notifications));
    assert_eq!(harness.state().event_log.unread(), 0);
    assert!(!harness
        .query_all_by_label("Could not rename: permission denied")
        .collect::<Vec<_>>()
        .is_empty());
    harness.get_by_label("Clear All").click();
    harness.run_steps(2);
    assert!(harness.state().event_log.is_empty());
}

/// Phase 6: Alt+F12 opens a real shell; typing runs a command.
#[test]
fn terminal_runs_the_shell_on_a_pty() {
    let (_dir, root) = project();
    let mut harness = harness(&root, Vec::new());
    harness.run_steps(2);
    harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::F12);
    harness.run_steps(3);
    assert!(
        harness.state().terminal.focused,
        "the terminal has the keyboard"
    );
    step_until(&mut harness, "the shell", |h| {
        h.state()
            .terminal
            .sessions
            .first()
            .is_some_and(|s| !s.screen_text().is_empty())
    });
    for c in "echo phpide-$((40 + 2))".chars() {
        harness.event(egui::Event::Text(c.to_string()));
    }
    harness.key_press(egui::Key::Enter);
    step_until(&mut harness, "the command output", |h| {
        h.state().terminal.sessions[0]
            .screen_text()
            .lines()
            .any(|l| l.trim() == "phpide-42")
    });
    // Tab and arrows went to the shell, not to the IDE.
    harness.key_press(egui::Key::Tab);
    harness.run_steps(2);
    assert!(harness.state().terminal.focused);
    // Esc from Alt+F12 again hides it and gives the keyboard back.
    harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::F12);
    harness.run_steps(3);
    assert!(!harness.state().terminal.focused);
}

/// The Structure tool window lists the file's declarations from the syntax
/// tree with no language server, and a click on one jumps to its line.
#[test]
fn structure_lists_declarations_without_a_language_server() {
    let (_dir, root) = project();
    let file = root.join("src/Order.php");
    std::fs::write(
        &file,
        "<?php\nclass Order\n{\n    private int $total = 0;\n\n    public function placeOrderNow(): void\n    {\n    }\n}\n",
    )
    .unwrap();
    let mut harness = harness(&root, vec![file]);
    harness
        .state_mut()
        .tw
        .set_visible(crate::toolwindows::ToolWindow::Structure, true);
    step_until(&mut harness, "the structure", |h| {
        h.state().ws.outline.len() == 3
    });
    let listed: Vec<(String, String, usize)> = harness
        .state()
        .ws
        .outline
        .iter()
        .map(|e| (e.kind.clone(), e.name.clone(), e.depth))
        .collect();
    assert_eq!(
        listed,
        [
            ("class".to_string(), "Order".to_string(), 0),
            ("property".to_string(), "$total".to_string(), 1),
            ("method".to_string(), "placeOrderNow".to_string(), 1),
        ]
    );
    harness.run_steps(2);
    harness.get_by_label("placeOrderNow").click();
    harness.run_steps(3);
    let app = harness.state();
    let buffer = app.ws.active_buffer().unwrap();
    assert_eq!(
        buffer.document.rope().char_to_line(app.cursor_char),
        5,
        "the caret is on the method"
    );
}

/// View Breakpoints keeps its size inside the window: it used to grow a bit
/// every frame until its top and bottom ran past the window edges.
#[test]
fn breakpoints_dialog_fits_the_window_and_keeps_its_size() {
    let (_dir, root) = project();
    let mut harness = harness(&root, Vec::new());
    harness.run_steps(2);
    harness.state_mut().rd.breakpoints_dialog = Some(None);
    harness.run_steps(5);
    let dialog = |h: &Harness<'static, PhpIdeApp>| {
        h.ctx
            .memory(|m| m.area_rect(egui::Id::new("view-breakpoints")))
            .expect("the dialog is shown")
    };
    let first = dialog(&harness);
    harness.run_steps(60);
    let later = dialog(&harness);
    assert_eq!(first.size(), later.size(), "the dialog keeps its size");
    let window = harness.ctx.content_rect();
    assert!(window.contains_rect(later), "{later:?} fits in {window:?}");
}
