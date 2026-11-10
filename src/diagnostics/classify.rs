//! Turns probe observations into checks and capabilities.
//!
//! Nothing here runs a process or touches the file system.

use std::fmt;
use std::time::Duration;

use serde_json::Value;

use super::{
    Capability, CapabilityStatus, Check, CheckStatus, Probes, Report, Run, StateProbe, ToolProbe,
};
use crate::config::{Agent, Preferences};
use crate::git::Checkout;
use crate::state;

/// Oldest Git that supports `rev-parse --path-format`, which discovery uses.
pub const MINIMUM_GIT: Version = Version::new(2, 31, 0);

/// Oldest Rust compiler that can build Leme, taken from `rust-version` in
/// `Cargo.toml`. That setting must cover every locked dependency's own
/// minimum, which a test checks.
pub const MINIMUM_RUST: Version = Version::parse_const(env!("CARGO_PKG_RUST_VERSION"));

/// The manual correction for a missing or outdated Rust toolchain.
const INSTALL_RUST: &str = concat!(
    "Install Rust ",
    env!("CARGO_PKG_RUST_VERSION"),
    " or later with rustup (https://rustup.rs)."
);

/// The Herdr API protocol Leme was tested against.
pub const TESTED_HERDR_PROTOCOL: u64 = 22;

/// Herdr API operations Leme uses to find its context and to open
/// workspaces, worktrees and tabs.
pub const HERDR_OPERATIONS: [&str; 7] = [
    "pane.current",
    "tab.create",
    "tab.list",
    "workspace.create",
    "workspace.list",
    "worktree.create",
    "worktree.list",
];

/// Longest excerpt of a tool's output quoted in a report.
const EXCERPT_LIMIT: usize = 160;

