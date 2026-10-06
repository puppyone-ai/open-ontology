//! Registry triple check for the answer-citations lock.
//!
//! Every crates.io package in the child lock must use the root lock's version,
//! source, and checksum. The only path package allowed is answer-citations.
//! Origin: evidence/.../a263/.../tools/lock_subset.rs
//! 7d24096b8a5ee9ec641e15625aabd2e69ebb71bbd99fa24ece28bc888304d34d
//! Delta: allowed path name is only answer-citations.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::process::ExitCode;

struct Package {
    name: String,
    version: String,
    source: String,
    checksum: String,
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let (Some(root_path), Some(child_path), None) = (args.next(), args.next(), args.next()) else {
        eprintln!("lock-subset: usage");
        return ExitCode::from(2);
    };
    let Ok(root) = fs::read_to_string(&root_path) else {
        eprintln!("lock-subset: unreadable root");
        return ExitCode::from(1);
    };
    let Ok(child) = fs::read_to_string(&child_path) else {
        eprintln!("lock-subset: unreadable child");
        return ExitCode::from(1);
    };
    let root_pkgs = packages(&root);
    let child_pkgs = packages(&child);
    let mut missing = 0_u32;
    let mut mismatch = 0_u32;
    let mut registry = 0_u32;
    let mut path_pkgs = 0_u32;
    for pkg in &child_pkgs {
        if pkg.source.starts_with("registry+") {
            registry = registry.saturating_add(1);
            let found = root_pkgs.iter().find(|item| {
                item.name == pkg.name && item.version == pkg.version && item.source == pkg.source
            });
            match found {
                Some(item) if item.checksum == pkg.checksum && !pkg.checksum.is_empty() => {}
                Some(_) => {
                    mismatch = mismatch.saturating_add(1);
                    eprintln!("mismatch {} {}", pkg.name, pkg.version);
                }
                None => {
                    missing = missing.saturating_add(1);
                    eprintln!("missing {} {}", pkg.name, pkg.version);
                }
            }
        } else if pkg.source.is_empty() || pkg.source.starts_with("path+") {
            path_pkgs = path_pkgs.saturating_add(1);
            if pkg.name != "answer-citations" {
                mismatch = mismatch.saturating_add(1);
                eprintln!("path {} {}", pkg.name, pkg.version);
            }
        } else {
            mismatch = mismatch.saturating_add(1);
            eprintln!("source {} {}", pkg.name, pkg.version);
        }
    }
    println!("registry={registry} path={path_pkgs} missing={missing} mismatch={mismatch}");
    if missing == 0 && mismatch == 0 && registry > 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn packages(text: &str) -> Vec<Package> {
    let mut out = Vec::new();
    let mut current: Option<Package> = None;
    for line in text.lines() {
        if line == "[[package]]" {
            if let Some(pkg) = current.take() {
                out.push(pkg);
            }
            current = Some(Package {
                name: String::new(),
                version: String::new(),
                source: String::new(),
                checksum: String::new(),
            });
            continue;
        }
        let Some(pkg) = current.as_mut() else {
            continue;
        };
        if let Some(value) = quoted(line, "name") {
            pkg.name = value;
        } else if let Some(value) = quoted(line, "version") {
            pkg.version = value;
        } else if let Some(value) = quoted(line, "source") {
            pkg.source = value;
        } else if let Some(value) = quoted(line, "checksum") {
            pkg.checksum = value;
        }
    }
    if let Some(pkg) = current {
        out.push(pkg);
    }
    out
}

fn quoted(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?.strip_prefix(" = ")?;
    let rest = rest.strip_prefix('"')?.strip_suffix('"')?;
    Some(rest.to_owned())
}
