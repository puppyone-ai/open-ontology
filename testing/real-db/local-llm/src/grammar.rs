pub const DIALOGUE_BEARER_MIN: usize = 16;
pub const DIALOGUE_BEARER_MAX: usize = 256;
pub const BEARER_MAX: usize = 512;

pub fn model_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=128).contains(&bytes.len())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-'))
}

pub fn dialogue_bearer_class(len: usize) -> &'static str {
    if (DIALOGUE_BEARER_MIN..=DIALOGUE_BEARER_MAX).contains(&len) {
        "ok"
    } else if len < DIALOGUE_BEARER_MIN {
        "below_minimum"
    } else {
        "above_maximum"
    }
}

pub fn visible_ascii(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.len() <= BEARER_MAX && bytes.iter().all(|byte| (0x21..=0x7e).contains(byte))
}

#[cfg(test)]
mod tests {
    use super::dialogue_bearer_class;
    use super::model_id;

    #[test]
    fn model_id_matches_dialogue_rule() {
        assert!(model_id("gpt-4.1"));
        assert!(model_id("Model_1"));
        assert!(model_id("a+b"));
        assert!(!model_id("openai/gpt"));
        assert!(!model_id(""));
        assert!(!model_id("a b"));
        assert!(!model_id(&"a".repeat(129)));
    }

    #[test]
    fn bearer_class_uses_dialogue_bounds() {
        assert_eq!(dialogue_bearer_class(6), "below_minimum");
        assert_eq!(dialogue_bearer_class(16), "ok");
        assert_eq!(dialogue_bearer_class(256), "ok");
        assert_eq!(dialogue_bearer_class(257), "above_maximum");
    }
}
