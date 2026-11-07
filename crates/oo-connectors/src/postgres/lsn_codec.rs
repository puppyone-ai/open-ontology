//! Dependency-free codec shared with the standalone preview during migration.
pub fn parse(text: &str) -> Option<u64> {
    let (hi, lo) = text.split_once('/')?;
    let half = |s: &str| {
        if s.is_empty() || s.len() > 8 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        u64::from_str_radix(s, 16).ok()
    };
    Some((half(hi)? << 32) | half(lo)?)
}

pub fn format(value: u64) -> String {
    format!("{:X}/{:X}", value >> 32, value & 0xFFFF_FFFF)
}
