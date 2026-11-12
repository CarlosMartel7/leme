//! Creates, lists and closes tabs, and starts commands in their panes.
//!
//! Every tab is created in an explicit workspace with an explicit working
//! directory, never wherever focus happens to be.

use std::ffi::OsString;
use std::path::Path;

use super::workspaces::missing;
use super::{Herdr, HerdrError, Operation};

/// A tab created by Leme, with the pane its shell runs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedTab {
    pub id: String,
    pub pane: String,
}

/// Creates a tab labelled `label` in `workspace`, whose shell starts in
/// `directory` with `environment` added.
///
/// `focus` makes it the active tab and focuses its workspace.
pub fn create_tab(
    herdr: &Herdr,
    workspace: &str,
    directory: &Path,
    label: &str,
    environment: &[(String, String)],
    focus: bool,
) -> Result<CreatedTab, HerdrError> {
    let operation = Operation::TabCreate;
    let mut arguments: Vec<OsString> = vec![
        "tab".into(),
        "create".into(),
        "--workspace".into(),
        workspace.into(),
        "--cwd".into(),
        directory.into(),
        "--label".into(),
        label.into(),
    ];
    for (name, value) in environment {
        arguments.push("--env".into());
        arguments.push(format!("{name}={value}").into());
    }
    arguments.push(if focus { "--focus" } else { "--no-focus" }.into());
    let reply = herdr.request(operation, arguments)?;
    let text = |value: &serde_json::Value, field| {
        value[field]
            .as_str()
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
            .ok_or(missing(field, operation))
    };
    let tab = text(&reply["tab"], "tab_id")?;
    if reply["tab"]["workspace_id"].as_str() != Some(workspace) {
        return Err(HerdrError::Mismatch {
            kind: "workspace",
            supplied: workspace.to_owned(),
            reported: reply["tab"]["workspace_id"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        });
    }
    Ok(CreatedTab {
        id: tab,
        pane: text(&reply["root_pane"], "pane_id")?,
    })
}

/// The IDs of the tabs currently open in `workspace`.
pub fn list_tabs(herdr: &Herdr, workspace: &str) -> Result<Vec<String>, HerdrError> {
    let operation = Operation::TabList;
    let reply = herdr.request(operation, ["tab", "list", "--workspace", workspace])?;
    let tabs = reply["tabs"].as_array().ok_or(missing("tabs", operation))?;
    Ok(tabs
        .iter()
        .filter_map(|tab| tab["tab_id"].as_str().map(str::to_owned))
        .collect())
}

/// Closes `tab`.
pub fn close_tab(herdr: &Herdr, tab: &str) -> Result<(), HerdrError> {
    herdr
        .request(Operation::TabClose, ["tab", "close", tab])
        .map(drop)
}

/// Types `command` followed by Enter into the shell of `pane`.
///
/// Herdr acknowledges that the input was queued, not that the command ran.
pub fn run_in_pane(herdr: &Herdr, pane: &str, command: &str) -> Result<(), HerdrError> {
    herdr.send(Operation::PaneSendInput, ["pane", "run", pane, command])
}
