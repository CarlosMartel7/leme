//! Read-only probes of the installed tools, the project and the state
//! directory.
//!
//! Every probe either reads (version output, Herdr's status and schema, the
//! repository, `leme.toml`, the database's schema version) or creates and
//! immediately removes one empty file to learn whether the state directory
//! accepts new files. Nothing is installed, configured or migrated.

use std::env;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::{Probes, Run, StateProbe, ToolProbe};
use crate::config::Preferences;
use crate::git;
use crate::state;

/// How often a running probe is checked for completion.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Most output read from one stream of a probe. Herdr's schema is the
/// largest expected output, at well under a megabyte.
const OUTPUT_LIMIT: u64 = 16 * 1024 * 1024;

/// Probes the environment of a `leme` started in `directory`, letting each
/// probe command run for at most `timeout`. Executables are looked up in
/// the `PATH` of this process.
///
/// `directory` is an error when the working directory cannot be read; the
/// project checks then fail, and the other probes still run.
pub fn collect(directory: Result<&Path, &io::Error>, timeout: Duration) -> Probes {
    let search_path = env::var_os("PATH");
    let search_path = search_path.as_deref();
    let tool = |name: &str, arguments: &[&str]| {
        probe_tool(name, search_path, timeout, |command| {
            command.args(arguments);
        })
    };

    thread::scope(|scope| {
        let git = scope.spawn(|| tool("git", &["--version"]));
        let herdr = scope.spawn(|| {
            let herdr = tool("herdr", &["--version"]);
            let (server, schema) = match &herdr {
                ToolProbe::Found {
                    path,
                    run: Run::Finished { success: true, .. },
                } => (
                    Some(run(
                        herdr_command(path, &["status", "server", "--json"]),
                        timeout,
                    )),
                    Some(run(
                        herdr_command(path, &["api", "schema", "--json"]),
                        timeout,
                    )),
                ),
                _ => (None, None),
            };
            (herdr, server, schema)
        });
        let neovim = scope.spawn(|| {
            probe_tool("nvim", search_path, timeout, |command| {
                // Otherwise even `nvim --version` creates a log file in
                // Neovim's state directory.
                command.arg("--version").env("NVIM_LOG_FILE", "/dev/null");
            })
        });
        let claude = scope.spawn(|| tool("claude", &["--version"]));
        let codex = scope.spawn(|| tool("codex", &["--version"]));
        let gh = scope.spawn(|| tool("gh", &["--version"]));
        let git_flow = scope.spawn(|| tool("git-flow", &["version"]));
        let cargo = scope.spawn(|| tool("cargo", &["--version"]));
        let rustc = scope.spawn(|| tool("rustc", &["--version"]));
        let cc = scope.spawn(|| tool("cc", &["--version"]));

        let repository = match directory {
            Ok(directory) => git::discover(directory).map_err(|error| error.to_string()),
            Err(error) => Err(format!("cannot read the current directory: {error}")),
        };
        let preferences = repository.as_ref().ok().map(|checkout| {
            Preferences::load(&checkout.repository).map_err(|error| error.to_string())
        });
        let state = state_probe();

        let (herdr, herdr_server, herdr_schema) = join(herdr);
        Probes {
            git: join(git),
            repository,
            preferences,
            state,
            herdr,
            herdr_server,
            herdr_schema,
            neovim: join(neovim),
            claude: join(claude),
            codex: join(codex),
            gh: join(gh),
            git_flow: join(git_flow),
            cargo: join(cargo),
            rustc: join(rustc),
            cc: join(cc),
        }
    })
}

