//! The Phase 7 Git workflow against the real `git`: hunks, rollback,
//! commits with hooks, branches, a bare remote (push, fetch, pull) and a
//! merge conflict. Skips itself when Git is missing.

use std::path::{Path, PathBuf};
use std::process::Command;

use phpide_vcs::commands;
use phpide_vcs::{
    accept_side, apply_hunk, branches, diff_between, file_at, ignored, log, operation, rollback,
    stage_paths, status, unstage_paths, ConflictSide, HunkAction, LogFilter, RepoOperation, Side,
};

fn have_git() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
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

fn configure(root: &Path) {
    git(root, &["config", "user.name", "Ana"]);
    git(root, &["config", "user.email", "ana@example.com"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    git(root, &["config", "pull.rebase", "false"]);
}

fn repo() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    configure(&root);
    (dir, root)
}

fn write(root: &Path, path: &str, text: &str) {
    let file = root.join(path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, text).unwrap();
}

fn read(root: &Path, path: &str) -> String {
    std::fs::read_to_string(root.join(path)).unwrap()
}

fn lines(n: usize) -> String {
    (1..=n).map(|i| format!("line {i}\n")).collect()
}

fn commit_all(root: &Path, message: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", message]);
}

#[test]
fn hunks_stage_unstage_and_revert_one_at_a_time() {
    if !have_git() {
        eprintln!("GATE SKIP: git is not installed");
        return;
    }
    let (_dir, root) = repo();
    write(&root, "a.php", &lines(30));
    commit_all(&root, "base");
    // Two changes far apart: two hunks.
    let changed = lines(30)
        .replace("line 2\n", "line 2 changed\n")
        .replace("line 25\n", "line 25\nadded after 25\n");
    write(&root, "a.php", &changed);
    let path = Path::new("a.php");
    let diff = diff_between(&root, &Side::Index, &Side::WorkTree, path, None)
        .unwrap()
        .unwrap();
    assert_eq!(diff.hunks.len(), 2);

    // Stage only the second hunk.
    apply_hunk(&root, &diff, 1, HunkAction::Stage).unwrap();
    let staged = diff_between(&root, &Side::Head, &Side::Index, path, None)
        .unwrap()
        .unwrap();
    assert_eq!(staged.hunks.len(), 1);
    assert!(staged.hunks[0]
        .lines
        .iter()
        .any(|l| l.content == "added after 25"));
    let index = String::from_utf8(file_at(&root, &Side::Index, path).unwrap().unwrap()).unwrap();
    assert!(index.contains("added after 25") && !index.contains("line 2 changed"));

    // Unstage it again: the index is back to HEAD.
    apply_hunk(&root, &staged, 0, HunkAction::Unstage).unwrap();
    assert!(diff_between(&root, &Side::Head, &Side::Index, path, None)
        .unwrap()
        .is_none());

    // Revert the first hunk on disk; the second stays.
    let diff = diff_between(&root, &Side::Index, &Side::WorkTree, path, None)
        .unwrap()
        .unwrap();
    apply_hunk(&root, &diff, 0, HunkAction::Revert).unwrap();
    let text = read(&root, "a.php");
    assert!(!text.contains("line 2 changed") && text.contains("added after 25"));
}

#[test]
fn sides_untracked_renamed_rollback_and_ignored() {
    if !have_git() {
        eprintln!("GATE SKIP: git is not installed");
        return;
    }
    let (_dir, root) = repo();
    write(&root, "keep.php", "<?php\necho 1;\n");
    write(&root, "old.php", &lines(10));
    write(&root, ".gitignore", "vendor/\n");
    commit_all(&root, "base");
    write(&root, "vendor/x/y.php", "ignored");
    write(&root, "new.php", "<?php\nnew\n");
    git(&root, &["mv", "old.php", "renamed.php"]);
    write(&root, "keep.php", "<?php\necho 2;\n");
    write(&root, "dir/nested/untracked.txt", "u");

    let list = status(&root).unwrap();
    let find = |p: &str| list.iter().find(|s| s.path == Path::new(p)).cloned();
    assert!(find("new.php").unwrap().untracked);
    // Untracked files inside new folders are listed one by one.
    assert!(find("dir/nested/untracked.txt").unwrap().untracked);
    let renamed = find("renamed.php").unwrap();
    assert_eq!(renamed.orig_path, Some(PathBuf::from("old.php")));
    assert_eq!(ignored(&root).unwrap(), [PathBuf::from("vendor/")]);

    // An untracked file compares as all added.
    let added = diff_between(
        &root,
        &Side::Index,
        &Side::WorkTree,
        Path::new("new.php"),
        None,
    )
    .unwrap()
    .unwrap();
    assert!(added.is_new());
    assert_eq!(added.hunks[0].new_count, 2);
    // A pure rename has no changed lines.
    assert!(diff_between(
        &root,
        &Side::Head,
        &Side::Index,
        Path::new("renamed.php"),
        Some(Path::new("old.php"))
    )
    .unwrap()
    .is_none());

    // Stage and unstage several files at once.
    stage_paths(
        &root,
        &[PathBuf::from("new.php"), PathBuf::from("keep.php")],
    )
    .unwrap();
    let list = status(&root).unwrap();
    assert!(list
        .iter()
        .any(|s| s.path == Path::new("new.php") && s.staged.is_some()));
    unstage_paths(&root, &[PathBuf::from("keep.php")]).unwrap();

    // Rollback: keep.php back to HEAD, new.php leaves the index but stays.
    rollback(
        &root,
        &[PathBuf::from("keep.php"), PathBuf::from("new.php")],
    )
    .unwrap();
    assert_eq!(read(&root, "keep.php"), "<?php\necho 1;\n");
    assert!(root.join("new.php").exists());
    let list = status(&root).unwrap();
    assert!(list
        .iter()
        .any(|s| s.path == Path::new("new.php") && s.untracked));
    assert!(!list.iter().any(|s| s.path == Path::new("keep.php")));
}

#[test]
fn commits_show_hook_output_and_a_failing_hook_keeps_the_index() {
    if !have_git() {
        eprintln!("GATE SKIP: git is not installed");
        return;
    }
    let (_dir, root) = repo();
    write(&root, "a.php", "a\n");
    commit_all(&root, "base");
    let hook = root.join(".git/hooks/pre-commit");
    std::fs::write(
        &hook,
        "#!/bin/sh\necho \"lint: checking\"\nif git diff --cached | grep -q FORBIDDEN; then echo \"lint: FORBIDDEN found\" >&2; exit 1; fi\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    write(&root, "a.php", "a\nFORBIDDEN\n");
    stage_paths(&root, &[PathBuf::from("a.php")]).unwrap();
    let cmd = commands::commit(&root, "Add forbidden", false).unwrap();
    let (ok, output) = commands::run(&root, &cmd).unwrap();
    assert!(!ok);
    assert!(output.contains("lint: FORBIDDEN found"), "{output}");
    let list = status(&root).unwrap();
    assert!(list[0].staged.is_some(), "the index was kept");

    write(&root, "a.php", "a\nfine\n");
    stage_paths(&root, &[PathBuf::from("a.php")]).unwrap();
    let message = "Add fine\n\nBody with 'quotes' and \"more\".";
    let (ok, output) =
        commands::run(&root, &commands::commit(&root, message, false).unwrap()).unwrap();
    assert!(ok, "{output}");
    assert!(output.contains("lint: checking"), "{output}");
    assert_eq!(phpide_vcs::last_commit_message(&root).unwrap(), message);

    // Amend keeps one commit with the new message.
    let (ok, _) = commands::run(
        &root,
        &commands::commit(&root, "Add fine (amended)", true).unwrap(),
    )
    .unwrap();
    assert!(ok);
    let entries = log(&root, &LogFilter::default()).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].subject, "Add fine (amended)");
}

