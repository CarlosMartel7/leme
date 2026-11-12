//! Fixtures for tests that run `leme` against real, disposable Git repositories.
//!
//! Each test target compiles this module separately and uses only part of it.
#![allow(dead_code)]

pub mod pty;

use std::env;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

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

    /// Removes an inherited environment variable from the process.
    pub fn env_remove(mut self, name: &str) -> Self {
        self.command.env_remove(name);
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

/// The real executable called `name` on the test's own `PATH`.
pub fn system_executable(name: &str) -> PathBuf {
    leme::diagnostics::find_executable(name, env::var_os("PATH").as_deref())
        .unwrap_or_else(|| panic!("{name} is not installed"))
}

/// A directory of controlled executables, used as the entire `PATH` of a
/// `leme` process so that only the tools a test provides can be found.
pub struct Toolbox {
    path: PathBuf,
}

impl Toolbox {
    /// Creates an empty toolbox at `path`.
    pub fn new(path: &Path) -> Self {
        fs::create_dir_all(path).expect("create toolbox");
        Self {
            path: path.to_path_buf(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The `PATH` value that makes only this toolbox visible.
    pub fn search_path(&self) -> &str {
        self.path.to_str().expect("UTF-8 toolbox path")
    }

    /// Adds an executable `/bin/sh` script called `name` running `body`.
    /// Scripts see only this toolbox on `PATH`, so they should call other
    /// programs by absolute path, for example through [`system_executable`].
    pub fn script(&self, name: &str, body: &str) -> PathBuf {
        let path = self.path.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("make executable");
        path
    }

    /// Adds a tool called `name` that prints `output` and succeeds,
    /// whatever its arguments.
    pub fn tool(&self, name: &str, output: &str) -> PathBuf {
        self.script(name, &format!("printf '%s\\n' {}", shell_quote(output)))
    }

    /// Makes the real executable called `name` available in the toolbox.
    pub fn link(&self, name: &str) -> PathBuf {
        let path = self.path.join(name);
        symlink(system_executable(name), &path).expect("link executable");
        path
    }
}

/// Quotes `text` as one `/bin/sh` word.
pub fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// The variables Herdr sets in the processes it starts. Tests remove them
/// so a test run inside Herdr never reaches the developer's session.
pub const HERDR_VARIABLES: [&str; 6] = [
    "HERDR_BIN_PATH",
    "HERDR_ENV",
    "HERDR_PANE_ID",
    "HERDR_SOCKET_PATH",
    "HERDR_TAB_ID",
    "HERDR_WORKSPACE_ID",
];

/// A scripted stand-in for the `herdr` CLI.
///
/// Each request is answered from files under `responses/`, keyed by its
/// first two arguments (`pane-current`, `workspace-list`, `tab-list`,
/// `worktree-open`, `status-server`, `api-schema`) or `version` for
/// `--version`. A new fake
/// answers like a healthy Herdr whose server runs the tested protocol, and
/// whose workspace, tab and pane lists are empty until a test fills them.
/// Every invocation's arguments are recorded one per line, so tests can
/// check that each value arrived as a single argument.
pub struct FakeHerdr {
    directory: PathBuf,
    executable: PathBuf,
}

/// A recorded invocation's arguments end with this line.
const CALL_END: &str = "--end-of-call--";

impl FakeHerdr {
    /// Writes the fake to `directory`/herdr. The directory may contain
    /// spaces or quotes.
    pub fn new(directory: &Path) -> Self {
        fs::create_dir_all(directory.join("responses")).expect("create fake Herdr");
        let executable = directory.join("herdr");
        let quoted = shell_quote(directory.to_str().expect("UTF-8 fake Herdr path"));
        let cat = shell_quote(system_executable("cat").to_str().unwrap());
        let sleep = shell_quote(system_executable("sleep").to_str().unwrap());
        let script = format!(
            "#!/bin/sh\n\
             dir={quoted}\n\
             {{ for argument in \"$@\"; do printf '%s\\n' \"$argument\"; done; \
             printf '%s\\n' {CALL_END}; }} >> \"$dir/calls\"\n\
             case \"$1\" in --version) key=version ;; *) key=\"$1-$2\" ;; esac\n\
             response=\"$dir/responses/$key\"\n\
             if [ -f \"$response.hang\" ]; then exec {sleep} 30; fi\n\
             if [ ! -f \"$response.stdout\" ] && [ ! -f \"$response.stderr\" ]; then\n\
             printf 'unexpected request: %s\\n' \"$*\" >&2; exit 64; fi\n\
             if [ -f \"$response.stderr\" ]; then {cat} \"$response.stderr\" >&2; fi\n\
             if [ -f \"$response.stdout\" ]; then {cat} \"$response.stdout\"; fi\n\
             if [ -f \"$response.status\" ]; then exit \"$({cat} \"$response.status\")\"; fi\n"
        );
        fs::write(&executable, script).expect("write fake Herdr");
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
            .expect("make fake Herdr executable");
        let fake = Self {
            directory: directory.to_path_buf(),
            executable,
        };
        fake.respond("version", "herdr 0.9.5");
        fake.respond("status-server", &herdr_server_status());
        fake.respond(
            "api-schema",
            &herdr_schema(leme::diagnostics::TESTED_HERDR_PROTOCOL),
        );
        fake.respond("workspace-list", &herdr_reply(json!({"workspaces": []})));
        fake.respond("tab-list", &herdr_reply(json!({"tabs": []})));
        fake
    }

    /// The fake executable.
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    fn response(&self, key: &str, extension: &str) -> PathBuf {
        self.directory
            .join("responses")
            .join(format!("{key}.{extension}"))
    }

    fn clear(&self, key: &str) {
        for extension in ["stdout", "stderr", "status", "hang"] {
            let _ = fs::remove_file(self.response(key, extension));
        }
    }

    /// Answers request `key` by printing `stdout` and succeeding.
    pub fn respond(&self, key: &str, stdout: &str) {
        self.clear(key);
        fs::write(self.response(key, "stdout"), format!("{stdout}\n")).expect("write response");
    }

    /// Answers request `key` by printing `stderr` and exiting with `status`.
    pub fn fail(&self, key: &str, status: i32, stderr: &str) {
        self.clear(key);
        fs::write(self.response(key, "stderr"), format!("{stderr}\n")).expect("write response");
        fs::write(self.response(key, "status"), status.to_string()).expect("write response");
    }

    /// Makes request `key` hang until it is stopped.
    pub fn hang(&self, key: &str) {
        self.clear(key);
        fs::write(self.response(key, "hang"), "").expect("write response");
    }

    /// Answers `worktree open` with workspace `workspace` for `checkout`.
    pub fn open_worktree(&self, workspace: &str, checkout: &Path, already_open: bool) {
        self.respond(
            "worktree-open",
            &herdr_worktree_opened(workspace, checkout, already_open),
        );
    }

    /// The arguments of every invocation, in order.
    pub fn calls(&self) -> Vec<Vec<String>> {
        let text = fs::read_to_string(self.directory.join("calls")).unwrap_or_default();
        let mut calls = Vec::new();
        let mut current = Vec::new();
        for line in text.lines() {
            if line == CALL_END {
                calls.push(std::mem::take(&mut current));
            } else {
                current.push(line.to_owned());
            }
        }
        calls
    }
}

/// A successful Herdr reply wrapping `result`, as the CLI prints it.
pub fn herdr_reply(result: Value) -> String {
    json!({"id": "cli:test", "result": result}).to_string()
}

/// A Herdr error reply, as the CLI prints it on stderr.
pub fn herdr_error(code: &str, message: &str) -> String {
    json!({"id": "cli:test", "error": {"code": code, "message": message}}).to_string()
}

/// `herdr worktree open` reporting workspace `workspace` for `checkout`, in
/// the shape Herdr 0.9.5 prints.
pub fn herdr_worktree_opened(workspace: &str, checkout: &Path, already_open: bool) -> String {
    let name = checkout
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    herdr_reply(json!({
        "type": "worktree_opened",
        "already_open": already_open,
        "workspace": {
            "workspace_id": workspace,
            "label": name,
            "focused": true,
            "worktree": {"checkout_path": checkout, "is_linked_worktree": true},
        },
        "tab": {"tab_id": format!("{workspace}:t1"), "workspace_id": workspace},
        "root_pane": {"pane_id": format!("{workspace}:p1"), "workspace_id": workspace},
    }))
}

/// `herdr status server --json` from a running, compatible server.
pub fn herdr_server_status() -> String {
    json!({
        "status": "running",
        "running": true,
        "version": "0.9.5",
        "protocol": leme::diagnostics::TESTED_HERDR_PROTOCOL,
        "compatible": true,
        "restart_needed": false,
    })
    .to_string()
}

/// `herdr api schema --json` listing every operation Leme uses.
pub fn herdr_schema(protocol: u64) -> String {
    let requests: Vec<_> = leme::diagnostics::HERDR_OPERATIONS
        .iter()
        .map(|method| json!({"properties": {"method": {"const": method, "type": "string"}}}))
        .collect();
    json!({"protocol": protocol, "schemas": {"request": {"oneOf": requests}}}).to_string()
}
