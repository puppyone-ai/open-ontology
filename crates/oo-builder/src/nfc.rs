//! Canonical NFC for `pgident/v1`.
//!
//! Unicode 17.0.0, from `unicode-normalization` 0.1.25. That is the same
//! crate and version the PostgreSQL connector applies before `pgident/v1`.
//! This is not NFKC and it does not case-fold. Compatibility decompositions
//! stay in the string.
//!
//! ASCII and strings that were already canonical NFC keep the same bytes.
//! The removed hand-written composer did not: `U+0045 U+0304 U+0300` became
//! `U+0112 U+0300` instead of `U+1E14`, and 34 Unicode 17 combining marks
//! were treated as class 0. Digests that include those identifiers were
//! computed from the wrong NFC. Recompile from the raw catalog. An old
//! digest is not a fresh binding.

use unicode_normalization::UnicodeNormalization;

pub fn nfc(input: &str) -> String {
    input.nfc().collect()
}

#[cfg(test)]
mod tests {
    use super::nfc;
    use unicode_normalization::UNICODE_VERSION;

    #[test]
    fn nfc_is_unicode_17_canonical() {
        assert_eq!(UNICODE_VERSION, (17, 0, 0));
        assert_eq!(nfc("customer"), "customer");
        assert_eq!(nfc("é"), "é");
        assert_eq!(nfc("e\u{0301}"), "é");
        assert_eq!(nfc("Å"), "Å");
        assert_eq!(nfc("A\u{030A}"), "Å");
        assert_eq!(nfc("\u{212B}"), "Å");
        assert_eq!(nfc("가"), "가");
        assert_eq!(nfc("\u{1100}\u{1161}"), "가");
        assert_eq!(nfc("\u{1100}\u{1161}\u{11A8}"), "각");
        assert_eq!(nfc("고객"), "고객");
        assert_eq!(nfc("客户"), "客户");
        assert_eq!(nfc("e\u{0301}\u{0323}"), "ẹ\u{0301}");
        assert_eq!(nfc("e\u{0323}\u{0301}"), "ẹ\u{0301}");
        assert_eq!(nfc("\u{1E14}"), "\u{1E14}");
        assert_eq!(nfc("\u{0045}\u{0304}\u{0300}"), "\u{1E14}");
        assert_eq!(nfc("\u{0112}\u{0300}"), "\u{1E14}");
        assert_eq!(nfc("e\u{1ACF}\u{0323}"), "\u{1EB9}\u{1ACF}");
        assert_eq!(nfc("ﬁ"), "ﬁ");
        assert_ne!(nfc("ﬁ"), "fi");
        assert_eq!(
            nfc(&nfc("\u{0045}\u{0304}\u{0300}")),
            nfc("\u{0045}\u{0304}\u{0300}")
        );
    }
}
