//! Fail when the tool lock has a git source or a checksum outside the
//! supplied committed locks.
//!
//! Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a301/SP04/cdc-apply-pg/linux/proof/lock_subset.rs
//! sha256 f3b92583125a52102445cc6fec45700db491b263ed88260cd3555f5ee4b187e9
//! Change: the allowed locks are every argument after the tool lock, and a
//! missing checksum is printed with its package name.
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(tool) = args.next() else {
        eprintln!("usage: lock_subset TOOL ALLOWED...");
        return ExitCode::from(2);
    };
    let allowed_paths: Vec<String> = args.collect();
    if allowed_paths.is_empty() {
        eprintln!("missing allowed locks");
        return ExitCode::from(2);
    }
    let tool_text = match fs::read_to_string(&tool) {
        Ok(text) => text,
        Err(_) => {
            eprintln!("read tool lock");
            return ExitCode::from(2);
        }
    };
    if let Some(line) = git_source(&tool_text) {
        eprintln!("tool lock has a git source at line {line}");
        return ExitCode::from(1);
    }
    let tool_packages = match packages(&tool_text) {
        Ok(packages) => packages,
        Err(err) => {
            eprintln!("tool lock: {err}");
            return ExitCode::from(1);
        }
    };
    if tool_packages.is_empty() {
        eprintln!("tool lock has no checksums");
        return ExitCode::from(1);
    }
    let mut allowed = BTreeSet::new();
    for path in &allowed_paths {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(_) => {
                eprintln!("read allowed lock");
                return ExitCode::from(2);
            }
        };
        match packages(&text) {
            Ok(found) => {
                for package in found {
                    allowed.insert(package.checksum);
                }
            }
            Err(err) => {
                eprintln!("allowed lock: {err}");
                return ExitCode::from(1);
            }
        }
    }
    let mut missing = Vec::new();
    for package in &tool_packages {
        if !allowed.contains(&package.checksum) {
            missing.push(package);
        }
    }
    if missing.is_empty() {
        println!("lock_subset_ok tool_checksums={}", tool_packages.len());
        ExitCode::SUCCESS
    } else {
        eprintln!("missing_checksums {}", missing.len());
        for package in missing {
            eprintln!("{} {} {}", package.name, package.version, package.checksum);
        }
        ExitCode::from(1)
    }
}

struct Package {
    name: String,
    version: String,
    checksum: String,
}

fn git_source(text: &str) -> Option<usize> {
    for (index, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("source = \"git+") {
            return Some(index + 1);
        }
    }
    None
}

fn packages(text: &str) -> Result<Vec<Package>, String> {
    let mut out = Vec::new();
    let mut name = String::new();
    let mut version = String::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed == "[[package]]" {
            name.clear();
            version.clear();
            continue;
        }
        if let Some(value) = quoted(trimmed, "name") {
            name = value;
            continue;
        }
        if let Some(value) = quoted(trimmed, "version") {
            version = value;
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("checksum = \"") else {
            continue;
        };
        let Some(hex) = rest.strip_suffix('"') else {
            return Err(format!("line {}: checksum is not quoted", index + 1));
        };
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
            return Err(format!("line {}: checksum is not 64 lowercase hex", index + 1));
        }
        if name.is_empty() || version.is_empty() {
            return Err(format!("line {}: checksum has no package", index + 1));
        }
        out.push(Package {
            name: name.clone(),
            version: version.clone(),
            checksum: hex.to_string(),
        });
    }
    Ok(out)
}

fn quoted(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?.strip_prefix(" = \"")?;
    rest.strip_suffix('"').map(str::to_string)
}
