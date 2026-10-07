//! Opaque token material. The plaintext is not a cache key and not a SQL parameter.

use zeroize::Zeroize;

use crate::error::StoreError;
use crate::model::TokenKind;
use crate::sha256::{hex, sha256};

pub struct PlainToken {
    bytes: Vec<u8>,
}

impl std::fmt::Debug for PlainToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PlainToken(redacted)")
    }
}

impl Drop for PlainToken {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

impl PlainToken {
    pub fn issue(kind: TokenKind, raw: &[u8; 32]) -> Self {
        let mut text = String::from(kind.prefix());
        text.push_str(&base64url(raw));
        Self {
            bytes: text.into_bytes(),
        }
    }

    pub fn sha256_hex(&self) -> Result<String, StoreError> {
        let digest = sha256(&self.bytes).ok_or(StoreError::Input)?;
        Ok(hex(&digest))
    }

    pub fn reveal_once(mut self) -> String {
        let bytes = std::mem::take(&mut self.bytes);
        match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(err) => {
                let mut bytes = err.into_bytes();
                bytes.zeroize();
                String::new()
            }
        }
    }
}

pub fn scrub(value: &mut String) {
    value.zeroize();
}

pub fn draw_32() -> Result<[u8; 32], StoreError> {
    let mut bytes = [0u8; 32];
    if getrandom::getrandom(&mut bytes).is_err() {
        return Err(StoreError::Randomness);
    }
    Ok(bytes)
}

pub fn token_hash(token: &str) -> Result<(TokenKind, String), StoreError> {
    let (rest, kind) = if let Some(rest) = token.strip_prefix("oost_") {
        (rest, TokenKind::Session)
    } else if let Some(rest) = token.strip_prefix("ooct_") {
        (rest, TokenKind::Client)
    } else {
        return Err(StoreError::Input);
    };
    if rest.len() != 43 || !rest.bytes().all(is_base64url) {
        return Err(StoreError::Input);
    }
    let digest = sha256(token.as_bytes()).ok_or(StoreError::Input)?;
    Ok((kind, hex(&digest)))
}

fn is_base64url(byte: u8) -> bool {
    matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_')
}

fn base64url(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    let mut index = 0;
    while index + 3 <= bytes.len() {
        let chunk = (u32::from(bytes[index]) << 16)
            | (u32::from(bytes[index + 1]) << 8)
            | u32::from(bytes[index + 2]);
        out.push(b64_char(TABLE, chunk, 18));
        out.push(b64_char(TABLE, chunk, 12));
        out.push(b64_char(TABLE, chunk, 6));
        out.push(b64_char(TABLE, chunk, 0));
        index += 3;
    }
    if index < bytes.len() {
        let first = u32::from(bytes[index]);
        let second = if index + 1 < bytes.len() {
            u32::from(bytes[index + 1])
        } else {
            0
        };
        let chunk = (first << 16) | (second << 8);
        out.push(b64_char(TABLE, chunk, 18));
        out.push(b64_char(TABLE, chunk, 12));
        if index + 1 < bytes.len() {
            out.push(b64_char(TABLE, chunk, 6));
        }
    }
    out
}

fn b64_char(table: &[u8; 64], chunk: u32, shift: u32) -> char {
    let narrowed = u8::try_from((chunk >> shift) & 63).unwrap_or(0);
    char::from(table[usize::from(narrowed)])
}

#[cfg(test)]
mod tests {
    use super::{draw_32, token_hash, PlainToken};
    use crate::model::TokenKind;
    use crate::sha256::{hex, sha256};
    use zeroize::Zeroize;

    #[test]
    fn os_csprng_sample_has_no_quality_claim() {
        let left = draw_32().expect("draw");
        let right = draw_32().expect("draw");
        assert_eq!(left.len(), 32);
        assert!(left.iter().any(|byte| *byte != 0), "sample was all zeroes");
        assert!(right.iter().any(|byte| *byte != 0), "sample was all zeroes");
        assert!(left != right, "two samples matched");
    }

    #[test]
    fn plaintext_debug_is_redacted_and_hash_matches_sha256() {
        let mut raw = draw_32().expect("draw");
        let token = PlainToken::issue(TokenKind::Session, &raw);
        raw.zeroize();
        let debug = format!("{token:?}");
        assert_eq!(debug, "PlainToken(redacted)");
        assert!(!debug.contains("oost_"), "debug leaked a prefix");
        let hash = token.sha256_hex().expect("hash");
        let mut revealed = token.reveal_once();
        assert!(revealed.starts_with("oost_"), "prefix missing");
        assert_eq!(revealed.len(), 48);
        let (kind, again) = token_hash(&revealed).expect("parse");
        assert_eq!(kind, TokenKind::Session);
        assert_eq!(again, hash);
        let digest = sha256(revealed.as_bytes()).expect("sha");
        assert_eq!(hex(&digest), hash);
        revealed.zeroize();
    }
}
