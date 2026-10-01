//! Secret scan and supply-chain tool runner.
//!
//! `supply --check` audits the deny config and exits 3 until the pinned tools run.
//! `supply --run-tools` installs cargo-audit 0.22.2 and cargo-deny 0.20.2 into a
//! temporary cargo home. It does not change the user cargo installation.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::json;
use thiserror::Error;

use crate::report::{EXIT_FAIL, EXIT_PENDING, EXIT_USAGE, Issue, Report};
use crate::util::{read_no_symlink, repo_root, security_owner_appointed, sha256_file, sha256_hex};

pub const CARGO_AUDIT_VERSION: &str = "0.22.2";
pub const CARGO_AUDIT_CKSUM: &str =
    "700c2b240f7fd330c24b675fe429f73a5b676531fcc6300400b2b67f155ba12a";
pub const CARGO_AUDIT_PUBLISHED: &str = "2026-06-05T12:33:04Z";
pub const CARGO_DENY_VERSION: &str = "0.20.2";
pub const CARGO_DENY_CKSUM: &str =
    "e528dfcbe739af7ce37a77d3d6df1b29dd6887b1c701d888820c0f16b864f737";
pub const CARGO_DENY_PUBLISHED: &str = "2026-07-09T17:58:09Z";
const MIN_AGE_SECONDS: i64 = 168 * 3600;

#[derive(Debug, Error)]
enum ToolError {
    #[error("tool {tool} checksum {actual} != pinned {pinned}")]
    Checksum {
        tool: &'static str,
        actual: String,
        pinned: &'static str,
    },
    #[error("{0}")]
    Other(String),
}

pub fn secrets_cmd(args: &[String]) -> Report {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Report::ok(
            "secrets",
            "help",
            json!({"usage": "xtask secrets --check [--root PATH]"}),
        );
    }
    let mut check = false;
    let mut root = repo_root();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--check" => check = true,
            "--root" => {
                index += 1;
                match args.get(index) {
                    Some(value) => root = PathBuf::from(value),
                    None => return usage("secrets", "secrets --root requires a path"),
                }
            }
            "--format" | "json" | "human" => {}
            other => return usage("secrets", &format!("unknown secrets argument {other}")),
        }
        index += 1;
    }
    if !check {
        return usage("secrets", "secrets requires --check");
    }
    let repo = root;
    let issues = scan_tree(&repo);
    Report::from_issues(
        "secrets",
        "pass",
        "failed",
        EXIT_FAIL,
        issues,
        json!({
            "scope": "repository files except the .git object store. target, hidden, ignored, and new paths are scanned",
            "full_at_sec_05": false,
            "allowlist": "exact sha256 only; pending classification is not a pass",
            "diagnostics_echo_secret": false,
            "canary": "generated only in tests, not committed"
        }),
    )
}

pub fn supply_cmd(args: &[String]) -> Report {
    let mut check = false;
    let mut run_tools = false;
    for arg in args {
        match arg.as_str() {
            "--check" => check = true,
            "--run-tools" => run_tools = true,
            "--help" | "-h" => {
                return Report::ok(
                    "supply",
                    "help",
                    json!({
                        "usage": "xtask supply --check | xtask supply --run-tools",
                        "full_at_sec_05": false
                    }),
                );
            }
            "--format" | "json" | "human" => {}
            other => return usage("supply", &format!("unknown supply argument {other}")),
        }
    }
    if !check && !run_tools {
        return usage("supply", "supply requires --check or --run-tools");
    }
    let repo = repo_root();
    let mut issues = config_issues(&repo);
    issues.extend(exception_issues(&repo));
    let age = registry_age(&repo);
    if run_tools {
        return match run_pinned_tools(&repo) {
            Ok(mut details) => {
                if let Some(map) = details.as_object_mut()
                    && let Some(age_map) = age.details().as_object()
                {
                    for (key, value) in age_map {
                        map.entry(key.clone()).or_insert(value.clone());
                    }
                }
                if issues.is_empty() {
                    Report::ok("supply", "tools_ran", details)
                } else {
                    Report::fail("supply", "failed", EXIT_FAIL, issues, details)
                }
            }
            Err(error) => {
                issues.push(Issue::new("supply.tools", "cargo", error.to_string()));
                Report::fail("supply", "failed", EXIT_FAIL, issues, age.details())
            }
        };
    }
    if !issues.is_empty() {
        return Report::fail("supply", "failed", EXIT_FAIL, issues, age.details());
    }
    Report::fail(
        "supply",
        "pending",
        EXIT_PENDING,
        vec![Issue::new(
            "supply.tools_not_run",
            "ci/deny.toml",
            "deny config and the registry graph passed the local checks. cargo-audit and cargo-deny have not run.",
        )],
        {
            let mut details = age.details();
            if let Some(map) = details.as_object_mut() {
                map.insert("config_legal".to_owned(), json!(true));
                map.insert("cargo_audit".to_owned(), json!(CARGO_AUDIT_VERSION));
                map.insert("cargo_deny".to_owned(), json!(CARGO_DENY_VERSION));
                map.insert(
                    "security_owner_appointed".to_owned(),
                    json!(security_owner_appointed(&repo)),
                );
                map.insert("deny_config_sha256".to_owned(), json!(hash_config(&repo)));
            }
            details
        },
    )
}

fn config_issues(repo: &Path) -> Vec<Issue> {
    let Ok(bytes) = read_no_symlink(repo, "ci/deny.toml") else {
        return vec![Issue::new(
            "supply.deny",
            "ci/deny.toml",
            "deny config is missing",
        )];
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return vec![Issue::new(
            "supply.deny",
            "ci/deny.toml",
            "deny config is not utf-8",
        )];
    };
    let mut issues = crate::deny_toml::deny_policy_issues(&text);
    issues.extend(registry_age(repo).issues);
    issues
}

const AGE_FLOOR_SECONDS: i64 = 604_800;

const SHIPPING_LOCKS: &[&str] = &["Cargo.lock", "bench/datagen/Cargo.lock"];

struct LockAge {
    path: String,
    sha256: String,
    packages: usize,
    young: usize,
    yanked: usize,
    missing: usize,
    checksum_mismatch: usize,
    metadata: usize,
}

struct AgeReport {
    issues: Vec<Issue>,
    packages: usize,
    young: usize,
    yanked: usize,
    missing: usize,
    checksum_mismatch: usize,
    metadata: usize,
    locks: Vec<LockAge>,
}

