//! Runs `leme feature open` on tasks that need a new checkout, with real Git
//! repositories, a real state database and a scripted `herdr` whose
//! `worktree create` runs the real `git worktree add` and which tracks the
//! tabs it opens.
//!
//! Paths have spaces. `PATH` holds only Git (and Neovim where a test adds
//! it), the fake Herdr is reached through `HERDR_BIN_PATH`, and the
//! developer's Herdr variables are removed, so no real Herdr session is used.

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use rusqlite::Connection;
use serde_json::{Value, json};
use support::{
    FakeHerdr, HERDR_VARIABLES, Leme, TestDir, Toolbox, add_worktree, commit, failure_message, git,
    herdr_error, init_repository, state_database,
};

/// A repository, a fake Herdr and a `PATH` holding only Git.
struct Environment {
    root: TestDir,
    repository: PathBuf,
    herdr: FakeHerdr,
    tools: Toolbox,
}

impl Environment {
    fn new() -> Self {
        let root = TestDir::new();
        let repository = root.path().join("main checkout");
        init_repository(&repository);
        let herdr = FakeHerdr::new(&root.path().join("herdr's tools dir"));
        let tools = Toolbox::new(&root.path().join("tools"));
        tools.link("git");
        Self {
            root,
            repository,
            herdr,
            tools,
        }
    }

    fn root(&self) -> &Path {
        self.root.path()
    }

    /// Makes an `nvim` visible to Leme, so the editor tab starts it.
    fn with_neovim(self) -> Self {
        self.tools.tool("nvim", "");
        self
    }

    /// Where the tests ask for the task's checkout.
    fn checkout(&self) -> PathBuf {
        self.root().join("feature checkouts/search feature")
    }

    /// `leme feature open <arguments>` run from `directory`.
    fn open(&self, directory: &Path, arguments: &[&str]) -> Leme {
        let mut all = vec!["feature", "open"];
        all.extend_from_slice(arguments);
        let mut leme = Leme::new(directory, self.root(), &all);
        for variable in HERDR_VARIABLES {
            leme = leme.env_remove(variable);
        }
        leme.env("PATH", self.tools.search_path())
            .env("HERDR_BIN_PATH", self.herdr.executable().to_str().unwrap())
            .env("HOME", self.root().join("home").to_str().unwrap())
    }

    /// Opens task T-1 on a new `feature/search` at [`Environment::checkout`],
    /// authorized with `--yes`.
    fn open_new(&self) -> Output {
        let checkout = self.checkout();
        self.open(
            &self.repository,
            &[
                "T-1",
                "--branch",
                "feature/search",
                "--path",
                checkout.to_str().unwrap(),
                "--yes",
                "--json",
            ],
        )
        .run()
    }

    /// The calls whose first two arguments are `group` and `action`.
    fn calls_to(&self, group: &str, action: &str) -> Vec<Vec<String>> {
        self.herdr
            .calls()
            .into_iter()
            .filter(|call| call.len() >= 2 && call[0] == group && call[1] == action)
            .collect()
    }

    /// The labels of the tabs created so far, in order.
    fn created_labels(&self) -> Vec<String> {
        self.calls_to("tab", "create")
            .iter()
            .map(|call| option(call, "--label").to_owned())
            .collect()
    }

    /// Answers `worktree open` as Herdr does for the open workspace `w5`.
    fn workspace_stays_open(&self) {
        self.herdr.open_worktree("w5", &self.checkout(), true);
    }

