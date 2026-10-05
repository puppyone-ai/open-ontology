//! Compare the fixed lock with its parent and with the fetched crate bytes.
//!
//! License, rust-version, yanked, checksum, and age come from the crate
//! Cargo.toml, the sparse index line, and the crates.io version JSON.
//! The digest is FIPS 180-4 in this file.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const SEVEN_DAYS: i64 = 7 * 86_400;

struct Pkg {
    name: String,
    version: String,
    source: String,
    checksum: String,
    deps: Vec<String>,
}

struct Row {
    name: &'static str,
    version: &'static str,
}

const NEW_ROWS: &[Row] = &[
    Row { name: "arrow-cmp", version: "60.0.0" },
    Row { name: "arrow-ipc", version: "60.0.0" },
    Row { name: "arrow-select", version: "60.0.0" },
    Row { name: "flatbuffers", version: "25.12.19" },
];

fn main() -> ExitCode {
    if let Err(err) = run() {
        eprintln!("supply_audit: {err}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn run() -> Result<(), String> {
    if hex(&sha256(b"")) != EMPTY_SHA || hex(&sha256(b"abc")) != ABC_SHA {
        return Err("sha256 self-check failed".to_owned());
    }
    if unix_seconds("1970-01-01T00:00:00Z")? != 0 || unix_seconds("1970-01-02T00:00:00Z")? != 86_400 {
        return Err("clock self-check failed".to_owned());
    }
    let mut args = env::args().skip(1);
    let new_lock = args.next().ok_or("usage: supply_audit <new-lock> <parent-lock> <cargo-home> <oracle-dir> <clock>")?;
    let parent_lock = args.next().ok_or("missing parent lock")?;
    let cargo_home = args.next().ok_or("missing cargo home")?;
    let oracle_dir = args.next().ok_or("missing oracle dir")?;
    let clock_file = args.next().ok_or("missing clock")?;
    if args.next().is_some() {
        return Err("too many arguments".to_owned());
    }
    let clock_text = fs::read_to_string(&clock_file).map_err(|err| format!("clock: {err}"))?;
    let clock = unix_seconds(clock_text.trim())?;
    println!("clock={}", clock_text.trim());
    let new_pkgs = parse_lock(&fs::read_to_string(&new_lock).map_err(|err| format!("new lock: {err}"))?)?;
    let parent_pkgs = parse_lock(&fs::read_to_string(&parent_lock).map_err(|err| format!("parent lock: {err}"))?)?;
    let mut failed = false;
    let mut additions: Vec<String> = Vec::new();
    for pkg in registry_only(&new_pkgs) {
        let key = format!("{} {}", pkg.name, pkg.version);
        let parent = find_registry(&parent_pkgs, &pkg.name, &pkg.version);
        match parent {
            Some(old) if old.checksum == pkg.checksum => {}
            Some(_) => {
                println!("FAIL checksum_move {key}");
                failed = true;
            }
            None => additions.push(key),
        }
    }
    for pkg in registry_only(&parent_pkgs) {
        if find_registry(&new_pkgs, &pkg.name, &pkg.version).is_none() {
            println!("FAIL removed {} {}", pkg.name, pkg.version);
            failed = true;
        } else if let Some(new_pkg) = find_registry(&new_pkgs, &pkg.name, &pkg.version) {
            if new_pkg.checksum != pkg.checksum {
                println!("FAIL parent_checksum {} {}", pkg.name, pkg.version);
                failed = true;
            }
        }
    }
    additions.sort();
    println!("additions={}", additions.len());
    for item in &additions {
        println!("addition {item}");
    }
    let mut expected: Vec<String> = NEW_ROWS.iter().map(|row| format!("{} {}", row.name, row.version)).collect();
    expected.sort();
    if additions != expected {
        println!("FAIL addition_set");
        failed = true;
    } else {
        println!("ok addition_set");
    }
    for row in NEW_ROWS {
        if audit_row(row, &new_pkgs, Path::new(&cargo_home), Path::new(&oracle_dir), clock).is_err() {
            failed = true;
        }
    }
    if failed {
        Err("supply audit failed".to_owned())
    } else {
        println!("supply_audit=ok");
        Ok(())
    }
}

fn audit_row(row: &Row, pkgs: &[Pkg], cargo_home: &Path, oracle_dir: &Path, clock: i64) -> Result<(), ()> {
    let mut failed = false;
    let Some(pkg) = find_registry(pkgs, row.name, row.version) else {
        println!("FAIL missing_lock {} {}", row.name, row.version);
        return Err(());
    };
    println!("package={} version={}", pkg.name, pkg.version);
    println!("lock_checksum={}", pkg.checksum);
    println!("source={}", pkg.source);
    if !pkg.source.starts_with("registry+https://github.com/rust-lang/crates.io-index") {
        println!("FAIL source {} {}", row.name, pkg.source);
        failed = true;
    }
    let deps = pkg.deps.join(",");
    println!("deps={deps}");
    let crate_name = format!("{}-{}.crate", row.name, row.version);
    match find_file(cargo_home, &crate_name) {
        Ok(path) => {
            let bytes = fs::read(&path).unwrap_or_default();
            let digest = hex(&sha256(&bytes));
            println!("crate_path={}", path.display());
            println!("crate_checksum={digest}");
            if digest != pkg.checksum {
                println!("FAIL crate_checksum {} {}", row.name, row.version);
                failed = true;
            } else {
                println!("ok crate_checksum {} {}", row.name, row.version);
            }
        }
        Err(err) => {
            println!("FAIL crate_file {err}");
            failed = true;
        }
    }
    let dir_name = format!("{}-{}", row.name, row.version);
    match find_dir(cargo_home, &dir_name) {
        Ok(dir) => {
            let toml_path = dir.join("Cargo.toml");
            match fs::read_to_string(&toml_path) {
                Ok(text) => {
                    let license = package_field(&text, "license").unwrap_or_default();
                    let rust_version = package_field(&text, "rust-version")
                        .or_else(|| package_field(&text, "rust_version"))
                        .unwrap_or_default();
                    println!("toml_license={license}");
                    println!("toml_rust_version={rust_version}");
                    if license != "Apache-2.0" {
                        println!("FAIL toml_license {} {license}", row.name);
                        failed = true;
                    }
                    match msrv_at_most(&rust_version, 1, 94, 0) {
                        Ok(true) => println!("ok msrv {} {rust_version}", row.name),
                        Ok(false) => {
                            println!("FAIL msrv {} {rust_version}", row.name);
                            failed = true;
                        }
                        Err(err) => {
                            println!("FAIL msrv_parse {} {err}", row.name);
                            failed = true;
                        }
                    }
                }
                Err(err) => {
                    println!("FAIL cargo_toml {err}");
                    failed = true;
                }
            }
        }
        Err(err) => {
            println!("FAIL crate_dir {err}");
            failed = true;
        }
    }
    let index_path = oracle_dir.join(format!("{}.index", row.name));
    let api_path = oracle_dir.join(format!("{}.api.json", row.name));
    match fs::read_to_string(&index_path) {
        Ok(text) => match index_line(&text, row.version) {
            Ok(line) => {
                match json_string(line, "cksum") {
                    Ok(cksum) => {
                        println!("index_checksum={cksum}");
                        if cksum != pkg.checksum {
                            println!("FAIL index_checksum {}", row.name);
                            failed = true;
                        }
                    }
                    Err(err) => {
                        println!("FAIL index_cksum {err}");
                        failed = true;
                    }
                }
                match json_bool(line, "yanked") {
                    Ok(false) => println!("ok index_yanked {}", row.name),
                    Ok(true) => {
                        println!("FAIL index_yanked {}", row.name);
                        failed = true;
                    }
                    Err(err) => {
                        println!("FAIL index_yanked {err}");
                        failed = true;
                    }
                }
                match json_string(line, "rust_version") {
                    Ok(value) => {
                        println!("index_rust_version={value}");
                        if msrv_at_most(&value, 1, 94, 0) != Ok(true) {
                            println!("FAIL index_msrv {}", row.name);
                            failed = true;
                        }
                    }
                    Err(err) => {
                        println!("FAIL index_rust_version {err}");
                        failed = true;
                    }
                }
                match json_string(line, "pubtime") {
                    Ok(value) => println!("index_pubtime={value}"),
                    Err(err) => {
                        println!("FAIL index_pubtime {err}");
                        failed = true;
                    }
                }
            }
            Err(err) => {
                println!("FAIL index_line {err}");
                failed = true;
            }
        },
        Err(err) => {
            println!("FAIL index_read {err}");
            failed = true;
        }
    }
    match fs::read_to_string(&api_path) {
        Ok(text) => match version_window(&text, row.version) {
            Ok(window) => {
                match json_string(window, "checksum") {
                    Ok(value) => {
                        println!("api_checksum={value}");
                        if value != pkg.checksum {
                            println!("FAIL api_checksum {}", row.name);
                            failed = true;
                        }
                    }
                    Err(err) => {
                        println!("FAIL api_checksum {err}");
                        failed = true;
                    }
                }
                match json_string(window, "license") {
                    Ok(value) => {
                        println!("api_license={value}");
                        if value != "Apache-2.0" {
                            println!("FAIL api_license {}", row.name);
                            failed = true;
                        }
                    }
                    Err(err) => {
                        println!("FAIL api_license {err}");
                        failed = true;
                    }
                }
                match json_bool(window, "yanked") {
                    Ok(false) => println!("ok api_yanked {}", row.name),
                    Ok(true) => {
                        println!("FAIL api_yanked {}", row.name);
                        failed = true;
                    }
                    Err(err) => {
                        println!("FAIL api_yanked {err}");
                        failed = true;
                    }
                }
                match json_string(window, "rust_version") {
                    Ok(value) => {
                        println!("api_rust_version={value}");
                        if msrv_at_most(&value, 1, 94, 0) != Ok(true) {
                            println!("FAIL api_msrv {}", row.name);
                            failed = true;
                        }
                    }
                    Err(err) => {
                        println!("FAIL api_rust_version {err}");
                        failed = true;
                    }
                }
                match json_string(window, "created_at") {
                    Ok(value) => {
                        println!("api_created_at={value}");
                        match unix_seconds(&value) {
                            Ok(created) => {
                                let age = clock - created;
                                println!("age_seconds={age}");
                                println!("age_days={}", age / 86_400);
                                if age < SEVEN_DAYS {
                                    println!("FAIL age {}", row.name);
                                    failed = true;
                                } else {
                                    println!("ok age {}", row.name);
                                }
                            }
                            Err(err) => {
                                println!("FAIL created_parse {err}");
                                failed = true;
                            }
                        }
                    }
                    Err(err) => {
                        println!("FAIL created_at {err}");
                        failed = true;
                    }
                }
            }
            Err(err) => {
                println!("FAIL api_window {err}");
                failed = true;
            }
        },
        Err(err) => {
            println!("FAIL api_read {err}");
            failed = true;
        }
    }
    if failed { Err(()) } else { Ok(()) }
}

fn registry_only(pkgs: &[Pkg]) -> Vec<&Pkg> {
    pkgs.iter().filter(|pkg| pkg.source.starts_with("registry+")).collect()
}

fn find_registry<'a>(pkgs: &'a [Pkg], name: &str, version: &str) -> Option<&'a Pkg> {
    pkgs.iter().find(|pkg| pkg.name == name && pkg.version == version && pkg.source.starts_with("registry+"))
}

