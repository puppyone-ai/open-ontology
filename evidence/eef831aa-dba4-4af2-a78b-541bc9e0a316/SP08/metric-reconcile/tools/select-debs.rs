//! Select Debian packages for the private libclang tree.
//! Reads a Packages file and an installed-package list. No Python.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::env;
use std::fs;
use std::process::ExitCode;

struct Package {
    filename: String,
    sha256: String,
    depends: Vec<String>,
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(packages_path) = args.next() else {
        eprintln!("usage: select-debs PACKAGES INSTALLED OUT");
        return ExitCode::from(2);
    };
    let Some(installed_path) = args.next() else {
        eprintln!("usage: select-debs PACKAGES INSTALLED OUT");
        return ExitCode::from(2);
    };
    let Some(out_path) = args.next() else {
        eprintln!("usage: select-debs PACKAGES INSTALLED OUT");
        return ExitCode::from(2);
    };
    let packages_text = match fs::read_to_string(&packages_path) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("read packages: {err}");
            return ExitCode::from(1);
        }
    };
    let installed_text = match fs::read_to_string(&installed_path) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("read installed: {err}");
            return ExitCode::from(1);
        }
    };
    let packages = parse_packages(&packages_text);
    let installed: BTreeSet<String> = installed_text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    let want = ["libclang-19-dev", "libclang1-19", "pkgconf"];
    let mut queue: VecDeque<String> = want.iter().map(|name| (*name).to_string()).collect();
    let mut seen = installed.clone();
    let mut selected = Vec::new();
    while let Some(name) = queue.pop_front() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let Some(package) = packages.get(&name) else {
            eprintln!("missing package metadata: {name}");
            return ExitCode::from(1);
        };
        selected.push((name, package));
        for dep in &package.depends {
            if !seen.contains(dep) {
                queue.push_back(dep.clone());
            }
        }
    }
    let mut lines = String::new();
    for (name, package) in &selected {
        lines.push_str(name);
        lines.push(' ');
        lines.push_str(&package.filename);
        lines.push(' ');
        lines.push_str(&package.sha256);
        lines.push('\n');
    }
    if let Err(err) = fs::write(&out_path, lines) {
        eprintln!("write list: {err}");
        return ExitCode::from(1);
    }
    println!("selected={}", selected.len());
    ExitCode::SUCCESS
}

fn parse_packages(text: &str) -> BTreeMap<String, Package> {
    let mut packages = BTreeMap::new();
    for stanza in text.split("\n\n") {
        let mut fields: BTreeMap<String, String> = BTreeMap::new();
        let mut key = String::new();
        for line in stanza.lines() {
            if line.is_empty() {
                continue;
            }
            if line.starts_with(' ') && !key.is_empty() {
                if let Some(value) = fields.get_mut(&key) {
                    value.push(' ');
                    value.push_str(line.trim());
                }
                continue;
            }
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            key = name.trim().to_string();
            fields.insert(key.clone(), value.trim().to_string());
        }
        let Some(name) = fields.get("Package").cloned() else {
            continue;
        };
        let filename = fields.get("Filename").cloned().unwrap_or_default();
        let sha256 = fields.get("SHA256").cloned().unwrap_or_default();
        let depends_raw = format!(
            "{}, {}",
            fields.get("Depends").map(String::as_str).unwrap_or(""),
            fields.get("Pre-Depends").map(String::as_str).unwrap_or("")
        );
        packages.insert(
            name,
            Package {
                filename,
                sha256,
                depends: dep_names(&depends_raw),
            },
        );
    }
    packages
}

fn dep_names(raw: &str) -> Vec<String> {
    let mut names = Vec::new();
    for part in raw.split(',') {
        let Some(alt) = part.split('|').next() else {
            continue;
        };
        let alt = alt.trim();
        if alt.is_empty() {
            continue;
        }
        let name = alt.split('(').next().unwrap_or("").trim();
        let name = name.split(':').next().unwrap_or("").trim();
        if !name.is_empty() {
            names.push(name.to_string());
        }
    }
    names
}
