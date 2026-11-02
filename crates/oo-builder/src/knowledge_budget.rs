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
