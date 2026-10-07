//! Assemble tools/ontology-dialogue/Cargo.lock from committed lock stanzas.
//! Every registry checksum must already appear in one of the input locks.
//! This program does not contact a registry and does not rewrite the root lock.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::env;
use std::fs;
use std::process::ExitCode;

#[derive(Clone)]
struct Pkg {
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
    deps: Vec<(String, String)>,
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let check = matches!(args.next().as_deref(), Some("--check"));
    let rest: Vec<String> = if check {
        args.collect()
    } else {
        let mut all = vec![env::args().nth(1).unwrap_or_default()];
        all.extend(args);
        all.into_iter().filter(|item| !item.is_empty()).collect()
    };
    if rest.len() != 4 {
        eprintln!("usage: lock-assemble [--check] <description-provider.lock> <root.lock> <answer-citations.lock> <output>");
        return ExitCode::from(2);
    }
    let locks = vec![
        parse_lock(&fs::read_to_string(&rest[0]).expect("read dp")),
        parse_lock(&fs::read_to_string(&rest[1]).expect("read root")),
        parse_lock(&fs::read_to_string(&rest[2]).expect("read citations")),
    ];
    let allowed = checksums_of(&[&rest[0], &rest[1], &rest[2]]);
    if check {
        let existing = fs::read_to_string(&rest[3]).expect("read existing");
        let mut bad = 0_u32;
        for pkg in parse_lock(&existing) {
            if let Some(checksum) = &pkg.checksum {
                if !allowed.contains(checksum) {
                    eprintln!("new checksum {} {} {}", pkg.name, pkg.version, checksum);
                    bad += 1;
                }
            }
        }
        if bad != 0 {
            eprintln!("checksums_outside_committed_locks={bad}");
            return ExitCode::from(1);
        }
        println!("checksum_subset=ok");
        return ExitCode::SUCCESS;
    }
    let selected = walk(&locks);
    let text = emit(&selected);
    for pkg in parse_lock(&text) {
        if let Some(checksum) = &pkg.checksum {
            if !allowed.contains(checksum) {
                eprintln!("assembler emitted a new checksum {} {}", pkg.name, pkg.version);
                return ExitCode::from(1);
            }
        }
    }
    fs::write(&rest[3], text).expect("write lock");
    println!("packages={}", selected.len());
    ExitCode::SUCCESS
}

fn checksums_of(paths: &[&String]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for path in paths {
        let text = fs::read_to_string(path).expect("checksum source");
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(value) = trimmed.strip_prefix("checksum = \"") {
                if let Some(checksum) = value.strip_suffix('"') {
                    out.insert(checksum.to_string());
                }
            }
        }
    }
    out
}

fn walk(locks: &[Vec<Pkg>]) -> BTreeMap<(String, String), Pkg> {
    let seeds = [
        ("answer-citations", "0.1.0", 2_usize),
        ("reqwest", "0.13.5", 0),
        ("serde", "1.0.229", 0),
        ("serde_json", "1.0.151", 0),
        ("sha2", "0.10.9", 0),
        ("tokio", "1.53.1", 0),
        ("hyper-util", "0.1.20", 0),
        ("tokio-rustls", "0.26.5", 0),
        ("rustls-platform-verifier", "0.7.0", 0),
        ("cc", "1.4.7", 0),
        ("find-msvc-tools", "0.1.13", 0),
        ("yoke-derive", "0.8.2", 0),
        ("rustls", "0.23.45", 0),
        ("tokio-postgres", "0.7.18", 1),
        ("postgres-types", "0.2.14", 1),
        ("tokio-postgres-rustls", "0.14.0", 1),
    ];
    let mut selected: BTreeMap<(String, String), Pkg> = BTreeMap::new();
    let mut queue = VecDeque::new();
    for (name, version, origin) in seeds {
        queue.push_back((name.to_string(), version.to_string(), origin));
    }
    while let Some((name, version, origin)) = queue.pop_front() {
        let key = (name.clone(), version.clone());
        if selected.contains_key(&key) {
            continue;
        }
        let pkg = find_pkg(locks, &name, &version, origin).unwrap_or_else(|| {
            panic!("missing package {name} {version}");
        });
        for dep in &pkg.deps {
            queue.push_back((dep.0.clone(), dep.1.clone(), origin));
        }
        selected.insert(key, pkg);
    }
    // Cargo's resolve for this manifest keeps description-provider edges, then
    // activates zeroize's derive feature and futures-sink, and it does not
    // activate hashbrown's default allocator features or indexmap's serde feature.
    // Those stanzas stay inside the three committed locks. Proof-1 showed the
    // smaller description-provider edges make `cargo fetch --locked` reject the file.
    let derive = find_pkg(locks, "zeroize_derive", "1.5.0", 1).expect("zeroize_derive");
    selected.insert(("zeroize_derive".to_string(), "1.5.0".to_string()), derive);
    add_dep(&mut selected, "zeroize", "1.9.0", "zeroize_derive");
    add_dep(&mut selected, "futures-channel", "0.3.34", "futures-sink");
    add_dep(&mut selected, "futures-util", "0.3.34", "futures-sink");
    remove_dep(&mut selected, "hashbrown", "0.17.1", "allocator-api2");
    remove_dep(&mut selected, "hashbrown", "0.17.1", "equivalent");
    remove_dep(&mut selected, "hashbrown", "0.17.1", "foldhash");
    remove_dep(&mut selected, "indexmap", "2.14.2", "serde");
    remove_dep(&mut selected, "indexmap", "2.14.2", "serde_core");
    selected.remove(&("allocator-api2".to_string(), "0.2.21".to_string()));
    selected.remove(&("foldhash".to_string(), "0.2.0".to_string()));
    selected
}

