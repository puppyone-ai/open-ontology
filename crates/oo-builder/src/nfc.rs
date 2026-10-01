//! NFC for `pgident/v1`.
//!
//! Hangul syllables use the Unicode algorithm. Every other canonical
//! decomposition comes from the Unicode 16.0.0 tables in `nfc_tables`.
//! Compatibility decompositions are not applied.

use crate::nfc_tables::{CCC, COMPOSE, DECOMP, DECOMP_POOL};

const SBASE: u32 = 0xAC00;
const LBASE: u32 = 0x1100;
const VBASE: u32 = 0x1161;
const TBASE: u32 = 0x11A7;
const LCOUNT: u32 = 19;
const VCOUNT: u32 = 21;
const TCOUNT: u32 = 28;
const NCOUNT: u32 = VCOUNT * TCOUNT;
const SCOUNT: u32 = LCOUNT * NCOUNT;

pub fn nfc(input: &str) -> String {
    let decomposed = nfd(input);
    let composed = compose(&decomposed);
    composed.into_iter().filter_map(char::from_u32).collect()
}

fn nfd(input: &str) -> Vec<u32> {
    let mut out = Vec::new();
    for ch in input.chars() {
        decompose(u32::from(ch), &mut out);
    }
    reorder(&mut out);
    out
}

fn decompose(cp: u32, out: &mut Vec<u32>) {
    if hangul_decompose(cp, out) {
        return;
    }
    if let Ok(index) = DECOMP.binary_search_by_key(&cp, |row| row.cp) {
        let row = &DECOMP[index];
        let start = usize::from(row.start);
        let end = start + usize::from(row.len);
        let parts: Vec<u32> = DECOMP_POOL[start..end].to_vec();
        for part in parts {
            decompose(part, out);
        }
        return;
    }
    out.push(cp);
}

fn hangul_decompose(cp: u32, out: &mut Vec<u32>) -> bool {
    if !(SBASE..SBASE + SCOUNT).contains(&cp) {
        return false;
    }
    let index = cp - SBASE;
    out.push(LBASE + index / NCOUNT);
    out.push(VBASE + (index % NCOUNT) / TCOUNT);
    let trail = TBASE + index % TCOUNT;
    if trail != TBASE {
        out.push(trail);
    }
    true
}

fn reorder(buf: &mut [u32]) {
    let mut index = 0;
    while index < buf.len() {
        if combining(buf[index]) == 0 {
            index += 1;
            continue;
        }
        let start = index;
        while index < buf.len() && combining(buf[index]) != 0 {
            index += 1;
        }
        buf[start..index].sort_by_key(|cp| combining(*cp));
    }
}

fn compose(chars: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::with_capacity(chars.len());
    let mut starter: Option<usize> = None;
    let mut previous_ccc = 0u8;
    for &cp in chars {
        let class = combining(cp);
        let composed = starter.and_then(|index| {
            if previous_ccc == 0 || previous_ccc < class {
                compose_pair(out[index], cp)
            } else {
                None
            }
        });
        if let (Some(value), Some(index)) = (composed, starter) {
            out[index] = value;
        } else {
            if class == 0 {
                starter = Some(out.len());
            }
            out.push(cp);
        }
        previous_ccc = class;
    }
    out
}

fn compose_pair(left: u32, right: u32) -> Option<u32> {
    compose_hangul(left, right).or_else(|| {
        COMPOSE
            .binary_search_by_key(&(left, right), |row| (row.0, row.1))
            .ok()
            .map(|index| COMPOSE[index].2)
    })
}

fn compose_hangul(left: u32, right: u32) -> Option<u32> {
    if (LBASE..LBASE + LCOUNT).contains(&left) && (VBASE..VBASE + VCOUNT).contains(&right) {
        let lindex = left - LBASE;
        let vindex = right - VBASE;
        return Some(SBASE + lindex * NCOUNT + vindex * TCOUNT);
    }
    if (SBASE..SBASE + SCOUNT).contains(&left) && (TBASE + 1..TBASE + TCOUNT).contains(&right) {
        let index = left - SBASE;
        if index.is_multiple_of(TCOUNT) {
            return Some(left + (right - TBASE));
        }
    }
    None
}

fn combining(cp: u32) -> u8 {
    CCC.binary_search_by_key(&cp, |row| row.0)
        .ok()
        .map(|index| CCC[index].1)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::nfc;

    #[test]
    fn nfc_matches_unicode_vectors() {
        assert_eq!(nfc("customer"), "customer");
        assert_eq!(nfc("é"), "é");
        assert_eq!(nfc("e\u{0301}"), "é");
        assert_eq!(nfc("Å"), "Å");
        assert_eq!(nfc("A\u{030A}"), "Å");
        assert_eq!(nfc("가"), "가");
        assert_eq!(nfc("\u{1100}\u{1161}"), "가");
        assert_eq!(nfc("고객"), "고객");
        assert_eq!(nfc("客户"), "客户");
        assert_eq!(nfc("e\u{0301}\u{0323}"), "ẹ\u{0301}");
        assert_eq!(nfc("\u{212B}"), "Å");
    }
}
