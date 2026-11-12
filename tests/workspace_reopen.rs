//! Runs `leme feature open` on existing checkouts, with real Git
//! repositories, a real state database and a scripted `herdr`.
//!
//! Repositories and worktrees have spaces in their paths. `PATH` holds only
//! Git, the fake Herdr is reached through `HERDR_BIN_PATH`, and the
//! developer's Herdr variables are removed, so no real Herdr session is used.

mod support;

use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::Barrier;
use std::thread;

use rusqlite::Connection;
use serde_json::{Value, json};
use support::{
    FakeHerdr, HERDR_VARIABLES, Leme, TestDir, Toolbox, add_worktree, failure_message, git,
    herdr_error, init_repository, state_database, status_json,
};

/// A project with a main checkout and a fake Herdr, sharing one state
/// directory with any other project the test creates under the same root.
struct Project {
    repository: PathBuf,
}

impl Project {
    /// Creates a repository at `root`/`name` with one commit on `main`.
    fn new(root: &Path, name: &str) -> Self {
        let repository = root.join(name);
        init_repository(&repository);
        Self { repository }
    }

    /// Adds a linked worktree at `root`/`name` on a new `branch`.
    fn worktree(&self, root: &Path, name: &str, branch: &str) -> PathBuf {
        let path = root.join(name);
        add_worktree(&self.repository, &path, branch);
        path
    }
}

/// The fake Herdr and a `PATH` that holds only Git.
struct Environment {
    root: TestDir,
    herdr: FakeHerdr,
    tools: Toolbox,
}

impl Environment {
    fn new() -> Self {
        let root = TestDir::new();
        let herdr = FakeHerdr::new(&root.path().join("herdr's tools dir"));
        let tools = Toolbox::new(&root.path().join("tools"));
        tools.link("git");
        Self { root, herdr, tools }
    }

    fn root(&self) -> &Path {
        self.root.path()
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
    }

