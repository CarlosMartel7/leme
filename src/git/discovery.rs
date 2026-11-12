//! Identifies the repository and checkout containing a directory.
//!
//! Every Git call here only reads, so discovery never changes HEAD, the index
//! or any ref. Arguments go straight to the Git process, so paths with spaces
//! need no quoting.

use std::ffi::OsStr;
use std::fmt;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde::Serialize;

/// The Git location of a directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Checkout {
    /// Root of the repository's main checkout, shared by its linked worktrees.
    pub repository: PathBuf,
    /// Current branch, or `None` when HEAD is detached.
    pub branch: Option<String>,
    /// Root of the checkout containing the directory.
    pub checkout: PathBuf,
}

/// Why a directory could not be resolved to a checkout.
#[derive(Debug)]
pub enum DiscoveryError {
    /// The `git` executable is not on `PATH`.
    GitNotFound,
    /// The `git` executable could not be started.
    GitNotStarted(io::Error),
    /// The directory is outside any Git repository.
    NotARepository(PathBuf),
    /// The directory is in a repository but not in a checkout of it.
    NotACheckout(PathBuf),
    /// Git reported a failure, such as refusing a repository it does not trust.
    GitFailed { command: String, message: String },
    /// Git succeeded with output Leme does not understand.
    UnexpectedOutput { command: String },
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GitNotFound => {
                write!(f, "git was not found; install Git and add it to PATH")
            }
            Self::GitNotStarted(error) => write!(f, "cannot run git: {error}"),
            Self::NotARepository(path) => write!(
                f,
                "{} is not inside a Git repository; run leme from a project checkout",
                path.display()
            ),
            Self::NotACheckout(path) => write!(
                f,
                "{} has no checked-out files (it is a bare repository or a .git directory); \
                 run leme from a project checkout",
                path.display()
            ),
            Self::GitFailed { command, message } => write!(f, "`{command}` failed: {message}"),
            Self::UnexpectedOutput { command } => {
                write!(f, "`{command}` returned output Leme cannot read")
            }
        }
    }
}

impl std::error::Error for DiscoveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::GitNotStarted(error) => Some(error),
            _ => None,
        }
    }
}

/// Resolves `directory` to its repository, branch and checkout.
pub fn discover(directory: &Path) -> Result<Checkout, DiscoveryError> {
    if read(directory, &["rev-parse", "--is-inside-work-tree"])? != b"true" {
        return Err(DiscoveryError::NotACheckout(directory.to_path_buf()));
    }

    let checkout = read_path(directory, &["rev-parse", "--show-toplevel"])?;
    let repository = main_checkout(directory, &checkout)?;
    let branch = current_branch(directory)?;

    Ok(Checkout {
        repository,
        branch,
        checkout,
    })
}

/// Finds the main checkout from any of the repository's checkouts.
///
/// `git worktree list` derives the main entry from the shared Git directory,
/// which for a submodule is a path under the parent's `.git/modules`. That is
/// why the list is consulted only from linked worktrees.
fn main_checkout(directory: &Path, checkout: &Path) -> Result<PathBuf, DiscoveryError> {
    let git_dir = read_path(
        directory,
        &["rev-parse", "--path-format=absolute", "--git-dir"],
    )?;
    let common_dir = read_path(
        directory,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    if git_dir == common_dir {
        return Ok(checkout.to_path_buf());
    }

    let arguments = ["worktree", "list", "--porcelain", "-z"];
    let list = read(directory, &arguments)?;
    first_worktree(&list).ok_or_else(|| DiscoveryError::UnexpectedOutput {
        command: command_line(&arguments),
    })
}

/// Extracts the first entry of `git worktree list --porcelain -z`.
fn first_worktree(list: &[u8]) -> Option<PathBuf> {
    let first_line = list.split(|byte| *byte == 0).next()?;
    let path = first_line.strip_prefix(b"worktree ")?;
    Some(path_from_bytes(path))
}

fn current_branch(directory: &Path) -> Result<Option<String>, DiscoveryError> {
    let arguments = ["symbolic-ref", "--quiet", "--short", "HEAD"];
    let output = run_git(directory, &arguments)?;
    // With --quiet, exit status 1 means HEAD is not a symbolic ref.
    if output.status.code() == Some(1) {
        return Ok(None);
    }
    let branch = success_output(&arguments, output)?;
    Ok(Some(String::from_utf8_lossy(&branch).into_owned()))
}

fn read_path(directory: &Path, arguments: &[&str]) -> Result<PathBuf, DiscoveryError> {
    Ok(path_from_bytes(&read(directory, arguments)?))
}

pub(super) fn read(directory: &Path, arguments: &[&str]) -> Result<Vec<u8>, DiscoveryError> {
    success_output(arguments, run_git(directory, arguments)?)
}

pub(super) fn run_git(directory: &Path, arguments: &[&str]) -> Result<Output, DiscoveryError> {
    Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        // A fixed locale keeps Git's messages recognizable.
        .env("LC_ALL", "C")
        .output()
        .map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => DiscoveryError::GitNotFound,
            _ => DiscoveryError::GitNotStarted(error),
        })
        .and_then(|output| {
            if output.status.success() || !is_not_a_repository(&output) {
                Ok(output)
            } else {
                Err(DiscoveryError::NotARepository(directory.to_path_buf()))
            }
        })
}

fn is_not_a_repository(output: &Output) -> bool {
    String::from_utf8_lossy(&output.stderr).contains("not a git repository")
}

pub(super) fn success_output(
    arguments: &[&str],
    output: Output,
) -> Result<Vec<u8>, DiscoveryError> {
    if output.status.success() {
        let mut stdout = output.stdout;
        // Remove only Git's line ending: a path may end in other whitespace.
        if stdout.last() == Some(&b'\n') {
            stdout.pop();
        }
        Ok(stdout)
    } else {
        Err(failure(arguments, &output))
    }
}

fn failure(arguments: &[&str], output: &Output) -> DiscoveryError {
    DiscoveryError::GitFailed {
        command: command_line(arguments),
        message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    }
}

pub(super) fn command_line(arguments: &[&str]) -> String {
    format!("git {}", arguments.join(" "))
}

/// Git reports paths as bytes, which need not be valid UTF-8 on Linux.
pub(super) fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(OsStr::from_bytes(bytes))
}
