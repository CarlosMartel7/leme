//! Runs `leme status` against real SQLite databases and Git repositories.

mod support;

use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::Barrier;
use std::thread;

use leme::state::SCHEMA_VERSION;
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use support::{
    Leme, TestDir, add_worktree, commit, failure_message, git, init_repository, state_database,
    status_json,
};

fn project_id(status: &Value) -> &str {
    status["project_id"].as_str().expect("project ID")
}

fn path_bytes(path: &Path) -> &[u8] {
    path.as_os_str().as_bytes()
}

fn connect(root: &Path) -> Connection {
    Connection::open(state_database(root)).expect("open state database")
}

fn project_count(root: &Path) -> i64 {
    connect(root)
        .query_row("SELECT count(*) FROM projects", [], |row| row.get(0))
        .expect("count projects")
}

fn schema_version(connection: &Connection) -> i64 {
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read schema version")
}

fn table_exists(connection: &Connection, name: &str) -> bool {
    connection
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            params![name],
            |row| row.get::<_, i64>(0),
        )
        .expect("read schema")
        == 1
}

fn register_task(root: &Path, project: &str, id: &str, branch: &str, worktree: &Path) {
    connect(root)
        .execute(
            "INSERT INTO tasks (project_id, id, branch, worktree) VALUES (?1, ?2, ?3, ?4)",
            params![project, id, branch, path_bytes(worktree)],
        )
        .expect("register task");
}

/// Creates a state database whose parent directory exists.
fn create_database(root: &Path) -> Connection {
    let path = state_database(root);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    Connection::open(path).expect("create state database")
}

#[test]
fn first_use_creates_an_empty_task_list() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    init_repository(&repository);

    let status = status_json(&repository, root.path());

    assert_eq!(project_id(&status).len(), 32);
    assert_eq!(status["tasks"], json!([]));
    assert_eq!(project_count(root.path()), 1);
}

#[test]
fn the_project_id_survives_a_restart() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    init_repository(&repository);

    let first = status_json(&repository, root.path());
    let second = status_json(&repository, root.path());

    assert_eq!(project_id(&first), project_id(&second));
    assert_eq!(project_count(root.path()), 1);
}

#[test]
fn worktrees_share_one_project_and_report_their_own_checkout() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    let worktree = root.path().join("feature");
    init_repository(&repository);
    add_worktree(&repository, &worktree, "feature/one");

    let main = status_json(&repository, root.path());
    let linked = status_json(&worktree, root.path());

    assert_eq!(project_id(&main), project_id(&linked));
    assert_eq!(project_count(root.path()), 1);
    assert_eq!(main["checkout"], json!(repository));
    assert_eq!(main["branch"], "main");
    assert_eq!(linked["checkout"], json!(worktree));
    assert_eq!(linked["branch"], "feature/one");
}

#[test]
fn the_project_outlives_a_removed_worktree() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    let worktree = root.path().join("feature");
    init_repository(&repository);
    add_worktree(&repository, &worktree, "feature/one");

    let before = status_json(&worktree, root.path());
    git(
        &repository,
        &["worktree", "remove", worktree.to_str().unwrap()],
    );
    let after = status_json(&repository, root.path());

    assert_eq!(project_id(&before), project_id(&after));
}

#[test]
fn separate_repositories_are_separate_projects() {
    let root = TestDir::new();
    let first = root.path().join("first");
    let second = root.path().join("second");
    init_repository(&first);
    init_repository(&second);

    let first = status_json(&first, root.path());
    let second = status_json(&second, root.path());

    assert_ne!(project_id(&first), project_id(&second));
    assert_eq!(project_count(root.path()), 2);
}

#[test]
fn a_repository_cannot_be_registered_twice() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    init_repository(&repository);
    status_json(&repository, root.path());

    let duplicate = connect(root.path()).execute(
        "INSERT INTO projects (id, repository) VALUES ('other', ?1)",
        params![path_bytes(&repository)],
    );

    assert!(duplicate.is_err());
    assert_eq!(project_count(root.path()), 1);
}