    /// Runs `leme feature open <arguments> --json`, requiring success.
    fn open_json(&self, directory: &Path, arguments: &[&str]) -> Value {
        let mut all = arguments.to_vec();
        all.push("--json");
        let output = self.open(directory, &all).run();
        assert!(
            output.status.success(),
            "leme feature open failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("feature JSON")
    }

    /// The `worktree open` requests sent to Herdr, in order.
    fn opened(&self) -> Vec<Vec<String>> {
        self.herdr
            .calls()
            .into_iter()
            .filter(|call| call.starts_with(&["worktree".to_owned(), "open".to_owned()]))
            .collect()
    }

    /// Every task row in the state database: project, ID, branch, worktree
    /// and workspace.
    fn task_rows(&self) -> Vec<(String, String, String, PathBuf, Option<String>)> {
        let connection = Connection::open(state_database(self.root())).expect("open state");
        let mut statement = connection
            .prepare(
                "SELECT project_id, id, branch, worktree, workspace FROM tasks \
                 ORDER BY project_id, id",
            )
            .unwrap();
        statement
            .query_map([], |row| {
                let path: Vec<u8> = row.get(3)?;
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    PathBuf::from(std::ffi::OsStr::from_bytes(&path)),
                    row.get(4)?,
                ))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }
}

/// The arguments Leme sends to open `checkout` of `repository`.
fn open_request(repository: &Path, checkout: &Path) -> Vec<String> {
    vec![
        "worktree".to_owned(),
        "open".to_owned(),
        "--cwd".to_owned(),
        repository.to_str().unwrap().to_owned(),
        "--path".to_owned(),
        checkout.to_str().unwrap().to_owned(),
        "--focus".to_owned(),
    ]
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn reopening_a_task_reuses_its_record_and_its_one_workspace() {
    let environment = Environment::new();
    let project = Project::new(environment.root(), "main checkout");
    let checkout = project.worktree(environment.root(), "feature one", "feature/one");
    environment.herdr.open_worktree("w2", &checkout, false);

    let first = environment.open_json(&project.repository, &["T-1", "--branch", "feature/one"]);

    assert_eq!(first["task"], "T-1");
    assert_eq!(first["branch"], "feature/one");
    assert_eq!(first["base"], "main");
    assert_eq!(first["checkout"], json!(checkout));
    assert_eq!(
        first["workspace"],
        json!({"id": "w2", "label": "feature one", "already_open": false})
    );
    assert_eq!(first["previous_workspace"], Value::Null);

    // Herdr now reports the workspace as open; the task needs no --branch.
    environment.herdr.open_worktree("w2", &checkout, true);
    let output = environment.open(&checkout, &["T-1"]).run();

    assert!(output.status.success(), "{output:?}");
    let text = stdout(&output);
    assert!(text.contains("Branch:    feature/one"), "{text}");
    assert!(
        text.contains("Workspace: w2  feature one  (already open, focused)"),
        "{text}"
    );
    // Each value reached Herdr as one argument, from either checkout.
    let request = open_request(&project.repository, &checkout);
    assert_eq!(environment.opened(), vec![request.clone(), request]);
    let rows = environment.task_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (&rows[0].1, &rows[0].2, &rows[0].3, &rows[0].4),
        (
            &"T-1".to_owned(),
            &"feature/one".to_owned(),
            &checkout,
            &Some("w2".to_owned())
        )
    );
    let status = status_json(&project.repository, environment.root());
    assert_eq!(status["tasks"][0]["workspace"], "w2");
}

#[test]
fn the_configured_integration_branch_is_reported_as_the_base() {
    let environment = Environment::new();
    let project = Project::new(environment.root(), "main checkout");
    fs::write(
        project.repository.join("leme.toml"),
        "integration_branch = \"develop\"\n",
    )
    .unwrap();
    let checkout = project.worktree(environment.root(), "feature one", "feature/one");
    environment.herdr.open_worktree("w2", &checkout, false);

    let report = environment.open_json(&checkout, &["T-1", "--branch", "feature/one"]);

    assert_eq!(report["base"], "develop");
}

#[test]
fn duplicate_tasks_and_branches_are_refused_before_herdr_is_asked() {
    let environment = Environment::new();
    let project = Project::new(environment.root(), "main checkout");
    let one = project.worktree(environment.root(), "feature one", "feature/one");
    project.worktree(environment.root(), "feature two", "feature/two");
    environment.herdr.open_worktree("w2", &one, false);
    environment.open_json(&project.repository, &["T-1", "--branch", "feature/one"]);

    let other_task = environment
        .open(&project.repository, &["T-2", "--branch", "feature/one"])
        .run();
    let other_branch = environment
        .open(&project.repository, &["T-1", "--branch", "feature/two"])
        .run();
    let unregistered = environment.open(&project.repository, &["T-3"]).run();

    let message = failure_message(&other_task);
    assert!(
        message.contains("branch feature/one already belongs to task T-1"),
        "{message}"
    );
    let message = failure_message(&other_branch);
    assert!(
        message.contains(
            "task T-1 is registered with branch feature/one, not feature/two; omit --branch \
             or pass --branch feature/one"
        ),
        "{message}"
    );
    let message = failure_message(&unregistered);
    assert!(
        message.contains("task T-3 is not registered yet; pass --branch"),
        "{message}"
    );
    assert_eq!(environment.opened().len(), 1);
    let rows = environment.task_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].2, "feature/one");
}

