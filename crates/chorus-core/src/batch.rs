//! When to flush PTY output to an attached view (ARCHITECTURE §3, spike S1).
//!
//! Output is coalesced so a 10 MB burst arrives as a few hundred messages rather than tens of
//! thousands, while a lone keystroke echo still shows within one frame. Time is an input.

/// Flush once the oldest pending byte is `window_ms` old, or as soon as `max_bytes` are pending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchPolicy {
    pub window_ms: u64,
    pub max_bytes: usize,
}

/// Measured in S1: an 8 ms window keeps echo under one 60 Hz frame; 256 KiB caps message size.
pub const PTY_BATCH: BatchPolicy = BatchPolicy { window_ms: 8, max_bytes: 256 * 1024 };

impl BatchPolicy {
    /// `oldest_pending_ms` is how long the oldest unsent byte has waited.
    pub fn should_flush(&self, pending_bytes: usize, oldest_pending_ms: u64) -> bool {
        pending_bytes > 0 && (pending_bytes >= self.max_bytes || oldest_pending_ms >= self.window_ms)
    }

    /// How long to wait for more output before flushing what is pending.
    pub fn wait_ms(&self, oldest_pending_ms: u64) -> u64 {
        self.window_ms.saturating_sub(oldest_pending_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_pending_never_flushes() {
        assert!(!PTY_BATCH.should_flush(0, 1_000));
    }

    #[test]
    fn small_output_waits_for_the_window() {
        assert!(!PTY_BATCH.should_flush(5, 3));
        assert!(PTY_BATCH.should_flush(5, 8));
        assert_eq!(PTY_BATCH.wait_ms(3), 5);
        assert_eq!(PTY_BATCH.wait_ms(20), 0);
    }

    #[test]
    fn a_full_batch_flushes_immediately() {
        assert!(PTY_BATCH.should_flush(256 * 1024, 0));
    }
}
