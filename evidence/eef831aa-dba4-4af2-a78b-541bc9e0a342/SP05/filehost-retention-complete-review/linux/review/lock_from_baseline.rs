//! Cut a child Cargo.lock from the fixed root lock.
//!
//! `--walk` keeps registry checksums and applies the feature edges this crate
//! actually activates. `--superset` keeps every root pin and adds this package
//! so a later offline metadata rewrite cannot invent a version. `--check`
//! rejects any registry pin that is not in the fixed lock.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::env;
use std::fs;
use std::process::ExitCode;

const PATH_PACKAGES: &[&str] = &["artifact-retention", "oo-control", "oo-core", "oo-jobs"];

struct Pkg {
    name: String,
    version: String,
    source: String,
    checksum: String,
    deps: Vec<String>,
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(mode) = args.first().map(String::as_str) else {
        eprintln!("usage: lock_from_baseline --walk|--superset|--check ROOT CHILD");
        return ExitCode::from(2);
    };
    if args.len() != 3 {
        eprintln!("usage: lock_from_baseline --walk|--superset|--check ROOT CHILD");
        return ExitCode::from(2);
    }
    let root_text = match fs::read_to_string(&args[1]) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("read root lock: {error}");
            return ExitCode::from(2);
        }
    };
    let root = parse_lock(&root_text);
    if root.is_empty() {
        eprintln!("root lock has no packages");
        return ExitCode::from(2);
    }
    match mode {
        "--check" => check_child(&root, &args[2]),
        "--walk" => emit_walk(&root, &args[2]),
        "--superset" => emit_superset(&root, &args[2]),
        _ => {
            eprintln!("unknown mode {mode}");
            ExitCode::from(2)
        }
    }
}

fn check_child(root: &[Pkg], path: &str) -> ExitCode {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("read child lock: {error}");
            return ExitCode::from(2);
        }
    };
    let child = parse_lock(&text);
    if child.is_empty() {
        eprintln!("child lock has no packages");
        return ExitCode::from(1);
    }
    let mut registry = 0usize;
    for package in &child {
        if package.source.is_empty() {
            if !PATH_PACKAGES.contains(&package.name.as_str()) {
                eprintln!("unexpected path package {}", package.name);
                return ExitCode::from(1);
            }
            continue;
        }
        if !package.source.starts_with("registry+https://github.com/rust-lang/crates.io-index") {
            eprintln!("unexpected source for {} {}", package.name, package.version);
            return ExitCode::from(1);
        }
        if package.checksum.is_empty() {
            eprintln!("missing checksum for {} {}", package.name, package.version);
            return ExitCode::from(1);
        }
        let Some(pinned) = root
            .iter()
            .find(|item| item.name == package.name && item.version == package.version)
        else {
            eprintln!("root lock has no {} {}", package.name, package.version);
            return ExitCode::from(1);
        };
        if pinned.checksum != package.checksum || pinned.source != package.source {
            eprintln!("checksum drift for {} {}", package.name, package.version);
            return ExitCode::from(1);
        }
        registry += 1;
    }
    for name in ["artifact-retention", "oo-control", "oo-core", "oo-jobs"] {
        match child.iter().find(|item| item.name == name) {
            Some(package) if package.source.is_empty() => {}
            Some(_) => {
                eprintln!("{name} must stay a path package");
                return ExitCode::from(1);
            }
            None => {
                eprintln!("child lock is missing {name}");
                return ExitCode::from(1);
            }
        }
    }
    if registry == 0 {
        eprintln!("child lock has no registry pins");
        return ExitCode::from(1);
    }
    println!("check_ok registry_pins={registry} packages={}", child.len());
    ExitCode::SUCCESS
}

fn emit_walk(root: &[Pkg], path: &str) -> ExitCode {
    let mut packages = root.to_vec_pkgs();
    packages.push(artifact_retention());
    let mut by_key: BTreeMap<(String, String), Pkg> = BTreeMap::new();
    for package in packages {
        by_key.insert((package.name.clone(), package.version.clone()), package);
    }
    let start = ("artifact-retention".to_owned(), "0.1.0".to_owned());
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    let mut queue = VecDeque::from([start]);
    let mut selected: Vec<Pkg> = Vec::new();
    while let Some(key) = queue.pop_front() {
        if !seen.insert(key.clone()) {
            continue;
        }
        let Some(package) = by_key.get(&key) else {
            eprintln!("missing package {} {}", key.0, key.1);
            return ExitCode::from(1);
        };
        let deps = match override_deps(&package.name, &package.version) {
            Some(deps) => deps,
            None => package.deps.clone(),
        };
        for dep in &deps {
            let Some(resolved) = resolve(&by_key, dep) else {
                eprintln!("unresolved edge {dep} from {} {}", package.name, package.version);
                return ExitCode::from(1);
            };
            queue.push_back((resolved.name.clone(), resolved.version.clone()));
        }
        let mut copy = clone_pkg(package);
        copy.deps = deps;
        selected.push(copy);
    }
    selected.sort_by(cmp_pkg);
    match write_lock(path, &selected) {
        Ok(()) => {
            println!("walk_packages={}", selected.len());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("write lock: {error}");
            ExitCode::from(1)
        }
    }
}

