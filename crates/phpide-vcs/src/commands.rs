//! Git commands that can take a while or print output the user must see
//! (hooks, merge results, remote messages). The UI runs them with streamed
//! output; [`run`] runs one to the end (tests, scripts).
//!
//! Nothing here can wait for input: no terminal prompt for credentials
//! (`GIT_TERMINAL_PROMPT=0`), SSH in batch mode unless the user configured
//! their own SSH command, and the commit message goes through a file.

use std::path::Path;

use phpide_core::{Component, ErrorKind, IdeError};

use crate::branches::{branches, remotes, RepoOperation};
use crate::repo::{git_error, run_git, PLUMBING_FLAGS};

/// A Git command line ready to spawn in the repository root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommand {
    /// What the UI shows while it runs ("Pushing main to origin").
    pub title: String,
    /// Arguments after `git` (without the plumbing flags).
    pub args: Vec<String>,
    /// Extra environment.
    pub env: Vec<(String, String)>,
    /// Talks to a remote (shows progress, may fail on credentials).
    pub network: bool,
}

impl GitCommand {
    fn new(title: impl Into<String>, args: &[&str]) -> Self {
        GitCommand {
            title: title.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            env: vec![("GIT_TERMINAL_PROMPT".into(), "0".into())],
            network: false,
        }
    }

    fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Arguments to pass to the `git` program.
    pub fn full_args(&self) -> Vec<String> {
        PLUMBING_FLAGS
            .iter()
            .map(|f| f.to_string())
            .chain(self.args.iter().cloned())
            .collect()
    }

    /// The command line as typed in a terminal (for the Git console).
    pub fn display(&self) -> String {
        let quoted: Vec<String> = self
            .args
            .iter()
            .map(|a| {
                if a.is_empty() || a.contains([' ', '"', '\'']) {
                    format!("'{}'", a.replace('\'', "'\\''"))
                } else {
                    a.clone()
                }
            })
            .collect();
        format!("git {}", quoted.join(" "))
    }

    fn remote(mut self, root: &Path) -> Self {
        self.network = true;
        self.env.extend(network_env(root));
        self
    }
}

/// Non-interactive SSH, unless the user chose an SSH command.
fn network_env(root: &Path) -> Vec<(String, String)> {
    let configured = std::env::var_os("GIT_SSH_COMMAND").is_some()
        || std::env::var_os("GIT_SSH").is_some()
        || run_git(root, &["config", "--get", "core.sshCommand"])
            .map(|o| o.status.success())
            .unwrap_or(false);
    if configured {
        Vec::new()
    } else {
        vec![("GIT_SSH_COMMAND".into(), "ssh -o BatchMode=yes".into())]
    }
}

/// Commits the index. The message is written to a file in the Git
/// directory, so it reaches Git byte for byte (`-F`).
pub fn commit(root: &Path, message: &str, amend: bool) -> Result<GitCommand, IdeError> {
    if message.trim().is_empty() {
        return Err(IdeError::new(
            Component::Git,
            "commit",
            ErrorKind::InvalidInput,
            "empty commit message",
            "write a message describing the change and try again",
        ));
    }
    let out = run_git(root, &["rev-parse", "--git-path", "PHPIDE_COMMIT_MSG"])?;
    if !out.status.success() {
        return Err(git_error("commit", &out));
    }
    let file = root.join(String::from_utf8_lossy(&out.stdout).trim());
    let mut text = message.trim_end().to_string();
    text.push('\n');
    std::fs::write(&file, text).map_err(|e| {
        IdeError::new(
            Component::Git,
            "commit",
            ErrorKind::Io,
            "could not write the commit message",
            "check the permissions of the .git folder",
        )
        .with_detail(e.to_string())
    })?;
    let title = if amend {
        "Amending the last commit"
    } else {
        "Committing"
    };
    let mut cmd = GitCommand::new(title, &["commit", "--cleanup=strip", "-F"])
        .arg(file.to_string_lossy().into_owned());
    if amend {
        cmd = cmd.arg("--amend");
    }
    Ok(cmd)
}

/// Fetches every remote and prunes deleted branches.
pub fn fetch(root: &Path) -> GitCommand {
    GitCommand::new("Fetching", &["fetch", "--all", "--prune"]).remote(root)
}

/// Update Project: pulls the current branch's upstream (merge or rebase as
/// the user configured `pull.rebase`).
pub fn pull(root: &Path) -> GitCommand {
    GitCommand::new("Updating the project", &["pull", "--no-edit"]).remote(root)
}

