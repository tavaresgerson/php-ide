//! Phase 5 gate: PHP intelligence through the UI, against a real LSP
//! conversation.
//!
//! `fixture_server_*` drives the app with the built-in fixture server, so it
//! runs everywhere. `real_servers_on_laravel_fixture` repeats the core of it
//! with intelephense and phpactor when they are installed (PATH or
//! `PHPIDE_LSP`), on `tests/fixtures/laravel-mini`; without them it is
//! skipped with a message.

use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use crate::ui_tests::{harness, step_until};
use crate::PhpIdeApp;

const GREETER: &str = "<?php\n\nnamespace App;\n\nclass Greeter\n{\n    /** Says hello to someone. */\n    public function greet(string $name, int $times): string\n    {\n        return str_repeat(\"Hi $name \", $times);\n    }\n\n    public function wave(): void\n    {\n    }\n}\n";
const HELPERS: &str = "<?php\n\n/** Adds two numbers. */\nfunction add(int $a, int $b): int\n{\n    return $a + $b;\n}\n";
const APP: &str =
    "<?php\n\nuse App\\Greeter;\n\n$greeter = new Greeter();\necho add(1, 2);\nmissing_call();\n";

fn fixture_project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/Greeter.php"), GREETER).unwrap();
    std::fs::write(root.join("src/helpers.php"), HELPERS).unwrap();
    std::fs::write(root.join("src/app.php"), APP).unwrap();
    std::fs::write(root.join("composer.json"), "{}").unwrap();
    (dir, root)
}

fn text(h: &Harness<'static, PhpIdeApp>) -> String {
    h.state().ws.active_buffer().unwrap().document.text()
}

/// Puts the caret on the first occurrence of `needle` (plus `offset`).
fn caret_at(h: &mut Harness<'static, PhpIdeApp>, needle: &str, offset: usize) -> usize {
    let content = text(h);
    let byte = content
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} in {content}"));
    let pos = content[..byte].chars().count() + offset;
    let app = h.state_mut();
    app.editors.set_caret(&mut app.ws, pos);
    app.cursor_char = pos;
    app.editors.request_focus();
    h.run_steps(2);
    pos
}

fn type_text(h: &mut Harness<'static, PhpIdeApp>, typed: &str) {
    for c in typed.chars() {
        h.event(egui::Event::Text(c.to_string()));
        h.run_steps(1);
    }
}

fn keys(h: &mut Harness<'static, PhpIdeApp>, modifiers: egui::Modifiers, key: egui::Key) {
    h.key_press_modifiers(modifiers, key);
    h.run_steps(2);
}

fn running(h: &Harness<'static, PhpIdeApp>) -> bool {
    h.state()
        .lsp
        .as_ref()
        .is_some_and(|l| matches!(l.status(), phpide_lsp::LspStatus::Running(_)))
}

/// The server's copy of `path` equals the buffer: the sync is exact.
fn server_text(h: &Harness<'static, PhpIdeApp>, path: &Path) -> serde_json::Value {
    let lsp = h.state().lsp.as_ref().unwrap();
    phpide_lsp::fixture::state(lsp, path).expect("fixture state")
}

