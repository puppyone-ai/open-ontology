//! Identifier checks shared by the API and the producer requests.

pub fn ident_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

pub fn host_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=253).contains(&bytes.len())
        && bytes.iter().all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'_' | b'-'))
}

pub fn hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub fn secret_ref_ok(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("secret://") else {
        return false;
    };
    (1..=128).contains(&rest.len())
        && rest.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'/' | b'-'))
}

pub fn session_cookie_ok(value: &str) -> bool {
    value.len() == 43
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub fn idempotency_ok(value: &str) -> bool {
    (16..=128).contains(&value.len())
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub fn operation_id_ok(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("op_") else {
        return false;
    };
    rest.len() == 32 && rest.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub fn uuid_ok(value: &str) -> bool {
    let parts: Vec<&str> = value.split('-').collect();
    parts.len() == 5
        && parts[0].len() == 8
        && parts[1].len() == 4
        && parts[2].len() == 4
        && parts[3].len() == 4
        && parts[4].len() == 12
        && parts.iter().all(|part| part.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()))
}

pub fn plain_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/' | b':'))
}

pub fn token_detail(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'.'))
}

pub fn ct_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0_u8;
    for (one, other) in left.iter().zip(right.iter()) {
        diff |= one ^ other;
    }
    diff == 0
}

pub fn new_operation_id() -> Option<String> {
    let mut bytes = [0_u8; 16];
    let mut file = std::fs::File::open("/dev/urandom").ok()?;
    std::io::Read::read_exact(&mut file, &mut bytes).ok()?;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::from("op_");
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Some(out)
}
