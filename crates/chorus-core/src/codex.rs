//! Finding the conversation a Codex run started. Codex names its own conversations, so a session
//! learns the id from Codex's rollout files after launch (`agent::ResumeKind::Discovered`; ported
//! from Chorus 1's `codex.ts` discovery, LESSONS L8).
//!
//! A rollout's first line is a `session_meta` record. Chorus launches Codex with
//! `CODEX_INTERNAL_ORIGINATOR_OVERRIDE` set to [`originator_for`] the session, which Codex writes
//! into that record. The variable is undocumented (Chorus 1 found it; it is in Codex 0.162.1's
//! binary), so a rollout without it is also accepted when its folder and start time match a single
//! launch closely.

use serde_json::Value;

/// How long after a launch an unmarked rollout may start and still be taken for it.
pub const MATCH_WINDOW_MS: i64 = 10_000;
/// Clock slack between Chorus noting the launch and Codex stamping the rollout.
const SLACK_MS: i64 = 2_000;

/// The marker for a Chorus 2.0 session. Distinct from Chorus 1's `chorus-<n>`, so neither app
/// claims the other's rollouts.
pub fn originator_for(session_id: u32) -> String {
    format!("chorus-v2-{session_id}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolloutHead {
    /// The conversation id `codex resume` takes (the one in the file name).
    pub id: String,
    pub cwd: String,
    pub started_ms: Option<i64>,
    pub originator: Option<String>,
    /// A subagent's thread, which belongs to its parent's conversation.
    pub subagent: bool,
}

/// Reads a rollout's first line. Anything that isn't a `session_meta` record with an id and a
/// folder is not a candidate.
pub fn parse_rollout_head(line: &str) -> Option<RolloutHead> {
    let v: Value = serde_json::from_str(line).ok()?;
    if v.get("type")?.as_str()? != "session_meta" {
        return None;
    }
    let p = v.get("payload")?;
    let text = |k: &str| p.get(k).and_then(Value::as_str).map(str::to_owned);
    Some(RolloutHead {
        id: text("id")?,
        cwd: text("cwd")?,
        started_ms: text("timestamp").as_deref().and_then(parse_utc_ms),
        originator: text("originator"),
        subagent: p.get("parent_thread_id").is_some_and(|x| !x.is_null()),
    })
}

/// The conversation a launch started, if exactly one rollout can be its: first one carrying the
/// session's marker, else a single unmarked one in the same folder that started within
/// [`MATCH_WINDOW_MS`] of the launch. Rollouts marked by any other Chorus session are never taken.
pub fn pick<'a>(heads: &'a [RolloutHead], originator: &str, cwd: &str, launched_ms: i64) -> Option<&'a str> {
    let usable = || {
        heads.iter().filter(|h| !h.subagent && crate::agent::is_uuid(&h.id)).filter(|h| {
            h.started_ms.is_some_and(|t| t >= launched_ms - SLACK_MS)
        })
    };
    if let Some(h) = usable().filter(|h| h.originator.as_deref() == Some(originator)).min_by_key(|h| h.started_ms) {
        return Some(&h.id);
    }
    let mut unmarked = usable().filter(|h| {
        !h.originator.as_deref().is_some_and(|o| o.starts_with("chorus"))
            && same_folder(&h.cwd, cwd)
            && h.started_ms.is_some_and(|t| t <= launched_ms + MATCH_WINDOW_MS)
    });
    match (unmarked.next(), unmarked.next()) {
        (Some(h), None) => Some(&h.id),
        _ => None,
    }
}

/// Windows paths compared the way Windows does: any case, either slash, no trailing separator.
pub fn same_folder(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.replace('/', "\\").trim_end_matches('\\').to_lowercase();
    norm(a) == norm(b)
}