fn emit_superset(root: &[Pkg], path: &str) -> ExitCode {
    let mut selected: Vec<Pkg> = root.iter().map(clone_pkg).collect();
    selected.push(artifact_retention());
    selected.sort_by(cmp_pkg);
    match write_lock(path, &selected) {
        Ok(()) => {
            println!("superset_packages={}", selected.len());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("write lock: {error}");
            ExitCode::from(1)
        }
    }
}

fn artifact_retention() -> Pkg {
    Pkg {
        name: "artifact-retention".to_owned(),
        version: "0.1.0".to_owned(),
        source: String::new(),
        checksum: String::new(),
        deps: vec![
            "oo-control".to_owned(),
            "sha2 0.10.9".to_owned(),
            "tokio 1.53.1".to_owned(),
        ],
    }
}

fn override_deps(name: &str, version: &str) -> Option<Vec<String>> {
    let deps: &[&str] = match (name, version) {
        ("artifact-retention", "0.1.0") => &["oo-control", "sha2 0.10.9", "tokio 1.53.1"],
        ("oo-control", "0.1.0") => &[
            "futures-util",
            "getrandom 0.2.17",
            "object_store",
            "oo-core",
            "oo-jobs",
            "sha2 0.10.9",
        ],
        ("oo-jobs", "0.1.0") => &["oo-core"],
        ("tokio", "1.53.1") => &["bytes", "pin-project-lite", "tokio-macros"],
        ("futures-util", "0.3.34") => &[
            "futures-core",
            "futures-macro",
            "futures-sink",
            "futures-task",
            "pin-project-lite",
            "slab",
        ],
        ("getrandom", "0.2.17") => &["cfg-if", "libc", "wasi 0.11.1+wasi-snapshot-preview1"],
        _ => return None,
    };
    Some(deps.iter().map(|item| (*item).to_owned()).collect())
}

fn resolve<'a>(by_key: &'a BTreeMap<(String, String), Pkg>, dep: &str) -> Option<&'a Pkg> {
    let (name, version) = split_dep(dep);
    if let Some(version) = version {
        return by_key.get(&(name.to_owned(), version.to_owned()));
    }
    let mut found = None;
    let mut count = 0usize;
    for package in by_key.values() {
        if package.name == name {
            count += 1;
            found = Some(package);
        }
    }
    if count == 1 { found } else { None }
}

fn write_lock(path: &str, packages: &[Pkg]) -> Result<(), String> {
    let mut names: BTreeMap<String, usize> = BTreeMap::new();
    for package in packages {
        *names.entry(package.name.clone()).or_insert(0) += 1;
    }
    let mut out = String::from(
        "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n",
    );
    for package in packages {
        out.push('\n');
        out.push_str("[[package]]\n");
        push_field(&mut out, "name", &package.name)?;
        push_field(&mut out, "version", &package.version)?;
        if !package.source.is_empty() {
            push_field(&mut out, "source", &package.source)?;
            push_field(&mut out, "checksum", &package.checksum)?;
        }
        if package.deps.is_empty() {
            continue;
        }
        let mut rendered = Vec::new();
        for dep in &package.deps {
            let (name, _) = split_dep(dep);
            let Some(count) = names.get(name) else {
                return Err(format!("dependency name {name} is not in the lock"));
            };
            if *count == 1 {
                rendered.push(name.to_owned());
            } else {
                let version = dep
                    .split_once(' ')
                    .map(|(_, version)| version)
                    .ok_or_else(|| format!("ambiguous dependency {dep} needs a version"))?;
                rendered.push(format!("{name} {version}"));
            }
        }
        rendered.sort();
        out.push_str("dependencies = [\n");
        for dep in rendered {
            push_quoted_line(&mut out, &dep)?;
        }
        out.push_str("]\n");
    }
    fs::write(path, out).map_err(|error| error.to_string())
}

