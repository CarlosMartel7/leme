//! The `leme feature open` command.
//!
//! Everything is resolved before Herdr is asked for anything: the project,
//! the task and its branch, the configured integration base and the checkout
//! that has the branch. Only then is the task recorded and its workspace
//! opened, so a request that cannot succeed leaves no trace.

use std::env;
use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Args, Subcommand};
use serde::Serialize;

use super::{Error, parse_timeout};
use crate::config::Preferences;
use crate::diagnostics::Check;
use crate::git::{self, InvalidBranch, Worktree};
use crate::herdr::{self, Herdr, HerdrError, OpenedWorkspace};
use crate::state::{Store, Task, TaskConflict};

#[derive(Debug, Subcommand)]
pub enum FeatureCommand {
    /// Open a task's feature checkout in its own Herdr workspace
    ///
    /// The task is recorded in Leme's state database with its branch, the
    /// checkout that has the branch, and the Herdr workspace opened for it.
    /// The first time a task is opened, --branch names its branch; after
    /// that the recorded branch is used.
    ///
    /// The branch must already be checked out in a linked worktree of this
    /// repository. Herdr keeps one workspace per checkout, so opening the same
    /// task again focuses its existing workspace. Git is only read.
    Open(OpenArgs),
}

#[derive(Debug, Args)]
pub struct OpenArgs {
    /// Task ID, such as a ticket or card key
    #[arg(value_name = "TASK")]
    task: String,

    /// Feature branch of the task; required the first time the task is opened
    #[arg(long, value_name = "NAME")]
    branch: Option<String>,

    /// Print the result as JSON
    #[arg(long)]
    json: bool,

    /// Seconds each Herdr request may take before it counts as failed
    #[arg(long, value_name = "SECONDS", default_value = "10", value_parser = parse_timeout)]
    timeout: Duration,
}

/// Why a feature could not be opened, before or apart from Herdr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeatureError {
    /// The task ID is empty or blank.
    EmptyTask,
    /// The task is not registered and no branch was given.
    BranchRequired { task: String },
    /// The branch name is not a valid Git branch name.
    InvalidBranch(InvalidBranch),
    /// The task conflicts with another task's branch or checkout.
    Conflict(TaskConflict),
    /// No checkout of the repository has the branch.
    NoCheckout { branch: String, repository: PathBuf },
    /// The branch is checked out in the main checkout, which is shared by
    /// the whole project rather than owned by the task.
    InMainCheckout { branch: String, checkout: PathBuf },
    /// Git records a checkout of the branch whose directory is missing.
    MissingCheckout { branch: String, checkout: PathBuf },
}

impl fmt::Display for FeatureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyTask => write!(f, "the task ID must not be empty"),
            Self::BranchRequired { task } => write!(
                f,
                "task {task} is not registered yet; pass --branch with its feature branch"
            ),
            Self::InvalidBranch(InvalidBranch { name }) => {
                write!(f, "`{name}` is not a valid Git branch name")
            }
            Self::Conflict(conflict) => conflict.fmt(f),
            Self::NoCheckout { branch, repository } => write!(
                f,
                "branch {branch} is not checked out in any worktree of {}; create one with \
                 `git worktree add <path> {branch}` and run this command again",
                repository.display()
            ),
            Self::InMainCheckout { branch, checkout } => write!(
                f,
                "branch {branch} is checked out in the main checkout {}; a feature needs its \
                 own linked worktree, so switch the main checkout to another branch first",
                checkout.display()
            ),
            Self::MissingCheckout { branch, checkout } => write!(
                f,
                "Git records {} as the checkout of {branch}, but the directory is missing; \
                 restore it, or run `git worktree prune` to forget it",
                checkout.display()
            ),
        }
    }
}

impl std::error::Error for FeatureError {}

impl From<FeatureError> for Error {
    fn from(error: FeatureError) -> Self {
        Self::Feature(error)
    }
}

/// Everything `leme feature open` reports. Text and JSON render this value.
#[derive(Debug, Serialize)]
struct Report {
    project_id: String,
    task: String,
    branch: String,
    /// The integration branch the project's features start from.
    base: String,
    checkout: PathBuf,
    workspace: OpenedWorkspace,
    /// The checkout recorded before, when Git now reports another one.
    previous_checkout: Option<PathBuf>,
    /// The workspace recorded before, when Herdr now reports another one.
    previous_workspace: Option<String>,
    /// Herdr checks whose compatibility `leme doctor` could not confirm.
    unconfirmed: Vec<Check>,
}

pub fn run(command: &FeatureCommand) -> Result<(), Error> {
    match command {
        FeatureCommand::Open(args) => open(args),
    }
}

