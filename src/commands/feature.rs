//! The `leme feature open` command.
//!
//! Everything is resolved before anything changes: the project, the task
//! and its branch, the configured integration base, and either the checkout
//! that has the branch or the checkout to create. Creating a branch or a
//! worktree needs the user's authorization. Only then is the task recorded,
//! its workspace opened and its default tabs laid out. Each tab is recorded
//! as it opens, so an interrupted setup resumes with the missing tabs.

use std::env;
use std::fmt;
use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Args, Subcommand};
use serde::Serialize;

use super::{Error, parse_timeout};
use crate::config::Preferences;
use crate::diagnostics::{self, Check};
use crate::git::{self, InvalidBranch, Worktree};
use crate::herdr::{self, Herdr, HerdrError, NewCheckout, OpenedWorkspace, TabRole};
use crate::state::{Store, Task, TaskConflict};

#[derive(Debug, Subcommand)]
pub enum FeatureCommand {
    /// Open a task in its own feature checkout and Herdr workspace
    ///
    /// The first time a task is opened, --branch names its feature branch;
    /// after that the recorded branch is used. When the branch is already
    /// checked out in a linked worktree, Herdr opens or focuses that
    /// checkout's workspace. Otherwise Leme shows the branch, the base and the
    /// path it would create and asks before changing Git (or pass --yes): a
    /// new branch starts from the project's integration branch, whichever
    /// checkout the command runs in.
    ///
    /// The workspace gets three tabs in the checkout: agent (a shell left for
    /// the agent launcher), neovim and tests. Each tab is recorded as it opens,
    /// so running the command again after an interruption adds only the
    /// missing tabs and keeps the checkout and its files.
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

    /// Where to create the checkout when the branch has none; defaults to
    /// ~/.herdr/worktrees/<repository>/<branch with / as ->
    #[arg(long, value_name = "PATH")]
    path: Option<PathBuf>,

    /// Create the branch and worktree without asking
    #[arg(long)]
    yes: bool,

    /// Print the result as JSON
    #[arg(long)]
    json: bool,

    /// Seconds each Herdr request may take before it counts as failed
    #[arg(long, value_name = "SECONDS", default_value = "30", value_parser = parse_timeout)]
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
    /// The branch is checked out in the main checkout, which is shared by
    /// the whole project rather than owned by the task.
    InMainCheckout { branch: String, checkout: PathBuf },
    /// Git records a checkout of the branch whose directory is missing.
    MissingCheckout { branch: String, checkout: PathBuf },
    /// `--path` names another place than the branch's existing checkout.
    PathMismatch {
        requested: PathBuf,
        checkout: PathBuf,
    },
    /// The new checkout's path already exists.
    PathExists(PathBuf),
    /// No default checkout path can be formed, so `--path` is needed.
    NoDefaultPath,
    /// The integration branch a new branch would start from does not exist.
    BaseMissing { base: String },
    /// Git would change but the user was not asked.
    Unauthorized { plan: String },
    /// The user declined the Git change.
    Declined,
    /// The workspace is open, but a tab of its layout could not be set up.
    LayoutIncomplete {
        task: String,
        workspace: String,
        opened: Vec<&'static str>,
        failed: &'static str,
        cause: Box<HerdrError>,
    },
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
            Self::PathMismatch {
                requested,
                checkout,
            } => write!(
                f,
                "the branch is already checked out at {}, not {}; omit --path to open it",
                checkout.display(),
                requested.display()
            ),
            Self::PathExists(path) => write!(
                f,
                "{} already exists; choose another location with --path",
                path.display()
            ),
            Self::NoDefaultPath => write!(
                f,
                "cannot choose a checkout location because HOME is not set; pass --path"
            ),
            Self::BaseMissing { base } => write!(
                f,
                "the integration branch {base} does not exist in this repository; create or \
                 fetch it, or set integration_branch in leme.toml"
            ),
            Self::Unauthorized { plan } => write!(
                f,
                "{plan}\nNothing was changed. Run again with --yes to confirm, or in a terminal \
                 to be asked."
            ),
            Self::Declined => write!(f, "cancelled; nothing was changed"),
            Self::LayoutIncomplete {
                task,
                workspace,
                opened,
                failed,
                cause,
            } => {
                write!(
                    f,
                    "workspace {workspace} is open, but its {failed} tab could not be set up: \
                     {cause}. The checkout"
                )?;
                if !opened.is_empty() {
                    write!(f, " and the {} tabs", opened.join(", "))?;
                }
                write!(
                    f,
                    " are kept; run `leme feature open {task}` again to add the missing tabs"
                )
            }
        }
    }
}

