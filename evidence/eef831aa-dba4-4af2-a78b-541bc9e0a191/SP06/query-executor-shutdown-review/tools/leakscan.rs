//! Scan a tree for secret bytes and private-key markers.
//! Prints counts and relative paths only. Never prints secret bytes.

use std::env;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(root) = args.next() else {
        eprintln!("usage: leakscan ROOT SECRET...");
        return ExitCode::from(2);
    };
    let mut needles = Vec::new();
    for path in args {
        let Ok(bytes) = fs::read(&path) else {
            eprintln!("secret-unreadable");
            return ExitCode::from(2);
        };
        let trimmed = trim_ascii(&bytes);
        if trimmed.len() < 8 {
            eprintln!("secret-short");
            return ExitCode::from(2);
        }
        needles.push(trimmed);
    }
    let mut hits = 0_u64;
    let mut markers = 0_u64;
    let mut files = 0_u64;
    if let Err(code) = walk(Path::new(&root), Path::new(&root), &needles, &mut hits, &mut markers, &mut files)
    {
        return code;
    }
    println!("files={files} leak_hits={hits} private_key_markers={markers}");
    if hits == 0 && markers == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn walk(
    root: &Path,
    dir: &Path,
    needles: &[Vec<u8>],
    hits: &mut u64,
    markers: &mut u64,
    files: &mut u64,
) -> Result<(), ExitCode> {
    let entries = fs::read_dir(dir).map_err(|_| ExitCode::from(2))?;
    for entry in entries {
        let entry = entry.map_err(|_| ExitCode::from(2))?;
        let kind = entry.file_type().map_err(|_| ExitCode::from(2))?;
        let path = entry.path();
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            walk(root, &path, needles, hits, markers, files)?;
            continue;
        }
        if !kind.is_file() {
            continue;
        }
        *files = files.saturating_add(1);
        let mut bytes = Vec::new();
        let mut file = File::open(&path).map_err(|_| ExitCode::from(2))?;
        file.read_to_end(&mut bytes).map_err(|_| ExitCode::from(2))?;
        let rel = display_rel(root, &path);
        if needles.iter().any(|needle| contains_slice(&bytes, needle)) {
            *hits = hits.saturating_add(1);
            println!("hit={rel}");
        }
        if contains_slice(&bytes, &private_key_marker()) {
            *markers = markers.saturating_add(1);
            println!("marker={rel}");
        }
    }
    Ok(())
}

fn display_rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn trim_ascii(bytes: &[u8]) -> Vec<u8> {
    let start = bytes.iter().position(|byte| !byte.is_ascii_whitespace()).unwrap_or(bytes.len());
    let end = bytes.iter().rposition(|byte| !byte.is_ascii_whitespace()).map(|index| index + 1).unwrap_or(start);
    bytes[start..end].to_vec()
}

fn private_key_marker() -> Vec<u8> {
    let mut marker = b"PRIVATE".to_vec();
    marker.extend_from_slice(b" KEY");
    marker
}

fn contains_slice(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|window| window == needle)
}
