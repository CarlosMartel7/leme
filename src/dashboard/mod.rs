//! The `leme dashboard` screen.
//!
//! The screen is split into three independent parts:
//!
//! - [`collect`] gathers a [`Snapshot`] of the project from Git, the project's
//!   preferences and the state database. It only reads Git and never starts
//!   an agent. It also claims the project's timer lock, and reads the saved
//!   timer only when this dashboard owns it.
//! - [`app::App`] holds what the screen shows and reacts to input. It never
//!   touches the terminal, reads a clock or collects data itself; it asks for
//!   a collection or a timer write by returning an [`app::Effect`], and
//!   learns the time from the inputs it is given.
//! - [`view::render`] draws an `App` into a Ratatui frame, with the local
//!   time from [`clock`] in its header.
//!
//! The `dashboard` command connects these to a real terminal.

pub mod app;
pub mod clock;
pub mod view;

use std::fmt;
use std::path::Path;

use crate::config::{ConfigError, Preferences};
use crate::git::{self, Checkout, DiscoveryError};
use crate::state::{Claim, StateError, Store, Task, TimerOwnership};
use crate::time::pomodoro::Saved;

/// Everything the dashboard shows about a project at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// The project's durable ID from the state database.
    pub project_id: String,
    /// The repository, branch and checkout the dashboard was opened from.
    pub checkout: Checkout,
    /// Preferences from the project's main checkout.
    pub preferences: Preferences,
    /// Tasks registered to the project, ordered by ID.
    pub tasks: Vec<Task>,
    /// Whether this dashboard controls the project's timer, with the saved
    /// timer when it does.
    pub timer: TimerAccess,
}

/// This dashboard's access to the project's focus/break timer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimerAccess {
    /// This dashboard holds the project's timer lock. The saved timer, read
    /// under the lock, is restored from the first such snapshot only;
    /// afterwards the timer on screen is the reference.
    Owned(Option<Saved>),
    /// Another dashboard of the project held the lock when this one claimed
    /// it. This dashboard never shows or writes the timer.
    HeldElsewhere,
    /// The lock could not be used, for this reason. The next collection
    /// tries again.
    Unavailable(String),
}

/// Reads a snapshot of the project containing `directory` from the user's
/// state database.
///
/// Git is only read. The project is registered in the state database on first
/// use, exactly as `leme status` does. The project's timer lock is claimed
/// through `ownership` before the saved timer is read.
pub fn collect(directory: &Path, ownership: &TimerOwnership) -> Result<Snapshot, CollectError> {
    collect_with(directory, Store::open_default, ownership)
}

/// [`collect`] with the state database opened by `open_store`.
pub fn collect_with(
    directory: &Path,
    open_store: impl FnOnce() -> Result<Store, StateError>,
    ownership: &TimerOwnership,
) -> Result<Snapshot, CollectError> {
    let checkout = git::discover(directory)?;
    let preferences = Preferences::load(&checkout.repository)?;
    let mut store = open_store()?;
    let project = store.project(&checkout.repository)?;
    let timer = match ownership.claim(&store, &project.id) {
        Ok(Claim::Owner) => TimerAccess::Owned(store.timer(&project.id)?),
        Ok(Claim::NotOwner) => TimerAccess::HeldElsewhere,
        Err(error) => TimerAccess::Unavailable(error.to_string()),
    };
    Ok(Snapshot {
        project_id: project.id,
        checkout,
        preferences,
        tasks: project.tasks,
        timer,
    })
}

/// Why a snapshot could not be collected.
#[derive(Debug)]
pub enum CollectError {
    /// The Git checkout could not be identified.
    Discovery(DiscoveryError),
    /// The project's preferences could not be read.
    Config(ConfigError),
    /// The project's state could not be read or recorded.
    State(StateError),
}

impl fmt::Display for CollectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(error) => error.fmt(f),
            Self::Config(error) => error.fmt(f),
            Self::State(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for CollectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Discovery(error) => Some(error),
            Self::Config(error) => Some(error),
            Self::State(error) => Some(error),
        }
    }
}

impl From<DiscoveryError> for CollectError {
    fn from(error: DiscoveryError) -> Self {
        Self::Discovery(error)
    }
}

impl From<ConfigError> for CollectError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<StateError> for CollectError {
    fn from(error: StateError) -> Self {
        Self::State(error)
    }
}
