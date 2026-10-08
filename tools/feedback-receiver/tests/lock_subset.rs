//! Child registry checksums must already exist in the root lock.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

#[test]
fn child_lock_is_a_root_checksum_subset() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = fs::read_to_string(manifest.join("../../Cargo.lock")).expect("root lock");
    let root_packages = registry_packages(&root);
    let text = fs::read_to_string(manifest.join("Cargo.lock")).expect("child lock");
    assert!(!text.contains("git+"), "git source");
    for ((name, version), checksum) in registry_packages(&text) {
        match root_packages.get(&(name.clone(), version.clone())) {
            Some(expected) => assert_eq!(expected, &checksum, "{name} {version}"),
            None => panic!("checksum not in root lock: {name} {version}"),
        }
    }
}

fn registry_packages(text: &str) -> BTreeMap<(String, String), String> {
    let mut out = BTreeMap::new();
    let mut name = String::new();
    let mut version = String::new();
    let mut source = String::new();
    let mut checksum = String::new();
    let mut open = false;
    let flush = |open: &mut bool,
                 name: &mut String,
                 version: &mut String,
                 source: &mut String,
                 checksum: &mut String,
                 out: &mut BTreeMap<(String, String), String>| {
        if *open && source.starts_with("registry+") {
            assert!(!checksum.is_empty(), "registry package {name} {version} has no checksum");
            out.insert((name.clone(), version.clone()), checksum.clone());
        }
        *open = false;
        name.clear();
        version.clear();
        source.clear();
        checksum.clear();
    };
    for line in text.lines() {
        if line == "[[package]]" {
            flush(&mut open, &mut name, &mut version, &mut source, &mut checksum, &mut out);
            open = true;
            continue;
        }
        if !open {
            continue;
        }
        if let Some(value) = line.strip_prefix("name = ") {
            name = unquote(value);
        } else if let Some(value) = line.strip_prefix("version = ") {
            version = unquote(value);
        } else if let Some(value) = line.strip_prefix("source = ") {
            source = unquote(value);
        } else if let Some(value) = line.strip_prefix("checksum = ") {
            checksum = unquote(value);
        }
    }
    flush(&mut open, &mut name, &mut version, &mut source, &mut checksum, &mut out);
    out
}

fn unquote(value: &str) -> String {
    value.trim().trim_matches('"').to_string()
}
