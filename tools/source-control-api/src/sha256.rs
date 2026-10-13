//! SHA-256 hex. The digest is lowercase.

use sha2::{Digest, Sha256};

pub fn action_hash(action: &str, body: &[u8]) -> String {
    let mut bytes = Vec::with_capacity(action.len() + 1 + body.len());
    bytes.extend_from_slice(action.as_bytes());
    bytes.push(b'\n');
    bytes.extend_from_slice(body);
    sha256_hex(&bytes)
}

pub fn upload_hash(source_ref: &str, body: &[u8]) -> String {
    let mut prefix = Vec::with_capacity(32 + source_ref.len());
    prefix.extend_from_slice(b"uploadSourceObject\n");
    prefix.extend_from_slice(source_ref.as_bytes());
    prefix.push(b'\n');
    prefix.extend_from_slice(body);
    sha256_hex(&prefix)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

pub fn contains_secret(haystack: &[u8], secrets: &[Vec<u8>]) -> bool {
    secrets.iter().any(|secret| {
        !secret.is_empty() && haystack.windows(secret.len()).any(|window| window == secret.as_slice())
    })
}
