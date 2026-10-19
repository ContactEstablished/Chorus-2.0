//! The scrollback mirror's rules (ARCHITECTURE §4; ported from Chorus 1's `scrollbackCore.ts`).
//! `src-tauri::sessions::mirror` does the file IO.
//!
//! Each session's file holds the scrubbed bytes its views were sent, capped to the newest
//! `MAX_BYTES`. A view that attaches is sent the file first, then live output, so reopening a
//! session shows what it showed before. When a new run starts in the same session (Restart, or a
//! relaunch after Chorus restarts), the old bytes are rewritten once by [`prepare_history`] so the
//! last screen of the old run stays visible above a dividing line instead of being painted over.

/// The cap on a session's mirror. Chorus 1 capped at 4,000,000 characters.
pub const MAX_BYTES: usize = 4 * 1024 * 1024;

/// A file may grow this far past the cap before it is rewritten. Rewriting on every append would
/// copy megabytes many times a second; with 25% slack it happens once per megabyte of overflow.
pub const SLACK_RATIO_PERCENT: usize = 125;

/// How far past a cut to look for a line start, so a replay doesn't begin mid escape sequence.
const LINE_SEARCH: usize = 4096;

/// Whether a file of `len` bytes is far enough over `max` to be rewritten.
pub fn exceeds_slack(len: usize, max: usize) -> bool {
    len > max.saturating_mul(SLACK_RATIO_PERCENT) / 100
}

/// The newest `max` bytes of `bytes`, starting on a UTF-8 boundary, and at the next line start if
/// one is close, so the replay never begins inside a character or (usually) an escape sequence.
pub fn cap_tail(bytes: &[u8], max: usize) -> &[u8] {
    if bytes.len() <= max {
        return bytes;
    }
    let mut start = bytes.len() - max;
    let window = &bytes[start..bytes.len().min(start + LINE_SEARCH)];
    if let Some(nl) = window.iter().position(|&b| b == b'\n') {
        start += nl + 1;
    } else {
        while start < bytes.len() && (bytes[start] & 0b1100_0000) == 0b1000_0000 {
            start += 1;
        }
    }
    &bytes[start..]
}

