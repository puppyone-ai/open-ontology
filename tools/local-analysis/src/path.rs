use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use crate::fault::Fault;

pub fn read_regular(path: &Path, max: usize, empty: Fault) -> Result<Vec<u8>, Fault> {
    let resolved = walk(path)?;
    let meta = fs::symlink_metadata(&resolved).map_err(|_| Fault::path())?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(Fault::path());
    }
    let mut file = File::open(&resolved).map_err(|_| Fault::path())?;
    let mut bytes = Vec::new();
    let limit = u64::try_from(max).unwrap_or(u64::MAX).saturating_add(1);
    file.by_ref()
        .take(limit)
        .read_to_end(&mut bytes)
        .map_err(|_| Fault::io())?;
    if bytes.len() > max {
        return Err(Fault::budget());
    }
    if bytes.is_empty() {
        return Err(empty);
    }
    Ok(bytes)
}

pub fn require_dir(path: &Path) -> Result<PathBuf, Fault> {
    let resolved = walk(path)?;
    let meta = fs::symlink_metadata(&resolved).map_err(|_| Fault::path())?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(Fault::path());
    }
    Ok(resolved)
}

pub fn require_absent(path: &Path) -> Result<(), Fault> {
    match fs::symlink_metadata(path) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        _ => Err(Fault::path()),
    }
}

pub fn cancel_requested(path: &Path) -> Result<bool, Fault> {
    let Some(name) = path.file_name() else {
        return Err(Fault::path());
    };
    if name.len() > 255 || name.is_empty() {
        return Err(Fault::path());
    }
    let parent = path.parent().filter(|dir| !dir.as_os_str().is_empty());
    let parent = parent.unwrap_or_else(|| Path::new("."));
    let dir = require_dir(parent)?;
    let full = dir.join(name);
    match fs::symlink_metadata(&full) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(Fault::path()),
        Ok(meta) if meta.file_type().is_symlink() => Err(Fault::path()),
        Ok(meta) if meta.is_file() => Ok(true),
        Ok(_) => Err(Fault::path()),
    }
}

pub fn read_capped(path: &Path, max: usize) -> Result<Vec<u8>, Fault> {
    let resolved = walk(path)?;
    let meta = fs::symlink_metadata(&resolved).map_err(|_| Fault::path())?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(Fault::path());
    }
    let limit = u64::try_from(max).unwrap_or(u64::MAX);
    if meta.len() == 0 || meta.len() > limit {
        return Err(Fault::budget());
    }
    let mut file = File::open(&resolved).map_err(|_| Fault::io())?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|_| Fault::io())?;
    if bytes.len() > max {
        return Err(Fault::budget());
    }
    Ok(bytes)
}

fn walk(path: &Path) -> Result<PathBuf, Fault> {
    if path.as_os_str().is_empty() {
        return Err(Fault::path());
    }
    let mut acc = if path.is_absolute() {
        PathBuf::new()
    } else {
        let cwd = std::env::current_dir().map_err(|_| Fault::path())?;
        let meta = fs::symlink_metadata(&cwd).map_err(|_| Fault::path())?;
        if meta.file_type().is_symlink() {
            return Err(Fault::path());
        }
        cwd
    };
    for component in path.components() {
        match component {
            Component::Prefix(_) => return Err(Fault::path()),
            Component::RootDir => acc = PathBuf::from("/"),
            Component::CurDir => {}
            Component::ParentDir => {
                if !acc.pop() {
                    return Err(Fault::path());
                }
            }
            Component::Normal(name) => {
                if name.is_empty() || name.len() > 255 {
                    return Err(Fault::path());
                }
                acc.push(name);
                match fs::symlink_metadata(&acc) {
                    Ok(meta) if meta.file_type().is_symlink() => return Err(Fault::path()),
                    Ok(_) => {}
                    Err(_) => return Err(Fault::path()),
                }
            }
        }
    }
    Ok(acc)
}
