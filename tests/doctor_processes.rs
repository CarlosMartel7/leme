//! Runs `leme doctor` against controlled executables on a controlled `PATH`.
//!
//! Every tool except Git is a small script, so each test decides exactly
//! what the tools answer. Real Git is linked in because repository discovery
//! needs it.

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, Instant};

use leme::diagnostics::{HERDR_OPERATIONS, TESTED_HERDR_PROTOCOL};
use rusqlite::Connection;
use serde_json::{Value, json};
use support::{
    Leme, TestDir, Toolbox, add_worktree, git, init_repository, shell_quote, state_database,
    system_executable,
};

/// Version lines printed by the healthy fake tools.
const VERSIONS: [(&str, &str); 9] = [
    ("herdr", "herdr 0.9.5"),
    ("nvim", "NVIM v0.12.6"),
    ("claude", "2.1.319 (Claude Code)"),
    ("codex", "codex-cli 0.167.0"),
    ("gh", "gh version 2.46.0 (2025-01-13)"),
    ("git-flow", "1.12.3 (AVH Edition)"),
    ("cargo", "cargo 1.94.1 (29ea6fb6a 2026-03-24)"),
    ("rustc", "rustc 1.94.1 (e408947bf 2026-03-25)"),
    ("cc", "cc (Debian 14.3.0-5) 14.3.0"),
];

/// A disposable repository, a toolbox and a log of every tool invocation.
struct Environment {
    root: TestDir,
    /// Outside `root`, so recording invocations never changes `root`.
    log_dir: TestDir,
    repository: PathBuf,
    tools: Toolbox,
}

impl Environment {
    /// A repository and a toolbox in which every tool works.
    fn healthy() -> Self {
        let root = TestDir::new();
        let log_dir = TestDir::new();
        let repository = root.path().join("project");
        init_repository(&repository);
        let tools = Toolbox::new(&root.path().join("tools"));
        tools.link("git");
        let environment = Self {
            root,
            log_dir,
            repository,
            tools,
        };
        for (name, version) in VERSIONS {
            if name != "herdr" {
                environment.logged(name, &print(version));
            }
        }
        environment.herdr(
            &print(&running_server()),
            &print(&schema(TESTED_HERDR_PROTOCOL)),
        );
        environment
    }

    fn log(&self) -> PathBuf {
        self.log_dir.path().join("invocations")
    }

    /// Replaces tool `name` with a script that records its arguments, then
    /// runs `body`.
    fn logged(&self, name: &str, body: &str) {
        let log = shell_quote(self.log().to_str().unwrap());
        self.tools.script(
            name,
            &format!("printf '%s\\n' \"{name} $*\" >> {log}\n{body}"),
        );
    }

    /// Replaces Herdr with a script answering `herdr --version` normally and
    /// the server status and schema queries with the given script bodies.
    fn herdr(&self, status: &str, schema: &str) {
        self.logged(
            "herdr",
            &format!(
                "case \"$*\" in\n\
                 '--version') {} ;;\n\
                 'status server --json') {status} ;;\n\
                 'api schema --json') {schema} ;;\n\
                 *) exit 64 ;;\n\
                 esac",
                print("herdr 0.9.5")
            ),
        );
    }

    fn remove(&self, name: &str) {
        fs::remove_file(self.tools.path().join(name)).expect("remove tool");
    }

    /// `leme doctor <arguments>` run in the repository with only the toolbox
    /// on `PATH`.
    fn doctor(&self, arguments: &[&str]) -> Leme {
        self.doctor_in(&self.repository, arguments)
    }

    fn doctor_in(&self, directory: &Path, arguments: &[&str]) -> Leme {
        let mut all = vec!["doctor"];
        all.extend_from_slice(arguments);
        Leme::new(directory, self.root.path(), &all)
            .env("PATH", self.tools.search_path())
            .env("HOME", self.root.path().join("home").to_str().unwrap())
    }

    fn invocations(&self) -> Vec<String> {
        let mut lines: Vec<_> = fs::read_to_string(self.log())
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect();
        lines.sort();
        lines
    }
}

/// A script body printing `text` exactly.
fn print(text: &str) -> String {
    format!("printf '%s\\n' {}", shell_quote(text))
}

fn running_server() -> String {
    json!({
        "status": "running",
        "running": true,
        "version": "0.9.5",
        "protocol": TESTED_HERDR_PROTOCOL,
        "compatible": true,
        "restart_needed": false,
    })
    .to_string()
}

