//! Resolves the Herdr workspace, tab and pane a Leme process belongs to.
//!
//! Herdr gives every process in a pane its caller IDs (`HERDR_WORKSPACE_ID`,
//! `HERDR_TAB_ID`, `HERDR_PANE_ID`). They are opaque, stable handles, so
//! resolution asks Herdr about those exact IDs and keeps the IDs Herdr
//! returns. It never asks which pane is focused, because the user may have
//! moved focus elsewhere by the time the request runs.

use std::env;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::Value;

use super::{Herdr, HerdrError, Operation, RequestFailure};

/// The Herdr IDs a Leme process was started with.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Invocation {
    pub workspace: Option<String>,
    pub tab: Option<String>,
    pub pane: Option<String>,
}

impl Invocation {
    /// The caller IDs Herdr placed in this process's environment.
    pub fn from_env() -> Self {
        Self::from_vars(|name| env::var(name).ok())
    }

    /// The caller IDs read through `var`, a lookup of environment variables.
    ///
    /// When a pane ID is present it is used alone: Herdr reports the pane's
    /// current tab and workspace, which stay correct even if the pane was
    /// moved after the inherited workspace and tab IDs were set.
    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Self {
        let id = |name| var(name).filter(|value| !value.trim().is_empty());
        match id("HERDR_PANE_ID") {
            Some(pane) => Self {
                workspace: None,
                tab: None,
                pane: Some(pane),
            },
            None => Self {
                workspace: id("HERDR_WORKSPACE_ID"),
                tab: id("HERDR_TAB_ID"),
                pane: None,
            },
        }
    }

    /// Whether no ID is present.
    pub fn is_empty(&self) -> bool {
        self.workspace.is_none() && self.tab.is_none() && self.pane.is_none()
    }
}

/// Where a Leme process sits in Herdr, as Herdr itself reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Context {
    pub workspace: WorkspaceInfo,
    /// `None` when only a workspace was supplied.
    pub tab: Option<TabInfo>,
    /// `None` when no pane was supplied, as for a Herdr popup.
    pub pane: Option<PaneInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspaceInfo {
    pub id: String,
    pub label: Option<String>,
    /// Position in Herdr's sidebar, which changes when workspaces move.
    pub number: Option<u64>,
    /// The Git checkout Herdr associates with the workspace, if any.
    pub checkout: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TabInfo {
    pub id: String,
    pub label: Option<String>,
    /// Position within the workspace, which changes when tabs move.
    pub number: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PaneInfo {
    pub id: String,
    /// The pane's working directory as Herdr last saw it.
    pub cwd: Option<PathBuf>,
}

/// Asks `herdr` about the IDs in `invocation`.
///
/// A pane is looked up first, because Herdr reports the tab and workspace
/// that contain it. Any tab or workspace also supplied must match them.
pub fn resolve(herdr: &Herdr, invocation: &Invocation) -> Result<Context, HerdrError> {
    if invocation.is_empty() {
        return Err(HerdrError::NoInvocation);
    }
    let mut workspace_id = invocation.workspace.clone();
    let mut tab_id = invocation.tab.clone();
    let mut pane = None;

    if let Some(id) = &invocation.pane {
        let operation = Operation::PaneCurrent;
        let reply = herdr
            .request(operation, ["pane", "current", "--pane", id.as_str()])
            .map_err(|error| not_found(error, "pane", id))?;
        let found = &reply["pane"];
        let reported_tab = text(found, "tab_id", operation)?;
        let reported_workspace = text(found, "workspace_id", operation)?;
        agree("workspace", workspace_id.as_deref(), &reported_workspace)?;
        agree("tab", tab_id.as_deref(), &reported_tab)?;
        workspace_id = Some(reported_workspace);
        tab_id = Some(reported_tab);
        pane = Some(PaneInfo {
            id: text(found, "pane_id", operation)?,
            cwd: found["cwd"].as_str().map(PathBuf::from),
        });
    }

    let Some(workspace_id) = workspace_id else {
        return Err(HerdrError::TabWithoutWorkspace);
    };
    let workspace = find_workspace(herdr, &workspace_id)?;
    let tab = tab_id
        .map(|id| find_tab(herdr, &workspace_id, &id))
        .transpose()?;
    Ok(Context {
        workspace,
        tab,
        pane,
    })
}

fn find_workspace(herdr: &Herdr, id: &str) -> Result<WorkspaceInfo, HerdrError> {
    let operation = Operation::WorkspaceList;
    let reply = herdr.request(operation, ["workspace", "list"])?;
    let entry = list(&reply, "workspaces", operation)?
        .iter()
        .find(|entry| entry["workspace_id"].as_str() == Some(id))
        .ok_or_else(|| HerdrError::NotFound {
            kind: "workspace",
            id: id.to_owned(),
        })?;
    Ok(WorkspaceInfo {
        id: id.to_owned(),
        label: entry["label"].as_str().map(str::to_owned),
        number: entry["number"].as_u64(),
        checkout: entry["worktree"]["checkout_path"]
            .as_str()
            .map(PathBuf::from),
    })
}

fn find_tab(herdr: &Herdr, workspace: &str, id: &str) -> Result<TabInfo, HerdrError> {
    let operation = Operation::TabList;
    let reply = herdr
        .request(operation, ["tab", "list", "--workspace", workspace])
        .map_err(|error| not_found(error, "workspace", workspace))?;
    let entry = list(&reply, "tabs", operation)?
        .iter()
        .find(|entry| entry["tab_id"].as_str() == Some(id))
        .ok_or_else(|| HerdrError::NotFound {
            kind: "tab",
            id: id.to_owned(),
        })?;
    if let Some(reported) = entry["workspace_id"].as_str() {
        agree("workspace", Some(workspace), reported)?;
    }
    Ok(TabInfo {
        id: id.to_owned(),
        label: entry["label"].as_str().map(str::to_owned),
        number: entry["number"].as_u64(),
    })
}

/// Turns Herdr's `<kind>_not_found` error into [`HerdrError::NotFound`].
fn not_found(error: HerdrError, kind: &'static str, id: &str) -> HerdrError {
    match &error {
        HerdrError::Request {
            cause: RequestFailure::Rejected {
                code: Some(code), ..
            },
            ..
        } if *code == format!("{kind}_not_found") => HerdrError::NotFound {
            kind,
            id: id.to_owned(),
        },
        _ => error,
    }
}

fn agree(kind: &'static str, supplied: Option<&str>, reported: &str) -> Result<(), HerdrError> {
    match supplied {
        Some(supplied) if supplied != reported => Err(HerdrError::Mismatch {
            kind,
            supplied: supplied.to_owned(),
            reported: reported.to_owned(),
        }),
        _ => Ok(()),
    }
}

fn text(value: &Value, field: &'static str, operation: Operation) -> Result<String, HerdrError> {
    value[field]
        .as_str()
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
        .ok_or(missing(field, operation))
}

fn list<'a>(
    value: &'a Value,
    field: &'static str,
    operation: Operation,
) -> Result<&'a Vec<Value>, HerdrError> {
    value[field].as_array().ok_or(missing(field, operation))
}

fn missing(field: &'static str, operation: Operation) -> HerdrError {
    HerdrError::Request {
        operation,
        cause: RequestFailure::MissingField { field },
    }
}