#[test]
fn concurrent_first_access_resolves_to_one_project() {
    const PROCESSES: usize = 12;

    for _ in 0..5 {
        let root = TestDir::new();
        let repository = root.path().join("project");
        let worktree = root.path().join("feature");
        init_repository(&repository);
        add_worktree(&repository, &worktree, "feature/one");

        let barrier = Barrier::new(PROCESSES);
        let statuses: Vec<Value> = thread::scope(|scope| {
            let handles: Vec<_> = (0..PROCESSES)
                .map(|index| {
                    let directory = if index % 2 == 0 {
                        &repository
                    } else {
                        &worktree
                    };
                    let barrier = &barrier;
                    let root = root.path();
                    scope.spawn(move || {
                        barrier.wait();
                        status_json(directory, root)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().expect("status process"))
                .collect()
        });

        let id = project_id(&statuses[0]);
        assert!(statuses.iter().all(|status| project_id(status) == id));
        assert_eq!(project_count(root.path()), 1);
        assert_eq!(schema_version(&connect(root.path())), SCHEMA_VERSION as i64);
    }
}

#[test]
fn registered_tasks_are_reported_in_text_and_json() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    let worktree = root.path().join("feature");
    init_repository(&repository);
    add_worktree(&repository, &worktree, "feature/one");
    let project = project_id(&status_json(&repository, root.path())).to_owned();
    let other_worktree = root.path().join("second");
    register_task(root.path(), &project, "T-2", "feature/two", &other_worktree);
    register_task(root.path(), &project, "T-1", "feature/one", &worktree);

    let status = status_json(&worktree, root.path());

    assert_eq!(
        status["tasks"],
        json!([
            {"id": "T-1", "branch": "feature/one", "worktree": worktree, "workspace": null},
            {"id": "T-2", "branch": "feature/two", "worktree": other_worktree, "workspace": null},
        ])
    );

    let output = Leme::new(&worktree, root.path(), &["status"]).run();
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    let expected = format!(
        "Tasks:\n  T-1  feature/one  {}\n  T-2  feature/two  {}\n",
        worktree.display(),
        other_worktree.display()
    );
    assert!(text.ends_with(&expected), "{text}");
}

#[test]
fn a_project_reports_only_its_own_tasks() {
    let root = TestDir::new();
    let first = root.path().join("first");
    let second = root.path().join("second");
    init_repository(&first);
    init_repository(&second);
    let project = project_id(&status_json(&first, root.path())).to_owned();
    register_task(
        root.path(),
        &project,
        "T-1",
        "feature/one",
        &root.path().join("feature"),
    );

    assert_eq!(status_json(&second, root.path())["tasks"], json!([]));
}

#[test]
fn a_task_key_cannot_repeat_within_a_project() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    init_repository(&repository);
    let project = project_id(&status_json(&repository, root.path())).to_owned();
    register_task(root.path(), &project, "T-1", "one", &root.path().join("a"));
    let connection = connect(root.path());
    let insert = |id: &str, branch: &str, worktree: &str| {
        connection.execute(
            "INSERT INTO tasks (project_id, id, branch, worktree) VALUES (?1, ?2, ?3, ?4)",
            params![project, id, branch, path_bytes(&root.path().join(worktree))],
        )
    };

    assert!(insert("T-1", "two", "b").is_err(), "repeated task ID");
    assert!(insert("T-2", "one", "b").is_err(), "repeated branch");
    assert!(insert("T-2", "two", "a").is_err(), "repeated worktree");
    assert!(insert("T-2", "two", "b").is_ok());
}

#[test]
fn a_failed_migration_rolls_back_and_can_be_retried() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    init_repository(&repository);
    // The migration creates `projects`, then fails on this existing table.
    create_database(root.path())
        .execute_batch("CREATE TABLE tasks (conflict TEXT);")
        .unwrap();

    let output = Leme::new(&repository, root.path(), &["status"]).run();

    let message = failure_message(&output);
    assert!(
        message.contains(state_database(root.path()).to_str().unwrap()),
        "{message}"
    );
    let connection = connect(root.path());
    assert_eq!(schema_version(&connection), 0);
    assert!(!table_exists(&connection, "projects"));

    connection.execute_batch("DROP TABLE tasks;").unwrap();
    let status = status_json(&repository, root.path());

    assert_eq!(status["tasks"], json!([]));
    assert_eq!(schema_version(&connection), SCHEMA_VERSION as i64);
}

#[test]
fn a_database_from_a_newer_leme_is_left_untouched() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    init_repository(&repository);
    let connection = create_database(root.path());
    connection.pragma_update(None, "user_version", 99).unwrap();

    let output = Leme::new(&repository, root.path(), &["status"]).run();

    let message = failure_message(&output);
    assert!(message.contains("schema version 99"), "{message}");
    assert!(message.contains("update Leme"), "{message}");
    assert_eq!(schema_version(&connection), 99);
    assert!(!table_exists(&connection, "projects"));
}

#[test]
fn an_unusable_state_directory_is_reported() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    init_repository(&repository);
    let blocked = root.path().join("state");
    fs::write(&blocked, "").unwrap();

    let output = Leme::new(&repository, root.path(), &["status"]).run();

    let message = failure_message(&output);
    assert!(message.contains(blocked.to_str().unwrap()), "{message}");
}

#[test]
fn state_defaults_to_the_home_directory() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    let home = root.path().join("home");
    init_repository(&repository);

    // A relative XDG_STATE_HOME must be ignored.
    let output = Leme::new(&repository, root.path(), &["status"])
        .env("XDG_STATE_HOME", "relative")
        .env("HOME", home.to_str().unwrap())
        .run();

    assert!(output.status.success());
    assert!(home.join(".local/state/leme/leme.db").is_file());
    assert!(!repository.join("relative").exists());
}

#[test]
fn persisting_state_leaves_git_untouched() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    let worktree = root.path().join("feature");
    init_repository(&repository);
    commit(&repository, "second");
    add_worktree(&repository, &worktree, "feature/one");
    fs::write(repository.join("staged"), "staged\n").unwrap();
    git(&repository, &["add", "staged"]);
    fs::write(repository.join("untracked"), "untracked\n").unwrap();

    let snapshot = || {
        (
            git(&repository, &["log", "--all", "--format=%H %s"]),
            git(&repository, &["status", "--porcelain"]),
            git(&worktree, &["status", "--porcelain"]),
            git(&repository, &["worktree", "list", "--porcelain"]),
            fs::read(repository.join(".git/index")).unwrap(),
        )
    };
    let before = snapshot();

    status_json(&repository, root.path());
    status_json(&worktree, root.path());

    assert_eq!(snapshot(), before);
}
