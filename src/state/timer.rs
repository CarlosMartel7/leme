//! The project's saved focus/break timer.
//!
//! Each project has at most one timer, kept here so it survives closing the
//! dashboard and is shared by every checkout of the project. Only the
//! dashboard holding the project's timer lock writes it (see
//! [`super::TimerOwnership`]). How the saved values are interpreted lives in
//! [`crate::time::pomodoro`].

use std::fmt;
use std::time::Duration;

use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::{Failure, StateError, Store};
use crate::time::pomodoro::{Mode, Saved, SavedPhase};
use crate::time::{from_unix_millis, unix_millis};

impl Store {
    /// The project's saved timer, if it has one.
    pub fn timer(&mut self, project_id: &str) -> Result<Option<Saved>, StateError> {
        read(&self.connection, project_id).map_err(|error| self.failure(error))
    }

    /// Replaces the project's timer with `timer`.
    pub fn save_timer(&mut self, project_id: &str, timer: &Saved) -> Result<(), StateError> {
        write(&self.connection, project_id, timer).map_err(|error| self.failure(error))
    }

    /// Removes the project's timer, if any.
    pub fn clear_timer(&mut self, project_id: &str) -> Result<(), StateError> {
        self.connection
            .execute(
                "DELETE FROM timers WHERE project_id = ?1",
                params![project_id],
            )
            .map(drop)
            .map_err(|error| self.failure(error.into()))
    }
}

fn read(connection: &Connection, project_id: &str) -> Result<Option<Saved>, Failure> {
    Ok(connection
        .query_row(
            "SELECT mode, duration_ms, phase, remaining_ms, running_since_ms, finished_at_ms
             FROM timers WHERE project_id = ?1",
            params![project_id],
            decode,
        )
        .optional()?)
}

fn write(connection: &Connection, project_id: &str, timer: &Saved) -> Result<(), Failure> {
    let mode = match timer.mode {
        Mode::Focus => "focus",
        Mode::Break => "break",
    };
    let (phase, remaining, since, finished_at) = match timer.phase {
        SavedPhase::Running { remaining, since } => (
            "running",
            Some(millis(remaining)),
            Some(unix_millis(since)),
            None,
        ),
        SavedPhase::Paused { remaining } => ("paused", Some(millis(remaining)), None, None),
        SavedPhase::Elapsed { finished_at } => {
            ("elapsed", None, None, Some(unix_millis(finished_at)))
        }
    };
    connection.execute(
        "INSERT OR REPLACE INTO timers
             (project_id, mode, duration_ms, phase, remaining_ms, running_since_ms,
              finished_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            project_id,
            mode,
            millis(timer.duration),
            phase,
            remaining,
            since,
            finished_at
        ],
    )?;
    Ok(())
}

fn decode(row: &Row) -> rusqlite::Result<Saved> {
    let mode = match row.get_ref(0)?.as_str()? {
        "focus" => Mode::Focus,
        "break" => Mode::Break,
        other => return Err(invalid(0, format!("unknown timer mode {other:?}"))),
    };
    let duration = duration_column(row, 1)?;
    let phase = match row.get_ref(2)?.as_str()? {
        "running" => SavedPhase::Running {
            remaining: duration_column(row, 3)?,
            since: from_unix_millis(row.get(4)?),
        },
        "paused" => SavedPhase::Paused {
            remaining: duration_column(row, 3)?,
        },
        "elapsed" => SavedPhase::Elapsed {
            finished_at: from_unix_millis(row.get(5)?),
        },
        other => return Err(invalid(2, format!("unknown timer phase {other:?}"))),
    };
    Ok(Saved {
        mode,
        duration,
        phase,
    })
}

fn duration_column(row: &Row, column: usize) -> rusqlite::Result<Duration> {
    let value: i64 = row.get(column)?;
    u64::try_from(value)
        .map(Duration::from_millis)
        .map_err(|_| invalid(column, format!("negative duration {value} ms")))
}

/// Whole milliseconds of `duration`, saturating far beyond any interval.
fn millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

fn invalid(column: usize, message: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, Type::Text, Box::new(Invalid(message)))
}

/// A stored value outside what the schema allows.
#[derive(Debug)]
struct Invalid(String);

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Invalid {}
