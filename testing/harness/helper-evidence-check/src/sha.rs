use std::path::Path;
use std::process::Command;

use crate::fail::{self, Fail, HASH_MISMATCH};

pub fn sha256_file(path: &Path) -> Result<String, Fail> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => {}
        Ok(_) => {
            return Err(fail::fail(
                "path-traversal",
                crate::fail::PATH_TRAVERSAL,
                "refusing symlink or non-file",
            ))
        }
        Err(_) => return Err(fail::fail("hash-mismatch", HASH_MISMATCH, "hash path missing")),
    }
    let (bin, prefix) = if Path::new("/usr/bin/sha256sum").is_file() {
        ("/usr/bin/sha256sum", &[][..])
    } else if Path::new("/usr/bin/shasum").is_file() {
        ("/usr/bin/shasum", &["-a", "256"][..])
    } else {
        return Err(fail::fail(
            "hash-mismatch",
            HASH_MISMATCH,
            "sha256sum and shasum are absent",
        ));
    };
    let output = Command::new(bin)
        .args(prefix)
        .arg(path)
        .output()
        .map_err(|_| fail::fail("hash-mismatch", HASH_MISMATCH, "hash command failed to start"))?;
    if !output.status.success() {
        return Err(fail::fail("hash-mismatch", HASH_MISMATCH, "hash command failed"));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let token = text.split_whitespace().next().unwrap_or("");
    if token.len() == 64 && token.chars().all(|ch| ch.is_ascii_hexdigit()) {
        Ok(token.to_ascii_lowercase())
    } else {
        Err(fail::fail("hash-mismatch", HASH_MISMATCH, "hash command output was not a digest"))
    }
}