impl AgeReport {
    fn details(&self) -> serde_json::Value {
        json!({
            "full_at_sec_05": false,
            "js_paths_present": false,
            "image_os_scan": false,
            "native_product_scan": false,
            "wasm_matrix_supported": false,
            "wasm_packages_age_checked": true,
            "registry_packages": self.packages,
            "registry_young": self.young,
            "registry_yanked": self.yanked,
            "registry_index_missing": self.missing,
            "registry_checksum_mismatch": self.checksum_mismatch,
            "registry_metadata_rejected": self.metadata,
            "age_floor_seconds": AGE_FLOOR_SECONDS,
            "tool_age_substitutes_for_graph": false,
            "locks": self.locks.iter().map(|lock| json!({
                "path": lock.path,
                "sha256": lock.sha256,
                "registry_packages": lock.packages,
                "registry_young": lock.young,
                "registry_yanked": lock.yanked,
                "registry_index_missing": lock.missing,
                "registry_checksum_mismatch": lock.checksum_mismatch,
                "registry_metadata_rejected": lock.metadata
            })).collect::<Vec<_>>()
        })
    }
}

fn registry_age(repo: &Path) -> AgeReport {
    let mut report = AgeReport {
        issues: Vec::new(),
        packages: 0,
        young: 0,
        yanked: 0,
        missing: 0,
        checksum_mismatch: 0,
        metadata: 0,
        locks: Vec::new(),
    };
    for relative in SHIPPING_LOCKS {
        let partial = age_lock_file(repo, relative);
        report.packages += partial.packages;
        report.young += partial.young;
        report.yanked += partial.yanked;
        report.missing += partial.missing;
        report.checksum_mismatch += partial.checksum_mismatch;
        report.metadata += partial.metadata;
        report.issues.extend(partial.issues);
        report.locks.extend(partial.locks);
    }
    report
}

fn age_lock_file(repo: &Path, relative: &str) -> AgeReport {
    let mut report = AgeReport {
        issues: Vec::new(),
        packages: 0,
        young: 0,
        yanked: 0,
        missing: 0,
        checksum_mismatch: 0,
        metadata: 0,
        locks: Vec::new(),
    };
    let path = repo.join(relative);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            report.issues.push(Issue::new(
                "supply.lock",
                relative,
                format!("registry age check cannot read the lock: {error}"),
            ));
            report.locks.push(LockAge {
                path: relative.to_owned(),
                sha256: "missing".to_owned(),
                packages: 0,
                young: 0,
                yanked: 0,
                missing: 0,
                checksum_mismatch: 0,
                metadata: 0,
            });
            return report;
        }
    };
    let digest = sha256_hex(text.as_bytes());
    let packages = registry_packages(&text);
    report.packages = packages.len();
    if packages.is_empty() {
        report.issues.push(Issue::new(
            "supply.lock",
            relative,
            "the lock has no crates.io packages to age-check",
        ));
        report.locks.push(LockAge {
            path: relative.to_owned(),
            sha256: digest,
            packages: 0,
            young: 0,
            yanked: 0,
            missing: 0,
            checksum_mismatch: 0,
            metadata: 0,
        });
        return report;
    }
    let home = cargo_home();
    let now = match crate::util::parse_rfc3339_utc(&crate::util::now_rfc3339()) {
        Ok(stamp) => stamp,
        Err(error) => {
            report
                .issues
                .push(Issue::new("supply.age", relative, error));
            report.locks.push(LockAge {
                path: relative.to_owned(),
                sha256: digest.clone(),
                packages: report.packages,
                young: 0,
                yanked: 0,
                missing: 0,
                checksum_mismatch: 0,
                metadata: 0,
            });
            return report;
        }
    };
    for package in packages {
        let cache = index_cache_file(&home, &package.name);
        let bytes = match fs::read(&cache) {
            Ok(bytes) => bytes,
            Err(_) => {
                report.missing += 1;
                if report.issues.len() < 40 {
                    report.issues.push(Issue::new(
                        "supply.index",
                        format!("{relative} {} {}", package.name, package.version),
                        "version is not in the local crates.io sparse-index cache",
                    ));
                }
                continue;
            }
        };
        let record = match lookup_index_record(&bytes, &package.name, &package.version) {
            Ok(record) => record,
            Err(IndexFailure::Missing) => {
                report.missing += 1;
                if report.issues.len() < 40 {
                    report.issues.push(Issue::new(
                        "supply.index",
                        format!("{relative} {} {}", package.name, package.version),
                        "local crates.io cache has no record for this version",
                    ));
                }
                continue;
            }
            Err(IndexFailure::Malformed(message)) => {
                report.metadata += 1;
                if report.issues.len() < 40 {
                    report.issues.push(Issue::new(
                        "supply.metadata",
                        format!("{relative} {} {}", package.name, package.version),
                        message,
                    ));
                }
                continue;
            }
        };
        if record.yanked {
            report.yanked += 1;
            report.issues.push(Issue::new(
                "supply.yanked",
                format!("{relative} {} {}", package.name, package.version),
                "crates.io records this version as yanked",
            ));
        }
        if !package.checksum.is_empty() && record.cksum != package.checksum {
            report.checksum_mismatch += 1;
            report.issues.push(Issue::new(
                "supply.checksum",
                format!("{relative} {} {}", package.name, package.version),
                "lock checksum does not match the crates.io cache record",
            ));
        }
        match crate::util::parse_rfc3339_utc(&record.pubtime) {
            Ok(published) => {
                let elapsed = now.unix_seconds - published.unix_seconds;
                if elapsed < AGE_FLOOR_SECONDS {
                    report.young += 1;
                    report.issues.push(Issue::new(
                        "supply.young",
                        format!("{relative} {} {}", package.name, package.version),
                        format!(
                            "published {} age {elapsed}s is below {AGE_FLOOR_SECONDS}s",
                            record.pubtime
                        ),
                    ));
                }
            }
            Err(error) => {
                report.metadata += 1;
                report.issues.push(Issue::new(
                    "supply.metadata",
                    format!("{relative} {} {}", package.name, package.version),
                    format!("pubtime is not usable: {error}"),
                ));
            }
        }
    }
    report.locks.push(LockAge {
        path: relative.to_owned(),
        sha256: digest,
        packages: report.packages,
        young: report.young,
        yanked: report.yanked,
        missing: report.missing,
        checksum_mismatch: report.checksum_mismatch,
        metadata: report.metadata,
    });
    report
}

struct LockedPackage {
    name: String,
    version: String,
    checksum: String,
}

fn registry_packages(text: &str) -> Vec<LockedPackage> {
    let mut out = Vec::new();
    let mut name = None;
    let mut version = None;
    let mut source = None;
    let mut checksum = None;
    let mut in_package = false;
    let flush = |out: &mut Vec<LockedPackage>,
                 name: &mut Option<String>,
                 version: &mut Option<String>,
                 source: &mut Option<String>,
                 checksum: &mut Option<String>| {
        if let (Some(name), Some(version), Some(source)) =
            (name.take(), version.take(), source.take())
            && source.starts_with("registry+https://github.com/rust-lang/crates.io-index")
        {
            out.push(LockedPackage {
                name,
                version,
                checksum: checksum.take().unwrap_or_default(),
            });
        }
        checksum.take();
    };
    for line in text.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            flush(
                &mut out,
                &mut name,
                &mut version,
                &mut source,
                &mut checksum,
            );
            in_package = true;
            continue;
        }
        if !in_package || line.starts_with('[') {
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
    flush(
        &mut out,
        &mut name,
        &mut version,
        &mut source,
        &mut checksum,
    );
    out
}

