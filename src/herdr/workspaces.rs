//! Opens the Herdr workspace of an existing Git checkout.
//!
//! Herdr keeps at most one workspace per checkout: `worktree.open` returns
//! the open one, or opens a new one, so asking twice never duplicates it.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{Herdr, HerdrError, Operation, RequestFailure};

/// The workspace Herdr opened or found for a checkout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenedWorkspace {
    pub id: String,
    pub label: Option<String>,
    /// Whether the workspace was already open before the request.
    pub already_open: bool,
}

/// Opens and focuses the workspace of the linked worktree at `checkout`,
/// a checkout of the repository whose main checkout is `repository`.
///
/// No label is passed: Herdr applies one even to an already open workspace,
/// which would rename a workspace the user named. A new workspace gets
/// Herdr's default label, the checkout's directory name.
pub fn open_worktree(
    herdr: &Herdr,
    repository: &Path,
    checkout: &Path,
) -> Result<OpenedWorkspace, HerdrError> {
    let operation = Operation::WorktreeOpen;
    let reply = herdr
        .request(
            operation,
            [
                "worktree".as_ref(),
                "open".as_ref(),
                "--cwd".as_ref(),
                repository.as_os_str(),
                "--path".as_ref(),
                checkout.as_os_str(),
                "--focus".as_ref(),
            ],
        )
        .map_err(|error| match &error {
            HerdrError::Request {
                cause:
                    RequestFailure::Rejected {
                        code: Some(code), ..
                    },
                ..
            } if code == "worktree_not_found" => HerdrError::NotFound {
                kind: "worktree",
                id: checkout.display().to_string(),
            },
            _ => error,
        })?;
    let workspace = &reply["workspace"];
    let id = workspace["workspace_id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or(missing("workspace_id", operation))?;
    let reported = workspace["worktree"]["checkout_path"]
        .as_str()
        .map(PathBuf::from)
        .ok_or(missing("checkout_path", operation))?;
    if !same_path(&reported, checkout) {
        return Err(HerdrError::Mismatch {
            kind: "checkout",
            supplied: checkout.display().to_string(),
            reported: reported.display().to_string(),
        });
    }
    Ok(OpenedWorkspace {
        id: id.to_owned(),
        label: workspace["label"].as_str().map(str::to_owned),
        already_open: reply["already_open"].as_bool().unwrap_or(false),
    })
}

/// Whether two paths name the same directory, comparing resolved paths when
/// both exist so that symbolic links do not cause a false mismatch.
fn same_path(left: &Path, right: &Path) -> bool {
    left == right
        || matches!(
            (fs::canonicalize(left), fs::canonicalize(right)),
            (Ok(left), Ok(right)) if left == right
        )
}

fn missing(field: &'static str, operation: Operation) -> HerdrError {
    HerdrError::Request {
        operation,
        cause: RequestFailure::MissingField { field },
    }
}
