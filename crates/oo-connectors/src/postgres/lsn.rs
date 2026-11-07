//! `pg_lsn` text codec. Converged from `tools/postgres-preview/src/capture.rs`
//! (`parse_lsn`, `format_lsn`) so watermarks and the later C1 reader share it.

use crate::SourceError;

#[path = "lsn_codec.rs"]
mod codec;

/// Parse two hexadecimal 32-bit halves, without accepting overflow.
pub fn parse_lsn(text: &str) -> Result<u64, SourceError> {
    codec::parse(text).ok_or_else(|| SourceError::protocol("lsn"))
}

/// Format as PostgreSQL prints a `pg_lsn`.
pub fn format_lsn(value: u64) -> String {
    codec::format(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for text in ["0/0", "0/16B3748", "1/0", "FFFFFFFF/FFFFFFFF"] {
            assert_eq!(format_lsn(parse_lsn(text).unwrap()), text);
        }
        assert_eq!(parse_lsn("0/16B3748").unwrap(), 0x16B_3748);
        assert_eq!(parse_lsn("1/2").unwrap(), (1 << 32) | 2);
    }

    #[test]
    fn rejects_malformed() {
        for text in ["", "0", "/1", "1/", "g/0", "100000000/0", "0/0/0", " 0/0"] {
            assert!(parse_lsn(text).is_err(), "{text}");
        }
    }
}