fn parse_lock(text: &str) -> Result<Vec<Pkg>, String> {
    let mut out = Vec::new();
    let mut current: Option<Pkg> = None;
    let mut in_deps = false;
    for line in text.lines() {
        if line.starts_with("[[") {
            if let Some(pkg) = current.take() {
                if !pkg.name.is_empty() {
                    out.push(pkg);
                }
            }
            in_deps = false;
            if line.starts_with("[[package]]") {
                current = Some(Pkg {
                    name: String::new(),
                    version: String::new(),
                    source: String::new(),
                    checksum: String::new(),
                    deps: Vec::new(),
                });
            }
            continue;
        }
        let Some(pkg) = current.as_mut() else {
            continue;
        };
        let trimmed = line.trim();
        if in_deps {
            if trimmed == "]" {
                in_deps = false;
            } else if let Some(body) = trimmed.trim_end_matches(',').strip_prefix('"') {
                let name = body.split('"').next().unwrap_or("");
                let name = name.split(' ').next().unwrap_or("");
                if !name.is_empty() {
                    pkg.deps.push(name.to_owned());
                }
            }
            continue;
        }
        if trimmed.starts_with("dependencies") {
            in_deps = true;
            continue;
        }
        if let Some(value) = quoted(trimmed, "name") {
            pkg.name = value;
        } else if let Some(value) = quoted(trimmed, "version") {
            pkg.version = value;
        } else if let Some(value) = quoted(trimmed, "source") {
            pkg.source = value;
        } else if let Some(value) = quoted(trimmed, "checksum") {
            pkg.checksum = value;
        }
    }
    if let Some(pkg) = current {
        if !pkg.name.is_empty() {
            out.push(pkg);
        }
    }
    Ok(out)
}

