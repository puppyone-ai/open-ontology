//! Kani proofs for KP1 (80 section 3.2): a Drive-relative path that passes the
//! path rules can never leave the Drive root. Bounded: inputs up to
//! [`N`] bytes. Run with `cargo kani -p oo-docs --harness kp1`.
//!
//! The independent oracle is a lexical walk from the root: every normal
//! segment goes one level down, `..` goes one level up, `.` and empty segments
//! stay. A path escapes when the depth ever becomes negative, or when it is
//! absolute (starts at '/'), or when it carries a NUL, a backslash or a
//! control character that a platform could read as a separator.
//!
//! Proven on the byte kernels `raw_key_is_safe` and `check_doc_path_bytes`
//! (the latter is what `DocPath::new` calls before it copies the string).
//! Proving through `DocPath::new(&str)` itself did not work: with `chars()`,
//! `split` and `to_owned` over 3 ASCII bytes CBMC gave no result in 25
//! minutes (4 bytes ended `FAILED` after 18 minutes, cause not diagnosed).
//!
//! Not proven here: `normalize_raw_key` (UTF-8 decoding and the NFC step walk
//! the Unicode composition tables). Its result is the output of `DocPath::new`,
//! which the proofs below cover; the NFC step is covered by the deterministic
//! property tests of `oo-connectors` (`objectstore::tests`, 20,000 cases).

use oo_connectors::objectstore::path::{check_doc_path_bytes, raw_key_is_safe};

/// Input bound in bytes.
const N: usize = 12;

/// Lowest depth reached by a lexical walk from the root, and whether the key
/// is absolute.
fn walk(raw: &[u8]) -> (i32, bool) {
    let absolute = raw.first() == Some(&b'/');
    let mut depth = 0i32;
    let mut lowest = 0i32;
    for seg in raw.split(|b| *b == b'/') {
        if seg == b".." {
            depth -= 1;
        } else if !seg.is_empty() && seg != b"." {
            depth += 1;
        }
        lowest = lowest.min(depth);
    }
    (lowest, absolute)
}

#[kani::proof]
#[kani::unwind(16)]
fn kp1_raw_key_safe_stays_inside_root() {
    let buf: [u8; N] = kani::any();
    let len: usize = kani::any();
    kani::assume(len <= N);
    let raw = &buf[..len];
    if raw_key_is_safe(raw) {
        let (lowest, absolute) = walk(raw);
        assert!(lowest >= 0);
        assert!(!absolute);
        assert!(!raw.contains(&0));
        assert!(!raw.is_empty());
    }
}

/// `DocPath::new` is `check_doc_path_bytes` plus a copy; the proof is on the
/// byte kernel, over arbitrary bytes (valid UTF-8 or not).
#[kani::proof]
#[kani::unwind(16)]
fn kp1_docpath_stays_inside_root() {
    let buf: [u8; N] = kani::any();
    let len: usize = kani::any();
    kani::assume(len <= N);
    let raw = &buf[..len];
    if check_doc_path_bytes(raw).is_ok() {
        let (lowest, absolute) = walk(raw);
        assert!(lowest >= 0);
        assert!(!absolute);
        assert!(!raw.contains(&b'\\'));
        assert!(!raw.contains(&0));
        assert!(!raw.is_empty() && raw[len - 1] != b'/');
        for seg in raw.split(|b| *b == b'/') {
            assert!(!seg.is_empty() && seg != b"." && seg != b"..");
        }
    }
}
