//! Optional local text vectors and the deterministic ranking built on them (10 11.3, S13).
//!
//! Off by default. When a compile request names a model version, vectors rank only two things: the
//! BRIEF "other related" section and resolve candidate recall. The embedding is the lightest thing
//! that works offline: signed feature hashing of character 3-grams into 256 integer buckets
//! (`hashgram/1`). It uses integers only (FNV-1a 64, no float, no table, no network), so the Rust
//! code here and the sidecar endpoint `POST /v1/embed` (`tools/doc-parser/doc_parser/embed.py`)
//! produce the same bytes; a shared digest test pins both. Scores are integer basis points
//! (`-10000..=10000`), exact cosine over the in-scope set (no ANN), and the model version is part
//! of the compile parameters, so a replay with the same version is byte-identical and another
//! version is another pack.

use std::collections::BTreeMap;

pub mod lexical;
pub mod walk;

/// The only model version this build can compute.
pub const MODEL_VERSION: &str = "hashgram/1";
/// Vector length; matches `_oo_docs.block_vector.embedding vector(256)`.
pub const DIM: usize = 256;
/// A vector hit below this score never enters "other related" (25 %).
pub const MIN_BP: i32 = 2500;
/// "Other related" takes at most this many entities that the graph walk did not reach.
pub const MAX_EXTRA: usize = 8;

/// One embedding: signed 3-gram counts, clamped to `i16`.
pub type Vector = [i16; DIM];

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{3000}')
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Embed one text with `hashgram/1`. Lowercase ASCII only, whitespace runs collapse to one space,
/// then every window of three characters (the whole text when it is shorter) is hashed.
pub fn embed(text: &str) -> Vector {
    let mut chars: Vec<char> = Vec::new();
    let mut gap = true;
    for c in text.chars() {
        if is_space(c) {
            if !gap {
                chars.push(' ');
            }
            gap = true;
        } else {
            chars.push(c.to_ascii_lowercase());
            gap = false;
        }
    }
    while chars.last() == Some(&' ') {
        chars.pop();
    }
    let mut acc = [0i32; DIM];
    let mut add = |gram: &[char]| {
        let s: String = gram.iter().collect();
        let h = fnv1a(s.as_bytes());
        let bucket = usize::try_from(h % DIM as u64).unwrap_or(0);
        acc[bucket] += if h >> 63 == 0 { 1 } else { -1 };
    };
    match chars.len() {
        0 => {}
        1..=2 => add(&chars),
        _ => chars.windows(3).for_each(&mut add),
    }
    let mut out = [0i16; DIM];
    for (o, a) in out.iter_mut().zip(acc) {
        *o = i16::try_from(a.clamp(i32::from(i16::MIN), i32::from(i16::MAX))).unwrap_or(0);
    }
    out
}

fn isqrt(n: u128) -> u128 {
    if n < 2 {
        return n;
    }
    let mut x = n;
    let mut y = x.div_ceil(2);
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// Cosine in integer basis points, truncated toward zero; `0` when either vector is all zero.
pub fn cosine_bp(a: &Vector, b: &Vector) -> i32 {
    let (mut dot, mut na, mut nb) = (0i64, 0i64, 0i64);
    for (x, y) in a.iter().zip(b) {
        let (x, y) = (i64::from(*x), i64::from(*y));
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    let denom = isqrt(na as u128 * nb as u128);
    if denom == 0 {
        return 0;
    }
    let bp = i128::from(dot) * 10_000 / denom as i128;
    i32::try_from(bp.clamp(-10_000, 10_000)).unwrap_or(0)
}

/// Digest of the embeddings of a fixed text list; the Python sidecar test pins the same value.
pub fn parity_digest(texts: &[&str]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for t in texts {
        for v in embed(t) {
            h.update(v.to_be_bytes());
        }
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Texts of the parity test (shared with `tools/doc-parser/tests/test_embed.py`).
pub const PARITY_TEXTS: [&str; 6] = [
    "",
    "ab",
    "Quarterly   budget review\tfor the Apollo project",
    "项目预算评审会议纪要",
    "Mixed 中文 and ASCII, ünïcode",
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
];

/// Candidates for resolve recall: `(key, name)` pairs ranked by `(score desc, key asc)`; only
/// scores of at least [`MIN_BP`] stay, at most `limit` of them. Pure, so the order never depends
/// on the input order.
pub fn recall(query: &str, candidates: &[(String, String)], limit: usize) -> Vec<(String, i32)> {
    let q = embed(query);
    let mut hits: Vec<(String, i32)> = candidates
        .iter()
        .map(|(k, n)| (k.clone(), cosine_bp(&q, &embed(n))))
        .filter(|(_, s)| *s >= MIN_BP)
        .collect();
    hits.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    hits.dedup_by(|a, b| a.0 == b.0);
    hits.truncate(limit);
    hits
}

/// Entity key to best block score, for BRIEF.
pub type Scores = BTreeMap<String, i32>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_text_scores_full_and_unrelated_low() {
        let a = embed("quarterly budget review for the apollo project");
        assert_eq!(cosine_bp(&a, &a), 10_000);
        let b = embed("zebra crossing paint schedule");
        assert!(cosine_bp(&a, &b) < MIN_BP);
        assert_eq!(cosine_bp(&embed(""), &a), 0);
    }

    #[test]
    fn case_and_space_folding() {
        assert_eq!(embed("Apollo   Budget"), embed("apollo budget"));
    }

    #[test]
    fn recall_is_order_independent() {
        let c: Vec<(String, String)> = ["apollo budget", "apollo budgets", "zebra"]
            .iter()
            .enumerate()
            .map(|(i, n)| (format!("k{i}"), (*n).to_string()))
            .collect();
        let mut r = c.clone();
        r.reverse();
        assert_eq!(
            recall("apollo budget", &c, 5),
            recall("apollo budget", &r, 5)
        );
        assert_eq!(recall("apollo budget", &c, 5)[0].0, "k0");
    }

    #[test]
    fn parity_digest_is_pinned() {
        assert_eq!(
            parity_digest(&PARITY_TEXTS),
            "8afb69c88427c67a6bcbdf0a4ac78a3e7986358861e3cba730c4f7918ac42e37"
        );
    }
}
