pub fn slug(value: &str, max: usize) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= max
        && (bytes[0] == b'_' || bytes[0].is_ascii_lowercase())
        && bytes[1..]
            .iter()
            .all(|byte| *byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

/// Role-pack knowledge slug. Length 1..=63. The first byte is an ASCII
/// lowercase letter. Later bytes are lowercase letters, digits, `_`, or `-`.
pub fn knowledge_id_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-'
        })
}

pub fn role_name(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("oo_r_") else {
        return false;
    };
    value.len() <= 63 && (1..=58).contains(&rest.len()) && slug(rest, 58)
}

pub fn version_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-'))
}

pub fn model_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=128).contains(&bytes.len()) && version_token_bytes(bytes)
}

fn version_token_bytes(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-'))
}

pub fn hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn as_of(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 20 {
        return false;
    }
    let digits = |range: std::ops::Range<usize>| bytes[range].iter().all(|byte| byte.is_ascii_digit());
    digits(0..4)
        && bytes[4] == b'-'
        && digits(5..7)
        && bytes[7] == b'-'
        && digits(8..10)
        && bytes[10] == b'T'
        && digits(11..13)
        && bytes[13] == b':'
        && digits(14..16)
        && bytes[16] == b':'
        && digits(17..19)
        && bytes[19] == b'Z'
        && (1..=12).contains(&number(&bytes[5..7]))
        && (1..=31).contains(&number(&bytes[8..10]))
        && number(&bytes[11..13]) <= 23
        && number(&bytes[14..16]) <= 59
        && number(&bytes[17..19]) <= 59
}

fn number(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0, |acc, byte| acc * 10 + u32::from(byte - b'0'))
}

pub fn caller_label(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_alphanumeric()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'-')
}

pub fn endpoint_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=256).contains(&bytes.len())
        && value.starts_with('/')
        && !value.contains("//")
        && !value.contains('\\')
        && !value.contains('?')
        && !value.contains('#')
        && !value.split('/').any(|segment| segment == "..")
}

pub fn development_local_origin(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("http://127.0.0.1:") else {
        return false;
    };
    if rest.is_empty() || rest.len() > 5 || rest.as_bytes()[0] == b'0' {
        return false;
    }
    if !rest.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    matches!(rest.parse::<u16>(), Ok(port) if port != 0)
}

pub fn origin_ok(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("https://") else {
        return false;
    };
    if rest.is_empty() || rest.contains('@') || rest.contains('?') || rest.contains('#') || rest.contains('\\')
    {
        return false;
    }
    let (hostport, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, ""),
    };
    if path != "/" && !path.is_empty() {
        return false;
    }
    if hostport.is_empty() || hostport.starts_with(':') || hostport.ends_with(':') {
        return false;
    }
    true
}

pub fn join_origin(origin: &str, path: &str) -> String {
    let mut base = origin.to_string();
    if base.ends_with('/') {
        base.pop();
    }
    base.push_str(path);
    base
}

pub fn in_range(value: u64, min: u64, max: u64) -> bool {
    (min..=max).contains(&value)
}
