//! Compare manifest lines. Digests come from sha256_manifest. This file does not hash.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("manifest_eq: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let expected_path = args.next().ok_or("usage: manifest_eq <expected> <fresh>")?;
    let fresh_path = args.next().ok_or("usage: manifest_eq <expected> <fresh>")?;
    if args.next().is_some() {
        return Err("usage: manifest_eq <expected> <fresh>".to_owned());
    }
    let expected = load(&expected_path)?;
    let fresh = load(&fresh_path)?;
    let mut bad = 0usize;
    for (path, digest) in &expected {
        match fresh.get(path) {
            Some(other) if other == digest => {}
            Some(other) => {
                eprintln!("mismatch {path} expected {digest} fresh {other}");
                bad += 1;
            }
            None => {
                eprintln!("missing {path}");
                bad += 1;
            }
        }
    }
    println!("expected={}", expected.len());
    println!("fresh={}", fresh.len());
    println!("bad={bad}");
    if bad != 0 {
        return Err(format!("{bad} entries differ"));
    }
    Ok(())
}

fn load(path: &str) -> Result<BTreeMap<String, String>, String> {
    let text = fs::read_to_string(path).map_err(|err| format!("read {path}: {err}"))?;
    let mut rows = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let (digest, name) = line
            .split_once("  ")
            .ok_or_else(|| format!("{path}:{} missing separator", index + 1))?;
        if digest.len() != 64 || !digest.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')) {
            return Err(format!("{path}:{} bad digest", index + 1));
        }
        if rows.insert(name.to_owned(), digest.to_owned()).is_some() {
            return Err(format!("duplicate {name}"));
        }
    }
    Ok(rows)
}
