//! Path checks. A symlink is refused before a byte is read.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::crypto::{CHUNK, MAX_PLAINTEXT};
use crate::error::Fault;

pub fn intake_name_ok(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && !name.contains("..")
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains(':')
        && bytes.iter().all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'_' | b'-'))
}

pub fn column_ok(name: &str) -> bool {
    !name.is_empty() && name.len() <= 256 && !name.bytes().any(|byte| byte == 0) && !name.contains("://")
}

pub fn idempotency_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    (16..=128).contains(&bytes.len()) && bytes.iter().all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'-')
}

pub fn object_id_ok(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub fn intake_file(root: &Path, name: &str) -> Result<PathBuf, Fault> {
    if !intake_name_ok(name) {
        return Err(Fault::path());
    }
    directory_private(root)?;
    let path = root.join(name);
    let meta = fs::symlink_metadata(&path).map_err(|_| Fault::path())?;
    if meta.file_type().is_symlink() || !meta.file_type().is_file() {
        return Err(Fault::path());
    }
    if meta.len() == 0 || meta.len() > MAX_PLAINTEXT as u64 {
        return Err(Fault::budget());
    }
    Ok(path)
}

pub fn executable_file(path: &Path) -> Result<(), Fault> {
    let meta = fs::symlink_metadata(path).map_err(|_| Fault::path())?;
    if meta.file_type().is_symlink() || !meta.file_type().is_file() {
        return Err(Fault::path());
    }
    if meta.permissions().mode() & 0o111 == 0 {
        return Err(Fault::path());
    }
    Ok(())
}

pub fn directory_private(path: &Path) -> Result<(), Fault> {
    let meta = fs::symlink_metadata(path).map_err(|_| Fault::path())?;
    if meta.file_type().is_symlink() || !meta.file_type().is_dir() {
        return Err(Fault::path());
    }
    if meta.permissions().mode() & 0o777 != 0o700 {
        return Err(Fault::path());
    }
    Ok(())
}

pub fn read_regular(path: &Path, max: usize, mode: Option<u32>) -> Result<Vec<u8>, Fault> {
    let meta = fs::symlink_metadata(path).map_err(|_| Fault::path())?;
    if meta.file_type().is_symlink() || !meta.file_type().is_file() {
        return Err(Fault::path());
    }
    if let Some(expected) = mode
        && meta.permissions().mode() & 0o777 != expected
    {
        return Err(Fault::path());
    }
    if meta.len() > max as u64 {
        return Err(Fault::budget());
    }
    let mut file = File::open(path).map_err(|_| Fault::path())?;
    let mut out = Vec::new();
    let mut buf = [0_u8; CHUNK];
    loop {
        let read = file.read(&mut buf).map_err(|_| Fault::path())?;
        if read == 0 {
            break;
        }
        if out.len() + read > max {
            return Err(Fault::budget());
        }
        out.extend_from_slice(&buf[..read]);
    }
    Ok(out)
}

pub fn replace_private(path: &Path, bytes: &[u8]) -> Result<(), Fault> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.file_type().is_file() => return Err(Fault::path()),
        Ok(_) => fs::remove_file(path).map_err(|_| Fault::path())?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(Fault::path()),
    }
    write_private(path, bytes)
}

pub fn write_private(path: &Path, bytes: &[u8]) -> Result<(), Fault> {
    if let Some(parent) = path.parent()
        && parent.as_os_str().is_empty()
    {
        return Err(Fault::path());
    }
    if fs::symlink_metadata(path).is_ok() {
        return Err(Fault::path());
    }
    let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(path).map_err(|_| Fault::path())?;
    file.write_all(bytes).map_err(|_| Fault::path())?;
    file.sync_all().map_err(|_| Fault::path())?;
    Ok(())
}

pub fn write_ciphertext(dir: &Path, object_id: &str, bytes: &[u8]) -> Result<(), Fault> {
    if !object_id_ok(object_id) {
        return Err(Fault::path());
    }
    directory_private(dir)?;
    let partial = dir.join(format!(".{object_id}.partial"));
    let final_path = dir.join(format!("{object_id}.bin"));
    if fs::symlink_metadata(&partial).is_ok() || fs::symlink_metadata(&final_path).is_ok() {
        return Err(Fault::path());
    }
    let result = write_partial(&partial, &final_path, bytes);
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}

fn write_partial(partial: &Path, final_path: &Path, bytes: &[u8]) -> Result<(), Fault> {
    let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(partial).map_err(|_| Fault::path())?;
    let mut offset = 0;
    while offset < bytes.len() {
        let end = (offset + CHUNK).min(bytes.len());
        file.write_all(&bytes[offset..end]).map_err(|_| Fault::path())?;
        offset = end;
    }
    file.sync_all().map_err(|_| Fault::path())?;
    drop(file);
    fs::rename(partial, final_path).map_err(|_| Fault::path())?;
    Ok(())
}

pub fn read_ciphertext(dir: &Path, object_id: &str) -> Result<Vec<u8>, Fault> {
    if !object_id_ok(object_id) {
        return Err(Fault::path());
    }
    let path = dir.join(format!("{object_id}.bin"));
    read_regular(&path, MAX_PLAINTEXT + 16, Some(0o600))
}

pub fn remove_object(dir: &Path, object_id: &str) -> Result<(), Fault> {
    if !object_id_ok(object_id) {
        return Err(Fault::path());
    }
    let path = dir.join(format!("{object_id}.bin"));
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() || meta.file_type().is_file() => {
            fs::remove_file(&path).map_err(|_| Fault::path())?;
            Ok(())
        }
        Ok(_) => Err(Fault::path()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(Fault::path()),
    }
}

pub fn remove_staging(object_dir: &Path, dir: &Path) -> Result<(), Fault> {
    let name = dir.file_name().and_then(|value| value.to_str()).ok_or_else(Fault::path)?;
    if !object_id_ok(name) {
        return Err(Fault::path());
    }
    let expected = object_dir.join("staging").join(name);
    if dir != expected {
        return Err(Fault::path());
    }
    let meta = fs::symlink_metadata(dir).map_err(|_| Fault::path())?;
    if meta.file_type().is_symlink() || !meta.file_type().is_dir() {
        return Err(Fault::path());
    }
    let entries = fs::read_dir(dir).map_err(|_| Fault::path())?;
    for entry in entries {
        let entry = entry.map_err(|_| Fault::path())?;
        let meta = fs::symlink_metadata(entry.path()).map_err(|_| Fault::path())?;
        if meta.file_type().is_file() || meta.file_type().is_symlink() {
            fs::remove_file(entry.path()).map_err(|_| Fault::path())?;
        } else {
            return Err(Fault::path());
        }
    }
    fs::remove_dir(dir).map_err(|_| Fault::path())
}
