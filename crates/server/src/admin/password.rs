//! Admin password hashing: PBKDF2-HMAC-SHA256 (via `ring`, already in the
//! tree for TLS) with a random 16-byte salt and OWASP's recommended 600,000
//! iterations. Stored as `pbkdf2-sha256$<iterations>$<salt hex>$<hash hex>`.
//!
//! Hashing takes on the order of a second on a Raspberry Pi, which also
//! slows password guessing; callers run it off the async executor.

use std::num::NonZeroU32;

use ring::pbkdf2;

const SCHEME: &str = "pbkdf2-sha256";
pub const ITERATIONS: u32 = 600_000;
const LEN: usize = 32;

/// Passwords must be 8 to 128 characters.
pub fn check_rules(password: &str) -> Result<(), String> {
    match password.chars().count() {
        0..8 => Err("the password must be at least 8 characters".into()),
        129.. => Err("the password must be at most 128 characters".into()),
        _ => Ok(()),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

/// Hashes `password` with a fresh salt.
pub fn hash(password: &str) -> Option<String> {
    hash_with(password, ITERATIONS)
}

pub(crate) fn hash_with(password: &str, iterations: u32) -> Option<String> {
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).ok()?;
    let mut out = [0u8; LEN];
    pbkdf2::derive(pbkdf2::PBKDF2_HMAC_SHA256, NonZeroU32::new(iterations)?, &salt, password.as_bytes(), &mut out);
    Some(format!("{SCHEME}${iterations}${}${}", hex(&salt), hex(&out)))
}

/// True when `password` matches `stored` (constant time).
pub fn verify(password: &str, stored: &str) -> bool {
    let mut parts = stored.split('$');
    let (Some(SCHEME), Some(iterations), Some(salt), Some(expected), None) =
        (parts.next(), parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let (Some(iterations), Some(salt), Some(expected)) =
        (iterations.parse().ok().and_then(NonZeroU32::new), unhex(salt), unhex(expected))
    else {
        return false;
    };
    pbkdf2::verify(pbkdf2::PBKDF2_HMAC_SHA256, iterations, &salt, password.as_bytes(), &expected).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_verify_and_differ_by_salt() {
        let a = hash_with("correct horse", 1000).unwrap();
        let b = hash_with("correct horse", 1000).unwrap();
        assert_ne!(a, b, "random salt");
        assert!(a.starts_with("pbkdf2-sha256$1000$"));
        assert!(verify("correct horse", &a) && verify("correct horse", &b));
        assert!(!verify("correct horsE", &a));
        assert!(!verify("correct horse", "garbage") && !verify("x", "pbkdf2-sha256$0$00$00"));
        assert!(!verify("correct horse", &format!("{a}$extra")));
    }

    #[test]
    fn default_strength() {
        let h = hash("longenough").unwrap();
        assert!(h.starts_with("pbkdf2-sha256$600000$"));
        assert!(verify("longenough", &h));
    }

    #[test]
    fn rules() {
        assert!(check_rules("short").is_err());
        assert!(check_rules("eight ch").is_ok());
        assert!(check_rules(&"x".repeat(129)).is_err());
    }
}
