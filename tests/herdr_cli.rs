//! Runs `leme herdr context` against a scripted `herdr`.
//!
//! The fake Herdr lives in a directory whose name has spaces and a quote,
//! is reached only through `HERDR_BIN_PATH`, and records every argument it
//! receives. `PATH` holds nothing else, and the developer's own Herdr
//! variables are removed, so these tests never reach a real Herdr session.

mod support;

use std::path::PathBuf;
use std::process::Output;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use support::{
    FakeHerdr, HERDR_VARIABLES, Leme, TestDir, Toolbox, failure_message, herdr_error, herdr_reply,
    herdr_schema,
};

/// Where the fake workspace's checkout and pane are, with spaces.
const CHECKOUT: &str = "/srv/leme work/feature one";
const PANE_CWD: &str = "/srv/leme work/feature one/src dir";

/// A disposable directory holding the fake Herdr and an empty `PATH`.
struct Environment {
    root: TestDir,
    herdr: FakeHerdr,
    empty_path: Toolbox,
}

impl Environment {
    /// A healthy Herdr with workspace `w1` (tabs `w1:t1`, `w1:t3`) and
    /// workspace `w2`, where pane `w1:p2` is in tab `w1:t3`.
    fn new() -> Self {
        let root = TestDir::new();
        let herdr = FakeHerdr::new(&root.path().join("herdr's tools dir"));
        let empty_path = Toolbox::new(&root.path().join("empty path"));
        let environment = Self {
            root,
            herdr,
            empty_path,
        };
        environment
            .herdr
            .respond("pane-current", &pane("w1:p2", "w1:t3", "w1"));
        environment.herdr.respond(
            "workspace-list",
            &herdr_reply(json!({"workspaces": [
                {"workspace_id": "w2", "label": "other", "number": 1,
                 "worktree": {"checkout_path": "/srv/other"}},
                {"workspace_id": "w1", "label": "feature one", "number": 2,
                 "worktree": {"checkout_path": CHECKOUT}},
            ]})),
        );
        environment.herdr.respond(
            "tab-list",
            &herdr_reply(json!({"tabs": [
                {"tab_id": "w1:t1", "workspace_id": "w1", "label": "1", "number": 1},
                {"tab_id": "w1:t3", "workspace_id": "w1", "label": "agent", "number": 2},
            ]})),
        );
        environment
    }

    /// `leme herdr context <arguments>`, seeing only the fake Herdr.
    fn context(&self, arguments: &[&str]) -> Leme {
        let mut all = vec!["herdr", "context"];
        all.extend_from_slice(arguments);
        let mut leme = Leme::new(self.root.path(), self.root.path(), &all);
        for variable in HERDR_VARIABLES {
            leme = leme.env_remove(variable);
        }
        leme.env("PATH", self.empty_path.search_path())
            .env("HERDR_BIN_PATH", self.herdr.executable().to_str().unwrap())
    }

    /// The calls after the three readiness probes.
    fn lookups(&self) -> Vec<Vec<String>> {
        let calls = self.herdr.calls();
        assert_eq!(
            calls[..3],
            [
                vec!["--version"],
                vec!["status", "server", "--json"],
                vec!["api", "schema", "--json"],
            ],
            "readiness probes run first"
        );
        calls[3..].to_vec()
    }
}

/// A `pane.current` reply placing `pane` in `tab` of `workspace`.
fn pane(pane: &str, tab: &str, workspace: &str) -> String {
    herdr_reply(json!({"type": "pane_current", "pane": {
        "pane_id": pane, "tab_id": tab, "workspace_id": workspace,
        "cwd": PANE_CWD, "focused": false,
    }}))
}

fn arguments(list: &[&str]) -> Vec<String> {
    list.iter().map(|argument| (*argument).to_owned()).collect()
}

fn success_stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "leme herdr context failed ({:?}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).expect("UTF-8 output")
}

fn context_json(leme: Leme) -> Value {
    serde_json::from_str(&success_stdout(&leme.run())).expect("context JSON")
}

