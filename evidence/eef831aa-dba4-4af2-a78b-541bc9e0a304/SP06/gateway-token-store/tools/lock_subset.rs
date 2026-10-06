//! Compare a crate lock to a root lock by registry name, version, and checksum.
//! A missing or mismatched triple fails before any fetch. This tool does not
//! update a lock.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::env;
use std::fs;
use std::process::ExitCode;

struct Package {
    name: String,
    version: String,
    checksum: String,
    registry: bool,
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let root_path = match args.next() {
        Some(path) => path,
        None => {
            eprintln!("usage: lock_subset <root-lock> <crate-lock>");
            return ExitCode::from(2);
        }
    };
    let crate_path = match args.next() {
        Some(path) => path,
        None => {
            eprintln!("usage: lock_subset <root-lock> <crate-lock>");
            return ExitCode::from(2);
        }
    };
    if args.next().is_some() {
        eprintln!("usage: lock_subset <root-lock> <crate-lock>");
        return ExitCode::from(2);
    }
    let root_text = match fs::read_to_string(&root_path) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("read root lock: {err}");
            return ExitCode::from(2);
        }
    };
    let crate_text = match fs::read_to_string(&crate_path) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("read crate lock: {err}");
            return ExitCode::from(2);
        }
    };
    let root = packages(&root_text);
    let crate_packages = packages(&crate_text);
    let mut root_registry = HashMap::<(String, String), String>::new();
    for package in &root {
        if package.registry {
            root_registry.insert((package.name.clone(), package.version.clone()), package.checksum.clone());
        }
    }
    let mut failed = false;
    let mut registry_count = 0u32;
    let mut path_names = Vec::new();
    for package in &crate_packages {
        if !package.registry {
            path_names.push(package.name.as_str());
            continue;
        }
        registry_count += 1;
        match root_registry.get(&(package.name.clone(), package.version.clone())) {
            Some(checksum) if checksum == &package.checksum && !package.checksum.is_empty() => {}
            Some(_) => {
                eprintln!("checksum {} {}", package.name, package.version);
                failed = true;
            }
            None => {
                eprintln!("missing {} {}", package.name, package.version);
                failed = true;
            }
        }
    }
    if path_names != ["gateway-token-store"] {
        eprintln!("path {}", path_names.join(","));
        failed = true;
    }
    if failed {
        eprintln!("subset_failed registry={registry_count}");
        return ExitCode::from(1);
    }
    println!("subset_ok registry={registry_count} path=gateway-token-store");
    ExitCode::SUCCESS
}

fn packages(text: &str) -> Vec<Package> {
    let mut out = Vec::new();
    let mut current: Option<Package> = None;
    for line in text.lines() {
        if line.starts_with("[[package]]") {
            if let Some(package) = current.take() {
                out.push(package);
            }
            current = Some(Package {
                name: String::new(),
                version: String::new(),
                checksum: String::new(),
                registry: false,
            });
            continue;
        }
        let Some(package) = current.as_mut() else {
            continue;
        };
        if let Some(value) = line.strip_prefix("name = ") {
            package.name = unquote(value);
        } else if let Some(value) = line.strip_prefix("version = ") {
            package.version = unquote(value);
        } else if let Some(value) = line.strip_prefix("checksum = ") {
            package.checksum = unquote(value);
        } else if line.starts_with("source = ") {
            package.registry = true;
        }
    }
    if let Some(package) = current {
        out.push(package);
    }
    out
}

fn unquote(value: &str) -> String {
    value.trim().trim_matches('"').to_owned()
}