#[test]
fn fixture_server_drives_every_php_feature_through_the_ui() {
    let (_dir, root) = fixture_project();
    let app_php = root.join("src/app.php");
    let mut h = harness(&root, vec![app_php.clone()]);
    h.run_steps(3);
    h.state_mut().use_fixture_lsp();
    step_until(&mut h, "the server", running);
    step_until(&mut h, "diagnostics", |h| {
        h.state()
            .ws
            .diagnostics
            .iter()
            .any(|d| d.message == "Undefined function 'missing_call'")
    });
    assert!(
        h.query_all_by_label_contains("fixture-php")
            .next()
            .is_some(),
        "status bar shows the server"
    );

    // ── Incremental sync: typing sends ranged changes only.
    caret_at(&mut h, "echo add", 0);
    type_text(&mut h, "// sync");
    h.key_press(egui::Key::Enter);
    h.run_steps(1);
    step_until(&mut h, "the server copy", |h| {
        server_text(h, &app_php)["text"].as_str() == Some(text(h).as_str())
    });
    let state = server_text(&h, &app_php);
    assert_eq!(state["fullChanges"], 0, "no full-text resync while typing");
    assert!(state["rangedChanges"].as_u64().unwrap() >= 1);

    // ── Completion: `->` opens it, typing filters, Enter inserts the snippet.
    caret_at(&mut h, "echo add", 0);
    type_text(&mut h, "$greeter->");
    step_until(&mut h, "member completion", |h| {
        h.state().completion.open
            && h.state()
                .completion
                .visible()
                .iter()
                .any(|i| i.label == "greet")
    });
    type_text(&mut h, "gr");
    step_until(&mut h, "the filter", |h| {
        h.state()
            .completion
            .selected_item()
            .map(|i| i.label.as_str())
            == Some("greet")
    });
    assert!(
        h.query_by_label("greet").is_some(),
        "the popup lists the item"
    );
    step_until(&mut h, "resolved documentation", |h| {
        h.state()
            .completion
            .selected_item()
            .and_then(|i| i.documentation.clone())
            .is_some_and(|d| d.contains("Says hello"))
    });
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert!(!h.state().completion.open);
    assert!(
        text(&h).contains("$greeter->greet($name, $times)"),
        "{}",
        text(&h)
    );
    // The first placeholder is selected; Tab goes to the next one.
    let sel = |h: &mut Harness<'static, PhpIdeApp>| {
        let app = h.state_mut();
        let (a, b) = app.editors.selection(&app.ws).unwrap();
        app.ws
            .active_buffer()
            .unwrap()
            .document
            .rope()
            .slice(a..b)
            .to_string()
    };
    assert_eq!(sel(&mut h), "$name");
    // Parameter Info opened after the `(`.
    step_until(&mut h, "parameter info", |h| {
        h.state().intel.signature.help.as_ref().is_some_and(|v| {
            v["signatures"][0]["label"] == "function greet(string $name, int $times): string"
        })
    });
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    assert_eq!(sel(&mut h), "$times");
    type_text(&mut h, "2");
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    assert!(
        text(&h).contains("$greeter->greet($name, 2)"),
        "{}",
        text(&h)
    );
    h.key_press(egui::Key::Escape);
    h.run_steps(2);

    // ── Documentation on mouse hover: rest the pointer on `add`.
    caret_at(&mut h, "add(1", 1);
    let at = h
        .state()
        .editors
        .caret_rect
        .expect("caret on screen")
        .center();
    h.event(egui::Event::PointerMoved(at + egui::vec2(3.0, 0.0)));
    std::thread::sleep(Duration::from_millis(550));
    step_until(&mut h, "hover documentation", |h| {
        h.state()
            .intel
            .hover
            .shown
            .as_ref()
            .and_then(|s| s.text.clone())
            .is_some_and(|t| t.contains("Adds two numbers"))
    });
    h.event(egui::Event::PointerMoved(egui::pos2(5.0, 790.0)));
    h.run_steps(2);
    assert!(
        h.state().intel.hover.shown.is_none(),
        "leaving the word hides it"
    );

    // ── Quick Documentation (Ctrl+Q) on `add`.
    caret_at(&mut h, "add(1", 1);
    keys(&mut h, egui::Modifiers::CTRL, egui::Key::Q);
    step_until(&mut h, "documentation", |h| {
        h.state()
            .intel
            .hover
            .shown
            .as_ref()
            .and_then(|s| s.text.clone())
            .is_some_and(|t| t.contains("Adds two numbers"))
    });
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(h.state().intel.hover.shown.is_none());

    // ── Context action (Alt+Enter) on the undefined function.
    caret_at(&mut h, "missing_call", 2);
    keys(&mut h, egui::Modifiers::ALT, egui::Key::Enter);
    step_until(&mut h, "context actions", |h| {
        h.state()
            .intel
            .actions
            .as_ref()
            .is_some_and(|a| !a.items.is_empty())
    });
    h.run_steps(2);
    assert!(h.query_by_label("Create function 'missing_call'").is_some());
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert!(text(&h).contains("function missing_call()"), "{}", text(&h));
    step_until(&mut h, "the fixed diagnostic", |h| {
        !h.state()
            .ws
            .diagnostics
            .iter()
            .any(|d| d.message.contains("missing_call"))
    });

    // ── Rename (Shift+F6) across files: helpers.php is not open.
    caret_at(&mut h, "add(1", 1);
    keys(&mut h, egui::Modifiers::SHIFT, egui::Key::F6);
    step_until(&mut h, "the rename dialog", |h| {
        h.state()
            .intel
            .rename
            .as_ref()
            .is_some_and(|r| r.name == "add")
    });
    h.run_steps(2);
    // Select the name in the field (Ctrl+A as egui-winit sends it).
    keys(
        &mut h,
        egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
        egui::Key::A,
    );
    type_text(&mut h, "sum");
    h.key_press(egui::Key::Enter);
    step_until(&mut h, "the rename", |h| h.state().intel.rename.is_none());
    assert!(text(&h).contains("echo sum(1, 2)"), "{}", text(&h));
    let helpers = std::fs::read_to_string(root.join("src/helpers.php")).unwrap();
    assert!(
        helpers.contains("function sum(int $a, int $b)"),
        "{helpers}"
    );
    assert!(h.state().ws.message.starts_with("Renamed 'add' to 'sum'"));

    // ── Reformat Code (Ctrl+Alt+L).
    caret_at(&mut h, "echo sum", 0);
    type_text(&mut h, "$x = 1;   ");
    h.key_press(egui::Key::Enter);
    h.run_steps(1);
    assert!(text(&h).contains("$x = 1;   \n"), "{}", text(&h));
    keys(
        &mut h,
        egui::Modifiers::CTRL | egui::Modifiers::ALT,
        egui::Key::L,
    );
    step_until(&mut h, "formatting", |h| text(h).contains("$x = 1;\n"));
    assert!(
        h.state().ws.message.starts_with("Reformatted app.php"),
        "{}",
        h.state().ws.message
    );

    // ── Server state, log and restart from the status bar widget.
    h.state_mut().intel.log_open = true;
    h.run_steps(2);
    assert!(h.query_by_label("Language Server Log").is_some());
    h.state_mut().intel.log_open = false;
    let ctx = h.ctx.clone();
    h.state_mut()
        .dispatch_command(crate::commands::Command::RestartLsp, &ctx);
    std::thread::sleep(Duration::from_millis(20));
    h.state_mut().use_fixture_lsp();
    step_until(&mut h, "the restarted server", running);
}

