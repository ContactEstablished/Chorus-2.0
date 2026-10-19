//! The key vault: Windows DPAPI, current-user scope (ARCHITECTURE §8, spike S4).
//!
//! Keys are encrypted with `CryptProtectData` and stored only as blobs in SQLite
//! (`store::credentials`). They are decrypted at launch and handed to the child as environment
//! variables, never argv, files or logs. A fingerprint (the first 16 hex digits of SHA-256)
//! identifies a key in the UI and in tests without revealing it. After saving, a key never comes
//! back over IPC.

pub mod commands;

use std::ffi::c_void;

use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{
    BCRYPT_HMAC_SHA256_ALG_HANDLE, BCRYPT_SHA256_ALG_HANDLE, BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptDeriveKeyPBKDF2,
    BCryptGenRandom, BCryptHash, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};
use windows::core::w;

use crate::store::{Store, credentials};

/// Bounds for what the vault accepts, checked at the IPC boundary.
pub const MAX_LABEL_CHARS: usize = 64;
pub const MAX_SECRET_BYTES: usize = 8192;

/// Ties blobs to Chorus 2.0: another app of the same user must know this to decrypt them.
const ENTROPY: &[u8] = b"chorus2.vault.v1";

fn blob(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB { cbData: bytes.len() as u32, pbData: bytes.as_ptr().cast_mut() }
}

/// Takes ownership of a DPAPI output buffer, copies it out and frees it.
///
/// SAFETY: `out` must have been filled by a successful DPAPI call.
unsafe fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    // SAFETY: DPAPI returned `cbData` valid bytes at `pbData`, allocated with LocalAlloc.
    unsafe {
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(Some(HLOCAL(out.pbData.cast::<c_void>())));
        v
    }
}

pub fn protect(plain: &[u8]) -> windows::core::Result<Vec<u8>> {
    let input = blob(plain);
    let entropy = blob(ENTROPY);
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY: input and entropy outlive the call; `out` is freed by `take`.
    unsafe {
        CryptProtectData(&input, w!("chorus2"), Some(&entropy), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)?;
        Ok(take(out))
    }
}

pub fn unprotect(sealed: &[u8]) -> windows::core::Result<Vec<u8>> {
    let input = blob(sealed);
    let entropy = blob(ENTROPY);
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY: as in `protect`.
    unsafe {
        CryptUnprotectData(&input, None, Some(&entropy), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)?;
        Ok(take(out))
    }
}

/// First 16 hex digits of SHA-256: safe to show, and enough to tell keys apart.
pub fn fingerprint(bytes: &[u8]) -> String {
    let mut digest = [0u8; 32];
    // SAFETY: the pseudo-handle needs no open/close; buffers are correctly sized.
    let status = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, bytes, &mut digest) };
    assert!(status.is_ok(), "BCryptHash failed: {status:?}");
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// Cryptographically random bytes from the OS.
pub fn random_bytes(n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    // SAFETY: the system RNG needs no algorithm handle with this flag.
    let status = unsafe { BCryptGenRandom(None, &mut buf, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    assert!(status.is_ok(), "BCryptGenRandom failed: {status:?}");
    buf
}

/// PBKDF2-HMAC-SHA256 through Windows CNG, for the agent lock's PIN (`chorus_core::lock`).
pub fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32, out: &mut [u8]) -> Result<(), String> {
    // SAFETY: the HMAC-SHA256 pseudo-handle needs no open/close; slices are valid for the call.
    let status = unsafe {
        BCryptDeriveKeyPBKDF2(BCRYPT_HMAC_SHA256_ALG_HANDLE, Some(password), Some(salt), u64::from(iterations), out, 0)
    };
    if status.is_ok() { Ok(()) } else { Err(format!("deriving the PIN hash failed: {status:?}")) }
}

/// Checks a label and a pasted key, returning the key trimmed of surrounding whitespace (a pasted
/// key often brings a newline).
pub fn check_new_key<'a>(label: &str, secret: &'a str) -> Result<&'a str, String> {
    let label_chars = label.trim().chars().count();
    if label_chars == 0 || label_chars > MAX_LABEL_CHARS || label.chars().any(char::is_control) {
        return Err(format!("A key's label needs 1 to {MAX_LABEL_CHARS} characters, with no control characters."));
    }
    let secret = secret.trim();
    if secret.is_empty() || secret.len() > MAX_SECRET_BYTES {
        return Err("The key is empty or too long.".into());
    }
    if secret.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("The key contains spaces or control characters; paste just the key.".into());
    }
    Ok(secret)
}

