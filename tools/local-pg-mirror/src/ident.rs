use crate::error::MirrorError;

/// Quote a catalog identifier. The pattern is the contract's closed set.
pub fn quote_ident(name: &str) -> Result<String, MirrorError> {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 {
        return Err(MirrorError::new("ident", "ident"));
    }
    let first = bytes[0];
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return Err(MirrorError::new("ident", "ident"));
    }
    if !bytes[1..]
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        return Err(MirrorError::new("ident", "ident"));
    }
    Ok(format!("\"{name}\""))
}

pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
