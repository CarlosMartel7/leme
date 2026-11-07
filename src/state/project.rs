//! Durable project identity and its registered tasks.
//!
//! The current branch and checkout always come from Git. Only what Git cannot
//! answer is stored here.

use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, TransactionBehavior, params};
use serde::Serialize;

use super::{Failure, StateError, Store};

/// A project's durable record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// Identifier that stays the same across checkouts and restarts.
    pub id: String,
    /// The project's registered tasks, ordered by ID.
    pub tasks: Vec<Task>,
}

/// A task registered to a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Task {
    pub id: String,
    pub branch: String,
    /// Root of the task's linked worktree.
    pub worktree: PathBuf,
}

impl Store {
    /// Returns the project whose main checkout is `repository`, registering
    /// it on first use.
    ///
    /// The unique key and the write lock make concurrent first lookups resolve
    /// to one record.
    pub fn project(&mut self, repository: &Path) -> Result<Project, StateError> {
        self.resolve(repository)
            .map_err(|error| self.failure(error))
    }

    fn resolve(&mut self, repository: &Path) -> Result<Project, Failure> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let key = repository.as_os_str().as_bytes();
        transaction.execute(
            "INSERT INTO projects (id, repository)
             VALUES (lower(hex(randomblob(16))), ?1)
             ON CONFLICT (repository) DO NOTHING",
            params![key],
        )?;
        let id = project_id(&transaction, key)?;
        let tasks = tasks(&transaction, &id)?;
        transaction.commit()?;
        Ok(Project { id, tasks })
    }
}

fn project_id(connection: &Connection, repository: &[u8]) -> Result<String, Failure> {
    Ok(connection.query_row(
        "SELECT id FROM projects WHERE repository = ?1",
        params![repository],
        |row| row.get(0),
    )?)
}

fn tasks(connection: &Connection, project_id: &str) -> Result<Vec<Task>, Failure> {
    let mut statement = connection
        .prepare("SELECT id, branch, worktree FROM tasks WHERE project_id = ?1 ORDER BY id")?;
    let rows = statement.query_map(params![project_id], |row| {
        Ok(Task {
            id: row.get(0)?,
            branch: row.get(1)?,
            worktree: path_from_bytes(row.get(2)?),
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(OsString::from_vec(bytes))
}