fn quoted(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?.trim_start().strip_prefix('=')?.trim();
    let rest = rest.trim_matches('"');
    if rest.is_empty() { None } else { Some(rest.to_owned()) }
}

fn package_field(toml: &str, key: &str) -> Option<String> {
    let package = toml.split("[package]").nth(1)?;
    let body = package.split("\n[").next().unwrap_or(package);
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some(value) = quoted(trimmed, key) {
            return Some(value);
        }
    }
    None
}

fn json_string(window: &str, key: &str) -> Result<String, String> {
    let patterns = [format!("\"{key}\":\""), format!("\"{key}\": \"")];
    for pattern in &patterns {
        if let Some(start) = window.find(pattern) {
            let rest = &window[start + pattern.len()..];
            let end = rest.find('"').ok_or_else(|| format!("unterminated {key}"))?;
            return Ok(rest[..end].to_owned());
        }
    }
    Err(format!("missing {key}"))
}

fn json_bool(window: &str, key: &str) -> Result<bool, String> {
    let patterns = [format!("\"{key}\":"), format!("\"{key}\": ")];
    for pattern in &patterns {
        if let Some(start) = window.find(pattern) {
            let rest = window[start + pattern.len()..].trim_start();
            if let Some(stripped) = rest.strip_prefix("true") {
                let _ = stripped;
                return Ok(true);
            }
            if rest.starts_with("false") {
                return Ok(false);
            }
            return Err(format!("bad bool {key}"));
        }
    }
    Err(format!("missing {key}"))
}