#[test]
fn the_invoking_pane_decides_the_tab_and_workspace() {
    let environment = Environment::new();
    // Stale inherited IDs, as left behind when a pane is moved, are ignored
    // because Herdr reports where the pane is now.
    let report = context_json(
        environment
            .context(&["--json"])
            .env("HERDR_PANE_ID", "w1:p2")
            .env("HERDR_TAB_ID", "w9:t9")
            .env("HERDR_WORKSPACE_ID", "w9"),
    );

    assert_eq!(report["source"], "environment");
    assert_eq!(
        report["invocation"],
        json!({"workspace": null, "tab": null, "pane": "w1:p2"})
    );
    assert_eq!(
        report["workspace"],
        json!({"id": "w1", "label": "feature one", "number": 2, "checkout": CHECKOUT})
    );
    assert_eq!(
        report["tab"],
        json!({"id": "w1:t3", "label": "agent", "number": 2})
    );
    assert_eq!(report["pane"], json!({"id": "w1:p2", "cwd": PANE_CWD}));
    assert_eq!(report["unconfirmed"], json!([]));
    assert_eq!(
        environment.lookups(),
        [
            arguments(&["pane", "current", "--pane", "w1:p2"]),
            arguments(&["workspace", "list"]),
            arguments(&["tab", "list", "--workspace", "w1"]),
        ]
    );
}

#[test]
fn without_a_pane_the_inherited_workspace_and_tab_are_resolved() {
    let environment = Environment::new();
    let report = context_json(
        environment
            .context(&["--json"])
            .env("HERDR_WORKSPACE_ID", "w1")
            .env("HERDR_TAB_ID", "w1:t1"),
    );

    assert_eq!(report["workspace"]["id"], "w1");
    assert_eq!(report["tab"]["id"], "w1:t1");
    assert_eq!(report["pane"], Value::Null);
    assert_eq!(
        environment.lookups(),
        [
            arguments(&["workspace", "list"]),
            arguments(&["tab", "list", "--workspace", "w1"]),
        ]
    );
}

#[test]
fn ids_and_paths_with_spaces_and_quotes_reach_herdr_as_single_arguments() {
    let environment = Environment::new();
    let odd_pane = "w 1:p '2'";
    let odd_workspace = "w 1 \"x\"";
    environment
        .herdr
        .respond("pane-current", &pane(odd_pane, "w1:t1", odd_workspace));
    environment.herdr.respond(
        "workspace-list",
        &herdr_reply(json!({"workspaces": [
            {"workspace_id": odd_workspace, "worktree": {"checkout_path": CHECKOUT}},
        ]})),
    );
    environment.herdr.respond(
        "tab-list",
        &herdr_reply(json!({"tabs": [{"tab_id": "w1:t1", "workspace_id": odd_workspace}]})),
    );

    // The fake is only reachable at its path with spaces and a quote.
    let report = context_json(environment.context(&["--json", "--pane", odd_pane]));

    assert_eq!(report["source"], "arguments");
    assert_eq!(report["pane"]["id"], odd_pane);
    assert_eq!(report["pane"]["cwd"], PANE_CWD);
    assert_eq!(report["workspace"]["id"], odd_workspace);
    assert_eq!(report["workspace"]["checkout"], CHECKOUT);
    assert_eq!(report["workspace"]["label"], Value::Null);
    assert_eq!(
        environment.lookups(),
        [
            arguments(&["pane", "current", "--pane", odd_pane]),
            arguments(&["workspace", "list"]),
            arguments(&["tab", "list", "--workspace", odd_workspace]),
        ]
    );
}

#[test]
fn text_and_json_report_the_same_values() {
    let environment = Environment::new();
    let text = success_stdout(&environment.context(&["--pane", "w1:p2"]).run());

    assert_eq!(
        text,
        format!(
            "Workspace: w1  feature one (#2)\n  Checkout:  {CHECKOUT}\n\
             Tab:       w1:t3  agent (#2)\nPane:      w1:p2\n  Directory: {PANE_CWD}\n\n\
             Resolved from the IDs given as arguments.\n"
        )
    );
}

#[test]
fn a_failed_exit_status_names_the_operation_and_herdrs_reason() {
    let environment = Environment::new();
    environment.herdr.fail(
        "pane-current",
        1,
        &herdr_error("server_unavailable", "cannot connect to the server"),
    );
    let message = failure_message(&environment.context(&["--pane", "w1:p2"]).run());
    assert_eq!(
        message,
        "leme: Herdr operation pane.current failed: cannot connect to the server \
         (server_unavailable)\n"
    );

    // An exit without Herdr's JSON error quotes the first line it printed.
    environment
        .herdr
        .respond("pane-current", &pane("w1:p2", "w1:t3", "w1"));
    environment
        .herdr
        .fail("workspace-list", 3, "\n  socket closed unexpectedly\nmore");
    let message = failure_message(&environment.context(&["--pane", "w1:p2"]).run());
    assert_eq!(
        message,
        "leme: Herdr operation workspace.list failed: socket closed unexpectedly\n"
    );
}