fn schema(protocol: u64) -> String {
    let requests: Vec<_> = HERDR_OPERATIONS
        .iter()
        .chain(&["server.stop", "workspace.close"])
        .map(|method| json!({"properties": {"method": {"const": method, "type": "string"}}}))
        .collect();
    json!({"protocol": protocol, "schemas": {"request": {"oneOf": requests}}}).to_string()
}

fn success_stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "leme doctor failed ({:?}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).expect("UTF-8 output")
}

fn doctor_json(leme: Leme) -> Value {
    serde_json::from_str(&success_stdout(&leme.run())).expect("doctor JSON")
}

fn entry<'a>(report: &'a Value, section: &str, id: &str) -> &'a Value {
    report[section]
        .as_array()
        .expect("section")
        .iter()
        .find(|entry| entry["id"] == id)
        .unwrap_or_else(|| panic!("no {section} entry {id}"))
}

fn capability<'a>(report: &'a Value, id: &str) -> &'a Value {
    entry(report, "capabilities", id)
}

fn check<'a>(report: &'a Value, id: &str) -> &'a Value {
    let section = if ["cargo", "rustc", "cc"].contains(&id) {
        "build"
    } else {
        "checks"
    };
    entry(report, section, id)
}

fn statuses(report: &Value, section: &str) -> BTreeMap<String, String> {
    report[section]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["id"].as_str().unwrap().to_owned(),
                entry["status"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[test]
fn every_action_is_available_when_every_tool_answers() {
    let environment = Environment::healthy();

    let report = doctor_json(environment.doctor(&["--json"]));

    for (id, status) in statuses(&report, "capabilities") {
        assert_eq!(status, "available", "capability {id}");
    }
    for section in ["checks", "build"] {
        for (id, status) in statuses(&report, section) {
            assert_eq!(status, "ok", "check {id}");
        }
    }
    let tools = environment.tools.path();
    assert_eq!(check(&report, "gh")["version"], "2.46.0");
    assert_eq!(
        check(&report, "gh")["path"],
        tools.join("gh").to_str().unwrap()
    );
    assert_eq!(check(&report, "neovim")["version"], "0.12.6");
    assert_eq!(
        check(&report, "git")["path"],
        tools.join("git").to_str().unwrap()
    );
    assert_eq!(check(&report, "herdr_server")["version"], "0.9.5");
    assert_eq!(
        check(&report, "repository")["path"],
        environment.repository.to_str().unwrap()
    );
}

#[test]
fn without_gh_only_github_queries_are_unavailable() {
    let environment = Environment::healthy();
    environment.remove("gh");

    let report = doctor_json(environment.doctor(&["--json"]));
    let text = success_stdout(&environment.doctor(&[]).run());

    assert_eq!(capability(&report, "github")["status"], "unavailable");
    assert_eq!(capability(&report, "github")["blocked_by"], json!(["gh"]));
    for id in [
        "status",
        "workspaces",
        "editor",
        "feature_agent",
        "captain",
        "gitflow",
    ] {
        assert_eq!(capability(&report, id)["status"], "available", "{id}");
    }
    assert_eq!(check(&report, "gh")["detail"], "`gh` was not found on PATH");
    assert!(text.contains("  unavailable  Query GitHub pull requests and checks\n"));
    assert!(text.contains("blocked by: GitHub CLI\n"));
    assert!(text.contains("  available    Open feature workspaces, worktrees and tabs in Herdr\n"));
}

#[test]
fn a_tool_that_hangs_is_stopped_at_the_timeout() {
    let environment = Environment::healthy();
    let pid_file = environment.log_dir.path().join("gh.pid");
    let sleep = system_executable("sleep");
    environment.tools.script(
        "gh",
        &format!(
            "echo $$ > {}\nexec {} 30",
            shell_quote(pid_file.to_str().unwrap()),
            shell_quote(sleep.to_str().unwrap())
        ),
    );

    let started = Instant::now();
    let report = doctor_json(environment.doctor(&["--json", "--timeout", "0.5"]));
    let elapsed = started.elapsed();

    assert!(elapsed < Duration::from_secs(10), "took {elapsed:?}");
    let gh = check(&report, "gh");
    assert_eq!(gh["status"], "failed");
    assert_eq!(
        gh["detail"],
        "`gh --version` did not finish within 0.5 seconds"
    );
    assert_eq!(capability(&report, "github")["status"], "unavailable");
    assert_eq!(capability(&report, "workspaces")["status"], "available");
    // The hung process was stopped, not left running.
    let pid = fs::read_to_string(&pid_file).expect("pid file");
    assert!(
        !Path::new("/proc").join(pid.trim()).exists(),
        "the timed-out process {} is still running",
        pid.trim()
    );
}

#[test]
fn a_hanging_herdr_server_query_fails_only_the_server_check() {
    let environment = Environment::healthy();
    let sleep = shell_quote(system_executable("sleep").to_str().unwrap());
    environment.herdr(
        &format!("exec {sleep} 30"),
        &print(&schema(TESTED_HERDR_PROTOCOL)),
    );

    let started = Instant::now();
    let report = doctor_json(environment.doctor(&["--json", "--timeout", "0.5"]));

    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(check(&report, "herdr")["status"], "ok");
    assert_eq!(check(&report, "herdr_server")["status"], "failed");
    assert_eq!(
        check(&report, "herdr_server")["detail"],
        "`herdr status server --json` did not finish within 0.5 seconds"
    );
    assert_eq!(check(&report, "herdr_api")["status"], "ok");
    assert_eq!(capability(&report, "workspaces")["status"], "unavailable");
    assert_eq!(capability(&report, "github")["status"], "available");
}

#[test]
fn a_background_process_holding_the_output_open_cannot_stall_doctor() {
    let environment = Environment::healthy();
    let pid_file = environment.log_dir.path().join("background.pid");
    let sleep = system_executable("sleep");
    // The tool exits at once, but the process it leaves behind keeps its
    // output stream open.
    environment.tools.script(
        "gh",
        &format!(
            "{} 30 &\necho $! > {}\n{}",
            shell_quote(sleep.to_str().unwrap()),
            shell_quote(pid_file.to_str().unwrap()),
            print("gh version 2.46.0")
        ),
    );

    let started = Instant::now();
    let report = doctor_json(environment.doctor(&["--json", "--timeout", "0.5"]));
    let elapsed = started.elapsed();
    if let Ok(pid) = fs::read_to_string(&pid_file) {
        let _ = std::process::Command::new(system_executable("kill"))
            .arg(pid.trim())
            .status();
    }

    assert!(elapsed < Duration::from_secs(10), "took {elapsed:?}");
    assert_eq!(check(&report, "gh")["status"], "failed");
    assert!(
        check(&report, "gh")["detail"]
            .as_str()
            .unwrap()
            .contains("did not finish within 0.5 seconds")
    );
}

#[test]
fn malformed_output_is_classified_instead_of_aborting() {
    let environment = Environment::healthy();
    // No minimum version applies to gh, so any answer is enough.
    environment.logged("gh", &print("GitHub CLI (development build)"));
    // Cargo has a minimum, so an unreadable version is uncertain.
    environment.logged("cargo", &print("cargo: version unknown"));
    // Bytes that are not UTF-8 are decoded leniently.
    environment.logged("claude", r"printf '\377\376 2.1.319\n'");
    environment.logged(
        "codex",
        "echo 'codex: broken installation' >&2\necho 'second line' >&2\nexit 3",
    );
    environment.herdr(&print("server: up"), &print("{\"protocol\": 22"));

    let report = doctor_json(environment.doctor(&["--json"]));

    assert_eq!(check(&report, "gh")["status"], "ok");
    assert_eq!(check(&report, "gh")["version"], Value::Null);
    assert!(
        check(&report, "gh")["detail"]
            .as_str()
            .unwrap()
            .contains("no recognizable version")
    );
    assert_eq!(check(&report, "cargo")["status"], "uncertain");
    assert_eq!(check(&report, "claude")["status"], "ok");
    assert_eq!(check(&report, "claude")["version"], "2.1.319");
    assert_eq!(check(&report, "codex")["status"], "failed");
    assert_eq!(
        check(&report, "codex")["detail"],
        "`codex --version` failed: codex: broken installation"
    );
    assert_eq!(check(&report, "herdr_server")["status"], "uncertain");
    assert_eq!(check(&report, "herdr_api")["status"], "uncertain");
    assert_eq!(capability(&report, "workspaces")["status"], "uncertain");
    assert_eq!(capability(&report, "captain")["status"], "unavailable");
    assert_eq!(capability(&report, "status")["status"], "available");
}

#[test]
fn a_herdr_that_cannot_report_its_version_is_not_queried_further() {
    let environment = Environment::healthy();
    environment.logged("herdr", "echo 'herdr: cannot load libc' >&2\nexit 127");

    let report = doctor_json(environment.doctor(&["--json"]));

    assert_eq!(check(&report, "herdr")["status"], "failed");
    assert_eq!(
        check(&report, "herdr")["detail"],
        "`herdr --version` failed: herdr: cannot load libc"
    );
    for id in ["herdr_server", "herdr_api"] {
        assert_eq!(check(&report, id)["status"], "skipped", "{id}");
    }
    let herdr_calls: Vec<_> = environment
        .invocations()
        .into_iter()
        .filter(|call| call.starts_with("herdr"))
        .collect();
    assert_eq!(herdr_calls, ["herdr --version"]);
    assert_eq!(capability(&report, "workspaces")["status"], "unavailable");
    assert_eq!(capability(&report, "github")["status"], "available");
}

#[test]
fn a_stopped_herdr_server_and_an_untested_protocol_are_told_apart() {
    let environment = Environment::healthy();
    environment.herdr(
        &format!(
            "{}\nexit 1",
            print(r#"{"status":"stopped","running":false}"#)
        ),
        &print(&schema(TESTED_HERDR_PROTOCOL + 1)),
    );

    let report = doctor_json(environment.doctor(&["--json"]));

    assert_eq!(check(&report, "herdr_server")["status"], "failed");
    assert_eq!(
        check(&report, "herdr_server")["detail"],
        "the server is not running"
    );
    assert_eq!(check(&report, "herdr_api")["status"], "uncertain");
    let workspaces = capability(&report, "workspaces");
    assert_eq!(workspaces["status"], "unavailable");
    assert_eq!(workspaces["blocked_by"], json!(["herdr_server"]));
    assert_eq!(workspaces["uncertain"], json!(["herdr_api"]));
}

#[test]
fn text_and_json_report_the_same_values() {
    let environment = Environment::healthy();
    environment.remove("gh");
    environment.logged("cargo", &print("cargo (unknown)"));
    environment.herdr(&print(&running_server()), &print(&schema(21)));

    let report = doctor_json(environment.doctor(&["--json"]));
    let text = success_stdout(&environment.doctor(&[]).run());

    let name_of = |id: &Value| {
        let id = id.as_str().unwrap();
        check(&report, id)["name"].as_str().unwrap().to_owned()
    };
    let mut expected = vec!["Leme actions".to_owned()];
    for capability in report["capabilities"].as_array().unwrap() {
        expected.push(format!(
            "  {:<12} {}",
            capability["status"].as_str().unwrap(),
            capability["action"].as_str().unwrap()
        ));
        for (field, label) in [("blocked_by", "blocked by"), ("uncertain", "unconfirmed")] {
            let names: Vec<_> = capability[field]
                .as_array()
                .unwrap()
                .iter()
                .map(name_of)
                .collect();
            if !names.is_empty() {
                expected.push(format!("  {:<12}   {label}: {}", "", names.join(", ")));
            }
        }
    }
    for (section, heading) in [
        ("checks", "Checks"),
        ("build", "Building from source (not needed to run Leme)"),
    ] {
        expected.push(String::new());
        expected.push(heading.to_owned());
        for check in report[section].as_array().unwrap() {
            let mut line = format!(
                "  {:<12} {}: {}",
                check["status"].as_str().unwrap(),
                check["name"].as_str().unwrap(),
                check["detail"].as_str().unwrap()
            );
            if let Some(path) = check["path"].as_str() {
                line.push_str(&format!(" ({path})"));
            }
            expected.push(line);
            if let Some(correction) = check["correction"].as_str() {
                expected.push(format!("  {:<12}   fix: {correction}", ""));
            }
        }
    }

    assert_eq!(text.lines().collect::<Vec<_>>(), expected);
    // The scenario exercised every kind of status.
    for word in ["available", "uncertain", "unavailable", "ok", "failed"] {
        assert!(text.contains(&format!("  {word} ")), "{word}");
    }
}

/// Every file under `root`, with its kind, permissions and contents.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, String> {
    let mut entries = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).expect("read directory") {
            let path = entry.expect("directory entry").path();
            let metadata = fs::symlink_metadata(&path).expect("metadata");
            let mode = metadata.permissions().mode();
            let description = if metadata.file_type().is_symlink() {
                format!("link {:?}", fs::read_link(&path).unwrap())
            } else if metadata.is_dir() {
                pending.push(path.clone());
                format!("directory {mode:o}")
            } else {
                format!("file {mode:o} {:?}", fs::read(&path).unwrap())
            };
            entries.insert(path.strip_prefix(root).unwrap().to_path_buf(), description);
        }
    }
    entries
}

