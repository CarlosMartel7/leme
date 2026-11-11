//! The `leme herdr context` command.

use std::io::{self, Write};
use std::time::Duration;

use clap::Args;
use serde::Serialize;

use super::{Error, parse_timeout};
use crate::diagnostics::Check;
use crate::herdr::{self, Context, Herdr, HerdrError, Invocation};

#[derive(Debug, Args)]
pub struct ContextArgs {
    /// Print the context as JSON
    #[arg(long)]
    json: bool,

    /// Workspace ID to resolve instead of the invoking one
    #[arg(long, value_name = "ID")]
    workspace: Option<String>,

    /// Tab ID to resolve instead of the invoking one; needs --workspace or --pane
    #[arg(long, value_name = "ID")]
    tab: Option<String>,

    /// Pane ID to resolve instead of the invoking one
    #[arg(long, value_name = "ID")]
    pane: Option<String>,

    /// Seconds each Herdr request may take before it counts as failed
    #[arg(long, value_name = "SECONDS", default_value = "5", value_parser = parse_timeout)]
    timeout: Duration,
}

/// Where the IDs came from.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum Source {
    /// Herdr's `HERDR_*_ID` variables.
    Environment,
    /// `--workspace`, `--tab` or `--pane`.
    Arguments,
}

/// Everything the command reports. Text and JSON render this same value.
#[derive(Debug, Serialize)]
struct Report {
    source: Source,
    /// The IDs that were looked up.
    invocation: Invocation,
    #[serde(flatten)]
    context: Context,
    /// Herdr checks whose compatibility `leme doctor` could not confirm.
    unconfirmed: Vec<Check>,
}

pub fn run(args: &ContextArgs) -> Result<(), Error> {
    let (source, invocation) = invocation(args);
    if invocation.is_empty() {
        return Err(HerdrError::NoInvocation.into());
    }
    let herdr = Herdr::locate(args.timeout).ok_or(HerdrError::NotInstalled)?;
    let readiness = herdr.readiness();
    if let Some(check) = readiness.blocker() {
        return Err(HerdrError::NotReady(Box::new(check.clone())).into());
    }
    let context = herdr::resolve(&herdr, &invocation)?;
    let report = Report {
        source,
        invocation,
        context,
        unconfirmed: readiness.uncertain().cloned().collect(),
    };
    let output = if args.json {
        serde_json::to_string_pretty(&report).map_err(Error::Json)?
    } else {
        render_text(&report)
    };
    writeln!(io::stdout(), "{output}").map_err(Error::Output)
}

/// Explicit IDs replace the environment entirely, so a mix of the two can
/// never describe two different places.
fn invocation(args: &ContextArgs) -> (Source, Invocation) {
    let explicit = Invocation {
        workspace: args.workspace.clone(),
        tab: args.tab.clone(),
        pane: args.pane.clone(),
    };
    if explicit.is_empty() {
        (Source::Environment, Invocation::from_env())
    } else {
        (Source::Arguments, explicit)
    }
}

fn render_text(report: &Report) -> String {
    let Context {
        workspace,
        tab,
        pane,
    } = &report.context;
    let named = |id: &str, label: Option<&str>, number: Option<u64>| {
        let mut text = id.to_owned();
        if let Some(label) = label {
            text.push_str(&format!("  {label}"));
        }
        if let Some(number) = number {
            text.push_str(&format!(" (#{number})"));
        }
        text
    };
    let mut lines = vec![format!(
        "Workspace: {}",
        named(&workspace.id, workspace.label.as_deref(), workspace.number)
    )];
    if let Some(checkout) = &workspace.checkout {
        lines.push(format!("  Checkout:  {}", checkout.display()));
    }
    lines.push(match tab {
        Some(tab) => format!(
            "Tab:       {}",
            named(&tab.id, tab.label.as_deref(), tab.number)
        ),
        None => "Tab:       (not supplied)".to_owned(),
    });
    match pane {
        Some(pane) => {
            lines.push(format!("Pane:      {}", pane.id));
            if let Some(cwd) = &pane.cwd {
                lines.push(format!("  Directory: {}", cwd.display()));
            }
        }
        None => lines.push("Pane:      (not supplied)".to_owned()),
    }
    lines.push(String::new());
    lines.push(match report.source {
        Source::Environment => "Resolved from the IDs Herdr gave this process.".to_owned(),
        Source::Arguments => "Resolved from the IDs given as arguments.".to_owned(),
    });
    for check in &report.unconfirmed {
        lines.push(format!("Unconfirmed: {}: {}", check.name, check.detail));
    }
    lines.join("\n")
}
