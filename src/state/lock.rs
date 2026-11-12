//! Which dashboard controls a project's timer.
//!
//! Exactly one dashboard per project owns the timer: only it shows the clock
//! and timer, accepts timer keys and writes the saved timer. Ownership is an
//! exclusive `flock(2)` on `locks/<project-id>.lock` beside the state
//! database, so it is outside every checkout and shared by all checkouts of
//! the repository, which resolve to one project ID.
//!
//! The kernel holds the lock for as long as the open file exists and drops it
//! when the process exits, normally or not. Leme never inspects or deletes
//! the lock file to decide ownership; the file only gives the lock a name.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::{StateError, Store};

/// Directory beside the state database that holds the lock files.
const LOCK_DIRECTORY: &str = "locks";

/// An exclusive hold on one project's timer, released when dropped or when
/// the process ends.
#[derive(Debug)]
pub struct TimerLock {
    /// Kept open only to hold the lock.
    _file: File,
    project_id: String,
}

impl TimerLock {
    /// The project whose timer this lock controls.
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
}

impl Store {
    /// Takes the exclusive lock on `project_id`'s timer without waiting.
    ///
    /// Returns `None` when another holder, in this or another process, has
    /// it. Each call opens the lock file anew, so two calls in one process
    /// exclude each other just as two processes do.
    pub fn lock_timer(&self, project_id: &str) -> Result<Option<TimerLock>, StateError> {
        let path = self.lock_path(project_id)?;
        let failure = |source| StateError::Lock {
            path: path.clone(),
            source,
        };
        if let Some(directory) = path.parent() {
            fs::create_dir_all(directory).map_err(failure)?;
        }
        // The descriptor is close-on-exec, so Git and other child processes
        // never inherit the lock.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(failure)?;
        if !try_lock_exclusive(&file).map_err(failure)? {
            return Ok(None);
        }
        Ok(Some(TimerLock {
            _file: file,
            project_id: project_id.to_owned(),
        }))
    }

    fn lock_path(&self, project_id: &str) -> Result<PathBuf, StateError> {
        let directory = self.path.parent().map_or_else(
            || PathBuf::from(LOCK_DIRECTORY),
            |parent| parent.join(LOCK_DIRECTORY),
        );
        let path = directory.join(format!("{project_id}.lock"));
        // Project IDs are generated hex strings; anything else must not
        // become a path component.
        let valid = !project_id.is_empty()
            && project_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
        if !valid {
            return Err(StateError::Lock {
                path,
                source: io::Error::new(io::ErrorKind::InvalidInput, "invalid project ID"),
            });
        }
        Ok(path)
    }
}

/// `flock(LOCK_EX | LOCK_NB)`: `Ok(false)` when another open file holds it.
fn try_lock_exclusive(file: &File) -> io::Result<bool> {
    loop {
        // SAFETY: the descriptor belongs to `file`, which outlives the call.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::EWOULDBLOCK) => return Ok(false),
            Some(libc::EINTR) => continue,
            _ => return Err(error),
        }
    }
}

/// Whether a dashboard may control its project's timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    /// This dashboard holds the lock.
    Owner,
    /// Another dashboard holds it, or held it when this one asked.
    NotOwner,
}

/// One dashboard's timer ownership, shared by its collections (which claim
/// it) and its timer writer (which checks it before every write).
///
/// Ownership is decided once, at the first claim that reaches the lock, and
/// kept for the life of the dashboard: a non-owner does not take over when
/// the owner exits; it becomes owner by being reopened. The lock is released
/// only when every clone has been dropped, so a writer holding a clone keeps
/// ownership until its last queued write has finished.
#[derive(Debug, Clone, Default)]
pub struct TimerOwnership {
    slot: Arc<Mutex<Slot>>,
}

#[derive(Debug, Default)]
enum Slot {
    #[default]
    Undecided,
    Owner(TimerLock),
    NotOwner,
}

impl TimerOwnership {
    pub fn new() -> Self {
        Self::default()
    }

    /// Decides, the first time, whether this dashboard owns `project_id`'s
    /// timer, and returns the same decision afterwards.
    ///
    /// A failure to use the lock decides nothing, so a later claim retries.
    /// A dashboard that owns another project's timer is not owner of this
    /// one; that only happens if the state database changed underneath it.
    pub fn claim(&self, store: &Store, project_id: &str) -> Result<Claim, StateError> {
        let mut slot = self.slot();
        match &*slot {
            Slot::Owner(lock) if lock.project_id() == project_id => return Ok(Claim::Owner),
            Slot::Owner(_) | Slot::NotOwner => return Ok(Claim::NotOwner),
            Slot::Undecided => {}
        }
        *slot = match store.lock_timer(project_id)? {
            Some(lock) => Slot::Owner(lock),
            None => Slot::NotOwner,
        };
        Ok(match &*slot {
            Slot::Owner(_) => Claim::Owner,
            Slot::Undecided | Slot::NotOwner => Claim::NotOwner,
        })
    }

    /// Whether this dashboard holds the lock on `project_id`'s timer.
    pub fn owns(&self, project_id: &str) -> bool {
        matches!(&*self.slot(), Slot::Owner(lock) if lock.project_id() == project_id)
    }

    fn slot(&self) -> MutexGuard<'_, Slot> {
        // The slot is always in a valid state, even after a panic elsewhere.
        self.slot.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
