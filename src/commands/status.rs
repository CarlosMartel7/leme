//! The `leme status` command.

use std::env;
use std::io::{self, Write};

use clap::Args;

use super::Error;
use crate::git::{self, Checkout};

#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Print the status as JSON
    #[arg(long)]
    json: bool,
}

pub fn run(args: &StatusArgs) -> Result<(), Error> {
    let directory = env::current_dir().map_err(Error::CurrentDirectory)?;
    let checkout = git::discover(&directory)?;
    let output = if args.json {
        render_json(&checkout)?
    } else {
        render_text(&checkout)
    };
    writeln!(io::stdout(), "{output}").map_err(Error::Output)
}

fn render_text(checkout: &Checkout) -> String {
    format!(
        "Repository: {}\nBranch:     {}\nCheckout:   {}",
        checkout.repository.display(),
        checkout.branch.as_deref().unwrap_or("(detached HEAD)"),
        checkout.checkout.display(),
    )
}

fn render_json(checkout: &Checkout) -> Result<String, Error> {
    serde_json::to_string_pretty(checkout).map_err(Error::Json)
}
