//! Check support-matrix claims against conformance evidence.
//! This crate does not promote grades and does not write the matrix.

pub mod activate;
pub mod audit;
pub mod cases;
pub mod evaluate;
pub mod gitobj;
pub mod historical;
pub mod identity;
pub mod jsonutil;
pub mod matrix;
pub mod nightly;
pub mod package;
pub mod scope;

pub use evaluate::{run_paths, Report, Request};
pub use scope::{run_scope, ScopeInput, ScopeReport};

use sha2::{Digest, Sha256};

pub fn sha256_hex(bytes: &[u8]) -> String {
    to_hex(&Sha256::digest(bytes))
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