fn add_dep(selected: &mut BTreeMap<(String, String), Pkg>, name: &str, version: &str, dep: &str) {
    let versions: Vec<String> = selected
        .keys()
        .filter(|(pkg, _)| pkg == dep)
        .map(|(_, dep_version)| dep_version.clone())
        .collect();
    if versions.len() != 1 {
        panic!("{name} {version} dep {dep} matches {} versions", versions.len());
    }
    let pkg = selected
        .get_mut(&(name.to_string(), version.to_string()))
        .unwrap_or_else(|| panic!("missing {name} {version}"));
    pkg.deps.push((dep.to_string(), versions[0].clone()));
    pkg.deps.sort();
    pkg.deps.dedup();
}

fn remove_dep(selected: &mut BTreeMap<(String, String), Pkg>, name: &str, version: &str, dep: &str) {
    let pkg = selected
        .get_mut(&(name.to_string(), version.to_string()))
        .unwrap_or_else(|| panic!("missing {name} {version}"));
    pkg.deps.retain(|(dep_name, _)| dep_name != dep);
}

fn find_pkg(locks: &[Vec<Pkg>], name: &str, version: &str, origin: usize) -> Option<Pkg> {
    let _ = origin;
    let order = [0_usize, 2, 1];
    for index in order {
        if let Some(pkg) = locks[index]
            .iter()
            .find(|pkg| pkg.name == name && pkg.version == version)
        {
            for later in order.iter().skip(1) {
                if let Some(other) = locks[*later]
                    .iter()
                    .find(|item| item.name == name && item.version == version)
                {
                    if other.checksum != pkg.checksum {
                        panic!("checksum mismatch for {name} {version}");
                    }
                }
            }
            return Some(pkg.clone());
        }
    }
    None
}

fn emit(selected: &BTreeMap<(String, String), Pkg>) -> String {
    let versions_of = |name: &str| -> usize {
        selected.keys().filter(|(pkg, _)| pkg == name).count()
    };
    let mut packages = Vec::new();
    packages.push(root_pkg(selected, versions_of));
    for pkg in selected.values() {
        packages.push(pkg.clone());
    }
    packages.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.version.cmp(&right.version))
    });
    let mut out = String::from(
        "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n",
    );
    for pkg in packages {
        out.push_str("\n[[package]]\n");
        out.push_str(&format!("name = \"{}\"\n", pkg.name));
        out.push_str(&format!("version = \"{}\"\n", pkg.version));
        if let Some(source) = &pkg.source {
            out.push_str(&format!("source = \"{source}\"\n"));
        }
        if let Some(checksum) = &pkg.checksum {
            out.push_str(&format!("checksum = \"{checksum}\"\n"));
        }
        if !pkg.deps.is_empty() {
            out.push_str("dependencies = [\n");
            for (name, version) in &pkg.deps {
                if versions_of(name) == 1 {
                    out.push_str(&format!(" \"{name}\",\n"));
                } else {
                    out.push_str(&format!(" \"{name} {version}\",\n"));
                }
            }
            out.push_str("]\n");
        }
    }
    out
}

