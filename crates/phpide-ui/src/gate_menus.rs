//! Phase 10 gate: menus driven through the UI. The editor context menu (in
//! the text and on the gutter), with and without a language server; the
//! main menu built from the command registry; the check items of the Project
//! view Options menu. `phase10_menus` (ignored) writes screenshots of the
//! menus in the three themes for review.

use std::path::{Path, PathBuf};

use eframe::egui;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;

use crate::ui_tests::{harness, project, step_until};
use crate::PhpIdeApp;

const ORDER: &str = "<?php\n\nnamespace App;\n\nfinal class Order\n{\n    public function total(): int\n    {\n        return 42;\n    }\n}\n";

fn order_project() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let (dir, root) = project();
    let file = root.join("src/Order.php");
    std::fs::write(&file, ORDER).unwrap();
    (dir, root, file)
}

fn breakpoints(h: &Harness<'static, PhpIdeApp>, file: &Path) -> Vec<u32> {
    h.state()
        .rd
        .bp_store
        .lock()
        .unwrap()
        .breakpoints
        .iter()
        .filter(|b| b.file.as_deref() == Some(file))
        .map(|b| b.line)
        .collect()
}

/// A right click at `pos` (pointer moved there first, as a mouse does).
fn right_click_at(h: &mut Harness<'static, PhpIdeApp>, pos: egui::Pos2) {
    h.event(egui::Event::PointerMoved(pos));
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::default(),
        });
    }
    h.run_steps(2);
}

fn editor_rect(h: &Harness<'static, PhpIdeApp>) -> egui::Rect {
    h.get_by_label("Editor").rect()
}

/// The first row of the gutter (line 1), measured now.
fn gutter_line_1(h: &Harness<'static, PhpIdeApp>) -> egui::Pos2 {
    editor_rect(h).left_top() + egui::vec2(10.0, 8.0)
}

fn server_running(h: &Harness<'static, PhpIdeApp>) -> bool {
    h.state()
        .lsp
        .as_ref()
        .is_some_and(|l| matches!(l.status(), phpide_lsp::LspStatus::Running(_)))
}

#[test]
fn editor_menu_without_a_server_offers_editing_run_and_breakpoints() {
    let (_dir, root, file) = order_project();
    let mut h = harness(&root, vec![file.clone()]);
    h.run_steps(3);
    let rect = editor_rect(&h);
    right_click_at(&mut h, rect.center());
    for label in [
        "Cut",
        "Copy",
        "Paste",
        "Run 'Order.php'",
        "Debug 'Order.php'",
    ] {
        assert!(h.query_by_label(label).is_some(), "{label} in the menu");
    }
    // No server: navigation, refactoring and formatting are not offered.
    for label in ["Go To", "Refactor", "Reformat Code", "Show Context Actions"] {
        assert!(h.query_by_label(label).is_none(), "{label} needs a server");
    }
    // Nothing is selected: Cut and Copy are off.
    assert!(h.get_by_label("Cut").accesskit_node().is_disabled());
    assert!(!h.get_by_label("Paste").accesskit_node().is_disabled());
    h.get_by_label("Toggle Breakpoint").click();
    h.run_steps(2);
    assert!(h.query_by_label("Cut").is_none(), "the menu closed");
    let caret_line = {
        let app = h.state();
        let buffer = app.ws.active_buffer().unwrap();
        buffer.document.rope().char_to_line(app.cursor_char) as u32 + 1
    };
    assert_eq!(breakpoints(&h, &file), vec![caret_line]);
}

#[test]
fn gutter_menu_toggles_a_breakpoint_on_its_line() {
    let (_dir, root, file) = order_project();
    let mut h = harness(&root, vec![file.clone()]);
    h.run_steps(3);
    let gutter = gutter_line_1(&h);
    right_click_at(&mut h, gutter);
    h.get_by_label("Toggle Breakpoint at Line 1").click();
    h.run_steps(2);
    assert_eq!(breakpoints(&h, &file), vec![1]);
    // The same item removes it.
    let gutter = gutter_line_1(&h);
    right_click_at(&mut h, gutter);
    h.get_by_label("Toggle Breakpoint at Line 1").click();
    h.run_steps(2);
    assert!(breakpoints(&h, &file).is_empty());
}

#[test]
fn editor_menu_with_a_server_offers_go_to_refactor_and_actions() {
    let (_dir, root, file) = order_project();
    // Every line is one long variable, so the click at the center of the
    // editor lands on a symbol the test server can rename.
    let body: String = (0..80)
        .map(|_| "$theOrderTotalWithTaxesShippingDiscountsAndEverythingElseOnTheInvoice = 1;\n")
        .collect();
    std::fs::write(&file, format!("<?php\n{body}")).unwrap();
    let mut h = harness(&root, vec![file.clone()]);
    h.run_steps(3);
    h.state_mut().use_fixture_lsp();
    step_until(&mut h, "the server", server_running);
    let center = editor_rect(&h).center();
    right_click_at(&mut h, center);
    for label in ["Go To", "Refactor", "Show Context Actions", "Cut"] {
        assert!(h.query_by_label(label).is_some(), "{label} in the menu");
    }
    h.get_by_label("Go To").hover();
    h.run_steps(3);
    assert!(h.query_by_label("Go to Definition").is_some());
    // Reopened, for the same reason as in the main menu test.
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    right_click_at(&mut h, center);
    h.get_by_label("Refactor").hover();
    h.run_steps(3);
    h.get_by_label("Rename Symbol…").click();
    step_until(&mut h, "the rename field", |h| {
        h.state()
            .intel
            .rename
            .as_ref()
            .is_some_and(|r| r.name.contains("theOrderTotal"))
    });
    // The server accepted it: the field stays open.
    h.run_steps(10);
    assert!(h.state().intel.rename.is_some(), "{}", h.state().ws.message);
}