fn index_line<'a>(body: &'a str, version: &str) -> Result<&'a str, String> {
    let needle = format!("\"vers\":\"{version}\"");
    for line in body.lines() {
        if line.contains(&needle) {
            return Ok(line);
        }
    }
    Err(format!("missing vers {version}"))
}

fn version_window<'a>(body: &'a str, version: &str) -> Result<&'a str, String> {
    let needles = [format!("\"num\":\"{version}\""), format!("\"num\": \"{version}\"")];
    for needle in &needles {
        if let Some(pos) = body.find(needle) {
            let end = (pos + 20_000).min(body.len());
            return Ok(&body[pos..end]);
        }
    }
    Err(format!("missing num {version}"))
}

fn msrv_at_most(text: &str, major: u32, minor: u32, patch: u32) -> Result<bool, String> {
    if text.is_empty() {
        return Err("empty rust-version".to_owned());
    }
    let mut parts = text.split('.');
    let parsed_major: u32 = parts.next().ok_or("major")?.parse().map_err(|_| "major".to_owned())?;
    let parsed_minor: u32 = parts.next().ok_or("minor")?.parse().map_err(|_| "minor".to_owned())?;
    let parsed_patch: u32 = match parts.next() {
        Some(value) => value.parse().map_err(|_| "patch".to_owned())?,
        None => 0,
    };
    if parts.next().is_some() {
        return Err(format!("extra rust-version {text}"));
    }
    Ok((parsed_major, parsed_minor, parsed_patch) <= (major, minor, patch))
}

