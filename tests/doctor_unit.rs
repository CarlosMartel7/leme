//! Classifies supplied probe results, without running any tool.
//!
//! Each test starts from a healthy environment and breaks one thing, so it
//! can assert both what that breaks and what it leaves alone.

use std::path::PathBuf;
use std::time::Duration;

use leme::config::{Agent, Preferences};
use leme::diagnostics::{
    CapabilityStatus, CheckStatus, HERDR_OPERATIONS, Probes, Report, Run, StateProbe,
    TESTED_HERDR_PROTOCOL, ToolProbe, Version, classify,
};
use leme::git::Checkout;
use leme::state::SCHEMA_VERSION;
use serde_json::json;

const ALL_CAPABILITIES: [&str; 7] = [
    "status",
    "workspaces",
    "editor",
    "feature_agent",
    "captain",
    "github",
    "gitflow",
];

/// Actions that drive Herdr.
const HERDR_CAPABILITIES: [&str; 4] = ["workspaces", "editor", "feature_agent", "captain"];

fn finished(stdout: &str) -> Run {
    Run::Finished {
        success: true,
        stdout: stdout.to_owned(),
        stderr: String::new(),
    }
}

fn found(executable: &str, stdout: &str) -> ToolProbe {
    ToolProbe::Found {
        path: PathBuf::from("/usr/bin").join(executable),
        run: finished(stdout),
    }
}

fn found_with(executable: &str, run: Run) -> ToolProbe {
    ToolProbe::Found {
        path: PathBuf::from("/usr/bin").join(executable),
        run,
    }
}

fn server_status(status: serde_json::Value) -> Option<Run> {
    Some(finished(&status.to_string()))
}

fn schema(protocol: u64, methods: &[&str]) -> Option<Run> {
    let requests: Vec<_> = methods
        .iter()
        .map(|method| json!({"properties": {"method": {"const": method, "type": "string"}}}))
        .collect();
    let schema = json!({
        "protocol": protocol,
        "schema_version": 1,
        "schemas": {"request": {"oneOf": requests}},
    });
    Some(finished(&schema.to_string()))
}

fn running_server() -> Option<Run> {
    server_status(json!({
        "status": "running",
        "running": true,
        "version": "0.9.5",
        "protocol": TESTED_HERDR_PROTOCOL,
        "compatible": true,
        "restart_needed": false,
    }))
}

fn checkout() -> Checkout {
    Checkout {
        repository: PathBuf::from("/work/project"),
        branch: Some("main".to_owned()),
        checkout: PathBuf::from("/work/project"),
    }
}

/// Probe results of an installation where everything works.
fn healthy() -> Probes {
    let mut methods = HERDR_OPERATIONS.to_vec();
    methods.push("server.stop");
    Probes {
        git: found("git", "git version 2.50.1\n"),
        repository: Ok(checkout()),
        preferences: Some(Ok(Preferences::default())),
        state: StateProbe::Usable {
            path: PathBuf::from("/home/user/.local/state/leme/leme.db"),
            schema: Some(SCHEMA_VERSION as i64),
        },
        herdr: found("herdr", "herdr 0.9.5\n"),
        herdr_server: running_server(),
        herdr_schema: schema(TESTED_HERDR_PROTOCOL, &methods),
        neovim: found("nvim", "NVIM v0.12.6\nBuild type: Release\n"),
        claude: found("claude", "2.1.319 (Claude Code)\n"),
        codex: found("codex", "codex-cli 0.167.0\n"),
        gh: found(
            "gh",
            "gh version 2.46.0 (2025-01-13)\nhttps://github.com/cli/cli\n",
        ),
        git_flow: found("git-flow", "1.12.3 (AVH Edition)\n"),
        cargo: found("cargo", "cargo 1.94.1 (29ea6fb6a 2026-03-24)\n"),
        rustc: found("rustc", "rustc 1.94.1 (e408947bf 2026-03-25)\n"),
        cc: found("cc", "cc (Debian 14.3.0-5) 14.3.0\n"),
    }
}

fn capability_status(report: &Report, id: &str) -> CapabilityStatus {
    report
        .capability(id)
        .unwrap_or_else(|| panic!("no capability {id}"))
        .status
}

fn blocked_by<'a>(report: &'a Report, id: &str) -> &'a [&'static str] {
    &report.capability(id).expect("capability").blocked_by
}

fn check_status(report: &Report, id: &str) -> CheckStatus {
    report
        .check(id)
        .unwrap_or_else(|| panic!("no check {id}"))
        .status
}

