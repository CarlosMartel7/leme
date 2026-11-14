//! Runs `leme dashboard` in a real pseudo-terminal, as a user would.
//!
//! The PTY tests are ignored by default because they need a Linux terminal
//! device. Run them with:
//!
//! ```sh
//! cargo test --locked --test dashboard_e2e -- --ignored --test-threads=1
//! ```

mod support;

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

use support::pty::Pty;
use support::{Leme, TestDir, add_worktree, failure_message, git, init_repository};

/// A narrow terminal, as in a Herdr side pane.
const NARROW: (u16, u16) = (40, 16);

/// Everything Git could change, captured to prove the dashboard changes none
/// of it.
fn git_state(repository: &Path) -> (String, String, Vec<u8>) {
    (
        git(
            repository,
            &[
                "status",
                "--porcelain=v2",
                "--branch",
                "--untracked-files=all",
            ],
        ),
        git(repository, &["for-each-ref"]),
        fs::read(repository.join(".git/index")).expect("read the index"),
    )
}

/// A directory of stand-in agent CLIs that record any invocation in
/// `marker`.
fn fake_agents(directory: &Path, marker: &Path) {
    fs::create_dir_all(directory).unwrap();
    for name in ["claude", "codex"] {
        let path = directory.join(name);
        fs::write(
            &path,
            format!("#!/bin/sh\necho {name} >> '{}'\n", marker.display()),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
#[ignore = "drives a real pseudo-terminal; run with --ignored --test-threads=1"]
fn a_new_project_opens_narrow_and_quits_back_to_a_normal_terminal() {
    let root = TestDir::new();
    let repository = root.path().join("my project");
    init_repository(&repository);
    let agents = root.path().join("agents");
    let marker = root.path().join("agent-invocations");
    fake_agents(&agents, &marker);
    let path = format!("{}:{}", agents.display(), env::var("PATH").unwrap());
    let before = git_state(&repository);

    let command = Leme::new(&repository, root.path(), &["dashboard"])
        .env("PATH", &path)
        .into_command();
    let (columns, rows) = NARROW;
    let mut pty = Pty::spawn(command, columns, rows);
    let shell_modes = pty.modes();
    assert!(shell_modes.is_canonical(), "a new PTY starts canonical");

    pty.expect("Tasks (0)");
    pty.expect("No tasks are registered");
    pty.expect("Up to date");
    pty.expect("r refresh  ? help  q quit");
    assert!(
        !pty.modes().is_canonical(),
        "the dashboard reads keys in raw mode"
    );

    pty.send(b"?");
    pty.expect("close this help");
    // Esc closes help. Waiting for that keeps the next key from arriving
    // together with Esc, which terminals read as Alt plus that key.
    pty.send(b"\x1b");
    pty.expect("No tasks are registered");
    pty.send(b"r");
    pty.expect("Refreshing");
    pty.expect("Up to date");
    pty.send(b"q");

    let status = pty.wait();
    assert!(status.success(), "exit status {status:?}");

    let raw = pty.raw_output();
    let entered = find(raw, b"\x1b[?1049h").expect("entered the alternate screen");
    let left = rfind(raw, b"\x1b[?1049l").expect("left the alternate screen");
    assert!(left > entered, "left the alternate screen last");
    assert!(
        rfind(raw, b"\x1b[?25h") > rfind(raw, b"\x1b[?25l"),
        "the cursor is visible again"
    );
    assert_eq!(pty.modes(), shell_modes, "terminal modes are restored");
    assert_eq!(
        pty.type_line("echo hello"),
        "echo hello\n",
        "the next program receives a normal line"
    );

    assert_eq!(git_state(&repository), before, "Git is unchanged");
    assert!(
        !marker.exists(),
        "an agent CLI was started: {}",
        fs::read_to_string(&marker).unwrap_or_default()
    );
}

#[test]
#[ignore = "drives a real pseudo-terminal; run with --ignored --test-threads=1"]
fn outside_a_repository_the_error_is_shown_and_ctrl_c_restores_the_terminal() {
    let root = TestDir::new();
    let outside = root.path().join("outside");
    fs::create_dir(&outside).unwrap();

    let command = Leme::new(&outside, root.path(), &["dashboard"]).into_command();
    let mut pty = Pty::spawn(command, 100, 20);
    let shell_modes = pty.modes();

    pty.expect("Cannot load the project");
    pty.expect("is not inside a Git repository");
    pty.send(b"r");
    pty.expect("Loading the project");
    pty.expect("Cannot load the project");
    pty.send(b"\x03");

    let status = pty.wait();
    assert!(status.success(), "exit status {status:?}");
    assert_eq!(pty.modes(), shell_modes, "terminal modes are restored");
    assert_eq!(pty.type_line("ls"), "ls\n");
}

#[test]
fn without_a_terminal_the_dashboard_refuses_to_start() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    init_repository(&repository);

    let output = Leme::new(&repository, root.path(), &["dashboard"]).run();
    let message = failure_message(&output);
    assert!(
        message.contains("the dashboard needs an interactive terminal"),
        "{message}"
    );
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .rposition(|window| window == needle)
}

/// The header row, where only the timer's owner shows the clock.
fn header(pty: &mut Pty) -> String {
    pty.screen().lines().next().unwrap_or_default().to_owned()
}

/// A dashboard of `directory` at 60x16, once its timer line has settled.
fn open_dashboard(directory: &Path, root: &Path, expected: &str) -> Pty {
    let command = Leme::new(directory, root, &["dashboard"]).into_command();
    let mut pty = Pty::spawn(command, 60, 16);
    pty.expect("Up to date");
    pty.expect(expected);
    pty
}

#[test]
#[ignore = "drives a real pseudo-terminal; run with --ignored --test-threads=1"]
fn a_second_dashboard_cannot_control_the_timer() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    let worktree = root.path().join("feature");
    init_repository(&repository);
    add_worktree(&repository, &worktree, "feature/one");

    let mut owner = open_dashboard(&repository, root.path(), "f focus  b break");
    assert!(
        header(&mut owner).chars().any(|c| c.is_ascii_digit()),
        "clock"
    );

    // A dashboard in a linked worktree of the same repository.
    let mut other = open_dashboard(
        &worktree,
        root.path(),
        "Another dashboard owns the timer; reopen after it closes",
    );
    assert!(
        !header(&mut other).chars().any(|c| c.is_ascii_digit()),
        "no clock: {}",
        header(&mut other)
    );
    assert!(
        other.screen().contains("feature/one"),
        "project details stay"
    );

    // Neither starting nor any other timer key does anything there.
    other.send(b"f");
    other.send(b"b");
    other.send(b"p");
    other.send(b"x");
    owner.send(b"f");
    owner.expect("p pause  x cancel");
    other.send(b"x");
    other.send(b" ");
    let screen = other.screen_after(Duration::from_millis(600));
    assert!(
        screen.contains("Another dashboard owns the timer"),
        "{screen}"
    );
    assert!(
        !screen.contains("Focus") && !screen.contains("Break"),
        "{screen}"
    );
    let screen = owner.screen_after(Duration::from_millis(300));
    assert!(
        screen.contains("p pause  x cancel"),
        "still running: {screen}"
    );

    // The owner quits; the other dashboard does not take over.
    owner.send(b"q");
    assert!(owner.wait().success());
    other.send(b"r");
    other.expect("Refreshing");
    other.expect("Up to date");
    let screen = other.screen_after(Duration::from_millis(300));
    assert!(
        screen.contains("Another dashboard owns the timer"),
        "{screen}"
    );
    other.send(b"q");
    assert!(other.wait().success());

    // Reopened, the worktree's dashboard owns the timer and restores it.
    let mut reopened = open_dashboard(&worktree, root.path(), "p pause  x cancel");
    assert!(
        reopened.screen().contains("Focus 2"),
        "{}",
        reopened.screen()
    );

    // After a crash, the next dashboard owns the timer again.
    reopened.kill();
    let mut recovered = open_dashboard(&repository, root.path(), "p pause  x cancel");
    recovered.send(b"x");
    recovered.expect("f focus  b break");
    recovered.send(b"q");
    assert!(recovered.wait().success());
}
