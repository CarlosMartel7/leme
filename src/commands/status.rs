//! The `leme status` command.

use std::env;
use std::io::{self, Write};

use clap::Args;
use serde::Serialize;

use super::Error;
use crate::config::Preferences;
use crate::git::{self, Checkout};

#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Print the status as JSON
    #[arg(long)]
    json: bool,
}

/// Everything `leme status` reports. Text and JSON render this same value.
#[derive(Debug, Serialize)]
struct Status {
    #[serde(flatten)]
    checkout: Checkout,
    preferences: Preferences,
}

pub fn run(args: &StatusArgs) -> Result<(), Error> {
    let directory = env::current_dir().map_err(Error::CurrentDirectory)?;
    let checkout = git::discover(&directory)?;
    let preferences = Preferences::load(&checkout.repository)?;
    let status = Status {
        checkout,
        preferences,
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
        checkout,
        preferences,
    } = status;
    format!(
        "Repository: {}\nBranch:     {}\nCheckout:   {}\n\n\
         Integration branch: {}\nPreferred agent:    {}",
        checkout.repository.display(),
        checkout.branch.as_deref().unwrap_or("(detached HEAD)"),
        checkout.checkout.display(),
        preferences.integration_branch,
        preferences.preferred_agent,
    )
}

fn render_json(status: &Status) -> Result<String, Error> {
    serde_json::to_string_pretty(status).map_err(Error::Json)
}
