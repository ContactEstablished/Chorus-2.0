//! Agent events (PLAN P3, ARCHITECTURE §5): the loopback listener hooks post to, and the recorder
//! that turns a run's events into state and history.

pub mod listener;
pub mod recorder;

/// Why a hook post was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookRefused {
    /// No running session with that id, or its run has no hook token.
    NotFound,
    Unauthorized,
    BadBody,
}

/// Compares two tokens in time that doesn't depend on where they differ.
pub fn same_token(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    #[test]
    fn tokens_compare_whole() {
        assert!(super::same_token("abc", "abc"));
        assert!(!super::same_token("abc", "abd") && !super::same_token("abc", "ab") && !super::same_token("", "a"));
    }
}
