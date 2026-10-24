//! P8's start-up: instruction files left by an earlier Chorus go before any session relaunches,
//! then the documents pass and the automation scheduler start on their own threads.

use std::sync::Arc;

use crate::sessions::SessionManager;

pub fn start(manager: &Arc<SessionManager>) -> Result<(), String> {
    crate::instructions::start(manager);
    crate::documents::start(manager)?;
    crate::automations::start(manager)
}
