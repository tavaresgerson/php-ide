//! Phase 8 gate: what survives a crash. Unsaved buffers are written to the
//! recovery folder while the IDE runs and offered back when the project
//! opens after an unclean exit; a clean exit empties it; a crash report
//! from the last run is shown once.

use std::path::Path;

use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use phpide_core::recovery::RecoveryStore;

use crate::ui_tests::{harness, project, step_until};
use crate::PhpIdeApp;

fn type_text(h: &mut Harness<'static, PhpIdeApp>, text: &str) {
    for c in text.chars() {
        h.event(egui::Event::Text(c.to_string()));
    }
    h.run_steps(2);
}

fn store(root: &Path) -> RecoveryStore {
    RecoveryStore::new(&crate::app_recovery::state_dir(), root)
}

/// Opens src/App.php, types at the end and waits for the autosave.
fn edit_and_autosave(root: &Path) -> Harness<'static, PhpIdeApp> {
    let file = root.join("src/App.php");
    let mut h = harness(root, vec![file]);
    h.run_steps(3);
    {
        let app = h.state_mut();
        let end = app.ws.active_buffer().unwrap().document.len_chars();
        app.editors.set_caret(&mut app.ws, end);
        app.editors.request_focus();
    }
    h.run_steps(2);
    type_text(&mut h, "// unsaved work");
    assert!(h.state().ws.active_buffer().unwrap().dirty);
    step_until(&mut h, "the autosave", |_| {
        store(root)
            .load()
            .iter()
            .any(|e| e.text.contains("// unsaved work"))
    });
    h
}

#[test]
fn unsaved_work_comes_back_after_an_unclean_exit() {
    let (_dir, root) = project();
    let h = edit_and_autosave(&root);
    // The process "dies": no on_exit, the window just goes away.
    drop(h);
    assert_eq!(
        std::fs::read_to_string(root.join("src/App.php")).unwrap(),
        "<?php\necho 'app';\n"
    );

    let mut h = harness(&root, Vec::new());
    h.run_steps(3);
    assert!(h.query_by_label("Recover Unsaved Changes").is_some());
    assert!(h.query_by_label_contains("src/App.php").is_some());
    h.get_by_label("Recover").click();
    h.run_steps(3);
    let buffer = h.state().ws.active_buffer().unwrap();
    assert!(buffer.dirty);
    assert!(buffer.document.text().ends_with("// unsaved work"));
    assert!(h.state().ws.message.contains("Recovered 1 unsaved file"));
    // Saving makes it clean; the next autosave empties the folder.
    h.state_mut()
        .ws
        .active_buffer_mut()
        .unwrap()
        .save()
        .unwrap();
    step_until(&mut h, "the recovery folder emptied", |_| {
        store(&root).load().is_empty()
    });
}

#[test]
fn discarding_and_a_clean_exit_empty_the_recovery_folder() {
    let (_dir, root) = project();
    let h = edit_and_autosave(&root);
    drop(h);
    let mut h = harness(&root, Vec::new());
    h.run_steps(3);
    h.get_by_label("Discard").click();
    h.run_steps(2);
    assert!(store(&root).load().is_empty());
    assert!(h.state().ws.buffers.iter().all(|b| !b.dirty));
    drop(h);

    // A clean exit after new unsaved work leaves nothing behind.
    let mut h = edit_and_autosave(&root);
    eframe::App::on_exit(h.state_mut());
    assert!(store(&root).load().is_empty());
}

#[test]
fn the_crash_report_of_the_last_run_is_shown_once() {
    let (_dir, root) = project();
    let crashes = crate::app_recovery::state_dir().join("crashes-gate");
    let report =
        phpide_core::diag::write_crash_report(&crashes, "PHP IDE 1.0.0 crashed\n").unwrap();
    let mut h = harness(&root, Vec::new());
    h.state_mut().recovery.crash_report = Some(report.clone());
    h.run_steps(3);
    assert!(h.query_by_label("PHP IDE Closed Unexpectedly").is_some());
    h.get_by_label("Close").click();
    h.run_steps(2);
    assert!(h.query_by_label("PHP IDE Closed Unexpectedly").is_none());
    assert!(!report.exists());
    assert!(phpide_core::diag::unseen_crash_reports(&crashes).is_empty());
}

#[test]
fn help_about_and_log_commands() {
    let (_dir, root) = project();
    let mut h = harness(&root, Vec::new());
    h.run_steps(2);
    let ctx = h.ctx.clone();
    h.state_mut()
        .dispatch_command(crate::commands::Command::About, &ctx);
    h.run_steps(2);
    assert!(h
        .query_by_label_contains(env!("CARGO_PKG_VERSION"))
        .is_some());
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(!h.state().recovery.about_open);
}

#[test]
fn the_status_bar_shows_and_converts_the_file_line_separator() {
    let (_dir, root) = project();
    let file = root.join("src/Win.php");
    std::fs::write(&file, "<?php\r\necho 1;\r\n").unwrap();
    let mut h = harness(&root, vec![file.clone()]);
    h.run_steps(3);
    h.get_by_label("CRLF").click();
    h.run_steps(2);
    h.get_by_label("LF - Unix and macOS (\\n)").click();
    h.run_steps(2);
    let buffer = h.state().ws.active_buffer().unwrap();
    assert_eq!(buffer.line_ending, phpide_core::LineEnding::Lf);
    assert!(buffer.dirty);
    h.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::S);
    h.run_steps(2);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "<?php\necho 1;\n");
    assert!(h.query_by_label("LF").is_some());
}

#[test]
fn the_window_icon_decodes() {
    let icon = crate::app_icon().expect("assets/app-icon.png");
    assert_eq!((icon.width, icon.height), (256, 256));
    assert_eq!(icon.rgba.len(), 256 * 256 * 4);
}
