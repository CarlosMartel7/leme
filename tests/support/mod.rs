//! Fixtures for tests that run `leme` against real, disposable Git repositories.
//!
//! Each test target compiles this module separately and uses only part of it.
#![allow(dead_code)]

pub mod pty;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

/// A unique directory in the system temporary directory, removed on drop.
pub struct TestDir {
    path: PathBuf,
}

impl TestDir {
    pub fn new() -> Self {
        let name = format!(
            "leme-test-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        );
        let path = env::temp_dir().join(name);
        fs::create_dir(&path).expect("create test directory");
        // Git reports resolved paths, so expected values must be resolved too.
        let path = path.canonicalize().expect("resolve test directory");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Builds a process that ignores the developer's Git configuration and any
/// repository outside the test directory.
fn isolated(mut command: Command, ceiling: &Path) -> Command {
    for variable in [
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_COMMON_DIR",
        "GIT_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_WORK_TREE",
    ] {
        command.env_remove(variable);
    }
    command
        .env("GIT_CEILING_DIRECTORIES", ceiling)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Leme Test")
        .env("GIT_AUTHOR_EMAIL", "leme-test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Leme Test")
        .env("GIT_COMMITTER_EMAIL", "leme-test@example.invalid");
    command
}

/// Runs Git in `directory`, returning its raw output.
pub fn git_output(directory: &Path, arguments: &[&str]) -> Output {
    let mut command = isolated(Command::new("git"), directory);
    command.current_dir(directory).args(arguments);
    command.output().expect("run git")
}

/// Runs Git in `directory`, requiring success, and returns its stdout.
pub fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = git_output(directory, arguments);
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_owned()
}

/// Creates a repository with one commit on `main`.
pub fn init_repository(path: &Path) {
    fs::create_dir_all(path).expect("create repository directory");
    git(path, &["init", "--quiet", "--initial-branch=main"]);
    commit(path, "first");
}

/// Creates an empty commit.
pub fn commit(path: &Path, message: &str) {
    git(
        path,
        &["commit", "--quiet", "--allow-empty", "--message", message],
    );
}

/// Adds a linked worktree at `path` on a new `branch`.
pub fn add_worktree(repository: &Path, path: &Path, branch: &str) {
    git(
        repository,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            branch,
            path.to_str().expect("UTF-8 worktree path"),
        ],
    );
}

/// A `leme` process ready to run in a test directory.
pub struct Leme {
    command: Command,
}

impl Leme {
    /// Prepares `leme <arguments>` to run from `directory`, seeing no
    /// repository above `ceiling` and keeping its state under `ceiling`.
    pub fn new(directory: &Path, ceiling: &Path, arguments: &[&str]) -> Self {
        let mut command = isolated(Command::new(env!("CARGO_BIN_EXE_leme")), ceiling);
        command
            .current_dir(directory)
            .args(arguments)
            .env("XDG_STATE_HOME", ceiling.join("state"));
        Self { command }
    }

    /// Sets an environment variable for the process.
    pub fn env(mut self, name: &str, value: &str) -> Self {
        self.command.env(name, value);
        self
    }

    /// The prepared process, for callers that start it themselves.
    pub fn into_command(self) -> Command {
        self.command
    }

    /// Runs the process and captures its result.
    pub fn run(mut self) -> Output {
        self.command.output().expect("run leme")
    }
}

/// The state database of a `leme` process created with this `ceiling`.
pub fn state_database(ceiling: &Path) -> PathBuf {
    ceiling.join("state/leme/leme.db")
}

/// Runs `leme status --json` from `directory` and returns the parsed result.
pub fn status_json(directory: &Path, ceiling: &Path) -> Value {
    let output = Leme::new(directory, ceiling, &["status", "--json"]).run();
    assert!(
        output.status.success(),
        "leme status --json failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("status JSON")
}

/// Requires a failed exit with status 1 and returns stderr.
pub fn failure_message(output: &Output) -> String {
    assert_eq!(output.status.code(), Some(1), "unexpected exit status");
    assert!(output.stdout.is_empty(), "failure printed to stdout");
    String::from_utf8_lossy(&output.stderr).into_owned()
}
