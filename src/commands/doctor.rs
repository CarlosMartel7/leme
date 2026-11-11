//! The `leme doctor` command.

use std::env;
use std::io::{self, Write};
use std::time::Duration;

use clap::Args;

use super::{Error, parse_timeout};
use crate::diagnostics::{self, Capability, CapabilityStatus, Check, CheckStatus, Report};

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Print the diagnosis as JSON
    #[arg(long)]
    json: bool,

    /// Seconds each tool may take to answer before it counts as failed
    #[arg(long, value_name = "SECONDS", default_value = "5", value_parser = parse_timeout)]
    timeout: Duration,
}

pub fn run(args: &DoctorArgs) -> Result<(), Error> {
    let directory = env::current_dir();
    let probes = diagnostics::collect(directory.as_deref(), args.timeout);
    let report = diagnostics::classify(&probes);
    let output = if args.json {
        serde_json::to_string_pretty(&report).map_err(Error::Json)?
    } else {
        render_text(&report)
    };
    writeln!(io::stdout(), "{output}").map_err(Error::Output)
}

fn render_text(report: &Report) -> String {
    let name_of = |id: &'static str| report.check(id).map_or(id, |check| check.name);
    let mut text = String::from("Leme actions\n");
    for capability in &report.capabilities {
        text.push_str(&render_capability(capability, name_of));
    }
    text.push_str("\nChecks\n");
    for check in &report.checks {
        text.push_str(&render_check(check));
    }
    text.push_str("\nBuilding from source (not needed to run Leme)\n");
    for check in &report.build {
        text.push_str(&render_check(check));
    }
    text.truncate(text.trim_end().len());
    text
}

fn render_capability(
    capability: &Capability,
    name_of: impl Fn(&'static str) -> &'static str,
) -> String {
    let status = match capability.status {
        CapabilityStatus::Available => "available",
        CapabilityStatus::Uncertain => "uncertain",
        CapabilityStatus::Unavailable => "unavailable",
    };
    let mut text = format!("  {status:<12} {}\n", capability.action);
    let list = |ids: &[&'static str]| {
        ids.iter()
            .map(|id| name_of(id))
            .collect::<Vec<_>>()
            .join(", ")
    };
    if !capability.blocked_by.is_empty() {
        text.push_str(&format!(
            "  {:<12}   blocked by: {}\n",
            "",
            list(&capability.blocked_by)
        ));
    }
    if !capability.uncertain.is_empty() {
        text.push_str(&format!(
            "  {:<12}   unconfirmed: {}\n",
            "",
            list(&capability.uncertain)
        ));
    }
    text
}

fn render_check(check: &Check) -> String {
    let status = match check.status {
        CheckStatus::Ok => "ok",
        CheckStatus::Uncertain => "uncertain",
        CheckStatus::Failed => "failed",
        CheckStatus::Skipped => "skipped",
    };
    let mut text = format!("  {status:<12} {}: {}", check.name, check.detail);
    if let Some(path) = &check.path {
        text.push_str(&format!(" ({})", path.display()));
    }
    text.push('\n');
    if let Some(correction) = &check.correction {
        text.push_str(&format!("  {:<12}   fix: {correction}\n", ""));
    }
    text
}
