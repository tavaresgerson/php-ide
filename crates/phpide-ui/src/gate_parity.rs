//! Phase 8 gate for the remaining PhpStorm parity items: Replace in Files
//! with a folder scope and a file mask (RF-NAV-21), Search Everywhere
//! filters (RF-SEL-05), Recent Locations (RF-SEL-08), colour scheme
//! management (RF-HL-12) and the configurable keymap (RF-UI-06).

use eframe::egui;
use egui_kittest::kittest::{NodeT, Queryable};

use crate::ui_tests::{harness, project, step_until};

#[test]
fn replace_in_files_in_a_folder_with_a_mask() {
    let (_dir, root) = project();
    std::fs::write(root.join("src/Other.php"), "<?php\n// app app\n").unwrap();
    std::fs::write(root.join("src/notes.txt"), "app\n").unwrap();
    std::fs::create_dir_all(root.join("tests")).unwrap();
    std::fs::write(root.join("tests/AppTest.php"), "<?php // app\n").unwrap();
    let app_php = root.join("src/App.php");
    let mut h = harness(&root, vec![app_php.clone()]);
    step_until(&mut h, "the index", |h| {
        h.state()
            .ws
            .files
            .iter()
            .any(|f| f.path.ends_with("tests/AppTest.php"))
    });

    h.state_mut().find_in_files(Some(root.join("src")), true);
    h.run_steps(2);
    assert!(h.query_by_label("Replace:").is_some());
    {
        let app = h.state_mut();
        app.search.query = "APP".into();
        app.search.replacement = "core".into();
        app.search.mask = "*.php".into();
    }
    h.run_steps(2);
    h.get_by_label("Search").click();
    step_until(&mut h, "the results", |h| {
        !h.state().search.busy() && !h.state().search.results.is_empty()
    });
    let files: Vec<String> = h
        .state()
        .search
        .results
        .iter()
        .map(|r| r.path.strip_prefix(&root).unwrap().display().to_string())
        .collect();
    assert!(
        files
            .iter()
            .all(|f| f.starts_with("src/") && f.ends_with(".php")),
        "{files:?}"
    );

    h.get_by_label("Replace All").click();
    h.step();
    h.run_steps(2);
    h.query_all_by_label("Replace")
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
        .expect("the confirmation's Replace button")
        .click();
    h.run_steps(2);
    // Closed file: written. Open file: an undoable edit, not saved yet.
    assert_eq!(
        std::fs::read_to_string(root.join("src/Other.php")).unwrap(),
        "<?php\n// core core\n"
    );
    let buffer = h.state().ws.active_buffer().unwrap();
    assert!(buffer.dirty);
    assert!(buffer.document.text().contains("echo 'core'"));
    assert_eq!(
        std::fs::read_to_string(&app_php).unwrap(),
        "<?php\necho 'app';\n"
    );
    // Out of scope or mask: untouched.
    assert_eq!(
        std::fs::read_to_string(root.join("src/notes.txt")).unwrap(),
        "app\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("tests/AppTest.php")).unwrap(),
        "<?php // app\n"
    );
    assert!(h
        .state()
        .ws
        .message
        .starts_with("Replaced 3 occurrences in 2 files"));
    // The Edit menu commands open the window.
    h.state_mut()
        .tw
        .set_visible(crate::toolwindows::ToolWindow::Find, false);
    h.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::F);
    h.run_steps(2);
    assert!(h
        .state()
        .tw
        .is_visible(crate::toolwindows::ToolWindow::Find));
    assert!(!h.state().search.replace_mode);
}