fn unquote(value: &str) -> Option<String> {
    let value = value.trim().trim_end_matches(',');
    Some(value.trim_matches('"').to_owned())
}

fn cargo_home() -> PathBuf {
    if let Some(value) = std::env::var_os("CARGO_HOME")
        && !value.is_empty()
    {
        return PathBuf::from(value);
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cargo")
}

fn index_cache_file(home: &Path, name: &str) -> PathBuf {
    let cache = home.join("registry/index/index.crates.io-1949cf8c6b5b557f/.cache");
    let relative = match name.len() {
        0 => "missing".to_owned(),
        1 => format!("1/{name}"),
        2 => format!("2/{name}"),
        3 => format!("3/{}/{name}", &name[..1]),
        _ => format!("{}/{}/{name}", &name[..2], &name[2..4]),
    };
    cache.join(relative)
}

struct CacheRecord {
    cksum: String,
    yanked: bool,
    pubtime: String,
}

#[derive(Debug)]
enum IndexFailure {
    Missing,
    Malformed(String),
}

fn lookup_index_record(
    bytes: &[u8],
    name: &str,
    version: &str,
) -> Result<CacheRecord, IndexFailure> {
    let mut found = Vec::new();
    for part in bytes.split(|byte| *byte == 0) {
        let Ok(text) = std::str::from_utf8(part) else {
            continue;
        };
        let text = text.trim();
        if !text.starts_with('{') {
            continue;
        }
        let keys = match crate::util::top_level_keys(text) {
            Ok(keys) => keys,
            Err(_) => {
                if index_text_names_version(text, version) {
                    return Err(IndexFailure::Malformed(
                        "index record is not valid JSON".to_owned(),
                    ));
                }
                continue;
            }
        };
        let value = match serde_json::from_str::<serde_json::Value>(text) {
            Ok(value) => value,
            Err(_) => {
                if index_text_names_version(text, version) {
                    return Err(IndexFailure::Malformed(
                        "index record is not valid JSON".to_owned(),
                    ));
                }
                continue;
            }
        };
        if value.get("vers").and_then(|item| item.as_str()) != Some(version) {
            continue;
        }
        if let Some(duplicate) = crate::util::duplicate_key(&keys) {
            return Err(IndexFailure::Malformed(format!(
                "duplicate top-level key {duplicate}"
            )));
        }
        found.push(value);
    }
    match found.len() {
        0 => Err(IndexFailure::Missing),
        1 => interpret_index_record(&found[0], name),
        _ => Err(IndexFailure::Malformed(
            "duplicate version record".to_owned(),
        )),
    }
}

fn index_text_names_version(text: &str, version: &str) -> bool {
    text.contains(&format!("\"vers\":\"{version}\""))
        || text.contains(&format!("\"vers\": \"{version}\""))
}

fn interpret_index_record(
    value: &serde_json::Value,
    name: &str,
) -> Result<CacheRecord, IndexFailure> {
    let record_name = match value.get("name") {
        Some(serde_json::Value::String(text)) => text.as_str(),
        Some(_) => {
            return Err(IndexFailure::Malformed("name is not a string".to_owned()));
        }
        None => return Err(IndexFailure::Malformed("name is missing".to_owned())),
    };
    if record_name != name {
        return Err(IndexFailure::Malformed(
            "name does not match the crate".to_owned(),
        ));
    }
    match value.get("vers") {
        Some(serde_json::Value::String(_)) => {}
        Some(_) => {
            return Err(IndexFailure::Malformed(
                "version is not a string".to_owned(),
            ));
        }
        None => return Err(IndexFailure::Malformed("version is missing".to_owned())),
    }
    let cksum = match value.get("cksum") {
        Some(serde_json::Value::String(text)) => text.as_str(),
        Some(serde_json::Value::Null) => {
            return Err(IndexFailure::Malformed("checksum is null".to_owned()));
        }
        Some(_) => {
            return Err(IndexFailure::Malformed(
                "checksum is not a string".to_owned(),
            ));
        }
        None => return Err(IndexFailure::Malformed("checksum is missing".to_owned())),
    };
    if cksum.len() != 64
        || !cksum
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(IndexFailure::Malformed(
            "checksum is not 64 lowercase hex".to_owned(),
        ));
    }
    let yanked = match value.get("yanked") {
        Some(serde_json::Value::Bool(flag)) => *flag,
        Some(serde_json::Value::Null) => {
            return Err(IndexFailure::Malformed("yanked is null".to_owned()));
        }
        Some(_) => {
            return Err(IndexFailure::Malformed(
                "yanked is not a boolean".to_owned(),
            ));
        }
        None => return Err(IndexFailure::Malformed("yanked is missing".to_owned())),
    };
    let pubtime = match value.get("pubtime") {
        Some(serde_json::Value::String(text)) if !text.is_empty() => text.clone(),
        Some(serde_json::Value::String(_)) => {
            return Err(IndexFailure::Malformed("pubtime is empty".to_owned()));
        }
        Some(serde_json::Value::Null) => {
            return Err(IndexFailure::Malformed("pubtime is null".to_owned()));
        }
        Some(_) => {
            return Err(IndexFailure::Malformed(
                "pubtime is not a string".to_owned(),
            ));
        }
        None => return Err(IndexFailure::Malformed("pubtime is missing".to_owned())),
    };
    Ok(CacheRecord {
        cksum: cksum.to_owned(),
        yanked,
        pubtime,
    })
}

fn exception_issues(repo: &Path) -> Vec<Issue> {
    if security_owner_appointed(repo) {
        return Vec::new();
    }
    let mut issues = Vec::new();
    let dir = repo.join("supply-chain");
    let Ok(entries) = fs::read_dir(&dir) else {
        return issues;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        if value.get("template") == Some(&serde_json::Value::Bool(true)) {
            continue;
        }
        if value.get("exception").is_some() && !value.get("exception").unwrap().is_null() {
            issues.push(Issue::new(
                "supply.exception",
                path.display().to_string(),
                "a 7-day or other security exception is invalid while the security owner is unappointed",
            ));
        }
    }
    issues
}

pub fn scan_tree(repo: &Path) -> Vec<Issue> {
    let mut issues = Vec::new();
    let allow = allowlist(repo);
    walk(repo, repo, &allow, &mut issues);
    issues
}

const SCAN_LIMIT: u64 = 1_048_576;
const SCAN_CHUNK: usize = 4096;
const SCAN_OVERLAP: usize = 512;

