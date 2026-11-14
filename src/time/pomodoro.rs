//! A focus or break countdown that can be paused, resumed and recovered after
//! the dashboard closes.
//!
//! While the process runs, a [`Timer`] measures its interval with the steady
//! clock of a [`Moment`], so moving the system time does not change the
//! countdown. Its [`Saved`] form records the wall-clock time at which the
//! current running stretch started; [`Timer::restore`] uses it to account for
//! the time that passed while no dashboard was open.
//!
//! Every function takes the current [`Moment`] as an argument, so the timer
//! never reads a clock itself and can be driven with any supplied values.

use std::time::{Duration, SystemTime};

use super::{Moment, from_unix_millis, unix_millis, whole_millis};

/// How far the wall clock may drift from the steady clock during a running
/// stretch before the saved start time is corrected. Small differences come
/// from reading the two clocks a moment apart.
pub const DRIFT_TOLERANCE: Duration = Duration::from_secs(2);

/// The kind of interval being timed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// A 25-minute stretch of focused work.
    Focus,
    /// A 5-minute break.
    Break,
}

impl Mode {
    /// The length of a new interval of this kind.
    pub fn duration(self) -> Duration {
        match self {
            Self::Focus => Duration::from_secs(25 * 60),
            Self::Break => Duration::from_secs(5 * 60),
        }
    }

    /// The name shown to the user: `Focus` or `Break`.
    pub fn label(self) -> &'static str {
        match self {
            Self::Focus => "Focus",
            Self::Break => "Break",
        }
    }
}

/// Where a timer is in its interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Counting down.
    Running,
    /// Stopped with time left, until resumed.
    Paused,
    /// The interval ran out. It stays visible until replaced or dismissed.
    Elapsed,
}

/// A focus or break interval in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timer {
    mode: Mode,
    duration: Duration,
    state: State,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Counting down from `remaining`, as of `since`.
    Running {
        remaining: Duration,
        since: Moment,
    },
    Paused {
        remaining: Duration,
    },
    Elapsed {
        finished_at: SystemTime,
    },
}

