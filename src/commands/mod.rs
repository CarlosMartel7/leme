//! Command-line interface and command dispatch.

mod status;

use std::fmt;
use std::io;

use clap::{Parser, Subcommand};

use crate::config::ConfigError;
use crate::git::DiscoveryError;
use crate::state::StateError;

#[derive(Debug, Parser)]
#[command(
    name = "leme",
    version,
    about = "Organize AI-assisted work on a project"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show the project, checkout, preferences and tasks for the current directory
    ///
    /// Preferences come from leme.toml in the repository's main checkout, so
    /// every linked worktree reports the same values. Defaults apply when the
    /// file or a setting is absent.
    ///
    /// The project ID and tasks come from Leme's state database, which is
    /// created on first use at $XDG_STATE_HOME/leme/leme.db, or
    /// ~/.local/state/leme/leme.db. The Git repository is not modified.
    Status(status::StatusArgs),
}

/// A failure to report to the user before exiting.
#[derive(Debug)]
pub enum Error {
    /// The working directory could not be read.
    CurrentDirectory(io::Error),
    /// The Git checkout could not be identified.
    Discovery(DiscoveryError),
    /// The project's preferences could not be read.
    Config(ConfigError),
    /// The project's state could not be read or recorded.
    State(StateError),
    /// The result could not be encoded as JSON.
    Json(serde_json::Error),
    /// The result could not be written to standard output.
    Output(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CurrentDirectory(error) => {
                write!(f, "cannot read the current directory: {error}")
            }
            Self::Discovery(error) => error.fmt(f),
            Self::Config(error) => error.fmt(f),
            Self::State(error) => error.fmt(f),
            Self::Json(error) => write!(f, "cannot encode the result as JSON: {error}"),
            Self::Output(error) => write!(f, "cannot write the result: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CurrentDirectory(error) | Self::Output(error) => Some(error),
            Self::Discovery(error) => Some(error),
            Self::Config(error) => Some(error),
            Self::State(error) => Some(error),
            Self::Json(error) => Some(error),
        }
    }
}

impl From<DiscoveryError> for Error {
    fn from(error: DiscoveryError) -> Self {
        Self::Discovery(error)
    }
}

impl From<ConfigError> for Error {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<StateError> for Error {
    fn from(error: StateError) -> Self {
        Self::State(error)
    }
}

/// Parses the process arguments and runs the selected command.
///
/// Invalid arguments and help requests exit through Clap.
pub fn run() -> Result<(), Error> {
    match Cli::parse().command {
        Command::Status(args) => status::run(&args),
    }
}
