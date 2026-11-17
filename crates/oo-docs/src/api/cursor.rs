//! Opaque keyset cursors (10 §8.1): base64url of a JSON tuple that also
//! carries a hash of the filters, so a cursor from another query is refused
//! with `DOCS_ENGINE_CURSOR_INVALID`.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::error::{ApiError, code};

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// base64url without padding.
pub fn b64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[n as usize & 63] as char);
        }
    }
    out
}

/// Inverse of [`b64_encode`]; `None` on any invalid character or length.
pub fn b64_decode(text: &str) -> Option<Vec<u8>> {
    let val = |c: u8| ALPHABET.iter().position(|a| *a == c).map(|p| p as u32);
    let bytes = text.as_bytes();
    if bytes.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            n |= val(*c)? << (18 - 6 * i as u32);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}

/// First 16 hex digits of SHA-256 over the canonical filter text.
pub fn filter_hash(canonical: &str) -> String {
    let digest = Sha256::digest(canonical.as_bytes());
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// Encode a key tuple with its filter hash.
pub fn encode(key: &[Value], filters: &str) -> String {
    b64_encode(
        json!({ "k": key, "f": filter_hash(filters) })
            .to_string()
            .as_bytes(),
    )
}

/// Decode and check the filter hash.
pub fn decode(token: &str, filters: &str) -> Result<Vec<Value>, ApiError> {
    let invalid = || ApiError::new(code::CURSOR_INVALID, "cursor does not belong to this query");
    let raw = b64_decode(token).ok_or_else(invalid)?;
    let value: Value = serde_json::from_slice(&raw).map_err(|_| invalid())?;
    if value["f"].as_str() != Some(filter_hash(filters).as_str()) {
        return Err(invalid());
    }
    value["k"].as_array().cloned().ok_or_else(invalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_round_trips_every_length() {
        for len in 0..40usize {
            let data: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            let text = b64_encode(&data);
            assert!(!text.contains(['+', '/', '=']));
            assert_eq!(b64_decode(&text).unwrap(), data, "len {len}");
        }
        assert!(b64_decode("a").is_none());
        assert!(b64_decode("a+b=").is_none());
    }

    #[test]
    fn cursor_is_bound_to_its_filters() {
        let token = encode(&[json!("a/b.md"), json!("id-1")], "drive|path=a");
        assert_eq!(
            decode(&token, "drive|path=a").unwrap(),
            vec![json!("a/b.md"), json!("id-1")]
        );
        let err = decode(&token, "drive|path=b").unwrap_err();
        assert_eq!(err.code, code::CURSOR_INVALID);
        assert_eq!(decode("!!", "x").unwrap_err().code, code::CURSOR_INVALID);
        assert_eq!(
            decode(&b64_encode(b"not json"), "x").unwrap_err().code,
            code::CURSOR_INVALID
        );
    }
}