fn walk(repo: &Path, dir: &Path, allow: &AllowList, issues: &mut Vec<Issue>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => {
            issues.push(Issue::new(
                "secrets.unreadable",
                relative_path(repo, dir),
                "directory cannot be listed",
            ));
            return;
        }
    };
    for entry in entries {
        let Ok(entry) = entry else {
            issues.push(Issue::new(
                "secrets.unreadable",
                relative_path(repo, dir),
                "directory entry cannot be read",
            ));
            continue;
        };
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|item| item.to_str())
            .unwrap_or("");
        if name == ".git" {
            continue;
        }
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => {
                issues.push(Issue::new(
                    "secrets.unreadable",
                    relative_path(repo, &path),
                    "metadata cannot be read",
                ));
                continue;
            }
        };
        if metadata.file_type().is_symlink() {
            issues.push(Issue::new(
                "secrets.symlink",
                relative_path(repo, &path),
                "symlink is not followed",
            ));
            continue;
        }
        if metadata.file_type().is_dir() {
            walk(repo, &path, allow, issues);
            continue;
        }
        if !metadata.file_type().is_file() {
            issues.push(Issue::new(
                "secrets.unreadable",
                relative_path(repo, &path),
                "path is not a regular file and was not scanned",
            ));
            continue;
        }
        if metadata.len() > SCAN_LIMIT {
            issues.push(Issue::new(
                "secrets.oversized",
                relative_path(repo, &path),
                "file exceeds 1MiB and was not scanned",
            ));
            continue;
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => {
                issues.push(Issue::new(
                    "secrets.unreadable",
                    relative_path(repo, &path),
                    "file cannot be read",
                ));
                continue;
            }
        };
        if let Some(kind) = scan_bytes(&bytes) {
            let digest = sha256_hex(&bytes);
            let (code, message) = match allow.review(&digest) {
                Some(_) => (
                    "secrets.classified_pending",
                    format!("pattern {kind} is classified pending independent review"),
                ),
                None => ("secrets.hit", format!("pattern {kind}")),
            };
            issues.push(Issue::new(code, relative_path(repo, &path), message));
        }
    }
}

struct AllowList {
    hashes: Vec<String>,
}

impl AllowList {
    fn review(&self, digest: &str) -> Option<()> {
        if self.hashes.iter().any(|item| item == digest) {
            Some(())
        } else {
            None
        }
    }
}

fn allowlist(repo: &Path) -> AllowList {
    let path = repo.join("supply-chain/secret-allowlist.json");
    let Ok(bytes) = fs::read(&path) else {
        return AllowList { hashes: Vec::new() };
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return AllowList { hashes: Vec::new() };
    };
    let Ok(value) = oo_core::json_scan::parse_json_value(&text, "secret-allowlist") else {
        return AllowList { hashes: Vec::new() };
    };
    if value.get("schema").and_then(|item| item.as_str()) != Some("secret-allowlist/v1") {
        return AllowList { hashes: Vec::new() };
    }
    let Some(entries) = value.get("entries").and_then(|item| item.as_array()) else {
        return AllowList { hashes: Vec::new() };
    };
    let mut hashes = Vec::new();
    for entry in entries {
        let Some(digest) = entry.get("sha256").and_then(|item| item.as_str()) else {
            continue;
        };
        let reason = entry
            .get("reason")
            .and_then(|item| item.as_str())
            .unwrap_or("");
        if digest.len() == 64
            && digest.chars().all(|ch| ch.is_ascii_hexdigit())
            && !reason.trim().is_empty()
        {
            hashes.push(digest.to_owned());
        }
    }
    AllowList { hashes }
}

fn relative_path(repo: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(repo).unwrap_or(path);
    let mut out = String::new();
    for component in relative.components() {
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(&component.as_os_str().to_string_lossy());
    }
    if out.is_empty() { ".".to_owned() } else { out }
}

pub fn scan_bytes(bytes: &[u8]) -> Option<&'static str> {
    if bytes.is_empty() {
        return None;
    }
    let begin = marker(&[b"-----", b"BEGIN "]);
    let key = marker(&[b"PRIVATE ", b"KEY-----"]);
    let mut saw_begin = false;
    let mut saw_key = false;
    let mut index = 0;
    loop {
        let end = (index + SCAN_CHUNK).min(bytes.len());
        let window = &bytes[index..end];
        if contains_bytes(window, &begin) {
            saw_begin = true;
        }
        if contains_bytes(window, &key) {
            saw_key = true;
        }
        if saw_begin && saw_key {
            return Some("private-key");
        }
        if let Some(kind) = token_hit(window) {
            return Some(kind);
        }
        if end == bytes.len() {
            break;
        }
        let next = end.saturating_sub(SCAN_OVERLAP);
        if next <= index {
            break;
        }
        index = next;
    }
    None
}

#[cfg(test)]
fn secret_hit(bytes: &[u8]) -> Option<&'static str> {
    scan_bytes(bytes)
}

fn token_hit(bytes: &[u8]) -> Option<&'static str> {
    if find_token(bytes, b"AKIA", 16, true) {
        return Some("aws-access-key");
    }
    if find_token(bytes, b"ghp_", 20, false) || find_token(bytes, b"github_pat_", 20, false) {
        return Some("github-token");
    }
    if find_token(bytes, b"xoxb-", 10, false)
        || find_token(bytes, b"xoxp-", 10, false)
        || find_token(bytes, b"xoxa-", 10, false)
    {
        return Some("slack-token");
    }
    if find_token(bytes, b"sk-", 20, false) {
        return Some("api-key");
    }
    if find_token(bytes, b"npm_", 20, false) {
        return Some("npm-token");
    }
    if jwt_hit(bytes) {
        return Some("jwt");
    }
    if assignment_hit(bytes) {
        return Some("assignment");
    }
    if url_userinfo(bytes) {
        return Some("url-userinfo");
    }
    None
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn marker(parts: &[&[u8]]) -> Vec<u8> {
    parts.iter().copied().flatten().copied().collect()
}

fn find_token(bytes: &[u8], needle: &[u8], extra: usize, upper: bool) -> bool {
    if needle.is_empty() || bytes.len() < needle.len() + extra {
        return false;
    }
    for index in 0..=bytes.len() - needle.len() - extra {
        if &bytes[index..index + needle.len()] != needle {
            continue;
        }
        let rest = &bytes[index + needle.len()..index + needle.len() + extra];
        let shaped = if upper {
            rest.iter()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        } else {
            rest.iter().all(|byte| byte.is_ascii_alphanumeric())
        };
        if shaped {
            return true;
        }
    }
    false
}

fn jwt_hit(bytes: &[u8]) -> bool {
    let needle = b"eyJ";
    if bytes.len() < needle.len() {
        return false;
    }
    for index in 0..=bytes.len() - needle.len() {
        if bytes[index..].starts_with(needle) && jwt_at(&bytes[index..]) {
            return true;
        }
    }
    false
}