/// Removes only the alternate-screen switches (`ESC[?1049h/l`, `?1047`, `?47`). The alternate
/// screen has no scrollback, so a full-screen TUI's last screen, replayed verbatim, would be
/// painted somewhere that is then thrown away. Every other escape is the layout and is kept.
/// (Chorus 1 measured that stripping more breaks the screen: F58.)
pub fn strip_alt_screen(bytes: &[u8]) -> Vec<u8> {
    const SWITCHES: [&[u8]; 6] = [b"\x1b[?1049h", b"\x1b[?1049l", b"\x1b[?1047h", b"\x1b[?1047l", b"\x1b[?47h", b"\x1b[?47l"];
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b
            && let Some(s) = SWITCHES.iter().find(|s| bytes[i..].starts_with(s))
        {
            i += s.len();
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// Lines scrolled to move a screen into scrollback, at least: a replayed screen must leave the
/// viewport before the next run draws, and the view may have more rows than the PTY had.
const MIN_LIFT_LINES: usize = 60;

/// Resets the scroll region and attributes and parks the cursor on the last row: a replayed screen
/// was painted with absolute cursor moves and can leave the cursor anywhere.
const PARK: &[u8] = b"\x1b[r\x1b[m\x1b[999;1H";

/// A dividing line written into a session's stream, so it is mirrored and replayed like output.
pub fn boundary(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(PARK);
    out.extend_from_slice(b"\r\n\x1b[2m");
    out.extend_from_slice(text.as_bytes());
    out.extend_from_slice(b"\x1b[0m\r\n");
    out
}

/// The row a run starts on after [`restart_screen`]: the one under the dividing line.
pub const ROW_AFTER_RESTART_SCREEN: u16 = 2;

/// What a view shows when a new run starts in a session that already has output: leave the
/// alternate screen (a full-screen TUI's last screen would otherwise be thrown away), lift the
/// whole screen into scrollback, clear the viewport, and put the dividing line on the top row. The
/// cursor ends at row [`ROW_AFTER_RESTART_SCREEN`], column 1, which is what Chorus tells ConPTY
/// (`conpty::Handshake`).
pub fn restart_screen(boundary_text: &str, rows: u16) -> Vec<u8> {
    let lift = usize::from(rows).max(MIN_LIFT_LINES);
    let mut out = b"\x1b[?1049l".to_vec();
    out.extend_from_slice(PARK);
    out.extend_from_slice("\r\n".repeat(lift).as_bytes());
    out.extend_from_slice(b"\x1b[H\x1b[2J\x1b[2m");
    out.extend_from_slice(boundary_text.as_bytes());
    out.extend_from_slice(b"\x1b[0m\r\n");
    out
}

/// The mirror rewritten for a new run: the capped old bytes on the normal screen, then
/// [`restart_screen`]. New output is appended after this, so a replay ends in the same state as a
/// view that watched it live.
pub fn prepare_history(old: &[u8], max: usize, boundary_text: &str, rows: u16) -> Vec<u8> {
    let screen = restart_screen(boundary_text, rows);
    let mut out = strip_alt_screen(cap_tail(old, max.saturating_sub(screen.len())));
    out.extend_from_slice(&screen);
    out
}

/// The line written when a run's process ends by itself.
pub fn exit_line(exit_code: u32) -> Vec<u8> {
    boundary(&format!("── Process exited with code {exit_code} ──"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slack_allows_a_quarter_over_the_cap() {
        assert!(!exceeds_slack(100, 100));
        assert!(!exceeds_slack(125, 100));
        assert!(exceeds_slack(126, 100));
    }

    #[test]
    fn cap_keeps_the_newest_bytes_from_a_line_start() {
        assert_eq!(cap_tail(b"short", 10), b"short");
        assert_eq!(cap_tail(b"aaaa\nbbbb\ncccc", 8), b"cccc");
        // No newline near the cut: start on a UTF-8 boundary instead of inside "é".
        let s = "xé".repeat(10);
        let tail = cap_tail(s.as_bytes(), 7);
        assert!(std::str::from_utf8(tail).is_ok());
        assert!(tail.len() <= 7);
    }

    #[test]
    fn strips_only_the_alternate_screen_switches() {
        let input = b"\x1b[?1049h\x1b[2J\x1b[1;1Hhello\x1b[31m red\x1b[?1049l\x1b[?47h";
        assert_eq!(strip_alt_screen(input), b"\x1b[2J\x1b[1;1Hhello\x1b[31m red".to_vec());
        assert_eq!(strip_alt_screen(b"\x1b"), b"\x1b".to_vec());
    }

    #[test]
    fn history_fits_the_cap_and_ends_on_a_cleared_screen_with_the_boundary() {
        let old = [b"\x1b[?1049h".as_slice(), &b"line\n".repeat(50_000)].concat();
        let max = 64 * 1024;
        let prepared = prepare_history(&old, max, "── Restarted ──", 30);
        assert!(prepared.len() <= max, "{} > {max}", prepared.len());
        assert!(!prepared.windows(8).any(|w| w == b"\x1b[?1049h"));
        assert!(prepared.ends_with(&restart_screen("── Restarted ──", 30)));
        // Nothing to keep: just the screen.
        assert_eq!(prepare_history(b"", max, "x", 30), restart_screen("x", 30));
    }

    #[test]
    fn the_restart_screen_lifts_at_least_the_whole_viewport() {
        let small = String::from_utf8(restart_screen("x", 10)).unwrap();
        assert_eq!(small.matches("\r\n").count(), MIN_LIFT_LINES + 1);
        let tall = String::from_utf8(restart_screen("x", 200)).unwrap();
        assert_eq!(tall.matches("\r\n").count(), 201);
        assert!(tall.contains("\x1b[H\x1b[2J\x1b[2mx\x1b[0m\r\n"));
    }

    #[test]
    fn the_exit_line_names_the_code() {
        let line = String::from_utf8(exit_line(3)).unwrap();
        assert!(line.contains("exited with code 3"));
    }
}
