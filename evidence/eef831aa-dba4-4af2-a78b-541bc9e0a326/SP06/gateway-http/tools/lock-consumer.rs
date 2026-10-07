//! Registry triple gate for the oo-gateway consumer lock.
//! Origin: a319 tools/lockdiff.rs parse of registry packages. This file adds the
//! five-crate allowlist and prints dependency-edge additions. It does not hash crates.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::process::ExitCode;

const EXPECTED: &[(&str, &str, &str)] = &[
    ("axum", "0.8.9", "31b698c5f9a010f6573133b09e0de5408834d0c82f8d7475a89fc1867a71cd90"),
    ("axum-core", "0.5.6", "08c78f31d7b1291f7ee735c1c6780ccde7785daae9a9206026862dab7d8792d1"),
    ("httpdate", "1.0.3", "df3b46402a9d5adb4c86a0cf463f42e19994e3ee891101b1841f30a545cb49a9"),
    ("matchit", "0.8.4", "47e1ffaa40ddd1f3ed91f717a33c8c0ee23fff369e3aa8772b9605cc1d22f4c3"),
    ("mime", "0.3.17", "6877bb514081ee2a7ff5ef9de3281f14a4dd4bceac4c09388074a6b5df8a139a"),
];

#[derive(Clone, Debug)]
struct Package {
    name: String,
    version: String,
    checksum: String,
    source: String,
    deps: BTreeSet<String>,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            eprintln!("lock-consumer: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<u8, String> {
    let mut args = env::args().skip(1);
    let old_path = args.next().ok_or("usage: lock-consumer OLD.lock NEW.lock")?;
    let new_path = args.next().ok_or("usage: lock-consumer OLD.lock NEW.lock")?;
    if args.next().is_some() {
        return Err("too many arguments".to_owned());
    }
    let old = parse_lock(&fs::read_to_string(&old_path).map_err(|err| format!("read {old_path}: {err}"))?)?;
    let new = parse_lock(&fs::read_to_string(&new_path).map_err(|err| format!("read {new_path}: {err}"))?)?;
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    for (key, package) in &new {
        match old.get(key) {
            None => added.push(package.clone()),
            Some(previous) if previous.checksum != package.checksum => {
                changed.push((previous.clone(), package.clone()));
            }
            Some(_) => {}
        }
    }
    for (key, package) in &old {
        if !new.contains_key(key) {
            removed.push(package.clone());
        }
    }
    added.sort_by(|left, right| left.name.cmp(&right.name).then(left.version.cmp(&right.version)));
    let mut denied = 0usize;
    for package in new.values() {
        if is_denied(&package.name) {
            denied += 1;
            println!("denied {} {}", package.name, package.version);
        }
    }
    println!("added={}", added.len());
    for package in &added {
        println!("added {} {} {}", package.name, package.version, package.checksum);
    }
    println!("removed={}", removed.len());
    for package in &removed {
        println!("removed {} {} {}", package.name, package.version, package.checksum);
    }
    println!("changed={}", changed.len());
    for (before, after) in &changed {
        println!(
            "changed {} {} {} -> {}",
            before.name, before.version, before.checksum, after.checksum
        );
    }
    println!("denied={denied}");
    for (key, package) in &new {
        let Some(previous) = old.get(key) else {
            continue;
        };
        for dep in package.deps.difference(&previous.deps) {
            println!("edge {} {} +{}", package.name, package.version, dep);
        }
        for dep in previous.deps.difference(&package.deps) {
            println!("edge {} {} -{}", package.name, package.version, dep);
        }
    }
    let allowlist_ok = added.len() == EXPECTED.len()
        && EXPECTED.iter().all(|(name, version, checksum)| {
            added.iter().any(|package| package.name == *name && package.version == *version && package.checksum == *checksum)
        });
    if removed.is_empty() && changed.is_empty() && denied == 0 && allowlist_ok {
        println!("consumer=pass");
        Ok(0)
    } else {
        println!("consumer=fail");
        Ok(5)
    }
}

fn is_denied(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["jwt", "jose", "oidc", "oauth", "openid", "aliri", "paseto", "jsonwebtoken"]
        .iter()
        .any(|needle| name.contains(needle))
}

fn parse_lock(text: &str) -> Result<BTreeMap<(String, String), Package>, String> {
    let mut packages = BTreeMap::new();
    let mut current: Option<Package> = None;
    let mut in_deps = false;
    let flush = |current: &mut Option<Package>, packages: &mut BTreeMap<(String, String), Package>| -> Result<(), String> {
        if let Some(package) = current.take() {
            if package.source.starts_with("registry+") {
                if package.checksum.len() != 64 {
                    return Err(format!("{} {} missing checksum", package.name, package.version));
                }
                packages.insert((package.name.clone(), package.version.clone()), package);
            }
        }
        Ok(())
    };
    for raw in text.lines() {
        let line = raw.trim();
        if line == "[[package]]" {
            flush(&mut current, &mut packages)?;
            in_deps = false;
            current = Some(Package {
                name: String::new(),
                version: String::new(),
                checksum: String::new(),
                source: String::new(),
                deps: BTreeSet::new(),
            });
            continue;
        }
        let Some(package) = current.as_mut() else {
            continue;
        };
        if in_deps {
            if line == "]" {
                in_deps = false;
                continue;
            }
            let item = line.trim_end_matches(',');
            package.deps.insert(unquote(item)?);
            continue;
        }
        if let Some(value) = line.strip_prefix("name = ") {
            package.name = unquote(value)?;
        } else if let Some(value) = line.strip_prefix("version = ") {
            if package.name.is_empty() {
                continue;
            }
            package.version = unquote(value)?;
        } else if let Some(value) = line.strip_prefix("checksum = ") {
            package.checksum = unquote(value)?;
        } else if let Some(value) = line.strip_prefix("source = ") {
            package.source = unquote(value)?;
        } else if let Some(rest) = line.strip_prefix("dependencies = ") {
            if rest == "[" {
                in_deps = true;
            } else if let Some(inner) = rest.strip_prefix('[').and_then(|value| value.strip_suffix(']')) {
                for item in inner.split(',') {
                    let item = item.trim();
                    if !item.is_empty() {
                        package.deps.insert(unquote(item)?);
                    }
                }
            }
        }
    }
    flush(&mut current, &mut packages)?;
    Ok(packages)
}

fn unquote(value: &str) -> Result<String, String> {
    let value = value.trim();
    if let Some(inner) = value.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')) {
        return Ok(inner.to_owned());
    }
    Err(format!("expected quoted value, got {value}"))
}