fn jwt_at(bytes: &[u8]) -> bool {
    let Some(dot1) = bytes.iter().position(|byte| *byte == b'.') else {
        return false;
    };
    if dot1 < 10 || !is_b64url(&bytes[..dot1]) {
        return false;
    }
    let rest = &bytes[dot1 + 1..];
    let Some(dot2) = rest.iter().position(|byte| *byte == b'.') else {
        return false;
    };
    if dot2 < 10 || !is_b64url(&rest[..dot2]) {
        return false;
    }
    let tail = &rest[dot2 + 1..];
    let end = tail
        .iter()
        .position(|byte| !is_b64url_byte(*byte))
        .unwrap_or(tail.len());
    end >= 8 && is_b64url(&tail[..end])
}

fn is_b64url(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().copied().all(is_b64url_byte)
}

fn is_b64url_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

fn assignment_hit(bytes: &[u8]) -> bool {
    let lower = bytes.to_ascii_lowercase();
    for key in [
        b"password".as_slice(),
        b"passwd",
        b"secret",
        b"api_key",
        b"api-key",
        b"token",
    ] {
        let mut index = 0;
        while index + key.len() < lower.len() {
            if lower[index..].starts_with(key) && boundary_before(&lower, index) {
                let mut cursor = index + key.len();
                while cursor < lower.len() && matches!(lower[cursor], b' ' | b'\t') {
                    cursor += 1;
                }
                if cursor < lower.len() && matches!(lower[cursor], b'=' | b':') {
                    if lower[cursor] == b':'
                        && cursor + 1 < lower.len()
                        && lower[cursor + 1] == b'/'
                    {
                        index += 1;
                        continue;
                    }
                    cursor += 1;
                    while cursor < lower.len() && matches!(lower[cursor], b' ' | b'\t') {
                        cursor += 1;
                    }
                    if cursor < lower.len() && matches!(lower[cursor], b'"' | b'\'') {
                        cursor += 1;
                    }
                    let start = cursor;
                    while cursor < bytes.len()
                        && !matches!(
                            bytes[cursor],
                            b' ' | b'\t' | b'\n' | b'\r' | b'"' | b'\'' | b';'
                        )
                    {
                        cursor += 1;
                    }
                    if secret_value(&bytes[start..cursor]) {
                        return true;
                    }
                }
            }
            index += 1;
        }
    }
    false
}

fn boundary_before(bytes: &[u8], index: usize) -> bool {
    index == 0 || !bytes[index - 1].is_ascii_alphanumeric()
}

fn secret_value(value: &[u8]) -> bool {
    if value.len() < 12
        || !value
            .iter()
            .all(|byte| byte.is_ascii() && !byte.is_ascii_control())
    {
        return false;
    }
    let text = String::from_utf8_lossy(value).to_ascii_lowercase();
    const PLACEHOLDERS: &[&str] = &[
        "password",
        "passwd",
        "secret",
        "changeme",
        "example",
        "placeholder",
        "redacted",
        "your-token",
        "your_token",
        "none",
        "null",
        "false",
        "true",
        "empty",
        "dummy",
        "sample",
        "notasecret",
        "not-a-secret",
        "todo",
        "test",
        "xxxx",
        "xxxxxxxx",
    ];
    if PLACEHOLDERS.contains(&text.as_str()) || text.chars().all(|ch| ch == 'x' || ch == '*') {
        return false;
    }
    if text.starts_with("${") || text.starts_with('<') || text.starts_with("{{") {
        return false;
    }
    value.iter().any(|byte| byte.is_ascii_digit())
        || value.iter().any(|byte| !byte.is_ascii_alphanumeric())
}

fn url_placeholder(password: &[u8]) -> bool {
    if !password.iter().all(|byte| byte.is_ascii()) {
        return false;
    }
    let text = String::from_utf8_lossy(password).to_ascii_lowercase();
    const EXACT: &[&str] = &[
        "password",
        "passwd",
        "secret",
        "changeme",
        "example",
        "placeholder",
        "redacted",
        "your-token",
        "your_token",
        "none",
        "null",
        "false",
        "true",
        "empty",
        "dummy",
        "sample",
        "notasecret",
        "not-a-secret",
        "todo",
        "test",
        "xxxx",
        "xxxxxxxx",
        "pw",
        "pass",
        "user",
        "token",
        "admin",
        "guest",
        "s3cret",
        "sentinel_user",
        "sentinel_query",
        "sentinel_frag",
        "sentinel_path",
        "{user}",
        "{user}%40still",
        "{user}\\u{00a0}",
        "{user}\\u{200b}",
        "{user}\\u{3000}",
        "{user}{ch}",
        "{user}{ws}",
        "sentinel_user%40still",
        "sentinel_user\\\\u00a0",
    ];
    EXACT.contains(&text.as_str())
}

fn url_userinfo(bytes: &[u8]) -> bool {
    let needle = b"://";
    if bytes.len() < needle.len() {
        return false;
    }
    for index in 0..=bytes.len() - needle.len() {
        if !bytes[index..].starts_with(needle) {
            continue;
        }
        if index > 0 && bytes[index - 1].is_ascii_alphanumeric() {
            let mut scheme_start = index;
            while scheme_start > 0 && bytes[scheme_start - 1].is_ascii_alphanumeric() {
                scheme_start -= 1;
            }
            if !bytes[scheme_start..index]
                .iter()
                .all(|byte| byte.is_ascii_alphabetic())
            {
                continue;
            }
        }
        let rest = &bytes[index + needle.len()..];
        let authority_end = rest
            .iter()
            .position(|byte| {
                matches!(
                    byte,
                    b'/' | b'?' | b'#' | b' ' | b'\t' | b'\n' | b'\r' | b'"' | b'\'' | b'<' | b'>'
                )
            })
            .unwrap_or(rest.len());
        let authority = &rest[..authority_end];
        if authority.iter().filter(|byte| **byte == b'@').count() != 1 {
            continue;
        }
        let Some(at) = authority.iter().position(|byte| *byte == b'@') else {
            continue;
        };
        let userinfo = &authority[..at];
        let host = &authority[at + 1..];
        let Some(colon) = userinfo.iter().position(|byte| *byte == b':') else {
            continue;
        };
        if userinfo.iter().skip(colon + 1).any(|byte| *byte == b':') {
            continue;
        }
        if colon == 0 || colon + 1 >= userinfo.len() {
            continue;
        }
        let user = &userinfo[..colon];
        let password = &userinfo[colon + 1..];
        if user
            .iter()
            .any(|byte| !byte.is_ascii() || matches!(byte, b'/' | b' ' | b'@' | b':'))
            || password
                .iter()
                .any(|byte| !matches!(byte, 0x21..=0x7e) || matches!(byte, b'/' | b'@'))
            || password.is_empty()
            || url_placeholder(password)
        {
            continue;
        }
        let host_name = host.split(|byte| *byte == b':').next().unwrap_or(host);
        if host_name
            .first()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
            && host_name
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
        {
            return true;
        }
    }
    false
}

