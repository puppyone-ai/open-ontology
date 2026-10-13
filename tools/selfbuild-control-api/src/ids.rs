//! Identifier checks shared by the HTTP surface and the ledger files.

pub fn ident_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

pub fn hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

pub fn build_id_ok(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("b_") else {
        return false;
    };
    rest.len() == 32 && rest.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

pub fn idempotency_ok(value: &str) -> bool {
    (16..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub fn secret_ref_ok(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("secret://") else {
        return false;
    };
    (1..=128).contains(&rest.len())
        && rest
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'/' | b'-'))
}

pub fn object_id_ok(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    (parts.len() == 2 || parts.len() == 3) && parts.iter().all(|part| source_ident_ok(part))
}

pub fn session_cookie_ok(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub fn ct_eq(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (one, two) in left.iter().zip(right.iter()) {
        diff |= one ^ two;
    }
    diff == 0
}

/// Original SQL/document names; separate from control object identifiers.
pub fn source_ident_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty() && bytes.len() <= 128
        && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        && bytes.iter().all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}