fn detail<'a>(report: &'a Report, id: &str) -> &'a str {
    &report.check(id).expect("check").detail
}

fn correction<'a>(report: &'a Report, id: &str) -> &'a str {
    report
        .check(id)
        .expect("check")
        .correction
        .as_deref()
        .unwrap_or_else(|| panic!("check {id} has no correction"))
}

/// Requires exactly `unavailable` to be unavailable and the rest available.
fn assert_only_unavailable(report: &Report, unavailable: &[&str]) {
    for id in ALL_CAPABILITIES {
        let expected = if unavailable.contains(&id) {
            CapabilityStatus::Unavailable
        } else {
            CapabilityStatus::Available
        };
        assert_eq!(capability_status(report, id), expected, "capability {id}");
    }
}

#[test]
fn a_healthy_installation_supports_every_action() {
    let report = classify(&healthy());

    assert_only_unavailable(&report, &[]);
    for check in report.checks.iter().chain(&report.build) {
        assert_eq!(check.status, CheckStatus::Ok, "check {}", check.id);
        assert_eq!(check.correction, None, "check {}", check.id);
    }
    assert_eq!(
        report.check("git").unwrap().version.as_deref(),
        Some("2.50.1")
    );
    assert_eq!(
        report.check("git").unwrap().path,
        Some(PathBuf::from("/usr/bin/git"))
    );
}

#[test]
fn every_capability_lists_its_requirements() {
    let report = classify(&healthy());

    let requires = |id| report.capability(id).unwrap().requires.clone();
    assert_eq!(
        requires("status"),
        ["git", "repository", "preferences", "state"]
    );
    assert_eq!(requires("github"), ["git", "repository", "gh"]);
    assert_eq!(requires("gitflow"), ["git", "repository", "git_flow"]);
    assert!(requires("editor").contains(&"neovim"));
    assert!(requires("captain").contains(&"codex"));
    for id in HERDR_CAPABILITIES {
        for check in ["herdr", "herdr_server", "herdr_api", "state"] {
            assert!(requires(id).contains(&check), "{id} requires {check}");
        }
    }
    for id in ALL_CAPABILITIES {
        for build in ["cargo", "rustc", "cc"] {
            assert!(!requires(id).contains(&build), "{id} requires {build}");
        }
    }
}

#[test]
fn missing_gh_disables_only_github_queries() {
    let mut probes = healthy();
    probes.gh = ToolProbe::Missing;

    let report = classify(&probes);

    assert_only_unavailable(&report, &["github"]);
    assert_eq!(blocked_by(&report, "github"), ["gh"]);
    assert_eq!(check_status(&report, "gh"), CheckStatus::Failed);
    assert_eq!(detail(&report, "gh"), "`gh` was not found on PATH");
    assert!(correction(&report, "gh").contains("https://cli.github.com"));
    assert_eq!(report.check("gh").unwrap().path, None);
}

#[test]
fn missing_herdr_disables_herdr_actions_and_skips_its_server_checks() {
    let mut probes = healthy();
    probes.herdr = ToolProbe::Missing;
    probes.herdr_server = None;
    probes.herdr_schema = None;

    let report = classify(&probes);

    assert_only_unavailable(&report, &HERDR_CAPABILITIES);
    for id in HERDR_CAPABILITIES {
        assert_eq!(
            blocked_by(&report, id),
            ["herdr", "herdr_server", "herdr_api"]
        );
    }
    assert_eq!(check_status(&report, "herdr"), CheckStatus::Failed);
    assert!(correction(&report, "herdr").contains("https://herdr.dev"));
    for id in ["herdr_server", "herdr_api"] {
        assert_eq!(check_status(&report, id), CheckStatus::Skipped, "{id}");
        assert_eq!(detail(&report, id), "requires a working herdr executable");
    }
}

#[test]
fn a_stopped_herdr_server_is_a_known_failure() {
    let mut probes = healthy();
    probes.herdr_server = server_status(json!({"status": "stopped", "running": false}));

    let report = classify(&probes);

    assert_only_unavailable(&report, &HERDR_CAPABILITIES);
    assert_eq!(blocked_by(&report, "workspaces"), ["herdr_server"]);
    assert_eq!(check_status(&report, "herdr_server"), CheckStatus::Failed);
    assert_eq!(detail(&report, "herdr_server"), "the server is not running");
    assert!(correction(&report, "herdr_server").contains("running `herdr`"));
}