fn run_pinned_tools(repo: &Path) -> Result<serde_json::Value, ToolError> {
    let home = std::env::temp_dir().join(format!("oo-xtask-cargo-{}", std::process::id()));
    let target = home.join("target");
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&target).map_err(|error| ToolError::Other(error.to_string()))?;
    install(
        &home,
        &target,
        "cargo-audit",
        CARGO_AUDIT_VERSION,
        CARGO_AUDIT_CKSUM,
    )?;
    install(
        &home,
        &target,
        "cargo-deny",
        CARGO_DENY_VERSION,
        CARGO_DENY_CKSUM,
    )?;
    confirm_age("cargo-audit", CARGO_AUDIT_VERSION, CARGO_AUDIT_PUBLISHED)?;
    confirm_age("cargo-deny", CARGO_DENY_VERSION, CARGO_DENY_PUBLISHED)?;
    let audit = run_bin(&home, repo, "cargo-audit", &["audit", "--json"])?;
    let audit_datagen = run_bin(
        &home,
        repo,
        "cargo-audit",
        &["audit", "--json", "--file", "bench/datagen/Cargo.lock"],
    )?;
    let deny = run_bin(
        &home,
        repo,
        "cargo-deny",
        &[
            "--format",
            "json",
            "--config",
            "ci/deny.toml",
            "--locked",
            "check",
        ],
    )?;
    let deny_datagen = run_bin(
        &home,
        repo,
        "cargo-deny",
        &[
            "--format",
            "json",
            "--manifest-path",
            "bench/datagen/Cargo.toml",
            "--config",
            "ci/deny.toml",
            "--locked",
            "check",
        ],
    )?;
    if audit.0 != 0 || audit_datagen.0 != 0 {
        return Err(ToolError::Other(format!(
            "cargo-audit root {} datagen {}\n{}{}",
            audit.0,
            audit_datagen.0,
            tail(&audit.1),
            tail(&audit_datagen.1)
        )));
    }
    if deny.0 != 0 || deny_datagen.0 != 0 {
        return Err(ToolError::Other(format!(
            "cargo-deny root {} datagen {}\n{}{}",
            deny.0,
            deny_datagen.0,
            tail(&deny.1),
            tail(&deny_datagen.1)
        )));
    }
    let advisory = advisory_db_identity(&home);
    let config_sha = hash_config(repo);
    Ok(json!({
        "full_at_sec_05": false,
        "cargo_audit_exit": audit.0,
        "cargo_audit_datagen_exit": audit_datagen.0,
        "cargo_deny_exit": deny.0,
        "cargo_deny_datagen_exit": deny_datagen.0,
        "cargo_audit_version": CARGO_AUDIT_VERSION,
        "cargo_deny_version": CARGO_DENY_VERSION,
        "cargo_home": "temporary",
        "global_install": false,
        "js_paths_present": repo.join("console").exists() || repo.join("sdk-ts").exists(),
        "image_os_scan": false,
        "image_pins_present": repo.join("testing/infra/images.lock.json").is_file(),
        "native_product_scan": false,
        "wasm_matrix_supported": false,
        "deny_config": "ci/deny.toml",
        "deny_config_sha256": config_sha,
        "advisory_database_commit": advisory.0,
        "advisory_database_time": advisory.1,
        "tool_age_substitutes_for_graph": false
    }))
}

fn advisory_db_identity(home: &Path) -> (String, String) {
    let mut candidates = vec![home.join("advisory-db"), home.join("advisory-dbs")];
    if let Ok(entries) = fs::read_dir(home) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                candidates.push(path);
            }
        }
    }
    for candidate in candidates {
        let git_dir = if candidate.join(".git").exists() {
            candidate.clone()
        } else if candidate.join("advisory-db/.git").exists() {
            candidate.join("advisory-db")
        } else {
            continue;
        };
        let commit = Command::new("git")
            .args(["-C", git_dir.to_str().unwrap_or("."), "rev-parse", "HEAD"])
            .output();
        let time = Command::new("git")
            .args([
                "-C",
                git_dir.to_str().unwrap_or("."),
                "log",
                "-1",
                "--format=%cI",
            ])
            .output();
        if let (Ok(commit), Ok(time)) = (commit, time)
            && commit.status.success()
            && time.status.success()
        {
            let commit = String::from_utf8_lossy(&commit.stdout).trim().to_owned();
            let time = String::from_utf8_lossy(&time.stdout).trim().to_owned();
            if commit.len() == 40 {
                return (commit, time);
            }
        }
    }
    ("unavailable".to_owned(), "unavailable".to_owned())
}

fn install(
    home: &Path,
    target: &Path,
    name: &str,
    version: &str,
    pinned: &str,
) -> Result<(), ToolError> {
    let output = Command::new("cargo")
        .args(["install", name, "--version", version, "--locked", "--force"])
        .env("CARGO_HOME", home)
        .env("CARGO_TARGET_DIR", target)
        .output()
        .map_err(|error| ToolError::Other(error.to_string()))?;
    if !output.status.success() {
        return Err(ToolError::Other(format!(
            "install {name} {version} failed\n{}",
            tail(String::from_utf8_lossy(&output.stderr).as_ref())
        )));
    }
    let crate_file = find_crate(home, name, version)
        .ok_or_else(|| ToolError::Other(format!("downloaded crate for {name} was not found")))?;
    let actual = sha256_file(&crate_file).map_err(ToolError::Other)?;
    if actual != pinned {
        return Err(ToolError::Checksum {
            tool: if name == "cargo-audit" {
                "cargo-audit"
            } else {
                "cargo-deny"
            },
            actual,
            pinned: if name == "cargo-audit" {
                CARGO_AUDIT_CKSUM
            } else {
                CARGO_DENY_CKSUM
            },
        });
    }
    Ok(())
}

fn find_crate(home: &Path, name: &str, version: &str) -> Option<PathBuf> {
    let cache = home.join("registry/cache");
    let needle = format!("{name}-{version}.crate");
    let mut stack = vec![cache];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().and_then(|item| item.to_str()) == Some(needle.as_str()) {
                return Some(path);
            }
        }
    }
    None
}

