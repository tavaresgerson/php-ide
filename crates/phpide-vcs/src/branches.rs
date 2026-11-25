//! Branches, remotes and the operation in progress (merge, rebase…).

use std::path::Path;

use phpide_core::IdeError;

use crate::repo::{git_error, run_git, run_git_env};

/// A local or remote-tracking branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    /// Short name: `main`, `origin/main`.
    pub name: String,
    pub remote: bool,
    /// Checked out in this worktree.
    pub current: bool,
    /// Tracked upstream (`origin/main`).
    pub upstream: Option<String>,
    /// Commits not on the upstream yet (outgoing).
    pub ahead: u32,
    /// Commits on the upstream not here yet (incoming).
    pub behind: u32,
    /// The upstream was deleted on the remote.
    pub upstream_gone: bool,
    pub sha: String,
    /// Committer date, epoch seconds.
    pub timestamp: i64,
    pub subject: String,
}

const SEP: char = '\u{1f}';

/// Every local branch, then every remote-tracking branch.
pub fn branches(root: &Path) -> Result<Vec<Branch>, IdeError> {
    let format = [
        "%(refname)",
        "%(refname:short)",
        "%(HEAD)",
        "%(upstream:short)",
        "%(upstream:track,nobracket)",
        "%(objectname:short)",
        "%(committerdate:unix)",
        "%(contents:subject)",
    ]
    .join(&SEP.to_string());
    // `upstream:track` is translated: read it in the C locale.
    let out = run_git_env(
        root,
        &[
            "for-each-ref",
            &format!("--format={format}"),
            "refs/heads",
            "refs/remotes",
        ],
        &[("LC_ALL", "C")],
    )?;
    if !out.status.success() {
        return Err(git_error("branches", &out));
    }
    Ok(parse_branches(&String::from_utf8_lossy(&out.stdout)))
}

fn parse_branches(text: &str) -> Vec<Branch> {
    let mut list: Vec<Branch> = text
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(SEP).collect();
            if f.len() < 8 {
                return None;
            }
            let remote = f[0].starts_with("refs/remotes/");
            // `origin/HEAD` is an alias, not a branch.
            if remote && f[0].ends_with("/HEAD") {
                return None;
            }
            let (mut ahead, mut behind) = (0, 0);
            for part in f[4].split(", ") {
                if let Some(n) = part.strip_prefix("ahead ") {
                    ahead = n.trim().parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix("behind ") {
                    behind = n.trim().parse().unwrap_or(0);
                }
            }
            Some(Branch {
                name: f[1].to_string(),
                remote,
                current: f[2] == "*",
                upstream: (!f[3].is_empty()).then(|| f[3].to_string()),
                ahead,
                behind,
                upstream_gone: f[4] == "gone",
                sha: f[5].to_string(),
                timestamp: f[6].parse().unwrap_or(0),
                subject: f[7..].join(&SEP.to_string()),
            })
        })
        .collect();
    // Current first, then local by name, then remote by name.
    list.sort_by(|a, b| {
        (a.remote, !a.current, a.name.to_lowercase()).cmp(&(
            b.remote,
            !b.current,
            b.name.to_lowercase(),
        ))
    });
    list
}

/// Configured remotes (`origin` first).
pub fn remotes(root: &Path) -> Result<Vec<String>, IdeError> {
    let out = run_git(root, &["remote"])?;
    if !out.status.success() {
        return Err(git_error("remotes", &out));
    }
    let mut list: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .filter(|r| !r.is_empty())
        .collect();
    list.sort_by_key(|r| (r != "origin", r.clone()));
    Ok(list)
}

/// A multi-step operation that stopped half-way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoOperation {
    Merge,
    Rebase,
    CherryPick,
    Revert,
}

impl RepoOperation {
    pub fn label(self) -> &'static str {
        match self {
            RepoOperation::Merge => "Merging",
            RepoOperation::Rebase => "Rebasing",
            RepoOperation::CherryPick => "Cherry-picking",
            RepoOperation::Revert => "Reverting",
        }
    }

    /// `git <op>` subcommand for `--abort` / `--continue`.
    pub fn subcommand(self) -> &'static str {
        match self {
            RepoOperation::Merge => "merge",
            RepoOperation::Rebase => "rebase",
            RepoOperation::CherryPick => "cherry-pick",
            RepoOperation::Revert => "revert",
        }
    }
}

/// The operation in progress, if any.
pub fn operation(root: &Path) -> Result<Option<RepoOperation>, IdeError> {
    let names = [
        "MERGE_HEAD",
        "rebase-merge",
        "rebase-apply",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
    ];
    let mut args = vec!["rev-parse"];
    for name in names {
        args.push("--git-path");
        args.push(name);
    }
    let out = run_git(root, &args)?;
    if !out.status.success() {
        return Err(git_error("operation", &out));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for (line, name) in text.lines().zip(names) {
        let path = root.join(line.trim());
        if path.exists() {
            return Ok(Some(match name {
                "MERGE_HEAD" => RepoOperation::Merge,
                "CHERRY_PICK_HEAD" => RepoOperation::CherryPick,
                "REVERT_HEAD" => RepoOperation::Revert,
                _ => RepoOperation::Rebase,
            }));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_refs_with_tracking_and_skips_remote_head() {
        let s = SEP;
        let text = format!(
            "refs/heads/feature{s}feature{s} {s}origin/feature{s}ahead 2, behind 1{s}abc1234{s}100{s}Add x\n\
             refs/heads/main{s}main{s}*{s}origin/main{s}{s}def5678{s}200{s}Init\n\
             refs/heads/old{s}old{s} {s}origin/old{s}gone{s}aaa1111{s}50{s}Old\n\
             refs/remotes/origin/HEAD{s}origin{s} {s}{s}{s}def5678{s}200{s}Init\n\
             refs/remotes/origin/main{s}origin/main{s} {s}{s}{s}def5678{s}200{s}Init\n"
        );
        let list = parse_branches(&text);
        let names: Vec<&str> = list.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["main", "feature", "old", "origin/main"]);
        assert!(list[0].current);
        assert_eq!((list[1].ahead, list[1].behind), (2, 1));
        assert!(list[2].upstream_gone);
        assert!(list[3].remote && list[3].upstream.is_none());
    }
}
