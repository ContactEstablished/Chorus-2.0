//! "Usual time" markers (P16, D121): once a named skill has enough earlier timed runs, a run far
//! shorter or longer than their median gets a quiet marker. Pure: the caller reads the earlier runs.
use serde::{Deserialize, Serialize};
use specta::Type;

/// Earlier timed runs needed before any marker (D121).
pub const MIN_RUNS: usize = 5;
/// How many earlier runs are compared.
pub const WINDOW: u32 = 20;
/// Under a third, or over three times, the median (D121).
pub const FACTOR: i64 = 3;
/// A median under a second is too small to compare.
pub const MIN_MEDIAN_MS: i64 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum Direction { Shorter, Longer }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UsualMarker {
    pub direction: Direction,
    /// The median of the earlier runs, rounded to seconds.
    pub usual_secs: u32,
    /// How many earlier runs it was compared with.
    pub runs: u32,
}

/// The median; for an even count, the mean of the middle two (rounded down). `None` when empty.
pub fn median_ms(past: &[i64]) -> Option<i64> {
    if past.is_empty() { return None; }
    let mut sorted = past.to_vec();
    sorted.sort_unstable();
    let mid = sorted.len() / 2;
    Some(if sorted.len() % 2 == 1 { sorted[mid] } else {
        let (a, b) = (sorted[mid - 1], sorted[mid]);
        a / 2 + b / 2 + (a % 2 + b % 2) / 2
    })
}

/// `past_ms`: earlier timed runs of one key, in any order. `this_ms`: this run so far. A run that
/// is still going can only be "longer": it may yet run long enough not to be short.
pub fn marker(past_ms: &[i64], this_ms: i64, finished: bool) -> Option<UsualMarker> {
    if past_ms.len() < MIN_RUNS { return None; }
    let median = median_ms(past_ms)?;
    if median < MIN_MEDIAN_MS { return None; }
    let this_ms = this_ms.max(0);
    let direction = if finished && this_ms.saturating_mul(FACTOR) < median {
        Direction::Shorter
    } else if this_ms > median.saturating_mul(FACTOR) {
        Direction::Longer
    } else {
        return None;
    };
    Some(UsualMarker {
        direction,
        usual_secs: u32::try_from((median.saturating_add(500)) / 1000).unwrap_or(u32::MAX),
        runs: u32::try_from(past_ms.len()).unwrap_or(u32::MAX),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIVE: [i64; 5] = [60_000, 50_000, 70_000, 65_000, 55_000]; // median 60 s

    #[test]
    fn no_marker_before_five_runs() {
        assert_eq!(marker(&FIVE[..4], 1_000, true), None);
        assert_eq!(marker(&FIVE[..4], 900_000, true), None);
        assert!(marker(&FIVE, 1_000, true).is_some());
    }

    #[test]
    fn a_run_under_a_third_of_the_median_is_shorter() {
        assert_eq!(marker(&FIVE, 19_999, true), Some(UsualMarker { direction: Direction::Shorter, usual_secs: 60, runs: 5 }));
    }

    #[test]
    fn a_run_over_three_times_the_median_is_longer() {
        assert_eq!(marker(&FIVE, 180_001, true), Some(UsualMarker { direction: Direction::Longer, usual_secs: 60, runs: 5 }));
    }

    #[test]
    fn exactly_a_third_or_three_times_is_not_marked() {
        assert_eq!(marker(&FIVE, 20_000, true), None);
        assert_eq!(marker(&FIVE, 180_000, true), None);
        assert_eq!(marker(&FIVE, 60_000, true), None);
    }

    #[test]
    fn a_running_skill_can_only_be_longer() {
        assert_eq!(marker(&FIVE, 1_000, false), None);
        assert_eq!(marker(&FIVE, 200_000, false).map(|m| m.direction), Some(Direction::Longer));
    }

    #[test]
    fn median_of_an_even_count_is_the_mean_of_the_middle_two() {
        assert_eq!(median_ms(&[4, 1, 3, 2]), Some(2));
        assert_eq!(median_ms(&[10_000, 20_000, 30_000, 41_000]), Some(25_000));
        assert_eq!(median_ms(&[3, 1, 2]), Some(2));
        assert_eq!(median_ms(&[]), None);
        assert_eq!(median_ms(&[i64::MAX, i64::MAX]), Some(i64::MAX));
    }

    #[test]
    fn tiny_medians_give_no_marker() {
        let tiny = [400, 500, 600, 500, 450];
        assert_eq!(marker(&tiny, 60_000, true), None);
        assert_eq!(marker(&tiny, 0, true), None);
    }
}