#[test]
fn a_herdr_server_that_differs_from_the_client_is_a_known_failure() {
    for status in [
        json!({"running": true, "compatible": false, "restart_needed": false}),
        json!({"running": true, "compatible": true, "restart_needed": true}),
    ] {
        let mut probes = healthy();
        probes.herdr_server = server_status(status.clone());

        let report = classify(&probes);

        assert_eq!(
            check_status(&report, "herdr_server"),
            CheckStatus::Failed,
            "{status}"
        );
        assert!(correction(&report, "herdr_server").contains("Restart the Herdr server"));
        assert_only_unavailable(&report, &HERDR_CAPABILITIES);
    }
}

#[test]
fn an_unreported_server_compatibility_is_uncertain() {
    for status in [
        json!({"running": true}),
        json!({"status": "running"}),
        json!(["not", "an", "object"]),
    ] {
        let mut probes = healthy();
        probes.herdr_server = server_status(status.clone());

        let report = classify(&probes);

        assert_eq!(
            check_status(&report, "herdr_server"),
            CheckStatus::Uncertain,
            "{status}"
        );
        assert_eq!(
            capability_status(&report, "workspaces"),
            CapabilityStatus::Uncertain
        );
    }
}

#[test]
fn an_untested_herdr_protocol_is_uncertain_not_failed() {
    let mut probes = healthy();
    probes.herdr_schema = schema(TESTED_HERDR_PROTOCOL + 1, &HERDR_OPERATIONS);

    let report = classify(&probes);

    assert_eq!(check_status(&report, "herdr_api"), CheckStatus::Uncertain);
    assert!(detail(&report, "herdr_api").contains(&format!(
        "protocol {}; Leme was tested with protocol {TESTED_HERDR_PROTOCOL}",
        TESTED_HERDR_PROTOCOL + 1
    )));
    for id in HERDR_CAPABILITIES {
        let capability = report.capability(id).unwrap();
        assert_eq!(capability.status, CapabilityStatus::Uncertain, "{id}");
        assert_eq!(capability.uncertain, ["herdr_api"], "{id}");
        assert!(capability.blocked_by.is_empty(), "{id}");
    }
    for id in ["status", "github", "gitflow"] {
        assert_eq!(capability_status(&report, id), CapabilityStatus::Available);
    }
}

#[test]
fn a_missing_herdr_operation_is_a_known_failure() {
    let methods: Vec<_> = HERDR_OPERATIONS
        .into_iter()
        .filter(|operation| *operation != "worktree.create")
        .collect();
    let mut probes = healthy();
    // A missing operation outweighs an untested protocol.
    probes.herdr_schema = schema(TESTED_HERDR_PROTOCOL + 1, &methods);

    let report = classify(&probes);

    assert_eq!(check_status(&report, "herdr_api"), CheckStatus::Failed);
    assert_eq!(
        detail(&report, "herdr_api"),
        "this Herdr does not provide: worktree.create"
    );
    assert!(correction(&report, "herdr_api").contains("Update Herdr"));
    assert_only_unavailable(&report, &HERDR_CAPABILITIES);
}

