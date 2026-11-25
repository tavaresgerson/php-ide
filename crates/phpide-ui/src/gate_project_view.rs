//! Phase 8 gate for the PhpStorm parity items of the Project view, the tab
//! strip and Home: folder marks, file nesting and its rules, size and date
//! details, Quick Preview with Space, scope views, the tab settings,
//! project groups, project icons and the default project directory.

use eframe::egui;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;

use crate::project_tree::Scope;
use crate::ui_tests::{harness, project, step_until};
use crate::PhpIdeApp;

fn rows(h: &Harness<'static, PhpIdeApp>) -> Vec<String> {
    let app = h.state();
    let options = crate::project_tree::tree_options(app);
    app.files
        .visible_rows(&options)
        .iter()
        .map(|r| format!("{}{}", "  ".repeat(r.depth), r.entry.name))
        .collect()
}

fn with_tests_folder() -> (tempfile::TempDir, std::path::PathBuf) {
    let (dir, root) = project();
    std::fs::create_dir_all(root.join("tests")).unwrap();
    std::fs::write(root.join("tests/AppTest.php"), "<?php\n").unwrap();
    std::fs::write(root.join("composer.lock"), "{}").unwrap();
    (dir, root)
}

#[test]
fn folders_are_marked_from_the_context_menu_and_excluded_ones_leave_the_index() {
    let (_dir, root) = with_tests_folder();
    let mut h = harness(&root, Vec::new());
    step_until(&mut h, "the tree", |h| h.query_by_label("tests").is_some());
    step_until(&mut h, "the index", |h| {
        h.state()
            .ws
            .files
            .iter()
            .any(|f| f.path.ends_with("tests/AppTest.php"))
    });

    h.get_by_label("tests").click_secondary();
    h.run_steps(2);
    h.get_by_label_contains("Mark Directory as").hover();
    h.run_steps(3);
    h.get_by_label("Test Sources Root").click();
    h.run_steps(2);
    let marks = &h.state().project_marks;
    assert_eq!(marks.tests, vec![root.join("tests")]);
    let saved = std::fs::read_to_string(root.join(".phpide/project.json")).unwrap();
    assert!(saved.contains("tests"), "{saved}");

    // Excluding a folder drops its files from the index (Go to File etc).
    h.get_by_label("vendor").click_secondary();
    h.run_steps(2);
    h.get_by_label_contains("Mark Directory as").hover();
    h.run_steps(3);
    h.get_by_label("Excluded").click();
    step_until(&mut h, "vendor out of the index", |h| {
        let files = &h.state().ws.files;
        !files.is_empty()
            && !files
                .iter()
                .any(|f| f.path.starts_with(root.join("vendor")))
    });
    // The marks come back with the project.
    drop(h);
    let h = harness(&root, Vec::new());
    assert_eq!(h.state().project_marks.excluded, vec![root.join("vendor")]);
}

#[test]
fn files_nest_under_their_parent_and_the_rules_are_editable() {
    let (_dir, root) = with_tests_folder();
    let mut h = harness(&root, Vec::new());
    step_until(&mut h, "the tree", |h| {
        rows(h).contains(&"composer.json".to_string())
    });
    let listed = rows(&h);
    assert!(!listed.contains(&"composer.lock".to_string()), "{listed:?}");

    // The parent row expands like a folder.
    h.get_by_label("composer.json").click();
    h.run_steps(2);
    h.ctx
        .memory_mut(|m| m.request_focus(crate::project_tree::tree_id()));
    h.run_steps(2);
    h.key_press(egui::Key::ArrowRight);
    h.run_steps(2);
    assert!(
        rows(&h).contains(&"  composer.lock".to_string()),
        "{:?}",
        rows(&h)
    );

    // Turning nesting off lists the file on its own.
    h.state_mut().settings.file_nesting = false;
    h.run_steps(2);
    assert!(rows(&h).contains(&"composer.lock".to_string()));
    h.state_mut().settings.file_nesting = true;

    // The rules dialog: clearing the rules and saving stops the nesting.
    h.state_mut().tree.nesting_editor = Some(h.state().settings.nesting_rules.join("\n"));
    h.run_steps(3);
    assert!(h.query_by_label("File Nesting Rules").is_some());
    h.state_mut().tree.nesting_editor = Some("not a rule\n.php=.phpt".into());
    h.run_steps(1);
    h.get_by_label("Save").click();
    h.run_steps(2);
    assert_eq!(
        h.state().settings.nesting_rules,
        vec![".php=.phpt".to_string()]
    );
    assert!(rows(&h).contains(&"composer.lock".to_string()));
    // Reset to Defaults brings composer.lock back under composer.json.
    h.state_mut().tree.nesting_editor = Some(String::new());
    h.run_steps(2);
    h.get_by_label("Reset to Defaults").click();
    h.run_steps(1);
    h.get_by_label("Save").click();
    h.run_steps(2);
    assert_eq!(
        h.state().settings.nesting_rules,
        crate::settings::default_nesting_rules()
    );
}

