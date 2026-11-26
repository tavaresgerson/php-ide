//! Phase 12 gate: the minimap in the app. It is drawn for a long file, a
//! click scrolls the editor there, View › Minimap turns it off, and its
//! settings are in Settings › Editor › General.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use crate::commands::Command;
use crate::editor_area::Pane;
use crate::ui_tests::harness;
use crate::PhpIdeApp;

fn long_file() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let mut text = String::from("<?php\n\nnamespace App;\n\nclass Ledger\n{\n");
    for i in 0..1500 {
        text.push_str(&format!(
            "    public function entry{i}(int $amount): int\n    {{\n        return $amount * {i};\n    }}\n"
        ));
    }
    text.push_str("}\n");
    let file = root.join("Ledger.php");
    std::fs::write(&file, text).unwrap();
    (dir, root, file)
}

fn editor_rect(h: &Harness<'static, PhpIdeApp>) -> egui::Rect {
    h.get_by_label("Editor").rect()
}

fn minimap_idle(h: &mut Harness<'static, PhpIdeApp>) -> bool {
    let app = h.state_mut();
    let index = app.ws.active;
    let (view, _) = app
        .editors
        .pane_parts(Pane::Primary, &app.ws.buffers[index]);
    view.minimap_idle()
}

fn settle(h: &mut Harness<'static, PhpIdeApp>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        h.step();
        if minimap_idle(h) {
            h.run_steps(2);
            return;
        }
        assert!(Instant::now() < deadline, "the minimap was not drawn");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn click_at(h: &mut Harness<'static, PhpIdeApp>, pos: egui::Pos2) {
    h.event(egui::Event::PointerMoved(pos));
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        });
        h.run_steps(1);
    }
    h.run_steps(1);
}

fn scroll_y(h: &mut Harness<'static, PhpIdeApp>) -> f32 {
    let app = h.state_mut();
    let index = app.ws.active;
    let (view, _) = app
        .editors
        .pane_parts(Pane::Primary, &app.ws.buffers[index]);
    view.scroll.y
}

#[test]
fn the_minimap_scrolls_the_editor_and_can_be_turned_off() {
    let (_dir, root, file) = long_file();
    let mut h = harness(&root, vec![file]);
    h.run_steps(3);
    assert!(h.state().settings.minimap, "on by default");
    settle(&mut h);
    assert_eq!(scroll_y(&mut h), 0.0);

    // The minimap is the 100 points left of the scrollbar; at the top of
    // the file it shows line N at 2N points.
    let rect = editor_rect(&h);
    let row = h
        .state()
        .editors
        .caret_rect
        .map(|r| r.height() + 4.0)
        .expect("caret on line 1");
    let target = egui::pos2(rect.right() - 12.0 - 50.0, rect.top() + 400.0);
    click_at(&mut h, target);
    let scrolled = scroll_y(&mut h);
    assert!(scrolled > 0.0, "the click scrolled the editor");
    // Line 200 is in the middle of the editor now.
    let middle = (scrolled + rect.height() * 0.5) / row;
    assert!((middle - 200.0).abs() < 3.0, "line {middle} in the middle");
    let caret_line_before = h.state().cursor_char;
    assert_eq!(caret_line_before, 0, "the click does not move the caret");

    // View › Minimap turns it off: the same click lands in the text.
    let ctx = h.ctx.clone();
    h.state_mut().dispatch_command(Command::ToggleMinimap, &ctx);
    h.run_steps(2);
    assert!(!h.state().settings.minimap);
    let before = scroll_y(&mut h);
    click_at(&mut h, target);
    assert_eq!(scroll_y(&mut h), before, "no minimap: no scroll");
    assert!(
        h.state().cursor_char > 0,
        "the click put the caret in the text"
    );
}

#[test]
fn minimap_settings_are_in_editor_appearance() {
    let (_dir, root, file) = long_file();
    let mut h = harness(&root, vec![file]);
    h.run_steps(2);
    h.state_mut()
        .settings_dialog
        .open(crate::settings_dialog::SettingsPage::EditorAppearance);
    h.run_steps(3);
    for label in [
        "Show minimap",
        "Narrow",
        "Normal",
        "Wide",
        "Characters",
        "Blocks",
        "Always",
        "On hover",
    ] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    h.get_by_label("Wide").click();
    h.get_by_label("Blocks").click();
    h.get_by_label("On hover").click();
    h.run_steps(2);
    let options = h.state().settings.minimap_options();
    assert_eq!(options.width, 140.0);
    assert_eq!(options.render, phpide_editor::MinimapRender::Blocks);
    assert_eq!(options.slider, phpide_editor::MinimapSlider::Hover);
    h.get_by_label("Show minimap").click();
    h.run_steps(2);
    assert!(!h.state().settings.minimap);
}

/// Review images of the minimap in the three themes
/// (`target/snapshots/phase12-*.png`):
/// `cargo test -p phpide-ui -- --ignored phase12`.
#[test]
#[ignore = "writes review images"]
fn phase12_screenshots() {
    use crate::design::{install, snapshot, Density, Theme, ThemeId};
    let (_dir, root, file) = long_file();
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/snapshots");
    std::fs::create_dir_all(&out).unwrap();
    let mut h = harness(&root, vec![file]);
    h.run_steps(3);
    settle(&mut h);
    // Some marks: a selection and a search.
    {
        let app = h.state_mut();
        app.editors.select(&mut app.ws, 300, 900);
    }
    h.run_steps(2);
    for (theme, label) in [
        (ThemeId::Dark, "dark"),
        (ThemeId::Light, "light"),
        (ThemeId::HighContrast, "high-contrast"),
    ] {
        for (render, name) in [("characters", "characters"), ("blocks", "blocks")] {
            h.state_mut().settings.minimap_render = render.into();
            let ctx = h.ctx.clone();
            install(&ctx, Theme::new(theme, Density::Comfortable));
            h.run_steps(1);
            settle(&mut h);
            h.state_mut().editors.clear_layouts();
            let image = snapshot::render(&ctx, egui::vec2(1280.0, 800.0), 12, |ui| {
                h.state_mut().frame_ui(ui)
            });
            image.write_png(&out.join(format!("phase12-minimap-{name}-{label}.png")));
        }
    }
}
