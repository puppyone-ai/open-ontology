use crate::facts::{GB18030_RARE_HEX, GBK_HX_HEX};

pub fn gbk_hx() -> Vec<u8> {
    decode_hex(GBK_HX_HEX)
}

pub fn gb18030_rare() -> Vec<u8> {
    decode_hex(GB18030_RARE_HEX)
}

pub fn decode_hex(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut index = 0;
    while index + 1 < bytes.len() {
        let hi = hex_val(bytes[index]);
        let lo = hex_val(bytes[index + 1]);
        out.push((hi << 4) | lo);
        index += 2;
    }
    out
}

fn hex_val(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::{gb18030_rare, gbk_hx};

    #[test]
    fn bytes_are_not_utf8() {
        assert!(std::str::from_utf8(&gbk_hx()).is_err());
        assert_eq!(gbk_hx().len(), 12);
        assert_eq!(gb18030_rare(), vec![0x95, 0x32, 0x82, 0x36]);
        assert!(std::str::from_utf8(&gb18030_rare()).is_err());
    }
}
