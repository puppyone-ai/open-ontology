//! Review oracle for registry lock triples. Std only. Not a product test.
//! A triple is (name, version, checksum). Checksums are not printed.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("lock_triples: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let release_path = args.next().ok_or("usage: lock_triples <release> <store> <reducer> <root>")?;
    let store_path = args.next().ok_or("usage: lock_triples <release> <store> <reducer> <root>")?;
    let reducer_path = args.next().ok_or("usage: lock_triples <release> <store> <reducer> <root>")?;
    let root_path = args.next().ok_or("usage: lock_triples <release> <store> <reducer> <root>")?;
    if args.next().is_some() {
        return Err("usage: lock_triples <release> <store> <reducer> <root>".to_owned());
    }
    let release = fs::read_to_string(&release_path).map_err(|err| format!("read release: {err}"))?;
    let store = fs::read_to_string(&store_path).map_err(|err| format!("read store: {err}"))?;
    let reducer = fs::read_to_string(&reducer_path).map_err(|err| format!("read reducer: {err}"))?;
    let root = fs::read_to_string(&root_path).map_err(|err| format!("read root: {err}"))?;
    let release_regs = registry(&release)?;
    let store_regs = registry(&store)?;
    let reducer_regs = registry(&reducer)?;
    let root_regs = registry(&root)?;
    let mut missing = 0u64;
    let mut mismatch = 0u64;
    if release_regs != store_regs {
        differ("store", &release_regs, &store_regs, &mut missing, &mut mismatch);
        differ("release", &store_regs, &release_regs, &mut missing, &mut mismatch);
    }
    subset("root", "release", &release_regs, &root_regs, &mut missing, &mut mismatch);
    subset("root", "reducer", &reducer_regs, &root_regs, &mut missing, &mut mismatch);
    subset("store", "reducer", &reducer_regs, &store_regs, &mut missing, &mut mismatch);
    path_local(&release, "session-release")?;
    path_local(&release, "session-store")?;
    path_local(&release, "session-transition")?;
    path_local(&store, "session-store")?;
    path_local(&store, "session-transition")?;
    path_local(&reducer, "session-transition")?;
    let pins = [
        ("serde", "1.0.229"),
        ("serde_json", "1.0.151"),
        ("tokio", "1.53.1"),
        ("tokio-postgres", "0.7.18"),
    ];
    let mut direct_pins_present = true;
    for (name, version) in pins {
        if !release_regs.contains_key(&(name.to_owned(), version.to_owned())) {
            direct_pins_present = false;
            missing += 1;
            eprintln!("release missing direct pin {name} {version}");
        }
    }
    println!(
        "summary release_registry={} store_registry={} reducer_registry={} root_registry={} missing={} mismatch={} direct_pins_present={}",
        release_regs.len(),
        store_regs.len(),
        reducer_regs.len(),
        root_regs.len(),
        missing,
        mismatch,
        direct_pins_present
    );
    if missing != 0 || mismatch != 0 {
        return Err("lock triples differ".to_owned());
    }
    Ok(())
}

fn differ(
    other_name: &str,
    left: &BTreeMap<(String, String), String>,
    right: &BTreeMap<(String, String), String>,
    missing: &mut u64,
    mismatch: &mut u64,
) {
    for (key, checksum) in left {
        match right.get(key) {
            None => {
                *missing += 1;
                eprintln!("{other_name} missing {} {}", key.0, key.1);
            }
            Some(other) if other != checksum => {
                *mismatch += 1;
                eprintln!("{other_name} checksum differs {} {}", key.0, key.1);
            }
            Some(_) => {}
        }
    }
}

fn subset(
    holder: &str,
    origin: &str,
    needles: &BTreeMap<(String, String), String>,
    haystack: &BTreeMap<(String, String), String>,
    missing: &mut u64,
    mismatch: &mut u64,
) {
    for (key, checksum) in needles {
        match haystack.get(key) {
            None => {
                *missing += 1;
                eprintln!("{holder} missing {origin} {} {}", key.0, key.1);
            }
            Some(other) if other != checksum => {
                *mismatch += 1;
                eprintln!("{holder} checksum differs for {origin} {} {}", key.0, key.1);
            }
            Some(_) => {}
        }
    }
}

fn path_local(text: &str, name: &str) -> Result<(), String> {
    let mut found = false;
    for package in packages(text)? {
        if package.name != name {
            continue;
        }
        found = true;
        if package.source.starts_with("registry+") {
            return Err(format!("{name} is a registry package"));
        }
    }
    if !found {
        return Err(format!("{name} is absent"));
    }
    Ok(())
}

fn registry(text: &str) -> Result<BTreeMap<(String, String), String>, String> {
    let mut pins = BTreeMap::new();
    for package in packages(text)? {
        if !package.source.starts_with("registry+") {
            continue;
        }
        if package.checksum.len() != 64 || !package.checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(format!("bad checksum length for {} {}", package.name, package.version));
        }
        let key = (package.name.clone(), package.version.clone());
        if let Some(previous) = pins.insert(key, package.checksum.clone()) {
            if previous != package.checksum {
                return Err(format!("duplicate pin {} {}", package.name, package.version));
            }
        }
    }
    Ok(pins)
}

struct Package {
    name: String,
    version: String,
    source: String,
    checksum: String,
}

fn packages(text: &str) -> Result<Vec<Package>, String> {
    let mut out = Vec::new();
    let mut current: Option<Package> = None;
    for line in text.lines() {
        if line == "[[package]]" {
            if let Some(package) = current.take() {
                out.push(package);
            }
            current = Some(Package {
                name: String::new(),
                version: String::new(),
                source: String::new(),
                checksum: String::new(),
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
        } else if let Some(value) = line.strip_prefix("source = ") {
            package.source = unquote(value);
        } else if let Some(value) = line.strip_prefix("checksum = ") {
            package.checksum = unquote(value);
        }
    }
    if let Some(package) = current {
        out.push(package);
    }
    if out.is_empty() {
        return Err("lock has no packages".to_owned());
    }
    Ok(out)
}

fn unquote(value: &str) -> String {
    value.trim().trim_matches('"').to_owned()
}
