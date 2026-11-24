//! Strict text decoding (10 §4.4.1): UTF-8 (BOM allowed), UTF-16 with BOM.
//! GB18030 is detected as "not decodable" and reported as
//! `DOCS_PARSE_ENCODING`; it needs `encoding_rs` (not admitted yet).

use oo_core::docir::TextEncoding;

use super::ParseError;

pub struct Decoded {
    pub text: String,
    pub encoding: TextEncoding,
}

pub fn decode_text(bytes: &[u8]) -> Result<Decoded, ParseError> {
    let (encoding, text) = if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        let s = std::str::from_utf8(rest).map_err(|_| ParseError::Encoding("invalid UTF-8"))?;
        (TextEncoding::Utf8Bom, s.to_owned())
    } else if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        (TextEncoding::Utf16Le, utf16(rest, u16::from_le_bytes)?)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        (TextEncoding::Utf16Be, utf16(rest, u16::from_be_bytes)?)
    } else {
        let s = std::str::from_utf8(bytes)
            .map_err(|_| ParseError::Encoding("not UTF-8; GB18030 decoding is not available"))?;
        (TextEncoding::Utf8, s.to_owned())
    };
    if text.contains('\0') {
        return Err(ParseError::NotText);
    }
    Ok(Decoded { text, encoding })
}

fn utf16(bytes: &[u8], from: fn([u8; 2]) -> u16) -> Result<String, ParseError> {
    if !bytes.len().is_multiple_of(2) {
        return Err(ParseError::Encoding("odd UTF-16 length"));
    }
    let units = bytes.as_chunks::<2>().0.iter().map(|c| from(*c));
    char::decode_utf16(units)
        .collect::<Result<String, _>>()
        .map_err(|_| ParseError::Encoding("invalid UTF-16"))
}
