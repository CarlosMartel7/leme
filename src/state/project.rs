//! Durable project identity and its registered tasks.
//!
//! The current branch and checkout always come from Git. Only what Git cannot
//! answer is stored here.

use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use super::tasks::{self, Task};
use super::{Failure, StateError, Store};
use rusqlite::{Connection, TransactionBehavior, params};

/// A project's durable record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// Identifier that stays the same across checkouts and restarts.
    pub id: String,
    /// The project's registered tasks, ordered by ID.
    pub tasks: Vec<Task>,
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
        let tasks = tasks::tasks(&transaction, &id)?;
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

pub(super) fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(OsString::from_vec(bytes))
}
