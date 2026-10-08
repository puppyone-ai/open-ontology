//! AES-256-GCM through ring 0.17.14. Nonces are unique per key in PostgreSQL.

use std::collections::VecDeque;
use std::sync::Mutex;

use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

use crate::error::Fault;

pub const CIPHER: &str = "ring-aes-256-gcm";

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(hex_digit(byte >> 4));
        out.push(hex_digit(byte & 0x0f));
    }
    out
}

pub fn hashes_equal(left: &str, right: &str) -> bool {
    crate::ids::ct_eq(left, right)
}

pub trait NonceSource: Send + Sync {
    fn next(&self) -> Result<[u8; 12], Fault>;
}

pub struct OsNonce;

impl NonceSource for OsNonce {
    fn next(&self) -> Result<[u8; 12], Fault> {
        let mut nonce = [0_u8; 12];
        getrandom::getrandom(&mut nonce).map_err(|_| Fault::not_ready())?;
        Ok(nonce)
    }
}

pub struct ScriptedNonce {
    pending: Mutex<VecDeque<[u8; 12]>>,
}

impl ScriptedNonce {
    pub fn new() -> Self {
        Self { pending: Mutex::new(VecDeque::new()) }
    }

    pub fn push(&self, nonce: [u8; 12]) -> Result<(), Fault> {
        self.pending.lock().map_err(|_| Fault::not_ready())?.push_back(nonce);
        Ok(())
    }
}

impl Default for ScriptedNonce {
    fn default() -> Self {
        Self::new()
    }
}

impl NonceSource for ScriptedNonce {
    fn next(&self) -> Result<[u8; 12], Fault> {
        self.pending.lock().map_err(|_| Fault::not_ready())?.pop_front().ok_or_else(Fault::malformed)
    }
}

pub fn seal(key: &[u8], nonce: &[u8; 12], aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, Fault> {
    let sealing = less_safe(key)?;
    let mut buffer = plaintext.to_vec();
    sealing
        .seal_in_place_append_tag(Nonce::assume_unique_for_key(*nonce), Aad::from(aad), &mut buffer)
        .map_err(|_| Fault::aead())?;
    Ok(buffer)
}

pub fn open(key: &[u8], nonce: &[u8; 12], aad: &[u8], ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>, Fault> {
    if ciphertext.len() < 16 {
        return Err(Fault::aead());
    }
    let opening = less_safe(key).map_err(|_| Fault::aead())?;
    let mut buffer = ciphertext.to_vec();
    let opened = opening
        .open_in_place(Nonce::assume_unique_for_key(*nonce), Aad::from(aad), &mut buffer)
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

fn hex_digit(value: u8) -> char {
    char::from(b"0123456789abcdef"[usize::from(value)])
}