fn join<T>(handle: thread::ScopedJoinHandle<'_, T>) -> T {
    handle
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

fn herdr_command(herdr: &Path, arguments: &[&str]) -> Command {
    let mut command = Command::new(herdr);
    command.args(arguments);
    command
}

/// Looks up `name` and, when found, runs it as `prepare` sets it up.
fn probe_tool(
    name: &str,
    search_path: Option<&OsStr>,
    timeout: Duration,
    prepare: impl FnOnce(&mut Command),
) -> ToolProbe {
    match find_executable(name, search_path) {
        None => ToolProbe::Missing,
        Some(path) => {
            let mut command = Command::new(&path);
            prepare(&mut command);
            let run = run(command, timeout);
            ToolProbe::Found { path, run }
        }
    }
}

/// Finds the first executable file called `name` in the absolute
/// directories of `search_path`, as a shell would. Relative entries are
/// skipped, because they would depend on the working directory.
pub fn find_executable(name: &str, search_path: Option<&OsStr>) -> Option<PathBuf> {
    env::split_paths(search_path?)
        .filter(|directory| directory.is_absolute())
        .map(|directory| directory.join(name))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

/// Runs `command` with no input, stopping it once `timeout` has passed.
/// Output is decoded leniently, since only excerpts are shown.
pub fn run(mut command: Command, timeout: Duration) -> Run {
    let deadline = Instant::now() + timeout;
    let spawned = command
        // A fixed locale keeps version output and messages recognizable.
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => return Run::NotStarted(error.to_string()),
    };
    // Reading both streams while waiting keeps a talkative process from
    // blocking on a full pipe.
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(POLL_INTERVAL),
            Ok(None) => {
                stop(&mut child);
                return Run::TimedOut(timeout);
            }
            Err(error) => {
                stop(&mut child);
                return Run::NotStarted(error.to_string());
            }
        }
    };

    // A process it started in the background can keep the streams open after
    // it exits, so collecting them is bounded by the same deadline.
    let collect = |receiver: Receiver<Vec<u8>>| {
        receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    };
    match (collect(stdout), collect(stderr)) {
        (Ok(stdout), Ok(stderr)) => Run::Finished {
            success: status.success(),
            stdout,
            stderr,
        },
        _ => Run::TimedOut(timeout),
    }
}

fn stop(child: &mut process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Reads a stream to its end, or to `OUTPUT_LIMIT`, on its own thread.
fn drain<R: Read + Send + 'static>(stream: Option<R>) -> Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(stream) = stream {
            let _ = stream.take(OUTPUT_LIMIT).read_to_end(&mut bytes);
        }
        let _ = sender.send(bytes);
    });
    receiver
}

/// Inspects the default state database location without creating the
/// database or migrating it.
fn state_probe() -> StateProbe {
    let Some(path) = state::default_path() else {
        return StateProbe::NoLocation;
    };
    let directory = path.parent().unwrap_or(&path);
    if let Err((directory, reason)) = accepts_files(directory) {
        return StateProbe::Unwritable {
            path: directory,
            reason,
        };
    }
    match state::existing_schema_version(&path) {
        Ok(schema) => StateProbe::Usable { path, schema },
        Err(error) => StateProbe::Unreadable {
            path,
            reason: error.to_string(),
        },
    }
}

/// Learns whether files can be created in `directory`, or, when it does not
/// exist yet, in the nearest ancestor `leme status` would create it under.
/// Creating a file is the only reliable test: permission bits miss access
/// control lists and read-only mounts.
fn accepts_files(directory: &Path) -> Result<(), (PathBuf, String)> {
    let existing = directory
        .ancestors()
        .find(|ancestor| ancestor.exists())
        .unwrap_or(directory);
    let failure = |reason: String| (existing.to_path_buf(), reason);
    if !existing.is_dir() {
        return Err(failure(format!(
            "{} is not a directory",
            existing.display()
        )));
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos());
    let probe = existing.join(format!(".leme-doctor-{}-{nanos}", process::id()));
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|error| {
            failure(format!(
                "cannot create files in {}: {error}",
                existing.display()
            ))
        })?;
    fs::remove_file(&probe).map_err(|error| {
        failure(format!(
            "cannot remove the probe file {}: {error}",
            probe.display()
        ))
    })
}
