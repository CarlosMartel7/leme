//! Runs the installed Herdr CLI and reads its JSON responses.
//!
//! Every request is a separate `herdr` process started with one argument
//! per value, never through a shell, so IDs and paths with spaces or quotes
//! reach Herdr unchanged. Each process is stopped once the timeout passes.

use std::env;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use super::{HerdrError, Operation};
use crate::diagnostics::{self, Check, CheckStatus, Run};

/// Longest excerpt of Herdr's output quoted in an error.
const EXCERPT_LIMIT: usize = 200;

/// The installed Herdr CLI, ready to send requests to the running server.
#[derive(Debug, Clone)]
pub struct Herdr {
    executable: PathBuf,
    timeout: Duration,
}

/// Whether Herdr can be used, as judged by `leme doctor`'s Herdr checks.
#[derive(Debug, Clone)]
pub struct Readiness {
    /// The executable, server and API checks, in that order.
    pub checks: Vec<Check>,
}

impl Readiness {
    /// The first check that failed or could not run, if any.
    pub fn blocker(&self) -> Option<&Check> {
        self.checks
            .iter()
            .find(|check| matches!(check.status, CheckStatus::Failed | CheckStatus::Skipped))
    }

    /// Checks whose compatibility could not be confirmed.
    pub fn uncertain(&self) -> impl Iterator<Item = &Check> {
        self.checks
            .iter()
            .filter(|check| check.status == CheckStatus::Uncertain)
    }
}

impl Herdr {
    /// Uses the Herdr at `executable`; each request may run for `timeout`.
    pub fn new(executable: PathBuf, timeout: Duration) -> Self {
        Self {
            executable,
            timeout,
        }
    }

    /// Finds the Herdr this process should use.
    ///
    /// Herdr gives the processes it starts `HERDR_BIN_PATH`, the binary of the
    /// running session, so that path is preferred when it names an absolute
    /// executable. Otherwise `herdr` is looked up on `PATH`.
    pub fn locate(timeout: Duration) -> Option<Self> {
        Self::locate_in(
            env::var_os("HERDR_BIN_PATH").as_deref(),
            env::var_os("PATH").as_deref(),
            timeout,
        )
    }

    /// [`Herdr::locate`] with explicit `HERDR_BIN_PATH` and `PATH` values.
    pub fn locate_in(
        bin_path: Option<&OsStr>,
        search_path: Option<&OsStr>,
        timeout: Duration,
    ) -> Option<Self> {
        let session_binary = bin_path
            .map(PathBuf::from)
            .filter(|path| path.is_absolute() && is_executable_file(path));
        session_binary
            .or_else(|| diagnostics::find_executable("herdr", search_path))
            .map(|executable| Self::new(executable, timeout))
    }

    /// The executable requests are sent through.
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    /// Runs `leme doctor`'s Herdr checks against this executable.
    pub fn readiness(&self) -> Readiness {
        let probes = diagnostics::probe_herdr(Some(self.executable.clone()), self.timeout);
        Readiness {
            checks: diagnostics::classify_herdr(&probes).into(),
        }
    }

    /// Sends one request and returns the `result` object of Herdr's reply.
    ///
    /// `arguments` follow the executable unchanged, one process argument each.
    pub fn request<I, S>(&self, operation: Operation, arguments: I) -> Result<Value, HerdrError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let stdout = self.run(operation, arguments)?;
        parse_reply(&stdout).map_err(|cause| HerdrError::Request { operation, cause })
    }

    /// Sends one request whose success Herdr reports only through its exit
    /// status, such as `pane run`, which prints nothing.
    pub fn send<I, S>(&self, operation: Operation, arguments: I) -> Result<(), HerdrError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.run(operation, arguments).map(drop)
    }

    /// Runs one request and returns the standard output of a successful one.
    fn run<I, S>(&self, operation: Operation, arguments: I) -> Result<String, HerdrError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let arguments: Vec<OsString> = arguments
            .into_iter()
            .map(|argument| argument.as_ref().to_owned())
            .collect();
        let mut command = Command::new(&self.executable);
        command.args(&arguments);
        let failure = |cause| HerdrError::Request { operation, cause };
        match diagnostics::run(command, self.timeout) {
            Run::NotStarted(reason) => Err(failure(RequestFailure::NotStarted {
                executable: self.executable.clone(),
                reason,
            })),
            Run::TimedOut(limit) => Err(failure(RequestFailure::TimedOut(limit))),
            Run::Finished {
                success: false,
                stdout,
                stderr,
            } => Err(failure(rejection(&stderr, &stdout))),
            Run::Finished {
                success: true,
                stdout,
                ..
            } => Ok(stdout),
        }
    }
}

/// Why a single Herdr request failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestFailure {
    /// The Herdr executable could not be started.
    NotStarted { executable: PathBuf, reason: String },
    /// Herdr did not answer within this time and was stopped.
    TimedOut(Duration),
    /// Herdr answered with an error. `code` is Herdr's error code, such as
    /// `pane_not_found` or `server_unavailable`, when it reported one.
    Rejected {
        code: Option<String>,
        message: String,
    },
    /// Herdr succeeded but printed something that is not a JSON reply.
    InvalidJson { excerpt: String },
    /// The reply was JSON but lacked a field Leme needs.
    MissingField { field: &'static str },
}

/// Reads Herdr's error reply, `{"error": {"code", "message"}}` on standard
/// error, or the first line of output when it is not in that form.
fn rejection(stderr: &str, stdout: &str) -> RequestFailure {
    let reply = [stderr, stdout]
        .into_iter()
        .find_map(|text| serde_json::from_str::<Value>(text.trim()).ok());
    if let Some(error) = reply.as_ref().map(|reply| &reply["error"])
        && let Some(message) = error["message"].as_str()
    {
        return RequestFailure::Rejected {
            code: error["code"].as_str().map(str::to_owned),
            message: excerpt(message),
        };
    }
    let message = [stderr, stdout]
        .into_iter()
        .flat_map(str::lines)
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map_or_else(|| "no error message".to_owned(), excerpt);
    RequestFailure::Rejected {
        code: None,
        message,
    }
}

fn parse_reply(stdout: &str) -> Result<Value, RequestFailure> {
    let mut reply: Value =
        serde_json::from_str(stdout.trim()).map_err(|_| RequestFailure::InvalidJson {
            excerpt: excerpt(stdout),
        })?;
    match reply.get_mut("result") {
        Some(result) if result.is_object() => Ok(result.take()),
        _ => Err(RequestFailure::MissingField { field: "result" }),
    }
}

fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

/// The first non-blank line of `text`, shortened for display.
fn excerpt(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    match line.char_indices().nth(EXCERPT_LIMIT) {
        Some((end, _)) => format!("{}…", &line[..end]),
        None => line.to_owned(),
    }
}
