//! How far the setup of each task's workspace got.
//!
//! Each tab of the default layout is recorded as soon as Herdr creates it,
//! and marked ready once its command has been started. A later invocation
//! reads this record to add only what is missing.

use rusqlite::params;

use super::{StateError, Store};

/// A tab Leme opened for a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutTab {
    /// The layout role, such as `agent`, `editor` or `tests`.
    pub role: String,
    pub workspace: String,
    pub tab: String,
    /// The pane the tab's shell runs in.
    pub pane: String,
    /// Whether the tab's command has been started.
    pub ready: bool,
}

impl Store {
    /// The tabs recorded for task `task` of `project`.
    pub fn layout_tabs(&self, project: &str, task: &str) -> Result<Vec<LayoutTab>, StateError> {
        let query = || -> rusqlite::Result<Vec<LayoutTab>> {
            let mut statement = self.connection.prepare(
                "SELECT role, workspace, tab, pane, ready FROM workspace_tabs
                 WHERE project_id = ?1 AND task_id = ?2 ORDER BY role",
            )?;
            let rows = statement.query_map(params![project, task], |row| {
                Ok(LayoutTab {
                    role: row.get(0)?,
                    workspace: row.get(1)?,
                    tab: row.get(2)?,
                    pane: row.get(3)?,
                    ready: row.get::<_, i64>(4)? == 1,
                })
            })?;
            rows.collect()
        };
        query().map_err(|error| self.failure(error.into()))
    }

    /// Records that `tab`, whose shell runs in `pane`, now fills `role` in
    /// `workspace`, replacing any earlier tab for that role. The tab is not
    /// ready until [`Store::mark_tab_ready`].
    pub fn record_tab(
        &mut self,
        project: &str,
        task: &str,
        role: &str,
        workspace: &str,
        tab: &str,
        pane: &str,
    ) -> Result<(), StateError> {
        self.connection
            .execute(
                "INSERT INTO workspace_tabs (project_id, task_id, role, workspace, tab, pane)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (project_id, task_id, role) DO UPDATE SET
                     workspace = excluded.workspace, tab = excluded.tab,
                     pane = excluded.pane, ready = 0",
                params![project, task, role, workspace, tab, pane],
            )
            .map(drop)
            .map_err(|error| self.failure(error.into()))
    }

    /// Records that the tab filling `role` has started its command.
    pub fn mark_tab_ready(
        &mut self,
        project: &str,
        task: &str,
        role: &str,
    ) -> Result<(), StateError> {
        self.connection
            .execute(
                "UPDATE workspace_tabs SET ready = 1
                 WHERE project_id = ?1 AND task_id = ?2 AND role = ?3",
                params![project, task, role],
            )
            .map(drop)
            .map_err(|error| self.failure(error.into()))
    }
}