#[test]
fn concurrent_registrations_of_one_branch_leave_one_task() {
    let environment = Environment::new();
    let project = Project::new(environment.root(), "main checkout");
    let checkout = project.worktree(environment.root(), "feature one", "feature/one");
    environment.herdr.open_worktree("w2", &checkout, true);
    let barrier = Barrier::new(4);

    let outputs: Vec<Output> = thread::scope(|scope| {
        let handles: Vec<_> = (1..=4)
            .map(|index| {
                let task = format!("T-{index}");
                let leme =
                    environment.open(&project.repository, &[&task, "--branch", "feature/one"]);
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    leme.run()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("leme process"))
            .collect()
    });

    let succeeded = outputs
        .iter()
        .filter(|output| output.status.success())
        .count();
    assert_eq!(succeeded, 1, "{outputs:?}");
    for output in outputs.iter().filter(|output| !output.status.success()) {
        let message = failure_message(output);
        assert!(message.contains("already belongs to task"), "{message}");
    }
    assert_eq!(environment.task_rows().len(), 1);
}

#[test]
fn the_same_branch_name_in_another_repository_is_a_separate_task() {
    let environment = Environment::new();
    let first = Project::new(environment.root(), "first project");
    let second = Project::new(environment.root(), "second project");
    let first_checkout = first.worktree(environment.root(), "first feature", "feature/one");
    let second_checkout = second.worktree(environment.root(), "second feature", "feature/one");

    environment
        .herdr
        .open_worktree("w2", &first_checkout, false);
    let one = environment.open_json(&first.repository, &["T-1", "--branch", "feature/one"]);
    environment
        .herdr
        .open_worktree("w3", &second_checkout, false);
    let two = environment.open_json(&second.repository, &["T-1", "--branch", "feature/one"]);

    assert_ne!(one["project_id"], two["project_id"]);
    assert_eq!(two["checkout"], json!(second_checkout));
    assert_eq!(
        environment.opened(),
        vec![
            open_request(&first.repository, &first_checkout),
            open_request(&second.repository, &second_checkout),
        ]
    );
    let rows = environment.task_rows();
    assert_eq!(rows.len(), 2);
    let workspaces: Vec<_> = rows
        .iter()
        .map(|row| (row.3.clone(), row.4.clone()))
        .collect();
    assert!(workspaces.contains(&(first_checkout, Some("w2".to_owned()))));
    assert!(workspaces.contains(&(second_checkout, Some("w3".to_owned()))));
}

#[test]
fn a_branch_in_the_main_checkout_is_identified_and_refused() {
    let environment = Environment::new();
    let project = Project::new(environment.root(), "main checkout");
    git(
        &project.repository,
        &["switch", "--quiet", "-c", "feature/one"],
    );

    let output = environment
        .open(&project.repository, &["T-1", "--branch", "feature/one"])
        .run();

    let message = failure_message(&output);
    assert!(
        message.contains(&format!(
            "branch feature/one is checked out in the main checkout {}",
            project.repository.display()
        )),
        "{message}"
    );
    assert!(environment.herdr.calls().is_empty());
    assert!(environment.task_rows().is_empty());
}

#[test]
fn a_branch_without_a_usable_checkout_is_refused() {
    let environment = Environment::new();
    let project = Project::new(environment.root(), "main checkout");
    git(&project.repository, &["branch", "feature/none"]);
    let gone = project.worktree(environment.root(), "gone feature", "feature/gone");
    fs::remove_dir_all(&gone).unwrap();

    let none = environment
        .open(&project.repository, &["T-1", "--branch", "feature/none"])
        .run();
    let missing = environment
        .open(&project.repository, &["T-2", "--branch", "feature/gone"])
        .run();
    let invalid = environment
        .open(&project.repository, &["T-3", "--branch", "@{-1}"])
        .run();

    // Without a checkout, one would have to be created, which needs --yes
    // when there is no terminal to ask in.
    let message = failure_message(&none);
    assert!(
        message.contains("check out the existing branch feature/none"),
        "{message}"
    );
    assert!(message.contains("Nothing was changed"), "{message}");
    let message = failure_message(&missing);
    assert!(
        message.contains(&format!(
            "Git records {} as the checkout of feature/gone, but the directory is missing",
            gone.display()
        )),
        "{message}"
    );
    let message = failure_message(&invalid);
    assert!(
        message.contains("`@{-1}` is not a valid Git branch name"),
        "{message}"
    );
    // Only the read-only readiness probes reached Herdr.
    assert!(
        environment
            .herdr
            .calls()
            .iter()
            .all(|call| call[0] == "--version"
                || call[..2] == ["status", "server"]
                || call[..2] == ["api", "schema"]),
        "{:?}",
        environment.herdr.calls()
    );
    assert!(environment.task_rows().is_empty());
}

#[test]
fn a_stale_workspace_id_is_replaced_by_the_one_herdr_returns() {
    let environment = Environment::new();
    let project = Project::new(environment.root(), "main checkout");
    let checkout = project.worktree(environment.root(), "feature one", "feature/one");
    environment.herdr.open_worktree("w2", &checkout, false);
    environment.open_json(&project.repository, &["T-1", "--branch", "feature/one"]);

    // The workspace was closed, so Herdr opens a new one.
    environment.herdr.open_worktree("w7", &checkout, false);
    let report = environment.open_json(&project.repository, &["T-1"]);

    assert_eq!(report["workspace"]["id"], "w7");
    assert_eq!(report["previous_workspace"], "w2");
    assert_eq!(environment.task_rows()[0].4.as_deref(), Some("w7"));
    let output = environment.open(&project.repository, &["T-1"]).run();
    assert!(
        !stdout(&output).contains("no longer this checkout's"),
        "an unchanged workspace is not reported as replaced"
    );
}

#[test]
fn a_moved_worktree_is_followed_and_the_old_path_reported() {
    let environment = Environment::new();
    let project = Project::new(environment.root(), "main checkout");
    let before = project.worktree(environment.root(), "feature one", "feature/one");
    environment.herdr.open_worktree("w2", &before, false);
    environment.open_json(&project.repository, &["T-1", "--branch", "feature/one"]);
    let after = environment.root().join("moved feature");
    git(
        &project.repository,
        &[
            "worktree",
            "move",
            before.to_str().unwrap(),
            after.to_str().unwrap(),
        ],
    );

    environment.herdr.open_worktree("w3", &after, false);
    let report = environment.open_json(&project.repository, &["T-1"]);

    assert_eq!(report["checkout"], json!(after));
    assert_eq!(report["previous_checkout"], json!(before));
    assert_eq!(environment.task_rows()[0].3, after);
}

#[test]
fn a_herdr_failure_keeps_the_task_without_a_workspace() {
    let environment = Environment::new();
    let project = Project::new(environment.root(), "main checkout");
    let checkout = project.worktree(environment.root(), "feature one", "feature/one");
    environment.herdr.fail(
        "worktree-open",
        1,
        &herdr_error("worktree_not_found", "worktree path not found"),
    );

    let output = environment
        .open(&project.repository, &["T-1", "--branch", "feature/one"])
        .run();

    let message = failure_message(&output);
    assert!(
        message.contains(&format!("Herdr has no worktree {}", checkout.display())),
        "{message}"
    );
    let rows = environment.task_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].4, None);

    // The next attempt completes the record.
    environment.herdr.open_worktree("w2", &checkout, false);
    environment.open_json(&project.repository, &["T-1"]);
    assert_eq!(environment.task_rows()[0].4.as_deref(), Some("w2"));
}