fn unix_seconds(text: &str) -> Result<i64, String> {
    if text.len() < 20 {
        return Err(format!("short timestamp {text}"));
    }
    let year: i32 = text[0..4].parse().map_err(|_| "year".to_owned())?;
    let month: u32 = text[5..7].parse().map_err(|_| "month".to_owned())?;
    let day: u32 = text[8..10].parse().map_err(|_| "day".to_owned())?;
    if &text[4..5] != "-" || &text[7..8] != "-" || &text[10..11] != "T" || &text[13..14] != ":" || &text[16..17] != ":" {
        return Err(format!("timestamp shape {text}"));
    }
    let hour: u32 = text[11..13].parse().map_err(|_| "hour".to_owned())?;
    let minute: u32 = text[14..16].parse().map_err(|_| "minute".to_owned())?;
    let second: u32 = text[17..19].parse().map_err(|_| "second".to_owned())?;
    let rest = &text[19..];
    let zone = if let Some(stripped) = rest.strip_prefix('.') {
        let index = stripped.find(['Z', '+', '-']).ok_or_else(|| format!("zone {text}"))?;
        &stripped[index..]
    } else {
        rest
    };
    if zone != "Z" && zone != "+00:00" {
        return Err(format!("zone {zone}"));
    }
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return Err(format!("range {text}"));
    }
    let days = civil_days(year, month, day)?;
    Ok(days * 86_400 + i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second))
}

fn civil_days(year: i32, month: u32, day: u32) -> Result<i64, String> {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = u64::try_from(y - era * 400).map_err(|_| "yoe".to_owned())?;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * u64::from(mp) + 2) / 5 + u64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let era_days = era.checked_mul(146_097).ok_or("era")?;
    Ok(era_days + i64::try_from(doe).map_err(|_| "doe".to_owned())? - 719_468)
}

fn find_file(root: &Path, file_name: &str) -> Result<PathBuf, String> {
    let mut hits = Vec::new();
    walk(root, file_name, false, &mut hits)?;
    one_hit(file_name, hits)
}

fn find_dir(root: &Path, dir_name: &str) -> Result<PathBuf, String> {
    let mut hits = Vec::new();
    walk(root, dir_name, true, &mut hits)?;
    one_hit(dir_name, hits)
}

fn one_hit(label: &str, hits: Vec<PathBuf>) -> Result<PathBuf, String> {
    if hits.len() == 1 {
        Ok(hits.into_iter().next().expect("hit"))
    } else {
        Err(format!("{label} matches {}", hits.len()))
    }
}

fn walk(dir: &Path, name: &str, want_dir: bool, hits: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(format!("read {}: {err}", dir.display())),
    };
    for entry in entries {
        let entry = entry.map_err(|err| format!("entry: {err}"))?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|err| format!("type {}: {err}", path.display()))?;
        if kind.is_symlink() {
            return Err(format!("symlink {}", path.display()));
        }
        let file_name = entry.file_name();
        let matched = file_name == name;
        if want_dir && kind.is_dir() && matched {
            hits.push(path);
            continue;
        }
        if !want_dir && kind.is_file() && matched {
            hits.push(path);
            continue;
        }
        if kind.is_dir() {
            walk(&path, name, want_dir, hits)?;
        }
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    out
}

fn sha256(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1, 0x923f_82a4,
        0xab1c_5ed5, 0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3, 0x72be_5d74, 0x80de_b1fe,
        0x9bdc_06a7, 0xc19b_f174, 0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f,
        0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da, 0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
        0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967, 0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc,
        0x5338_0d13, 0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85, 0xa2bf_e8a1, 0xa81a_664b,
        0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070, 0x19a4_c116,
        0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
        0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208, 0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7,
        0xc671_78f2,
    ];
    let mut state = [
        0x6a09_e667_u32,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];
    let bit_len = u64::try_from(input.len()).expect("len").checked_mul(8).expect("bits");
    let mut msg = input.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut word = [0u32; 64];
        for (index, slot) in word.iter_mut().enumerate().take(16) {
            let start = index * 4;
            *slot = u32::from_be_bytes(chunk[start..start + 4].try_into().expect("word"));
        }
        for index in 16..64 {
            let earlier = word[index - 15];
            let later = word[index - 2];
            let s0 = earlier.rotate_right(7) ^ earlier.rotate_right(18) ^ (earlier >> 3);
            let s1 = later.rotate_right(17) ^ later.rotate_right(19) ^ (later >> 10);
            word[index] = word[index - 16]
                .wrapping_add(s0)
                .wrapping_add(word[index - 7])
                .wrapping_add(s1);
        }
        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(choose)
                .wrapping_add(K[index])
                .wrapping_add(word[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }
    let mut out = [0u8; 32];
    for (index, value) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    out
}