#[test]
fn ids_herdr_does_not_know_are_reported_as_not_found() {
    let environment = Environment::new();
    environment.herdr.fail(
        "pane-current",
        1,
        &herdr_error("pane_not_found", "pane not found"),
    );
    let message = failure_message(&environment.context(&["--pane", "w1:p8"]).run());
    assert_eq!(message, "leme: Herdr has no pane w1:p8\n");

    let message = failure_message(&environment.context(&["--workspace", "w7"]).run());
    assert_eq!(message, "leme: Herdr has no workspace w7\n");

    let message = failure_message(
        &environment
            .context(&["--workspace", "w1", "--tab", "w1:t8"])
            .run(),
    );
    assert_eq!(message, "leme: Herdr has no tab w1:t8\n");
}

#[test]
fn a_hanging_request_is_stopped_at_the_timeout() {
    let environment = Environment::new();
    environment.herdr.hang("workspace-list");

    let started = Instant::now();
    let output = environment
        .context(&["--pane", "w1:p2", "--timeout", "0.5"])
        .run();
    let elapsed = started.elapsed();

    let message = failure_message(&output);
    assert!(
        message.starts_with(
            "leme: Herdr operation workspace.list failed: no answer within 0.5 seconds"
        ),
        "{message}"
    );
    // The hung process sleeps for 30 seconds unless it is stopped.
    assert!(elapsed < Duration::from_secs(10), "took {elapsed:?}");
}

#[test]
fn a_reply_that_is_not_json_is_reported() {
    let environment = Environment::new();
    environment
        .herdr
        .respond("pane-current", "Pane w1:p2 is in tab 3");
    let message = failure_message(&environment.context(&["--pane", "w1:p2"]).run());
    assert_eq!(
        message,
        "leme: Herdr operation pane.current failed: the reply is not JSON: \
         \"Pane w1:p2 is in tab 3\"\n"
    );
}

#[test]
fn a_reply_missing_context_fields_is_reported() {
    let cases = [
        (
            "pane-current",
            herdr_reply(json!({"pane": {"pane_id": "w1:p2", "workspace_id": "w1"}})),
            "pane.current failed: the reply has no `tab_id` field",
        ),
        (
            "pane-current",
            herdr_reply(json!({"pane": {"pane_id": "w1:p2", "tab_id": "", "workspace_id": "w1"}})),
            "pane.current failed: the reply has no `tab_id` field",
        ),
        (
            "pane-current",
            json!({"id": "cli:pane:current"}).to_string(),
            "pane.current failed: the reply has no `result` field",
        ),
        (
            "workspace-list",
            herdr_reply(json!({"type": "workspace_list"})),
            "workspace.list failed: the reply has no `workspaces` field",
        ),
        (
            "tab-list",
            herdr_reply(json!({"tabs": "none"})),
            "tab.list failed: the reply has no `tabs` field",
        ),
    ];
    for (key, reply, expected) in cases {
        let fresh = Environment::new();
        fresh.herdr.respond(key, &reply);
        let message = failure_message(&fresh.context(&["--pane", "w1:p2"]).run());
        assert_eq!(
            message,
            format!("leme: Herdr operation {expected}\n"),
            "{key}"
        );
    }
}

#[test]
fn supplied_ids_that_contradict_herdr_are_rejected() {
    let environment = Environment::new();
    let message = failure_message(
        &environment
            .context(&["--pane", "w1:p2", "--workspace", "w2"])
            .run(),
    );
    assert_eq!(
        message,
        "leme: the supplied workspace w2 does not match the workspace Herdr reports, w1\n"
    );

    let message = failure_message(
        &environment
            .context(&["--pane", "w1:p2", "--tab", "w1:t1"])
            .run(),
    );
    assert_eq!(
        message,
        "leme: the supplied tab w1:t1 does not match the tab Herdr reports, w1:t3\n"
    );
}

