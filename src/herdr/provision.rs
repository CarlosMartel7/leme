//! Creates a feature's branch and worktree through Herdr, and defines the
//! default tabs of a feature workspace.

use std::ffi::OsString;
use std::path::Path;

use super::workspaces::{OpenedWorkspace, workspace_from_reply};
use super::{Herdr, HerdrError, Operation};

/// The role of a tab in a feature workspace's default layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TabRole {
    /// A shell left ready for the agent launcher.
    Agent,
    /// Neovim, started in the checkout.
    Editor,
    /// A shell for running tests.
    Tests,
}

impl TabRole {
    /// Every role, in the order the tabs are opened.
    pub const ALL: [Self; 3] = [Self::Agent, Self::Editor, Self::Tests];

    /// The name stored in Leme's state database.
    pub fn key(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Editor => "editor",
            Self::Tests => "tests",
        }
    }

    /// The role stored as `key`.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|role| role.key() == key)
    }

    /// The tab's label in Herdr.
    pub fn label(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Editor => "neovim",
            Self::Tests => "tests",
        }
    }

    /// The command typed into the tab's shell once it opens, if any.
    pub fn command(self) -> Option<&'static str> {
        match self {
            Self::Editor => Some("nvim"),
            Self::Agent | Self::Tests => None,
        }
    }
}

/// What Herdr is asked to check out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewCheckout<'a> {
    /// A new branch starting at `base`.
    NewBranch { base: &'a str },
    /// A branch that exists but is not checked out anywhere.
    ExistingBranch,
}

/// Creates a worktree of the repository whose main checkout is
/// `repository`, with `branch` checked out at `path`, and opens and focuses
/// its new workspace.
///
/// Herdr runs `git worktree add`, so the branch, the directory and the
/// workspace all appear in this one request.
pub fn create_worktree(
    herdr: &Herdr,
    repository: &Path,
    branch: &str,
    checkout: NewCheckout<'_>,
    path: &Path,
) -> Result<OpenedWorkspace, HerdrError> {
    let operation = Operation::WorktreeCreate;
    let mut arguments: Vec<OsString> = vec![
        "worktree".into(),
        "create".into(),
        "--cwd".into(),
        repository.into(),
        "--branch".into(),
        branch.into(),
    ];
    if let NewCheckout::NewBranch { base } = checkout {
        arguments.push("--base".into());
        arguments.push(base.into());
    }
    arguments.extend(["--path".into(), path.into(), "--focus".into()]);
    let reply = herdr.request(operation, arguments)?;
    workspace_from_reply(&reply, operation, path)
}
