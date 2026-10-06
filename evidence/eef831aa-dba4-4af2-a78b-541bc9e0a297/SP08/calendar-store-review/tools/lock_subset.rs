//! Compare every registry triple in the calendar-store lock to the root lock.
//! A triple is name, version, checksum, and source. Path packages must be
//! the three allowlisted crates. Std only. This tool does not run Cargo.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::process::ExitCode;

const PATH_ALLOWLIST: [&str; 3] = ["business-calendar", "calendar-store", "oo-core"];
const DIRECT_PINS: [(&str, &str); 9] = [
    ("postgres-protocol", "0.6.12"),
    ("postgres-types", "0.2.14"),
    ("rustls", "0.23.45"),
    ("serde", "1.0.229"),
    ("serde_json", "1.0.151"),
    ("tokio", "1.53.1"),
    ("tokio-postgres", "0.7.18"),
    ("tokio-postgres-rustls", "0.14.0"),
    ("tokio-rustls", "0.26.5"),
];
const HELPER_PINS: [(&str, &str); 3] = [
    ("chrono", "0.4.45"),
    ("chrono-tz", "0.10.4"),
    ("sha2", "0.10.9"),
];

#[derive(Clone, Ord, PartialOrd, Eq, PartialEq)]
struct Triple {
    name: String,
    version: String,
    checksum: String,
    source: String,
}

#[derive(Clone, Ord, PartialOrd, Eq, PartialEq)]
struct PathPackage {
    name: String,
    version: String,
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
    let crate_lock = args.next().ok_or("usage: lock_subset <crate-lock> <root-lock>")?;
    let root_lock = args.next().ok_or("usage: lock_subset <crate-lock> <root-lock>")?;
    if args.next().is_some() {
        return Err("usage: lock_subset <crate-lock> <root-lock>".to_owned());
    }
    let crate_text = fs::read_to_string(&crate_lock).map_err(|err| format!("read {crate_lock}: {err}"))?;
    let root_text = fs::read_to_string(&root_lock).map_err(|err| format!("read {root_lock}: {err}"))?;
    let (crate_triples, crate_paths) = split_packages(&crate_text, true)?;
    let (root_triples, _) = split_packages(&root_text, false)?;
    for package in &crate_paths {
        println!("path {} {}", package.name, package.version);
    }
    let mut missing = 0u32;
    let mut mismatch = 0u32;
    for triple in &crate_triples {
        if root_triples.iter().any(|other| other == triple) {
            println!(
                "triple {} {} {} {}",
                triple.name, triple.version, triple.checksum, triple.source
            );
            continue;
        }
        let same_name = root_triples
            .iter()
            .any(|other| other.name == triple.name && other.version == triple.version);
        if same_name {
            mismatch += 1;
            println!(
                "mismatch {} {} {} {}",
                triple.name, triple.version, triple.checksum, triple.source
            );
        } else {
            missing += 1;
            println!(
                "missing {} {} {} {}",
                triple.name, triple.version, triple.checksum, triple.source
            );
        }
    }
    let direct_present = DIRECT_PINS.iter().all(|(name, version)| {
        crate_triples
            .iter()
            .any(|triple| triple.name == *name && triple.version == *version)
    });
    let helper_present = HELPER_PINS.iter().all(|(name, version)| {
        crate_triples
            .iter()
            .any(|triple| triple.name == *name && triple.version == *version)
    });
    println!(
        "summary registry_triples={} missing={missing} mismatch={mismatch} direct_pins_present={direct_present} helper_pins_present={helper_present} path_packages={}",
        crate_triples.len(),
        crate_paths.len()
    );
    if crate_paths.len() != PATH_ALLOWLIST.len() {
        return Err(format!("path package count is {}", crate_paths.len()));
    }
    for name in PATH_ALLOWLIST {
        if !crate_paths.iter().any(|package| package.name == name && package.version == "0.1.0") {
            return Err(format!("missing path package {name} 0.1.0"));
        }
    }
    if crate_triples.len() != 208 || !direct_present || !helper_present || missing != 0 || mismatch != 0 {
        return Err(format!(
            "registry_triples={} missing={missing} mismatch={mismatch} direct={direct_present} helper={helper_present}",
            crate_triples.len()
        ));
    }
    Ok(())
}

fn split_packages(text: &str, crate_lock: bool) -> Result<(Vec<Triple>, Vec<PathPackage>), String> {
    let mut triples = Vec::new();
    let mut paths = Vec::new();
    let mut current = Package::default();
    let mut saw_package = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line == "[[package]]" {
            if saw_package {
                push_package(&mut triples, &mut paths, &mut current, crate_lock)?;
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
        push_package(&mut triples, &mut paths, &mut current, crate_lock)?;
    }
    triples.sort();
    paths.sort();
    Ok((triples, paths))
}

fn push_package(
    triples: &mut Vec<Triple>,
    paths: &mut Vec<PathPackage>,
    package: &mut Package,
    crate_lock: bool,
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
        other => {
            if !crate_lock {
                return Ok(());
            }
            if !PATH_ALLOWLIST.contains(&name.as_str()) {
                let shown = other.unwrap_or_else(|| "none".to_owned());
                return Err(format!("{name} {version} source is not an allowlisted path package: {shown}"));
            }
            if checksum.is_some() {
                return Err(format!("{name} {version} path package has a checksum"));
            }
            paths.push(PathPackage { name, version });
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
