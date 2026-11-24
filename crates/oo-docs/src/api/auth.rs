//! Bearer token check and the Host and Origin guards (10 §8.1).
//!
//! Tokens are never kept: only their SHA-256. A presented token is hashed and
//! compared with every stored hash in constant time, without stopping at the
//! first match.

use sha2::{Digest, Sha256};

/// SHA-256 of a token.
pub fn token_hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

/// Constant-time equality of two digests.
pub fn ct_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// True when `presented` hashes to one of `accepted`. Every entry is
/// compared; the result is folded without a data dependent branch.
pub fn token_matches(accepted: &[[u8; 32]], presented: &str) -> bool {
    let h = token_hash(presented);
    let mut hit = 0u8;
    for a in accepted {
        hit |= u8::from(ct_eq(a, &h));
    }
    hit == 1
}

/// The bearer value of an `Authorization` header.
pub fn bearer(header: Option<&str>) -> Option<&str> {
    let value = header?;
    let (scheme, token) = value.split_once(' ')?;
    (scheme.eq_ignore_ascii_case("bearer") && !token.trim().is_empty()).then(|| token.trim())
}

/// Hosts the engine answers to: `127.0.0.1:<port>` and `localhost:<port>`,
/// plus configured extras (a compose service name, say).
pub fn allowed_hosts(port: u16, extra: &[String]) -> Vec<String> {
    let mut hosts = vec![format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    hosts.extend(extra.iter().map(|h| h.to_ascii_lowercase()));
    hosts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_is_exact_and_covers_all_entries() {
        let accepted = [token_hash("oodi_one"), token_hash("oodt_two")];
        assert!(token_matches(&accepted, "oodi_one"));
        assert!(token_matches(&accepted, "oodt_two"));
        assert!(!token_matches(&accepted, "oodi_on"));
        assert!(!token_matches(&accepted, ""));
        assert!(!token_matches(&[], "oodi_one"));
    }

    #[test]
    fn bearer_parsing() {
        assert_eq!(bearer(Some("Bearer abc")), Some("abc"));
        assert_eq!(bearer(Some("bearer  abc ")), Some("abc"));
        assert_eq!(bearer(Some("Basic abc")), None);
        assert_eq!(bearer(Some("Bearer ")), None);
        assert_eq!(bearer(None), None);
    }

    #[test]
    fn hosts() {
        let h = allowed_hosts(8700, &["Engine:8700".into()]);
        assert_eq!(h, ["127.0.0.1:8700", "localhost:8700", "engine:8700"]);
    }
}
