//! A capped OpenRouter key per run (ARCHITECTURE §8). It is minted before the launch with the
//! provider's management key, lives only in the child's environment, is revoked when the run
//! ends, and its cost is read from OpenRouter's analytics after revoking: reading usage before
//! revoking misses the last turn (LESSONS L16). It also expires by itself within a day, and
//! start-up revokes any a previous Chorus left live.

use std::time::Duration;

use chorus_core::providers::{self as p, ProviderKind};

use crate::store::Store;
use crate::store::providers::{self as rows, ProviderRow};
use crate::vault;

/// After revoking, how often and how many times to ask analytics for the cost.
const COST_POLLS: u32 = 6;
const COST_POLL_GAP: Duration = Duration::from_secs(5);

/// A key minted for one run.
pub struct Minted {
    pub ledger_id: u32,
    pub key: String,
    pub limit_usd: f64,
}

/// Mints a key for a session's next run. A ledger row is written first, so a crash after minting
/// still leaves a record for the start-up sweep.
pub fn mint_for_run(store: &Store, provider: &ProviderRow, session_id: u32) -> Result<Minted, String> {
    let management_id = provider.management_credential_id.ok_or("the provider has no management key")?;
    let management = vault::decrypt(store, management_id)?;
    let limit_usd = provider.cap_usd.unwrap_or(p::DEFAULT_CAP_USD);
    let ledger_id = rows::open_capped_key(store, provider.id, session_id, limit_usd)?;
    match super::mint(&management, ledger_id, limit_usd) {
        Ok(minted) => {
            rows::set_capped_hash(store, ledger_id, &minted.hash)?;
            Ok(Minted { ledger_id, key: minted.key, limit_usd })
        }
        Err(e) => {
            let _ = rows::close_capped_key(store, ledger_id, Some(&format!("Not minted: {e}")));
            Err(e)
        }
    }
}

fn management_for(store: &Store, provider_id: Option<u32>) -> Result<String, String> {
    let provider = rows::get(store, provider_id.ok_or("no provider")?)?.ok_or("the provider was deleted")?;
    vault::decrypt(store, provider.management_credential_id.ok_or("the provider has no management key")?)
}

/// Revokes a run's key at once, then reads what it cost. Blocks for up to half a minute, so
/// callers run it on a thread of its own.
pub fn settle(store: &Store, ledger_id: u32) {
    let Ok(Some(row)) = rows::capped_key(store, ledger_id) else { return };
    let Some(hash) = row.hash.clone() else {
        let _ = rows::close_capped_key(store, ledger_id, Some("Never minted."));
        return;
    };
    let management = match management_for(store, row.provider_id) {
        Ok(m) => m,
        Err(e) => {
            let _ = rows::set_capped_cost(store, ledger_id, None, &format!("Couldn't revoke ({e}); it expires by itself within a day."));
            return;
        }
    };
    if let Err(e) = super::revoke(&management, &hash) {
        // Left open, so the next start-up tries again.
        let _ = rows::set_capped_cost(store, ledger_id, None, &format!("Couldn't revoke ({e}); it expires by itself within a day."));
        return;
    }
    let _ = rows::close_capped_key(store, ledger_id, None);
    let mut best: Option<f64> = None;
    for attempt in 0..COST_POLLS {
        if attempt > 0 {
            std::thread::sleep(COST_POLL_GAP);
        }
        if let Ok(Some(cost)) = super::key_cost(&management, &hash, row.minted_at - 60_000, crate::store::now_ms() + 60_000) {
            best = Some(best.map_or(cost, |b: f64| b.max(cost)));
        }
    }
    let note = match best {
        Some(_) => "From OpenRouter's analytics; it can still grow for a few minutes.",
        None => "OpenRouter's analytics had no cost for it yet.",
    };
    let _ = rows::set_capped_cost(store, ledger_id, best, note);
}

/// Revokes a run's key without waiting for its cost (when Chorus quits). The cost is left for
/// later: the ledger keeps the hash.
pub fn revoke_now(store: &Store, ledger_id: u32) {
    let Ok(Some(row)) = rows::capped_key(store, ledger_id) else { return };
    let (Some(hash), Ok(management)) = (row.hash, management_for(store, row.provider_id)) else { return };
    if super::revoke(&management, &hash).is_ok() {
        let _ = rows::close_capped_key(store, ledger_id, Some("Revoked when Chorus quit; cost not read."));
    }
}

/// At start-up, before any session relaunches: revokes every key Chorus 2.0 minted (by name) with
/// a ledger id up to `up_to`, and closes the ledger rows they belong to. Keys not named by Chorus
/// 2.0 are never touched.
pub fn sweep(store: &Store, up_to: u32) {
    let Ok(providers) = rows::list(store) else { return };
    for provider in providers.iter().filter(|p| p.kind == ProviderKind::Openrouter) {
        let Some(management) = provider.management_credential_id.and_then(|id| vault::decrypt(store, id).ok()) else {
            continue;
        };
        let Ok(live) = super::list_keys(&management) else { continue };
        let mut still_live: Vec<u32> = Vec::new();
        for (hash, name) in live {
            let Some(ledger) = p::capped_key_ledger(&name).filter(|l| *l <= up_to) else { continue };
            if super::revoke(&management, &hash).is_ok() {
                let _ = rows::close_capped_key(store, ledger, Some("Revoked at start-up: the run that used it had ended."));
            } else {
                still_live.push(ledger);
            }
        }
        // This provider's rows still open whose key it no longer lists: revoked or expired.
        let Ok(open) = rows::open_capped_keys(store) else { continue };
        for row in open.into_iter().filter(|r| r.id <= up_to && r.provider_id == Some(provider.id) && !still_live.contains(&r.id)) {
            let note = if row.hash.is_none() { "Never minted." } else { "Gone at OpenRouter by start-up; cost unknown." };
            let _ = rows::close_capped_key(store, row.id, Some(note));
        }
    }
}
