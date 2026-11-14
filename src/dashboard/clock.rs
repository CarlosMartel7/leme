//! The local time shown in the dashboard header.
//!
//! The dashboard loop reads the time itself on a short interval, separately
//! from project collection, so the clock keeps moving while Git or the state
//! database is slow or failing. This module only turns a reading into text.

use crate::config::ClockFormat;
use crate::time::local::{LocalTime, Reading};

/// What the header shows when the local time cannot be read.
pub const UNAVAILABLE: &str = "--:--";

/// `reading` written in `format`, followed by the time-zone abbreviation when
/// one is known: `14:05 UTC` or `2:05 PM UTC`.
pub fn display(reading: &Reading, format: ClockFormat) -> String {
    let Ok(time) = reading else {
        return UNAVAILABLE.to_owned();
    };
    let clock = time_of_day(time, format);
    match &time.zone {
        Some(zone) => format!("{clock} {zone}"),
        None => clock,
    }
}

fn time_of_day(time: &LocalTime, format: ClockFormat) -> String {
    let LocalTime { hour, minute, .. } = *time;
    match format {
        ClockFormat::TwentyFourHour => format!("{hour:02}:{minute:02}"),
        ClockFormat::TwelveHour => {
            let suffix = if hour < 12 { "AM" } else { "PM" };
            let hour = match hour % 12 {
                0 => 12,
                hour => hour,
            };
            format!("{hour}:{minute:02} {suffix}")
        }
    }
}
