//! GB 32100-2015 unified social credit identifier checksum.
//! The 18th character is a base-31 check digit. This is not a network lookup.

use crate::error::{Result, err};

const CHARSET: &[u8; 31] = b"0123456789ABCDEFGHJKLMNPQRTUWXY";
const WEIGHTS: [u32; 17] = [
    1, 3, 9, 27, 19, 26, 16, 17, 20, 29, 25, 13, 8, 24, 10, 30, 28,
];

pub fn checksum_char(body17: &str) -> Result<char> {
    let bytes = body17.as_bytes();
    if bytes.len() != 17 {
        return Err(err("uscc body must be 17 characters"));
    }
    let mut total = 0u32;
    for (index, byte) in bytes.iter().enumerate() {
        let value = CHARSET
            .iter()
            .position(|item| item == byte)
            .ok_or_else(|| err("uscc body has a character outside the alphabet"))?;
        total += u32::try_from(value).unwrap_or(0) * WEIGHTS[index];
    }
    let remainder = (31 - (total % 31)) % 31;
    Ok(CHARSET[remainder as usize] as char)
}

pub fn complete(body17: &str) -> Result<String> {
    let check = checksum_char(body17)?;
    Ok(format!("{body17}{check}"))
}

pub fn synthetic(rng: &mut crate::prng::SplitMix64) -> Result<String> {
    // GB 32100 registration departments are only 1, 5, 9, and Y.
    const DEPTS: &[u8] = b"159Y";
    let mut body = String::with_capacity(17);
    let dept = usize::try_from(rng.gen_range(4)).unwrap_or(0);
    body.push(DEPTS[dept] as char);
    for _ in 0..16 {
        let index = usize::try_from(rng.gen_range(31)).unwrap_or(0);
        body.push(CHARSET[index] as char);
    }
    complete(&body)
}

pub fn is_valid(code: &str) -> bool {
    let bytes = code.as_bytes();
    if bytes.len() != 18 || !matches!(bytes[0], b'1' | b'5' | b'9' | b'Y') {
        return false;
    }
    checksum_char(&code[..17])
        .map(|check| check == bytes[17] as char)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{complete, is_valid};

    #[test]
    fn known_vector() {
        let code = complete("91350100M000100Y4").unwrap();
        assert_eq!(code, "91350100M000100Y43");
        assert!(is_valid(&code));
        assert!(!is_valid("91350100M000100Y44"));
        assert!(!is_valid("000000000000000000"));
        assert!(!is_valid("310101234567890"));
    }
}
