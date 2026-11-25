//! Phase 11 gate: PHPantom through the UI.
//!
//! Runs the real PHPantom (bundled, `third_party/phpantom/` from
//! `scripts/fetch-phpantom.sh`, or `phpantom_lsp` on PATH) on a small
//! project: indexing progress, Go to Super Method, the type and call
//! hierarchies, code lenses, Extend/Shrink Selection, document links and the
//! restart after a crash. Without PHPantom the tests are skipped, unless
//! `PHPIDE_REQUIRE_PHPANTOM` (CI) or `PHPIDE_REQUIRE_INTEGRATIONS` is set.
//!
//! The settings PHPantom gets from the IDE (excluded folders, attached
//! projects) are checked with the fixture server, which reports what it
//! received.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use egui_kittest::Harness;
use phpide_lsp::engine::{Discovery, EngineChoice, EngineKind};

use crate::toolwindows::ToolWindow;
use crate::ui_tests::{harness, step_until};
use crate::PhpIdeApp;

const ANIMAL: &str =
    "<?php\n\nnamespace Zoo;\n\ninterface Animal\n{\n    public function speak(): string;\n}\n";
const DOG: &str = "<?php\n\nnamespace Zoo;\n\nclass Dog implements Animal\n{\n    public function speak(): string\n    {\n        return 'woof';\n    }\n}\n";
const PUPPY: &str = "<?php\n\nnamespace Zoo;\n\nclass Puppy extends Dog\n{\n    public function speak(): string\n    {\n        return parent::speak() . '!';\n    }\n}\n";
const MAIN: &str = "<?php\n\nnamespace Zoo;\n\nrequire __DIR__ . '/Dog.php';\n\nfunction talk(Animal $animal): string\n{\n    return $animal->speak();\n}\n\nfunction main(): void\n{\n    echo talk(new Puppy());\n}\n";

fn zoo() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("composer.json"),
        r#"{ "name": "acme/zoo", "autoload": { "psr-4": { "Zoo\\": "src/" } } }"#,
    )
    .unwrap();
    for (name, text) in [
        ("Animal.php", ANIMAL),
        ("Dog.php", DOG),
        ("Puppy.php", PUPPY),
        ("main.php", MAIN),
    ] {
        std::fs::write(root.join("src").join(name), text).unwrap();
    }
    (dir, root)
}

/// Whether PHPantom is installed; prints the skip otherwise.
fn phpantom(root: &Path) -> bool {
    let found = Discovery {
        env_command: None,
        ..Discovery::system(root)
    }
    .resolve(&EngineChoice::Phpantom);
    match found {
        Ok(server) => {
            assert_eq!(server.kind, EngineKind::Phpantom);
            true
        }
        Err(e) => {
            if std::env::var_os("PHPIDE_REQUIRE_PHPANTOM").is_some()
                || std::env::var_os("PHPIDE_REQUIRE_INTEGRATIONS").is_some()
            {
                panic!("PHPantom is required: {e}");
            }
            eprintln!("GATE SKIP: {e}");
            false
        }
    }
}