#[test]
fn recent_locations_list_visited_and_changed_places() {
    let (_dir, root) = project();
    let long: String = (1..=40).map(|i| format!("$line{i} = {i};\n")).collect();
    std::fs::write(root.join("src/Long.php"), format!("<?php\n{long}")).unwrap();
    let app_php = root.join("src/App.php");
    let long_php = root.join("src/Long.php");
    let mut h = harness(&root, vec![app_php.clone()]);
    h.run_steps(3);
    // Visit Long.php line 30, then come back to App.php.
    h.state_mut().ws.open_file_at(&long_php, 29, 0);
    h.run_steps(3);
    h.state_mut().ws.open_file_at(&app_php, 1, 0);
    h.run_steps(3);
    // An edit in App.php marks that place as changed.
    h.state_mut().ws.record_edit(&app_php, 1, 0);

    h.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::E);
    h.run_steps(3);
    assert!(h.query_by_label("Recent Locations").is_some());
    assert!(h.query_by_label("Long.php:30").is_some());
    assert!(h.query_by_label("App.php:2").is_some());
    assert!(h.query_by_label_contains("$line30 = 30;").is_some());

    // Again: only the changed places.
    h.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::E);
    h.run_steps(2);
    assert!(h.query_by_label("Recently Changed Locations").is_some());
    assert!(h.query_by_label("Long.php:30").is_none());
    h.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::E);
    h.run_steps(2);

    // Typing filters; Enter opens the place.
    for c in "line30".chars() {
        h.event(egui::Event::Text(c.to_string()));
    }
    h.run_steps(2);
    assert!(h.query_by_label("App.php:2").is_none());
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert!(!h.state().recent_locations.open);
    let active = h.state().ws.active_buffer().unwrap().path.clone();
    assert_eq!(active, Some(long_php));
}

#[test]
fn search_everywhere_includes_non_project_items_on_request() {
    let (_dir, root) = project();
    let mut h = harness(&root, Vec::new());
    step_until(&mut h, "the index", |h| !h.state().ws.files.is_empty());
    h.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::N);
    h.run_steps(2);
    for c in "Lib".chars() {
        h.event(egui::Event::Text(c.to_string()));
    }
    h.run_steps(2);
    assert!(
        h.query_by_label("Lib.php").is_none(),
        "vendor/ is not a project folder"
    );
    h.get_by_label("Include non-project items").click();
    step_until(&mut h, "the vendor scan", |h| {
        h.query_by_label("Lib.php").is_some()
    });
    // The type filter hides PHP files.
    h.get_by_label("All file types").click();
    h.run_steps(2);
    h.get_by_label("PHP").click();
    h.run_steps(2);
    assert!(h.query_by_label("Lib.php").is_none());
}

#[test]
fn color_schemes_are_duplicated_exported_imported_and_deleted() {
    use crate::color_scheme;
    let (dir, root) = project();
    let mut h = harness(&root, Vec::new());
    h.state_mut()
        .settings_dialog
        .open(crate::settings_dialog::SettingsPage::ColorScheme);
    h.run_steps(3);
    let theme = h.state().theme.name();

    h.get_by_label("Scheme actions").click();
    h.run_steps(2);
    h.get_by_label("Duplicate…").click();
    h.run_steps(2);
    h.state_mut()
        .settings_dialog
        .editor_pages
        .set_scheme_input("Team");
    h.get_by_label("Duplicate").click();
    h.run_steps(2);
    let value = h.state().settings.color_scheme.clone();
    assert_eq!(color_scheme::active_scheme(&value, theme), "Team");

    // Export, delete, then import it back.
    let file = dir.path().join("team.json");
    h.get_by_label("Scheme actions").click();
    h.run_steps(2);
    h.get_by_label("Export…").click();
    h.run_steps(2);
    h.state_mut()
        .settings_dialog
        .editor_pages
        .set_scheme_input(&file.display().to_string());
    h.get_by_label("Export").click();
    h.run_steps(2);
    let exported = std::fs::read_to_string(&file).unwrap();
    assert!(exported.contains("\"name\": \"Team\""), "{exported}");

    h.get_by_label("Scheme actions").click();
    h.run_steps(2);
    h.get_by_label("Delete").click();
    h.run_steps(2);
    let value = h.state().settings.color_scheme.clone();
    assert_eq!(color_scheme::active_scheme(&value, theme), theme);
    assert!(color_scheme::custom_schemes(&value).is_empty());

    h.get_by_label("Scheme actions").click();
    h.run_steps(2);
    h.get_by_label("Import…").click();
    h.run_steps(2);
    h.state_mut()
        .settings_dialog
        .editor_pages
        .set_scheme_input(&file.display().to_string());
    h.get_by_label("Import").click();
    h.run_steps(2);
    let value = h.state().settings.color_scheme.clone();
    assert_eq!(color_scheme::active_scheme(&value, theme), "Team");
    assert!(h
        .state()
        .ws
        .message
        .contains("Imported the color scheme Team"));
}

