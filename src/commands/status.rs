//! The `leme status` command.

use std::env;
use std::io::{self, Write};

use clap::Args;
use serde::Serialize;

use super::Error;
use crate::config::Preferences;
use crate::git::{self, Checkout};
use crate::state::{Store, Task};

#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Print the status as JSON
    #[arg(long)]
    json: bool,
}

/// Everything `leme status` reports. Text and JSON render this same value.
#[derive(Debug, Serialize)]
struct Status {
    project_id: String,
    #[serde(flatten)]
    checkout: Checkout,
    preferences: Preferences,
    tasks: Vec<Task>,
}

pub fn run(args: &StatusArgs) -> Result<(), Error> {
    let directory = env::current_dir().map_err(Error::CurrentDirectory)?;
    let checkout = git::discover(&directory)?;
    let preferences = Preferences::load(&checkout.repository)?;
    let project = Store::open_default()?.project(&checkout.repository)?;
    let status = Status {
        project_id: project.id,
        checkout,
        preferences,
        tasks: project.tasks,
    };
    let output = if args.json {
        render_json(&status)?
    } else {
        render_text(&status)
    };
    writeln!(io::stdout(), "{output}").map_err(Error::Output)
}

fn render_text(status: &Status) -> String {
    let Status {
        project_id,
        checkout,
        preferences,
        tasks,
    } = status;
    format!(
        "Project:    {project_id}\nRepository: {}\nBranch:     {}\nCheckout:   {}\n\n\
         Integration branch: {}\nPreferred agent:    {}\n\n{}",
        checkout.repository.display(),
        checkout.branch.as_deref().unwrap_or("(detached HEAD)"),
        checkout.checkout.display(),
        preferences.integration_branch,
        preferences.preferred_agent,
        render_tasks(tasks),
    )
}

fn render_tasks(tasks: &[Task]) -> String {
    if tasks.is_empty() {
        return "Tasks: none".to_owned();
    }
    let rows = tasks.iter().map(|task| {
        format!(
            "  {}  {}  {}",
            task.id,
            task.branch,
            task.worktree.display()
        )
    });
    format!("Tasks:\n{}", rows.collect::<Vec<_>>().join("\n"))
}

fn render_json(status: &Status) -> Result<String, Error> {
    serde_json::to_string_pretty(status).map_err(Error::Json)
}
