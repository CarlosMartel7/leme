//! The local time of day, as the system's C library reports it.
//!
//! This is the same conversion `date` performs on Linux and WSL: the C
//! library applies `TZ` when it is set and `/etc/localtime` otherwise. The
//! time zone is re-read on every call, so a zone change made while the
//! dashboard is open shows up at its next reading.
//!
//! Leme never changes its own environment, which keeps the C library's
//! time-zone state safe to read from any thread.

use std::ffi::CStr;
use std::fmt;
use std::mem::MaybeUninit;
use std::time::{SystemTime, UNIX_EPOCH};

unsafe extern "C" {
    /// POSIX `tzset(3)`: reloads the time-zone rules from `TZ` or
    /// `/etc/localtime`. The `libc` crate does not declare it for Unix.
    fn tzset();
}

/// A wall-clock time of day in the local time zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalTime {
    /// Hour of the day, 0 to 23.
    pub hour: u8,
    /// Minute of the hour, 0 to 59.
    pub minute: u8,
    /// Second of the minute, 0 to 60 (60 only during a leap second).
    pub second: u8,
    /// Abbreviation of the time zone in effect, such as `UTC` or `-03`, when
    /// the C library reports one.
    pub zone: Option<String>,
}

/// The local time could not be determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unavailable;

impl fmt::Display for Unavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the local time is unavailable")
    }
}

impl std::error::Error for Unavailable {}

/// The outcome of reading the local time.
pub type Reading = Result<LocalTime, Unavailable>;

/// The current local time.
pub fn now() -> Reading {
    at(SystemTime::now())
}

/// The local time of day at `time`.
///
/// Fails when `time` cannot be represented by the C library, which for the
/// current time means the system clock is set far outside any usable range.
pub fn at(time: SystemTime) -> Reading {
    let seconds = unix_seconds(time).ok_or(Unavailable)?;
    let mut parts = MaybeUninit::<libc::tm>::zeroed();
    // SAFETY: `tzset` and `localtime_r` only read the environment, which Leme
    // never modifies. `localtime_r` writes into `parts`, which outlives the
    // call, and returns null without writing a usable value on failure.
    let parts = unsafe {
        tzset();
        if libc::localtime_r(&seconds, parts.as_mut_ptr()).is_null() {
            return Err(Unavailable);
        }
        parts.assume_init()
    };
    let field = |value: libc::c_int| u8::try_from(value).map_err(|_| Unavailable);
    Ok(LocalTime {
        hour: field(parts.tm_hour)?,
        minute: field(parts.tm_min)?,
        second: field(parts.tm_sec)?,
        zone: zone_name(parts.tm_zone),
    })
}

/// Whole seconds since the Unix epoch, rounded down, as the C library's
/// `time_t`.
fn unix_seconds(time: SystemTime) -> Option<libc::time_t> {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => libc::time_t::try_from(after.as_secs()).ok(),
        Err(before) => {
            let before = before.duration();
            let whole = libc::time_t::try_from(before.as_secs()).ok()?;
            // Round toward the past, so 0.5 s before the epoch is second -1.
            let partial = libc::time_t::from(before.subsec_nanos() > 0);
            whole.checked_add(partial).map(|seconds| -seconds)
        }
    }
}

fn zone_name(zone: *const libc::c_char) -> Option<String> {
    if zone.is_null() {
        return None;
    }
    // SAFETY: a non-null `tm_zone` from `localtime_r` points to a
    // NUL-terminated abbreviation owned by the C library (glibc and musl keep
    // these for the life of the process). It is copied before returning.
    let name = unsafe { CStr::from_ptr(zone) }.to_string_lossy();
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}
