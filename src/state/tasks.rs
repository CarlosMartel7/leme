//! Tasks and the checkout and Herdr workspace each one is associated with.
//!
//! A task is identified by its ID within a project and owns one branch. Git
//! remains the authority on where that branch is checked out; the recorded
//! path is updated to follow it.

use std::fmt;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;

use super::project::path_from_bytes;
use super::{Failure, StateError, Store};

/// A task registered to a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Task {
    pub id: String,
    pub branch: String,
    /// Root of the task's linked worktree.
    pub worktree: PathBuf,
    /// The Herdr workspace last opened for the task, which may since have
    /// been closed.
    pub workspace: Option<String>,
}

/// Why a task could not be associated with a branch or checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskConflict {
    /// The task is already registered with another branch.
    TaskHasOtherBranch {
        task: String,
        recorded: String,
        requested: String,
    },
    /// Another task of the project owns the branch.
    BranchHasOtherTask { branch: String, task: String },
    /// Another task, possibly of another project, is recorded at the checkout.
    WorktreeHasOtherTask { worktree: PathBuf, task: String },
}

impl fmt::Display for TaskConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TaskHasOtherBranch {
                task,
                recorded,
                requested,
            } => write!(
                f,
                "task {task} is registered with branch {recorded}, not {requested}; omit \
                 --branch or pass --branch {recorded}"
            ),
            Self::BranchHasOtherTask { branch, task } => {
                write!(f, "branch {branch} already belongs to task {task}")
            }
            Self::WorktreeHasOtherTask { worktree, task } => write!(
                f,
                "{} is already recorded as the checkout of task {task}",
                worktree.display()
            ),
        }
    }
}

impl std::error::Error for TaskConflict {}

impl Store {
    /// Associates task `id` of `project` with `branch` checked out at
    /// `worktree`, registering the task on first use.
    ///
    /// Returns the task as it was before, if it existed. Checking and writing
    /// under one write lock keeps concurrent registrations consistent.
    pub fn register_task(
        &mut self,
        project: &str,
        id: &str,
        branch: &str,
        worktree: &Path,
    ) -> Result<Result<Option<Task>, TaskConflict>, StateError> {
        self.register(project, id, branch, worktree)
            .map_err(|error| self.failure(error))
    }

    fn register(
        &mut self,
        project: &str,
        id: &str,
        branch: &str,
        worktree: &Path,
    ) -> Result<Result<Option<Task>, TaskConflict>, Failure> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let path = worktree.as_os_str().as_bytes();
        let previous = task(&transaction, project, id)?;
        let conflict = match &previous {
            Some(task) if task.branch != branch => Some(TaskConflict::TaskHasOtherBranch {
                task: id.to_owned(),
                recorded: task.branch.clone(),
                requested: branch.to_owned(),
            }),
            _ => branch_owner(&transaction, project, branch, id)?
                .map(|task| TaskConflict::BranchHasOtherTask {
                    branch: branch.to_owned(),
                    task,
                })
                .or(
                    worktree_owner(&transaction, path, project, id)?.map(|task| {
                        TaskConflict::WorktreeHasOtherTask {
                            worktree: worktree.to_path_buf(),
                            task,
                        }
                    }),
                ),
        };
        if let Some(conflict) = conflict {
            return Ok(Err(conflict));
        }
        transaction.execute(
            "INSERT INTO tasks (project_id, id, branch, worktree) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (project_id, id) DO UPDATE SET worktree = excluded.worktree",
            params![project, id, branch, path],
        )?;
        transaction.commit()?;
        Ok(Ok(previous))
    }

    /// Records `workspace` as the Herdr workspace of task `id`.
    pub fn record_workspace(
        &mut self,
        project: &str,
        id: &str,
        workspace: &str,
    ) -> Result<(), StateError> {
        self.connection
            .execute(
                "UPDATE tasks SET workspace = ?3 WHERE project_id = ?1 AND id = ?2",
                params![project, id, workspace],
            )
            .map(drop)
            .map_err(|error| self.failure(error.into()))
    }
}

pub(super) fn task(
    connection: &Connection,
    project: &str,
    id: &str,
) -> Result<Option<Task>, Failure> {
    Ok(connection
        .query_row(
            "SELECT id, branch, worktree, workspace FROM tasks
             WHERE project_id = ?1 AND id = ?2",
            params![project, id],
            from_row,
        )
        .optional()?)
}

pub(super) fn tasks(connection: &Connection, project: &str) -> Result<Vec<Task>, Failure> {
    let mut statement = connection.prepare(
        "SELECT id, branch, worktree, workspace FROM tasks WHERE project_id = ?1 ORDER BY id",
    )?;
    let rows = statement.query_map(params![project], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: row.get(0)?,
        branch: row.get(1)?,
        worktree: path_from_bytes(row.get(2)?),
        workspace: row.get(3)?,
    })
}

fn branch_owner(
    connection: &Connection,
    project: &str,
    branch: &str,
    id: &str,
) -> Result<Option<String>, Failure> {
    Ok(connection
        .query_row(
            "SELECT id FROM tasks WHERE project_id = ?1 AND branch = ?2 AND id <> ?3",
            params![project, branch, id],
            |row| row.get(0),
        )
        .optional()?)
}

fn worktree_owner(
    connection: &Connection,
    worktree: &[u8],
    project: &str,
    id: &str,
) -> Result<Option<String>, Failure> {
    Ok(connection
        .query_row(
            "SELECT id FROM tasks WHERE worktree = ?1 AND NOT (project_id = ?2 AND id = ?3)",
            params![worktree, project, id],
            |row| row.get(0),
        )
        .optional()?)
}