fn push_field(out: &mut String, key: &str, value: &str) -> Result<(), String> {
    reject_unquoted(value)?;
    out.push_str(key);
    out.push_str(" = \"");
    out.push_str(value);
    out.push_str("\"\n");
    Ok(())
}

fn push_quoted_line(out: &mut String, value: &str) -> Result<(), String> {
    reject_unquoted(value)?;
    out.push_str(" \"");
    out.push_str(value);
    out.push_str("\",\n");
    Ok(())
}

fn reject_unquoted(value: &str) -> Result<(), String> {
    if value.contains('"') || value.contains('\\') || value.contains('\n') {
        return Err(format!("value cannot be quoted: {value}"));
    }
    Ok(())
}

fn split_dep(dep: &str) -> (&str, Option<&str>) {
    match dep.split_once(' ') {
        Some((name, version)) => (name, Some(version)),
        None => (dep, None),
    }
}

fn cmp_pkg(left: &Pkg, right: &Pkg) -> std::cmp::Ordering {
    left.name
        .cmp(&right.name)
        .then_with(|| cmp_version(&left.version, &right.version))
}

fn cmp_version(left: &str, right: &str) -> std::cmp::Ordering {
    let (left_num, left_build) = left.split_once('+').unwrap_or((left, ""));
    let (right_num, right_build) = right.split_once('+').unwrap_or((right, ""));
    let mut left_parts = left_num.split('.');
    let mut right_parts = right_num.split('.');
    loop {
        match (left_parts.next(), right_parts.next()) {
            (Some(left_part), Some(right_part)) => {
                let left_value = left_part.parse::<u64>().ok();
                let right_value = right_part.parse::<u64>().ok();
                match (left_value, right_value) {
                    (Some(left_n), Some(right_n)) => {
                        let order = left_n.cmp(&right_n);
                        if order != std::cmp::Ordering::Equal {
                            return order;
                        }
                    }
                    _ => {
                        let order = left_part.cmp(right_part);
                        if order != std::cmp::Ordering::Equal {
                            return order;
                        }
                    }
                }
            }
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (None, None) => return left_build.cmp(right_build),
        }
    }
}

fn parse_lock(text: &str) -> Vec<Pkg> {
    let mut packages = Vec::new();
    let mut current: Option<Pkg> = None;
    let mut in_deps = false;
    for line in text.lines() {
        if line == "[[package]]" {
            if let Some(package) = current.take() {
                packages.push(package);
            }
            current = Some(empty_pkg());
            in_deps = false;
            continue;
        }
        let Some(package) = current.as_mut() else {
            continue;
        };
        if in_deps {
            if line == "]" {
                in_deps = false;
            } else if let Some(dep) = quoted(line.trim().trim_end_matches(',')) {
                package.deps.push(dep);
            }
            continue;
        }
        if line == "dependencies = [" {
            in_deps = true;
            continue;
        }
        if let Some(value) = field(line, "name") {
            package.name = value;
        } else if let Some(value) = field(line, "version") {
            package.version = value;
        } else if let Some(value) = field(line, "source") {
            package.source = value;
        } else if let Some(value) = field(line, "checksum") {
            package.checksum = value;
        }
    }
    if let Some(package) = current {
        packages.push(package);
    }
    packages
}

fn field(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?.strip_prefix(" = ")?;
    quoted(rest)
}

fn quoted(value: &str) -> Option<String> {
    let value = value.trim();
    let value = value.strip_prefix('"')?.strip_suffix('"')?;
    if value.contains('\\') {
        return None;
    }
    Some(value.to_owned())
}

fn empty_pkg() -> Pkg {
    Pkg {
        name: String::new(),
        version: String::new(),
        source: String::new(),
        checksum: String::new(),
        deps: Vec::new(),
    }
}

fn clone_pkg(package: &Pkg) -> Pkg {
    Pkg {
        name: package.name.clone(),
        version: package.version.clone(),
        source: package.source.clone(),
        checksum: package.checksum.clone(),
        deps: package.deps.clone(),
    }
}

trait ToVecPkgs {
    fn to_vec_pkgs(&self) -> Vec<Pkg>;
}

impl ToVecPkgs for [Pkg] {
    fn to_vec_pkgs(&self) -> Vec<Pkg> {
        self.iter().map(clone_pkg).collect()
    }
}
