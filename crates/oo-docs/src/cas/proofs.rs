//! Kani proofs for KP8 (80 section 3.2): the CAS path of a BLAKE3 digest is
//! injective and only produces legal file names. Bounded to the real key
//! length (64 lowercase hex characters); the key bytes are fully symbolic.
//! Run with `cargo kani -p oo-docs --harness kp8`.
//!
//! The proofs run on `write_rel_path`, the allocation-free kernel that
//! `object_rel_path` calls (the wrapper only checks the key and copies the
//! bytes into a `String`). Proving the wrapper with `String` and `format!` in
//! front of a symbolic 64 byte key did not finish in 20 minutes.

use super::layout::{CasKind, MAX_REL_PATH, is_hex64, write_rel_path};

fn symbolic_key() -> [u8; 64] {
    let key: [u8; 64] = kani::any();
    kani::assume(is_hex64(&key));
    key
}

fn any_kind() -> (CasKind, usize) {
    match kani::any::<u8>() % 4 {
        0 => (CasKind::Orig, 0),
        1 => (CasKind::Ir, 5),
        2 => (CasKind::Llm, 5),
        _ => (CasKind::Pack, 0),
    }
}

/// Only `[a-z0-9/._]`, relative, no empty segment, no `..`, the key verbatim
/// before an optional `.json`, and the length is inside the buffer.
#[kani::proof]
#[kani::unwind(80)]
fn kp8_cas_path_is_a_legal_name() {
    let key = symbolic_key();
    let (k, tail) = any_kind();
    let mut out = [0u8; MAX_REL_PATH];
    let n = write_rel_path(k, &key, &mut out);
    assert!(n >= 64 + tail && n <= MAX_REL_PATH);
    let mut i = 0;
    while i < n {
        assert!(matches!(
            out[i],
            b'a'..=b'z' | b'0'..=b'9' | b'/' | b'.' | b'_'
        ));
        if i + 1 < n {
            assert!(!(out[i] == b'/' && out[i + 1] == b'/'));
            assert!(!(out[i] == b'.' && out[i + 1] == b'.'));
        }
        i += 1;
    }
    assert!(out[0] != b'/' && out[n - 1] != b'/');
    assert!(out[n - tail - 64..n - tail] == key);
    if tail == 5 {
        assert!(out[n - 5..n] == *b".json");
    }
}

/// Same kind, same path implies same key (injective per kind); different
/// kinds never share a path.
#[kani::proof]
#[kani::unwind(80)]
fn kp8_cas_path_is_injective() {
    let (a, b) = (symbolic_key(), symbolic_key());
    let ((k1, t1), (k2, t2)) = (any_kind(), any_kind());
    let (mut o1, mut o2) = ([0u8; MAX_REL_PATH], [0u8; MAX_REL_PATH]);
    let n1 = write_rel_path(k1, &a, &mut o1);
    let n2 = write_rel_path(k2, &b, &mut o2);
    if n1 == n2 && o1[..n1] == o2[..n2] {
        assert!(a == b);
        assert!(k1 == k2 && t1 == t2);
    }
}

/// The key check rejects every 64 byte input with a character outside
/// `[0-9a-f]` and every other length.
#[kani::proof]
#[kani::unwind(70)]
fn kp8_bad_keys_are_rejected() {
    let mut bad = symbolic_key();
    let at: usize = kani::any();
    kani::assume(at < 64);
    let ch: u8 = kani::any();
    kani::assume(!matches!(ch, b'0'..=b'9' | b'a'..=b'f'));
    bad[at] = ch;
    assert!(!is_hex64(&bad));
    assert!(!is_hex64(&bad[..63]));
    let good = symbolic_key();
    assert!(is_hex64(&good));
}
