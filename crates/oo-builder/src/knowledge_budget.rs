//! Deterministic release-gate estimate, not a model-specific tokenizer.
pub fn unicode_word_tokens(text: &str) -> u64 {
    let mut count = 0_u64;
    let mut in_word = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            in_word = false;
        } else if ch.is_alphanumeric() {
            if !in_word {
                count = count.saturating_add(1);
                in_word = true;
            }
        } else {
            count = count.saturating_add(1);
            in_word = false;
        }
    }
    count
}

/// Name of the estimator below, stored in manifests (10 §7.5).
pub const TOKEN_ESTIMATOR_V2: &str = "oo.token-estimate/v2";

/// CJK unified ideographs, kana and hangul: one token per character.
fn is_cjk_like(ch: char) -> bool {
    matches!(
        u32::from(ch),
        0x3400..=0x4DBF      // CJK extension A
        | 0x4E00..=0x9FFF    // CJK unified ideographs
        | 0xF900..=0xFAFF    // compatibility ideographs
        | 0x20000..=0x2FA1F  // extensions B to F, compatibility supplement
        | 0x30000..=0x323AF  // extensions G to H
        | 0x3040..=0x309F    // hiragana
        | 0x30A0..=0x30FF    // katakana
        | 0x31F0..=0x31FF    // katakana phonetic extensions
        | 0xFF66..=0xFF9F    // half-width katakana
        | 0x1100..=0x11FF    // hangul jamo
        | 0x3130..=0x318F    // hangul compatibility jamo
        | 0xAC00..=0xD7AF    // hangul syllables
    )
}

/// `oo.token-estimate/v2` (10 §7.5): integers only, deterministic, built so
/// that Chinese text is not under-counted by the word-based v1 estimate.
///
/// * every CJK ideograph, kana or hangul character is 1 token;
/// * every other run of letters and digits is `max(1, ceil(chars / 4))`;
/// * every other non-whitespace character is 1 token; whitespace is 0.
pub fn cjk_calibrated_tokens(text: &str) -> u64 {
    tokens_of_classes(text.chars().map(char_class))
}

/// What the `v2` estimate sees of a character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharClass {
    /// CJK ideograph, kana or hangul: one token by itself.
    Cjk,
    /// Letter or digit: part of a run.
    Alnum,
    /// Whitespace: ends a run, costs nothing.
    Space,
    /// Anything else: ends a run, one token by itself.
    Other,
}

/// The class of one character.
pub fn char_class(ch: char) -> CharClass {
    if is_cjk_like(ch) {
        CharClass::Cjk
    } else if ch.is_alphanumeric() {
        CharClass::Alnum
    } else if ch.is_whitespace() {
        CharClass::Space
    } else {
        CharClass::Other
    }
}

/// The estimate over a stream of classes. Kani proves it on class sequences
/// (`oo-docs` `compile/proofs.rs`, KP6): never more than the two parts of a
/// concatenation together, at most one less.
pub fn tokens_of_classes(classes: impl Iterator<Item = CharClass>) -> u64 {
    let mut total = 0_u64;
    let mut run = 0_u64;
    let flush = |run: &mut u64, total: &mut u64| {
        if *run > 0 {
            *total = total.saturating_add((*run).div_ceil(4).max(1));
            *run = 0;
        }
    };
    for c in classes {
        match c {
            CharClass::Cjk => {
                flush(&mut run, &mut total);
                total = total.saturating_add(1);
            }
            CharClass::Alnum => run += 1,
            CharClass::Space => flush(&mut run, &mut total),
            CharClass::Other => {
                flush(&mut run, &mut total);
                total = total.saturating_add(1);
            }
        }
    }
    flush(&mut run, &mut total);
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_is_one_token_per_character() {
        assert_eq!(cjk_calibrated_tokens("星图看板"), 4);
        assert_eq!(cjk_calibrated_tokens("こんにちは"), 5);
        assert_eq!(cjk_calibrated_tokens("한국어"), 3);
        assert_eq!(cjk_calibrated_tokens("星图 看板"), 4, "whitespace is free");
        // v1 counts a CJK run as one word; v2 must not.
        assert!(
            cjk_calibrated_tokens("星图看板改用开源图表库")
                > unicode_word_tokens("星图看板改用开源图表库")
        );
    }

    #[test]
    fn alphanumeric_runs_are_a_quarter_of_their_length_rounded_up() {
        assert_eq!(cjk_calibrated_tokens("a"), 1);
        assert_eq!(cjk_calibrated_tokens("abcd"), 1);
        assert_eq!(cjk_calibrated_tokens("abcde"), 2);
        assert_eq!(cjk_calibrated_tokens("Starmap"), 2);
        assert_eq!(cjk_calibrated_tokens("2026"), 1);
        assert_eq!(
            cjk_calibrated_tokens("2026-01-01"),
            3 + 2,
            "three runs of 4, 2, 2 chars is 1+1+1, plus two dashes"
        );
    }

    #[test]
    fn mixed_text_and_punctuation() {
        // 陈子墨 (3) + 的 (1) + Ethan (2) + ， (1) + 小陈 (2)
        assert_eq!(cjk_calibrated_tokens("陈子墨的Ethan，小陈"), 9);
        // A CJK character ends an alphanumeric run.
        assert_eq!(cjk_calibrated_tokens("abc星abc"), 1 + 1 + 1);
        // [ src : a # L1 - L2 ]
        assert_eq!(cjk_calibrated_tokens("[src:a#L1-L2]"), 9);
        assert_eq!(cjk_calibrated_tokens(""), 0);
        assert_eq!(cjk_calibrated_tokens(" \n\t"), 0);
    }

    #[test]
    fn never_below_the_cjk_character_count() {
        let samples = [
            "周报改为每周四发送",
            "决定：星图看板改用开源图表库",
            "Ethan Chen，团队里也叫小陈。",
        ];
        for s in samples {
            let cjk = s.chars().filter(|c| is_cjk_like(*c)).count() as u64;
            assert!(cjk_calibrated_tokens(s) >= cjk, "{s}");
        }
    }
}