#[test]
fn an_unreadable_herdr_schema_is_uncertain() {
    let unreadable = [
        finished("not json"),
        finished(r#"{"protocol": 22, "schemas": {}}"#),
        finished(r#"{"protocol": 22, "schemas": {"request": {"oneOf": []}}}"#),
        Run::Finished {
            success: false,
            stdout: String::new(),
            stderr: "error: unknown command\n".to_owned(),
        },
        Run::TimedOut(Duration::from_secs(5)),
        Run::NotStarted("permission denied".to_owned()),
    ];
    for run in unreadable {
        let mut probes = healthy();
        probes.herdr_schema = Some(run.clone());

        let report = classify(&probes);

        assert_eq!(
            check_status(&report, "herdr_api"),
            CheckStatus::Uncertain,
            "{run:?}"
        );
        assert!(correction(&report, "herdr_api").contains("herdr api schema"));
        assert_eq!(
            capability_status(&report, "workspaces"),
            CapabilityStatus::Uncertain
        );
    }
}

#[test]
fn git_older_than_the_minimum_is_unsupported() {
    let mut probes = healthy();
    probes.git = found("git", "git version 2.30.9\n");

    let report = classify(&probes);

    assert_eq!(check_status(&report, "git"), CheckStatus::Failed);
    assert_eq!(
        detail(&report, "git"),
        "version 2.30.9 is older than the required 2.31.0"
    );
    assert_eq!(correction(&report, "git"), "Update Git to 2.31.0 or later.");
    assert_only_unavailable(&report, &ALL_CAPABILITIES);
}

#[test]
fn git_at_the_minimum_version_is_supported() {
    let mut probes = healthy();
    probes.git = found("git", "git version 2.31.0\n");

    let report = classify(&probes);

    assert_eq!(check_status(&report, "git"), CheckStatus::Ok);
    assert_only_unavailable(&report, &[]);
}

#[test]
fn an_unrecognized_version_is_uncertain_only_where_a_minimum_applies() {
    let mut probes = healthy();
    probes.git = found("git", "git (custom build)\n");
    probes.gh = found("gh", "GitHub CLI, development build\n");

    let report = classify(&probes);

    assert_eq!(check_status(&report, "git"), CheckStatus::Uncertain);
    assert!(correction(&report, "git").contains("2.31.0 or later"));
    assert_eq!(report.check("git").unwrap().version, None);
    // gh has no minimum version, so any version that runs is enough.
    assert_eq!(check_status(&report, "gh"), CheckStatus::Ok);
    assert!(detail(&report, "gh").contains("no recognizable version"));
    for id in ALL_CAPABILITIES {
        assert_eq!(
            capability_status(&report, id),
            CapabilityStatus::Uncertain,
            "{id}"
        );
    }
}

#[test]
fn build_requirements_never_affect_an_action() {
    let mut probes = healthy();
    probes.rustc = found("rustc", "rustc 1.84.1 (e71f9a9a9 2025-01-27)\n");
    probes.cargo = ToolProbe::Missing;
    probes.cc = ToolProbe::Missing;

    let report = classify(&probes);

    assert_eq!(check_status(&report, "rustc"), CheckStatus::Failed);
    assert_eq!(
        detail(&report, "rustc"),
        "version 1.84.1 is older than the required 1.85.0"
    );
    assert_eq!(check_status(&report, "cargo"), CheckStatus::Failed);
    assert_eq!(check_status(&report, "cc"), CheckStatus::Failed);
    assert!(report.checks.iter().all(|check| check.id != "rustc"));
    assert_only_unavailable(&report, &[]);
}

#[test]
fn outside_a_repository_project_actions_are_unavailable() {
    let mut probes = healthy();
    probes.repository =
        Err("/tmp is not inside a Git repository; run leme from a project checkout".to_owned());
    probes.preferences = None;

    let report = classify(&probes);

    assert_eq!(check_status(&report, "repository"), CheckStatus::Failed);
    assert!(detail(&report, "repository").contains("/tmp is not inside a Git repository"));
    assert!(correction(&report, "repository").contains("inside the project's checkout"));
    assert_eq!(check_status(&report, "preferences"), CheckStatus::Skipped);
    assert_only_unavailable(&report, &ALL_CAPABILITIES);
    assert_eq!(blocked_by(&report, "github"), ["repository"]);
    assert_eq!(blocked_by(&report, "status"), ["repository", "preferences"]);
    // Checks that do not depend on the project still run.
    for id in ["git", "state", "herdr", "herdr_server", "gh"] {
        assert_eq!(check_status(&report, id), CheckStatus::Ok, "{id}");
    }
}

#[test]
fn invalid_preferences_block_project_actions_but_not_tool_queries() {
    let mut probes = healthy();
    probes.preferences = Some(Err(
        "invalid /work/project/leme.toml: unknown variant `copilot`".to_owned(),
    ));

    let report = classify(&probes);

    assert_eq!(check_status(&report, "preferences"), CheckStatus::Failed);
    assert!(correction(&report, "preferences").contains("docs/configuration.md"));
    let agent = report.capability("feature_agent").unwrap();
    // The preferred agent is unknown, so no agent check is guessed.
    assert_eq!(agent.action, "Start the preferred feature agent");
    assert!(!agent.requires.contains(&"claude"));
    assert_eq!(agent.blocked_by, ["preferences"]);
    assert_only_unavailable(
        &report,
        &["status", "workspaces", "editor", "feature_agent", "captain"],
    );
}

#[test]
fn the_preferred_agent_decides_which_agent_cli_is_required() {
    let mut probes = healthy();
    probes.claude = ToolProbe::Missing;

    let report = classify(&probes);

    assert_eq!(
        report.capability("feature_agent").unwrap().action,
        "Start the preferred feature agent (claude)"
    );
    assert_only_unavailable(&report, &["feature_agent"]);
    assert_eq!(blocked_by(&report, "feature_agent"), ["claude"]);

    probes.preferences = Some(Ok(Preferences {
        preferred_agent: Agent::Codex,
        ..Preferences::default()
    }));

    let report = classify(&probes);

    // With Codex preferred, a missing Claude Code affects no action.
    assert_eq!(
        report.capability("feature_agent").unwrap().action,
        "Start the preferred feature agent (codex)"
    );
    assert_eq!(check_status(&report, "claude"), CheckStatus::Failed);
    assert_only_unavailable(&report, &[]);
}

#[test]
fn missing_codex_disables_the_captain_and_a_codex_feature_agent() {
    let mut probes = healthy();
    probes.codex = ToolProbe::Missing;

    let report = classify(&probes);

    assert_only_unavailable(&report, &["captain"]);
    assert_eq!(blocked_by(&report, "captain"), ["codex"]);

    probes.preferences = Some(Ok(Preferences {
        preferred_agent: Agent::Codex,
        ..Preferences::default()
    }));

    assert_only_unavailable(&classify(&probes), &["captain", "feature_agent"]);
}

#[test]
fn missing_neovim_or_git_flow_affects_only_its_action() {
    let mut probes = healthy();
    probes.neovim = ToolProbe::Missing;
    assert_only_unavailable(&classify(&probes), &["editor"]);

    let mut probes = healthy();
    probes.git_flow = ToolProbe::Missing;
    let report = classify(&probes);
    assert_only_unavailable(&report, &["gitflow"]);
    assert_eq!(
        detail(&report, "git_flow"),
        "`git-flow` was not found on PATH"
    );
}

#[test]
fn a_state_directory_that_cannot_be_located_blocks_project_actions() {
    let mut probes = healthy();
    probes.state = StateProbe::NoLocation;

    let report = classify(&probes);

    assert_eq!(check_status(&report, "state"), CheckStatus::Failed);
    assert!(correction(&report, "state").contains("XDG_STATE_HOME"));
    assert_eq!(report.check("state").unwrap().path, None);
    assert_only_unavailable(
        &report,
        &["status", "workspaces", "editor", "feature_agent", "captain"],
    );
}

#[test]
fn an_unwritable_or_unreadable_state_location_is_reported_with_its_path() {
    let directory = PathBuf::from("/home/user/.local/state");
    let database = directory.join("leme/leme.db");
    let cases = [
        (
            StateProbe::Unwritable {
                path: directory.clone(),
                reason: "cannot create files in /home/user/.local/state: Permission denied"
                    .to_owned(),
            },
            directory.clone(),
            "writable",
        ),
        (
            StateProbe::Unreadable {
                path: database.clone(),
                reason: "cannot use /home/user/.local/state/leme/leme.db: file is not a database"
                    .to_owned(),
            },
            database.clone(),
            "move it aside",
        ),
    ];
    for (probe, path, advice) in cases {
        let mut probes = healthy();
        probes.state = probe;

        let report = classify(&probes);

        let state = report.check("state").unwrap();
        assert_eq!(state.status, CheckStatus::Failed);
        assert_eq!(state.path.as_ref(), Some(&path));
        assert!(state.correction.as_deref().unwrap().contains(advice));
        assert_eq!(blocked_by(&report, "status"), ["state"]);
        assert_eq!(
            capability_status(&report, "github"),
            CapabilityStatus::Available
        );
    }
}

#[test]
fn the_state_schema_version_decides_whether_the_database_is_usable() {
    let path = PathBuf::from("/state/leme/leme.db");
    let supported = SCHEMA_VERSION as i64;
    let cases = [
        (None, CheckStatus::Ok, "not created yet"),
        (Some(supported), CheckStatus::Ok, "schema version"),
        (Some(supported - 1), CheckStatus::Ok, "upgrades it"),
        (
            Some(supported + 1),
            CheckStatus::Failed,
            "newer than this Leme",
        ),
        (Some(-1), CheckStatus::Failed, "newer than this Leme"),
    ];
    for (schema, expected, wording) in cases {
        let mut probes = healthy();
        probes.state = StateProbe::Usable {
            path: path.clone(),
            schema,
        };

        let report = classify(&probes);

        assert_eq!(check_status(&report, "state"), expected, "{schema:?}");
        assert!(detail(&report, "state").contains(wording), "{schema:?}");
        if expected == CheckStatus::Failed {
            assert_eq!(correction(&report, "state"), "Update Leme.");
            assert_eq!(blocked_by(&report, "status"), ["state"]);
        }
    }
}

#[test]
fn a_tool_that_cannot_answer_is_failed_with_the_reason() {
    let cases = [
        (
            Run::TimedOut(Duration::from_millis(1500)),
            "`gh --version` did not finish within 1.5 seconds",
        ),
        (
            Run::NotStarted("Permission denied (os error 13)".to_owned()),
            "cannot run `gh --version`: Permission denied (os error 13)",
        ),
        (
            Run::Finished {
                success: false,
                stdout: "partial\n".to_owned(),
                stderr: "\nerror: broken install\nmore detail\n".to_owned(),
            },
            "`gh --version` failed: error: broken install",
        ),
    ];
    for (run, expected) in cases {
        let mut probes = healthy();
        probes.gh = found_with("gh", run);

        let report = classify(&probes);

        assert_eq!(check_status(&report, "gh"), CheckStatus::Failed);
        assert_eq!(detail(&report, "gh"), expected);
        assert_eq!(
            report.check("gh").unwrap().path,
            Some(PathBuf::from("/usr/bin/gh"))
        );
        assert_only_unavailable(&report, &["github"]);
    }
}

#[test]
fn reported_output_is_limited_to_one_bounded_line() {
    let secret = "ghp_0123456789abcdefSECRET";
    let mut probes = healthy();
    probes.gh = found("gh", &format!("gh version 2.46.0\ntoken: {secret}\n"));
    probes.git_flow = found_with(
        "git-flow",
        Run::Finished {
            success: false,
            stdout: String::new(),
            stderr: format!("fatal: {}\nGITHUB_TOKEN={secret}\n", "x".repeat(400)),
        },
    );
    probes.neovim = found("nvim", &format!("{}\n{secret}\n", "y".repeat(400)));
    probes.codex = found_with(
        "codex",
        Run::NotStarted(format!("exec format error\nOPENAI_API_KEY={secret}")),
    );
    probes.herdr_schema = Some(Run::NotStarted(format!("denied\n{secret}")));

    let report = classify(&probes);
    let encoded = serde_json::to_string(&report).unwrap();

    assert!(
        !encoded.contains(secret),
        "a later output line was reported"
    );
    let longest = report
        .checks
        .iter()
        .map(|check| check.detail.chars().count())
        .max()
        .unwrap();
    assert!(longest < 260, "detail of {longest} characters");
}

#[test]
fn versions_are_found_in_common_version_lines() {
    let cases = [
        ("git version 2.50.1", Some(Version::new(2, 50, 1))),
        ("NVIM v0.12.6", Some(Version::new(0, 12, 6))),
        ("2.1.319 (Claude Code)", Some(Version::new(2, 1, 319))),
        ("codex-cli 0.167.0", Some(Version::new(0, 167, 0))),
        ("cc (Debian 14.3.0-5) 14.3.0", Some(Version::new(14, 3, 0))),
        ("tool 3.4", Some(Version::new(3, 4, 0))),
        ("tool 1.2.3.4", Some(Version::new(1, 2, 3))),
        ("tool 1.", None),
        ("tool 7", None),
        ("no version here", None),
        ("", None),
    ];
    for (text, expected) in cases {
        assert_eq!(Version::find(text), expected, "{text:?}");
    }
    assert!(Version::new(2, 31, 0) > Version::new(2, 30, 99));
    assert!(Version::new(10, 0, 0) > Version::new(9, 99, 99));
    assert_eq!(Version::new(2, 31, 0).to_string(), "2.31.0");
}

#[test]
fn statuses_are_encoded_as_lowercase_words() {
    let mut probes = healthy();
    probes.gh = ToolProbe::Missing;
    probes.herdr_schema = schema(TESTED_HERDR_PROTOCOL + 1, &HERDR_OPERATIONS);
    probes.preferences = None;
    probes.repository = Err("outside".to_owned());

    let report = serde_json::to_value(classify(&probes)).unwrap();

    let statuses = |section: &str| -> Vec<String> {
        report[section]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["status"].as_str().unwrap().to_owned())
            .collect()
    };
    let checks = statuses("checks");
    for word in ["ok", "failed", "skipped", "uncertain"] {
        assert!(checks.iter().any(|status| status == word), "{word}");
    }
    assert!(
        statuses("capabilities")
            .iter()
            .all(|status| status == "unavailable")
    );
}
