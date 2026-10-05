//! Compare two Cargo.lock files. The only allowed change is the oo-gateway
//! dependency list gaining serde and serde_json.

use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let before_path = args.next().expect("before");
    let after_path = args.next().expect("after");
    let before = fs::read_to_string(&before_path).expect("read before");
    let after = fs::read_to_string(&after_path).expect("read after");
    let before_pkgs = split_packages(&before);
    let after_pkgs = split_packages(&after);
    if before_pkgs.0 != after_pkgs.0 {
        eprintln!("lock preamble changed");
        return ExitCode::from(1);
    }
    if before_pkgs.1.len() != after_pkgs.1.len() {
        eprintln!(
            "package count {} -> {}",
            before_pkgs.1.len(),
            after_pkgs.1.len()
        );
        return ExitCode::from(1);
    }
    for (left, right) in before_pkgs.1.iter().zip(&after_pkgs.1) {
        if left.0 != right.0 {
            eprintln!("package order changed at {}", left.0);
            return ExitCode::from(1);
        }
        if left.0 == "oo-gateway" {
            if !gateway_edge_only(&left.1, &right.1) {
                eprintln!("oo-gateway block changed outside the dependency edge");
                return ExitCode::from(1);
            }
        } else if left.1 != right.1 {
            eprintln!("package block changed: {}", left.0);
            return ExitCode::from(1);
        }
    }
    println!("lock_edge=serde,serde_json");
    ExitCode::SUCCESS
}

fn split_packages(text: &str) -> (String, Vec<(String, String)>) {
    let mut parts = text.split("[[package]]\n");
    let preamble = parts.next().unwrap_or("").to_string();
    let mut packages = Vec::new();
    for part in parts {
        let mut name = String::new();
        for line in part.lines() {
            if let Some(rest) = line.strip_prefix("name = \"") {
                name = rest.trim_end_matches('"').to_string();
                break;
            }
        }
        if name.is_empty() {
            panic!("package without a name");
        }
        packages.push((name, format!("[[package]]\n{part}")));
    }
    (preamble, packages)
}

fn gateway_edge_only(before: &str, after: &str) -> bool {
    let Some(before_deps) = dep_names(before) else {
        return false;
    };
    let Some(after_deps) = dep_names(after) else {
        return false;
    };
    let mut expected = before_deps.clone();
    expected.push("serde".to_string());
    expected.push("serde_json".to_string());
    expected.sort();
    let mut after_sorted = after_deps.clone();
    after_sorted.sort();
    if expected != after_sorted {
        eprintln!("dependency edge {before_deps:?} -> {after_deps:?}");
        return false;
    }
    let added = after_deps
        .iter()
        .filter(|item| *item == "serde" || *item == "serde_json")
        .count();
    if added != 2 {
        return false;
    }
    without_deps(before) == without_deps(after)
}

fn dep_names(block: &str) -> Option<Vec<String>> {
    let start = block.find("dependencies = [")?;
    let tail = &block[start..];
    let end = tail.find("]\n")?;
    let mut deps = Vec::new();
    for line in tail[..end].lines().skip(1) {
        let trimmed = line.trim().trim_end_matches(',').trim();
        if trimmed.is_empty() {
            continue;
        }
        let inner = trimmed.trim_matches('"');
        let name = inner.split_whitespace().next().unwrap_or("");
        if name.is_empty() {
            return None;
        }
        deps.push(name.to_string());
    }
    Some(deps)
}

fn without_deps(block: &str) -> String {
    let Some(start) = block.find("dependencies = [") else {
        return block.to_string();
    };
    let tail = &block[start..];
    let Some(end) = tail.find("]\n") else {
        return block.to_string();
    };
    let mut out = String::new();
    out.push_str(&block[..start]);
    out.push_str(&tail[end + 2..]);
    out
}