#[test]
fn shortcuts_are_assigned_from_find_action_and_the_keymap_page() {
    use crate::commands::{self, Command};
    let (_dir, root) = project();
    let files = vec![root.join("src/App.php"), root.join("composer.json")];
    let mut h = harness(&root, files);
    h.run_steps(3);
    // Find Action → Close Tab → Alt+Enter.
    h.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::A);
    h.run_steps(2);
    for c in "close tab".chars() {
        h.event(egui::Event::Text(c.to_string()));
    }
    h.run_steps(2);
    assert!(h.query_by_label("Alt+Enter assigns a shortcut").is_some());
    h.key_press_modifiers(egui::Modifiers::ALT, egui::Key::Enter);
    h.run_steps(3);
    assert!(h.state().settings_dialog.open);
    assert!(h
        .query_by_label_contains("New shortcut for File: Close Tab")
        .is_some());
    let combo = egui::Modifiers::CTRL | egui::Modifiers::ALT | egui::Modifiers::SHIFT;
    h.key_press_modifiers(combo, egui::Key::K);
    h.run_steps(2);
    assert_eq!(commands::shortcut_text(Command::CloseTab), "Ctrl+F4");
    assert_eq!(
        commands::shortcuts_of(Command::CloseTab).len(),
        2,
        "added next to Ctrl+F4"
    );
    assert_eq!(h.state().settings.shortcuts.len(), 1);

    // A taken shortcut asks first; "Remove from the Others" moves it.
    h.get_by_label("Add Keyboard Shortcut").click();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::S);
    h.run_steps(2);
    assert!(h
        .query_by_label_contains("is already assigned to File: Save")
        .is_some());
    h.get_by_label("Remove from the Others").click();
    h.run_steps(2);
    assert!(commands::shortcuts_of(Command::Save).is_empty());

    // The new shortcut works once the dialog is closed.
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(!h.state().settings_dialog.open);
    let before = h.state().ws.buffers.len();
    h.key_press_modifiers(combo, egui::Key::K);
    h.run_steps(2);
    assert_eq!(h.state().ws.buffers.len(), before - 1);

    // Reset All brings the defaults back.
    h.state_mut()
        .settings_dialog
        .open(crate::settings_dialog::SettingsPage::Keymap);
    h.run_steps(2);
    h.get_by_label("Reset All").click();
    h.run_steps(2);
    assert_eq!(commands::shortcut_text(Command::Save), "Ctrl+S");
    assert!(h.state().settings.shortcuts.is_empty());
}