/// A dotted release number such as `2.50.1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    /// Parses `major.minor` or `major.minor.patch` while compiling, so a
    /// malformed version stops the build instead of reaching users.
    const fn parse_const(text: &str) -> Self {
        let bytes = text.as_bytes();
        let mut parts = [0u64; 3];
        let mut part = 0;
        let mut digits = 0;
        let mut index = 0;
        while index < bytes.len() {
            let byte = bytes[index];
            if byte == b'.' {
                assert!(digits > 0 && part < 2, "malformed version");
                part += 1;
                digits = 0;
            } else {
                assert!(byte.is_ascii_digit(), "malformed version");
                parts[part] = parts[part] * 10 + (byte - b'0') as u64;
                digits += 1;
            }
            index += 1;
        }
        assert!(digits > 0 && part >= 1, "malformed version");
        Self::new(parts[0], parts[1], parts[2])
    }

    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Finds the first dotted release number in `text`, such as `2.50.1` in
    /// `git version 2.50.1` or `0.12.6` in `NVIM v0.12.6`. A missing patch
    /// number counts as zero.
    pub fn find(text: &str) -> Option<Self> {
        text.split_whitespace().find_map(|word| {
            let word = word.strip_prefix('v').unwrap_or(word);
            let end = word
                .find(|character: char| !character.is_ascii_digit() && character != '.')
                .unwrap_or(word.len());
            let numbers = word[..end]
                .trim_end_matches('.')
                .split('.')
                .map(|part| part.parse::<u64>().ok())
                .collect::<Option<Vec<_>>>()?;
            match numbers[..] {
                [major, minor] => Some(Self::new(major, minor, 0)),
                [major, minor, patch, ..] => Some(Self::new(major, minor, patch)),
                _ => None,
            }
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// How to identify and install one executable.
struct Tool {
    id: &'static str,
    name: &'static str,
    /// The probe command, as shown to the user.
    command: &'static str,
    minimum: Option<Version>,
    install: &'static str,
}

const GIT: Tool = Tool {
    id: "git",
    name: "Git",
    command: "git --version",
    minimum: Some(MINIMUM_GIT),
    install: "Install Git 2.31 or later (sudo apt install git) and add it to PATH.",
};

const HERDR: Tool = Tool {
    id: "herdr",
    name: "Herdr",
    command: "herdr --version",
    minimum: None,
    install: "Install Herdr (https://herdr.dev) and add `herdr` to PATH.",
};

const NEOVIM: Tool = Tool {
    id: "neovim",
    name: "Neovim",
    command: "nvim --version",
    minimum: None,
    install: "Install Neovim (https://neovim.io) and add `nvim` to PATH.",
};

const CLAUDE: Tool = Tool {
    id: "claude",
    name: "Claude Code",
    command: "claude --version",
    minimum: None,
    install: "Install Claude Code (https://claude.com/claude-code) and add `claude` to PATH.",
};

const CODEX: Tool = Tool {
    id: "codex",
    name: "Codex CLI",
    command: "codex --version",
    minimum: None,
    install: "Install the Codex CLI (https://github.com/openai/codex) and add `codex` to PATH.",
};

const GH: Tool = Tool {
    id: "gh",
    name: "GitHub CLI",
    command: "gh --version",
    minimum: None,
    install: "Install the GitHub CLI (https://cli.github.com) and add `gh` to PATH.",
};

const GIT_FLOW: Tool = Tool {
    id: "git_flow",
    name: "git-flow",
    command: "git-flow version",
    minimum: None,
    install: "Install git-flow (sudo apt install git-flow) and add `git-flow` to PATH.",
};

const CARGO: Tool = Tool {
    id: "cargo",
    name: "Cargo",
    command: "cargo --version",
    minimum: Some(MINIMUM_RUST),
    install: INSTALL_RUST,
};

const RUSTC: Tool = Tool {
    id: "rustc",
    name: "Rust compiler",
    command: "rustc --version",
    minimum: Some(MINIMUM_RUST),
    install: INSTALL_RUST,
};

const CC: Tool = Tool {
    id: "cc",
    name: "C compiler",
    command: "cc --version",
    minimum: None,
    install: "Install a C compiler for the bundled SQLite (sudo apt install build-essential).",
};

/// Checks every action that works with a project's state needs.
const PROJECT_CHECKS: [&str; 4] = ["git", "repository", "preferences", "state"];

/// Checks every action that drives Herdr needs.
const HERDR_CHECKS: [&str; 3] = ["herdr", "herdr_server", "herdr_api"];

/// Judges the observations in `probes`.
pub fn classify(probes: &Probes) -> Report {
    let checks = vec![
        tool_check(&GIT, &probes.git),
        repository_check(&probes.repository),
        preferences_check(probes.preferences.as_ref()),
        state_check(&probes.state),
        tool_check(&HERDR, &probes.herdr),
        herdr_server_check(probes.herdr_server.as_ref()),
        herdr_api_check(probes.herdr_schema.as_ref()),
        tool_check(&NEOVIM, &probes.neovim),
        tool_check(&CLAUDE, &probes.claude),
        tool_check(&CODEX, &probes.codex),
        tool_check(&GH, &probes.gh),
        tool_check(&GIT_FLOW, &probes.git_flow),
    ];
    let build = vec![
        tool_check(&CARGO, &probes.cargo),
        tool_check(&RUSTC, &probes.rustc),
        tool_check(&CC, &probes.cc),
    ];
    let preferred = match &probes.preferences {
        Some(Ok(preferences)) => Some(preferences.preferred_agent),
        _ => None,
    };
    let capabilities = capability_definitions(preferred)
        .into_iter()
        .map(|(id, action, requires)| capability(id, action, requires, &checks))
        .collect();
    Report {
        capabilities,
        checks,
        build,
    }
}

/// Each Leme action with the checks it requires.
fn capability_definitions(
    preferred: Option<Agent>,
) -> Vec<(&'static str, String, Vec<&'static str>)> {
    let workspace: Vec<_> = PROJECT_CHECKS
        .iter()
        .chain(&HERDR_CHECKS)
        .copied()
        .collect();
    let with = |extra: &[&'static str]| {
        let mut requires = workspace.clone();
        requires.extend_from_slice(extra);
        requires
    };
    let (agent_action, agent_checks) = match preferred {
        Some(agent) => (
            format!("Start the preferred feature agent ({agent})"),
            with(&[agent_check(agent)]),
        ),
        // Without readable preferences the agent is unknown, and the
        // failed preferences check already blocks the action.
        None => (
            "Start the preferred feature agent".to_owned(),
            workspace.clone(),
        ),
    };
    vec![
        (
            "status",
            "Show the project status (leme status)".to_owned(),
            PROJECT_CHECKS.to_vec(),
        ),
        (
            "workspaces",
            "Open feature workspaces, worktrees and tabs in Herdr".to_owned(),
            workspace.clone(),
        ),
        (
            "editor",
            "Open Neovim in a feature workspace".to_owned(),
            with(&["neovim"]),
        ),
        ("feature_agent", agent_action, agent_checks),
        (
            "captain",
            "Start the Codex captain and dictator".to_owned(),
            with(&["codex"]),
        ),
        (
            "github",
            "Query GitHub pull requests and checks".to_owned(),
            vec!["git", "repository", "gh"],
        ),
        (
            "gitflow",
            "Finish features through GitFlow".to_owned(),
            vec!["git", "repository", "git_flow"],
        ),
    ]
}

fn agent_check(agent: Agent) -> &'static str {
    match agent {
        Agent::Claude => CLAUDE.id,
        Agent::Codex => CODEX.id,
    }
}

fn capability(
    id: &'static str,
    action: String,
    requires: Vec<&'static str>,
    checks: &[Check],
) -> Capability {
    let status_of = |required: &str| {
        checks
            .iter()
            .find(|check| check.id == required)
            .map(|check| check.status)
    };
    let blocked_by: Vec<_> = requires
        .iter()
        .copied()
        .filter(|required| {
            matches!(
                status_of(required),
                Some(CheckStatus::Failed | CheckStatus::Skipped)
            )
        })
        .collect();
    let uncertain: Vec<_> = requires
        .iter()
        .copied()
        .filter(|required| status_of(required) == Some(CheckStatus::Uncertain))
        .collect();
    let status = if !blocked_by.is_empty() {
        CapabilityStatus::Unavailable
    } else if !uncertain.is_empty() {
        CapabilityStatus::Uncertain
    } else {
        CapabilityStatus::Available
    };
    Capability {
        id,
        action,
        status,
        requires,
        blocked_by,
        uncertain,
    }
}

fn check(id: &'static str, name: &'static str, status: CheckStatus, detail: String) -> Check {
    Check {
        id,
        name,
        status,
        detail,
        path: None,
        version: None,
        correction: None,
    }
}

impl Check {
    fn correction(mut self, correction: impl Into<String>) -> Self {
        self.correction = Some(correction.into());
        self
    }
}

fn tool_check(tool: &Tool, probe: &ToolProbe) -> Check {
    let check = |status, detail| check(tool.id, tool.name, status, detail);
    let (path, run) = match probe {
        ToolProbe::Missing => {
            let executable = tool.command.split(' ').next().unwrap_or(tool.command);
            return check(
                CheckStatus::Failed,
                format!("`{executable}` was not found on PATH"),
            )
            .correction(tool.install);
        }
        ToolProbe::Found { path, run } => (path, run),
    };
    let mut result = match run {
        Run::NotStarted(reason) => check(
            CheckStatus::Failed,
            format!("cannot run `{}`: {}", tool.command, excerpt(reason)),
        )
        .correction(tool.install),
        Run::TimedOut(limit) => check(CheckStatus::Failed, timed_out(tool.command, *limit))
            .correction(format!(
                "Check that `{}` finishes when run by itself; reinstall {} if it hangs.",
                tool.command, tool.name
            )),
        Run::Finished {
            success: false,
            stderr,
            ..
        } => check(
            CheckStatus::Failed,
            exited_unsuccessfully(tool.command, stderr),
        )
        .correction(tool.install),
        Run::Finished {
            success: true,
            stdout,
            stderr,
        } => {
            let line = first_line(stdout).or_else(|| first_line(stderr));
            version_check(tool, line, check)
        }
    };
    result.path = Some(path.clone());
    result
}

fn version_check(
    tool: &Tool,
    line: Option<String>,
    check: impl Fn(CheckStatus, String) -> Check,
) -> Check {
    let line = line.unwrap_or_default();
    let Some(version) = Version::find(&line) else {
        let detail = format!(
            "`{}` printed no recognizable version: {line:?}",
            tool.command
        );
        return match tool.minimum {
            Some(minimum) => check(CheckStatus::Uncertain, detail).correction(format!(
                "Confirm that this is {} {minimum} or later.",
                tool.name
            )),
            None => check(CheckStatus::Ok, detail),
        };
    };
    let mut result = match tool.minimum {
        Some(minimum) if version < minimum => check(
            CheckStatus::Failed,
            format!("version {version} is older than the required {minimum}"),
        )
        .correction(format!("Update {} to {minimum} or later.", tool.name)),
        _ => check(CheckStatus::Ok, format!("version {version}")),
    };
    result.version = Some(version.to_string());
    result
}

fn repository_check(repository: &Result<Checkout, String>) -> Check {
    let name = "Project repository";
    match repository {
        Ok(checkout) => {
            let mut result = check(
                "repository",
                name,
                CheckStatus::Ok,
                format!(
                    "checkout {} on {}",
                    checkout.checkout.display(),
                    checkout.branch.as_deref().unwrap_or("detached HEAD")
                ),
            );
            result.path = Some(checkout.repository.clone());
            result
        }
        Err(reason) => check("repository", name, CheckStatus::Failed, reason.clone())
            .correction("Run leme doctor from inside the project's checkout to check its actions."),
    }
}

fn preferences_check(preferences: Option<&Result<Preferences, String>>) -> Check {
    let name = "Project preferences";
    match preferences {
        None => check(
            "preferences",
            name,
            CheckStatus::Skipped,
            "requires the project repository".to_owned(),
        ),
        Some(Ok(preferences)) => check(
            "preferences",
            name,
            CheckStatus::Ok,
            format!(
                "integration branch {}, preferred agent {}",
                preferences.integration_branch, preferences.preferred_agent
            ),
        ),
        Some(Err(reason)) => check("preferences", name, CheckStatus::Failed, reason.clone())
            .correction("Correct the setting named above in leme.toml; see docs/configuration.md."),
    }
}

fn state_check(probe: &StateProbe) -> Check {
    let check = |status, detail| check("state", "State database", status, detail);
    let (mut result, path) = match probe {
        StateProbe::NoLocation => {
            return check(
                CheckStatus::Failed,
                "neither XDG_STATE_HOME nor HOME gives a state directory".to_owned(),
            )
            .correction("Set HOME, or set XDG_STATE_HOME to an absolute directory.");
        }
        StateProbe::Unwritable { path, reason } => (
            check(CheckStatus::Failed, reason.clone()).correction(
                "Make that location writable, or set XDG_STATE_HOME to an absolute, \
                 writable directory outside your checkouts.",
            ),
            path,
        ),
        StateProbe::Unreadable { path, reason } => (
            check(CheckStatus::Failed, reason.clone()).correction(
                "Check the database's permissions; if it is damaged, move it aside so \
                 Leme creates a new one.",
            ),
            path,
        ),
        StateProbe::Usable { path, schema: None } => (
            check(
                CheckStatus::Ok,
                "not created yet; leme status creates it on first use".to_owned(),
            ),
            path,
        ),
        StateProbe::Usable {
            path,
            schema: Some(found),
        } => {
            let supported = state::SCHEMA_VERSION;
            let newer = usize::try_from(*found).map_or(true, |found| found > supported);
            let result = if newer {
                check(
                    CheckStatus::Failed,
                    format!(
                        "schema version {found} is newer than this Leme supports ({supported})"
                    ),
                )
                .correction("Update Leme.")
            } else if usize::try_from(*found) == Ok(supported) {
                check(CheckStatus::Ok, format!("schema version {found}"))
            } else {
                check(
                    CheckStatus::Ok,
                    format!(
                        "schema version {found}; leme status upgrades it to {supported} on next use"
                    ),
                )
            };
            (result, path)
        }
    };
    result.path = Some(path.clone());
    result
}

const HERDR_STATUS: &str = "herdr status server --json";
const HERDR_SCHEMA: &str = "herdr api schema --json";

fn herdr_server_check(run: Option<&Run>) -> Check {
    let check = |status, detail| check("herdr_server", "Herdr server", status, detail);
    let start = "Start Herdr by running `herdr`, then run leme doctor again.";
    let unconfirmed = "Run `herdr status` to inspect the server; Leme cannot confirm \
                       that it is compatible.";
    let Some(run) = run else {
        return check(
            CheckStatus::Skipped,
            "requires a working herdr executable".to_owned(),
        );
    };
    let (success, stdout, stderr) = match run {
        Run::NotStarted(reason) => {
            return check(
                CheckStatus::Failed,
                format!("cannot run `{HERDR_STATUS}`: {}", excerpt(reason)),
            )
            .correction(HERDR.install);
        }
        Run::TimedOut(limit) => {
            return check(CheckStatus::Failed, timed_out(HERDR_STATUS, *limit)).correction(
                "Check that the Herdr server responds to `herdr status`; restart Herdr if it \
                 hangs.",
            );
        }
        Run::Finished {
            success,
            stdout,
            stderr,
        } => (*success, stdout, stderr),
    };
    let Ok(status) = serde_json::from_str::<Value>(stdout.trim()) else {
        return if success {
            check(
                CheckStatus::Uncertain,
                format!("`{HERDR_STATUS}` printed output Leme does not recognize"),
            )
            .correction(unconfirmed)
        } else {
            check(
                CheckStatus::Failed,
                exited_unsuccessfully(HERDR_STATUS, stderr),
            )
            .correction(start)
        };
    };
    let reported_version = status["version"].as_str().map(str::to_owned);
    let mut result = match status["running"].as_bool() {
        Some(false) => {
            check(CheckStatus::Failed, "the server is not running".to_owned()).correction(start)
        }
        Some(true)
            if status["compatible"].as_bool() == Some(false)
                || status["restart_needed"].as_bool() == Some(true) =>
        {
            check(
                CheckStatus::Failed,
                "the running server does not match the installed Herdr".to_owned(),
            )
            .correction(
                "Restart the Herdr server so it runs the installed version; `herdr status` \
                 shows what is out of date.",
            )
        }
        Some(true) if status["compatible"].as_bool() == Some(true) => {
            let protocol = status["protocol"]
                .as_u64()
                .map_or_else(|| "unknown".to_owned(), |protocol| protocol.to_string());
            check(
                CheckStatus::Ok,
                format!("running and compatible (protocol {protocol})"),
            )
        }
        Some(true) => check(
            CheckStatus::Uncertain,
            "the server is running but did not report whether it is compatible".to_owned(),
        )
        .correction(unconfirmed),
        None => check(
            CheckStatus::Uncertain,
            format!("`{HERDR_STATUS}` did not report whether the server is running"),
        )
        .correction(unconfirmed),
    };
    result.version = reported_version;
    result
}

fn herdr_api_check(run: Option<&Run>) -> Check {
    let check = |status, detail| check("herdr_api", "Herdr operations", status, detail);
    let unreadable = "Run `herdr api schema` to check that Herdr can describe its operations; \
                      update Herdr if it cannot.";
    let Some(run) = run else {
        return check(
            CheckStatus::Skipped,
            "requires a working herdr executable".to_owned(),
        );
    };
    // Without a schema Leme cannot tell whether an operation exists, which is
    // uncertainty about compatibility rather than a known failure.
    let stdout = match run {
        Run::Finished {
            success: true,
            stdout,
            ..
        } => stdout,
        Run::Finished {
            success: false,
            stderr,
            ..
        } => {
            return check(
                CheckStatus::Uncertain,
                exited_unsuccessfully(HERDR_SCHEMA, stderr),
            )
            .correction(unreadable);
        }
        Run::TimedOut(limit) => {
            return check(CheckStatus::Uncertain, timed_out(HERDR_SCHEMA, *limit))
                .correction(unreadable);
        }
        Run::NotStarted(reason) => {
            return check(
                CheckStatus::Uncertain,
                format!("cannot run `{HERDR_SCHEMA}`: {}", excerpt(reason)),
            )
            .correction(unreadable);
        }
    };
    let Some((protocol, methods)) = schema_methods(stdout) else {
        return check(
            CheckStatus::Uncertain,
            format!("`{HERDR_SCHEMA}` printed a schema Leme does not recognize"),
        )
        .correction(unreadable);
    };
    let missing: Vec<_> = HERDR_OPERATIONS
        .iter()
        .filter(|operation| !methods.contains(operation))
        .copied()
        .collect();
    let protocol_text =
        protocol.map_or_else(|| "an unknown".to_owned(), |protocol| protocol.to_string());
    let mut result = if !missing.is_empty() {
        check(
            CheckStatus::Failed,
            format!("this Herdr does not provide: {}", missing.join(", ")),
        )
        .correction("Update Herdr to a version that provides these operations.")
    } else if protocol != Some(TESTED_HERDR_PROTOCOL) {
        check(
            CheckStatus::Uncertain,
            format!(
                "provides the operations Leme uses, but speaks API protocol {protocol_text}; \
                 Leme was tested with protocol {TESTED_HERDR_PROTOCOL}"
            ),
        )
        .correction(
            "Leme may still work. Check Herdr's release notes for changes to these \
             operations, or update Leme.",
        )
    } else {
        check(
            CheckStatus::Ok,
            format!(
                "provides the {} operations Leme uses (protocol {protocol_text})",
                HERDR_OPERATIONS.len()
            ),
        )
    };
    result.version = protocol.map(|protocol| format!("protocol {protocol}"));
    result
}

/// Reads the protocol and request method names from Herdr's API schema.
fn schema_methods(text: &str) -> Option<(Option<u64>, Vec<&'static str>)> {
    let schema: Value = serde_json::from_str(text.trim()).ok()?;
    let requests = schema["schemas"]["request"]["oneOf"].as_array()?;
    let listed: Vec<&str> = requests
        .iter()
        .filter_map(|request| request["properties"]["method"]["const"].as_str())
        .collect();
    if listed.is_empty() {
        return None;
    }
    let present = HERDR_OPERATIONS
        .iter()
        .filter(|operation| listed.contains(operation))
        .copied()
        .collect();
    Some((schema["protocol"].as_u64(), present))
}

fn timed_out(command: &str, limit: Duration) -> String {
    format!(
        "`{command}` did not finish within {} seconds",
        limit.as_secs_f64()
    )
}

fn exited_unsuccessfully(command: &str, stderr: &str) -> String {
    match first_line(stderr) {
        Some(line) => format!("`{command}` failed: {line}"),
        None => format!("`{command}` failed"),
    }
}

/// The first non-blank line of a tool's output, shortened for display.
fn first_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(excerpt)
}

/// Shortens `text` so a report quotes at most one bounded line of output.
fn excerpt(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default().trim();
    match line.char_indices().nth(EXCERPT_LIMIT) {
        Some((end, _)) => format!("{}…", &line[..end]),
        None => line.to_owned(),
    }
}
