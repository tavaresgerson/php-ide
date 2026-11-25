//! Phase 7 gate: Git through the UI, against the real `git`, a bare remote
//! and a pre-commit hook. Commit with one hunk staged from the diff tab,
//! hook output in the Commit window, push, update, a new branch from the
//! branch popup, the log with a commit's diff, rollback and a merge
//! conflict. Skips itself without Git.

use std::path::{Path, PathBuf};

use eframe::egui;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;

use crate::services::vcs::{ConflictSide, QuickOp, VcsStatus};
use crate::toolwindows::ToolWindow;
use crate::ui_tests::{harness, step_until};
use crate::PhpIdeApp;

fn have_git() -> bool {
    std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn configure(dir: &Path) {
    git(dir, &["config", "user.name", "Ana"]);
    git(dir, &["config", "user.email", "ana@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    git(dir, &["config", "pull.rebase", "false"]);
}

fn lines(n: usize) -> String {
    (1..=n).map(|i| format!("line {i}\n")).collect()
}

/// `work/` (the project, pushed to `remote.git`) and `other/`, a second
/// clone that pushes on its own.
struct Repos {
    _dir: tempfile::TempDir,
    root: PathBuf,
    remote: PathBuf,
    other: PathBuf,
}

fn repos() -> Repos {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().to_path_buf();
    let root = base.join("work");
    let remote = base.join("remote.git");
    let other = base.join("other");
    std::fs::create_dir_all(&root).unwrap();
    git(
        &base,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    git(&root, &["init", "-q", "-b", "main"]);
    configure(&root);
    git(
        &root,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    std::fs::write(root.join("a.php"), lines(30)).unwrap();
    std::fs::write(root.join("composer.json"), "{}").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "Initial import"]);
    git(&root, &["push", "-q", "-u", "origin", "main"]);
    git(
        &base,
        &[
            "clone",
            "-q",
            remote.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    configure(&other);
    // A hook that prints, like a linter.
    let hook = root.join(".git/hooks/pre-commit");
    std::fs::write(
        &hook,
        "#!/bin/sh\necho \"lint: 2 files checked\"\nif git diff --cached | grep -q FORBIDDEN; then echo \"lint: FORBIDDEN is not allowed\" >&2; exit 1; fi\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    Repos {
        _dir: dir,
        root,
        remote,
        other,
    }
}

fn wait(h: &mut Harness<'static, PhpIdeApp>, what: &str, done: impl Fn(&PhpIdeApp) -> bool) {
    step_until(h, what, |h| done(h.state()));
}

/// The Git service is idle and the snapshot is fresh.
fn settled(app: &PhpIdeApp) -> bool {
    !app.vcs.busy() && app.vcs.snapshot().is_some()
}

/// The Commit button (the tool window stripe has a "Commit" button too).
fn click_commit(h: &mut Harness<'static, PhpIdeApp>) {
    let node = h
        .get_all_by_label("Commit")
        .find(|n| {
            n.accesskit_node().role() == egui::accesskit::Role::Button && n.rect().width() > 50.0
        })
        .expect("the Commit button");
    node.click();
    h.run_steps(2);
}

fn console_tail(app: &PhpIdeApp) -> String {
    app.vcs
        .console
        .last()
        .map(|e| {
            e.lines
                .iter()
                .map(|(l, _)| l.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

#[test]
fn commit_a_hunk_push_update_branch_log_and_rollback() {
    if !have_git() {
        eprintln!("GATE SKIP: git is not installed");
        return;
    }
    let r = repos();
    let root = &r.root;
    // Two changes far apart, and a new file.
    let changed = lines(30)
        .replace("line 2\n", "line 2 changed\n")
        .replace("line 25\n", "line 25\nadded after 25\n");
    std::fs::write(root.join("a.php"), &changed).unwrap();
    std::fs::write(root.join("new.php"), "<?php\necho 'new';\n").unwrap();
    let mut h = harness(root, Vec::new());
    h.state_mut().tw.set_visible(ToolWindow::Project, false);
    h.run_steps(3);
    wait(&mut h, "the repository", |app| {
        app.vcs.snapshot().is_some_and(|s| s.changes.len() == 2)
    });
    let top = h.state().vcs.toplevel().unwrap().to_path_buf();
    // Names are colored by status (Project view and tabs read this).
    assert_eq!(
        h.state().vcs.status_of(&top.join("a.php")),
        Some(VcsStatus::Modified)
    );
    assert_eq!(
        h.state().vcs.status_of(&top.join("new.php")),
        Some(VcsStatus::Untracked)
    );

    // Ctrl+K: the Commit window opens with the message focused.
    h.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::K);
    h.run_steps(3);
    assert!(h.state().tw.is_visible(ToolWindow::Commit));
    assert!(h.query_by_label_contains("Changes").is_some());
    assert!(h.query_by_label_contains("Unversioned Files").is_some());

    // Double click a.php: its diff opens with both changes.
    h.get_by_label("a.php").click();
    h.run_steps(1);
    h.get_by_label("a.php").click();
    h.run_steps(1);
    wait(&mut h, "the diff", |app| {
        app.diffs
            .active_tab()
            .is_some_and(|t| t.data.as_ref().is_some_and(|d| d.is_ok()))
    });
    h.run_steps(2);
    assert_eq!(h.state().diffs.active_tab().unwrap().change_count(), 2);
    assert!(h.query_by_label_contains("2 differences").is_some());
    // Stage only the second change, from the diff tab.
    let stage_buttons: Vec<_> = h.get_all_by_label("Stage").collect();
    assert_eq!(stage_buttons.len(), 2);
    stage_buttons[1].click();
    h.run_steps(2);
    wait(&mut h, "the staged hunk", |app| {
        settled(app)
            && app.vcs.snapshot().is_some_and(|s| {
                s.changes.iter().any(|c| {
                    c.path == Path::new("a.php") && c.staged.is_some() && c.unstaged.is_some()
                })
            })
    });
    // The diff reloaded: one change left to stage.
    wait(&mut h, "the reloaded diff", |app| {
        app.diffs
            .active_tab()
            .is_some_and(|t| t.change_count() == 1)
    });

    // Stage new.php with its row button, then commit.
    h.get_by_label("new.php").click();
    h.run_steps(2);
    h.get_by_label("Stage new.php").click();
    h.run_steps(2);
    wait(&mut h, "new.php staged", |app| {
        settled(app)
            && app.vcs.snapshot().is_some_and(|s| {
                s.changes
                    .iter()
                    .any(|c| c.path == Path::new("new.php") && c.staged.is_some())
            })
    });
    h.state_mut().commit_ui.focus_message = true;
    h.run_steps(2);
    for c in "Add the new page".chars() {
        h.event(egui::Event::Text(c.to_string()));
    }
    h.run_steps(2);
    click_commit(&mut h);
    h.run_steps(2);
    wait(&mut h, "the commit", |app| {
        settled(app) && app.vcs.console.last().is_some_and(|e| e.exit.is_some())
    });
    h.run_steps(3);
    assert!(
        h.state().vcs.console.last().unwrap().succeeded(),
        "{}",
        console_tail(h.state())
    );
    // The hook's output shows under the Commit button; the message is reset.
    assert!(h.query_by_label("lint: 2 files checked").is_some());
    assert!(h.state().commit_ui.message.is_empty());
    let head = git(root, &["log", "-1", "--format=%s"]);
    assert_eq!(head.trim(), "Add the new page");
    let committed = git(root, &["show", "HEAD:a.php"]);
    assert!(committed.contains("added after 25") && !committed.contains("line 2 changed"));

    // A failing hook: the commit is refused, the output says why, the
    // staged change stays.
    std::fs::write(root.join("b.php"), "FORBIDDEN\n").unwrap();
    git(root, &["add", "b.php"]);
    h.state_mut().vcs.refresh();
    wait(&mut h, "b.php staged", |app| {
        settled(app)
            && app
                .vcs
                .snapshot()
                .is_some_and(|s| s.changes.iter().any(|c| c.path == Path::new("b.php")))
    });
    h.state_mut().commit_ui.message = "Forbidden".into();
    h.run_steps(2);
    click_commit(&mut h);
    wait(&mut h, "the refused commit", |app| {
        settled(app)
            && app
                .vcs
                .console
                .last()
                .is_some_and(|e| e.exit.is_some() && !e.succeeded())
    });
    h.run_steps(3);
    assert!(h.query_by_label("lint: FORBIDDEN is not allowed").is_some());
    assert!(
        h.state().ws.message.contains("failed"),
        "{}",
        h.state().ws.message
    );
    assert_eq!(h.state().commit_ui.message, "Forbidden");
    git(root, &["rm", "-q", "--cached", "b.php"]);
    std::fs::remove_file(root.join("b.php")).unwrap();

    // Ctrl+Shift+K pushes; the remote has the commit.
    h.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::K);
    wait(&mut h, "the push", |app| {
        settled(app)
            && app
                .vcs
                .console
                .last()
                .is_some_and(|e| e.title.starts_with("Pushing") && e.exit.is_some())
    });
    assert!(
        h.state().vcs.console.last().unwrap().succeeded(),
        "{}",
        console_tail(h.state())
    );
    assert_eq!(
        git(&r.remote, &["log", "-1", "--format=%s", "main"]).trim(),
        "Add the new page"
    );

    // The other clone pushes; Ctrl+T brings its commit.
    git(&r.other, &["pull", "-q"]);
    std::fs::write(r.other.join("c.php"), "<?php // from the other clone\n").unwrap();
    git(&r.other, &["add", "-A"]);
    git(&r.other, &["commit", "-q", "-m", "From the other clone"]);
    git(&r.other, &["push", "-q"]);
    h.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::T);
    wait(&mut h, "the update", |app| {
        settled(app)
            && app
                .vcs
                .console
                .last()
                .is_some_and(|e| e.title.starts_with("Updating") && e.exit.is_some())
    });
    assert!(
        h.state().vcs.console.last().unwrap().succeeded(),
        "{}",
        console_tail(h.state())
    );
    assert!(root.join("c.php").exists());

    // Branch popup › New Branch…: created and checked out.
    let ctx = h.ctx.clone();
    h.state_mut()
        .dispatch_command(crate::commands::Command::GitBranches, &ctx);
    h.run_steps(3);
    assert!(h.query_by_label("Update Project…").is_some());
    h.get_by_label("New Branch…").click();
    h.run_steps(3);
    for c in "feature/login".chars() {
        h.event(egui::Event::Text(c.to_string()));
    }
    h.run_steps(1);
    h.get_by_label("Create").click();
    wait(&mut h, "the new branch", |app| {
        settled(app)
            && app
                .vcs
                .snapshot()
                .and_then(|s| s.current_branch().map(|b| b.name == "feature/login"))
                .unwrap_or(false)
    });

    // The log: the commit, its files, and the diff of one of them.
    h.state_mut()
        .tw
        .set_visible(ToolWindow::VersionControl, true);
    h.state_mut().tw.set_visible(ToolWindow::Commit, false);
    wait(&mut h, "the log", |app| app.vcs_log.entries().len() >= 3);
    h.run_steps(3);
    h.get_by_label("Add the new page").click();
    wait(&mut h, "the commit details", |app| {
        app.vcs_log.details().is_some_and(|d| d.files.len() == 2)
    });
    h.run_steps(3);
    h.get_by_label("new.php").click();
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    wait(&mut h, "the commit's diff", |app| {
        app.diffs.active_tab().is_some_and(|t| {
            t.title.starts_with("new.php (") && t.data.as_ref().is_some_and(|d| d.is_ok())
        })
    });
    let tab = h.state().diffs.active_tab().unwrap();
    assert_eq!(tab.change_count(), 1);
    // Ctrl+F4 closes the diff tab in front.
    let tabs = h.state().diffs.tabs.len();
    h.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::F4);
    h.run_steps(2);
    assert_eq!(h.state().diffs.tabs.len(), tabs - 1);

    // Rollback of the open file (Ctrl+Alt+Z) after confirming.
    let a = root.join("a.php");
    h.state_mut().diffs.active = None;
    h.state_mut().open_file_tab(&a, false);
    h.run_steps(2);
    std::fs::write(&a, "broken\n").unwrap();
    h.state_mut().vcs.refresh();
    wait(&mut h, "a.php modified", |app| {
        settled(app) && app.vcs.status_of(&a) == Some(VcsStatus::Modified)
    });
    h.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::ALT, egui::Key::Z);
    h.run_steps(2);
    assert!(h.query_by_label_contains("will be lost").is_some());
    h.get_by_label("Rollback").click();
    wait(&mut h, "the rollback", |app| {
        settled(app) && app.vcs.status_of(&a).is_none()
    });
    // Back to HEAD: the committed hunk, without the edit.
    let text = std::fs::read_to_string(&a).unwrap();
    assert!(
        text.contains("added after 25") && !text.contains("broken"),
        "{text}"
    );
    // The open buffer reloaded from disk.
    h.run_steps(3);
    assert_eq!(h.state().ws.active_buffer().unwrap().document.text(), text);
}

#[test]
fn merge_conflict_shows_and_resolves_in_the_commit_window() {
    if !have_git() {
        eprintln!("GATE SKIP: git is not installed");
        return;
    }
    let r = repos();
    let root = &r.root;
    git(root, &["switch", "-q", "-c", "feature"]);
    std::fs::write(root.join("a.php"), "feature side\n").unwrap();
    git(root, &["commit", "-q", "-am", "Feature side"]);
    git(root, &["switch", "-q", "main"]);
    std::fs::write(root.join("a.php"), "main side\n").unwrap();
    git(root, &["commit", "-q", "-am", "Main side"]);
    let mut h = harness(root, Vec::new());
    h.run_steps(3);
    wait(&mut h, "the repository", settled);
    h.state_mut().git_merge("feature");
    wait(&mut h, "the merge", |app| {
        settled(app) && app.vcs.snapshot().is_some_and(|s| s.operation.is_some())
    });
    assert!(
        h.state().ws.message.contains("failed"),
        "{}",
        h.state().ws.message
    );
    h.state_mut().tw.set_visible(ToolWindow::Commit, true);
    h.run_steps(3);
    assert!(h.query_by_label_contains("Merge Conflicts").is_some());
    assert!(h.query_by_label_contains("resolve the conflicts").is_some());
    let a = h.state().vcs.toplevel().unwrap().join("a.php");
    assert_eq!(h.state().vcs.status_of(&a), Some(VcsStatus::Conflict));

    h.state_mut().git_quick(QuickOp::Accept {
        path: "a.php".into(),
        side: ConflictSide::Theirs,
    });
    wait(&mut h, "the resolution", |app| {
        settled(app)
            && app
                .vcs
                .snapshot()
                .is_some_and(|s| !s.changes.iter().any(|c| c.conflict))
    });
    h.run_steps(3);
    assert!(h
        .query_by_label_contains("commit to conclude the merge")
        .is_some());
    h.state_mut().commit_ui.message = "Merge feature".into();
    h.run_steps(2);
    click_commit(&mut h);
    wait(&mut h, "the merge commit", |app| {
        settled(app) && app.vcs.snapshot().is_some_and(|s| s.operation.is_none())
    });
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "feature side\n");
    let parents = git(root, &["log", "-1", "--format=%P"]);
    assert_eq!(parents.split_whitespace().count(), 2);
}

/// Writes the Phase 7 screenshots to `target/snapshots` in the three
/// themes: `cargo test -p phpide-ui phase7_screenshots -- --ignored`.
#[test]
#[ignore]
fn phase7_screenshots() {
    use crate::design::{install, snapshot, Density, Theme, ThemeId};
    if !have_git() {
        return;
    }
    let r = repos();
    let root = &r.root;
    std::fs::create_dir_all(root.join("src/Http")).unwrap();
    let controller = "<?php\n\nnamespace App\\Http;\n\nfinal class UserController\n{\n    public function index(): array\n    {\n        return $this->users->all();\n    }\n\n    public function show(int $id): ?User\n    {\n        return $this->users->find($id);\n    }\n}\n";
    std::fs::write(root.join("src/Http/UserController.php"), controller).unwrap();
    std::fs::write(root.join("README.md"), "# Shop\n").unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "Add the user controller"]);
    git(root, &["switch", "-q", "-c", "feature/search"]);
    std::fs::write(root.join("src/Http/Search.php"), "<?php\n// search\n").unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "Search endpoint"]);
    git(root, &["switch", "-q", "main"]);
    std::fs::write(root.join("README.md"), "# Shop\n\nOnline shop.\n").unwrap();
    git(root, &["commit", "-q", "-am", "Describe the shop"]);
    git(root, &["merge", "-q", "--no-edit", "feature/search"]);
    git(root, &["tag", "v1.0"]);
    // Working changes of every kind.
    let edited = controller
        .replace(
            "return $this->users->all();",
            "return $this->users->active()->all();",
        )
        .replace(
            "    public function show",
            "    /** One user. */\n    public function show",
        );
    std::fs::write(root.join("src/Http/UserController.php"), edited).unwrap();
    std::fs::write(root.join("src/Http/Orders.php"), "<?php\n// orders\n").unwrap();
    git(root, &["add", "src/Http/Orders.php"]);
    std::fs::remove_file(root.join("README.md")).unwrap();
    std::fs::write(root.join("notes.txt"), "todo\n").unwrap();

    let mut h = harness(root, Vec::new());
    h.run_steps(3);
    wait(&mut h, "the repository", |app| {
        settled(app) && app.vcs.snapshot().is_some_and(|s| s.changes.len() == 4)
    });
    {
        let app = h.state_mut();
        app.tw.set_visible(ToolWindow::Commit, true);
        app.commit_ui.message = "Show only active users".into();
        let ctx = h.ctx.clone();
        let app = h.state_mut();
        app.open_diff(
            &ctx,
            crate::services::vcs::DiffRequest {
                path: "src/Http/UserController.php".into(),
                old_path: None,
                from: crate::services::vcs::Side::Index,
                to: crate::services::vcs::Side::WorkTree,
            },
            "UserController.php".into(),
        );
    }
    wait(&mut h, "the diff", |app| {
        app.diffs.active_tab().is_some_and(|t| t.data.is_some())
    });
    h.run_steps(3);
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/snapshots");
    std::fs::create_dir_all(&out).unwrap();
    let shoot = |h: &mut Harness<'static, PhpIdeApp>, name: &str, popup: bool| {
        for (theme, label) in [
            (ThemeId::Dark, "dark"),
            (ThemeId::Light, "light"),
            (ThemeId::HighContrast, "high-contrast"),
        ] {
            let ctx = egui::Context::default();
            install(&ctx, Theme::new(theme, Density::Comfortable));
            if popup {
                egui::Popup::open_id(&ctx, crate::header::branch_popup());
            }
            h.state_mut().editors.clear_layouts();
            let image = snapshot::render(&ctx, egui::vec2(1280.0, 800.0), 12, |ui| {
                h.state_mut().frame_ui(ui)
            });
            image.write_png(&out.join(format!("phase7-{name}-{label}.png")));
        }
        h.state_mut().editors.clear_layouts();
    };
    shoot(&mut h, "commit-diff", false);

    // Project view with Git colors and the log.
    {
        let app = h.state_mut();
        app.diffs.active = None;
        app.tw.set_visible(ToolWindow::Commit, false);
        app.tw.set_visible(ToolWindow::Project, true);
        app.tw.set_visible(ToolWindow::VersionControl, true);
        let ctx = h.ctx.clone();
        let app = h.state_mut();
        app.files
            .reveal(&app.jobs, &ctx, &root.join("src/Http/UserController.php"));
    }
    wait(&mut h, "the log", |app| app.vcs_log.entries().len() >= 5);
    h.run_steps(3);
    h.get_by_label("Merge branch 'feature/search'").click();
    wait(&mut h, "the details", |app| app.vcs_log.details().is_some());
    h.run_steps(5);
    shoot(&mut h, "log", false);

    // The branch popup.
    shoot(&mut h, "branches", true);
}