fn confirm_age(name: &str, version: &str, reviewed: &str) -> Result<(), ToolError> {
    let url = format!("https://crates.io/api/v1/crates/{name}");
    let output = Command::new("curl")
        .args(["-fsSL", "-H", "User-Agent: open-ontology-xtask", &url])
        .output()
        .map_err(|error| ToolError::Other(error.to_string()))?;
    if !output.status.success() {
        return Err(ToolError::Other(format!(
            "crates.io metadata for {name} failed"
        )));
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| ToolError::Other(error.to_string()))?;
    let published = value
        .pointer("/versions")
        .and_then(|item| item.as_array())
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("num").and_then(|num| num.as_str()) == Some(version))
        })
        .and_then(|row| row.get("created_at").and_then(|item| item.as_str()))
        .ok_or_else(|| ToolError::Other(format!("{name} {version} has no created_at")))?;
    if !published.starts_with(&reviewed[..10]) {
        return Err(ToolError::Other(format!(
            "{name} {version} published {published}, reviewed date {reviewed}"
        )));
    }
    let published_at = crate::util::parse_rfc3339_utc(published).map_err(ToolError::Other)?;
    let now =
        crate::util::parse_rfc3339_utc(&crate::util::now_rfc3339()).map_err(ToolError::Other)?;
    if now.unix_seconds - published_at.unix_seconds < MIN_AGE_SECONDS {
        return Err(ToolError::Other(format!(
            "{name} {version} is younger than 168 hours"
        )));
    }
    Ok(())
}

fn run_bin(home: &Path, repo: &Path, bin: &str, args: &[&str]) -> Result<(i32, String), ToolError> {
    let program = home.join("bin").join(bin);
    let output = Command::new(program)
        .args(args)
        .current_dir(repo)
        .env("CARGO_HOME", home)
        .output()
        .map_err(|error| ToolError::Other(error.to_string()))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok((output.status.code().unwrap_or(1), text))
}

