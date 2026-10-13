//! Reject private-key material in evidence bytes.
//! The public proxy marker is allowed and is not a credential.

use crate::PUBLIC_MARKER;

const NEEDLES: &[&[u8]] = &[
    b"BEGIN PRIVATE KEY",
    b"BEGIN RSA PRIVATE KEY",
    b"BEGIN OPENSSH PRIVATE KEY",
    b"BEGIN EC PRIVATE KEY",
    b"AKIA",
    b"sk-ant-",
    b"sk-proj-",
    b"xai-",
];

pub fn secret_hit(bytes: &[u8]) -> Option<&'static str> {
    let mut stripped = bytes.to_vec();
    let marker = PUBLIC_MARKER.as_bytes();
    while let Some(position) = find_slice(&stripped, marker) {
        stripped.drain(position..position + marker.len());
    }
    for needle in NEEDLES {
        if find_slice(&stripped, needle).is_some() {
            return Some(std::str::from_utf8(needle).unwrap_or("secret"));
        }
    }
    None
}

pub fn scan_tree(root: &std::path::Path) -> Result<(), String> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let meta = std::fs::symlink_metadata(&path).map_err(|err| format!("stat {}: {err}", path.display()))?;
        if meta.file_type().is_symlink() {
            return Err(format!("symlink refused {}", path.display()));
        }
        if meta.is_dir() {
            let entries = std::fs::read_dir(&path).map_err(|err| format!("read dir {}: {err}", path.display()))?;
            for entry in entries {
                let entry = entry.map_err(|err| format!("dirent: {err}"))?;
                pending.push(entry.path());
            }
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        if meta.len() > 8 * 1_048_576 {
            return Err(format!("{} exceeds the evidence scan budget", path.display()));
        }
        let bytes = std::fs::read(&path).map_err(|err| format!("read {}: {err}", path.display()))?;
        if let Some(hit) = secret_hit(&bytes) {
            return Err(format!("{} contains {hit}", path.display()));
        }
    }
    Ok(())
}

fn find_slice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|window| window == needle)
}
