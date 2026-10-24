//! Automation schedules (P8): every N minutes, daily at a time, or on chosen weekdays at a time, in
//! local time. Schedules run only while Chorus runs; a time that passed while it was closed is
//! missed, recorded and not caught up (decision 3).
//!
//! The local offset is a function the caller passes (`offset_at(utc_ms)` in minutes east of UTC),
//! so a daylight-saving change between now and the next run is accounted for without a clock here.

use serde::{Deserialize, Serialize};
use specta::Type;

pub const MIN_EVERY_MINUTES: u32 = 15;
pub const MAX_EVERY_MINUTES: u32 = 7 * 24 * 60;
const MINUTE_MS: i64 = 60_000;
const DAY_MS: i64 = 24 * 60 * MINUTE_MS;
const DAY_NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Schedule {
    /// Every `minutes` (at least [`MIN_EVERY_MINUTES`]), counted from when it was set up.
    Every { minutes: u32 },
    /// Each day at `hour:minute` local time.
    Daily { hour: u8, minute: u8 },
    /// On `days` (0 is Monday … 6 is Sunday) at `hour:minute` local time.
    Weekdays { days: Vec<u8>, hour: u8, minute: u8 },
}

/// What the scheduler should do about an automation now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Due {
    Wait,
    Fire,
    /// It was due while Chorus was closed: record it as missed and move on.
    Missed,
}

/// Checks a schedule and returns it with its days sorted and repeats dropped.
pub fn check(s: &Schedule) -> Result<Schedule, String> {
    let time = |hour: u8, minute: u8| {
        if hour > 23 || minute > 59 { Err("Choose a time between 00:00 and 23:59.".to_owned()) } else { Ok(()) }
    };
    match s {
        Schedule::Every { minutes } if *minutes < MIN_EVERY_MINUTES => {
            Err(format!("An automation runs at most every {MIN_EVERY_MINUTES} minutes."))
        }
        Schedule::Every { minutes } if *minutes > MAX_EVERY_MINUTES => Err("Choose an interval of up to a week.".into()),
        Schedule::Every { .. } => Ok(s.clone()),
        Schedule::Daily { hour, minute } => time(*hour, *minute).map(|()| s.clone()),
        Schedule::Weekdays { days, hour, minute } => {
            time(*hour, *minute)?;
            let mut d: Vec<u8> = days.clone();
            d.sort_unstable();
            d.dedup();
            if d.is_empty() {
                return Err("Choose at least one day.".into());
            }
            if d.iter().any(|x| *x > 6) {
                return Err("A day is 0 (Monday) to 6 (Sunday).".into());
            }
            Ok(Schedule::Weekdays { days: d, hour: *hour, minute: *minute })
        }
    }
}

/// In words, for the Automations screen.
pub fn describe(s: &Schedule) -> String {
    match s {
        Schedule::Every { minutes } if minutes.is_multiple_of(60) && *minutes >= 60 => {
            let h = minutes / 60;
            if h == 1 { "Every hour".into() } else { format!("Every {h} hours") }
        }
        Schedule::Every { minutes } => format!("Every {minutes} minutes"),
        Schedule::Daily { hour, minute } => format!("Daily at {hour:02}:{minute:02}"),
        Schedule::Weekdays { days, hour, minute } => {
            let names: Vec<&str> = days.iter().filter_map(|d| DAY_NAMES.get(*d as usize).copied()).collect();
            let which = match days.as_slice() {
                [0, 1, 2, 3, 4] => "Weekdays".to_owned(),
                [5, 6] => "Weekends".to_owned(),
                _ => names.join(", "),
            };
            format!("{which} at {hour:02}:{minute:02}")
        }
    }
}

/// The first due time strictly after `after` (all Unix milliseconds, UTC). `anchor` is when an
/// "every" schedule counts from. `offset_at` gives the local offset, in minutes, at a UTC instant.
pub fn next_run(s: &Schedule, after: i64, anchor: i64, offset_at: &dyn Fn(i64) -> i32) -> i64 {
    match s {
        Schedule::Every { minutes } => {
            let period = i64::from((*minutes).max(MIN_EVERY_MINUTES)) * MINUTE_MS;
            if anchor > after {
                return anchor;
            }
            anchor + ((after - anchor) / period + 1) * period
        }
        Schedule::Daily { hour, minute } => next_local(after, *hour, *minute, &[0, 1, 2, 3, 4, 5, 6], offset_at),
        Schedule::Weekdays { days, hour, minute } => next_local(after, *hour, *minute, days, offset_at),
    }
}

fn next_local(after: i64, hour: u8, minute: u8, days: &[u8], offset_at: &dyn Fn(i64) -> i32) -> i64 {
    let local_now = after + i64::from(offset_at(after)) * MINUTE_MS;
    let today = local_now.div_euclid(DAY_MS);
    let at = (i64::from(hour) * 60 + i64::from(minute)) * MINUTE_MS;
    for d in 0..=8 {
        let day = today + d;
        // 1970-01-01 was a Thursday (3, counting Monday as 0).
        let weekday = (day + 3).rem_euclid(7) as u8;
        if !days.contains(&weekday) {
            continue;
        }
        let local = day * DAY_MS + at;
        // The offset at that local time, not now's: a daylight-saving change may lie between.
        let guess = local - i64::from(offset_at(after)) * MINUTE_MS;
        let utc = local - i64::from(offset_at(guess)) * MINUTE_MS;
        if utc > after {
            return utc;
        }
    }
    // Unreachable for a checked schedule (it has a day); a week on is a safe answer.
    after + 7 * DAY_MS
}