#[test]
fn file_structure_popup_filters_and_jumps() {
    let (_dir, root) = fixture_project();
    let greeter = root.join("src/Greeter.php");
    let mut h = harness(&root, vec![greeter.clone()]);
    h.run_steps(3);
    h.state_mut().use_fixture_lsp();
    step_until(&mut h, "the server", running);
    step_until(&mut h, "the outline", |h| h.state().ws.outline.len() >= 3);
    h.state_mut().editors.request_focus();
    h.run_steps(2);
    keys(&mut h, egui::Modifiers::CTRL, egui::Key::F12);
    assert!(h.state().intel.structure.is_some());
    type_text(&mut h, "wave");
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert!(h.state().intel.structure.is_none());
    let app = h.state();
    let buf = app.ws.active_buffer().unwrap();
    let caret_line = buf.document.rope().char_to_line(app.cursor_char);
    assert_eq!(caret_line, 12, "the caret is on `wave`");
}

/// intelephense / phpactor on a small Laravel-shaped project. Skipped when
/// no server is installed.
#[test]
fn real_servers_on_laravel_fixture() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/laravel-mini");
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&fixture, dir.path());
    let root = dir.path().to_path_buf();
    let Some((command, args)) = phpide_lsp::detect_server(&root) else {
        eprintln!("SKIP real_servers_on_laravel_fixture: install intelephense or phpactor");
        return;
    };
    eprintln!("real server: {command} {}", args.join(" "));
    let controller = root.join("app/Http/Controllers/UserController.php");
    let mut h = harness(&root, vec![controller.clone()]);
    h.run_steps(3);
    {
        let app = h.state_mut();
        app.restart_lsp();
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while !running(&h) {
        h.step();
        assert!(
            std::time::Instant::now() < deadline,
            "the server did not start"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let caps = h.state().lsp.as_ref().unwrap().capabilities();
    // Let the server index the project.
    std::thread::sleep(Duration::from_secs(3));

    // Completion on `$user->`.
    caret_at(&mut h, "return view", 0);
    type_text(&mut h, "$user->");
    let slow = |h: &mut Harness<'static, PhpIdeApp>,
                what: &str,
                done: &dyn Fn(&Harness<'static, PhpIdeApp>) -> bool| {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while !done(h) {
            h.step();
            assert!(std::time::Instant::now() < deadline, "timed out: {what}");
            std::thread::sleep(Duration::from_millis(30));
        }
    };
    slow(&mut h, "completion of User methods", &|h| {
        h.state()
            .completion
            .visible()
            .iter()
            // PHPantom labels methods with their parameters: "fullName()".
            .any(|i| i.label == "fullName" || i.label.starts_with("fullName("))
    });
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    // Undo the probe text.
    for _ in 0..7 {
        h.key_press(egui::Key::Backspace);
        h.run_steps(1);
    }

    if caps.hover_provider {
        caret_at(&mut h, "User::find", 1);
        keys(&mut h, egui::Modifiers::CTRL, egui::Key::Q);
        slow(&mut h, "hover on User", &|h| {
            h.state()
                .intel
                .hover
                .shown
                .as_ref()
                .and_then(|s| s.text.clone())
                .is_some_and(|t| t.contains("User"))
        });
        h.key_press(egui::Key::Escape);
        h.run_steps(2);
    }
    if caps.formatting_provider {
        caret_at(&mut h, "return view", 0);
        type_text(&mut h, "   ");
        keys(
            &mut h,
            egui::Modifiers::CTRL | egui::Modifiers::ALT,
            egui::Key::L,
        );
        slow(&mut h, "formatting", &|h| {
            !text(h).contains("           return view")
        });
    }
    if caps.rename_provider {
        caret_at(&mut h, "$users", 2);
        keys(&mut h, egui::Modifiers::SHIFT, egui::Key::F6);
        slow(&mut h, "rename dialog", &|h| {
            h.state()
                .intel
                .rename
                .as_ref()
                .is_some_and(|r| !r.name.is_empty())
        });
        h.run_steps(2);
        keys(
            &mut h,
            egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
            egui::Key::A,
        );
        type_text(&mut h, "$people");
        h.key_press(egui::Key::Enter);
        slow(&mut h, "rename", &|h| h.state().intel.rename.is_none());
        // intelephense renames only in its premium edition: then the
        // message says why and the text is unchanged.
        let renamed = text(&h).contains("$people");
        eprintln!("rename applied: {renamed} ({})", h.state().ws.message);
    }
}

fn copy_dir(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            std::fs::create_dir_all(&target).unwrap();
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// Review images of the Phase 5 popups in the three themes
/// (`target/snapshots/phase5-*.png`): `cargo test -p phpide-ui -- --ignored phase5`.
#[test]
#[ignore = "writes review images"]
fn phase5_screenshots() {
    use crate::design::{install, snapshot, Density, Theme, ThemeId};
    let (_dir, root) = fixture_project();
    let app_php = root.join("src/app.php");
    let mut h = harness(&root, vec![app_php]);
    h.run_steps(3);
    h.state_mut().use_fixture_lsp();
    step_until(&mut h, "the server", running);
    step_until(&mut h, "diagnostics", |h| {
        !h.state().ws.diagnostics.is_empty()
    });
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/snapshots");
    std::fs::create_dir_all(&out).unwrap();
    let shoot = |h: &mut Harness<'static, PhpIdeApp>, name: &str| {
        for (theme, label) in [
            (ThemeId::Dark, "dark"),
            (ThemeId::Light, "light"),
            (ThemeId::HighContrast, "high-contrast"),
        ] {
            let ctx = egui::Context::default();
            install(&ctx, Theme::new(theme, Density::Comfortable));
            h.state_mut().editors.clear_layouts();
            let image = snapshot::render(&ctx, egui::vec2(1280.0, 800.0), 4, |ui| {
                h.state_mut().frame_ui(ui)
            });
            image.write_png(&out.join(format!("phase5-{name}-{label}.png")));
        }
        h.state_mut().editors.clear_layouts();
    };

    caret_at(&mut h, "echo add", 0);
    type_text(&mut h, "$greeter->g");
    step_until(&mut h, "completion docs", |h| {
        h.state()
            .completion
            .selected_item()
            .is_some_and(|i| i.documentation.is_some())
    });
    shoot(&mut h, "completion");
    h.key_press(egui::Key::Escape);
    h.run_steps(2);

    caret_at(&mut h, "add(1, 2)", 7);
    keys(&mut h, egui::Modifiers::CTRL, egui::Key::P);
    step_until(&mut h, "parameter info", |h| {
        h.state().intel.signature.help.is_some()
    });
    shoot(&mut h, "parameter-info");
    h.key_press(egui::Key::Escape);
    h.run_steps(2);

    caret_at(&mut h, "add(1, 2)", 1);
    keys(&mut h, egui::Modifiers::CTRL, egui::Key::Q);
    step_until(&mut h, "documentation", |h| {
        h.state()
            .intel
            .hover
            .shown
            .as_ref()
            .is_some_and(|s| s.text.is_some())
    });
    shoot(&mut h, "documentation");
    h.key_press(egui::Key::Escape);
    h.run_steps(2);

    caret_at(&mut h, "missing_call", 2);
    keys(&mut h, egui::Modifiers::ALT, egui::Key::Enter);
    step_until(&mut h, "context actions", |h| {
        h.state()
            .intel
            .actions
            .as_ref()
            .is_some_and(|a| !a.items.is_empty())
    });
    shoot(&mut h, "context-actions");
    h.key_press(egui::Key::Escape);
    h.run_steps(2);

    caret_at(&mut h, "add(1", 1);
    keys(&mut h, egui::Modifiers::SHIFT, egui::Key::F6);
    step_until(&mut h, "rename", |h| h.state().intel.rename.is_some());
    h.run_steps(3);
    shoot(&mut h, "rename");
}
