//! Comparing a file between two sides (HEAD, index, working tree or any
//! revision) and staging, unstaging or reverting one hunk at a time.
//!
//! The side-by-side diff needs both full texts plus the hunks that align
//! them: [`file_at`] reads a side, [`diff_between`] asks Git for the hunks.
//! [`apply_hunk`] turns one hunk into a patch and feeds it to `git apply`,
//! so the rest of the file is untouched.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use phpide_core::{Component, ErrorKind, IdeError};

use crate::diff::{parse_unified, DiffLine, DiffLineKind, FileDiff, Hunk};
use crate::repo::{git_error, run_git_env, PLUMBING_FLAGS};

/// One side of a comparison.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Side {
    Head,
    /// The staging area.
    Index,
    /// The file on disk.
    WorkTree,
    /// Any revision Git understands (sha, branch, `sha^`).
    Rev(String),
}

impl Side {
    /// Short label for diff headers.
    pub fn label(&self) -> String {
        match self {
            Side::Head => "HEAD".into(),
            Side::Index => "Staged".into(),
            Side::WorkTree => "Your version".into(),
            Side::Rev(rev) => rev.chars().take(10).collect(),
        }
    }
}

/// The content of `path` (relative to `root`) on `side`; `None` when the
/// file does not exist there.
pub fn file_at(root: &Path, side: &Side, path: &Path) -> Result<Option<Vec<u8>>, IdeError> {
    let spec = match side {
        Side::WorkTree => {
            return match std::fs::read(root.join(path)) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(IdeError::new(
                    Component::Git,
                    "file_at",
                    ErrorKind::Io,
                    format!("could not read {}", path.display()),
                    "check the file permissions",
                )
                .with_detail(e.to_string())),
            };
        }
        Side::Index => format!(":{}", git_path(path)),
        Side::Head => format!("HEAD:{}", git_path(path)),
        Side::Rev(rev) => format!("{rev}:{}", git_path(path)),
    };
    let out = run_git_env(root, &["cat-file", "blob", &spec], &[("LC_ALL", "C")])?;
    if out.status.success() {
        return Ok(Some(out.stdout));
    }
    let err = String::from_utf8_lossy(&out.stderr);
    // Missing path, missing index entry or unborn HEAD: the file is absent.
    if err.contains("does not exist")
        || err.contains("not a valid object name")
        || err.contains("Not a valid object name")
        || err.contains("invalid object name")
        || err.contains("bad revision")
        || err.contains("exists on disk, but not in")
        || err.contains("path not in the index")
    {
        return Ok(None);
    }
    Err(git_error("file_at", &out))
}

