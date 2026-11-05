//! Command-line interface and command dispatch.

mod status;

use std::fmt;
use std::io;

use clap::{Parser, Subcommand};

use crate::git::DiscoveryError;

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
    /// Show the repository, branch and checkout for the current directory
    Status(status::StatusArgs),
}

/// A failure to report to the user before exiting.
#[derive(Debug)]
pub enum Error {
    /// The working directory could not be read.
    CurrentDirectory(io::Error),
    /// The Git checkout could not be identified.
    Discovery(DiscoveryError),
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
            Self::Json(error) => Some(error),
        }
    }
}

impl From<DiscoveryError> for Error {
    fn from(error: DiscoveryError) -> Self {
        Self::Discovery(error)
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
