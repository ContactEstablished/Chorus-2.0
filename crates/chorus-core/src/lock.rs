//! The agent lock's PIN (ported from Chorus 1's `agentLockCore.ts`).
//!
//! The lock is an **accident guard**: a locked session refuses Close and Restart, so a stray click
//! can't end an agent that has been working for an hour. It is not access control. The PIN can be
//! set and cleared without knowing the old one (Matthew's product decision in Chorus 1), so anyone
//! at the keyboard can remove it; it only makes unlocking deliberate.
//!
//! The PIN is still stored as a slow salted hash, because people reuse PINs (a phone or bank PIN
//! here would otherwise be one table lookup away for anything that can read the database). The
//! key derivation itself (PBKDF2-HMAC-SHA256 through Windows CNG) is in `src-tauri`; this module
//! validates PINs and parses, formats and compares the stored form.

use std::fmt::Write as _;

pub const PIN_MIN_CHARS: usize = 4;
pub const PIN_MAX_CHARS: usize = 64;

/// PBKDF2-HMAC-SHA256 iterations for new PINs (OWASP's 2023 recommendation).
pub const PBKDF2_ITERATIONS: u32 = 600_000;
pub const SALT_BYTES: usize = 16;
pub const HASH_BYTES: usize = 32;
const SCHEME: &str = "pbkdf2-sha256";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinRefusal {
    TooShort,
    TooLong,
    /// Refused rather than trimmed: a PIN is write-only, so a trim on one side and not the other
    /// could never be debugged by the person typing it.
    HasWhitespace,
}

impl PinRefusal {
    pub fn message(self) -> String {
        match self {
            Self::TooShort => format!("A PIN needs at least {PIN_MIN_CHARS} characters."),
            Self::TooLong => format!("A PIN can have at most {PIN_MAX_CHARS} characters."),
            Self::HasWhitespace => "A PIN cannot contain spaces.".into(),
        }
    }
}

pub fn validate_pin(pin: &str) -> Result<(), PinRefusal> {
    if pin.chars().any(char::is_whitespace) {
        return Err(PinRefusal::HasWhitespace);
    }
    let n = pin.chars().count();
    if n < PIN_MIN_CHARS {
        return Err(PinRefusal::TooShort);
    }
    if n > PIN_MAX_CHARS {
        return Err(PinRefusal::TooLong);
    }
    Ok(())
}

/// The stored form: `pbkdf2-sha256$<iterations>$<salt hex>$<hash hex>`. Self-describing, so a
/// later change of parameters is detected on read instead of mis-verifying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinHash {
    pub iterations: u32,
    pub salt: Vec<u8>,
    pub hash: Vec<u8>,
}

impl PinHash {
    pub fn format(&self) -> String {
        format!("{SCHEME}${}${}${}", self.iterations, to_hex(&self.salt), to_hex(&self.hash))
    }

    /// `None` for anything malformed: a row that cannot be parsed cannot confirm a PIN.
    pub fn parse(stored: &str) -> Option<Self> {
        let mut parts = stored.split('$');
        if parts.next()? != SCHEME {
            return None;
        }
        let iterations: u32 = parts.next()?.parse().ok().filter(|&n| n > 0)?;
        let salt = from_hex(parts.next()?)?;
        let hash = from_hex(parts.next()?)?;
        if parts.next().is_some() || salt.is_empty() || hash.len() != HASH_BYTES {
            return None;
        }
        Some(Self { iterations, salt, hash })
    }
}

/// Compares in time that depends only on the lengths, which are constants here.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_are_bounded_and_never_trimmed() {
        assert_eq!(validate_pin("1234"), Ok(()));
        assert_eq!(validate_pin("123"), Err(PinRefusal::TooShort));
        assert_eq!(validate_pin(&"9".repeat(65)), Err(PinRefusal::TooLong));
        assert_eq!(validate_pin(" 1234"), Err(PinRefusal::HasWhitespace));
        assert_eq!(validate_pin("12\t34"), Err(PinRefusal::HasWhitespace));
        // Characters, not bytes.
        assert_eq!(validate_pin("éééé"), Ok(()));
    }

    #[test]
    fn the_stored_form_round_trips_and_rejects_anything_malformed() {
        let h = PinHash { iterations: 600_000, salt: vec![1, 2, 3], hash: vec![7; HASH_BYTES] };
        assert_eq!(PinHash::parse(&h.format()), Some(h.clone()));
        let good = h.format();
        for bad in [
            "",
            "scrypt$00$11",
            &good.replace("600000", "0"),
            &good.replace("600000", "x"),
            &format!("{good}$extra"),
            "pbkdf2-sha256$1$$0707",
            "pbkdf2-sha256$1$0102$0707",
            "pbkdf2-sha256$1$0g$0707",
        ] {
            assert_eq!(PinHash::parse(bad), None, "{bad:?} should not parse");
        }
    }

    #[test]
    fn comparison_and_hex() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert_eq!(to_hex(&[0, 15, 255]), "000fff");
        assert_eq!(from_hex("000fff"), Some(vec![0, 15, 255]));
        assert_eq!(from_hex("0"), None);
        assert_eq!(from_hex("zz"), None);
    }
}