/// Git wants forward slashes in `rev:path`.
fn git_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Hunks between two sides of `path` (and `old_path` for a rename).
///
/// Supported pairs: index → working tree, revision → index, revision →
/// working tree and revision → revision (`Head` is a revision). A side
/// where the file is missing gives an all-added or all-deleted diff.
/// `None` when the two sides are equal.
pub fn diff_between(
    root: &Path,
    from: &Side,
    to: &Side,
    path: &Path,
    old_path: Option<&Path>,
) -> Result<Option<FileDiff>, IdeError> {
    let rev = |side: &Side| match side {
        Side::Head => Some("HEAD".to_string()),
        Side::Rev(r) => Some(r.clone()),
        _ => None,
    };
    let mut args: Vec<String> = vec![
        "diff".into(),
        "--no-color".into(),
        "--no-ext-diff".into(),
        "--src-prefix=a/".into(),
        "--dst-prefix=b/".into(),
        "--no-textconv".into(),
        "-U3".into(),
    ];
    match (from, to) {
        (Side::Index, Side::WorkTree) => {}
        (f, Side::Index) if rev(f).is_some() => {
            args.push("--cached".into());
            args.push(rev(f).unwrap());
        }
        (f, Side::WorkTree) if rev(f).is_some() => args.push(rev(f).unwrap()),
        (f, t) if rev(f).is_some() && rev(t).is_some() => {
            args.push(rev(f).unwrap());
            args.push(rev(t).unwrap());
        }
        _ => {
            return Err(IdeError::new(
                Component::Git,
                "diff_between",
                ErrorKind::InvalidInput,
                format!("cannot compare {from:?} with {to:?}"),
                "compare an older side with a newer one",
            ))
        }
    }
    // Absent on one side (untracked file, unborn HEAD, root commit): Git
    // either cannot name the revision or leaves the file out of the diff.
    let old = old_path.unwrap_or(path);
    let before = file_at(root, from, old)?;
    let after = file_at(root, to, path)?;
    match (&before, &after) {
        (None, None) => return Ok(None),
        (None, Some(text)) => return Ok(Some(whole_file(path, text, true))),
        (Some(text), None) => return Ok(Some(whole_file(old, text, false))),
        _ => {}
    }
    if before == after {
        return Ok(None);
    }
    if old_path.is_some_and(|o| o != path) {
        args.push("-M".into());
    }
    args.push("--".into());
    if let Some(o) = old_path.filter(|o| *o != path) {
        args.push(git_path(o));
    }
    args.push(git_path(path));
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = run_git_env(root, &refs, &[])?;
    if !out.status.success() {
        return Err(git_error("diff", &out));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(parse_unified(&text)
        .into_iter()
        .next()
        .filter(|f| f.binary || !f.is_empty()))
}

/// A diff where every line was added (`added`) or deleted.
fn whole_file(path: &Path, bytes: &[u8], added: bool) -> FileDiff {
    let binary = bytes.contains(&0);
    let text = String::from_utf8_lossy(bytes);
    let ends_with_newline = text.ends_with('\n');
    let lines: Vec<&str> = text.lines().collect();
    let count = lines.len();
    let lines = lines
        .into_iter()
        .enumerate()
        .map(|(i, l)| DiffLine {
            kind: if added {
                DiffLineKind::Add
            } else {
                DiffLineKind::Del
            },
            content: l.to_string(),
            old_lineno: (!added).then_some(i as u32 + 1),
            new_lineno: added.then_some(i as u32 + 1),
            no_newline: i + 1 == count && !ends_with_newline,
        })
        .collect();
    let null = PathBuf::from("/dev/null");
    let hunk = Hunk {
        old_start: if added { 0 } else { 1 },
        old_count: if added { 0 } else { count as u32 },
        new_start: if added { 1 } else { 0 },
        new_count: if added { count as u32 } else { 0 },
        lines,
    };
    FileDiff {
        old_path: if added {
            null.clone()
        } else {
            path.to_path_buf()
        },
        new_path: if added { path.to_path_buf() } else { null },
        hunks: if binary || count == 0 {
            Vec::new()
        } else {
            vec![hunk]
        },
        binary,
    }
}

/// What to do with one hunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HunkAction {
    /// Index ← working tree (the hunk comes from the unstaged diff).
    Stage,
    /// Index → HEAD (the hunk comes from the staged diff).
    Unstage,
    /// Working tree → index (the hunk comes from the unstaged diff).
    Revert,
}

/// A patch with only `hunk` of `diff`, as if it were the only change.
pub fn hunk_patch(diff: &FileDiff, hunk: &Hunk) -> String {
    let name = |p: &Path, prefix: &str| {
        if p == Path::new("/dev/null") {
            "/dev/null".to_string()
        } else {
            format!("{prefix}{}", git_path(p))
        }
    };
    // Alone, the hunk starts where it starts in the old file.
    let before = if hunk.old_count == 0 {
        hunk.old_start
    } else {
        hunk.old_start.saturating_sub(1)
    };
    let new_start = if hunk.new_count == 0 {
        before
    } else {
        before + 1
    };
    let mut patch = format!(
        "--- {}\n+++ {}\n@@ -{},{} +{},{} @@\n",
        name(&diff.old_path, "a/"),
        name(&diff.new_path, "b/"),
        hunk.old_start,
        hunk.old_count,
        new_start,
        hunk.new_count
    );
    for line in &hunk.lines {
        patch.push(match line.kind {
            DiffLineKind::Add => '+',
            DiffLineKind::Del => '-',
            DiffLineKind::Context => ' ',
        });
        patch.push_str(&line.content);
        patch.push('\n');
        if line.no_newline {
            patch.push_str("\\ No newline at end of file\n");
        }
    }
    patch
}