#[test]
fn doctor_installs_nothing_and_changes_no_files() {
    let environment = Environment::healthy();
    let worktree = environment.root.path().join("feature");
    add_worktree(&environment.repository, &worktree, "feature/one");
    fs::write(
        environment.repository.join("leme.toml"),
        "preferred_agent = \"codex\"\n",
    )
    .unwrap();
    fs::write(
        environment.repository.join("untracked.txt"),
        "work in progress\n",
    )
    .unwrap();
    let home = environment.root.path().join("home");
    fs::create_dir_all(home.join(".config/herdr")).unwrap();
    fs::write(home.join(".config/herdr/config.toml"), "theme = \"dark\"\n").unwrap();
    fs::write(home.join(".gitconfig"), "[user]\n\tname = Someone\n").unwrap();
    // A database created by `leme status` must survive doctor untouched.
    let status = Leme::new(
        &environment.repository,
        environment.root.path(),
        &["status"],
    )
    .run();
    assert!(status.status.success());
    let database = state_database(environment.root.path());
    let modified = fs::metadata(&database).unwrap().modified().unwrap();
    let head = git(&environment.repository, &["rev-parse", "HEAD"]);
    let before = snapshot(environment.root.path());

    for directory in [&environment.repository, &worktree] {
        doctor_json(environment.doctor_in(directory, &["--json"]));
        success_stdout(&environment.doctor_in(directory, &[]).run());
    }

    assert_eq!(snapshot(environment.root.path()), before);
    assert_eq!(
        fs::metadata(&database).unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(git(&environment.repository, &["rev-parse", "HEAD"]), head);
    // Only version, status and schema queries were made: nothing was
    // installed, configured or authenticated.
    let expected: BTreeSet<String> = VERSIONS
        .iter()
        .map(|(name, _)| match *name {
            "git-flow" => "git-flow version".to_owned(),
            name => format!("{name} --version"),
        })
        .chain([
            "herdr status server --json".to_owned(),
            "herdr api schema --json".to_owned(),
        ])
        .collect();
    let invocations = environment.invocations();
    assert_eq!(invocations.len(), expected.len() * 4, "{invocations:#?}");
    assert_eq!(invocations.into_iter().collect::<BTreeSet<_>>(), expected);
}

#[test]
fn doctor_does_not_create_the_state_database() {
    let environment = Environment::healthy();

    let report = doctor_json(environment.doctor(&["--json"]));

    assert_eq!(check(&report, "state")["status"], "ok");
    assert!(
        check(&report, "state")["detail"]
            .as_str()
            .unwrap()
            .contains("not created yet")
    );
    assert!(!environment.root.path().join("state").exists());
    let leftovers: Vec<_> = fs::read_dir(environment.root.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name.to_string_lossy().starts_with(".leme-doctor"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn a_database_from_a_newer_leme_is_reported_and_left_unchanged() {
    let environment = Environment::healthy();
    let database = state_database(environment.root.path());
    fs::create_dir_all(database.parent().unwrap()).unwrap();
    Connection::open(&database)
        .unwrap()
        .pragma_update(None, "user_version", 99)
        .unwrap();
    let bytes = fs::read(&database).unwrap();

    let report = doctor_json(environment.doctor(&["--json"]));

    let state = check(&report, "state");
    assert_eq!(state["status"], "failed");
    assert!(
        state["detail"]
            .as_str()
            .unwrap()
            .contains("schema version 99")
    );
    assert_eq!(state["correction"], "Update Leme.");
    assert_eq!(capability(&report, "status")["status"], "unavailable");
    assert_eq!(capability(&report, "github")["status"], "available");
    assert_eq!(fs::read(&database).unwrap(), bytes);
}

#[test]
fn a_state_directory_that_rejects_new_files_is_reported() {
    let environment = Environment::healthy();
    let locked = environment.root.path().join("locked");
    fs::create_dir(&locked).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
    // Some users, such as root, may write regardless of permissions.
    let writable = fs::write(locked.join("probe"), "").is_ok();

    let report = doctor_json(
        environment
            .doctor(&["--json"])
            .env("XDG_STATE_HOME", locked.join("state").to_str().unwrap()),
    );
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

    if writable {
        eprintln!("skipped: this user can write to a read-only directory");
        return;
    }
    let state = check(&report, "state");
    assert_eq!(state["status"], "failed");
    assert_eq!(state["path"], locked.to_str().unwrap());
    assert!(
        state["detail"]
            .as_str()
            .unwrap()
            .contains("cannot create files in")
    );
    assert_eq!(capability(&report, "status")["status"], "unavailable");
    assert_eq!(capability(&report, "gitflow")["status"], "available");
}

#[test]
fn lookup_skips_relative_entries_and_files_that_cannot_run() {
    let environment = Environment::healthy();
    environment.remove("gh");
    let shadows = environment.root.path().join("shadows");
    fs::create_dir(&shadows).unwrap();
    fs::write(shadows.join("gh"), "#!/bin/sh\necho 'gh version 9.9.9'\n").unwrap();
    fs::create_dir(shadows.join("nvim")).unwrap();
    let relative = environment.repository.join("bin");
    fs::create_dir(&relative).unwrap();
    let relative_gh = Toolbox::new(&relative);
    relative_gh.tool("gh", "gh version 8.8.8");
    let fallback = Toolbox::new(&environment.root.path().join("fallback"));
    fallback.tool("gh", "gh version 2.46.0");
    let search_path = format!(
        "bin:{}:{}:{}",
        shadows.display(),
        environment.tools.search_path(),
        fallback.search_path()
    );

    let report = doctor_json(environment.doctor(&["--json"]).env("PATH", &search_path));

    let gh = check(&report, "gh");
    assert_eq!(gh["path"], fallback.path().join("gh").to_str().unwrap());
    assert_eq!(gh["version"], "2.46.0");
    assert_eq!(
        check(&report, "neovim")["path"],
        environment.tools.path().join("nvim").to_str().unwrap()
    );
}

#[test]
fn outside_a_repository_tools_are_still_checked() {
    let environment = Environment::healthy();
    let elsewhere = environment.root.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();

    let report = doctor_json(environment.doctor_in(&elsewhere, &["--json"]));

    assert_eq!(check(&report, "repository")["status"], "failed");
    assert!(
        check(&report, "repository")["detail"]
            .as_str()
            .unwrap()
            .contains("is not inside a Git repository")
    );
    assert_eq!(check(&report, "preferences")["status"], "skipped");
    for id in ["git", "herdr", "herdr_server", "herdr_api", "gh", "state"] {
        assert_eq!(check(&report, id)["status"], "ok", "{id}");
    }
    for (id, status) in statuses(&report, "capabilities") {
        assert_eq!(status, "unavailable", "{id}");
    }
}

#[test]
fn with_nothing_on_path_every_tool_is_reported_missing() {
    let environment = Environment::healthy();
    let empty = Toolbox::new(&environment.root.path().join("empty"));

    let output = environment
        .doctor(&["--json"])
        .env("PATH", empty.search_path())
        .run();
    let report: Value = serde_json::from_str(&success_stdout(&output)).unwrap();

    for id in [
        "git", "herdr", "neovim", "claude", "codex", "gh", "git_flow", "cargo",
    ] {
        assert_eq!(check(&report, id)["status"], "failed", "{id}");
        assert!(
            check(&report, id)["detail"]
                .as_str()
                .unwrap()
                .ends_with("was not found on PATH"),
            "{id}"
        );
    }
    assert!(
        check(&report, "repository")["detail"]
            .as_str()
            .unwrap()
            .contains("git was not found")
    );
    assert_eq!(check(&report, "herdr_server")["status"], "skipped");
    for (id, status) in statuses(&report, "capabilities") {
        assert_eq!(status, "unavailable", "{id}");
    }
}

#[test]
fn an_invalid_timeout_is_rejected_before_any_tool_runs() {
    let environment = Environment::healthy();

    for timeout in ["0", "-2", "soon", "601"] {
        let output = environment.doctor(&[&format!("--timeout={timeout}")]).run();

        assert_eq!(output.status.code(), Some(2), "{timeout}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("--timeout <SECONDS>"),
            "{timeout}"
        );
    }
    assert!(environment.invocations().is_empty());
}