fn root_pkg(
    selected: &BTreeMap<(String, String), Pkg>,
    versions_of: impl Fn(&str) -> usize,
) -> Pkg {
    let direct = [
        "answer-citations",
        "cc",
        "find-msvc-tools",
        "hyper-util",
        "postgres-types",
        "reqwest",
        "rustls",
        "rustls-platform-verifier",
        "serde",
        "serde_json",
        "sha2",
        "tokio",
        "tokio-postgres",
        "tokio-postgres-rustls",
        "tokio-rustls",
        "yoke-derive",
    ];
    let mut deps = Vec::new();
    for name in direct {
        let version = if name == "sha2" {
            "0.10.9"
        } else {
            selected
                .keys()
                .find(|(pkg, _)| pkg == name)
                .map(|(_, version)| version.as_str())
                .unwrap_or_else(|| panic!("root dep {name}"))
        };
        if versions_of(name) == 0 {
            panic!("root dep {name} was not selected");
        }
        deps.push((name.to_string(), version.to_string()));
    }
    deps.sort();
    Pkg {
        name: "ontology-dialogue".to_string(),
        version: "0.1.0".to_string(),
        source: None,
        checksum: None,
        deps,
    }
}

fn parse_lock(text: &str) -> Vec<Pkg> {
    let raws = parse_raw(text);
    let mut versions: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for raw in &raws {
        versions.entry(raw.name.clone()).or_default().push(raw.version.clone());
    }
    raws.into_iter().map(|raw| finish(raw, &versions)).collect()
}

fn parse_raw(text: &str) -> Vec<Raw> {
    let mut packages = Vec::new();
    let mut current: Option<Raw> = None;
    let mut in_deps = false;
    for line in text.lines() {
        if line == "[[package]]" {
            if let Some(raw) = current.take() {
                packages.push(raw);
            }
            current = Some(Raw::default());
            in_deps = false;
            continue;
        }
        let Some(raw) = current.as_mut() else {
            continue;
        };
        if in_deps {
            if line.trim() == "]" {
                in_deps = false;
            } else if let Some(dep) = quoted(line.trim().trim_end_matches(',')) {
                raw.deps.push(dep);
            }
            continue;
        }
        if line == "dependencies = [" {
            in_deps = true;
            continue;
        }
        if let Some(value) = field(line, "name") {
            raw.name = value;
        } else if let Some(value) = field(line, "version") {
            raw.version = value;
        } else if let Some(value) = field(line, "source") {
            raw.source = Some(value);
        } else if let Some(value) = field(line, "checksum") {
            raw.checksum = Some(value);
        }
    }
    if let Some(raw) = current {
        packages.push(raw);
    }
    packages
}

#[derive(Default)]
struct Raw {
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
    deps: Vec<String>,
}

fn finish(raw: Raw, versions: &BTreeMap<String, Vec<String>>) -> Pkg {
    let mut deps = Vec::new();
    for dep in raw.deps {
        let (name, version) = if let Some((name, version)) = dep.split_once(' ') {
            (name.to_string(), version.to_string())
        } else {
            let list = versions.get(&dep).unwrap_or_else(|| {
                panic!("{} {} is missing dep {dep}", raw.name, raw.version);
            });
            if list.len() != 1 {
                panic!(
                    "{} {} dep {dep} matches {} versions",
                    raw.name,
                    raw.version,
                    list.len()
                );
            }
            (dep, list[0].clone())
        };
        deps.push((name, version));
    }
    deps.sort();
    deps.dedup();
    Pkg {
        name: raw.name,
        version: raw.version,
        source: raw.source,
        checksum: raw.checksum,
        deps,
    }
}

fn field(line: &str, key: &str) -> Option<String> {
    let prefix = format!("{key} = ");
    let rest = line.trim().strip_prefix(&prefix)?;
    quoted(rest)
}

fn quoted(text: &str) -> Option<String> {
    let text = text.trim();
    let inner = text.strip_prefix('"')?.strip_suffix('"')?;
    Some(inner.to_string())
}