/// Applies one hunk: stage it, unstage it or revert it on disk.
pub fn apply_hunk(
    root: &Path,
    diff: &FileDiff,
    hunk: usize,
    action: HunkAction,
) -> Result<(), IdeError> {
    let Some(h) = diff.hunks.get(hunk) else {
        return Err(IdeError::new(
            Component::Git,
            "apply_hunk",
            ErrorKind::InvalidInput,
            "the change is no longer there",
            "refresh the diff and try again",
        ));
    };
    let patch = hunk_patch(diff, h);
    let mut args = vec!["apply", "--recount", "--whitespace=nowarn"];
    match action {
        HunkAction::Stage => args.push("--cached"),
        HunkAction::Unstage => args.extend(["--cached", "-R"]),
        HunkAction::Revert => args.push("-R"),
    }
    args.push("-");
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(PLUMBING_FLAGS)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            IdeError::new(
                Component::Git,
                "git",
                ErrorKind::NotFound,
                "could not run git",
                "install Git and try again",
            )
            .with_detail(e.to_string())
        })?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(patch.as_bytes());
    }
    let out = child.wait_with_output().map_err(|e| {
        IdeError::new(
            Component::Git,
            "apply",
            ErrorKind::Io,
            "git apply did not finish",
            "try again",
        )
        .with_detail(e.to_string())
    })?;
    if !out.status.success() {
        return Err(IdeError::new(
            Component::Git,
            "apply",
            ErrorKind::Other,
            "Git could not apply the change",
            "the file changed since the diff was computed; refresh and try again",
        )
        .with_detail(String::from_utf8_lossy(&out.stderr).into_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hunk(old: (u32, u32), new: (u32, u32), lines: &[(&str, &str)]) -> Hunk {
        Hunk {
            old_start: old.0,
            old_count: old.1,
            new_start: new.0,
            new_count: new.1,
            lines: lines
                .iter()
                .map(|(k, c)| DiffLine {
                    kind: match *k {
                        "+" => DiffLineKind::Add,
                        "-" => DiffLineKind::Del,
                        _ => DiffLineKind::Context,
                    },
                    content: c.to_string(),
                    old_lineno: None,
                    new_lineno: None,
                    no_newline: false,
                })
                .collect(),
        }
    }

    #[test]
    fn a_later_hunk_alone_starts_where_it_starts_in_the_old_file() {
        let diff = FileDiff {
            old_path: "a.php".into(),
            new_path: "a.php".into(),
            hunks: vec![],
            binary: false,
        };
        // Second hunk of a file whose first hunk added 5 lines.
        let h = hunk(
            (20, 3),
            (25, 4),
            &[(" ", "x"), ("+", "new"), (" ", "y"), (" ", "z")],
        );
        let patch = hunk_patch(&diff, &h);
        assert!(patch.contains("@@ -20,3 +20,4 @@"), "{patch}");
        assert!(patch.starts_with("--- a/a.php\n+++ b/a.php\n"));
        // Pure insertion after line 7 and pure deletion of line 8.
        assert!(hunk_patch(&diff, &hunk((7, 0), (9, 1), &[("+", "n")])).contains("@@ -7,0 +8,1 @@"));
        assert!(hunk_patch(&diff, &hunk((8, 1), (9, 0), &[("-", "o")])).contains("@@ -8,1 +7,0 @@"));
    }

    #[test]
    fn missing_newline_marker_is_kept() {
        let diff = FileDiff {
            old_path: "/dev/null".into(),
            new_path: "n.php".into(),
            hunks: vec![],
            binary: false,
        };
        let mut h = hunk((0, 0), (1, 1), &[("+", "end")]);
        h.lines[0].no_newline = true;
        let patch = hunk_patch(&diff, &h);
        assert!(patch.starts_with("--- /dev/null\n+++ b/n.php\n"));
        assert!(patch.ends_with("+end\n\\ No newline at end of file\n"));
    }

    #[test]
    fn whole_file_diffs_count_lines_and_spot_binaries() {
        let added = whole_file(Path::new("x"), b"a\nb", true);
        assert!(added.is_new());
        assert_eq!(added.hunks[0].new_count, 2);
        assert!(added.hunks[0].lines[1].no_newline);
        let gone = whole_file(Path::new("x"), b"a\n", false);
        assert!(gone.is_deleted());
        assert!(whole_file(Path::new("x"), b"\x00\x01", true).binary);
    }
}
