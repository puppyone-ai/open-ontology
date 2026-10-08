#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::Path;

#[test]
fn registry_pins_match_store_token_union_and_root_lock() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ours = std::fs::read_to_string(manifest.join("Cargo.lock")).expect("session-control-api lock");
    let store = std::fs::read_to_string(manifest.join("../session-store/Cargo.lock")).expect("session-store lock");
    let token = std::fs::read_to_string(manifest.join("../gateway-token-store/Cargo.lock")).expect("token lock");
    let release = std::fs::read_to_string(manifest.join("../session-release/Cargo.lock")).expect("release lock");
    let root_path = std::env::var("OO_ROOT_LOCK").expect("OO_ROOT_LOCK");
    let root = std::fs::read_to_string(&root_path).expect("root lock");
    let mut allowed = registry_pins(&store);
    for (key, checksum) in registry_pins(&token) {
        if let Some(previous) = allowed.insert(key.clone(), checksum.clone()) {
            assert_eq!(previous, checksum, "store/token pin drift {} {}", key.0, key.1);
        }
    }
    for (key, checksum) in registry_pins(&release) {
        let Some(previous) = allowed.get(&key) else {
            panic!("session-release pin {} {} is outside the store/token union", key.0, key.1);
        };
        assert_eq!(previous, &checksum, "release pin drift {} {}", key.0, key.1);
    }
    let ours_pins = registry_pins(&ours);
    let root_pins = registry_pins(&root);
    for (key, checksum) in &ours_pins {
        let Some(allowed_checksum) = allowed.get(key) else {
            panic!("new registry crate {} {} is not in session-store or gateway-token-store", key.0, key.1);
        };
        assert_eq!(allowed_checksum, checksum, "checksum drift {} {}", key.0, key.1);
        let Some(root_checksum) = root_pins.get(key) else {
            panic!("root lock has no {} {}", key.0, key.1);
        };
        assert_eq!(root_checksum, checksum, "root checksum drift {} {}", key.0, key.1);
    }
    assert!(ours.contains("name = \"session-control-api\""));
    assert!(!package_has_registry_source(&ours, "session-control-api"));
    let manifest_text = std::fs::read_to_string(manifest.join("Cargo.toml")).expect("manifest");
    for banned in ["axum", "sha2", "ring"] {
        assert!(!direct_dependency(&manifest_text, banned), "{banned} is a direct dependency");
    }
}

fn direct_dependency(toml: &str, name: &str) -> bool {
    let mut section = false;
    for line in toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            section = trimmed == "[dependencies]" || trimmed == "[dev-dependencies]";
            continue;
        }
        if section && (trimmed.starts_with(&format!("{name} ")) || trimmed.starts_with(&format!("{name}="))) {
            return true;
        }
    }
    false
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
