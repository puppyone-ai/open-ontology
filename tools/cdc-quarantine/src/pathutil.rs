//! Regular-file helpers. Symlinks are not followed and nothing is deleted.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path};

use crate::model::FaultCode;

#[cfg(target_os = "linux")]
const O_NOFOLLOW: i32 = 0x20000;
#[cfg(target_os = "macos")]
const O_NOFOLLOW: i32 = 0x0100;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
const O_NOFOLLOW: i32 = 0;

pub fn check_store_path(path: &Path) -> Result<(), FaultCode> {
    if path.as_os_str().is_empty() {
        return Err(FaultCode::PathRefused);
    }
    let mut prefix = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::ParentDir => return Err(FaultCode::PathRefused),
            Component::RootDir => {
                prefix.push(component);
                continue;
            }
            Component::CurDir => continue,
            Component::Normal(_) => prefix.push(component),
        }
        if prefix.as_os_str().is_empty() {
            return Err(FaultCode::PathRefused);
        }
        match fs::symlink_metadata(&prefix) {
            Ok(meta) if meta.file_type().is_symlink() => return Err(FaultCode::PathRefused),
            Ok(meta) if prefix == path => {
                if meta.is_file() {
                    return Err(FaultCode::PathRefused);
                }
                if !meta.is_dir() {
                    return Err(FaultCode::PathRefused);
                }
                if meta.uid() != current_uid()? {
                    return Err(FaultCode::PathRefused);
                }
            }
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(FaultCode::PathRefused),
            Err(error) if error.kind() == io::ErrorKind::NotFound && prefix == path => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(FaultCode::PathRefused);
            }
            Err(_) => return Err(FaultCode::Io),
        }
    }
    Ok(())
}

pub fn current_uid() -> Result<u32, FaultCode> {
    let text = fs::read_to_string("/proc/self/status").map_err(|_| FaultCode::Io)?;
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("Uid:") else {
            continue;
        };
        let first = rest.split_whitespace().next().ok_or(FaultCode::Io)?;
        return first.parse().map_err(|_| FaultCode::Io);
    }
    Err(FaultCode::Io)
}

pub fn kind(path: &Path) -> Result<Option<fs::FileType>, FaultCode> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(meta.file_type())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(FaultCode::Io),
    }
}

pub fn create_dir_new(path: &Path) -> Result<(), FaultCode> {
    match fs::create_dir(path) {
        Ok(()) => {
            let meta = fs::symlink_metadata(path).map_err(|_| FaultCode::Io)?;
            if meta.file_type().is_symlink() || !meta.is_dir() || meta.uid() != current_uid()? {
                return Err(FaultCode::PathRefused);
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Err(FaultCode::PathRefused),
        Err(_) => Err(FaultCode::Io),
    }
}

pub fn create_write(path: &Path, bytes: &[u8]) -> Result<File, FaultCode> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(O_NOFOLLOW)
        .open(path)
        .map_err(map_create)?;
    file.write_all(bytes).map_err(|_| FaultCode::Io)?;
    file.flush().map_err(|_| FaultCode::Io)?;
    Ok(file)
}

pub fn sync_file(file: &File) -> Result<(), FaultCode> {
    file.sync_all().map_err(|_| FaultCode::Io)
}

pub fn sync_dir(path: &Path) -> Result<(), FaultCode> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW)
        .open(path)
        .map_err(|_| FaultCode::Io)?;
    let meta = file.metadata().map_err(|_| FaultCode::Io)?;
    if !meta.is_dir() {
        return Err(FaultCode::PathRefused);
    }
    file.sync_all().map_err(|_| FaultCode::Io)
}

pub fn rename_absent(from: &Path, to: &Path) -> Result<(), FaultCode> {
    match fs::symlink_metadata(to) {
        Ok(_) => return Err(FaultCode::PathRefused),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err(FaultCode::Io),
    }
    fs::rename(from, to).map_err(|_| FaultCode::Io)
}

pub fn read_limited(path: &Path, max: u64) -> Result<Vec<u8>, FaultCode> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW)
        .open(path)
        .map_err(|_| FaultCode::Io)?;
    let meta = file.metadata().map_err(|_| FaultCode::Io)?;
    if !meta.is_file() {
        return Err(FaultCode::PathRefused);
    }
    if meta.len() > max {
        return Err(FaultCode::PayloadTooLarge);
    }
    let mut buf = Vec::new();
    file.take(max.saturating_add(1))
        .read_to_end(&mut buf)
        .map_err(|_| FaultCode::Io)?;
    if u64::try_from(buf.len()).map_err(|_| FaultCode::Io)? != meta.len() {
        return Err(FaultCode::Io);
    }
    Ok(buf)
}

pub fn file_len(path: &Path) -> Result<u64, FaultCode> {
    let meta = fs::symlink_metadata(path).map_err(|_| FaultCode::Io)?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(FaultCode::PathRefused);
    }
    Ok(meta.len())
}

fn map_create(error: io::Error) -> FaultCode {
    if error.kind() == io::ErrorKind::AlreadyExists {
        FaultCode::PathRefused
    } else {
        FaultCode::Io
    }
}
