//! Environment diagnostics: which Leme actions this installation supports.
//!
//! Diagnosis has two halves. [`collect`] runs read-only probes against the
//! installed tools, the project and the state directory, and records what it
//! observed in [`Probes`]. [`classify`] turns those observations into a
//! [`Report`] without running anything, so the same observations always
//! produce the same report.
//!
//! Probes read only version output, Herdr's status and Herdr's bundled API
//! schema. They never run authentication commands or read credentials.

mod classify;
mod probe;

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;

use crate::config::Preferences;
use crate::git::Checkout;

pub use classify::{
    HERDR_OPERATIONS, MINIMUM_GIT, MINIMUM_RUST, TESTED_HERDR_PROTOCOL, Version, classify,
};
pub use probe::{collect, find_executable, run};

/// How long a single probe may run before it is stopped.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Everything the probes observed, before any judgement is made.
#[derive(Debug, Clone)]
pub struct Probes {
    /// `git --version`.
    pub git: ToolProbe,
    /// Repository discovery from the working directory.
    pub repository: Result<Checkout, String>,
    /// `leme.toml` from the main checkout; `None` without a repository.
    pub preferences: Option<Result<Preferences, String>>,
    /// The state database location.
    pub state: StateProbe,
    /// `herdr --version`.
    pub herdr: ToolProbe,
    /// `herdr status server --json`; `None` when Herdr could not be run.
    pub herdr_server: Option<Run>,
    /// `herdr api schema --json`; `None` when Herdr could not be run.
    pub herdr_schema: Option<Run>,
    /// `nvim --version`.
    pub neovim: ToolProbe,
    /// `claude --version`.
    pub claude: ToolProbe,
    /// `codex --version`.
    pub codex: ToolProbe,
    /// `gh --version`.
    pub gh: ToolProbe,
    /// `git-flow version`.
    pub git_flow: ToolProbe,
    /// `cargo --version`, needed only to build Leme.
    pub cargo: ToolProbe,
    /// `rustc --version`, needed only to build Leme.
    pub rustc: ToolProbe,
    /// `cc --version`, needed only to build Leme's bundled SQLite.
    pub cc: ToolProbe,
}

/// What looking up and running an executable revealed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolProbe {
    /// No executable of that name is on `PATH`.
    Missing,
    /// The executable was found at `path` and run.
    Found { path: PathBuf, run: Run },
}

/// The outcome of running a probe command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Run {
    /// The process exited on its own.
    Finished {
        success: bool,
        stdout: String,
        stderr: String,
    },
    /// The process was stopped after running for this long.
    TimedOut(Duration),
    /// The process could not be started.
    NotStarted(String),
}

/// What inspecting the state database location revealed, without creating
/// or migrating the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateProbe {
    /// Neither `XDG_STATE_HOME` nor `HOME` gives a location.
    NoLocation,
    /// The database's directory, or the nearest existing ancestor that would
    /// hold it, does not accept new files.
    Unwritable { path: PathBuf, reason: String },
    /// The database exists but could not be read.
    Unreadable { path: PathBuf, reason: String },
    /// The location is usable. `schema` is `None` when the database has not
    /// been created yet.
    Usable { path: PathBuf, schema: Option<i64> },
}

/// The diagnosis: which actions work, and the checks behind them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    /// Leme actions and whether this installation supports them.
    pub capabilities: Vec<Capability>,
    /// Checks of what Leme needs at run time.
    pub checks: Vec<Check>,
    /// Checks of what building Leme from source needs. They affect no action.
    pub build: Vec<Check>,
}

impl Report {
    /// The capability with this ID.
    pub fn capability(&self, id: &str) -> Option<&Capability> {
        self.capabilities
            .iter()
            .find(|capability| capability.id == id)
    }

    /// The run-time or build check with this ID.
    pub fn check(&self, id: &str) -> Option<&Check> {
        self.checks
            .iter()
            .chain(&self.build)
            .find(|check| check.id == id)
    }
}

/// One Leme action and whether its requirements are met.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Capability {
    pub id: &'static str,
    /// What the user can do when this capability is available.
    pub action: String,
    pub status: CapabilityStatus,
    /// IDs of the checks this action depends on.
    pub requires: Vec<&'static str>,
    /// Required checks that failed or could not run.
    pub blocked_by: Vec<&'static str>,
    /// Required checks whose compatibility could not be confirmed.
    pub uncertain: Vec<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CapabilityStatus {
    /// Every required check passed.
    Available,
    /// Nothing failed, but some compatibility could not be confirmed.
    Uncertain,
    /// A required check failed or could not run.
    Unavailable,
}

/// The result of checking one tool or resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    pub id: &'static str,
    /// Human-readable name of what was checked.
    pub name: &'static str,
    pub status: CheckStatus,
    /// What was observed.
    pub detail: String,
    /// The executable or file that was examined, when there is one.
    pub path: Option<PathBuf>,
    /// The version reported, when one was recognized.
    pub version: Option<String>,
    /// The manual step that would fix a failed or uncertain check.
    pub correction: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    /// The requirement is met.
    Ok,
    /// The requirement may be met, but Leme cannot confirm it.
    Uncertain,
    /// The requirement is not met.
    Failed,
    /// The check could not run because something it depends on failed.
    Skipped,
}