#[test]
fn branches_remote_push_fetch_pull_and_a_merge_conflict() {
    if !have_git() {
        eprintln!("GATE SKIP: git is not installed");
        return;
    }
    let (dir, root) = repo();
    let remote = dir.path().join("remote.git");
    git(
        dir.path(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    git(
        &root,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    write(&root, "a.php", "base\n");
    commit_all(&root, "base");

    // First push sets the upstream.
    let push = commands::push(&root).unwrap();
    assert_eq!(push.args, ["push", "--set-upstream", "origin", "HEAD"]);
    let (ok, out) = commands::run(&root, &push).unwrap();
    assert!(ok, "{out}");
    let list = branches(&root).unwrap();
    let main = list.iter().find(|b| b.name == "main").unwrap();
    assert!(main.current);
    assert_eq!(main.upstream.as_deref(), Some("origin/main"));
    assert!(list.iter().any(|b| b.remote && b.name == "origin/main"));
    assert_eq!(commands::push(&root).unwrap().args, ["push"]);

    // A second clone pushes a commit: fetch shows it as incoming, pull brings it.
    let other = dir.path().join("other");
    git(
        dir.path(),
        &[
            "clone",
            "-q",
            remote.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    configure(&other);
    write(&other, "b.php", "from other\n");
    commit_all(&other, "From the other clone");
    git(&other, &["push", "-q"]);
    let (ok, out) = commands::run(&root, &commands::fetch(&root)).unwrap();
    assert!(ok, "{out}");
    let main = branches(&root)
        .unwrap()
        .into_iter()
        .find(|b| b.name == "main")
        .unwrap();
    assert_eq!((main.ahead, main.behind), (0, 1));
    let (ok, out) = commands::run(&root, &commands::pull(&root)).unwrap();
    assert!(ok, "{out}");
    assert_eq!(read(&root, "b.php"), "from other\n");

    // Branches: create, check out, commit, back to main, rename, delete.
    let (ok, out) = commands::run(&root, &commands::new_branch("feature", None, true)).unwrap();
    assert!(ok, "{out}");
    write(&root, "a.php", "feature side\n");
    commit_all(&root, "Feature change");
    commands::run(&root, &commands::checkout("main", false)).unwrap();
    write(&root, "a.php", "main side\n");
    commit_all(&root, "Main change");

    // Merging conflicts: the operation and the conflict are visible.
    let (ok, out) = commands::run(&root, &commands::merge("feature")).unwrap();
    assert!(!ok, "{out}");
    assert!(out.contains("CONFLICT"), "{out}");
    assert_eq!(operation(&root).unwrap(), Some(RepoOperation::Merge));
    let conflicted = status(&root).unwrap();
    assert!(conflicted
        .iter()
        .any(|s| s.path == Path::new("a.php") && s.conflict));
    accept_side(&root, Path::new("a.php"), ConflictSide::Theirs).unwrap();
    assert_eq!(read(&root, "a.php"), "feature side\n");
    assert!(!status(&root).unwrap().iter().any(|s| s.conflict));
    let (ok, out) = commands::run(
        &root,
        &commands::commit(&root, "Merge feature", false).unwrap(),
    )
    .unwrap();
    assert!(ok, "{out}");
    assert_eq!(operation(&root).unwrap(), None);

    // The log: a merge has two parents; filters by text, author and paging.
    let entries = log(
        &root,
        &LogFilter {
            all: true,
            without_files: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(entries[0].subject, "Merge feature");
    assert_eq!(entries[0].parents.len(), 2);
    // A merge lists what it brought in, against its first parent.
    let files = phpide_vcs::commit_files(&root, &entries[0].sha).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, PathBuf::from("a.php"));
    // A root commit lists every file as added.
    let first = entries.last().unwrap();
    let files = phpide_vcs::commit_files(&root, &first.sha).unwrap();
    assert!(files
        .iter()
        .all(|f| f.kind == phpide_vcs::ChangeKind::Added));
    assert!(entries[0].refs.iter().any(|r| r == "HEAD -> main"));
    let found = log(
        &root,
        &LogFilter {
            text: Some("OTHER clone".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
        log(
            &root,
            &LogFilter {
                author: Some("nobody".into()),
                ..Default::default()
            }
        )
        .unwrap()
        .len(),
        0
    );
    let page = log(
        &root,
        &LogFilter {
            skip: 1,
            max_count: 2,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page.len(), 2);
    assert_ne!(page[0].subject, "Merge feature");

    commands::run(&root, &commands::rename_branch("feature", "feature-2")).unwrap();
    let (ok, out) = commands::run(&root, &commands::delete_branch("feature-2", false)).unwrap();
    assert!(ok, "merged branch deletes without force: {out}");
    assert!(!branches(&root)
        .unwrap()
        .iter()
        .any(|b| b.name.starts_with("feature")));

    // Push the merge; then the local branch is even with the remote.
    let (ok, out) = commands::run(&root, &commands::push(&root).unwrap()).unwrap();
    assert!(ok, "{out}");
    let main = branches(&root)
        .unwrap()
        .into_iter()
        .find(|b| b.name == "main")
        .unwrap();
    assert_eq!((main.ahead, main.behind), (0, 0));
}