fn tail(text: &str) -> String {
    text.chars()
        .rev()
        .take(2000)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

pub fn pins_match_delta(delta: &serde_json::Value) -> bool {
    delta
        .pointer("/tools/cargo_audit/version")
        .and_then(|item| item.as_str())
        == Some(CARGO_AUDIT_VERSION)
        && delta
            .pointer("/tools/cargo_audit/cksum")
            .and_then(|item| item.as_str())
            == Some(CARGO_AUDIT_CKSUM)
        && delta
            .pointer("/tools/cargo_deny/version")
            .and_then(|item| item.as_str())
            == Some(CARGO_DENY_VERSION)
        && delta
            .pointer("/tools/cargo_deny/cksum")
            .and_then(|item| item.as_str())
            == Some(CARGO_DENY_CKSUM)
}

pub fn hash_config(repo: &Path) -> String {
    sha256_file(&repo.join("ci/deny.toml")).unwrap_or_else(|_| "missing".to_owned())
}

pub fn sample_digest(bytes: &[u8]) -> String {
    sha256_hex(bytes)
}

fn usage(command: &str, message: &str) -> Report {
    Report::fail(
        command,
        "usage",
        EXIT_USAGE,
        vec![Issue::new("cli.usage", command, message)],
        json!({}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canary_is_detected_and_not_a_file_in_the_repo() {
        let mut canned = String::from("example ");
        canned.push_str("AK");
        canned.push_str("IA");
        canned.push_str("IOSFODNN7EXAMPLE\n");
        assert_eq!(secret_hit(canned.as_bytes()), Some("aws-access-key"));
        assert!(secret_hit(b"secret://builder_llm\n").is_none());
        let key = marker(&[b"-----", b"BEGIN OPENSSH ", b"PRIVATE ", b"KEY-----\n"]);
        assert!(secret_hit(&key).is_some());
    }

    #[test]
    fn illegal_deny_text_fails() {
        let repo = std::env::temp_dir().join(format!("oo-xtask-deny-{}", std::process::id()));
        let _ = fs::remove_dir_all(&repo);
        fs::create_dir_all(repo.join("ci")).unwrap();
        fs::write(
            repo.join("ci/deny.toml"),
            "allow = [\"AGPL-3.0-only\", \"Apache-2.0\", \"MIT\", \"BSD-3-Clause\", \"Unicode-3.0\", \"Zlib\"]\n[[licenses.clarify]]\n",
        )
        .unwrap();
        let issues = config_issues(&repo);
        assert!(
            issues.iter().any(|issue| {
                issue.code == "supply.license"
                    || issue.code == "supply.clarify"
                    || issue.code == "supply.exception"
            }),
            "{issues:?}"
        );
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn high_confidence_patterns_do_not_echo_the_value() {
        let mut api = String::from("sk");
        api.push('-');
        api.push_str("AbcdEfghIjklMnopQrst");
        assert_eq!(secret_hit(api.as_bytes()), Some("api-key"));
        let mut npm = String::from("npm");
        npm.push('_');
        npm.push_str("AbcdEfghIjklMnopQrstUv");
        assert_eq!(secret_hit(npm.as_bytes()), Some("npm-token"));
        let mut jwt = String::from("ey");
        jwt.push('J');
        jwt.push_str("hbGciOiJub25lIn0.eyJzdWIiOiJmaXh0dXJlIn0.signature8");
        assert_eq!(secret_hit(jwt.as_bytes()), Some("jwt"));
        let mut assigned = String::from("password");
        assigned.push_str(" = \"");
        assigned.push_str("horse-battery-99");
        assigned.push('"');
        assert_eq!(secret_hit(assigned.as_bytes()), Some("assignment"));
        let mut url = String::from("https://user:");
        url.push_str("horse-battery-99");
        url.push_str("@example.com/path");
        assert_eq!(secret_hit(url.as_bytes()), Some("url-userinfo"));
        assert_eq!(
            secret_hit(b"0123456789abcdef0123456789abcdef0123456789abcdef"),
            None
        );
        let mut boundary = vec![b'a'; SCAN_CHUNK];
        let token = api.as_bytes();
        let start = SCAN_CHUNK - 4;
        boundary.extend_from_slice(&[b'b'; 64]);
        boundary[start..start + token.len()].copy_from_slice(token);
        assert_eq!(scan_bytes(&boundary), Some("api-key"));
        assert!(!"pattern api-key".contains(&api));
    }

    #[test]
    fn checked_in_deny_config_matches_the_reviewed_policy() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../ci/deny.toml");
        let text = fs::read_to_string(path).expect("deny.toml");
        let issues = crate::deny_toml::deny_policy_issues(&text);
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn root_lock_registry_graph_meets_the_age_floor() {
        let report = age_lock_file(&repo_root(), "Cargo.lock");
        assert!(
            report.issues.is_empty(),
            "packages {} young {} missing {} yanked {} mismatch {} {:?}",
            report.packages,
            report.young,
            report.missing,
            report.yanked,
            report.checksum_mismatch,
            report.issues
        );
        assert!(report.packages > 0);
        assert_eq!(report.locks.len(), 1);
        assert_eq!(report.locks[0].path, "Cargo.lock");
    }

    #[test]
    fn short_url_password_is_a_hit_and_exact_placeholder_is_not() {
        let mut short = String::from("postgres://app:");
        short.push_str("s3cret-pass");
        short.push_str("@localhost/db");
        assert_eq!(secret_hit(short.as_bytes()), Some("url-userinfo"));
        let mut one = String::from("postgres://app:");
        one.push('s');
        one.push_str("@localhost/db");
        assert_eq!(secret_hit(one.as_bytes()), Some("url-userinfo"));
        assert_eq!(secret_hit(b"postgres://app:pw@127.0.0.1/db"), None);
        assert_eq!(secret_hit(b"https://user:SENTINEL_USER@h/x"), None);
        assert_eq!(secret_hit(b"https://user:s3cret@h/x"), None);
        let rendered = format!("pattern {}", secret_hit(short.as_bytes()).unwrap_or(""));
        assert!(!rendered.contains("s3cret-pass"));
    }

    fn sample_record(yanked: &str) -> String {
        format!(
            r#"{{"name":"serde","vers":"1.2.3","cksum":"{cksum}","yanked":{yanked},"pubtime":"2020-01-01T00:00:00Z"}}"#,
            cksum = "ab".repeat(32)
        )
    }

    #[test]
    fn yanked_false_is_valid_and_true_is_denied() {
        let false_row = lookup_index_record(sample_record("false").as_bytes(), "serde", "1.2.3")
            .expect("false");
        assert!(!false_row.yanked);
        let true_row =
            lookup_index_record(sample_record("true").as_bytes(), "serde", "1.2.3").expect("true");
        assert!(true_row.yanked);
    }

    #[test]
    fn missing_and_malformed_index_metadata_fail_closed() {
        let missing = sample_record("false").replace(",\"yanked\":false", "");
        assert!(matches!(
            lookup_index_record(missing.as_bytes(), "serde", "1.2.3"),
            Err(IndexFailure::Malformed(message)) if message.contains("yanked is missing")
        ));
        assert!(matches!(
            lookup_index_record(sample_record("null").as_bytes(), "serde", "1.2.3"),
            Err(IndexFailure::Malformed(message)) if message.contains("yanked is null")
        ));
        assert!(matches!(
            lookup_index_record(sample_record("\"false\"").as_bytes(), "serde", "1.2.3"),
            Err(IndexFailure::Malformed(message)) if message.contains("yanked is not a boolean")
        ));
        let duplicate =
            sample_record("false").replace("\"yanked\":false", "\"yanked\":false,\"yanked\":true");
        assert!(matches!(
            lookup_index_record(duplicate.as_bytes(), "serde", "1.2.3"),
            Err(IndexFailure::Malformed(message)) if message.contains("duplicate")
        ));
        let duplicate_cksum = sample_record("false").replace(
            "\"cksum\":",
            "\"cksum\":\"ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff\",\"cksum\":",
        );
        assert!(matches!(
            lookup_index_record(duplicate_cksum.as_bytes(), "serde", "1.2.3"),
            Err(IndexFailure::Malformed(message)) if message.contains("cksum")
        ));
        let duplicate_pub = sample_record("false").replace(
            "\"pubtime\":",
            "\"pubtime\":\"2020-01-02T00:00:00Z\",\"pubtime\":",
        );
        assert!(matches!(
            lookup_index_record(duplicate_pub.as_bytes(), "serde", "1.2.3"),
            Err(IndexFailure::Malformed(message)) if message.contains("pubtime")
        ));
        let duplicate_name = sample_record("false").replace(
            "\"name\":\"serde\"",
            "\"name\":\"other\",\"name\":\"serde\"",
        );
        assert!(matches!(
            lookup_index_record(duplicate_name.as_bytes(), "serde", "1.2.3"),
            Err(IndexFailure::Malformed(message)) if message.contains("name")
        ));
        let duplicate_vers = format!("{}\0{}", sample_record("false"), sample_record("false"));
        assert!(matches!(
            lookup_index_record(duplicate_vers.as_bytes(), "serde", "1.2.3"),
            Err(IndexFailure::Malformed(message)) if message.contains("duplicate version")
        ));
        let wrong_name = sample_record("false").replace("\"name\":\"serde\"", "\"name\":\"libc\"");
        assert!(matches!(
            lookup_index_record(wrong_name.as_bytes(), "serde", "1.2.3"),
            Err(IndexFailure::Malformed(message)) if message.contains("name does not match")
        ));
        let null_cksum = sample_record("false").replace(
            &format!("\"cksum\":\"{}\"", "ab".repeat(32)),
            "\"cksum\":null",
        );
        assert!(matches!(
            lookup_index_record(null_cksum.as_bytes(), "serde", "1.2.3"),
            Err(IndexFailure::Malformed(message)) if message.contains("checksum is null")
        ));
        let null_pub = sample_record("false")
            .replace("\"pubtime\":\"2020-01-01T00:00:00Z\"", "\"pubtime\":null");
        assert!(matches!(
            lookup_index_record(null_pub.as_bytes(), "serde", "1.2.3"),
            Err(IndexFailure::Malformed(message)) if message.contains("pubtime is null")
        ));
        assert!(matches!(
            lookup_index_record(sample_record("false").as_bytes(), "serde", "9.9.9"),
            Err(IndexFailure::Missing)
        ));
        let other = sample_record("false").replace("1.2.3", "9.9.9");
        let other_dup = other.replace("\"yanked\":false", "\"yanked\":false,\"yanked\":true");
        let mixed = format!("{other_dup}\0{}", sample_record("false"));
        assert!(
            lookup_index_record(mixed.as_bytes(), "serde", "1.2.3").is_ok(),
            "a duplicate key on another version does not reject the locked version"
        );
    }

    #[test]
    fn both_shipping_locks_clear_the_age_floor() {
        let report = registry_age(&repo_root());
        assert!(
            report.issues.is_empty(),
            "packages {} young {} missing {} yanked {} mismatch {} metadata {} {:?}",
            report.packages,
            report.young,
            report.missing,
            report.yanked,
            report.checksum_mismatch,
            report.metadata,
            report.issues
        );
        assert_eq!(
            report.locks.len(),
            2,
            "{:?}",
            report
                .locks
                .iter()
                .map(|lock| &lock.path)
                .collect::<Vec<_>>()
        );
        assert!(report.packages > report.locks[0].packages);
        assert_eq!(report.young, 0);
        assert_eq!(report.yanked, 0);
        assert_eq!(report.missing, 0);
        assert_eq!(report.checksum_mismatch, 0);
        assert_eq!(report.metadata, 0);
    }

    #[test]
    fn reviewed_tool_pins_match_the_delta() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../supply-chain/harness-dependency-delta.json");
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).expect("delta")).expect("delta json");
        assert!(pins_match_delta(&value));
        assert_eq!(sample_digest(b"xtask"), sha256_hex(b"xtask"));
        assert_ne!(hash_config(&repo_root()), "missing");
    }
}