/// Steps until `done`, for answers that come from a real server.
fn wait(
    h: &mut Harness<'static, PhpIdeApp>,
    what: &str,
    done: impl Fn(&Harness<'static, PhpIdeApp>) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done(h) {
        h.step();
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn running(h: &Harness<'static, PhpIdeApp>) -> bool {
    h.state()
        .lsp
        .as_ref()
        .is_some_and(|l| matches!(l.status(), phpide_lsp::LspStatus::Running(_)))
}

/// Opens the zoo with PHPantom started the way the app starts it.
fn open(root: &Path, file: &str) -> Harness<'static, PhpIdeApp> {
    let mut h = harness(root, vec![root.join("src").join(file)]);
    h.run_steps(2);
    {
        let app = h.state_mut();
        app.settings.lsp_engine = "phpantom".into();
        app.restart_lsp();
        assert_eq!(app.lsp_name, "PHPantom", "{:?}", app.lsp_start_error);
    }
    wait(&mut h, "PHPantom running", running);
    h
}

fn active_file(h: &Harness<'static, PhpIdeApp>) -> String {
    h.state()
        .ws
        .active_buffer()
        .and_then(|b| b.path.as_ref())
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Puts the caret on `needle` (plus `offset` chars) in the active file.
fn caret_at(h: &mut Harness<'static, PhpIdeApp>, needle: &str, offset: usize) -> usize {
    let text = h.state().ws.active_buffer().unwrap().document.text();
    let byte = text.find(needle).unwrap_or_else(|| panic!("{needle:?}"));
    let pos = text[..byte].chars().count() + offset;
    let app = h.state_mut();
    app.editors.set_caret(&mut app.ws, pos);
    app.cursor_char = pos;
    app.editors.request_focus();
    h.run_steps(2);
    pos
}

fn press(h: &mut Harness<'static, PhpIdeApp>, modifiers: egui::Modifiers, key: egui::Key) {
    h.key_press_modifiers(modifiers, key);
    h.run_steps(2);
}

fn selected(h: &mut Harness<'static, PhpIdeApp>) -> String {
    let app = h.state_mut();
    let (a, b) = app.editors.selection(&app.ws).unwrap();
    let text = app.ws.active_buffer().unwrap().document.text();
    text.chars().skip(a).take(b - a).collect()
}

#[test]
fn phpantom_indexes_and_navigates() {
    let (_dir, root) = zoo();
    if !phpantom(&root) {
        return;
    }
    let mut h = open(&root, "Puppy.php");
    let caps = h.state().lsp.as_ref().unwrap().capabilities();
    assert_eq!(caps.server_name, "PHPantom");
    assert!(!caps.server_version.is_empty());

    // Indexing is reported: begin and end reach the Event Log.
    wait(&mut h, "indexing in the Event Log", |h| {
        h.state()
            .event_log
            .entries()
            .any(|e| e.text.contains("Indexing done"))
    });

    // Go to Super Method (Ctrl+U): Puppy::speak overrides the interface's.
    caret_at(&mut h, "speak", 1);
    press(&mut h, egui::Modifiers::CTRL, egui::Key::U);
    wait(&mut h, "Animal.php opened", |h| {
        active_file(h) == "Animal.php"
    });

    // Go to Implementation (Ctrl+Alt+B) on the interface method: two, so
    // they are listed.
    caret_at(&mut h, "speak", 1);
    press(
        &mut h,
        egui::Modifiers::CTRL | egui::Modifiers::ALT,
        egui::Key::B,
    );
    wait(&mut h, "implementations listed", |h| {
        h.state().search.results.len() == 2
    });
    let files: Vec<String> = h
        .state()
        .search
        .results
        .iter()
        .map(|r| r.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(files.contains(&"Dog.php".to_string()) && files.contains(&"Puppy.php".to_string()));
}

#[test]
fn phpantom_hierarchies() {
    let (_dir, root) = zoo();
    if !phpantom(&root) {
        return;
    }
    let mut h = open(&root, "Puppy.php");
    // Type Hierarchy (Ctrl+H): Puppy, then its supertypes.
    caret_at(&mut h, "Puppy", 1);
    press(&mut h, egui::Modifiers::CTRL, egui::Key::H);
    wait(&mut h, "Puppy's supertypes", |h| {
        let hierarchy = &h.state().hierarchy;
        hierarchy
            .nodes
            .iter()
            .any(|n| n.name == "Dog" && n.depth == 1)
    });
    assert!(h.state().tw.is_visible(ToolWindow::Hierarchy));
    h.run_steps(2);
    assert!(
        egui_kittest::kittest::Queryable::query_by_label(&h, "Supertypes").is_some(),
        "the direction switch is shown"
    );
    // Expanding Dog loads Animal.
    let dog = h
        .state()
        .hierarchy
        .nodes
        .iter()
        .position(|n| n.name == "Dog")
        .unwrap();
    h.state_mut().hierarchy.nodes[dog].expanded = false;
    h.run_steps(1);
    let expand = egui_kittest::kittest::Queryable::get_all_by_label(&h, "Expand")
        .next()
        .expect("an expand button");
    expand.click();
    wait(&mut h, "Animal under Dog", |h| {
        h.state()
            .hierarchy
            .nodes
            .iter()
            .any(|n| n.name == "Animal" && n.depth == 2)
    });
    // Clicking a row opens its declaration.
    egui_kittest::kittest::Queryable::get_by_label(&h, "Animal").click();
    wait(&mut h, "Animal.php opened", |h| {
        active_file(h) == "Animal.php"
    });

    // Call Hierarchy (Ctrl+Alt+H) of talk(): main() calls it.
    h.state_mut()
        .ws
        .open_file_at(&root.join("src/main.php"), 0, 0);
    h.run_steps(2);
    caret_at(&mut h, "talk(Animal", 1);
    press(
        &mut h,
        egui::Modifiers::CTRL | egui::Modifiers::ALT,
        egui::Key::H,
    );
    wait(&mut h, "callers of talk", |h| {
        h.state()
            .hierarchy
            .nodes
            .iter()
            .any(|n| n.name == "main" && n.depth == 1)
    });
    assert!(egui_kittest::kittest::Queryable::query_by_label(&h, "Callers").is_some());
}

#[test]
fn phpantom_lenses_selection_and_links() {
    let (_dir, root) = zoo();
    if !phpantom(&root) {
        return;
    }
    let mut h = open(&root, "Animal.php");
    // Code lenses after the declarations, resolved: "2 implementations".
    wait(&mut h, "resolved lenses", |h| {
        h.state()
            .code_lens
            .current
            .as_ref()
            .is_some_and(|(_, _, lenses)| lenses.iter().any(|l| l.title == "2 implementations"))
    });
    let index = h
        .state()
        .code_lens
        .current
        .as_ref()
        .unwrap()
        .2
        .iter()
        .position(|l| l.title == "2 implementations")
        .unwrap();
    h.state_mut().click_lens(index);
    h.run_steps(2);
    assert_eq!(h.state().search.results.len(), 2, "the lens lists them");
    assert!(h.state().tw.is_visible(ToolWindow::Find));

    // Extend Selection (Ctrl+W), twice, then Shrink (Ctrl+Shift+W).
    h.state_mut()
        .ws
        .open_file_at(&root.join("src/main.php"), 0, 0);
    h.run_steps(2);
    caret_at(&mut h, "$animal->speak", 3);
    press(&mut h, egui::Modifiers::CTRL, egui::Key::W);
    wait(&mut h, "first extension", |h| {
        h.state().selection_ranges_cached()
    });
    h.run_steps(1);
    assert_eq!(selected(&mut h), "$animal");
    press(&mut h, egui::Modifiers::CTRL, egui::Key::W);
    assert_eq!(selected(&mut h), "$animal->speak()");
    press(
        &mut h,
        egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
        egui::Key::W,
    );
    assert_eq!(selected(&mut h), "$animal");

    // Ctrl+click on the required path follows the document link.
    wait(&mut h, "document links", |h| {
        h.state()
            .document_links
            .current
            .as_ref()
            .is_some_and(|(_, _, links)| !links.is_empty())
    });
    let pos = caret_at(&mut h, "/Dog.php", 2);
    let ctx = h.ctx.clone();
    assert!(h.state_mut().follow_link_at(pos, &ctx));
    h.run_steps(2);
    assert_eq!(active_file(&h), "Dog.php");
}

#[test]
fn phpantom_is_restarted_after_a_crash() {
    let (_dir, root) = zoo();
    if !phpantom(&root) {
        return;
    }
    let mut h = open(&root, "Dog.php");
    // The process dies.
    *h.state()
        .lsp
        .as_ref()
        .unwrap()
        .shared
        .status
        .lock()
        .unwrap() = phpide_lsp::LspStatus::Failed("killed".into());
    h.run_steps(2);
    assert!(
        h.state().ws.message.contains("restarting it in 1 s"),
        "{}",
        h.state().ws.message
    );
    assert!(h.state().lsp_supervisor.waiting());
    wait(&mut h, "restarted", running);
    assert!(!h.state().lsp_supervisor.waiting());
}

/// Fixture server: the excluded folders and the attached projects reach
/// the server at startup and when they change.
#[test]
fn indexing_filters_and_attached_projects_reach_the_server() {
    let (_dir, root) = zoo();
    let other = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.join("build")).unwrap();
    let mut h = harness(&root, vec![root.join("src/Dog.php")]);
    h.run_steps(2);
    h.state_mut().project_marks.set(
        &root.join("build"),
        Some(crate::project_marks::Mark::Excluded),
    );
    h.state_mut().use_fixture_lsp();
    step_until(&mut h, "the fixture server", running);
    let state = |h: &Harness<'static, PhpIdeApp>| {
        let lsp = h.state().lsp.as_ref().unwrap();
        phpide_lsp::fixture::state(lsp, &root.join("src/Dog.php")).unwrap()
    };
    let first = state(&h);
    assert_eq!(first["settings"][0]["indexing"]["exclude"][0], "/build/");
    assert_eq!(first["folders"].as_array().unwrap().len(), 1);
    // The progress the fixture reports reaches the Event Log.
    step_until(&mut h, "indexing in the Event Log", |h| {
        let log: Vec<String> = h
            .state()
            .event_log
            .entries()
            .map(|e| e.text.clone())
            .collect();
        log.iter().any(|t| t == "fixture-php: Indexing…")
            && log
                .iter()
                .any(|t| t.starts_with("fixture-php: Indexing done: "))
    });

    // A new mark and a file type: didChangeConfiguration.
    h.state_mut().project_marks.set(
        &root.join("src"),
        Some(crate::project_marks::Mark::Excluded),
    );
    h.state_mut()
        .settings
        .file_type_patterns
        .push(("*.module".into(), "php".into()));
    h.run_steps(3);
    let changed = state(&h);
    let last = changed["settings"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(
        last["indexing"]["exclude"],
        serde_json::json!(["/build/", "/src/"])
    );
    assert_eq!(
        last["indexing"]["extensions"],
        serde_json::json!(["module"])
    );

    // Attaching a project adds a workspace folder; detaching removes it.
    h.state_mut().attach_project(other.path());
    h.run_steps(3);
    assert_eq!(state(&h)["folders"].as_array().unwrap().len(), 2);
    h.state_mut().detach_project(other.path());
    h.run_steps(3);
    assert_eq!(state(&h)["folders"].as_array().unwrap().len(), 1);
}

/// Settings › PHP › Language Server lists the engines and what runs.
#[test]
fn language_server_settings_page() {
    let (_dir, root) = zoo();
    let mut h = harness(&root, Vec::new());
    h.run_steps(2);
    let global = h.state().settings.lsp_engine.clone();
    h.state_mut().use_fixture_lsp();
    step_until(&mut h, "the fixture server", running);
    h.state_mut()
        .settings_dialog
        .open(crate::settings_dialog::SettingsPage::PhpLanguageServer);
    h.run_steps(3);
    use egui_kittest::kittest::Queryable;
    for label in [
        "Automatic (bundled PHPantom first)",
        "PHPantom",
        "intelephense",
        "phpactor",
        "Custom command",
        "Use this choice for this project only",
        "Restart",
        "Show Log",
        "Open .phpantom.toml",
    ] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    // A custom command for this project only: stored in project.json.
    h.get_by_label("Use this choice for this project only")
        .click();
    h.run_steps(2);
    assert_eq!(
        crate::lsp_engine::project_choice(&root),
        Some(EngineChoice::Automatic)
    );
    h.get_by_label("Custom command").click();
    h.run_steps(3);
    assert_eq!(
        crate::lsp_engine::project_choice(&root),
        Some(EngineChoice::Custom(String::new()))
    );
    assert!(h.state().lsp.is_none());
    assert!(
        h.state()
            .lsp_start_error
            .as_deref()
            .is_some_and(|e| e.contains("empty")),
        "{:?}",
        h.state().lsp_start_error
    );
    // The global choice is untouched.
    assert_eq!(h.state().settings.lsp_engine, global);
}

/// Review images of Phase 11 (`target/snapshots/phase11-*.png`): code
/// lenses, the Hierarchy tool window, the Language Server settings and the
/// status bar while PHPantom works.
/// `cargo test -p phpide-ui -- --ignored phase11`.
#[test]
#[ignore = "writes review images"]
fn phase11_screenshots() {
    use crate::design::{install, snapshot, Density, Theme, ThemeId};
    let (_dir, root) = zoo();
    if !phpantom(&root) {
        return;
    }
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/snapshots");
    std::fs::create_dir_all(&out).unwrap();
    let shoot = |h: &mut Harness<'static, PhpIdeApp>, name: &str| {
        for (theme, label) in [(ThemeId::Dark, "dark"), (ThemeId::Light, "light")] {
            let ctx = h.ctx.clone();
            install(&ctx, Theme::new(theme, Density::Comfortable));
            h.state_mut().editors.clear_layouts();
            let image = snapshot::render(&ctx, egui::vec2(1280.0, 800.0), 12, |ui| {
                h.state_mut().frame_ui(ui)
            });
            image.write_png(&out.join(format!("phase11-{name}-{label}.png")));
        }
        install(&h.ctx, Theme::default());
        h.state_mut().editors.clear_layouts();
    };
    let mut h = open(&root, "Animal.php");
    wait(&mut h, "resolved lenses", |h| {
        h.state()
            .code_lens
            .current
            .as_ref()
            .is_some_and(|(_, _, lenses)| {
                lenses
                    .iter()
                    .filter(|l| l.title.contains("implementations"))
                    .count()
                    == 2
            })
    });
    caret_at(&mut h, "Animal", 1);
    press(&mut h, egui::Modifiers::CTRL, egui::Key::H);
    wait(&mut h, "subtypes", |h| {
        !h.state().hierarchy.nodes.is_empty()
    });
    h.state_mut().hierarchy.upward = true;
    h.run_steps(1);
    egui_kittest::kittest::Queryable::get_by_label(&h, "Subtypes").click();
    wait(&mut h, "Dog", |h| {
        h.state()
            .hierarchy
            .nodes
            .iter()
            .any(|n| n.name == "Dog" && n.depth == 1)
    });
    h.run_steps(1);
    egui_kittest::kittest::Queryable::get_all_by_label(&h, "Expand")
        .next()
        .expect("Dog can expand")
        .click();
    wait(&mut h, "Puppy", |h| {
        h.state().hierarchy.nodes.iter().any(|n| n.name == "Puppy")
    });
    h.run_steps(2);
    shoot(&mut h, "lenses-hierarchy");

    // Work in progress in the status bar.
    {
        let lsp = h.state().lsp.as_ref().unwrap();
        lsp.shared
            .progress
            .lock()
            .unwrap()
            .push(phpide_lsp::Progress {
                token: "shot".into(),
                title: "PHPantom: Full index".into(),
                message: "Parsing workspace files".into(),
                percentage: Some(42),
            });
    }
    h.state_mut()
        .settings_dialog
        .open(crate::settings_dialog::SettingsPage::PhpLanguageServer);
    h.run_steps(3);
    shoot(&mut h, "settings");
}
