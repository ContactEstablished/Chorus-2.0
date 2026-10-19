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

/// What a view shows when a new run starts in a session that already has output. It leaves the
/// alternate screen (a full-screen TUI's last screen would otherwise be thrown away), pushes the
/// screen into scrollback, writes the dividing line and pushes that up too, so the run starts at
/// the top-left of a blank screen, where `conpty::CURSOR_REPORT` tells ConPTY the cursor is.
///
/// The dividing line can't stay on screen: ConPTY doesn't know it is there, and erases it the
/// next time it repaints the top row (seen with Claude Code, LESSONS L34).
///
/// The pushes are Erase in Display (`ESC[2J`). Chorus's views set xterm.js's
/// `scrollOnEraseInDisplay`, which makes it move the rows up to the last one with text into
/// scrollback, so the old screen's trailing blank rows aren't kept (`src/components/terminal.ts`).
pub fn restart_screen(boundary_text: &str) -> Vec<u8> {
    // The scroll region is reset first: only a full-screen scroll feeds the scrollback.
    let mut out = b"\x1b[?1049l\x1b[r\x1b[m\x1b[2J\x1b[H\x1b[2m".to_vec();
    out.extend_from_slice(boundary_text.as_bytes());
    out.extend_from_slice(b"\x1b[0m\x1b[2J\x1b[H");
    out
}

/// The mirror rewritten for a new run: the capped old bytes on the normal screen, then
/// [`restart_screen`]. New output is appended after this, so a replay ends in the same state as a
/// view that watched it live.
pub fn prepare_history(old: &[u8], max: usize, boundary_text: &str) -> Vec<u8> {
    let screen = restart_screen(boundary_text);
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
        let prepared = prepare_history(&old, max, "── Restarted ──");
        assert!(prepared.len() <= max, "{} > {max}", prepared.len());
        assert!(!prepared.windows(8).any(|w| w == b"\x1b[?1049h"));
        assert!(prepared.ends_with(&restart_screen("── Restarted ──")));
        // Nothing to keep: just the screen.
        assert_eq!(prepare_history(b"", max, "x"), restart_screen("x"));
    }

    #[test]
    fn the_restart_screen_pushes_the_screen_then_the_line_into_scrollback() {
        // The same bytes are replayed into xterm.js by src/components/terminal.test.ts.
        assert_eq!(
            restart_screen("── Restarted ──"),
            "\x1b[?1049l\x1b[r\x1b[m\x1b[2J\x1b[H\x1b[2m── Restarted ──\x1b[0m\x1b[2J\x1b[H".as_bytes()
        );
    }

    #[test]
    fn the_exit_line_names_the_code() {
        let line = String::from_utf8(exit_line(3)).unwrap();
        assert!(line.contains("exited with code 3"));
    }
}
