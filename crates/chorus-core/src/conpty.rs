//! ConPTY's opening handshake.
//!
//! portable-pty creates the pseudoconsole with `PSEUDOCONSOLE_INHERIT_CURSOR`, so the first thing
//! ConPTY writes is a cursor-position request (`ESC[6n`), and it draws nothing until the terminal
//! answers. Sessions belong to the back end and often start with no view attached (a new session
//! before its window attaches, or every session relaunched at boot), so Chorus answers it itself
//! and takes the request out of the stream; otherwise an attached xterm.js would answer a second
//! time and the extra reply would arrive as typed input.
//!
//! Chorus knows where the cursor is because every run starts on a screen it prepared: the top-left
//! of a fresh terminal for a new session, or the row under the dividing line that
//! `scrollback::restart_screen` draws for a Restart or a relaunch.

pub const CURSOR_REQUEST: &[u8] = b"\x1b[6n";

/// How far into a run's output to look for the request before giving up on it.
const LOOK_LIMIT: usize = 4096;

/// The reply to the request: the cursor is at `row`, column 1 (both 1-based).
pub fn cursor_report(row: u16) -> Vec<u8> {
    format!("\x1b[{row};1R").into_bytes()
}

/// Finds and removes the opening cursor request in a run's output.
#[derive(Debug, Default)]
pub struct Handshake {
    done: bool,
    held: Vec<u8>,
    seen: usize,
}

impl Handshake {
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes the next chunk of output. Returns what to pass on and whether the request was just
    /// seen (answer it now). A trailing piece that could be the start of the request is held.
    pub fn push(&mut self, chunk: &[u8]) -> (Vec<u8>, bool) {
        if self.done {
            return (chunk.to_vec(), false);
        }
        let mut work = std::mem::take(&mut self.held);
        work.extend_from_slice(chunk);
        if let Some(i) = work.windows(CURSOR_REQUEST.len()).position(|w| w == CURSOR_REQUEST) {
            self.done = true;
            let mut out = work[..i].to_vec();
            out.extend_from_slice(&work[i + CURSOR_REQUEST.len()..]);
            return (out, true);
        }
        self.seen += work.len();
        if self.seen > LOOK_LIMIT {
            self.done = true;
            return (work, false);
        }
        let keep = (1..CURSOR_REQUEST.len()).rev().find(|&n| work.ends_with(&CURSOR_REQUEST[..n])).unwrap_or(0);
        self.held = work.split_off(work.len() - keep);
        self.seen -= keep;
        (work, false)
    }

    /// Whatever is still held, when the run's output ends.
    pub fn flush(&mut self) -> Vec<u8> {
        self.done = true;
        std::mem::take(&mut self.held)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_opening_request_is_removed_and_answered_once() {
        let mut h = Handshake::new();
        assert_eq!(h.push(b"\x1b[6n"), (Vec::new(), true));
        // Later requests pass through: by then a terminal may be attached to answer.
        assert_eq!(h.push(b"a\x1b[6nb"), (b"a\x1b[6nb".to_vec(), false));
    }

    #[test]
    fn a_request_split_across_chunks_is_still_found() {
        let mut h = Handshake::new();
        assert_eq!(h.push(b"x\x1b["), (b"x".to_vec(), false));
        assert_eq!(h.push(b"6ny"), (b"y".to_vec(), true));
    }

    #[test]
    fn output_without_a_request_passes_and_the_search_gives_up() {
        let mut h = Handshake::new();
        let big = vec![b'z'; LOOK_LIMIT + 1];
        assert_eq!(h.push(&big), (big.clone(), false));
        assert_eq!(h.push(b"\x1b[6n"), (b"\x1b[6n".to_vec(), false));
        let mut h = Handshake::new();
        assert_eq!(h.push(b"ab\x1b"), (b"ab".to_vec(), false));
        assert_eq!(h.flush(), b"\x1b".to_vec());
    }

    #[test]
    fn the_reply_names_the_row() {
        assert_eq!(cursor_report(2), b"\x1b[2;1R".to_vec());
    }
}
