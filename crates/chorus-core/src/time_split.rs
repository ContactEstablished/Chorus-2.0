//! "Where the time went" for the Progress panel (P16, D119, D123 K6).
//!
//! Splits a run's time into four parts from the activity changes Chorus recorded, each timed by
//! Chorus's clock: agent working, waiting on you, waiting on approval and idle. Nothing here reads
//! a clock; the caller passes `now`.
//!
//! A waiting stretch (Ready, Failed, a final message ending with `?`, or Unknown before the first
//! event) counts as waiting on you for its first [`IDLE_AFTER_MS`] and as idle after that. A
//! question the agent asked is always waiting on you and an open approval always waiting on
//! approval, however long (D125). Nothing after the run's end counts.
//!
//! Known limit (D125): Claude Code sends nothing between an Allow and the tool finishing, so an
//! approved tool's running time counts as waiting on approval. Only the agent's own events end a
//! request (LESSONS L53), so Chorus does not count its own reply as the agent going back to work.
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::activity::Activity;

/// One recorded change: from `at_ms` the session's activity was `activity`. `guessed` marks a
/// NeedsAnswer that only came from the final message ending with `?` (`Tracker::guessed_question`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mark { pub at_ms: i64, pub activity: Activity, pub guessed: bool }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TimeSplit {
    /// Why there is no split ("not split"); `None` when the figures below are real.
    pub not_split: Option<String>,
    pub working_secs: u32,
    pub you_secs: u32,
    pub approval_secs: u32,
    pub idle_secs: u32,
    pub started_at_secs: u32,
    pub ended_at_secs: Option<u32>,
    pub turns: u32,
}

/// The most marks one split takes. One more is read, so a run with more reads "not split" rather
/// than a wrong total.
pub const MAX_MARKS: u32 = 20_000;
/// A waiting stretch counts as waiting on you for this long, then as idle (Matthew, 2026-11-15).
pub const IDLE_AFTER_MS: i64 = 15 * 60 * 1000;

pub const NOT_RECORDED: &str = "Recorded before Chorus kept state changes.";
pub const TOO_MANY: &str = "More state changes than Chorus splits (20,000).";
/// The run has only its starting row: nothing came from the agent, so there is no evidence to split.
pub const NO_EVENTS_YET: &str = "No events from the agent yet.";
pub const NO_EVENTS: &str = "The agent sent no events.";

#[derive(Clone, Copy)]
enum Bucket { Working, You, Approval, Waiting }

fn bucket(a: Activity, guessed: bool) -> Bucket {
    match a {
        Activity::Working => Bucket::Working,
        Activity::AwaitingApproval => Bucket::Approval,
        Activity::NeedsAnswer if guessed => Bucket::Waiting,
        Activity::NeedsAnswer => Bucket::You,
        // Inside a run, Unknown is only the time before the first event.
        Activity::Ready | Activity::Failed | Activity::Unknown => Bucket::Waiting,
    }
}

/// Instants: whole seconds, saturating.
fn at_secs(ms: i64) -> u32 { u32::try_from(ms.max(0) / 1000).unwrap_or(u32::MAX) }
/// Durations: rounded to the nearest second, saturating.
fn dur_secs(ms: i64) -> u32 { u32::try_from((ms.max(0) + 500) / 1000).unwrap_or(u32::MAX) }

