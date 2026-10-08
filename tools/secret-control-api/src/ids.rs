//! Identifier grammar for references, cookies, and idempotency keys.

pub fn ident_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

pub fn deployment_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    let Some(first) = bytes.first() else {
        return false;
    };
    bytes.len() <= 63
        && (first.is_ascii_lowercase() || first.is_ascii_digit())
        && bytes.iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-')
}

pub fn hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

pub fn idempotency_ok(value: &str) -> bool {
    (16..=128).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub fn session_cookie_ok(value: &str) -> bool {
    value.len() == 43 && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub fn subject_ok(value: &str) -> bool {
    (1..=256).contains(&value.len()) && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
}

pub fn workload_ok(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("urn:oo:workload:") else {
        return false;
    };
    (1..=128).contains(&rest.len())
        && rest.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-'))
}

pub fn ct_eq(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    if left.len() != right.len() {
        return false;
    }
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, b"oo-secret-eq");
    let tag = ring::hmac::sign(&key, left);
    ring::hmac::verify(&key, right, tag.as_ref()).is_ok()
}

pub fn path_ok(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('\0')
        && value.split(['/', '\\']).all(|part| part != "..")
        && value.len() <= 512
}
