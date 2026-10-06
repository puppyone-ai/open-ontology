//! Compare registry triples in one or more committed locks to the root lock.
//! A triple is name, version, checksum, and source. Path packages are printed
//! and are not required to appear in the root lock. Std only.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::process::ExitCode;

#[derive(Clone, Ord, PartialOrd, Eq, PartialEq)]
struct Triple {
    name: String,
    version: String,
    checksum: String,
    source: String,
}

struct Package {
    name: Option<String>,
    version: Option<String>,
    source: Option<String>,
    checksum: Option<String>,
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
    let root_lock = args.next().ok_or("usage: lock_subset <root-lock> <crate-lock>... ")?;
    let mut crate_locks = Vec::new();
    for arg in args {
        crate_locks.push(arg);
    }
    if crate_locks.is_empty() {
        return Err("usage: lock_subset <root-lock> <crate-lock>...".to_owned());
    }
    let root_text = fs::read_to_string(&root_lock).map_err(|err| format!("read {root_lock}: {err}"))?;
    let (root_triples, _) = split_packages(&root_text)?;
    let mut missing = 0u32;
    let mut mismatch = 0u32;
    let mut seen = 0u32;
    for crate_lock in &crate_locks {
        let text = fs::read_to_string(crate_lock).map_err(|err| format!("read {crate_lock}: {err}"))?;
        let (triples, paths) = split_packages(&text)?;
        println!("lock {crate_lock}");
        for package in &paths {
            println!("path {crate_lock} {} {}", package.0, package.1);
        }
        for triple in &triples {
            seen += 1;
            if root_triples.iter().any(|other| other == triple) {
                continue;
            }
            let same_name = root_triples
                .iter()
                .any(|other| other.name == triple.name && other.version == triple.version);
            if same_name {
                mismatch += 1;
                println!(
                    "mismatch {crate_lock} {} {} {} {}",
                    triple.name, triple.version, triple.checksum, triple.source
                );
            } else {
                missing += 1;
                println!(
                    "missing {crate_lock} {} {} {} {}",
                    triple.name, triple.version, triple.checksum, triple.source
                );
            }
        }
        println!(
            "lock_summary {crate_lock} registry_triples={} path_packages={}",
            triples.len(),
            paths.len()
        );
    }
    println!("summary registry_triples={seen} missing={missing} mismatch={mismatch}");
    if seen == 0 || missing != 0 || mismatch != 0 {
        return Err(format!("missing={missing} mismatch={mismatch}"));
    }
    Ok(())
}

fn split_packages(text: &str) -> Result<(Vec<Triple>, Vec<(String, String)>), String> {
    let mut triples = Vec::new();
    let mut paths = Vec::new();
    let mut current = Package::default();
    let mut saw_package = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line == "[[package]]" {
            if saw_package {
                push_package(&mut triples, &mut paths, &mut current)?;
            }
            saw_package = true;
            current = Package::default();
            continue;
        }
        if !saw_package {
            continue;
        }
        let Some((key, value)) = line.split_once(" = ") else {
            continue;
        };
        let Some(value) = quoted(value) else {
            continue;
        };
        match key {
            "name" => current.name = Some(value),
            "version" => current.version = Some(value),
            "source" => current.source = Some(value),
            "checksum" => current.checksum = Some(value),
            _ => {}
        }
    }
    if saw_package {
        push_package(&mut triples, &mut paths, &mut current)?;
    }
    triples.sort();
    paths.sort();
    Ok((triples, paths))
}

fn push_package(
    triples: &mut Vec<Triple>,
    paths: &mut Vec<(String, String)>,
    package: &mut Package,
) -> Result<(), String> {
    let name = package.name.take();
    let version = package.version.take();
    let source = package.source.take();
    let checksum = package.checksum.take();
    if name.is_none() && version.is_none() && source.is_none() && checksum.is_none() {
        return Ok(());
    }
    let name = name.ok_or("package without name")?;
    let version = version.ok_or_else(|| format!("{name} without version"))?;
    match source {
        Some(source) if source.starts_with("registry+") => {
            let checksum = checksum.ok_or_else(|| format!("{name} {version} registry package has no checksum"))?;
            triples.push(Triple {
                name,
                version,
                checksum,
                source,
            });
            Ok(())
        }
        _ => {
            if checksum.is_some() {
                return Err(format!("{name} {version} non-registry package has a checksum"));
            }
            paths.push((name, version));
            Ok(())
        }
    }
}

fn quoted(value: &str) -> Option<String> {
    let value = value.trim();
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        Some(value[1..value.len() - 1].to_owned())
    } else {
        None
    }
}

impl Default for Package {
    fn default() -> Self {
        Self {
            name: None,
            version: None,
            source: None,
            checksum: None,
        }
    }
}