/// Seals and stores a key. Returns its row id.
pub fn add(store: &Store, label: &str, secret: &str) -> Result<u32, String> {
    let secret = check_new_key(label, secret)?;
    let sealed = protect(secret.as_bytes()).map_err(|e| format!("encrypting the key: {e}"))?;
    credentials::insert(store, label.trim(), &sealed, &fingerprint(secret.as_bytes()))
}

/// Decrypts one stored key for a launch.
pub fn decrypt(store: &Store, id: u32) -> Result<String, String> {
    let sealed = credentials::blob(store, id)?.ok_or("The key this session uses has been deleted.")?;
    let plain = unprotect(&sealed).map_err(|e| format!("decrypting a stored key: {e}"))?;
    String::from_utf8(plain).map_err(|_| "a stored key is not text".into())
}

/// Every stored key in plaintext, for a session's scrubber, plus how many couldn't be decrypted
/// (shown in the session's note: fail visible).
pub fn all_for_scrubbing(store: &Store) -> (Vec<Vec<u8>>, usize) {
    let Ok(blobs) = credentials::all_blobs(store) else { return (Vec::new(), 0) };
    let mut failed = 0;
    let keys = blobs
        .into_iter()
        .filter_map(|(_, sealed)| {
            unprotect(&sealed).map_err(|_| failed += 1).ok()
        })
        .collect();
    (keys, failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_the_blob_does_not_contain_the_plaintext() {
        let plain = b"not-a-real-key-0123456789abcdef".to_vec();
        let sealed = protect(&plain).expect("protect");
        assert!(sealed.len() > plain.len());
        assert!(!sealed.windows(plain.len()).any(|w| w == plain.as_slice()));
        assert_eq!(unprotect(&sealed).expect("unprotect"), plain);
    }

    #[test]
    fn a_tampered_blob_is_refused() {
        let mut sealed = protect(b"value").expect("protect");
        let last = sealed.len() - 1;
        sealed[last] ^= 0xff;
        assert!(unprotect(&sealed).is_err());
    }

    #[test]
    fn fingerprint_is_sha256_prefix() {
        // SHA-256("abc") = ba7816bf8f01cfea…
        assert_eq!(fingerprint(b"abc"), "ba7816bf8f01cfea");
    }

    #[test]
    fn pbkdf2_matches_the_rfc_7914_vector() {
        // RFC 7914 §11: PBKDF2-HMAC-SHA256("passwd", "salt", 1, 64) begins 55 ac 04 6e.
        let mut out = [0u8; 64];
        pbkdf2_sha256(b"passwd", b"salt", 1, &mut out).unwrap();
        assert_eq!(&out[..4], &[0x55, 0xac, 0x04, 0x6e]);
    }

    #[test]
    fn keys_are_stored_sealed_and_come_back_whole() {
        let store = Store::in_memory();
        let id = add(&store, " Test ", "  not-a-real-key-0123456789abcdef\n").unwrap();
        assert_eq!(decrypt(&store, id).unwrap(), "not-a-real-key-0123456789abcdef");
        let row = credentials::get(&store, id).unwrap().unwrap();
        assert_eq!(row.label, "Test");
        assert_eq!(row.fingerprint, fingerprint(b"not-a-real-key-0123456789abcdef"));
        let blob = credentials::blob(&store, id).unwrap().unwrap();
        assert!(!blob.windows(8).any(|w| w == b"not-a-re"));
        let (keys, failed) = all_for_scrubbing(&store);
        assert_eq!((keys.len(), failed), (1, 0));
        assert!(add(&store, "x", "has space").is_err());
        assert!(add(&store, "", "abc").is_err());
    }
}
