//! Finding the rollout a Codex run started, so later runs can resume its conversation. The rules
//! are in `chorus_core::codex`; this reads the files. Codex writes the rollout only once the first
//! prompt is sent (Chorus 1 measured 22 s to 3 minutes), so the session keeps looking while the
//! run lives.

use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use chorus_core::codex::{RolloutHead, parse_rollout_head, pick};

use super::env_value;

/// A rollout's first line holds Codex's base instructions, so it can be large.
const MAX_HEAD_BYTES: u64 = 4 * 1024 * 1024;
/// How many of the newest day folders to search: a launch near midnight can land in either.
const DAY_FOLDERS: usize = 3;
pub const POLL: Duration = Duration::from_secs(2);

/// `$CODEX_HOME/sessions`, or `~/.codex/sessions`.
pub fn sessions_root(parent_env: &[(String, String)]) -> Option<PathBuf> {
    env_value(parent_env, "CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| env_value(parent_env, "USERPROFILE").map(|h| Path::new(&h).join(".codex")))
        .map(|h| h.join("sessions"))
}

/// The conversation id of the rollout a launch at `launched_ms` started, if one can be told
/// apart yet.
pub fn find(root: &Path, originator: &str, cwd: &str, launched_ms: i64) -> Option<String> {
    let heads: Vec<RolloutHead> = recent_day_folders(root)
        .iter()
        .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("rollout-") && n.ends_with(".jsonl"))
        })
        .filter(|p| modified_ms(p).is_some_and(|m| m >= launched_ms - 5_000))
        .filter_map(|p| first_line(&p))
        .filter_map(|l| parse_rollout_head(&l))
        .collect();
    pick(&heads, originator, cwd, launched_ms).map(str::to_owned)
}

/// `sessions/YYYY/MM/DD` folders, newest first.
fn recent_day_folders(root: &Path) -> Vec<PathBuf> {
    let children = |p: &Path| -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        v.sort();
        v.reverse();
        v
    };
    let mut out = Vec::new();
    'outer: for year in children(root) {
        for month in children(&year) {
            for day in children(&month) {
                out.push(day);
                if out.len() == DAY_FOLDERS {
                    break 'outer;
                }
            }
        }
    }
    out
}

fn modified_ms(p: &Path) -> Option<i64> {
    let t = std::fs::metadata(p).ok()?.modified().ok()?;
    Some(t.duration_since(UNIX_EPOCH).ok()?.as_millis() as i64)
}

fn first_line(p: &Path) -> Option<String> {
    let mut line = String::new();
    BufReader::new(File::open(p).ok()?.take(MAX_HEAD_BYTES)).read_line(&mut line).ok()?;
    line.ends_with('\n').then_some(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chorus_core::codex::originator_for;

    #[test]
    fn finds_its_rollout_in_the_newest_day_folder() {
        let root = std::env::temp_dir().join(format!("chorus-codex-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let old = root.join("2026").join("10").join("01");
        let today = root.join("2026").join("10").join("19");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&today).unwrap();
        let id = "0f8fad5b-d9cb-469f-a165-70867728950e";
        let meta = |ts: &str, originator: &str| {
            format!(
                r#"{{"type":"session_meta","payload":{{"id":"{id}","timestamp":"{ts}","cwd":"C:\\work","originator":"{originator}"}}}}"#
            ) + "\n{\"type\":\"event_msg\"}\n"
        };
        let now_ms = std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64;
        // Stamped one second after a launch at `now_ms - 1000`.
        let secs = (now_ms / 1000) as u64;
        let ts = chrono_like(secs);
        std::fs::write(today.join(format!("rollout-x-{id}.jsonl")), meta(&ts, &originator_for(3))).unwrap();
        std::fs::write(today.join("notes.txt"), "not a rollout").unwrap();
        assert_eq!(find(&root, &originator_for(3), "C:\\work", now_ms - 1_000).as_deref(), Some(id));
        assert_eq!(find(&root, &originator_for(4), "C:\\other", now_ms - 1_000), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Seconds since the epoch as `YYYY-MM-DDTHH:MM:SSZ` (civil_from_days), for the fixture.
    fn chrono_like(secs: u64) -> String {
        let days = (secs / 86_400) as i64;
        let rem = secs % 86_400;
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + i64::from(m <= 2);
        format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
    }
}