/// Writes the Phase 8 screenshots to `target/snapshots` in the three
/// themes: `cargo test -p phpide-ui phase8_screenshots -- --ignored`.
#[test]
#[ignore]
fn phase8_screenshots() {
    use crate::design::{install, snapshot, Density, Theme, ThemeId};
    use crate::toolwindows::ToolWindow;
    use egui_kittest::Harness;
    use std::path::Path;

    let (_dir, root) = project();
    std::fs::create_dir_all(root.join("tests/Unit")).unwrap();
    std::fs::write(root.join("tests/Unit/AppTest.php"), "<?php\n// app test\n").unwrap();
    std::fs::write(root.join("composer.lock"), "{}\n").unwrap();
    let body: String = (1..=30)
        .map(|i| format!("    $total += $line{i}; // app\n"))
        .collect();
    std::fs::write(
        root.join("src/Invoice.php"),
        format!("<?php\n\nfinal class Invoice\n{{\n    public function total(): int\n    {{\n        $total = 0;\n{body}        return $total;\n    }}\n}}\n"),
    )
    .unwrap();
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/snapshots");
    std::fs::create_dir_all(&out).unwrap();
    let shoot = |h: &mut Harness<'static, crate::PhpIdeApp>, name: &str| {
        for (theme, label) in [
            (ThemeId::Dark, "dark"),
            (ThemeId::Light, "light"),
            (ThemeId::HighContrast, "high-contrast"),
        ] {
            let ctx = egui::Context::default();
            install(&ctx, Theme::new(theme, Density::Comfortable));
            h.state_mut().editors.clear_layouts();
            let image = snapshot::render(&ctx, egui::vec2(1280.0, 800.0), 12, |ui| {
                h.state_mut().frame_ui(ui)
            });
            image.write_png(&out.join(format!("phase8-{name}-{label}.png")));
        }
        h.state_mut().editors.clear_layouts();
    };

    // Project view: marks, nesting, size and date, indent guides; Replace
    // in Files below.
    let invoice = root.join("src/Invoice.php");
    let mut h = harness(&root, vec![invoice.clone()]);
    step_until(&mut h, "the index", |h| {
        h.state()
            .ws
            .files
            .iter()
            .any(|f| f.path.ends_with("AppTest.php"))
    });
    {
        let app = h.state_mut();
        app.settings.tree_file_details = true;
        app.settings.tree_indent_guides = true;
        app.project_marks
            .set(&root.join("src"), Some(crate::project_marks::Mark::Sources));
        app.project_marks
            .set(&root.join("tests"), Some(crate::project_marks::Mark::Tests));
        app.project_marks.set(
            &root.join("vendor"),
            Some(crate::project_marks::Mark::Excluded),
        );
        let ctx = h.ctx.clone();
        let app = h.state_mut();
        app.files
            .reveal(&app.jobs, &ctx, &root.join("tests/Unit/AppTest.php"));
        app.files
            .toggle(&app.jobs, &ctx, &root.join("composer.json"));
        app.find_in_files(None, true);
        app.search.query = "app".into();
        app.search.replacement = "core".into();
        app.search.mask = "*.php".into();
    }
    h.run_steps(2);
    h.get_by_label("Search").click();
    step_until(&mut h, "the results", |h| {
        !h.state().search.busy() && !h.state().search.results.is_empty()
    });
    h.run_steps(3);
    shoot(&mut h, "project-replace");

    // Recent Locations.
    h.state_mut().tw.set_visible(ToolWindow::Find, false);
    h.state_mut().ws.open_file_at(&invoice, 20, 4);
    h.run_steps(3);
    h.state_mut()
        .ws
        .open_file_at(&root.join("src/App.php"), 1, 0);
    h.run_steps(3);
    h.state_mut().ws.record_edit(&invoice, 8, 4);
    h.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::E);
    h.run_steps(3);
    shoot(&mut h, "recent-locations");
    h.key_press(egui::Key::Escape);
    h.run_steps(2);

    // Search Everywhere with the file filter and non-project items.
    h.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::N);
    h.run_steps(2);
    h.state_mut().palette.include_non_project = true;
    for c in "php".chars() {
        h.event(egui::Event::Text(c.to_string()));
    }
    step_until(&mut h, "the vendor scan", |h| {
        h.state().palette.libraries.is_some()
    });
    h.run_steps(3);
    shoot(&mut h, "search-everywhere");
    h.key_press(egui::Key::Escape);
    h.run_steps(2);

    // Keymap with a conflict.
    {
        let app = h.state_mut();
        app.settings_dialog
            .open(crate::settings_dialog::SettingsPage::Keymap);
        app.settings_dialog
            .keymap
            .assign(crate::commands::Command::CloseTab);
    }
    h.run_steps(3);
    h.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::S);
    h.run_steps(3);
    shoot(&mut h, "keymap");
    h.state_mut().settings_dialog.keymap.recording = false;
    h.state_mut()
        .settings_dialog
        .open(crate::settings_dialog::SettingsPage::ColorScheme);
    h.run_steps(3);
    shoot(&mut h, "color-scheme");
    h.state_mut().settings_dialog.open = false;
    h.run_steps(2);

    // Unsaved changes offered back, then the crash notice.
    h.state_mut().recovery.offered = vec![phpide_core::recovery::RecoveryEntry {
        path: Some(invoice.clone()),
        name: "Invoice.php".into(),
        text: "<?php\n".into(),
        disk_hash: None,
    }];
    h.run_steps(3);
    shoot(&mut h, "recovery");
    h.state_mut().recovery.offered.clear();
    let report = phpide_core::diag::write_crash_report(
        &crate::app_recovery::state_dir().join("crashes-shots"),
        "PHP IDE 0.1.0 crashed\n",
    )
    .unwrap();
    h.state_mut().recovery.crash_report = Some(report);
    h.run_steps(3);
    shoot(&mut h, "crash");
    drop(h);

    // Home with a project group.
    let other = root.join("vendor/acme");
    let recents = vec![root.display().to_string(), other.display().to_string()];
    let mut h = crate::ui_tests::home_harness(&root, recents);
    h.state_mut().settings.project_groups =
        vec![("Clients".into(), vec![other.display().to_string()])];
    h.run_steps(4);
    shoot(&mut h, "home-groups");
}
