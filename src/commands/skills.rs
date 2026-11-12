//! The `leme skills install` command.

use std::collections::BTreeSet;
use std::io::{self, Write};

use clap::{Args, ValueEnum};

use super::Error;
use crate::skills::{
    self, Action, Catalog, Environment, Outcome, Report, SkillKind, SkillResult, Target,
};

#[derive(Debug, Args)]
pub struct InstallArgs {
    /// Agent whose user skills directory receives the skills
    #[arg(long, value_enum)]
    target: TargetChoice,

    /// Show what would be installed without writing anything
    #[arg(long)]
    dry_run: bool,

    /// Print the result as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum TargetChoice {
    /// Codex: $HOME/.agents/skills
    Codex,
    /// Claude Code: ~/.claude/skills, or $CLAUDE_CONFIG_DIR/skills
    Claude,
    /// Both agents
    Both,
}

impl TargetChoice {
    fn targets(self) -> &'static [Target] {
        match self {
            Self::Codex => &[Target::Codex],
            Self::Claude => &[Target::Claude],
            Self::Both => &[Target::Codex, Target::Claude],
        }
    }
}

pub fn run(args: &InstallArgs) -> Result<(), Error> {
    let catalog = Catalog::bundled().map_err(Error::Catalog)?;
    let report = skills::install(
        &catalog,
        args.target.targets(),
        &Environment::from_process(),
        args.dry_run,
    )
    .map_err(Error::Install)?;
    let output = if args.json {
        serde_json::to_string_pretty(&report).map_err(Error::Json)?
    } else {
        render_text(&report)
    };
    writeln!(io::stdout(), "{output}").map_err(Error::Output)?;
    match report.outcome {
        Outcome::Planned | Outcome::Installed => Ok(()),
        outcome => Err(Error::SkillsIncomplete(outcome)),
    }
}

fn render_text(report: &Report) -> String {
    let mut text = String::new();
    for target in &report.targets {
        text.push_str(&format!(
            "{}: {}\n",
            target.target.label(),
            target.root.display()
        ));
        for skill in &target.skills {
            let state = match (skill.result, skill.action) {
                (Some(result), _) => result_label(result),
                (None, action) => action_label(action),
            };
            let helper = match skill.kind {
                SkillKind::Requested => "",
                SkillKind::Dependency => " (helper)",
            };
            text.push_str(&format!("  {state:<14} {}{helper}\n", skill.name));
            if let Some(conflict) = skill.conflict {
                text.push_str(&format!(
                    "                 {}: {}\n",
                    skill.destination.display(),
                    conflict.describe()
                ));
            }
        }
        if let Some(error) = &target.error {
            text.push_str(&format!("  Stopped: {error}\n"));
        }
        text.push('\n');
    }

    text.push_str(match report.outcome {
        Outcome::Planned => "Dry run: nothing was written.",
        Outcome::Conflict => {
            "Nothing was written. Move the conflicting directories aside, or merge your \
             changes elsewhere, then run again."
        }
        Outcome::Installed => "Done. Installed skills are available to new agent sessions.",
        Outcome::Partial => "Only some targets were completed; see Stopped above.",
        Outcome::Failed => "No target was completed; see Stopped above.",
    });

    let requirements: BTreeSet<&str> = report
        .targets
        .iter()
        .flat_map(|target| &target.skills)
        .flat_map(|skill| skill.requirements.iter().map(String::as_str))
        .collect();
    if !requirements.is_empty() {
        text.push_str("\n\nSome skills expect tools Leme does not install or configure:\n");
        for requirement in requirements {
            text.push_str(&format!("  - {requirement}\n"));
        }
    }
    text.truncate(text.trim_end().len());
    text
}

/// Label for a skill that was not written: a dry run, or a refused run.
fn action_label(action: Action) -> &'static str {
    match action {
        Action::Install => "would install",
        Action::Update => "would update",
        Action::Unchanged => "unchanged",
        Action::Conflict => "conflict",
    }
}

fn result_label(result: SkillResult) -> &'static str {
    match result {
        SkillResult::Installed => "installed",
        SkillResult::Updated => "updated",
        SkillResult::Unchanged => "unchanged",
        SkillResult::Failed => "failed",
        SkillResult::NotAttempted => "not attempted",
    }
}