    /// The recorded layout tabs: role, workspace, tab, pane and readiness.
    fn layout_rows(&self) -> Vec<(String, String, String, String, i64)> {
        let connection = Connection::open(state_database(self.root())).expect("open state");
        let mut statement = connection
            .prepare("SELECT role, workspace, tab, pane, ready FROM workspace_tabs ORDER BY role")
            .unwrap();
        statement
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    /// How many checkouts of the repository have `branch`.
    fn checkouts_of(&self, branch: &str) -> usize {
        git(&self.repository, &["worktree", "list", "--porcelain"])
            .lines()
            .filter(|line| *line == format!("branch refs/heads/{branch}"))
            .count()
    }
}

/// The value following `name` in a recorded call.
fn option<'a>(call: &'a [String], name: &str) -> &'a str {
    let index = call
        .iter()
        .position(|argument| argument == name)
        .unwrap_or_else(|| panic!("{name} missing from {call:?}"));
    &call[index + 1]
}

/// Every value passed with `--env` in a recorded call.
fn env_values(call: &[String]) -> Vec<&str> {
    call.windows(2)
        .filter(|pair| pair[0] == "--env")
        .map(|pair| pair[1].as_str())
        .collect()
}

fn report(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "leme feature open failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("feature JSON")
}

fn statuses(report: &Value) -> Vec<(String, String)> {
    report["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tab| {
            (
                tab["label"].as_str().unwrap().to_owned(),
                tab["status"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[test]
fn an_authorized_feature_gets_its_branch_checkout_and_three_tabs() {
    let environment = Environment::new().with_neovim();
    let checkout = environment.checkout();
    // The command runs from a linked worktree whose branch is ahead of main:
    // the new branch must still start at main.
    let elsewhere = environment.root().join("other work");
    add_worktree(&environment.repository, &elsewhere, "feature/other");
    commit(&elsewhere, "other work");
    let main = git(&environment.repository, &["rev-parse", "main"]);

    let output = environment
        .open(
            &elsewhere,
            &[
                "T-1",
                "--branch",
                "feature/search",
                "--path",
                checkout.to_str().unwrap(),
                "--yes",
                "--json",
            ],
        )
        .run();

    let report = report(&output);
    assert_eq!(report["created"], json!({"branch": true, "base": "main"}));
    assert_eq!(report["checkout"], json!(checkout));
    assert_eq!(report["workspace"]["id"], "w5");
    assert_eq!(
        environment.calls_to("worktree", "create"),
        vec![vec![
            "worktree".to_owned(),
            "create".to_owned(),
            "--cwd".to_owned(),
            environment.repository.to_str().unwrap().to_owned(),
            "--branch".to_owned(),
            "feature/search".to_owned(),
            "--base".to_owned(),
            "main".to_owned(),
            "--path".to_owned(),
            checkout.to_str().unwrap().to_owned(),
            "--focus".to_owned(),
        ]]
    );
    assert_eq!(
        git(&checkout, &["branch", "--show-current"]),
        "feature/search"
    );
    assert_eq!(git(&checkout, &["rev-parse", "HEAD"]), main);

    // Each tab is opened in w5 with the checkout as an explicit directory
    // and the task's variables; only the agent tab takes focus.
    let creates = environment.calls_to("tab", "create");
    assert_eq!(environment.created_labels(), ["agent", "neovim", "tests"]);
    for call in &creates {
        assert_eq!(option(call, "--workspace"), "w5");
        assert_eq!(option(call, "--cwd"), checkout.to_str().unwrap());
        let variables = env_values(call);
        assert!(variables.contains(&"LEME_TASK_ID=T-1"), "{variables:?}");
        assert!(
            variables.contains(&"LEME_BRANCH=feature/search"),
            "{variables:?}"
        );
        assert!(
            variables.contains(&format!("LEME_CHECKOUT={}", checkout.display()).as_str()),
            "{variables:?}"
        );
        assert!(
            variables.contains(
                &format!("LEME_PROJECT_ID={}", report["project_id"].as_str().unwrap()).as_str()
            ),
            "{variables:?}"
        );
    }
    let focus: Vec<_> = creates
        .iter()
        .map(|call| call.last().unwrap().as_str())
        .collect();
    assert_eq!(focus, ["--focus", "--no-focus", "--no-focus"]);
    // Neovim is started in the editor tab's own pane.
    assert_eq!(
        environment.calls_to("pane", "run"),
        vec![vec![
            "pane".to_owned(),
            "run".to_owned(),
            "w5:p3".to_owned(),
            "nvim".to_owned()
        ]]
    );
    // Herdr's initial shell tab is closed, leaving exactly the layout.
    assert_eq!(environment.herdr.tabs(), ["w5:t2", "w5:t3", "w5:t4"]);
    assert_eq!(
        statuses(&report),
        [
            ("agent".to_owned(), "opened".to_owned()),
            ("neovim".to_owned(), "opened".to_owned()),
            ("tests".to_owned(), "opened".to_owned()),
        ]
    );
    let rows = environment.layout_rows();
    assert_eq!(rows.len(), 3);
    assert!(
        rows.iter().all(|row| row.1 == "w5" && row.4 == 1),
        "{rows:?}"
    );
}

#[test]
fn setup_interrupted_after_the_first_tab_adds_only_the_missing_tabs() {
    let environment = Environment::new().with_neovim();
    let checkout = environment.checkout();
    environment.herdr.allow_tabs(1);

    let interrupted = environment.open_new();

    let message = failure_message(&interrupted);
    assert!(
        message.contains("workspace w5 is open, but its neovim tab could not be set up"),
        "{message}"
    );
    assert!(message.contains("cannot open another tab"), "{message}");
    assert!(
        message.contains("The checkout and the agent tabs are kept"),
        "{message}"
    );
    assert!(
        message.contains("run `leme feature open T-1` again"),
        "{message}"
    );
    assert_eq!(environment.created_labels(), ["agent", "neovim"]);
    assert_eq!(environment.layout_rows().len(), 1);

    // Work done in the checkout meanwhile must survive the retry.
    fs::write(checkout.join("tracked.txt"), "first\n").unwrap();
    git(&checkout, &["add", "tracked.txt"]);
    git(&checkout, &["commit", "--quiet", "-m", "tracked"]);
    fs::write(checkout.join("tracked.txt"), "first\nchanged\n").unwrap();
    fs::write(checkout.join("untracked.txt"), "draft\n").unwrap();
    let head = git(&checkout, &["rev-parse", "HEAD"]);

    environment.herdr.allow_all_tabs();
    environment.workspace_stays_open();
    let resumed = report(
        &environment
            .open(&environment.repository, &["T-1", "--json"])
            .run(),
    );

    assert_eq!(resumed["created"], Value::Null, "no second checkout");
    assert_eq!(resumed["workspace"]["id"], "w5");
    assert_eq!(
        statuses(&resumed),
        [
            ("agent".to_owned(), "kept".to_owned()),
            ("neovim".to_owned(), "opened".to_owned()),
            ("tests".to_owned(), "opened".to_owned()),
        ]
    );
    // Only the missing tabs were created, each in the checkout.
    assert_eq!(
        environment.created_labels(),
        ["agent", "neovim", "neovim", "tests"]
    );
    for call in &environment.calls_to("tab", "create")[2..] {
        assert_eq!(option(call, "--workspace"), "w5");
        assert_eq!(option(call, "--cwd"), checkout.to_str().unwrap());
    }
    assert_eq!(environment.calls_to("worktree", "create").len(), 1);
    assert_eq!(environment.checkouts_of("feature/search"), 1);
    assert_eq!(git(&checkout, &["rev-parse", "HEAD"]), head);
    assert_eq!(
        git(&checkout, &["status", "--porcelain"]),
        " M tracked.txt\n?? untracked.txt"
    );
    assert_eq!(
        fs::read_to_string(checkout.join("untracked.txt")).unwrap(),
        "draft\n"
    );
    let rows = environment.layout_rows();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row.4 == 1), "{rows:?}");

    // A third invocation finds everything in place.
    let settled = report(
        &environment
            .open(&environment.repository, &["T-1", "--json"])
            .run(),
    );
    assert!(
        statuses(&settled)
            .iter()
            .all(|(_, status)| status == "kept")
    );
    assert_eq!(environment.calls_to("tab", "create").len(), 4);
}

#[test]
fn an_editor_whose_command_did_not_start_is_finished_in_its_own_tab() {
    let environment = Environment::new().with_neovim();
    environment.herdr.fail(
        "pane-run",
        1,
        &herdr_error("pane_not_found", "pane w5:p3 not found"),
    );

    let interrupted = environment.open_new();

    let message = failure_message(&interrupted);
    assert!(
        message.contains("its neovim tab could not be set up"),
        "{message}"
    );
    let editor = environment
        .layout_rows()
        .into_iter()
        .find(|row| row.0 == "editor")
        .expect("editor tab recorded");
    assert_eq!(
        (editor.2.as_str(), editor.3.as_str(), editor.4),
        ("w5:t3", "w5:p3", 0)
    );

    environment.herdr.respond("pane-run", "");
    environment.workspace_stays_open();
    let resumed = report(
        &environment
            .open(&environment.repository, &["T-1", "--json"])
            .run(),
    );

    assert_eq!(
        statuses(&resumed),
        [
            ("agent".to_owned(), "kept".to_owned()),
            ("neovim".to_owned(), "resumed".to_owned()),
            ("tests".to_owned(), "opened".to_owned()),
        ]
    );
    // No second editor tab: Neovim is started in the recorded pane.
    assert_eq!(environment.created_labels(), ["agent", "neovim", "tests"]);
    let runs = environment.calls_to("pane", "run");
    assert_eq!(runs.len(), 2);
    assert!(
        runs.iter()
            .all(|call| call[2] == "w5:p3" && call[3] == "nvim")
    );
}

#[test]
fn closed_tabs_and_a_replaced_workspace_are_laid_out_again() {
    let environment = Environment::new().with_neovim();
    report(&environment.open_new());

    // The user closed the tests tab.
    let tests_tab = environment
        .layout_rows()
        .into_iter()
        .find(|row| row.0 == "tests")
        .unwrap()
        .2;
    environment.herdr.close_tab(&tests_tab);
    environment.workspace_stays_open();
    let reopened = report(
        &environment
            .open(&environment.repository, &["T-1", "--json"])
            .run(),
    );
    assert_eq!(
        statuses(&reopened),
        [
            ("agent".to_owned(), "kept".to_owned()),
            ("neovim".to_owned(), "kept".to_owned()),
            ("tests".to_owned(), "opened".to_owned()),
        ]
    );

    // The workspace was closed and Herdr reopened the checkout as w8.
    environment
        .herdr
        .open_worktree("w8", &environment.checkout(), false);
    let replaced = report(
        &environment
            .open(&environment.repository, &["T-1", "--json"])
            .run(),
    );
    assert_eq!(replaced["previous_workspace"], "w5");
    assert!(
        statuses(&replaced)
            .iter()
            .all(|(_, status)| status == "opened")
    );
    let in_w8: Vec<_> = environment
        .calls_to("tab", "create")
        .into_iter()
        .filter(|call| option(call, "--workspace") == "w8")
        .collect();
    assert_eq!(in_w8.len(), 3);
    assert!(environment.layout_rows().iter().all(|row| row.1 == "w8"));
}

#[test]
fn an_existing_branch_is_checked_out_without_a_base() {
    let environment = Environment::new();
    git(&environment.repository, &["branch", "feature/search"]);

    let report = report(&environment.open_new());

    assert_eq!(report["created"], json!({"branch": false, "base": null}));
    let create = &environment.calls_to("worktree", "create")[0];
    assert!(!create.contains(&"--base".to_owned()), "{create:?}");
    assert_eq!(
        git(&environment.checkout(), &["branch", "--show-current"]),
        "feature/search"
    );
    // Without Neovim on PATH the editor tab keeps its shell.
    assert!(environment.calls_to("pane", "run").is_empty());
    let notes = report["notes"].as_array().unwrap();
    assert!(
        notes.iter().any(|note| note
            .as_str()
            .unwrap()
            .contains("Neovim (nvim) was not found")),
        "{notes:?}"
    );
}

#[test]
fn the_default_location_follows_herdrs_convention() {
    let environment = Environment::new();
    let expected = environment
        .root()
        .join("home/.herdr/worktrees/main checkout/feature-search");

    let report = report(
        &environment
            .open(
                &environment.repository,
                &["T-1", "--branch", "feature/search", "--yes", "--json"],
            )
            .run(),
    );

    assert_eq!(report["checkout"], json!(expected));
    assert!(expected.join(".git").exists());
}

#[test]
fn nothing_changes_without_authorization() {
    let environment = Environment::new();
    let checkout = environment.checkout();

    let output = environment
        .open(
            &environment.repository,
            &[
                "T-1",
                "--branch",
                "feature/search",
                "--path",
                checkout.to_str().unwrap(),
            ],
        )
        .run();

    let message = failure_message(&output);
    assert!(
        message.contains("Opening task T-1 changes Git:"),
        "{message}"
    );
    assert!(
        message.contains("create branch feature/search from main, the integration branch"),
        "{message}"
    );
    assert!(
        message.contains(&format!("in a new worktree at {}", checkout.display())),
        "{message}"
    );
    assert!(message.contains("Run again with --yes"), "{message}");
    assert!(environment.calls_to("worktree", "create").is_empty());
    assert!(environment.calls_to("tab", "create").is_empty());
    assert!(!checkout.exists());
    assert_eq!(
        git(
            &environment.repository,
            &["branch", "--list", "feature/search"]
        ),
        ""
    );
    let connection = Connection::open(state_database(environment.root())).unwrap();
    let tasks: i64 = connection
        .query_row("SELECT count(*) FROM tasks", [], |row| row.get(0))
        .unwrap();
    assert_eq!(tasks, 0);
}

#[test]
fn an_occupied_path_or_a_missing_base_is_refused_before_git_changes() {
    let environment = Environment::new();
    fs::create_dir_all(environment.checkout()).unwrap();

    let occupied = environment.open_new();

    let message = failure_message(&occupied);
    assert!(
        message.contains(&format!(
            "{} already exists; choose another location with --path",
            environment.checkout().display()
        )),
        "{message}"
    );

    fs::remove_dir_all(environment.checkout()).unwrap();
    fs::write(
        environment.repository.join("leme.toml"),
        "integration_branch = \"develop\"\n",
    )
    .unwrap();
    let no_base = environment.open_new();

    let message = failure_message(&no_base);
    assert!(
        message.contains("the integration branch develop does not exist in this repository"),
        "{message}"
    );
    assert!(environment.calls_to("worktree", "create").is_empty());
    assert_eq!(
        git(
            &environment.repository,
            &["branch", "--list", "feature/search"]
        ),
        ""
    );
}

#[test]
fn a_failed_worktree_creation_keeps_no_workspace_and_can_be_retried() {
    let environment = Environment::new();
    environment.herdr.fail(
        "worktree-create",
        1,
        &herdr_error("worktree_create_failed", "fatal: cannot lock ref"),
    );

    let failed = environment.open_new();

    let message = failure_message(&failed);
    assert!(
        message.contains("Herdr operation worktree.create failed: fatal: cannot lock ref"),
        "{message}"
    );
    assert!(environment.calls_to("tab", "create").is_empty());

    environment.herdr.reset("worktree-create");
    // The retry needs neither --branch nor --path: both were recorded.
    let retried = report(
        &environment
            .open(&environment.repository, &["T-1", "--yes", "--json"])
            .run(),
    );
    assert_eq!(retried["checkout"], json!(environment.checkout()));
    assert_eq!(environment.checkouts_of("feature/search"), 1);
}
