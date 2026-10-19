//! The agent lock's PIN (rules and the full argument in `chorus_core::lock`). A locked session
//! refuses Close and Restart; unlocking needs the PIN when one is set. Setting or clearing the PIN
//! needs no old PIN: it is an accident guard, not access control.

use std::sync::Arc;

use chorus_core::lock::{HASH_BYTES, PBKDF2_ITERATIONS, PinHash, SALT_BYTES, constant_time_eq, validate_pin};
use tauri::State;

use crate::sessions::SessionManager;
use crate::store::{Store, settings};
use crate::vault::{pbkdf2_sha256, random_bytes};

const PIN_KEY: &str = "lock.pin";

pub fn pin_is_set(store: &Store) -> Result<bool, String> {
    Ok(settings::get(store, PIN_KEY)?.is_some())
}

pub fn set_pin(store: &Store, pin: &str) -> Result<(), String> {
    set_pin_with(store, pin, PBKDF2_ITERATIONS)
}

fn set_pin_with(store: &Store, pin: &str, iterations: u32) -> Result<(), String> {
    validate_pin(pin).map_err(|r| r.message())?;
    let salt = random_bytes(SALT_BYTES);
    let mut hash = vec![0u8; HASH_BYTES];
    pbkdf2_sha256(pin.as_bytes(), &salt, iterations, &mut hash)?;
    settings::set(store, PIN_KEY, &PinHash { iterations, salt, hash }.format())
}

pub fn clear_pin(store: &Store) -> Result<(), String> {
    settings::delete(store, PIN_KEY)
}

/// Whether `pin` unlocks. With no PIN set, anything does. A stored value that can't be parsed
/// confirms nothing, so it refuses (and the PIN can be cleared and set again in Settings).
pub fn check_unlock(store: &Store, pin: Option<&str>) -> Result<(), String> {
    let Some(stored) = settings::get(store, PIN_KEY)? else { return Ok(()) };
    let pin = pin.ok_or("Enter the PIN to unlock.")?;
    let ok = PinHash::parse(&stored).is_some_and(|h| {
        let mut actual = vec![0u8; h.hash.len()];
        pbkdf2_sha256(pin.as_bytes(), &h.salt, h.iterations, &mut actual).is_ok() && constant_time_eq(&actual, &h.hash)
    });
    if ok { Ok(()) } else { Err("That PIN is not right.".into()) }
}

#[tauri::command]
#[specta::specta]
pub fn lock_pin_status(state: State<'_, Arc<SessionManager>>) -> Result<bool, String> {
    pin_is_set(state.store())
}

#[tauri::command]
#[specta::specta]
pub async fn lock_pin_set(state: State<'_, Arc<SessionManager>>, pin: String) -> Result<(), String> {
    let m = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || set_pin(m.store(), &pin)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
#[specta::specta]
pub fn lock_pin_clear(state: State<'_, Arc<SessionManager>>) -> Result<(), String> {
    clear_pin(state.store())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlocking_needs_the_right_pin_only_when_one_is_set() {
        let store = Store::in_memory();
        assert!(check_unlock(&store, None).is_ok());
        set_pin_with(&store, "2468", 1000).unwrap();
        assert!(pin_is_set(&store).unwrap());
        assert!(check_unlock(&store, None).is_err());
        assert!(check_unlock(&store, Some("1357")).is_err());
        assert!(check_unlock(&store, Some("2468")).is_ok());
        let stored = settings::get(&store, PIN_KEY).unwrap().unwrap();
        assert!(!stored.contains("2468"));
        assert!(set_pin_with(&store, "12 34", 1000).is_err());
        clear_pin(&store).unwrap();
        assert!(check_unlock(&store, None).is_ok());
    }

    #[test]
    fn a_corrupt_stored_pin_refuses() {
        let store = Store::in_memory();
        settings::set(&store, PIN_KEY, "garbage").unwrap();
        assert!(check_unlock(&store, Some("anything")).is_err());
    }
}
