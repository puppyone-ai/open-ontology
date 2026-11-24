//! Kani proofs for KP2 (80 section 3.2): anchor formatting and parsing.
//! Run with `cargo kani -p oo-core --harness kp2`.
//!
//! Covered, over the whole input range (no bound below the type or format
//! limits): the A1 cell codec kernels `a1_write` and `a1_parse_bytes` (what
//! `a1_from_cell` and `a1_parse` call) round trip for every column A..ZZZ and
//! row 1..9 999 999, reject everything outside, are total on arbitrary input
//! up to 8 bytes, cannot overflow, and whatever parses is in range, ordered
//! and canonical; `Anchor::check` of a docx paragraph range is exact.
//!
//! Not covered: `Anchor::fragment` and the `String`/`format!` wrappers (the
//! core::fmt integer formatting does not terminate under CBMC: a one digit
//! text-fragment round trip ran 10 minutes without a result); `Anchor::check`
//! of the pdf, slide, text and cells variants through a symbolic value (the
//! enum with its `String` niche made even the pdf comparison run over 8
//! minutes; the code is three comparisons and is covered by unit tests);
//! `rc_parse` (docx cell ranges, private to `anchor.rs`); the sheet-name quoting.

use super::anchor::{Anchor, MAX_A1, a1_parse_bytes, a1_write};

const MAX_COL: u32 = 18_278;
const MAX_ROW: u32 = 9_999_999;

/// The proofs run on the allocation-free kernels `a1_write` and
/// `a1_parse_bytes` that `a1_from_cell` and `a1_parse` call (the wrappers only
/// copy bytes to and from `String`/`&str`).
#[kani::proof]
#[kani::unwind(12)]
fn kp2_a1_roundtrip() {
    let col: u32 = kani::any();
    let row: u32 = kani::any();
    kani::assume(col >= 1 && col <= MAX_COL && row >= 1 && row <= MAX_ROW);
    let mut out = [0u8; MAX_A1];
    let n = a1_write(col, row, &mut out).expect("in range");
    assert!(n >= 2 && n <= MAX_A1);
    assert_eq!(a1_parse_bytes(&out[..n]), Some(((col, row), (col, row))));
}

#[kani::proof]
#[kani::unwind(12)]
fn kp2_a1_write_rejects_out_of_range() {
    let col: u32 = kani::any();
    let row: u32 = kani::any();
    kani::assume(col == 0 || col > MAX_COL || row == 0 || row > MAX_ROW);
    let mut out = [0u8; MAX_A1];
    assert!(a1_write(col, row, &mut out).is_none());
}

/// Any short input (up to 8 bytes): no panic, no overflow, and whatever parses
/// is in range, ordered and canonical (re-formatting the first corner gives
/// the bytes before the colon).
#[kani::proof]
#[kani::unwind(12)]
fn kp2_a1_parse_is_total_and_sound() {
    let buf: [u8; 8] = kani::any();
    let len: usize = kani::any();
    kani::assume(len <= 8);
    let text = &buf[..len];
    if let Some(((c0, r0), (c1, r1))) = a1_parse_bytes(text) {
        assert!(c0 >= 1 && c0 <= c1 && c1 <= MAX_COL);
        assert!(r0 >= 1 && r0 <= r1 && r1 <= MAX_ROW);
        let mut head = 0;
        while head < len && text[head] != b':' {
            head += 1;
        }
        let mut out = [0u8; MAX_A1];
        let n = a1_write(c0, r0, &mut out).expect("in range");
        assert!(out[..n] == text[..head]);
    }
}

/// A docx anchor without a table: ordered paragraphs pass, reversed ones do not.
#[kani::proof]
#[kani::unwind(12)]
fn kp2_docx_check_is_total() {
    let (s, e): (u32, u32) = (kani::any(), kani::any());
    let docx = Anchor::Docx {
        para_start: s,
        para_end: e,
        table_index: None,
        cell_range: None,
    };
    assert_eq!(docx.check().is_ok(), s <= e);
}