/// Pushes the current branch; without an upstream it sets one on the first
/// remote (`origin` when there is one).
pub fn push(root: &Path) -> Result<GitCommand, IdeError> {
    let list = branches(root)?;
    let current = list
        .iter()
        .find(|b| b.current && !b.remote)
        .ok_or_else(|| {
            IdeError::new(
                Component::Git,
                "push",
                ErrorKind::InvalidInput,
                "no branch is checked out",
                "check out a branch before pushing",
            )
        })?;
    if current.upstream.is_some() && !current.upstream_gone {
        return Ok(GitCommand::new(format!("Pushing {}", current.name), &["push"]).remote(root));
    }
    let remote = remotes(root)?.into_iter().next().ok_or_else(|| {
        IdeError::new(
            Component::Git,
            "push",
            ErrorKind::InvalidInput,
            "the repository has no remote",
            "add one with git remote add origin <url>",
        )
    })?;
    Ok(GitCommand::new(
        format!("Pushing {} to {remote}", current.name),
        &["push", "--set-upstream", &remote, "HEAD"],
    )
    .remote(root))
}

/// Checks out a branch; a remote branch gets a local tracking branch.
pub fn checkout(name: &str, remote: bool) -> GitCommand {
    if remote {
        GitCommand::new(format!("Checking out {name}"), &["switch", "--track", name])
    } else {
        GitCommand::new(format!("Checking out {name}"), &["switch", name])
    }
}

/// Creates a branch at `start` (HEAD when `None`), optionally checking it out.
pub fn new_branch(name: &str, start: Option<&str>, checkout: bool) -> GitCommand {
    let mut cmd = if checkout {
        GitCommand::new(format!("Creating {name}"), &["switch", "-c", name])
    } else {
        GitCommand::new(format!("Creating {name}"), &["branch", name])
    };
    if let Some(start) = start {
        cmd = cmd.arg(start);
    }
    cmd
}

pub fn rename_branch(old: &str, new: &str) -> GitCommand {
    GitCommand::new(format!("Renaming {old}"), &["branch", "-m", old, new])
}

/// Deletes a local branch; without `force` Git refuses unmerged work.
pub fn delete_branch(name: &str, force: bool) -> GitCommand {
    GitCommand::new(
        format!("Deleting {name}"),
        &["branch", if force { "-D" } else { "-d" }, name],
    )
}

/// Merges `name` into the current branch.
pub fn merge(name: &str) -> GitCommand {
    GitCommand::new(format!("Merging {name}"), &["merge", "--no-edit", name])
}

/// Abandons a merge, rebase, cherry-pick or revert.
pub fn abort(op: RepoOperation) -> GitCommand {
    GitCommand::new(
        format!("Aborting the {}", op.subcommand()),
        &[op.subcommand(), "--abort"],
    )
}

/// Goes on with a rebase, cherry-pick or revert after resolving conflicts
/// (a merge continues by committing).
pub fn continue_op(op: RepoOperation) -> GitCommand {
    let mut cmd = GitCommand::new(
        format!("Continuing the {}", op.subcommand()),
        &[op.subcommand(), "--continue"],
    );
    // Keep the prepared message instead of opening an editor.
    cmd.env.push(("GIT_EDITOR".into(), "true".into()));
    cmd
}

/// Runs a command to the end: `(success, stdout + stderr)`.
pub fn run(root: &Path, cmd: &GitCommand) -> Result<(bool, String), IdeError> {
    let out = std::process::Command::new("git")
        .current_dir(root)
        .args(cmd.full_args())
        .envs(cmd.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(std::process::Stdio::null())
        .output()
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
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok((out.status.success(), text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_quotes_arguments_with_spaces() {
        let cmd = new_branch("feature/x", Some("origin/main"), true);
        assert_eq!(cmd.display(), "git switch -c feature/x origin/main");
        assert_eq!(
            merge("my branch").display(),
            "git merge --no-edit 'my branch'"
        );
        assert!(cmd
            .full_args()
            .starts_with(&["-c".to_string(), "color.ui=false".to_string()]));
        assert!(cmd
            .env
            .contains(&("GIT_TERMINAL_PROMPT".into(), "0".into())));
    }

    #[test]
    fn empty_message_is_refused_before_touching_the_repository() {
        let err = commit(Path::new("/nonexistent"), "  \n", false).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
    }
}