impl Timer {
    /// A new interval of `mode`'s full length, running from `now`.
    pub fn start(mode: Mode, now: Moment) -> Self {
        let duration = mode.duration();
        Self {
            mode,
            duration,
            state: State::Running {
                remaining: duration,
                since: now,
            },
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The full length of the interval.
    pub fn duration(&self) -> Duration {
        self.duration
    }

    /// The phase as last updated. A running timer whose time has run out
    /// still reports [`Phase::Running`] until [`Timer::update`] sees it.
    pub fn phase(&self) -> Phase {
        match self.state {
            State::Running { .. } => Phase::Running,
            State::Paused { .. } => Phase::Paused,
            State::Elapsed { .. } => Phase::Elapsed,
        }
    }

    /// The time left at `now`, never less than zero.
    pub fn remaining(&self, now: Moment) -> Duration {
        match self.state {
            State::Running { remaining, since } => remaining.saturating_sub(ran(since, now)),
            State::Paused { remaining } => remaining,
            State::Elapsed { .. } => Duration::ZERO,
        }
    }

    /// When the interval ran out, once it has.
    pub fn finished_at(&self) -> Option<SystemTime> {
        match self.state {
            State::Elapsed { finished_at } => Some(finished_at),
            State::Running { .. } | State::Paused { .. } => None,
        }
    }

    /// Brings a running timer up to `now` and returns whether its saved form
    /// changed.
    ///
    /// A timer whose time has run out becomes [`Phase::Elapsed`]. When the
    /// wall clock has moved away from the steady clock, as after a manual
    /// clock change, the running stretch restarts at `now` with the time
    /// still left, so the saved start time matches the countdown again.
    pub fn update(&mut self, now: Moment) -> bool {
        let State::Running { remaining, since } = self.state else {
            return false;
        };
        let ran = ran(since, now);
        if ran >= remaining {
            // The steady clock says how long ago the time ran out; going back
            // that far on the wall clock gives the finish time even if the
            // wall clock was moved meanwhile.
            let overshoot = ran - remaining;
            let finished_at = now.wall.checked_sub(overshoot).unwrap_or(now.wall);
            self.state = State::Elapsed { finished_at };
            return true;
        }
        if drift(since, now) > DRIFT_TOLERANCE {
            self.state = State::Running {
                remaining: remaining - ran,
                since: now,
            };
            return true;
        }
        false
    }

    /// Pauses a running timer and returns whether anything changed. A timer
    /// whose time has already run out becomes elapsed instead.
    pub fn pause(&mut self, now: Moment) -> bool {
        if !matches!(self.state, State::Running { .. }) {
            return false;
        }
        self.update(now);
        if self.phase() == Phase::Running {
            self.state = State::Paused {
                remaining: self.remaining(now),
            };
        }
        true
    }

    /// Resumes a paused timer from `now` and returns whether it was paused.
    pub fn resume(&mut self, now: Moment) -> bool {
        let State::Paused { remaining } = self.state else {
            return false;
        };
        self.state = State::Running {
            remaining,
            since: now,
        };
        true
    }

    /// What must be stored to recover this timer in another process.
    ///
    /// Durations and times are rounded down to whole milliseconds, the
    /// precision of the store, so a saved timer reads back unchanged.
    pub fn saved(&self) -> Saved {
        let phase = match self.state {
            State::Running { remaining, since } => SavedPhase::Running {
                remaining: whole_millis(remaining),
                since: to_millis(since.wall),
            },
            State::Paused { remaining } => SavedPhase::Paused {
                remaining: whole_millis(remaining),
            },
            State::Elapsed { finished_at } => SavedPhase::Elapsed {
                finished_at: to_millis(finished_at),
            },
        };
        Saved {
            mode: self.mode,
            duration: whole_millis(self.duration),
            phase,
        }
    }

    /// Recovers a timer saved by an earlier or concurrent process.
    ///
    /// A paused timer stays paused with the same time left, and an elapsed
    /// one stays elapsed. A running timer loses the wall-clock time since its
    /// saved start; if that covers its remaining time, it becomes elapsed at
    /// the moment it would have finished. A wall clock now earlier than the
    /// saved start counts as no time passed, so a clock set back never ends
    /// an interval early.
    pub fn restore(saved: &Saved, now: Moment) -> Self {
        let state = match saved.phase {
            SavedPhase::Paused { remaining } => State::Paused {
                remaining: remaining.min(saved.duration),
            },
            SavedPhase::Elapsed { finished_at } => State::Elapsed { finished_at },
            SavedPhase::Running { remaining, since } => {
                let remaining = remaining.min(saved.duration);
                let passed = now.wall.duration_since(since).unwrap_or(Duration::ZERO);
                if passed >= remaining {
                    State::Elapsed {
                        finished_at: since + remaining,
                    }
                } else {
                    State::Running {
                        remaining: remaining - passed,
                        since: now,
                    }
                }
            }
        };
        Self {
            mode: saved.mode,
            duration: saved.duration,
            state,
        }
    }
}

/// Steady-clock time from `since` to `now`; zero if `now` is earlier, which
/// only happens across a reboot.
fn ran(since: Moment, now: Moment) -> Duration {
    now.steady.saturating_sub(since.steady)
}

/// How much more or less the wall clock moved than the steady clock.
fn drift(since: Moment, now: Moment) -> Duration {
    let steady = ran(since, now);
    match now.wall.duration_since(since.wall) {
        Ok(wall) => wall.abs_diff(steady),
        Err(backwards) => backwards.duration() + steady,
    }
}

fn to_millis(time: SystemTime) -> SystemTime {
    from_unix_millis(unix_millis(time))
}

/// A timer as stored between processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Saved {
    pub mode: Mode,
    /// The full length of the interval.
    pub duration: Duration,
    pub phase: SavedPhase,
}

impl Saved {
    /// Whether the timer is still counting or waiting to resume, as opposed
    /// to finished.
    pub fn is_active(&self) -> bool {
        !matches!(self.phase, SavedPhase::Elapsed { .. })
    }
}

/// The phase of a stored timer, with the timing it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavedPhase {
    /// `remaining` was left at wall-clock time `since`.
    Running {
        remaining: Duration,
        since: SystemTime,
    },
    /// Paused with `remaining` left.
    Paused { remaining: Duration },
    /// The interval ran out at `finished_at`.
    Elapsed { finished_at: SystemTime },
}
