//! Leme's operational state, kept in SQLite outside every checkout.
//!
//! The database survives the removal of any worktree. Its schema follows
//! `MIGRATIONS`, and `PRAGMA user_version` records how many of them have run.

mod project;
mod tasks;

pub use project::Project;
pub use tasks::{Task, TaskConflict};

use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, TransactionBehavior};

const MIGRATIONS: [&str; 2] = [
    include_str!("migrations/001_project.sql"),
    include_str!("migrations/002_task_workspace.sql"),
];

/// The schema version this Leme creates and understands.
pub const SCHEMA_VERSION: usize = MIGRATIONS.len();

/// How long to wait for another Leme process to finish writing.
const BUSY_TIMEOUT: Duration = Duration::from_secs(10);

/// An open, fully migrated state database.
#[derive(Debug)]
pub struct Store {
    connection: Connection,
    path: PathBuf,
}

impl Store {
    /// Opens the user's state database, creating it on first use.
    pub fn open_default() -> Result<Self, StateError> {
        Self::open(&default_path().ok_or(StateError::NoStateDirectory)?)
    }

    /// Opens the database at `path`, creating it and applying any pending
    /// migrations.
    pub fn open(path: &Path) -> Result<Self, StateError> {
        if let Some(directory) = path.parent() {
            fs::create_dir_all(directory).map_err(|source| StateError::Directory {
                path: directory.to_path_buf(),
                source,
            })?;
        }
        let mut store = Self {
            connection: Connection::open(path).map_err(|source| StateError::Database {
                path: path.to_path_buf(),
                source,
            })?,
            path: path.to_path_buf(),
        };
        store
            .configure()
            .and_then(|()| store.migrate())
            .map_err(|error| store.failure(error))?;
        Ok(store)
    }

    fn configure(&self) -> Result<(), Failure> {
        self.connection.busy_timeout(BUSY_TIMEOUT)?;
        self.connection.pragma_update(None, "foreign_keys", true)?;
        Ok(())
    }

    /// Applies pending migrations in one transaction, so a failure leaves the
    /// schema as it was. Taking the write lock first makes concurrent
    /// processes wait here instead of migrating twice.
    fn migrate(&mut self) -> Result<(), Failure> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let found: i64 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let applied = usize::try_from(found)
            .ok()
            .filter(|applied| *applied <= MIGRATIONS.len())
            .ok_or(Failure::NewerSchema { found })?;
        for (index, migration) in MIGRATIONS.iter().enumerate().skip(applied) {
            transaction.execute_batch(migration)?;
            transaction.pragma_update(None, "user_version", index as i64 + 1)?;
        }
        transaction.commit()?;
        Ok(())
    }

    fn failure(&self, failure: Failure) -> StateError {
        let path = self.path.clone();
        match failure {
            Failure::Database(source) => StateError::Database { path, source },
            Failure::NewerSchema { found } => StateError::NewerSchema {
                path,
                found,
                supported: MIGRATIONS.len(),
            },
        }
    }
}

/// A failure inside an open database, before the path is attached.
enum Failure {
    Database(rusqlite::Error),
    NewerSchema { found: i64 },
}

impl From<rusqlite::Error> for Failure {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

/// Reads the schema version of the database at `path` without creating,
/// migrating or writing to it. Returns `None` when no database exists yet.
pub fn existing_schema_version(path: &Path) -> Result<Option<i64>, StateError> {
    if !path
        .try_exists()
        .map_err(|source| StateError::Inaccessible {
            path: path.to_path_buf(),
            source,
        })?
    {
        return Ok(None);
    }
    let database = |source| StateError::Database {
        path: path.to_path_buf(),
        source,
    };
    let connection =
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(database)?;
    connection.busy_timeout(BUSY_TIMEOUT).map_err(database)?;
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map(Some)
        .map_err(database)
}

/// `$XDG_STATE_HOME/leme/leme.db`, or `~/.local/state/leme/leme.db`.
pub fn default_path() -> Option<PathBuf> {
    // The XDG specification requires ignoring a relative path.
    let directory = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            env::var_os("HOME")
                .filter(|home| !home.is_empty())
                .map(|home| PathBuf::from(home).join(".local/state"))
        })?;
    Some(directory.join("leme/leme.db"))
}

/// Why the state database could not be used.
#[derive(Debug)]
pub enum StateError {
    /// Neither `XDG_STATE_HOME` nor `HOME` gives a location for the database.
    NoStateDirectory,
    /// The directory holding the database could not be created.
    Directory { path: PathBuf, source: io::Error },
    /// The database location could not be examined.
    Inaccessible { path: PathBuf, source: io::Error },
    /// SQLite reported a failure.
    Database {
        path: PathBuf,
        source: rusqlite::Error,
    },
    /// The database was written by a newer version of Leme.
    NewerSchema {
        path: PathBuf,
        found: i64,
        supported: usize,
    },
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoStateDirectory => write!(
                f,
                "cannot locate the state directory; set XDG_STATE_HOME or HOME"
            ),
            Self::Directory { path, source } => {
                write!(f, "cannot create {}: {source}", path.display())
            }
            Self::Inaccessible { path, source } => {
                write!(f, "cannot access {}: {source}", path.display())
            }
            Self::Database { path, source } => {
                write!(f, "cannot use {}: {source}", path.display())
            }
            Self::NewerSchema {
                path,
                found,
                supported,
            } => write!(
                f,
                "{} has schema version {found}, but this Leme supports up to {supported}; \
                 update Leme",
                path.display()
            ),
        }
    }
}

impl std::error::Error for StateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Directory { source, .. } | Self::Inaccessible { source, .. } => Some(source),
            Self::Database { source, .. } => Some(source),
            Self::NoStateDirectory | Self::NewerSchema { .. } => None,
        }
    }
}
