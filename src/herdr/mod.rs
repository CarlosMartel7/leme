//! A read-only adapter for the installed Herdr.
//!
//! [`Herdr`] runs the `herdr` CLI and reads its JSON replies. [`context`]
//! resolves the workspace, tab and pane a Leme process was invoked from,
//! using the IDs Herdr supplied rather than whatever is focused when the
//! request runs, so a user switching workspaces cannot redirect it.
//!
//! Before talking to Herdr, callers check [`Herdr::readiness`], which runs
//! the same Herdr checks as `leme doctor`.

mod cli;
mod context;

use std::fmt;
use std::time::Duration;

use crate::diagnostics::Check;

pub use cli::{Herdr, Readiness, RequestFailure};
pub use context::{Context, Invocation, PaneInfo, TabInfo, WorkspaceInfo, resolve};

/// The Herdr API operations this adapter sends. Each is one of the
/// operations `leme doctor` confirms Herdr provides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// `pane.current`: one pane with its tab and workspace.
    PaneCurrent,
    /// `tab.list`: the tabs of one workspace.
    TabList,
    /// `workspace.list`: every workspace.
    WorkspaceList,
}

impl Operation {
    /// The API method name, as listed in Herdr's schema.
    pub fn method(self) -> &'static str {
        match self {
            Self::PaneCurrent => "pane.current",
            Self::TabList => "tab.list",
            Self::WorkspaceList => "workspace.list",
        }
    }
}

impl fmt::Display for Operation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.method())
    }
}

/// Why the Herdr context could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HerdrError {
    /// No `herdr` executable was found.
    NotInstalled,
    /// A `leme doctor` Herdr check failed, so no request was sent.
    NotReady(Box<Check>),
    /// Neither the arguments nor the environment named a workspace, tab or
    /// pane: Leme is not running inside Herdr.
    NoInvocation,
    /// A tab was named without the workspace that contains it.
    TabWithoutWorkspace,
    /// A request to Herdr failed.
    Request {
        operation: Operation,
        cause: RequestFailure,
    },
    /// Herdr does not know an ID it was asked about.
    NotFound { kind: &'static str, id: String },
    /// Herdr places the pane or tab somewhere other than the supplied IDs say.
    Mismatch {
        kind: &'static str,
        supplied: String,
        reported: String,
    },
}

impl fmt::Display for HerdrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInstalled => write!(
                f,
                "cannot find the herdr executable; install Herdr (https://herdr.dev) and add \
                 `herdr` to PATH"
            ),
            Self::NotReady(check) => {
                write!(f, "Herdr is not ready: {}: {}", check.name, check.detail)?;
                if let Some(correction) = &check.correction {
                    write!(f, ". {correction}")?;
                }
                Ok(())
            }
            Self::NoInvocation => write!(
                f,
                "no Herdr context: run Leme from a Herdr pane, which sets HERDR_PANE_ID, or pass \
                 --pane, --tab or --workspace"
            ),
            Self::TabWithoutWorkspace => {
                write!(f, "a tab can only be resolved together with its workspace")
            }
            Self::Request { operation, cause } => {
                write!(f, "Herdr operation {operation} failed: ")?;
                match cause {
                    RequestFailure::NotStarted { executable, reason } => {
                        write!(f, "cannot run {}: {reason}", executable.display())
                    }
                    RequestFailure::TimedOut(limit) => write!(
                        f,
                        "no answer within {} seconds; check that the server responds to \
                         `herdr status`",
                        limit.as_secs_f64()
                    ),
                    RequestFailure::Rejected {
                        code: Some(code),
                        message,
                    } => write!(f, "{message} ({code})"),
                    RequestFailure::Rejected {
                        code: None,
                        message,
                    } => f.write_str(message),
                    RequestFailure::InvalidJson { excerpt } => {
                        write!(f, "the reply is not JSON: {excerpt:?}")
                    }
                    RequestFailure::MissingField { field } => {
                        write!(f, "the reply has no `{field}` field")
                    }
                }
            }
            Self::NotFound { kind, id } => write!(f, "Herdr has no {kind} {id}"),
            Self::Mismatch {
                kind,
                supplied,
                reported,
            } => write!(
                f,
                "the supplied {kind} {supplied} does not match the {kind} Herdr reports, \
                 {reported}"
            ),
        }
    }
}

impl std::error::Error for HerdrError {}

/// How long one Herdr request may run by default.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