impl std::error::Error for FeatureError {}

impl From<FeatureError> for Error {
    fn from(error: FeatureError) -> Self {
        Self::Feature(Box::new(error))
    }
}

/// Where the task's checkout is, or will be.
#[derive(Debug)]
enum Target {
    /// A linked worktree already has the branch.
    Existing(PathBuf),
    /// The checkout will be created at `path`. `base` is set when the branch
    /// itself is new.
    New { path: PathBuf, base: Option<String> },
}

/// What setting up one tab did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum TabStatus {
    /// The tab was set up by an earlier invocation and is still open.
    Kept,
    /// The tab was opened now.
    Opened,
    /// The tab was opened earlier, but its setup is finished now.
    Resumed,
}

#[derive(Debug, Serialize)]
struct TabReport {
    role: &'static str,
    label: &'static str,
    tab: String,
    status: TabStatus,
}

/// The branch and worktree created by this invocation.
#[derive(Debug, Serialize)]
struct Created {
    /// Whether the branch was created, starting at `base`, rather than an
    /// existing branch being checked out.
    branch: bool,
    base: Option<String>,
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
    /// Set when this invocation created the checkout.
    created: Option<Created>,
    workspace: OpenedWorkspace,
    tabs: Vec<TabReport>,
    /// The checkout recorded before, when Git now reports another one.
    previous_checkout: Option<PathBuf>,
    /// The workspace recorded before, when Herdr now reports another one.
    previous_workspace: Option<String>,
    /// Anything the user should know that did not stop the command.
    notes: Vec<String>,
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
    let requested_path = args.path.as_ref().map(|path| directory.join(path));
    let target = target(
        &repository,
        &branch,
        &base,
        registered,
        requested_path.as_deref(),
    )?;
    if let Some(conflict) = conflict(&project.tasks, task, &branch) {
        return Err(FeatureError::Conflict(conflict).into());
    }

    let herdr = Herdr::locate(args.timeout).ok_or(HerdrError::NotInstalled)?;
    let readiness = herdr.readiness();
    if let Some(check) = readiness.blocker() {
        return Err(HerdrError::NotReady(Box::new(check.clone())).into());
    }
    if let Target::New { path, base } = &target {
        authorize(&plan(task, &branch, path, base.as_deref()), args.yes)?;
    }

    let checkout = match &target {
        Target::Existing(path) | Target::New { path, .. } => path.clone(),
    };
    let previous = store
        .register_task(&project.id, task, &branch, &checkout)?
        .map_err(FeatureError::Conflict)?;
    let (workspace, created) = match &target {
        Target::Existing(path) => (herdr::open_worktree(&herdr, &repository, path)?, None),
        Target::New { path, base } => {
            let new = match base {
                Some(base) => NewCheckout::NewBranch { base },
                None => NewCheckout::ExistingBranch,
            };
            let workspace = herdr::create_worktree(&herdr, &repository, &branch, new, path)?;
            let created = Created {
                branch: base.is_some(),
                base: base.clone(),
            };
            (workspace, Some(created))
        }
    };
    store.record_workspace(&project.id, task, &workspace.id)?;

    let environment = [
        ("LEME_PROJECT_ID", project.id.clone()),
        ("LEME_TASK_ID", task.to_owned()),
        ("LEME_BRANCH", branch.clone()),
        ("LEME_CHECKOUT", checkout.to_string_lossy().into_owned()),
    ]
    .map(|(name, value)| (name.to_owned(), value));
    let mut notes = Vec::new();
    let tabs = lay_out(
        &herdr,
        &mut store,
        Layout {
            project: &project.id,
            task,
            workspace: &workspace,
            checkout: &checkout,
            environment: &environment,
        },
        &mut notes,
    )?;