#[test]
fn a_failed_readiness_check_stops_before_any_lookup() {
    let environment = Environment::new();
    environment.herdr.respond(
        "status-server",
        &json!({"status": "stopped", "running": false}).to_string(),
    );
    let message = failure_message(&environment.context(&["--pane", "w1:p2"]).run());
    assert_eq!(
        message,
        "leme: Herdr is not ready: Herdr server: the server is not running. Start Herdr by \
         running `herdr`, then run leme doctor again.\n"
    );
    assert!(environment.lookups().is_empty());
}

#[test]
fn a_herdr_missing_an_operation_is_not_ready() {
    let environment = Environment::new();
    environment.herdr.respond(
        "api-schema",
        &json!({"protocol": 22, "schemas": {"request": {"oneOf": [
            {"properties": {"method": {"const": "pane.current"}}},
        ]}}})
        .to_string(),
    );
    let message = failure_message(&environment.context(&["--pane", "w1:p2"]).run());
    assert!(
        message
            .starts_with("leme: Herdr is not ready: Herdr operations: this Herdr does not provide"),
        "{message}"
    );
    assert!(environment.lookups().is_empty());
}

#[test]
fn unconfirmed_compatibility_is_reported_without_stopping() {
    let environment = Environment::new();
    environment.herdr.respond("api-schema", &herdr_schema(21));

    let report = context_json(environment.context(&["--json", "--pane", "w1:p2"]));
    assert_eq!(report["pane"]["id"], "w1:p2");
    let unconfirmed = report["unconfirmed"].as_array().expect("unconfirmed list");
    assert_eq!(unconfirmed.len(), 1);
    assert_eq!(unconfirmed[0]["id"], "herdr_api");
    assert_eq!(unconfirmed[0]["status"], "uncertain");

    let text = success_stdout(&environment.context(&["--pane", "w1:p2"]).run());
    assert!(
        text.ends_with(
            "Unconfirmed: Herdr operations: provides the operations Leme uses, but speaks API \
             protocol 21; Leme was tested with protocol 22\n"
        ),
        "{text}"
    );
}

#[test]
fn without_any_context_nothing_is_run() {
    let environment = Environment::new();
    let message = failure_message(&environment.context(&[]).run());
    assert_eq!(
        message,
        "leme: no Herdr context: run Leme from a Herdr pane, which sets HERDR_PANE_ID, or pass \
         --pane, --tab or --workspace\n"
    );
    assert!(environment.herdr.calls().is_empty());

    // Blank inherited IDs count as absent.
    let message = failure_message(&environment.context(&[]).env("HERDR_PANE_ID", " ").run());
    assert!(message.starts_with("leme: no Herdr context"), "{message}");

    let message = failure_message(&environment.context(&["--tab", "w1:t1"]).run());
    assert_eq!(
        message,
        "leme: a tab can only be resolved together with its workspace\n"
    );
}

#[test]
fn herdr_is_found_on_path_when_herdr_bin_path_is_unusable() {
    let environment = Environment::new();
    let on_path = environment.herdr.executable().parent().unwrap();
    let path_value = on_path.to_str().unwrap();

    for bin_path in ["herdr", "/nonexistent/herdr", ""] {
        let report = context_json(
            environment
                .context(&["--json", "--pane", "w1:p2"])
                .env("HERDR_BIN_PATH", bin_path)
                .env("PATH", path_value),
        );
        assert_eq!(report["pane"]["id"], "w1:p2", "HERDR_BIN_PATH={bin_path:?}");
    }

    let message = failure_message(
        &environment
            .context(&["--pane", "w1:p2"])
            .env_remove("HERDR_BIN_PATH")
            .run(),
    );
    assert_eq!(
        message,
        "leme: cannot find the herdr executable; install Herdr (https://herdr.dev) and add \
         `herdr` to PATH\n"
    );
}

#[test]
fn a_herdr_that_cannot_start_is_not_ready() {
    let environment = Environment::new();
    // An executable file whose interpreter does not exist.
    let broken: PathBuf = environment.empty_path.script("broken-herdr", "exit 0");
    std::fs::write(&broken, "#!/nonexistent/interpreter\n").unwrap();

    let message = failure_message(
        &environment
            .context(&["--pane", "w1:p2"])
            .env("HERDR_BIN_PATH", broken.to_str().unwrap())
            .run(),
    );
    assert!(
        message.starts_with("leme: Herdr is not ready: Herdr:"),
        "{message}"
    );
    assert!(environment.herdr.calls().is_empty());
}