#[test]
fn a_workspace_for_another_checkout_is_not_recorded() {
    let environment = Environment::new();
    let project = Project::new(environment.root(), "main checkout");
    project.worktree(environment.root(), "feature one", "feature/one");
    let elsewhere = environment.root().join("elsewhere");
    environment.herdr.open_worktree("w9", &elsewhere, true);

    let output = environment
        .open(&project.repository, &["T-1", "--branch", "feature/one"])
        .run();

    let message = failure_message(&output);
    assert!(message.contains("the supplied checkout"), "{message}");
    assert!(
        message.contains(&elsewhere.display().to_string()),
        "{message}"
    );
    assert_eq!(environment.task_rows()[0].4, None);
}

#[test]
fn an_unready_herdr_records_nothing_and_opens_nothing() {
    let environment = Environment::new();
    let project = Project::new(environment.root(), "main checkout");
    project.worktree(environment.root(), "feature one", "feature/one");
    environment.herdr.fail(
        "status-server",
        1,
        &herdr_error("server_unavailable", "no server running"),
    );

    let output = environment
        .open(&project.repository, &["T-1", "--branch", "feature/one"])
        .run();

    let message = failure_message(&output);
    assert!(message.contains("Herdr is not ready"), "{message}");
    assert!(environment.opened().is_empty());
    assert!(!state_database(environment.root()).exists() || environment.task_rows().is_empty());
}
