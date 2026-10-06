//! Compare every registry triple in a crate lock to a root lock.
//! A triple is name, version, checksum, and source. Std only.
//!
//! Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a278/SP08/enum-history-review/tools/lock_subset.rs
//! sha256 8f3d29ba03f4245088e59e510a410c539a5d1370d1047fe0e3f01058ed0c7597
//! That file cites evidence/eef831aa-dba4-4af2-a78b-541bc9e0a272/SP11/session-transition-review/tools/lock_subset.rs
//! Change: path packages may only be business-calendar and oo-core.
//! Registry count must be 115.
//! Direct pins add chrono 0.4.45, chrono-tz 0.10.4, and sha2 0.10.9
//! to serde 1.0.229, serde_core 1.0.229, serde_derive 1.0.229, and serde_json 1.0.151.

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

#[derive(Clone, Ord, PartialOrd, Eq, PartialEq)]
struct PathPkg {
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
    let crate_lock = args
        .next()
        .ok_or("usage: lock_subset <crate-lock> <root-lock>")?;
    let root_lock = args
        .next()
        .ok_or("usage: lock_subset <crate-lock> <root-lock>")?;
    if args.next().is_some() {
        return Err("usage: lock_subset <crate-lock> <root-lock>".to_owned());
    }
    let crate_text = fs::read_to_string(&crate_lock)
        .map_err(|err| format!("read {crate_lock}: {err}"))?;
    let root_text =
        fs::read_to_string(&root_lock).map_err(|err| format!("read {root_lock}: {err}"))?;
    let (crate_triples, path_pkgs) = registry_triples(&crate_text, true)?;
    let (root_triples, _) = registry_triples(&root_text, false)?;
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
    for pkg in &path_pkgs {
        println!("path {} {}", pkg.name, pkg.version);
    }
    let direct_present = has(&crate_triples, "chrono", "0.4.45")
        && has(&crate_triples, "chrono-tz", "0.10.4")
        && has(&crate_triples, "serde", "1.0.229")
        && has(&crate_triples, "serde_core", "1.0.229")
        && has(&crate_triples, "serde_derive", "1.0.229")
        && has(&crate_triples, "serde_json", "1.0.151")
        && has(&crate_triples, "sha2", "0.10.9");
    println!(
        "summary registry_triples={} missing={missing} mismatch={mismatch} path_packages={} direct_pins_present={direct_present}",
        crate_triples.len(),
        path_pkgs.len()
    );
    if crate_triples.len() != 115 {
        return Err(format!("registry_triples={}", crate_triples.len()));
    }
    if path_pkgs.len() != 2 {
        return Err(format!("path_packages={}", path_pkgs.len()));
    }
    let path_names_ok = path_pkgs.iter().any(|pkg| pkg.name == "business-calendar" && pkg.version == "0.1.0")
        && path_pkgs.iter().any(|pkg| pkg.name == "oo-core" && pkg.version == "0.1.0");
    if !path_names_ok {
        return Err("path packages are not business-calendar 0.1.0 and oo-core 0.1.0".to_owned());
    }
    if !direct_present || missing != 0 || mismatch != 0 {
        return Err(format!("missing={missing} mismatch={mismatch}"));
    }
    Ok(())
}

fn has(triples: &[Triple], name: &str, version: &str) -> bool {
    triples
        .iter()
        .any(|triple| triple.name == name && triple.version == version)
}

fn allowed_path(name: &str) -> bool {
    name == "business-calendar" || name == "oo-core"
}

fn registry_triples(text: &str, crate_lock: bool) -> Result<(Vec<Triple>, Vec<PathPkg>), String> {
    let mut out = Vec::new();
    let mut paths = Vec::new();
    let mut current = Package::default();
    let mut saw_package = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line == "[[package]]" {
            if saw_package {
                push_package(&mut out, &mut paths, &mut current, crate_lock)?;
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
        push_package(&mut out, &mut paths, &mut current, crate_lock)?;
    }
    out.sort();
    paths.sort();
    Ok((out, paths))
}

fn push_package(
    out: &mut Vec<Triple>,
    paths: &mut Vec<PathPkg>,
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
            let checksum =
                checksum.ok_or_else(|| format!("{name} {version} registry package has no checksum"))?;
            out.push(Triple {
                name,
                version,
                checksum,
                source,
            });
            Ok(())
        }
        other => {
            if crate_lock && !allowed_path(&name) {
                let shown = other.unwrap_or_else(|| "none".to_owned());
                return Err(format!("{name} {version} source is not registry: {shown}"));
            }
            if crate_lock {
                paths.push(PathPkg { name, version });
            }
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