#[test]
fn main_menu_lists_registry_commands_and_tool_window_toggles() {
    let (_dir, root, file) = order_project();
    let mut h = harness(&root, vec![file]);
    h.run_steps(3);
    h.get_by_label("Main menu").click();
    h.run_steps(2);
    h.get_by_label("Edit").hover();
    h.run_steps(3);
    assert!(h.query_by_label("Undo").is_some());
    assert!(h.query_by_label("Find in Files…").is_some());
    // Reopened: egui keeps a submenu open while the pointer seems to head
    // into it, so going straight from one submenu to the next is flaky.
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    h.get_by_label("Main menu").click();
    h.run_steps(2);
    h.get_by_label("View").hover();
    h.run_steps(3);
    // Tool windows are check items, read as checkboxes (the side bar has a
    // button of the same name).
    let structure = h.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Structure");
    let shown = h
        .state()
        .tw
        .is_visible(crate::toolwindows::ToolWindow::Structure);
    structure.click();
    h.run_steps(2);
    assert_ne!(
        h.state()
            .tw
            .is_visible(crate::toolwindows::ToolWindow::Structure),
        shown
    );
}

#[test]
fn project_options_menu_toggles_settings_and_stays_open() {
    let (_dir, root) = project();
    let mut h = harness(&root, Vec::new());
    h.run_steps(3);
    let before = h.state().settings.tree_indent_guides;
    h.get_by_label("Options").click();
    h.run_steps(2);
    h.get_by_label("Show Indent Guides").click();
    h.run_steps(2);
    assert_ne!(h.state().settings.tree_indent_guides, before);
    // Several settings can be changed in a row.
    assert!(h.query_by_label("Show File Size and Date").is_some());
    h.get_by_label("By Type").click();
    h.run_steps(2);
    assert_eq!(h.state().settings.tree_sort, "type");
}

/// Screenshots of the menus in the three themes, for review:
/// `cargo test -p phpide-ui phase10_menus -- --ignored`, written to
/// `target/snapshots/phase10-*.png`.
#[test]
#[ignore]
fn phase10_menus() {
    use crate::design::{install, snapshot, Density, Theme, ThemeId};

    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/snapshots");
    std::fs::create_dir_all(&out).unwrap();
    // Renders the harness's own context, where the menu is open.
    let shoot = |h: &mut Harness<'static, PhpIdeApp>, name: &str| {
        for (theme, label) in [
            (ThemeId::Dark, "dark"),
            (ThemeId::Light, "light"),
            (ThemeId::HighContrast, "high-contrast"),
        ] {
            let ctx = h.ctx.clone();
            install(&ctx, Theme::new(theme, Density::Comfortable));
            h.state_mut().editors.clear_layouts();
            let image = snapshot::render(&ctx, egui::vec2(1280.0, 800.0), 12, |ui| {
                h.state_mut().frame_ui(ui)
            });
            image.write_png(&out.join(format!("phase10-{name}-{label}.png")));
        }
        install(&h.ctx, Theme::default());
        h.state_mut().editors.clear_layouts();
    };

    let (_dir, root, file) = order_project();
    let mut h = harness(&root, vec![file.clone()]);
    h.run_steps(3);
    h.state_mut().use_fixture_lsp();
    step_until(&mut h, "the server", server_running);
    let rect = editor_rect(&h);

    right_click_at(&mut h, rect.center());
    shoot(&mut h, "editor");
    h.key_press(egui::Key::Escape);
    h.run_steps(2);

    let gutter = gutter_line_1(&h);
    right_click_at(&mut h, gutter);
    shoot(&mut h, "gutter");
    h.key_press(egui::Key::Escape);
    h.run_steps(2);

    // The file is in the tree and in a tab.
    step_until(&mut h, "the tree", |h| {
        h.query_all_by_label("Order.php").count() >= 2
    });
    let row = h
        .get_all_by_label("Order.php")
        .map(|n| n.rect())
        // Not the tab (top) nor the navigation bar (bottom): the tree row.
        .find(|r| r.top() > 100.0 && r.bottom() < 700.0)
        .unwrap();
    right_click_at(&mut h, row.center());
    shoot(&mut h, "project-tree");
    h.key_press(egui::Key::Escape);
    h.run_steps(2);

    h.get_by_label("Options").click();
    h.run_steps(2);
    shoot(&mut h, "project-options");
    h.key_press(egui::Key::Escape);
    h.run_steps(2);

    h.get_by_label("Main menu").click();
    h.run_steps(2);
    shoot(&mut h, "main-menu");
}

/// A right click elsewhere while the menu is open moves the menu there, as
/// in any desktop application (Escape closes it).
#[test]
fn a_second_right_click_reopens_the_menu_where_it_landed() {
    let (_dir, root, file) = order_project();
    let mut h = harness(&root, vec![file]);
    h.run_steps(3);
    let rect = editor_rect(&h);
    right_click_at(&mut h, rect.center());
    assert!(h.query_by_label("Paste").is_some());
    let gutter = gutter_line_1(&h);
    right_click_at(&mut h, gutter);
    assert!(
        h.query_by_label("Toggle Bookmark at Line 1").is_some(),
        "the gutter menu replaced the text menu"
    );
    assert!(h.query_by_label("Paste").is_none());
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(
        h.query_by_label("Toggle Bookmark at Line 1").is_none(),
        "Escape closes the menu"
    );
}
