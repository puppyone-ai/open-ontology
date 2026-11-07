//! Locator reads are bounded by the chunk's canonical-byte budget, never SUBSTR.
use std::io::Read;
use std::time::Instant;

use crate::SourceError;

use super::worker::DriverError;

pub(super) fn read_locator(
    lob: &mut oracledb::Lob,
    binary: bool,
    limit: u64,
    deadline: Instant,
) -> Result<String, DriverError> {
    let units = lob.get_size()? as u64;
    if units.saturating_mul(if binary { 2 } else { 1 }) > limit {
        return Err(DriverError::Source(SourceError::budget(
            "lob_value_over_budget",
        )));
    }
    let value = read(lob, binary, limit, deadline)?;
    let read_units = if binary {
        value.len() / 2
    } else {
        value.encode_utf16().count()
    };
    if read_units as u64 != units {
        return Err(DriverError::Source(SourceError::protocol(
            "lob_incomplete_read",
        )));
    }
    Ok(value)
}

pub(super) fn read(
    reader: &mut impl Read,
    binary: bool,
    limit: u64,
    deadline: Instant,
) -> Result<String, DriverError> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        if Instant::now() > deadline {
            return Err(DriverError::Source(SourceError::budget("lob_deadline")));
        }
        let count = reader
            .read(&mut buffer)
            .map_err(|_| DriverError::Source(SourceError::protocol("lob_read_failed")))?;
        if count == 0 {
            break;
        }
        let canonical_bytes = (bytes.len() as u64 + count as u64) * if binary { 2 } else { 1 };
        if canonical_bytes > limit {
            return Err(DriverError::Source(SourceError::budget(
                "lob_value_over_budget",
            )));
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    if binary {
        Ok(super::chunk::hex(&bytes))
    } else {
        String::from_utf8(bytes)
            .map_err(|_| DriverError::Source(SourceError::protocol("lob_invalid_utf8")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn bounded_multichunk_text_binary_empty_and_overflow() {
        let deadline = Instant::now() + Duration::from_secs(10);
        let text = "\u{4e2d}\u{1f600}\0".repeat(5000);
        assert_eq!(
            read(&mut text.as_bytes(), false, text.len() as u64, deadline).unwrap(),
            text
        );
        assert!(read(&mut text.as_bytes(), false, text.len() as u64 - 1, deadline).is_err());
        let bytes = vec![255; 20_000];
        assert_eq!(
            read(&mut bytes.as_slice(), true, 40_000, deadline).unwrap(),
            "ff".repeat(20_000)
        );
        assert!(read(&mut bytes.as_slice(), true, 39_999, deadline).is_err());
        assert_eq!(read(&mut &b""[..], false, 0, deadline).unwrap(), "");
        assert!(read(&mut &b"\xff"[..], false, 1, deadline).is_err());
        assert!(read(&mut &b"x"[..], false, 1, Instant::now()).is_err());
    }
}
