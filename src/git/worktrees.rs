//! Lists a repository's checkouts and checks branch names.
//!
//! Like discovery, every Git call here only reads.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::discovery::{DiscoveryError, command_line, path_from_bytes, read, run_git};

/// One checkout of a repository, as `git worktree list` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Worktree {
    /// Root of the checkout.
    pub path: PathBuf,
    /// Checked-out branch, without `refs/heads/`, or `None` when HEAD is
    /// detached or the entry is the bare repository itself.
    pub branch: Option<String>,
    /// Whether this is the repository's main checkout rather than a linked one.
    pub main: bool,
    /// Whether Git still records the checkout although its directory is gone.
    pub prunable: bool,
}

/// Lists every checkout of the repository containing `directory`, main
/// checkout first.
pub fn worktrees(directory: &Path) -> Result<Vec<Worktree>, DiscoveryError> {
    let arguments = ["worktree", "list", "--porcelain", "-z"];
    let list = read(directory, &arguments)?;
    parse_worktrees(&list).ok_or_else(|| DiscoveryError::UnexpectedOutput {
        command: command_line(&arguments),
    })
}

/// The checkout that has `branch` checked out, if any.
pub fn checkout_of<'a>(worktrees: &'a [Worktree], branch: &str) -> Option<&'a Worktree> {
    worktrees
        .iter()
        .find(|worktree| worktree.branch.as_deref() == Some(branch))
}

/// Parses `git worktree list --porcelain -z`: one NUL-terminated attribute
/// per field, with an empty field after each checkout.
fn parse_worktrees(list: &[u8]) -> Option<Vec<Worktree>> {
    let mut worktrees: Vec<Worktree> = Vec::new();
    let mut current: Option<Worktree> = None;
    for field in list.split(|byte| *byte == 0) {
        if field.is_empty() {
            worktrees.extend(current.take());
            continue;
        }
        if let Some(path) = field.strip_prefix(b"worktree ") {
            worktrees.extend(current.take());
            current = Some(Worktree {
                path: path_from_bytes(path),
                branch: None,
                main: worktrees.is_empty(),
                prunable: false,
            });
            continue;
        }
        let worktree = current.as_mut()?;
        if let Some(reference) = field.strip_prefix(b"branch ") {
            let reference = String::from_utf8_lossy(reference);
            worktree.branch = Some(
                reference
                    .strip_prefix("refs/heads/")
                    .unwrap_or(&reference)
                    .to_owned(),
            );
        } else if field == b"prunable" || field.starts_with(b"prunable ") {
            worktree.prunable = true;
        }
    }
    worktrees.extend(current.take());
    (!worktrees.is_empty()).then_some(worktrees)
}

/// Why a branch name was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidBranch {
    pub name: String,
}

/// Checks that `name` is a valid, literal branch name.
///
/// `git check-ref-format --branch` also expands shorthands such as `@{-1}`;
/// a name it rewrites is refused, so the branch used is always the one the
/// user typed.
pub fn check_branch_name(
    directory: &Path,
    name: &str,
) -> Result<Result<(), InvalidBranch>, DiscoveryError> {
    let invalid = || {
        Ok(Err(InvalidBranch {
            name: name.to_owned(),
        }))
    };
    if name.starts_with('-') {
        return invalid();
    }
    let output = run_git(directory, &["check-ref-format", "--branch", name])?;
    let normalized = String::from_utf8_lossy(&output.stdout);
    if output.status.success() && normalized.trim_end_matches('\n') == name {
        Ok(Ok(()))
    } else {
        invalid()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_main_linked_detached_and_prunable_checkouts() {
        let list = b"worktree /repo\0HEAD 1111\0branch refs/heads/main\0\0\
                     worktree /work/a b\0HEAD 2222\0branch refs/heads/feature/a\0\0\
                     worktree /work/detached\0HEAD 3333\0detached\0\0\
                     worktree /work/gone\0HEAD 4444\0branch refs/heads/feature/gone\0prunable gitdir file points to non-existent location\0\0";
        let worktrees = parse_worktrees(list).unwrap();
        assert_eq!(worktrees.len(), 4);
        assert!(worktrees[0].main);
        assert_eq!(worktrees[0].branch.as_deref(), Some("main"));
        assert_eq!(worktrees[1].path, PathBuf::from("/work/a b"));
        assert_eq!(worktrees[1].branch.as_deref(), Some("feature/a"));
        assert!(!worktrees[1].main);
        assert_eq!(worktrees[2].branch, None);
        assert!(worktrees[3].prunable);
        assert_eq!(
            checkout_of(&worktrees, "feature/a").map(|w| &w.path),
            Some(&PathBuf::from("/work/a b"))
        );
        assert!(checkout_of(&worktrees, "feature/none").is_none());
    }

    #[test]
    fn refuses_output_without_a_worktree_line() {
        assert_eq!(parse_worktrees(b""), None);
        assert_eq!(parse_worktrees(b"HEAD 1111\0\0"), None);
    }
}
