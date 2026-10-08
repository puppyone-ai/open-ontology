//! Canonical checkout checks. Symlinks and embedded repositories stop the call.

use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::error::CuratedGitError;

pub fn strict_directory(path: &Path) -> Result<PathBuf, CuratedGitError> {
    if !path.is_absolute() {
        return Err(unregistered());
    }
    let mut acc = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) => {
                return Err(CuratedGitError::new(
                    "root.unsafe",
                    "the checkout is not a canonical git directory",
                ));
            }
            Component::RootDir => acc.push(component),
            Component::CurDir => {}
            Component::ParentDir => {
                let before = acc.clone();
                if !acc.pop() || acc.as_os_str().is_empty() || acc == before {
                    return Err(CuratedGitError::new(
                        "root.escape",
                        "a path escapes the checkout",
                    ));
                }
            }
            Component::Normal(name) => {
                let next = acc.join(name);
                let meta = fs::symlink_metadata(&next).map_err(|_| unsafe_root())?;
                if meta.file_type().is_symlink() {
                    return Err(CuratedGitError::new(
                        "root.symlink",
                        "the checkout path contains a symlink",
                    ));
                }
                if !meta.is_dir() {
                    return Err(unsafe_root());
                }
                acc = next;
            }
        }
    }
    if acc.as_os_str().is_empty() || acc == Path::new("/") {
        return Err(unsafe_root());
    }
    Ok(acc)
}

pub fn check_checkout(root: &Path) -> Result<(), CuratedGitError> {
    let git = root.join(".git");
    require_real_dir(&git)?;
    require_real_dir(&git.join("objects"))?;
    if git.join("objects/info/alternates").exists() || root.join(".gitmodules").exists() {
        return Err(embedded());
    }
    let config = fs::read(git.join("config")).unwrap_or_default();
    if config_has_submodule(&config) {
        return Err(embedded());
    }
    require_real_file(&git.join("HEAD"))?;
    refuse_symlink_file(&git.join("refs"))?;
    refuse_symlink_file(&git.join("refs/heads"))?;
    refuse_symlink_file(&git.join("refs/heads/curated"))?;
    Ok(())
}

fn config_has_submodule(config: &[u8]) -> bool {
    config.split(|byte| *byte == b'\n').any(|line| {
        let trimmed = trim_ascii(line);
        trimmed.starts_with(b"[submodule")
    })
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map(|index| index + 1)
        .unwrap_or(start);
    &bytes[start..end]
}

fn require_real_dir(path: &Path) -> Result<(), CuratedGitError> {
    let meta = fs::symlink_metadata(path).map_err(|_| unsafe_root())?;
    if meta.file_type().is_symlink() {
        return Err(CuratedGitError::new(
            "root.symlink",
            "the checkout path contains a symlink",
        ));
    }
    if !meta.is_dir() {
        return Err(unsafe_root());
    }
    Ok(())
}

fn require_real_file(path: &Path) -> Result<(), CuratedGitError> {
    let meta = fs::symlink_metadata(path).map_err(|_| unsafe_root())?;
    if meta.file_type().is_symlink() {
        return Err(CuratedGitError::new(
            "root.symlink",
            "the checkout path contains a symlink",
        ));
    }
    if !meta.is_file() {
        return Err(unsafe_root());
    }
    Ok(())
}

fn refuse_symlink_file(path: &Path) -> Result<(), CuratedGitError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(CuratedGitError::new(
            "root.symlink",
            "the checkout path contains a symlink",
        )),
        Ok(_) | Err(_) => Ok(()),
    }
}

fn unsafe_root() -> CuratedGitError {
    CuratedGitError::new(
        "root.unsafe",
        "the checkout is not a canonical git directory",
    )
}

fn unregistered() -> CuratedGitError {
    CuratedGitError::new(
        "root.unregistered",
        "the project root is not the registered checkout",
    )
}

fn embedded() -> CuratedGitError {
    CuratedGitError::new(
        "root.embedded",
        "the checkout contains another git directory or submodule",
    )
}