#[test]
fn size_and_date_details_and_quick_preview() {
    let (_dir, root) = with_tests_folder();
    let mut h = harness(&root, Vec::new());
    h.state_mut().settings.tree_file_details = true;
    step_until(&mut h, "the tree", |h| {
        h.query_by_label("composer.json").is_some()
    });
    let node = h.get_by_label("composer.json");
    let detail = node
        .accesskit_node()
        .value()
        .unwrap_or_default()
        .to_string();
    assert!(detail.contains("2 B"), "{detail}");

    // Space on a file opens Quick Preview; Space again closes it.
    h.get_by_label("src").click();
    step_until(&mut h, "src", |h| h.query_by_label("App.php").is_some());
    h.get_by_label("App.php").click();
    h.run_steps(2);
    // A click opens a preview tab; give focus back to the tree.
    h.ctx
        .memory_mut(|m| m.request_focus(crate::project_tree::tree_id()));
    h.run_steps(2);
    h.event(egui::Event::Text(" ".into()));
    step_until(&mut h, "the preview", |h| {
        h.query_by_label_contains("echo 'app'").is_some()
    });
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(h.state().tree.preview.is_none());
}

#[test]
fn scope_views_list_open_changed_and_test_files() {
    let (_dir, root) = with_tests_folder();
    let file = root.join("src/App.php");
    let mut h = harness(&root, vec![file.clone()]);
    step_until(&mut h, "the tree", |h| h.query_by_label("tests").is_some());

    h.get_by_label("View scope: Project").click();
    h.run_steps(2);
    h.get_by_label("Open Files").click();
    h.run_steps(2);
    assert_eq!(h.state().tree.scope, Scope::OpenFiles);
    let listed = |h: &Harness<'static, PhpIdeApp>| {
        crate::project_tree::scope_files(h.state())
            .iter()
            .map(|p| p.strip_prefix(&root).unwrap().display().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(listed(&h), ["src/App.php"]);
    assert!(
        h.query_by_label("composer.json").is_none(),
        "the tree is replaced"
    );

    // Tests needs a test root; marked through composer or the menu.
    h.state_mut()
        .project_marks
        .set(&root.join("tests"), Some(crate::project_marks::Mark::Tests));
    h.state_mut().tree.scope = Scope::Tests;
    step_until(&mut h, "the test files", |h| {
        h.query_by_label("AppTest.php").is_some()
    });
    assert_eq!(listed(&h), ["tests/AppTest.php"]);

    h.state_mut().tree.scope = Scope::Production;
    h.run_steps(2);
    let production = listed(&h);
    assert!(
        production.contains(&"src/App.php".to_string()),
        "{production:?}"
    );
    assert!(
        !production.iter().any(|p| p.starts_with("tests")),
        "{production:?}"
    );
    assert!(h.query_by_label("AppTest.php").is_none());

    // Not a repository: Changed Files says so.
    h.state_mut().tree.scope = Scope::ChangedFiles;
    step_until(&mut h, "the VCS status", |h| h.state().vcs.not_repo());
    h.run_steps(2);
    assert!(h
        .query_by_label_contains("not in a Git repository")
        .is_some());
}

#[test]
fn tab_settings_close_button_order_and_the_tab_after_close() {
    let (_dir, root) = with_tests_folder();
    let files = vec![
        root.join("src/App.php"),
        root.join("composer.json"),
        root.join("tests/AppTest.php"),
    ];
    let mut h = harness(&root, files);
    h.run_steps(3);
    // Alphabetical order.
    h.state_mut().settings.tabs_alphabetical = true;
    h.run_steps(2);
    let names: Vec<String> = h
        .state()
        .ws
        .buffers
        .iter()
        .map(|b| b.name().to_string())
        .collect();
    let order: Vec<&str> = crate::tabs::display_order(h.state(), &names)
        .into_iter()
        .map(|i| names[i].as_str())
        .collect();
    assert_eq!(order, ["App.php", "AppTest.php", "composer.json"]);

    // No close button.
    h.state_mut().settings.tab_close_button = "None".into();
    h.run_steps(2);
    assert!(h.query_by_label("Close App.php").is_none());
    h.state_mut().settings.tab_close_button = "Left".into();
    h.run_steps(2);
    assert!(h.query_by_label("Close App.php").is_some());

    // Closing the active tab activates the most recently used one.
    h.state_mut().settings.tab_after_close = "Most recently opened".into();
    for i in [0, 2, 1] {
        h.state_mut().ws.active = i;
        h.run_steps(1);
    }
    h.state_mut().ws.close_tab(1);
    h.run_steps(1);
    let active = h.state().ws.active_buffer().unwrap().name().to_string();
    assert_eq!(active, "AppTest.php");

    // Placement None hides the strip.
    h.state_mut().settings.tab_placement = "None".into();
    h.run_steps(2);
    assert!(h.query_by_label("Close App.php").is_none());
}

#[test]
fn home_groups_projects_and_shows_their_icons() {
    let (_dir, root) = project();
    let alpha = root.join("alpha-site");
    let beta = root.join("beta");
    std::fs::create_dir_all(alpha.join(".phpide")).unwrap();
    std::fs::create_dir_all(&beta).unwrap();
    // A 2×2 red PNG as alpha's icon.
    let png = image::RgbaImage::from_pixel(2, 2, image::Rgba([200, 0, 0, 255]));
    png.save(alpha.join(".phpide/icon.png")).unwrap();
    let recents = vec![alpha.display().to_string(), beta.display().to_string()];
    let mut h = crate::ui_tests::home_harness(&root, recents);
    step_until(&mut h, "the icon", |h| {
        h.state().welcome_icon(&alpha.display().to_string())
    });
    assert!(!h.state().welcome_icon(&beta.display().to_string()));

    // Move to Group → New Project Group…
    h.get_by_label("Open beta").click_secondary();
    h.run_steps(2);
    h.get_by_label_contains("Move to Group").hover();
    h.run_steps(3);
    h.get_by_label("New Project Group…").click();
    h.run_steps(2);
    for c in "Work".chars() {
        h.event(egui::Event::Text(c.to_string()));
    }
    h.run_steps(1);
    h.get_by_label("OK").click();
    h.run_steps(2);
    let groups = h.state().settings.project_groups.clone();
    assert_eq!(
        groups,
        vec![("Work".to_string(), vec![beta.display().to_string()])]
    );

    // The group folds.
    assert!(h.query_by_label("Open beta").is_some());
    h.get_by_label("Group Work").click();
    h.run_steps(2);
    assert!(h.query_by_label("Open beta").is_none());
    assert!(h.query_by_label("Open alpha-site").is_some());
    h.get_by_label("Group Work").click();
    h.run_steps(2);

    // Removing the group puts the project back in the plain list.
    h.get_by_label("Group Work").click_secondary();
    h.run_steps(2);
    h.get_by_label("Remove Group").click();
    h.run_steps(2);
    assert!(h.state().settings.project_groups.is_empty());
    assert!(h.query_by_label("Open beta").is_some());
}

#[test]
fn new_projects_go_to_the_default_project_directory() {
    let (_dir, root) = project();
    let mut h = crate::ui_tests::home_harness(&root, Vec::new());
    h.state_mut().settings.projects_dir = root.join("src").display().to_string();
    h.run_steps(2);
    h.get_by_label("New Project").click();
    h.run_steps(2);
    assert_eq!(
        h.state().welcome_location(),
        root.join("src").display().to_string()
    );
    assert_eq!(crate::welcome::initials("php-ide"), "PI");
    assert_eq!(crate::welcome::initials("basis"), "BA");
}
