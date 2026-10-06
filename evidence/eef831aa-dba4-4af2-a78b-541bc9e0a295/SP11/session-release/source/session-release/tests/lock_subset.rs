#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::Path;

#[test]
fn registry_checksums_match_session_store_and_root_lock() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ours = std::fs::read_to_string(manifest.join("Cargo.lock")).expect("session-release lock");
    let store = std::fs::read_to_string(manifest.join("../session-store/Cargo.lock")).expect("session-store lock");
    let root_path = std::env::var("OO_ROOT_LOCK").expect("OO_ROOT_LOCK");
    let root = std::fs::read_to_string(&root_path).expect("root lock");
    let our_regs = registry_pins(&ours);
    let store_regs = registry_pins(&store);
    assert_eq!(our_regs, store_regs, "session-release registry pins differ from session-store");
    let root_regs = registry_pins(&root);
    for (key, checksum) in &our_regs {
        let Some(root_checksum) = root_regs.get(key) else {
            panic!("root lock has no {} {}", key.0, key.1);
        };
        assert_eq!(root_checksum, checksum, "checksum drift for {} {}", key.0, key.1);
    }
    assert!(ours.contains("name = \"session-release\""));
    assert!(!package_has_registry_source(&ours, "session-release"));
}

fn package_has_registry_source(text: &str, name: &str) -> bool {
    packages(text).into_iter().any(|package| package.name == name && package.source.starts_with("registry+"))
}

fn registry_pins(text: &str) -> BTreeMap<(String, String), String> {
    let mut pins = BTreeMap::new();
    for package in packages(text) {
        if !package.source.starts_with("registry+") {
            continue;
        }
        assert!(!package.checksum.is_empty(), "missing checksum for {} {}", package.name, package.version);
        let previous = pins.insert((package.name.clone(), package.version.clone()), package.checksum.clone());
        if let Some(previous) = previous {
            assert_eq!(previous, package.checksum, "duplicate pin {} {}", package.name, package.version);
        }
    }
    pins
}

struct Package {
    name: String,
    version: String,
    source: String,
    checksum: String,
}

fn packages(text: &str) -> Vec<Package> {
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
    out
}

fn unquote(value: &str) -> String {
    value.trim().trim_matches('"').to_owned()
}
