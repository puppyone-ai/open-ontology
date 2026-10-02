use std::io::{Read, Write};

use crate::limits::{MAX_FRAME_BODY, PROTOCOL_VERSION};

pub const MAGIC: [u8; 4] = *b"P05P";

pub const KIND_READY: u16 = 1;
pub const KIND_ADMIT: u16 = 2;
pub const KIND_RESPONSE: u16 = 3;
pub const KIND_SPIN: u16 = 10;
pub const KIND_CRASH: u16 = 11;
pub const KIND_GARBAGE: u16 = 12;
pub const KIND_FAKE_ALLOW: u16 = 13;

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub version: u16,
    pub kind: u16,
    pub body: Vec<u8>,
}

pub fn write_frame(mut w: impl Write, kind: u16, body: &[u8]) -> Result<(), &'static str> {
    if body.len() > MAX_FRAME_BODY {
        return Err("RESPONSE_CAP");
    }
    w.write_all(&MAGIC).map_err(|_| "INVALID_OUTPUT")?;
    w.write_all(&PROTOCOL_VERSION.to_le_bytes()).map_err(|_| "INVALID_OUTPUT")?;
    w.write_all(&kind.to_le_bytes()).map_err(|_| "INVALID_OUTPUT")?;
    w.write_all(&(body.len() as u32).to_le_bytes()).map_err(|_| "INVALID_OUTPUT")?;
    w.write_all(body).map_err(|_| "INVALID_OUTPUT")?;
    w.flush().map_err(|_| "INVALID_OUTPUT")?;
    Ok(())
}

pub fn read_frame(mut r: impl Read) -> Result<Frame, &'static str> {
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic).map_err(|_| "INVALID_OUTPUT")?;
    if magic != MAGIC {
        return Err("INVALID_OUTPUT");
    }
    let mut buf2 = [0u8; 2];
    r.read_exact(&mut buf2).map_err(|_| "INVALID_OUTPUT")?;
    let version = u16::from_le_bytes(buf2);
    if version != PROTOCOL_VERSION {
        return Err("INVALID_OUTPUT");
    }
    r.read_exact(&mut buf2).map_err(|_| "INVALID_OUTPUT")?;
    let kind = u16::from_le_bytes(buf2);
    let mut buf4 = [0u8; 4];
    r.read_exact(&mut buf4).map_err(|_| "INVALID_OUTPUT")?;
    let len = u32::from_le_bytes(buf4) as usize;
    if len > MAX_FRAME_BODY {
        return Err("RESPONSE_CAP");
    }
    let mut body = vec![0u8; len];
    if len > 0 {
        r.read_exact(&mut body).map_err(|_| "INVALID_OUTPUT")?;
    }
    Ok(Frame { version, kind, body })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::{MAX_FRAME_BODY, PROTOCOL_VERSION};
    use std::io::Cursor;

    #[test]
    fn roundtrip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, KIND_ADMIT, b"hi").unwrap();
        let frame = read_frame(Cursor::new(buf)).unwrap();
        assert_eq!(frame.version, PROTOCOL_VERSION);
        assert_eq!(frame.kind, KIND_ADMIT);
        assert_eq!(frame.body, b"hi");
    }

    #[test]
    fn bad_magic_and_version_are_invalid() {
        assert_eq!(read_frame(Cursor::new(b"XXXX".to_vec())).unwrap_err(), "INVALID_OUTPUT");
        let mut buf = Vec::new();
        buf.extend_from_slice(&MAGIC);
        buf.extend_from_slice(&2u16.to_le_bytes());
        buf.extend_from_slice(&KIND_READY.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(read_frame(Cursor::new(buf)).unwrap_err(), "INVALID_OUTPUT");
    }

    #[test]
    fn length_above_cap_is_rejected_before_the_body() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&MAGIC);
        buf.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
        buf.extend_from_slice(&KIND_RESPONSE.to_le_bytes());
        buf.extend_from_slice(&((MAX_FRAME_BODY as u32) + 1).to_le_bytes());
        assert_eq!(read_frame(Cursor::new(buf)).unwrap_err(), "RESPONSE_CAP");
        let mut huge = vec![0u8; MAX_FRAME_BODY + 1];
        huge[0] = b'x';
        assert_eq!(write_frame(&mut Vec::new(), KIND_RESPONSE, &huge).unwrap_err(), "RESPONSE_CAP");
    }
}
