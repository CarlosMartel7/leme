//! Time sources used by the dashboard.
//!
//! [`local`] converts the system clock to the local time of day shown in the
//! dashboard header. [`pomodoro`] counts focus and break intervals down from
//! a [`Moment`], which reads two clocks at once:
//!
//! - a steady clock that only moves forward, used to measure an interval
//!   while the process runs, so changing the system time cannot make the
//!   countdown jump;
//! - the wall clock, saved with the timer, so a later process can work out
//!   how much of a running interval passed while no dashboard was open.

pub mod local;
pub mod pomodoro;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// One instant read from the steady clock and the wall clock together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Moment {
    /// Time since the machine booted, including time spent suspended. Only
    /// differences between two readings in the same boot are meaningful.
    pub steady: Duration,
    /// The system's wall-clock time, which an administrator or time
    /// synchronization may move in either direction.
    pub wall: SystemTime,
}

impl Moment {
    /// Reads both clocks.
    pub fn now() -> Self {
        Self {
            steady: steady_now(),
            wall: SystemTime::now(),
        }
    }
}

/// Linux's `CLOCK_BOOTTIME`: like the monotonic clock behind
/// [`std::time::Instant`], it is never set by clock changes, but it also
/// keeps counting while the machine is suspended, so a focus interval keeps
/// running through a laptop sleep.
fn steady_now() -> Duration {
    let read = |clock| {
        let mut time = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: `clock_gettime` only writes the `timespec` it is given.
        let status = unsafe { libc::clock_gettime(clock, &mut time) };
        let seconds = u64::try_from(time.tv_sec).ok()?;
        let nanos = u32::try_from(time.tv_nsec).ok()?;
        (status == 0).then(|| Duration::new(seconds, nanos))
    };
    // Every supported kernel has CLOCK_BOOTTIME; the monotonic clock is only
    // a fallback, and the same clock is chosen on every call.
    read(libc::CLOCK_BOOTTIME)
        .or_else(|| read(libc::CLOCK_MONOTONIC))
        .expect("Linux provides a monotonic clock")
}

/// Milliseconds since the Unix epoch, rounded toward the past. Negative
/// before the epoch; saturates far outside any real clock value.
pub fn unix_millis(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_millis()).unwrap_or(i64::MAX),
        Err(before) => {
            let before = before.duration();
            let whole = i64::try_from(before.as_millis()).unwrap_or(i64::MAX);
            let partial = i64::from(before.subsec_nanos() % 1_000_000 > 0);
            whole.saturating_add(partial).saturating_neg()
        }
    }
}

/// The wall-clock time `millis` milliseconds after the Unix epoch.
pub fn from_unix_millis(millis: i64) -> SystemTime {
    let offset = Duration::from_millis(millis.unsigned_abs());
    if millis >= 0 {
        UNIX_EPOCH + offset
    } else {
        UNIX_EPOCH - offset
    }
}

/// `duration` rounded down to whole milliseconds.
pub fn whole_millis(duration: Duration) -> Duration {
    Duration::from_secs(duration.as_secs())
        + Duration::from_millis(u64::from(duration.subsec_millis()))
}
