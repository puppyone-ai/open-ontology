//! AES-256-GCM and HMAC-SHA256 through ring 0.17.14.

use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use ring::hmac;
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

use crate::error::Fault;

pub const CHUNK: usize = 8192;
pub const MAX_PLAINTEXT: usize = 33_554_432;
pub const TTL_MS: i64 = 86_400_000;

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub fn ct_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let key = hmac::Key::new(hmac::HMAC_SHA256, b"oo-artifact-eq");
    let tag = hmac::sign(&key, left);
    hmac::verify(&key, right, tag.as_ref()).is_ok()
}

pub fn mac_hex(key: &[u8], bytes: &[u8]) -> Result<String, Fault> {
    if key.len() != 32 {
        return Err(Fault::malformed());
    }
    let mac_key = hmac::Key::new(hmac::HMAC_SHA256, key);
    Ok(hex(hmac::sign(&mac_key, bytes).as_ref()))
}

pub fn mac_ok(key: &[u8], bytes: &[u8], presented_hex: &str) -> Result<bool, Fault> {
    let expected = mac_hex(key, bytes)?;
    Ok(ct_eq(expected.as_bytes(), presented_hex.as_bytes()))
}

pub fn random_bytes(len: usize) -> Result<Vec<u8>, Fault> {
    let mut bytes = vec![0_u8; len];
    getrandom::getrandom(&mut bytes).map_err(|_| Fault::not_ready())?;
    Ok(bytes)
}

pub fn random_hex(len: usize) -> Result<String, Fault> {
    Ok(hex(&random_bytes(len)?))
}

pub fn token() -> Result<String, Fault> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    Ok(URL_SAFE_NO_PAD.encode(random_bytes(32)?))
}

pub fn token_ok(value: &str) -> bool {
    value.len() == 43 && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

pub fn aad(fields: &[&[u8]]) -> Result<Vec<u8>, Fault> {
    let mut out = Vec::new();
    for field in fields {
        let len = u16::try_from(field.len()).map_err(|_| Fault::malformed())?;
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(field);
    }
    Ok(out)
}

pub fn seal(key: &[u8], nonce: &[u8; 12], aad_bytes: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, Fault> {
    let sealing = less_safe(key)?;
    let mut buffer = plaintext.to_vec();
    sealing
        .seal_in_place_append_tag(Nonce::assume_unique_for_key(*nonce), Aad::from(aad_bytes), &mut buffer)
        .map_err(|_| Fault::aead())?;
    Ok(buffer)
}

pub fn open(key: &[u8], nonce: &[u8; 12], aad_bytes: &[u8], ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>, Fault> {
    if ciphertext.len() < 16 {
        return Err(Fault::aead());
    }
    let opening = less_safe(key).map_err(|_| Fault::aead())?;
    let mut buffer = ciphertext.to_vec();
    let opened = opening
        .open_in_place(Nonce::assume_unique_for_key(*nonce), Aad::from(aad_bytes), &mut buffer)
        .map_err(|_| Fault::aead())?;
    let plain = Zeroizing::new(opened.to_vec());
    buffer.zeroize();
    Ok(plain)
}

fn less_safe(key: &[u8]) -> Result<LessSafeKey, Fault> {
    if key.len() != 32 {
        return Err(Fault::malformed());
    }
    let unbound = UnboundKey::new(&AES_256_GCM, key).map_err(|_| Fault::malformed())?;
    Ok(LessSafeKey::new(unbound))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}