fn open(args: &OpenArgs) -> Result<(), Error> {
    let task = args.task.trim();
    if task.is_empty() {
        return Err(FeatureError::EmptyTask.into());
    }
    let directory = env::current_dir().map_err(Error::CurrentDirectory)?;
    let repository = git::discover(&directory)?.repository;
    let base = Preferences::load(&repository)?.integration_branch;
    let mut store = Store::open_default()?;
    let project = store.project(&repository)?;
    let registered = project.tasks.iter().find(|recorded| recorded.id == task);
    let branch = branch(task, registered, args.branch.as_deref())?;
    if let Err(invalid) = git::check_branch_name(&repository, &branch)? {
        return Err(FeatureError::InvalidBranch(invalid).into());
    }
    let checkout = checkout(&git::worktrees(&repository)?, &branch, &repository)?;

    let herdr = Herdr::locate(args.timeout).ok_or(HerdrError::NotInstalled)?;
    let readiness = herdr.readiness();
    if let Some(check) = readiness.blocker() {
        return Err(HerdrError::NotReady(Box::new(check.clone())).into());
    }

    let previous = store
        .register_task(&project.id, task, &branch, &checkout)?
        .map_err(FeatureError::Conflict)?;
    let workspace = herdr::open_worktree(&herdr, &repository, &checkout)?;
    store.record_workspace(&project.id, task, &workspace.id)?;

    let report = Report {
        project_id: project.id,
        task: task.to_owned(),
        branch,
        base,
        previous_checkout: previous
            .as_ref()
            .map(|task| task.worktree.clone())
            .filter(|recorded| *recorded != checkout),
        previous_workspace: previous
            .and_then(|task| task.workspace)
            .filter(|recorded| *recorded != workspace.id),
        checkout,
        workspace,
        unconfirmed: readiness.uncertain().cloned().collect(),
    };
    let output = if args.json {
        serde_json::to_string_pretty(&report).map_err(Error::Json)?
    } else {
        render_text(&report)
    };
    writeln!(io::stdout(), "{output}").map_err(Error::Output)
}

/// The task's branch: the registered one, which `requested` may only repeat,
/// or `requested` for a task opened for the first time.
fn branch(
    task: &str,
    registered: Option<&Task>,
    requested: Option<&str>,
) -> Result<String, FeatureError> {
    match (registered, requested) {
        (Some(recorded), Some(requested)) if recorded.branch != requested => {
            Err(FeatureError::Conflict(TaskConflict::TaskHasOtherBranch {
                task: task.to_owned(),
                recorded: recorded.branch.clone(),
                requested: requested.to_owned(),
            }))
        }
        (Some(recorded), _) => Ok(recorded.branch.clone()),
        (None, Some(requested)) => Ok(requested.to_owned()),
        (None, None) => Err(FeatureError::BranchRequired {
            task: task.to_owned(),
        }),
    }
}

/// The linked worktree that has `branch` checked out.
fn checkout(
    worktrees: &[Worktree],
    branch: &str,
    repository: &Path,
) -> Result<PathBuf, FeatureError> {
    let Some(worktree) = git::checkout_of(worktrees, branch) else {
        return Err(FeatureError::NoCheckout {
            branch: branch.to_owned(),
            repository: repository.to_path_buf(),
        });
    };
    let found = || (branch.to_owned(), worktree.path.clone());
    if worktree.main {
        let (branch, checkout) = found();
        return Err(FeatureError::InMainCheckout { branch, checkout });
    }
    if worktree.prunable || !worktree.path.is_dir() {
        let (branch, checkout) = found();
        return Err(FeatureError::MissingCheckout { branch, checkout });
    }
    Ok(worktree.path.clone())
}

fn render_text(report: &Report) -> String {
    let workspace = &report.workspace;
    let mut lines = vec![
        format!("Task:      {}", report.task),
        format!("Branch:    {}", report.branch),
        format!("Base:      {} (integration branch)", report.base),
        format!("Checkout:  {}", report.checkout.display()),
        format!(
            "Workspace: {}{}  ({}, focused)",
            workspace.id,
            workspace
                .label
                .as_deref()
                .map(|label| format!("  {label}"))
                .unwrap_or_default(),
            if workspace.already_open {
                "already open"
            } else {
                "opened"
            }
        ),
    ];
    if let Some(previous) = &report.previous_checkout {
        lines.push(format!(
            "The task was recorded at {}; Git now has the branch at the checkout above.",
            previous.display()
        ));
    }
    if let Some(previous) = &report.previous_workspace {
        lines.push(format!(
            "The recorded workspace {previous} is no longer this checkout's; recorded {} instead.",
            workspace.id
        ));
    }
    for check in &report.unconfirmed {
        lines.push(format!("Unconfirmed: {}: {}", check.name, check.detail));
    }
    lines.join("\n")
}
