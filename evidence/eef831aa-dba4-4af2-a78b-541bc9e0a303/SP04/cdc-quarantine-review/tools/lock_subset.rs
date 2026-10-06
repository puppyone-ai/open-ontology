//! Registry subset check for the cdc-quarantine lock.
//! Exactly eleven crates.io packages, each matched to the root lock by
//! name, version, and checksum. This file does not shell out.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::process::ExitCode;

const EXPECTED: [&str; 11] = [
    "itoa",
    "memchr",
    "proc-macro2",
    "quote",
    "serde",
    "serde_core",
    "serde_derive",
    "serde_json",
    "syn",
    "unicode-ident",
    "zmij",
];

struct Package {
    name: String,
    version: String,
    source: String,
    checksum: String,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("lock_subset: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let root = args.next().ok_or("usage: lock_subset <root.lock> <child.lock>")?;
    let child = args.next().ok_or("usage: lock_subset <root.lock> <child.lock>")?;
    if args.next().is_some() {
        return Err("usage: lock_subset <root.lock> <child.lock>".to_owned());
    }
    let root_pkgs = registry_packages(&fs::read_to_string(&root).map_err(|err| format!("read {root}: {err}"))?)?;
    let child_pkgs = registry_packages(&fs::read_to_string(&child).map_err(|err| format!("read {child}: {err}"))?)?;
    if child_pkgs.len() != EXPECTED.len() {
        return Err(format!("child registry count {} want {}", child_pkgs.len(), EXPECTED.len()));
    }
    for (index, package) in child_pkgs.iter().enumerate() {
        if package.name != EXPECTED[index] {
            return Err(format!("child registry[{index}] {} want {}", package.name, EXPECTED[index]));
        }
        let matched = root_pkgs.iter().find(|item| item.name == package.name && item.version == package.version);
        let Some(found) = matched else {
            return Err(format!("root lock has no {} {}", package.name, package.version));
        };
        if found.checksum != package.checksum || found.source != package.source {
            return Err(format!(
                "drift {} {} child {} {} root {} {}",
                package.name, package.version, package.source, package.checksum, found.source, found.checksum
            ));
        }
        println!("{} {} {}", package.name, package.version, package.checksum);
    }
    println!("registry={}", child_pkgs.len());
    Ok(())
}

fn registry_packages(text: &str) -> Result<Vec<Package>, String> {
    let mut packages = Vec::new();
    for block in text.split("[[package]]").skip(1) {
        let name = field(block, "name");
        let version = field(block, "version");
        let Some(name) = name else {
            return Err("package block missing name".to_owned());
        };
        let Some(version) = version else {
            return Err(format!("package {name} missing version"));
        };
        let Some(source) = field(block, "source") else {
            continue;
        };
        if !source.starts_with("registry+") {
            continue;
        }
        let Some(checksum) = field(block, "checksum") else {
            return Err(format!("registry package {name} missing checksum"));
        };
        packages.push(Package {
            name,
            version,
            source,
            checksum,
        });
    }
    packages.sort_by(|left, right| left.name.cmp(&right.name).then(left.version.cmp(&right.version)));
    Ok(packages)
}

fn field(block: &str, key: &str) -> Option<String> {
    let prefix = format!("{key} = \"");
    for line in block.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix(&prefix) else {
            continue;
        };
        let end = rest.find('"')?;
        return Some(rest[..end].to_owned());
    }
    None
}
