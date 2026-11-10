//! The `leme dashboard` screen.
//!
//! The screen is split into three independent parts:
//!
//! - [`collect`] gathers a [`Snapshot`] of the project from Git, the project's
//!   preferences and the state database. It only reads Git and never starts
//!   an agent.
//! - [`app::App`] holds what the screen shows and reacts to input. It never
//!   touches the terminal or collects data itself; it asks for a collection by
//!   returning [`app::Effect::Collect`].
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
use crate::state::{StateError, Store, Task};

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
}

/// Reads a snapshot of the project containing `directory`.
///
/// Git is only read. The project is registered in the state database on first
/// use, exactly as `leme status` does.
pub fn collect(directory: &Path) -> Result<Snapshot, CollectError> {
    let checkout = git::discover(directory)?;
    let preferences = Preferences::load(&checkout.repository)?;
    let project = Store::open_default()?.project(&checkout.repository)?;
    Ok(Snapshot {
        project_id: project.id,
        checkout,
        preferences,
        tasks: project.tasks,
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
