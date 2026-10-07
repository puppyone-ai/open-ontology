use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::Error;

pub(crate) fn identity_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 {
        return false;
    }
    if !bytes[0].is_ascii_lowercase() {
        return false;
    }
    bytes.iter().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-'
    })
}

pub(crate) fn token_ok(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b' ' && byte != b'=')
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

pub(crate) fn hex_decode(value: &str) -> Result<Vec<u8>, Error> {
    if !value.len().is_multiple_of(2) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::Corrupt);
    }
    let mut out = Vec::with_capacity(value.len() / 2);
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let hi = hex_nibble(bytes[index])?;
        let lo = hex_nibble(bytes[index + 1])?;
        out.push((hi << 4) | lo);
        index += 2;
    }
    Ok(out)
}

fn hex_nibble(byte: u8) -> Result<u8, Error> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(Error::Corrupt),
    }
}

pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

pub(crate) fn domain_id(parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"oo.artifact-retention/v1 registration\0");
    for part in parts {
        hasher.update(part);
        hasher.update(b"\0");
    }
    hex_encode(&hasher.finalize())
}

pub(crate) fn write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), Error> {
    if name.contains('/') || name.contains('\0') {
        return Err(Error::Corrupt);
    }
    let final_path = dir.join(name);
    let tmp_path = dir.join(format!("{name}.tmp"));
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp_path)
            .map_err(|_| Error::Store)?;
        file.write_all(bytes).map_err(|_| Error::Store)?;
        file.sync_all().map_err(|_| Error::Store)?;
    }
    std::fs::rename(&tmp_path, &final_path).map_err(|_| Error::Store)?;
    sync_dir(dir)
}

pub(crate) fn write_hashed(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), Error> {
    write_atomic(dir, name, bytes)?;
    let digest = format!("{}\n", hex_encode(&sha256(bytes)));
    write_atomic(dir, &format!("{name}.sha256"), digest.as_bytes())
}

pub(crate) fn read_hashed(dir: &Path, name: &str) -> Result<Vec<u8>, Error> {
    let body = std::fs::read(dir.join(name)).map_err(|_| Error::Corrupt)?;
    let side = std::fs::read(dir.join(format!("{name}.sha256"))).map_err(|_| Error::Corrupt)?;
    let side = std::str::from_utf8(&side).map_err(|_| Error::Corrupt)?;
    let expected = side.strip_suffix('\n').unwrap_or(side);
    if expected != hex_encode(&sha256(&body)) {
        return Err(Error::Corrupt);
    }
    Ok(body)
}

pub(crate) fn sync_dir(dir: &Path) -> Result<(), Error> {
    let file = std::fs::File::open(dir).map_err(|_| Error::Store)?;
    file.sync_all().map_err(|_| Error::Store)?;
    Ok(())
}
