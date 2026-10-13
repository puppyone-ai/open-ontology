use std::fs;
use std::path::Path;

use crate::error::{malformed, Fault};
use crate::sha::{hex_encode, sha256};

pub fn pack_digest(dir: &Path) -> Result<String, Fault> {
    let mut lines = Vec::new();
    walk(dir, dir, &mut lines)?;
    lines.sort();
    let mut body = String::new();
    for (path, hash) in lines {
        body.push_str(&path);
        body.push(' ');
        body.push_str(&hash);
        body.push('\n');
    }
    Ok(hex_encode(&sha256(body.as_bytes())))
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) -> Result<(), Fault> {
    let entries = fs::read_dir(dir).map_err(|_| malformed("pack"))?;
    for entry in entries {
        let entry = entry.map_err(|_| malformed("pack"))?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).map_err(|_| malformed("pack"))?;
        if meta.file_type().is_symlink() {
            return Err(malformed("pack_symlink"));
        }
        if meta.is_dir() {
            walk(root, &path, out)?;
        } else if meta.is_file() {
            let bytes = fs::read(&path).map_err(|_| malformed("pack"))?;
            let rel = path.strip_prefix(root).map_err(|_| malformed("pack"))?;
            let text = rel.to_str().ok_or_else(|| malformed("pack"))?.replace('\\', "/");
            out.push((text, hex_encode(&sha256(&bytes))));
        }
    }
    Ok(())
}
