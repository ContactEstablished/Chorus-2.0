//! P18: Chorus's pairing with OpenMausBot (spikes/P18.md §4). Matthew makes a pairing code in
//! MausBot; Chorus redeems it and keeps the token as a DPAPI credential labelled `MausBot`, read
//! at use and never shown again. What the token may do (its scopes and expiry) is plain settings.

use chorus_core::maus::{self, CREDENTIAL_LABEL};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::maus_client::{self as client, Failure, PairingFacts};
use super::refresh::Scrubber;
use crate::sessions::SessionManager;
use crate::store::{self, credentials, settings};
use crate::vault;

const FACTS_KEY: &str = "mausbot.pairing";

/// The stored token, decrypted for one call. `None` when Chorus isn't paired.
pub fn token(m: &SessionManager) -> Result<Option<String>, String> {
    let Some(row) = credentials::list(m.store())?.into_iter().find(|c| c.label == CREDENTIAL_LABEL) else { return Ok(None) };
    vault::decrypt(m.store(), row.id).map(Some)
}

fn facts(m: &SessionManager) -> PairingFacts {
    settings::get(m.store(), FACTS_KEY).ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// What Settings → MausBot shows.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MausStatus {
    pub running: bool,
    pub version: Option<String>,
    pub problem: Option<String>,
    pub paired: bool,
    pub scopes: Vec<String>,
    /// Whether the token may pin a conversation to a folder (the admin scope): without it, each
    /// hand-off starts by hand.
    pub can_choose_folder: bool,
    pub expires_at_ms: Option<f64>,
    pub token_problem: Option<String>,
}

pub fn status(m: &SessionManager) -> Result<MausStatus, String> {
    let s = Scrubber::new(m)?;
    let (running, version, problem) = match client::health() {
        Ok(v) => (true, Some(v), None),
        Err(Failure::NotRunning) => (false, None, None),
        Err(f) => (false, None, Some(s.clean(&f.text()))),
    };
    let token = token(m);
    let paired = matches!(token, Ok(Some(_)));
    let mut token_problem = token.as_ref().err().map(|e| format!("Chorus can't read its MausBot key: {e}"));
    let mut facts = facts(m);
    if running && let Ok(Some(t)) = &token {
        match client::check(t) {
            Ok(fresh) if !fresh.scopes.is_empty() => facts = fresh,
            Ok(_) => {}
            Err(Failure::Refused(e)) => token_problem = Some(s.clean(&format!("MausBot no longer accepts Chorus's key ({e}). Pair again."))),
            Err(_) => {}
        }
    }
    Ok(MausStatus {
        running,
        version,
        problem,
        paired,
        can_choose_folder: paired && facts.scopes.iter().any(|x| x == "admin"),
        scopes: if paired { facts.scopes } else { Vec::new() },
        expires_at_ms: if paired { facts.expires_at_ms.map(|v| v as f64) } else { None },
        token_problem,
    })
}

/// Pairs with a code from MausBot, or the link it copies, or takes an `omb_sess_` token, and keeps
/// the token. Only the code (or the link's code or credential) is sent to MausBot.
pub fn pair(m: &SessionManager, input: &str) -> Result<MausStatus, String> {
    let input = input.trim();
    let s = Scrubber::new(m)?;
    let (token, facts) = if maus::is_token(input) {
        let facts = client::check(input).map_err(|f| s.clean(&f.text()))?;
        (input.to_owned(), facts)
    } else if let Some(code) = maus::pairing_secret(input) {
        client::pair(&code).map_err(|f| s.clean(&f.text()))?
    } else {
        return Err(
            "That isn't a MausBot pairing code or link. In MausBot, open Settings → Phone & devices, use Pair a computer, then paste its link or type its code (like 7KQ2-M9XD-H4TP)."
                .into(),
        );
    };
    // The vault's labels are unique and can't be renamed, so the old key goes first. Storing the
    // new one only fails if the vault itself does, and then this says so.
    forget_token(m)?;
    vault::add(m.store(), CREDENTIAL_LABEL, &token)
        .map_err(|e| format!("Chorus couldn't keep the new MausBot key ({e}), so it isn't paired now. Pair again."))?;
    settings::set(m.store(), FACTS_KEY, &serde_json::to_string(&facts).map_err(|e| e.to_string())?)?;
    status(m)
}

fn forget_token(m: &SessionManager) -> Result<(), String> {
    for row in credentials::list(m.store())?.into_iter().filter(|c| c.label == CREDENTIAL_LABEL) {
        credentials::delete(m.store(), row.id)?;
    }
    Ok(())
}

/// Deletes the token and what was known about it. MausBot still lists the device until it's
/// signed out there.
pub fn forget(m: &SessionManager) -> Result<MausStatus, String> {
    forget_token(m)?;
    settings::delete(m.store(), FACTS_KEY)?;
    status(m)
}

/// One bot, for the Hand to MausBot picker.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BotView {
    pub id: String,
    pub name: String,
    pub title: Option<String>,
    pub busy: bool,
    pub folder_matches: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BotsView {
    pub running: bool,
    pub problem: Option<String>,
    pub bots: Vec<BotView>,
    pub default_id: Option<String>,
    pub paired: bool,
    pub can_choose_folder: bool,
}

pub fn bots(m: &SessionManager, project_id: u32) -> Result<BotsView, String> {
    let s = Scrubber::new(m)?;
    let folder = store::projects::get(m.store(), project_id)?.and_then(|p| p.folder).unwrap_or_default();
    let paired = matches!(token(m), Ok(Some(_)));
    let can_choose_folder = paired && facts(m).scopes.iter().any(|x| x == "admin");
    match client::bots() {
        Ok(list) => {
            let last = store::handoff::last_bot(m.store(), project_id)?;
            Ok(BotsView {
                running: true,
                problem: None,
                default_id: maus::default_bot(&list, &folder, last.as_deref()),
                bots: list
                    .iter()
                    .map(|b| BotView {
                        id: b.id.clone(),
                        name: s.clean(&b.name),
                        title: b.title.as_deref().map(|t| s.clean(t)),
                        busy: b.busy,
                        folder_matches: b.cwd.as_deref().is_some_and(|c| chorus_core::projects::same_folder(c, &folder)),
                    })
                    .collect(),
                paired,
                can_choose_folder,
            })
        }
        Err(f) => Ok(BotsView {
            running: !matches!(f, Failure::NotRunning),
            problem: Some(s.clean(&f.text())),
            bots: Vec::new(),
            default_id: None,
            paired,
            can_choose_folder,
        }),
    }
}