/// Whether an automation due at `next_run_at` fires now. A time before this Chorus started was
/// missed while it was closed.
pub fn due(next_run_at: i64, now: i64, app_started_at: i64) -> Due {
    if next_run_at > now {
        Due::Wait
    } else if next_run_at < app_started_at {
        Due::Missed
    } else {
        Due::Fire
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = 60 * MINUTE_MS;
    /// 2026-10-23 00:00 UTC, a Friday.
    const FRI: i64 = 1_792_713_600_000;

    fn utc(_: i64) -> i32 {
        0
    }

    #[test]
    fn friday_is_a_friday() {
        assert_eq!((FRI / DAY_MS + 3).rem_euclid(7), 4);
    }

    #[test]
    fn every_counts_from_its_anchor() {
        let s = Schedule::Every { minutes: 15 };
        assert_eq!(next_run(&s, FRI, FRI, &utc), FRI + 15 * MINUTE_MS);
        assert_eq!(next_run(&s, FRI + 20 * MINUTE_MS, FRI, &utc), FRI + 30 * MINUTE_MS);
        assert_eq!(next_run(&s, FRI, FRI + HOUR, &utc), FRI + HOUR);
    }

    #[test]
    fn the_minimum_interval_and_times_are_checked() {
        assert!(check(&Schedule::Every { minutes: 14 }).is_err());
        assert!(check(&Schedule::Every { minutes: 15 }).is_ok());
        assert!(check(&Schedule::Every { minutes: MAX_EVERY_MINUTES + 1 }).is_err());
        assert!(check(&Schedule::Daily { hour: 24, minute: 0 }).is_err());
        assert!(check(&Schedule::Weekdays { days: vec![], hour: 9, minute: 0 }).is_err());
        assert!(check(&Schedule::Weekdays { days: vec![7], hour: 9, minute: 0 }).is_err());
        assert_eq!(
            check(&Schedule::Weekdays { days: vec![4, 0, 4], hour: 9, minute: 0 }).unwrap(),
            Schedule::Weekdays { days: vec![0, 4], hour: 9, minute: 0 }
        );
    }

    #[test]
    fn daily_runs_today_if_the_time_is_ahead_else_tomorrow() {
        let s = Schedule::Daily { hour: 9, minute: 30 };
        assert_eq!(next_run(&s, FRI + 8 * HOUR, 0, &utc), FRI + 9 * HOUR + 30 * MINUTE_MS);
        assert_eq!(next_run(&s, FRI + 10 * HOUR, 0, &utc), FRI + DAY_MS + 9 * HOUR + 30 * MINUTE_MS);
        // Exactly at the time: the next one.
        assert_eq!(next_run(&s, FRI + 9 * HOUR + 30 * MINUTE_MS, 0, &utc), FRI + DAY_MS + 9 * HOUR + 30 * MINUTE_MS);
    }

    #[test]
    fn local_time_uses_the_offset() {
        // UTC-4 (EDT): 09:00 local is 13:00 UTC.
        let edt = |_: i64| -240;
        let s = Schedule::Daily { hour: 9, minute: 0 };
        assert_eq!(next_run(&s, FRI, 0, &edt), FRI + 13 * HOUR);
        // Late on Friday UTC is still Friday evening locally; Monday-only skips to Monday.
        let s = Schedule::Weekdays { days: vec![0], hour: 9, minute: 0 };
        assert_eq!(next_run(&s, FRI + 2 * HOUR, 0, &edt), FRI + 3 * DAY_MS + 13 * HOUR);
    }

    #[test]
    fn weekdays_skip_the_weekend() {
        let s = Schedule::Weekdays { days: vec![0, 1, 2, 3, 4], hour: 9, minute: 0 };
        // Friday 10:00: next is Monday 09:00.
        assert_eq!(next_run(&s, FRI + 10 * HOUR, 0, &utc), FRI + 3 * DAY_MS + 9 * HOUR);
        assert_eq!(describe(&s), "Weekdays at 09:00");
        assert_eq!(describe(&Schedule::Weekdays { days: vec![0, 2], hour: 7, minute: 5 }), "Mon, Wed at 07:05");
        assert_eq!(describe(&Schedule::Every { minutes: 120 }), "Every 2 hours");
        assert_eq!(describe(&Schedule::Every { minutes: 15 }), "Every 15 minutes");
    }

    #[test]
    fn a_daylight_saving_change_keeps_the_local_time() {
        // Clocks go back at 2026-11-01 06:00 UTC (02:00 EDT becomes 01:00 EST).
        let change = FRI + 9 * DAY_MS + 6 * HOUR;
        let ny = move |t: i64| if t < change { -240 } else { -300 };
        let s = Schedule::Daily { hour: 9, minute: 0 };
        // From Saturday 31 October 14:00 UTC (10:00 EDT), the next 09:00 is Sunday's, in EST.
        let sat = FRI + 8 * DAY_MS + 14 * HOUR;
        assert_eq!(next_run(&s, sat, 0, &ny), FRI + 9 * DAY_MS + 14 * HOUR);
    }

    #[test]
    fn times_before_this_start_were_missed() {
        assert_eq!(due(100, 50, 0), Due::Wait);
        assert_eq!(due(100, 150, 120), Due::Missed);
        assert_eq!(due(100, 150, 90), Due::Fire);
        assert_eq!(due(100, 100, 90), Due::Fire);
    }
}
