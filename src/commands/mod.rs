//! Command-line interface and command dispatch.

mod dashboard;
mod doctor;
mod herdr_context;
mod skills;
mod status;

use std::fmt;
use std::io;
use std::time::Duration;

use clap::{Parser, Subcommand};

use crate::config::ConfigError;
use crate::git::DiscoveryError;
use crate::herdr::HerdrError;
use crate::skills::{CatalogError, InstallError, Outcome};
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

    /// Check which Leme actions this installation supports
    ///
    /// Looks for Git, Herdr and its server, Neovim, the Claude Code and Codex
    /// CLIs, gh and git-flow, then checks the current project, its
    /// preferences and the state directory. Each action is reported as
    /// available, uncertain (compatibility could not be confirmed) or
    /// unavailable, with the manual correction for each failed check. Rust,
    /// Cargo and a C compiler are reported separately: they are needed only to
    /// build Leme.
    ///
    /// Doctor only reads: it runs version and status commands, never installs
    /// or configures a tool, and changes no repository file. The only write is
    /// an empty probe file, removed at once, that shows whether the state
    /// directory accepts new files. Missing tools are part of the diagnosis,
    /// so the exit status is 0 whenever the report is printed.
    Doctor(doctor::DoctorArgs),
    /// Open the project dashboard in this terminal
    ///
    /// Shows the project, its checkout and its registered tasks. Press ? for
    /// keys, r to refresh and q to quit. Refreshing only reads Git and Leme's
    /// state database; it never changes the repository or starts an agent.
    Dashboard(dashboard::DashboardArgs),

    /// Inspect how Herdr sees this Leme process
    #[command(subcommand)]
    Herdr(HerdrCommand),

    /// Install the workflow skills bundled with Leme
    #[command(subcommand)]
    Skills(SkillsCommand),
}

#[derive(Debug, Subcommand)]
enum SkillsCommand {
    /// Copy the bundled skills into an agent's user skills directory
    ///
    /// Installs every bundled skill, helpers included, as a complete
    /// directory: $HOME/.agents/skills/<name> for Codex and
    /// ~/.claude/skills/<name> for Claude Code ($CLAUDE_CONFIG_DIR/skills when
    /// that is set). The skills come from this executable, so no network or
    /// source checkout is needed.
    ///
    /// Every destination is checked before anything is written. Skills that
    /// already match are left alone, and other skills in the directory are
    /// never touched. A directory Leme did not install, or a Leme copy changed
    /// since, is a conflict: the run stops without writing and the exit
    /// status is 1. Leme records what it installed in .leme-skills.toml in
    /// each skills directory.
    ///
    /// Installing never runs a skill and does not change agent settings,
    /// permissions, authentication or connectors.
    Install(skills::InstallArgs),
}

#[derive(Debug, Subcommand)]
enum HerdrCommand {
    /// Show the Herdr workspace, tab and pane this command was run from
    ///
    /// Herdr gives every process in a pane the IDs of its workspace, tab and
    /// pane. This command asks Herdr about those exact IDs, so it reports
    /// where it was started even if focus has since moved elsewhere. Pass
    /// --pane, --tab or --workspace to look up other IDs instead; they
    /// replace the inherited IDs entirely.
    ///
    /// Herdr is first checked as leme doctor checks it, and nothing is sent
    /// when a check fails. Only read operations are used: pane.current,
    /// workspace.list and tab.list.
    Context(herdr_context::ContextArgs),
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
    /// The dashboard was started without an interactive terminal.
    NotATerminal,
    /// The terminal could not be set up, read or drawn on.
    Terminal(io::Error),
    /// Herdr could not be used or did not resolve the context.
    Herdr(HerdrError),
    /// The bundled skills failed validation.
    Catalog(CatalogError),
    /// The skills could not be installed.
    Install(InstallError),
    /// The skills report was printed, but not every target was completed.
    SkillsIncomplete(Outcome),
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
            Self::NotATerminal => write!(
                f,
                "the dashboard needs an interactive terminal; run it in a terminal or Herdr pane"
            ),
            Self::Terminal(error) => write!(f, "cannot use the terminal: {error}"),
            Self::Herdr(error) => error.fmt(f),
            Self::Catalog(error) => error.fmt(f),
            Self::Install(error) => error.fmt(f),
            Self::SkillsIncomplete(Outcome::Conflict) => {
                write!(f, "some skills conflict with existing directories")
            }
            Self::SkillsIncomplete(_) => write!(f, "the skills were not installed everywhere"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CurrentDirectory(error) | Self::Output(error) | Self::Terminal(error) => {
                Some(error)
            }
            Self::Discovery(error) => Some(error),
            Self::Config(error) => Some(error),
            Self::State(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Herdr(error) => Some(error),
            Self::Catalog(error) => Some(error),
            Self::Install(error) => Some(error),
            Self::NotATerminal | Self::SkillsIncomplete(_) => None,
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

impl From<HerdrError> for Error {
    fn from(error: HerdrError) -> Self {
        Self::Herdr(error)
    }
}

/// Longest timeout accepted for a tool or Herdr request, in seconds.
const MAXIMUM_TIMEOUT: f64 = 600.0;

/// Parses a `--timeout` value in seconds.
fn parse_timeout(text: &str) -> Result<Duration, String> {
    let seconds: f64 = text
        .parse()
        .map_err(|_| format!("`{text}` is not a number of seconds"))?;
    if !(seconds > 0.0 && seconds <= MAXIMUM_TIMEOUT) {
        return Err(format!(
            "the timeout must be more than 0 and at most {MAXIMUM_TIMEOUT} seconds"
        ));
    }
    Ok(Duration::from_secs_f64(seconds))
}

/// Parses the process arguments and runs the selected command.
///
/// Invalid arguments and help requests exit through Clap.
pub fn run() -> Result<(), Error> {
    match Cli::parse().command {
        Command::Status(args) => status::run(&args),
        Command::Doctor(args) => doctor::run(&args),
        Command::Dashboard(args) => dashboard::run(&args),
        Command::Herdr(HerdrCommand::Context(args)) => herdr_context::run(&args),
        Command::Skills(SkillsCommand::Install(args)) => skills::run(&args),
    }
}