/// `2026-10-19T16:35:29.900Z` (or without fractions) as Unix milliseconds. Only UTC (`Z`) is
/// accepted, which is what Codex writes.
pub fn parse_utc_ms(s: &str) -> Option<i64> {
    let s = s.strip_suffix('Z')?;
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-').map(|x| x.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let (hms, frac) = time.split_once('.').unwrap_or((time, "0"));
    let mut t = hms.split(':').map(|x| x.parse::<i64>().ok());
    let (h, min, sec) = (t.next()??, t.next()??, t.next()??);
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) || h > 23 || min > 59 || sec > 60 {
        return None;
    }
    let millis: i64 = format!("{:0<3}", frac.get(..3).unwrap_or(frac)).parse().ok()?;
    // Days from 1970-01-01 (Howard Hinnant's days_from_civil).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 24 + h) * 60 + min) * 60_000 + sec * 1000 + millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";
    const B: &str = "7c9e6679-7425-40de-944b-e07fc1f90ae7";

    fn head(id: &str, cwd: &str, started_ms: i64, originator: Option<&str>) -> RolloutHead {
        RolloutHead {
            id: id.into(),
            cwd: cwd.into(),
            started_ms: Some(started_ms),
            originator: originator.map(str::to_owned),
            subagent: false,
        }
    }

    #[test]
    fn reads_the_session_meta_line() {
        let line = r#"{"timestamp":"x","type":"session_meta","payload":{"id":"0f8fad5b-d9cb-469f-a165-70867728950e","session_id":"0f8fad5b-d9cb-469f-a165-70867728950e","parent_thread_id":null,"timestamp":"2026-10-19T16:35:29.900Z","cwd":"C:\\work","originator":"chorus-v2-7"}}"#;
        let h = parse_rollout_head(line).unwrap();
        assert_eq!((h.id.as_str(), h.cwd.as_str(), h.originator.as_deref(), h.subagent), (A, "C:\\work", Some("chorus-v2-7"), false));
        assert_eq!(h.started_ms, Some(1_792_427_729_900));
        assert!(parse_rollout_head(r#"{"type":"event_msg","payload":{}}"#).is_none());
        assert!(parse_rollout_head("not json").is_none());
        let sub = line.replace(r#""parent_thread_id":null"#, r#""parent_thread_id":"abc""#);
        assert!(parse_rollout_head(&sub).unwrap().subagent);
    }

    #[test]
    fn parses_utc_timestamps() {
        assert_eq!(parse_utc_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_utc_ms("2000-03-01T00:00:00.5Z"), Some(951_868_800_500));
        assert_eq!(parse_utc_ms("2026-10-19T16:35:29.900Z"), Some(1_792_427_729_900));
        assert_eq!(parse_utc_ms("2026-10-19T16:35:29+02:00"), None);
        assert_eq!(parse_utc_ms("2026-13-01T00:00:00Z"), None);
    }

    #[test]
    fn its_own_marker_wins() {
        let ours = originator_for(7);
        let heads = [head(B, "C:\\other", 5_000, Some("chorus-v2-7")), head(A, "C:\\work", 1_500, None)];
        assert_eq!(pick(&heads, &ours, "C:\\work", 1_000), Some(B));
    }

    #[test]
    fn an_unmarked_rollout_needs_the_folder_the_window_and_no_rival() {
        let ours = originator_for(7);
        let launched = 100_000;
        assert_eq!(pick(&[head(A, "c:/WORK/", launched + 3_000, Some("codex-tui"))], &ours, "C:\\work", launched), Some(A));
        // Too late, or before the launch.
        assert_eq!(pick(&[head(A, "C:\\work", launched + MATCH_WINDOW_MS + 1, None)], &ours, "C:\\work", launched), None);
        assert_eq!(pick(&[head(A, "C:\\work", launched - 5_000, None)], &ours, "C:\\work", launched), None);
        // Two candidates: never guess.
        let two = [head(A, "C:\\work", launched + 1_000, None), head(B, "C:\\work", launched + 2_000, None)];
        assert_eq!(pick(&two, &ours, "C:\\work", launched), None);
        // Another Chorus session's (or Chorus 1's) rollout is never taken.
        for other in ["chorus-v2-8", "chorus-12"] {
            assert_eq!(pick(&[head(A, "C:\\work", launched + 1_000, Some(other))], &ours, "C:\\work", launched), None);
        }
    }

    #[test]
    fn subagent_threads_and_bad_ids_are_skipped() {
        let ours = originator_for(7);
        let mut sub = head(A, "C:\\work", 2_000, Some("chorus-v2-7"));
        sub.subagent = true;
        assert_eq!(pick(&[sub], &ours, "C:\\work", 1_000), None);
        assert_eq!(pick(&[head("abc", "C:\\work", 2_000, Some("chorus-v2-7"))], &ours, "C:\\work", 1_000), None);
    }
}
