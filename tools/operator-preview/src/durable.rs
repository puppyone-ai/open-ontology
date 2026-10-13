//! Durable files: temp, fsync, rename, directory fsync.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

use crate::output::Fail;

pub fn write_durable(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), Fail> {
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return Err(Fail::Request);
    }
    fs::create_dir_all(dir).map_err(|_| Fail::Io)?;
    let tmp = dir.join(format!(".{name}.tmp"));
    let final_path = dir.join(name);
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)
            .map_err(|_| Fail::Io)?;
        file.write_all(bytes).map_err(|_| Fail::Io)?;
        file.sync_all().map_err(|_| Fail::Io)?;
    }
    fs::rename(&tmp, &final_path).map_err(|_| Fail::Io)?;
    File::open(dir)
        .and_then(|dir_file| dir_file.sync_all())
        .map_err(|_| Fail::Io)?;
    Ok(())
}

pub fn read_capped(path: &Path, max: usize) -> Result<Vec<u8>, Fail> {
    refuse_symlink(path)?;
    let file = File::open(path).map_err(|_| Fail::Io)?;
    let meta = file.metadata().map_err(|_| Fail::Io)?;
    if !meta.is_file() {
        return Err(Fail::Request);
    }
    let len = usize::try_from(meta.len()).map_err(|_| Fail::Request)?;
    if len == 0 || len > max {
        return Err(Fail::Request);
    }
    let bytes = fs::read(path).map_err(|_| Fail::Io)?;
    if bytes.len() != len {
        return Err(Fail::Io);
    }
    Ok(bytes)
}

pub fn refuse_symlink(path: &Path) -> Result<(), Fail> {
    let meta = fs::symlink_metadata(path).map_err(|_| Fail::Io)?;
    if meta.file_type().is_symlink() {
        Err(Fail::Request)
    } else {
        Ok(())
    }
}

pub fn execution_id() -> Result<String, Fail> {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|_| Fail::Io)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(format_uuid(&bytes))
}

pub fn execution_id_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (index, byte) in bytes.iter().enumerate() {
        let hyphen = matches!(index, 8 | 13 | 18 | 23);
        if hyphen {
            if *byte != b'-' {
                return false;
            }
            continue;
        }
        if !byte.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

fn format_uuid(bytes: &[u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(36);
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
        hex.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::{execution_id, execution_id_ok};

    #[test]
    fn execution_ids_differ_and_parse() {
        let left = execution_id().expect("id");
        let right = execution_id().expect("id");
        assert_ne!(left, right);
        assert!(execution_id_ok(&left));
        assert!(!execution_id_ok("../etc"));
        assert!(!execution_id_ok("not-an-id"));
    }
}