    let report = Report {
        project_id: project.id,
        task: task.to_owned(),
        branch,
        base,
        created,
        tabs,
        previous_checkout: previous
            .as_ref()
            .map(|task| task.worktree.clone())
            .filter(|recorded| *recorded != checkout),
        previous_workspace: previous
            .and_then(|task| task.workspace)
            .filter(|recorded| *recorded != workspace.id),
        checkout,
        workspace,
        notes,
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

/// Another task of the project that already owns `branch`. Checked before
/// asking the user; registration checks again under the write lock.
fn conflict(tasks: &[Task], task: &str, branch: &str) -> Option<TaskConflict> {
    tasks
        .iter()
        .find(|other| other.id != task && other.branch == branch)
        .map(|other| TaskConflict::BranchHasOtherTask {
            branch: branch.to_owned(),
            task: other.id.clone(),
        })
}

/// The linked worktree that has `branch`, or the checkout to create.
fn target(
    repository: &Path,
    branch: &str,
    base: &str,
    registered: Option<&Task>,
    requested: Option<&Path>,
) -> Result<Target, Error> {
    let worktrees = git::worktrees(repository)?;
    if let Some(worktree) = git::checkout_of(&worktrees, branch) {
        let path = existing_checkout(worktree, branch)?;
        if let Some(requested) = requested.filter(|requested| *requested != path) {
            return Err(FeatureError::PathMismatch {
                requested: requested.to_path_buf(),
                checkout: path,
            }
            .into());
        }
        return Ok(Target::Existing(path));
    }

    // An earlier attempt may have recorded where the checkout was to go.
    let path = match (requested, registered) {
        (Some(requested), _) => requested.to_path_buf(),
        (None, Some(task)) => task.worktree.clone(),
        (None, None) => default_path(repository, branch).ok_or(FeatureError::NoDefaultPath)?,
    };
    if fs::symlink_metadata(&path).is_ok() {
        return Err(FeatureError::PathExists(path).into());
    }
    let base = if git::branch_exists(repository, branch)? {
        None
    } else if git::branch_exists(repository, base)? {
        Some(base.to_owned())
    } else {
        return Err(FeatureError::BaseMissing {
            base: base.to_owned(),
        }
        .into());
    };
    Ok(Target::New { path, base })
}

/// Checks that the checkout Git reports for `branch` can belong to a task.
fn existing_checkout(worktree: &Worktree, branch: &str) -> Result<PathBuf, FeatureError> {
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

/// `~/.herdr/worktrees/<repository directory>/<branch with / as ->`, the
/// location Herdr itself chooses for a new worktree.
fn default_path(repository: &Path, branch: &str) -> Option<PathBuf> {
    let home = env::var_os("HOME").filter(|home| !home.is_empty())?;
    let name = repository.file_name()?;
    Some(
        PathBuf::from(home)
            .join(".herdr/worktrees")
            .join(name)
            .join(branch.replace('/', "-")),
    )
}

/// The Git change the user is asked to authorize.
fn plan(task: &str, branch: &str, path: &Path, base: Option<&str>) -> String {
    let branch_line = match base {
        Some(base) => format!("  - create branch {branch} from {base}, the integration branch"),
        None => format!("  - check out the existing branch {branch}"),
    };
    format!(
        "Opening task {task} changes Git:\n{branch_line}\n  - in a new worktree at {}\n\
         Herdr then opens it in a new workspace with agent, neovim and tests tabs.",
        path.display()
    )
}

/// Asks the user to confirm `plan` unless `confirmed` already does.
///
/// Without a terminal to ask in, the change is refused rather than assumed.
fn authorize(plan: &str, confirmed: bool) -> Result<(), Error> {
    if confirmed {
        return Ok(());
    }
    if !(io::stdin().is_terminal() && io::stderr().is_terminal()) {
        return Err(FeatureError::Unauthorized {
            plan: plan.to_owned(),
        }
        .into());
    }
    let mut stderr = io::stderr();
    write!(stderr, "{plan}\nCreate them? [y/N] ").map_err(Error::Output)?;
    stderr.flush().map_err(Error::Output)?;
    let mut answer = String::new();
    io::stdin()
        .lock()
        .read_line(&mut answer)
        .map_err(Error::Terminal)?;
    match answer.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => Ok(()),
        _ => Err(FeatureError::Declined.into()),
    }
}

/// What the default layout is laid out for.
struct Layout<'a> {
    project: &'a str,
    task: &'a str,
    workspace: &'a OpenedWorkspace,
    checkout: &'a Path,
    environment: &'a [(String, String)],
}

/// Opens the layout tabs that are missing from the workspace and finishes
/// any whose setup was interrupted.
///
/// A recorded tab counts only while it belongs to the current workspace and
/// Herdr still lists it. Once every tab is set up, the idle shell tab Herdr
/// creates with a new workspace is closed, leaving exactly the layout.
fn lay_out(
    herdr: &Herdr,
    store: &mut Store,
    layout: Layout<'_>,
    notes: &mut Vec<String>,
) -> Result<Vec<TabReport>, Error> {
    let Layout {
        project,
        task,
        workspace,
        checkout,
        environment,
    } = layout;
    let incomplete =
        |opened: &[TabReport], failed: TabRole, cause| FeatureError::LayoutIncomplete {
            task: task.to_owned(),
            workspace: workspace.id.clone(),
            opened: opened.iter().map(|tab| tab.label).collect(),
            failed: failed.label(),
            cause: Box::new(cause),
        };
    let live = herdr::list_tabs(herdr, &workspace.id)
        .map_err(|cause| incomplete(&[], TabRole::Agent, cause))?;
    let recorded = store.layout_tabs(project, task)?;
    let editor_found =
        diagnostics::find_executable("nvim", env::var_os("PATH").as_deref()).is_some();

    let mut tabs = Vec::new();
    for role in TabRole::ALL {
        let existing = recorded.iter().find(|tab| {
            tab.role == role.key() && tab.workspace == workspace.id && live.contains(&tab.tab)
        });
        let (tab, pane, status) = match existing {
            Some(tab) if tab.ready => {
                tabs.push(TabReport {
                    role: role.key(),
                    label: role.label(),
                    tab: tab.tab.clone(),
                    status: TabStatus::Kept,
                });
                continue;
            }
            Some(tab) => (tab.tab.clone(), tab.pane.clone(), TabStatus::Resumed),
            None => {
                let focus = role == TabRole::Agent;
                let created = herdr::create_tab(
                    herdr,
                    &workspace.id,
                    checkout,
                    role.label(),
                    environment,
                    focus,
                )
                .map_err(|cause| incomplete(&tabs, role, cause))?;
                store.record_tab(
                    project,
                    task,
                    role.key(),
                    &workspace.id,
                    &created.id,
                    &created.pane,
                )?;
                (created.id, created.pane, TabStatus::Opened)
            }
        };
        match role.command() {
            Some(command) if role == TabRole::Editor && !editor_found => notes.push(format!(
                "Neovim (nvim) was not found on PATH, so the {} tab holds a shell; install \
                 Neovim and run `{command}` there",
                role.label()
            )),
            Some(command) => herdr::run_in_pane(herdr, &pane, command)
                .map_err(|cause| incomplete(&tabs, role, cause))?,
            None => {}
        }
        store.mark_tab_ready(project, task, role.key())?;
        tabs.push(TabReport {
            role: role.key(),
            label: role.label(),
            tab,
            status,
        });
    }

    if let Some(tab) = workspace
        .new_tab
        .as_deref()
        .filter(|tab| live.iter().any(|live| live == tab))
        && let Err(error) = herdr::close_tab(herdr, tab)
    {
        notes.push(format!(
            "the initial tab {tab} of the new workspace could not be closed: {error}"
        ));
    }
    Ok(tabs)
}

fn render_text(report: &Report) -> String {
    let workspace = &report.workspace;
    let mut lines = vec![
        format!("Task:      {}", report.task),
        format!("Branch:    {}", report.branch),
        format!("Base:      {} (integration branch)", report.base),
        format!("Checkout:  {}", report.checkout.display()),
    ];
    if let Some(created) = &report.created {
        lines.push(match &created.base {
            Some(base) => format!(
                "Created:   branch {} from {base}, and its worktree",
                report.branch
            ),
            None => "Created:   a worktree for the existing branch".to_owned(),
        });
    }
    lines.push(format!(
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
    ));
    let tabs: Vec<String> = report
        .tabs
        .iter()
        .map(|tab| {
            let status = match tab.status {
                TabStatus::Kept => "kept",
                TabStatus::Opened => "opened",
                TabStatus::Resumed => "resumed",
            };
            format!("{} {} ({status})", tab.label, tab.tab)
        })
        .collect();
    lines.push(format!("Tabs:      {}", tabs.join(", ")));
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
    for note in &report.notes {
        lines.push(format!("Note: {note}"));
    }
    for check in &report.unconfirmed {
        lines.push(format!("Unconfirmed: {}: {}", check.name, check.detail));
    }
    lines.join("\n")
}
