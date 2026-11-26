//! Markdown editor gate: the preview renders every kind of block, a local
//! image, links that open files and anchors, and follows the editor.

use std::path::PathBuf;

use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use crate::ui_tests::harness;
use crate::PhpIdeApp;

const SAMPLE: &str = r#"# PHP IDE

A **fast** editor for _PHP_ with ~~no~~ `zero` setup. See [the guide](docs/guide.md) or [usage](#usage).

![logo](logo.png)

## Usage

> Open a folder and start typing.
> It **just works**.

1. Install
2. Run `phpide`
   - nested item

- [x] Minimap
- [ ] Markdown preview

```php
<?php
function greet(string $name): string
{
    return "Hello, $name";
}
```

| Shortcut | Action |
|:---------|-------:|
| Ctrl+B   | Bold   |
| Ctrl+I   | Italic |

---

### End
"#;

fn project() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let file = root.join("README.md");
    let mut text = SAMPLE.to_string();
    for i in 0..60 {
        text.push_str(&format!("\nParagraph {i} of filler text.\n"));
    }
    std::fs::write(&file, text).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/guide.md"), "# Guide\n\n## Install\n").unwrap();
    let logo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/linux/icons/64x64/io.github.tavaresgerson.PhpIde.png");
    std::fs::copy(logo, root.join("logo.png")).unwrap();
    (dir, root, file)
}

fn open(h: &mut Harness<'static, PhpIdeApp>) {
    h.run_steps(4);
    assert!(h.query_by_label("Markdown Preview").is_some());
}

#[test]
fn the_preview_renders_blocks_images_and_links() {
    let (_dir, root, file) = project();
    let mut h = harness(&root, vec![file]);
    open(&mut h);
    // The local image became a texture (no "[image: …]" fallback).
    assert!(h.query_by_label_contains("[image:").is_none());
    for text in ["Install", "Shortcut", "Action", "nested item"] {
        assert!(
            h.query_by_label_contains(text).is_some(),
            "{text} is in the preview"
        );
    }
    assert_eq!(
        h.query_all_by_role(egui::accesskit::Role::CheckBox).count(),
        2
    );
    // A link to another Markdown file opens it in the IDE.
    let dir = root.clone();
    let index = h.state().ws.buffers.len();
    h.state_mut()
        .markdown_open_link(&dir, "docs/guide.md".into());
    h.run_steps(3);
    assert_eq!(h.state().ws.buffers.len(), index + 1);
    assert_eq!(
        h.state().ws.active_buffer().unwrap().path.as_deref(),
        Some(root.join("docs/guide.md").as_path())
    );
}

#[test]
fn the_preview_follows_the_editor() {
    let (_dir, root, file) = project();
    let mut h = harness(&root, vec![file]);
    open(&mut h);
    let offset = |h: &Harness<'static, PhpIdeApp>| {
        let id = h.state().ws.buffers[0].id;
        h.state().markdown.preview_offset(id)
    };
    assert_eq!(offset(&h), 0.0);
    // The editor scrolls to the end: so does the preview.
    let end = h.state().ws.buffers[0].document.len_chars();
    {
        let app = h.state_mut();
        app.editors.set_caret(&mut app.ws, end);
    }
    h.run_steps(6);
    assert!(offset(&h) > 300.0, "the preview followed: {}", offset(&h));
}

/// Review images of the Markdown editor (`target/snapshots/markdown-*.png`):
/// `cargo test -p phpide-ui -- --ignored markdown_screenshots`.
#[test]
#[ignore = "writes review images"]
fn markdown_screenshots() {
    use crate::design::{install, snapshot, Density, Theme, ThemeId};
    let (_dir, root, file) = project();
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/snapshots");
    std::fs::create_dir_all(&out).unwrap();
    let mut h = harness(&root, vec![file]);
    open(&mut h);
    for (theme, label) in [(ThemeId::Dark, "dark"), (ThemeId::Light, "light")] {
        let ctx = h.ctx.clone();
        install(&ctx, Theme::new(theme, Density::Comfortable));
        h.run_steps(3);
        h.state_mut().editors.clear_layouts();
        // The image is loaded again in the rendered frames.
        h.state_mut().markdown = Default::default();
        let image = snapshot::render(&ctx, egui::vec2(1280.0, 800.0), 12, |ui| {
            h.state_mut().frame_ui(ui)
        });
        image.write_png(&out.join(format!("markdown-split-{label}.png")));
    }
}
