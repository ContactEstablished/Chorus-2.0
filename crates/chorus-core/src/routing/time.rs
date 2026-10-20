//! UTC instants as Unix milliseconds. Chorus 1 passed ISO strings and read them with
//! `Date.parse`; 2.0 takes `now_ms` and stores `i64` milliseconds, so ISO text is parsed only
//! where it arrives as text (the bundled registry's verification records).

pub const MINUTE_MS: i64 = 60_000;
pub const DAY_MS: i64 = 86_400_000;

fn digits(s: &[u8]) -> Option<i64> {
    if s.is_empty() || !s.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(s.iter().fold(0, |n, d| n * 10 + i64::from(d - b'0')))
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Howard Hinnant's `days_from_civil`: days since 1970-01-01.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `YYYY-MM-DDTHH:MM[:SS[.fraction]]Z` as Unix milliseconds: exactly the strings Chorus 1's
/// `z.iso.datetime()` accepts (a real calendar date, hour 00–23, no offset), read as
/// `Date.parse` reads them (a fraction beyond milliseconds is dropped). `None` otherwise.
pub fn parse_utc_instant(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let body = b.strip_suffix(b"Z")?;
    if body.len() < 16 || body[4] != b'-' || body[7] != b'-' || body[10] != b'T' || body[13] != b':' {
        return None;
    }
    let (year, month, day) = (digits(&body[0..4])?, digits(&body[5..7])?, digits(&body[8..10])?);
    let (hour, minute) = (digits(&body[11..13])?, digits(&body[14..16])?);
    let mut second = 0;
    let mut millis = 0;
    let rest = &body[16..];
    if !rest.is_empty() {
        if rest.len() < 3 || rest[0] != b':' {
            return None;
        }
        second = digits(&rest[1..3])?;
        let fraction = &rest[3..];
        if !fraction.is_empty() {
            let f = fraction.strip_prefix(b".")?;
            digits(f)?;
            millis = f.iter().take(3).chain(b"000".iter()).take(3).fold(0, |n, d| n * 10 + i64::from(d - b'0'));
        }
    }
    let month_days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => return None,
    };
    if !(1..=month_days).contains(&day) || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(days * DAY_MS + ((hour * 60 + minute) * 60 + second) * 1000 + millis)
}

/// The UTC weekday of an instant, 0 for Sunday (JavaScript's `getUTCDay`).
pub fn utc_weekday(ms: i64) -> u32 {
    // 1970-01-01 was a Thursday.
    (ms.div_euclid(DAY_MS) + 4).rem_euclid(7) as u32
}

/// The UTC time of day as HHMM, seconds dropped (`getUTCHours() * 100 + getUTCMinutes()`).
pub fn utc_hhmm(ms: i64) -> u32 {
    let minutes = ms.rem_euclid(DAY_MS) / MINUTE_MS;
    (minutes / 60 * 100 + minutes % 60) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instants_parse_as_date_parse_reads_them() {
        assert_eq!(parse_utc_instant("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_utc_instant("2026-10-02T09:20:00Z"), Some(1_790_932_800_000));
        assert_eq!(parse_utc_instant("2026-10-02T09:20:00.000Z"), Some(1_790_932_800_000));
        assert_eq!(parse_utc_instant("2026-10-02T09:20Z"), Some(1_790_932_800_000), "seconds are optional");
        assert_eq!(parse_utc_instant("2026-10-02T10:05:00.001Z"), Some(1_790_935_500_001));
        assert_eq!(parse_utc_instant("2026-10-02T10:05:00.0019Z"), Some(1_790_935_500_001));
        assert_eq!(parse_utc_instant("2026-10-02T10:05:00.5Z"), Some(1_790_935_500_500));
        assert_eq!(parse_utc_instant("1969-12-31T23:59:59Z"), Some(-1000));
        assert_eq!(parse_utc_instant("2024-02-29T00:00:00Z").map(|m| m % DAY_MS), Some(0));
    }

    #[test]
    fn only_what_z_iso_datetime_accepts_parses() {
        for bad in [
            "not a date",
            "",
            "2026-10-02",
            "2026-10-02T09:20:00",
            "2026-10-02T09:20:00+00:00",
            "2026-10-02T09:05:00+01:00",
            "2026-10-02 09:20",
            "2026-10-02T24:00:00Z",
            "2026-10-02T09:60:00Z",
            "2026-10-02T09:20:60Z",
            "2026-02-30T00:00:00Z",
            "2025-02-29T00:00:00Z",
            "2100-02-29T00:00:00Z",
            "2026-13-01T00:00:00Z",
            "2026-10-02T09:20:00.Z",
            "2026-10-02T09:20.5Z",
            "2026-10-02t09:20:00Z",
            "2026-10-02T09:20:00z",
        ] {
            assert_eq!(parse_utc_instant(bad), None, "{bad}");
        }
        assert!(parse_utc_instant("2000-02-29T00:00:00Z").is_some());
    }

    #[test]
    fn weekday_and_time_of_day_are_utc() {
        let friday = parse_utc_instant("2026-10-02T23:59:59Z").unwrap();
        assert_eq!((utc_weekday(friday), utc_hhmm(friday)), (5, 2359));
        let saturday = parse_utc_instant("2026-10-03T00:00:00Z").unwrap();
        assert_eq!((utc_weekday(saturday), utc_hhmm(saturday)), (6, 0));
        assert_eq!(utc_weekday(0), 4, "1970-01-01 was a Thursday");
        assert_eq!(utc_weekday(-1), 3);
        assert_eq!(utc_hhmm(-60_000), 2359);
    }
}
