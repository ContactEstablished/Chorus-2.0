//! The key vault: Windows DPAPI, current-user scope (ARCHITECTURE §8, spike S4).
//!
//! Keys are encrypted with `CryptProtectData` and kept only as blobs. They are decrypted at launch
//! and handed to the child as environment variables, never argv, files or logs. A fingerprint (the
//! first 16 hex digits of SHA-256) identifies a key in logs and tests without revealing it.

use std::ffi::c_void;

use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{
    BCRYPT_SHA256_ALG_HANDLE, BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom, BCryptHash, CRYPT_INTEGER_BLOB,
    CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};
use windows::core::w;

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
}
