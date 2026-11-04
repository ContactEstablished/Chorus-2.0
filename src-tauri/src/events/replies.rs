//! In-memory lifetime of one held HTTP request. Dropping/expiring it restores the CLI's own flow.
use std::time::{Duration, Instant};
use chorus_core::replies::{HookRequest, REPLY_WAIT_SECS, ReplyDetails};
use serde::Serialize;
use serde_json::Value;
use specta::Type;
use tokio::sync::oneshot;

pub type HookReceiver = oneshot::Receiver<Value>;
pub struct HeldReply {
    pub request: HookRequest,
    pub sender: Option<oneshot::Sender<Value>>,
    pub opened: Instant,
}

impl HeldReply {
    pub fn available(&self) -> bool {
        self.opened.elapsed() < Duration::from_secs(REPLY_WAIT_SECS) && self.sender.as_ref().is_some_and(|s| !s.is_closed())
    }

    /// A Claude Code question whose hold ended unanswered: its own question is now showing in Raw,
    /// so Friendly can still answer it as a message (D112). Never an approval, and never after an
    /// attempt (any reply or release takes the sender).
    pub fn late_answer(&self) -> bool {
        matches!(self.request.details, ReplyDetails::Question { .. }) && self.request.protocol.is_none() && self.sender.is_some() && !self.available()
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PendingReply {
    pub ask_id: u32,
    pub run_id: u32,
    pub details: ReplyDetails,
    pub denial_reason_supported: bool,
    pub remaining_secs: u32,
    pub blocked_reason: Option<String>,
    /// The hold ended, but the answers can still reach the agent as a message (D112).
    pub late_answer: bool,
}