/// The split of one run, from `run_started_ms` to its end: `run_ended_ms`, else `now_ms` while the
/// run is `live`, else its last mark (a run that stopped without a recorded end never grows).
pub fn split(run_started_ms: i64, run_ended_ms: Option<i64>, live: bool, marks: &[Mark], now_ms: i64, turns: u32) -> TimeSplit {
    let start = run_started_ms;
    let ended = run_ended_ms.or_else(|| (!live).then(|| marks.iter().map(|m| m.at_ms).max().unwrap_or(start)));
    let end = ended.unwrap_or(now_ms).max(start);
    let mut out = TimeSplit {
        not_split: None, working_secs: 0, you_secs: 0, approval_secs: 0, idle_secs: 0,
        started_at_secs: at_secs(start), ended_at_secs: ended.map(|e| at_secs(e.max(start))), turns,
    };
    if marks.is_empty() {
        out.not_split = Some(NOT_RECORDED.into());
        return out;
    }
    if marks.len() > MAX_MARKS as usize {
        out.not_split = Some(TOO_MANY.into());
        return out;
    }
    if marks.iter().all(|m| m.activity == Activity::Unknown) {
        out.not_split = Some(if ended.is_some() { NO_EVENTS } else { NO_EVENTS_YET }.into());
        return out;
    }
    let mut sorted = marks.to_vec();
    sorted.sort_by_key(|m| m.at_ms);

    let (mut working, mut you, mut approval, mut idle) = (0i64, 0i64, 0i64, 0i64);
    // Time already spent in the current waiting stretch, which runs across every Waiting state.
    let mut waited = 0i64;
    let mut count = |(activity, guessed): (Activity, bool), from: i64, to: i64| {
        let d = to.min(end) - from.max(start);
        let d = d.max(0);
        match bucket(activity, guessed) {
            Bucket::Working => { working += d; waited = 0; }
            Bucket::Approval => { approval += d; waited = 0; }
            Bucket::You => { you += d; waited = 0; }
            Bucket::Waiting => {
                let as_you = d.min((IDLE_AFTER_MS - waited).max(0));
                you += as_you;
                idle += d - as_you;
                waited += d;
            }
        }
    };
    let mut state = (Activity::Unknown, false);
    let mut from = start;
    for m in &sorted {
        // A mark before the run's start sets the state it starts in; it adds no time.
        let at = m.at_ms.max(start);
        count(state, from, at);
        state = (m.activity, m.guessed);
        from = at;
    }
    count(state, from, end);

    out.working_secs = dur_secs(working);
    out.you_secs = dur_secs(you);
    out.approval_secs = dur_secs(approval);
    out.idle_secs = dur_secs(idle);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use Activity::*;

    const MIN: i64 = 60_000;

    fn m(at_ms: i64, activity: Activity) -> Mark { Mark { at_ms, activity, guessed: false } }
    fn guess(at_ms: i64) -> Mark { Mark { at_ms, activity: NeedsAnswer, guessed: true } }
    fn parts(s: &TimeSplit) -> [u32; 4] { [s.working_secs, s.you_secs, s.approval_secs, s.idle_secs] }
    fn total(s: &TimeSplit) -> i64 { parts(s).iter().map(|&x| i64::from(x)).sum() }

    #[test]
    fn no_marks_is_not_split() {
        let s = split(1_000, Some(9_000), false, &[], 20_000, 3);
        assert_eq!(s.not_split.as_deref(), Some(NOT_RECORDED));
        assert_eq!(parts(&s), [0; 4]);
        assert_eq!((s.started_at_secs, s.ended_at_secs, s.turns), (1, Some(9), 3));
    }

    #[test]
    fn too_many_marks_is_not_split() {
        let marks = vec![m(0, Working); MAX_MARKS as usize];
        assert_eq!(split(0, None, true, &marks, 10, 0).not_split, None, "exactly the limit is split");
        let marks = vec![m(0, Working); MAX_MARKS as usize + 1];
        assert_eq!(split(0, None, true, &marks, 10, 0).not_split.as_deref(), Some(TOO_MANY));
    }

    #[test]
    fn each_state_counts_in_its_bucket() {
        let marks = [m(0, Unknown), m(10_000, Working), m(40_000, AwaitingApproval), m(60_000, Working), m(70_000, NeedsAnswer), m(100_000, Working), m(110_000, Ready)];
        let s = split(0, Some(130_000), false, &marks, 999_999, 2);
        assert_eq!(s.not_split, None);
        // Working 30 + 10 + 10; you: 10 Unknown + 30 question + 20 Ready; approval 20.
        assert_eq!(parts(&s), [50, 60, 20, 0]);
    }

    #[test]
    fn a_live_run_counts_up_to_now() {
        let s = split(0, None, true, &[m(0, Unknown), m(5_000, Working)], 65_000, 1);
        assert_eq!(parts(&s), [60, 5, 0, 0]);
        assert_eq!(s.ended_at_secs, None);
    }

    #[test]
    fn nothing_after_the_run_end_counts() {
        let marks = [m(0, Working), m(50_000, Ready), m(60_000, Unknown)];
        let s = split(0, Some(60_000), false, &marks, 10 * 3_600_000, 1);
        assert_eq!(parts(&s), [50, 10, 0, 0]);
        assert_eq!(s.ended_at_secs, Some(60));
    }

    #[test]
    fn an_end_before_the_start_gives_zero_time() {
        let s = split(10_000, Some(5_000), false, &[m(10_000, Working)], 20_000, 0);
        assert_eq!(parts(&s), [0; 4]);
        assert_eq!(s.ended_at_secs, Some(10));
    }

    #[test]
    fn marks_outside_the_run_are_clipped() {
        // A Working mark before the start sets the opening state; one after the end adds nothing.
        let marks = [m(-30_000, Working), m(20_000, Ready), m(90_000, AwaitingApproval)];
        let s = split(0, Some(40_000), false, &marks, 100_000, 0);
        assert_eq!(parts(&s), [20, 20, 0, 0]);
    }

    #[test]
    fn totals_add_up_to_the_span_within_rounding() {
        // Odd millisecond boundaries in all four buckets.
        let marks = [m(0, Unknown), m(1_499, Working), m(3_001, AwaitingApproval), m(4_499, NeedsAnswer), m(6_501, Ready)];
        let end = 6_501 + IDLE_AFTER_MS + 2_499;
        let s = split(0, Some(end), false, &marks, end, 0);
        assert!(parts(&s).iter().all(|&x| x > 0), "{s:?}");
        let span_secs = (end + 500) / 1000;
        assert!((total(&s) - span_secs).abs() <= 2, "{} vs {span_secs}", total(&s));
    }

    #[test]
    fn out_of_order_marks_are_sorted() {
        let a = split(0, Some(30_000), false, &[m(0, Working), m(10_000, NeedsAnswer), m(20_000, Working)], 0, 0);
        let b = split(0, Some(30_000), false, &[m(20_000, Working), m(0, Working), m(10_000, NeedsAnswer)], 0, 0);
        assert_eq!(a, b);
        assert_eq!(parts(&a), [20, 10, 0, 0]);
    }

    #[test]
    fn ready_for_exactly_fifteen_minutes_is_all_waiting_on_you() {
        let s = split(0, Some(MIN + IDLE_AFTER_MS), false, &[m(0, Working), m(MIN, Ready)], 0, 1);
        assert_eq!(parts(&s), [60, 900, 0, 0]);
    }

    #[test]
    fn ready_for_fifteen_minutes_and_a_second_has_one_second_idle() {
        let s = split(0, Some(MIN + IDLE_AFTER_MS + 1_000), false, &[m(0, Working), m(MIN, Ready)], 0, 1);
        assert_eq!(parts(&s), [60, 900, 0, 1]);
    }

    #[test]
    fn two_short_ready_gaps_never_add_up_to_idle() {
        let marks = [m(0, Ready), m(10 * MIN, Working), m(11 * MIN, Ready)];
        let s = split(0, Some(21 * MIN), false, &marks, 0, 1);
        assert_eq!(parts(&s), [60, 20 * 60, 0, 0]);
    }

    #[test]
    fn an_open_question_is_never_idle() {
        let s = split(0, None, true, &[m(0, Working), m(MIN, NeedsAnswer)], 61 * MIN, 1);
        assert_eq!(parts(&s), [60, 60 * 60, 0, 0]);
    }

    #[test]
    fn an_open_approval_is_never_idle() {
        let s = split(0, None, true, &[m(0, Working), m(MIN, AwaitingApproval)], 61 * MIN, 1);
        assert_eq!(parts(&s), [60, 0, 60 * 60, 0]);
    }

    #[test]
    fn failed_follows_the_fifteen_minute_rule() {
        let s = split(0, None, true, &[m(0, Working), m(MIN, Failed)], 61 * MIN, 1);
        assert_eq!(parts(&s), [60, 15 * 60, 0, 45 * 60]);
    }

    #[test]
    fn unknown_before_the_first_event_becomes_idle_after_fifteen_minutes() {
        let s = split(0, None, true, &[m(0, Unknown), m(20 * MIN, Working)], 21 * MIN, 1);
        assert_eq!(parts(&s), [60, 15 * 60, 0, 5 * 60]);
        // With no row at the start, the time before the first mark is Unknown too.
        assert_eq!(split(0, None, true, &[m(20 * MIN, Working)], 21 * MIN, 1), s);
    }

    #[test]
    fn failed_then_ready_is_one_stretch() {
        let marks = [m(0, Working), m(MIN, Failed), m(11 * MIN, Ready)];
        let s = split(0, None, true, &marks, 21 * MIN, 1);
        assert_eq!(parts(&s), [60, 15 * 60, 0, 5 * 60]);
    }

    #[test]
    fn a_stopped_run_without_an_end_stops_at_its_last_mark() {
        let s = split(0, None, false, &[m(0, Working), m(MIN, Ready)], 600 * MIN, 1);
        assert_eq!(parts(&s), [60, 0, 0, 0]);
        assert_eq!(s.ended_at_secs, Some(60));
        assert_eq!(split(5_000, None, false, &[], 9_000_000, 0).ended_at_secs, Some(5), "no marks: it ends where it started");
    }

    #[test]
    fn a_question_resets_the_stretch() {
        let marks = [m(0, Ready), m(10 * MIN, NeedsAnswer), m(11 * MIN, Ready)];
        let s = split(0, Some(21 * MIN), false, &marks, 0, 0);
        assert_eq!(parts(&s), [0, 21 * 60, 0, 0]);
    }

    #[test]
    fn a_final_message_ending_with_a_question_goes_idle_after_fifteen_minutes() {
        let s = split(0, None, true, &[m(0, Working), guess(MIN)], 61 * MIN, 1);
        assert_eq!(parts(&s), [60, 15 * 60, 0, 45 * 60]);
        // A real question after it is never idle.
        let marks = [m(0, Working), guess(MIN), m(11 * MIN, NeedsAnswer)];
        assert_eq!(parts(&split(0, None, true, &marks, 61 * MIN, 1)), [60, 60 * 60, 0, 0]);
    }

    #[test]
    fn a_run_with_only_its_starting_row_is_not_split() {
        let live = split(0, None, true, &[m(0, Unknown)], 30 * MIN, 0);
        assert_eq!((live.not_split.as_deref(), parts(&live)), (Some(NO_EVENTS_YET), [0; 4]));
        let ended = split(0, Some(30 * MIN), false, &[m(0, Unknown)], 40 * MIN, 0);
        assert_eq!((ended.not_split.as_deref(), ended.ended_at_secs), (Some(NO_EVENTS), Some(30 * 60)));
        assert_eq!(split(0, None, true, &[m(0, Unknown), m(MIN, Working)], 2 * MIN, 1).not_split, None, "one event is enough");
    }
}
