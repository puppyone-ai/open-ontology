//! Review oracle for the frozen Axum/Tower baseline.
//! Std-only. Hash bytes stay in sha256_manifest.rs. This file checks metadata,
//! license markers, lock triples, and the httpdate feature edge.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

const AS_OF_ADOPTION: &str = "2026-10-06T12:47:28Z";
const SEVEN_DAYS: i64 = 7 * 86_400;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("crate_verify: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let mode = args.next().ok_or("missing mode")?;
    match mode.as_str() {
        "selfcheck" => {
            no_more(&mut args)?;
            selfcheck()?;
            print!("selfcheck=pass\n");
        }
        "meta" => {
            let json = read_arg(&mut args, "json")?;
            let name = args.next().ok_or("name")?;
            let version = args.next().ok_or("version")?;
            let as_of = args.next().ok_or("as_of")?;
            let official = read_arg(&mut args, "official sha")?;
            let cached = read_arg(&mut args, "cached sha")?;
            no_more(&mut args)?;
            let report = meta_report(&json, &name, &version, &as_of, &official, &cached)?;
            print!("{report}");
        }
        "licenses" => {
            let dir = args.next().ok_or("extract dir")?;
            let meta = read_arg(&mut args, "meta")?;
            no_more(&mut args)?;
            print!("{}", license_report(&dir, &meta)?);
        }
        "manifest" => {
            let toml = read_arg(&mut args, "toml")?;
            let meta = read_arg(&mut args, "meta")?;
            no_more(&mut args)?;
            print!("{}", manifest_report(&toml, &meta)?);
        }
        "axum-edges" => {
            let toml = read_arg(&mut args, "axum toml")?;
            no_more(&mut args)?;
            print!("{}", axum_edges(&toml)?);
        }
        "hyper-httpdate" => {
            let toml = read_arg(&mut args, "hyper toml")?;
            no_more(&mut args)?;
            print!("{}", hyper_httpdate(&toml)?);
        }
        "hyper-util-server" => {
            let toml = read_arg(&mut args, "hyper-util toml")?;
            no_more(&mut args)?;
            print!("{}", hyper_util_server(&toml)?);
        }
        "probe-manifest" => {
            let toml = read_arg(&mut args, "probe toml")?;
            no_more(&mut args)?;
            print!("{}", probe_manifest(&toml)?);
        }
        "root-pins" => {
            let toml = read_arg(&mut args, "root toml")?;
            no_more(&mut args)?;
            print!("{}", root_pins(&toml)?);
        }
        "members" => {
            let dir = args.next().ok_or("crates dir")?;
            no_more(&mut args)?;
            print!("{}", members(Path::new(&dir))?);
        }
        "collect-pins" => {
            let mut texts = Vec::new();
            for path in args {
                texts.push(fs::read_to_string(&path).map_err(|err| format!("read {path}: {err}"))?);
            }
            if texts.is_empty() {
                return Err("collect-pins needs meta files".to_owned());
            }
            print!("{}", collect_pins(&texts)?);
        }
        "subset-report" => {
            let stdout = read_arg(&mut args, "lockdiff stdout")?;
            let pins = read_arg(&mut args, "pins")?;
            no_more(&mut args)?;
            print!("{}", subset_report(&stdout, &pins)?);
        }
        "edges" => {
            let root = read_arg(&mut args, "root lock")?;
            let probe = read_arg(&mut args, "probe lock")?;
            let pins = read_arg(&mut args, "pins")?;
            no_more(&mut args)?;
            print!("{}", edges(&root, &probe, &pins)?);
        }
        "index-newest" => {
            let json = read_arg(&mut args, "crate json")?;
            let name = args.next().ok_or("name")?;
            no_more(&mut args)?;
            print!("{}", index_newest(&json, &name)?);
        }
        "test-report" => {
            let stdout = read_arg(&mut args, "test stdout")?;
            no_more(&mut args)?;
            print!("{}", test_report(&stdout)?);
        }
        "lock-sha" => {
            let lock = read_arg(&mut args, "lock")?;
            let name = args.next().ok_or("name")?;
            let version = args.next().ok_or("version")?;
            let sha_file = read_arg(&mut args, "sha")?;
            no_more(&mut args)?;
            print!("{}", lock_sha(&lock, &name, &version, &sha_file)?);
        }
        other => return Err(format!("unknown mode {other}")),
    }
    Ok(())
}

fn no_more(args: &mut impl Iterator<Item = String>) -> Result<(), String> {
    if args.next().is_some() {
        return Err("too many arguments".to_owned());
    }
    Ok(())
}

fn read_arg(args: &mut impl Iterator<Item = String>, label: &str) -> Result<String, String> {
    let path = args.next().ok_or_else(|| format!("missing {label}"))?;
    fs::read_to_string(&path).map_err(|err| format!("read {path}: {err}"))
}

struct Meta {
    name: String,
    version: String,
    yanked: bool,
    license: String,
    rust_version: Option<String>,
    checksum: String,
    created_at: String,
}

fn meta_report(
    json: &str,
    name: &str,
    version: &str,
    as_of: &str,
    official_sha: &str,
    cached_sha: &str,
) -> Result<String, String> {
    let meta = parse_version_json(json)?;
    evaluate_meta(&meta, name, version, as_of, official_sha, cached_sha)
}

fn evaluate_meta(
    meta: &Meta,
    name: &str,
    version: &str,
    as_of: &str,
    official_sha: &str,
    cached_sha: &str,
) -> Result<String, String> {
    if meta.name != name || meta.version != version {
        return Err(format!(
            "metadata {} {} != expected {name} {version}",
            meta.name, meta.version
        ));
    }
    if meta.yanked {
        return Err(format!("{name} {version} is yanked"));
    }
    if !allowed_license(&meta.license) {
        return Err(format!("{name} license {} is not an admitted expression", meta.license));
    }
    let checksum = meta.checksum.to_ascii_lowercase();
    if checksum.len() != 64 || !checksum.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(format!("{name} checksum is not 64 hex digits"));
    }
    let official = first_sha(official_sha)?;
    let cached = first_sha(cached_sha)?;
    if official != checksum || cached != checksum {
        return Err(format!(
            "{name} checksum json {checksum} official {official} cached {cached}"
        ));
    }
    let published = parse_rfc3339(&meta.created_at)?;
    let adopted = parse_rfc3339(as_of)?;
    if adopted < published {
        return Err(format!("{name} adopted before publication"));
    }
    let age = adopted - published;
    if age < SEVEN_DAYS {
        return Err(format!("{name} age {age}s is under 7 days"));
    }
    let msrv = match &meta.rust_version {
        None => "undeclared",
        Some(value) => value.as_str(),
    };
    if !msrv_at_most_194(meta.rust_version.as_deref())? {
        return Err(format!("{name} rust-version {msrv} is above 1.94.0"));
    }
    let pin_license = &meta.license;
    let created_at = &meta.created_at;
    Ok(format!(
        "name={name}\nversion={version}\nyanked=false\nlicense={pin_license}\nrust_version={msrv}\nchecksum={checksum}\ncreated_at={created_at}\nage_seconds={age}\nage_ok=pass\nmsrv_ok=pass\nsha_ok=pass\npin={name} {version} {created_at} {checksum} {pin_license}\n"
    ))
}

fn allowed_license(license: &str) -> bool {
    matches!(
        license,
        "MIT" | "MIT OR Apache-2.0" | "MIT AND BSD-3-Clause"
    )
}

fn parse_version_json(text: &str) -> Result<Meta, String> {
    let object = object_after_key(text, "version")?;
    let fields = fields_depth1(object)?;
    let name = field_string(&fields, "crate")?;
    let version = field_string(&fields, "num")?;
    let yanked = field_bool(&fields, "yanked")?;
    let license = field_string(&fields, "license")?;
    let checksum = field_string(&fields, "checksum")?;
    let created_at = field_string(&fields, "created_at")?;
    let rust_version = match fields.get("rust_version") {
        None => None,
        Some(JsonValue::Null) => None,
        Some(JsonValue::String(value)) => Some(value.clone()),
        Some(other) => return Err(format!("rust_version has unexpected shape {other:?}")),
    };
    Ok(Meta {
        name,
        version,
        yanked,
        license,
        rust_version,
        checksum,
        created_at,
    })
}

fn license_report(dir: &str, meta_text: &str) -> Result<String, String> {
    let license = meta_value(meta_text, "license")?;
    let mut files = Vec::new();
    walk_licenses(Path::new(dir), &mut files)?;
    files.sort();
    if files.is_empty() {
        return Err(format!("no license files under {dir}"));
    }
    let mut corpus = String::new();
    let mut listed = String::new();
    for path in &files {
        let bytes = fs::read(path).map_err(|err| format!("read {}: {err}", path.display()))?;
        let text = String::from_utf8_lossy(&bytes);
        corpus.push_str(&text);
        corpus.push('\n');
        listed.push_str(&path.display().to_string());
        listed.push('\n');
    }
    let mit = corpus.contains("Permission is hereby granted");
    let apache = corpus.contains("Apache License") && corpus.contains("Version 2.0");
    let bsd = corpus.contains("BSD 3-Clause License")
        && corpus.contains("Redistribution and use in source and binary forms");
    match license.as_str() {
        "MIT" => {
            if !mit {
                return Err("MIT expression has no MIT license text".to_owned());
            }
        }
        "MIT OR Apache-2.0" => {
            if !mit || !apache {
                return Err(format!("dual license files incomplete mit={mit} apache={apache}"));
            }
        }
        "MIT AND BSD-3-Clause" => {
            if !mit || !bsd {
                return Err(format!("matchit license files incomplete mit={mit} bsd={bsd}"));
            }
        }
        other => return Err(format!("license {other} is not checked")),
    }
    let choice = match license.as_str() {
        "MIT OR Apache-2.0" => "Apache-2.0",
        "MIT AND BSD-3-Clause" => "MIT AND BSD-3-Clause",
        "MIT" => "MIT",
        _ => "unset",
    };
    Ok(format!(
        "license_files={}\nlicense_expression={license}\nproject_relies_on={choice}\nlicenses=pass\n{listed}",
        files.len()
    ))
}

fn walk_licenses(dir: &Path, files: &mut Vec<std::path::PathBuf>) -> Result<(), String> {
    let mut entries = fs::read_dir(dir)
        .map_err(|err| format!("read {}: {err}", dir.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| format!("read {}: {err}", dir.display()))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|err| format!("type {}: {err}", path.display()))?;
        if kind.is_symlink() {
            return Err(format!("symlink refused: {}", path.display()));
        }
        if kind.is_dir() {
            walk_licenses(&path, files)?;
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let upper = name.to_ascii_uppercase();
        if upper.starts_with("LICENSE") || upper.starts_with("LICENCE") || upper.starts_with("COPYING") || upper.starts_with("NOTICE")
        {
            files.push(path);
        }
    }
    Ok(())
}

fn manifest_report(toml: &str, meta_text: &str) -> Result<String, String> {
    let package = package_table(toml)?;
    let name = package.get("name").ok_or("package name missing")?.clone();
    let version = package.get("version").ok_or("package version missing")?.clone();
    let license = package.get("license").ok_or("package license missing")?.clone();
    let rust_version = package.get("rust-version").cloned();
    let expect_name = meta_value(meta_text, "name")?;
    let expect_version = meta_value(meta_text, "version")?;
    let expect_license = meta_value(meta_text, "license")?;
    let expect_rust = meta_value(meta_text, "rust_version")?;
    if name != expect_name || version != expect_version || license != expect_license {
        return Err(format!(
            "manifest {name} {version} {license} != meta {expect_name} {expect_version} {expect_license}"
        ));
    }
    let actual = rust_version.unwrap_or_else(|| "undeclared".to_owned());
    if actual != expect_rust {
        return Err(format!("manifest rust-version {actual} != meta {expect_rust}"));
    }
    Ok(format!("manifest=pass\nname={name}\nversion={version}\nlicense={license}\nrust_version={actual}\n"))
}

fn package_table(text: &str) -> Result<BTreeMap<String, String>, String> {
    let mut in_package = false;
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let trim = line.trim();
        if trim.starts_with('[') {
            if in_package {
                break;
            }
            in_package = trim == "[package]";
            continue;
        }
        if !in_package || trim.is_empty() || trim.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trim.split_once('=') else {
            continue;
        };
        map.insert(key.trim().to_owned(), unquote_toml(value.trim()));
    }
    if map.is_empty() {
        return Err("package table missing".to_owned());
    }
    Ok(map)
}

fn unquote_toml(value: &str) -> String {
    let value = value.trim().trim_end_matches(',');
    if let Some(inner) = value.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')) {
        return inner.to_owned();
    }
    value.to_owned()
}

fn axum_edges(toml: &str) -> Result<String, String> {
    require_version(toml, "[dependencies.matchit]", "version = \"=0.8.4\"")?;
    require_version(toml, "[dependencies.mime]", "version = \"0.3.16\"")?;
    require_version(toml, "[dependencies.tower]", "version = \"0.5.2\"")?;
    let tower_http = table_body(toml, "[dependencies.tower-http]")?;
    if !tower_http.contains("version = \"0.6.8\"") || !tower_http.contains("optional = true") {
        return Err(format!("axum tower-http requirement is not optional 0.6.8: {tower_http}"));
    }
    let http1 = feature_array(toml, "http1")?;
    if !http1.contains("hyper?/http1") {
        return Err(format!("axum http1 feature: {http1}"));
    }
    if http1.contains("httpdate") {
        return Err("axum http1 feature names httpdate".to_owned());
    }
    let hyper_util = table_body(toml, "[dependencies.hyper-util]")?;
    if !hyper_util.contains("\"server\"") {
        return Err("axum hyper-util dependency does not enable server".to_owned());
    }
    Ok("axum_edges=pass\nhttpdate_edge=hyper-util-server-enables-hyper-server\n".to_owned())
}

fn hyper_httpdate(toml: &str) -> Result<String, String> {
    let http1 = feature_array(toml, "http1")?;
    if http1.contains("httpdate") {
        return Err(format!("hyper http1 enables httpdate: {http1}"));
    }
    let server = feature_array(toml, "server")?;
    if !server.contains("dep:httpdate") {
        return Err(format!("hyper server does not enable httpdate: {server}"));
    }
    let dep = table_body(toml, "[dependencies.httpdate]")?;
    if !dep.contains("version = \"1.0\"") || !dep.contains("optional = true") {
        return Err(format!("hyper httpdate dependency: {dep}"));
    }
    Ok("hyper_httpdate=server\n".to_owned())
}

fn hyper_util_server(toml: &str) -> Result<String, String> {
    let server = feature_array(toml, "server")?;
    if !server.contains("hyper/server") {
        return Err(format!("hyper-util server feature: {server}"));
    }
    let http1 = feature_array(toml, "http1")?;
    if !http1.contains("hyper/http1") {
        return Err(format!("hyper-util http1 feature: {http1}"));
    }
    Ok("hyper_util_server=hyper/server\n".to_owned())
}

fn require_version(toml: &str, header: &str, version_line: &str) -> Result<(), String> {
    let body = table_body(toml, header)?;
    if !body.contains(version_line) {
        return Err(format!("{header} missing {version_line}: {body}"));
    }
    Ok(())
}

fn table_body(text: &str, header: &str) -> Result<String, String> {
    let mut found = false;
    let mut body = String::new();
    for line in text.lines() {
        if !found {
            if line.trim() == header {
                found = true;
            }
            continue;
        }
        if line.trim_start().starts_with('[') {
            break;
        }
        body.push_str(line);
        body.push('\n');
    }
    if !found {
        return Err(format!("missing {header}"));
    }
    Ok(body)
}

fn feature_array(text: &str, name: &str) -> Result<String, String> {
    let mut in_features = false;
    let lines: Vec<&str> = text.lines().collect();
    let header = format!("{name} = [");
    for (index, line) in lines.iter().enumerate() {
        let trim = line.trim();
        if trim.starts_with('[') {
            in_features = trim == "[features]";
            continue;
        }
        if !in_features || !trim.starts_with(&header) {
            continue;
        }
        let rest = trim[header.len()..].trim();
        if rest.is_empty() {
            let mut body = String::new();
            for next in &lines[index + 1..] {
                if next.trim() == "]" {
                    break;
                }
                body.push_str(next);
                body.push('\n');
            }
            return Ok(body);
        }
        let inner = rest
            .strip_suffix(']')
            .ok_or_else(|| format!("feature {name} is not closed"))?;
        return Ok(inner.to_owned());
    }
    Err(format!("feature {name} missing"))
}

fn probe_manifest(toml: &str) -> Result<String, String> {
    const REQUIRED: [&str; 9] = [
        "axum = { version = \"=0.8.9\", default-features = false, features = [\"http1\", \"tokio\"] }",
        "tower = { version = \"=0.5.3\", default-features = false, features = [\"limit\", \"make\", \"timeout\", \"util\"] }",
        "tower-http = { version = \"=0.6.11\", default-features = false, features = [\"limit\", \"timeout\"] }",
        "axum-core = { version = \"=0.5.6\", default-features = false }",
        "httpdate = { version = \"=1.0.3\", default-features = false }",
        "matchit = { version = \"=0.8.4\", default-features = false }",
        "mime = { version = \"=0.3.17\", default-features = false }",
        "hyper = { version = \"=1.11.1\", default-features = false }",
        "hyper-util = { version = \"=0.1.20\", default-features = false }",
    ];
    for line in REQUIRED {
        if !toml.contains(line) {
            return Err(format!("probe manifest missing {line}"));
        }
    }
    let lower = toml.to_ascii_lowercase();
    for token in ["jwt", "oauth", "oidc", "paseto", "jsonwebtoken", "jose", "\"auth\""] {
        if lower.contains(token) {
            return Err(format!("probe manifest contains {token}"));
        }
    }
    Ok("probe_manifest=pass\n".to_owned())
}

fn root_pins(toml: &str) -> Result<String, String> {
    const AXUM: &str = "axum = { version = \"=0.8.9\", default-features = false, features = [\"http1\", \"tokio\"] }";
    const TOWER: &str = "tower = { version = \"=0.5.3\", default-features = false, features = [\"limit\", \"make\", \"timeout\", \"util\"] }";
    const TOWER_HTTP: &str = "tower-http = { version = \"=0.6.11\", default-features = false, features = [\"limit\", \"timeout\"] }";
    let body = table_body(toml, "[workspace.dependencies]")?;
    for line in body.lines() {
        let trim = line.trim();
        if trim.is_empty() || trim.starts_with('#') {
            continue;
        }
        if trim != AXUM && trim != TOWER && trim != TOWER_HTTP {
            return Err(format!("unexpected workspace dependency {trim}"));
        }
    }
    for line in [AXUM, TOWER, TOWER_HTTP] {
        if !body.contains(line) {
            return Err(format!("root pins missing {line}"));
        }
    }
    if toml.contains("oo-gateway-http-baseline-probe") {
        return Err("root manifest names the probe package".to_owned());
    }
    Ok("workspace_pins=pass\n".to_owned())
}

fn members(dir: &Path) -> Result<String, String> {
    let mut files = Vec::new();
    walk_cargo_toml(dir, &mut files)?;
    if files.is_empty() {
        return Err(format!("no member manifests under {}", dir.display()));
    }
    for path in &files {
        let text = fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))?;
        for (index, line) in text.lines().enumerate() {
            if line.contains("axum") || line.contains("tower-http") {
                return Err(format!(
                    "{}:{} uses a new HTTP pin: {line}",
                    path.display(),
                    index + 1
                ));
            }
            let trim = line.trim();
            if trim.starts_with("tower") && trim.contains("workspace") {
                return Err(format!(
                    "{}:{} uses the workspace tower pin: {line}",
                    path.display(),
                    index + 1
                ));
            }
        }
    }
    Ok(format!("members=pass\nmember_manifests={}\n", files.len()))
}

fn walk_cargo_toml(dir: &Path, files: &mut Vec<std::path::PathBuf>) -> Result<(), String> {
    let mut entries = fs::read_dir(dir)
        .map_err(|err| format!("read {}: {err}", dir.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| format!("read {}: {err}", dir.display()))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|err| format!("type {}: {err}", path.display()))?;
        if kind.is_symlink() {
            return Err(format!("symlink refused: {}", path.display()));
        }
        if kind.is_dir() {
            walk_cargo_toml(&path, files)?;
        } else if path.file_name().and_then(|name| name.to_str()) == Some("Cargo.toml") {
            files.push(path);
        }
    }
    Ok(())
}

fn collect_pins(texts: &[String]) -> Result<String, String> {
    let mut pins = BTreeMap::new();
    for text in texts {
        let line = meta_value(text, "pin")?;
        let mut parts = line.split_whitespace();
        let name = parts.next().ok_or("pin name")?.to_owned();
        if pins.insert(name.clone(), line).is_some() {
            return Err(format!("duplicate pin {name}"));
        }
    }
    let expect = ["axum", "axum-core", "httpdate", "matchit", "mime"];
    let got: BTreeSet<&str> = pins.keys().map(String::as_str).collect();
    let want: BTreeSet<&str> = expect.into_iter().collect();
    if got != want {
        return Err(format!("pin set {got:?} != {want:?}"));
    }
    let mut out = String::from("# name version created_at checksum license_field\n# Live crates.io metadata. Age gate is 2026-10-06T12:47:28Z.\n");
    for name in expect {
        out.push_str(pins.get(name).ok_or("pin")?);
        out.push('\n');
    }
    Ok(out)
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Pin {
    name: String,
    version: String,
    checksum: String,
}

fn parse_pins(text: &str) -> Result<BTreeMap<String, Pin>, String> {
    let mut pins = BTreeMap::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let name = parts.next().ok_or("pin name")?.to_owned();
        let version = parts.next().ok_or("pin version")?.to_owned();
        let _created = parts.next().ok_or("pin time")?;
        let checksum = parts.next().ok_or("pin checksum")?.to_owned();
        if checksum.len() != 64 {
            return Err(format!("pin {name} checksum length"));
        }
        pins.insert(
            name.clone(),
            Pin {
                name,
                version,
                checksum,
            },
        );
    }
    Ok(pins)
}

fn subset_report(stdout: &str, pins_text: &str) -> Result<String, String> {
    let pins = parse_pins(pins_text)?;
    let added = parse_added(stdout)?;
    if !stdout.lines().any(|line| line == "changed=0") {
        return Err("lockdiff changed is not 0".to_owned());
    }
    if !stdout.lines().any(|line| line == "denied=0") {
        return Err("lockdiff denied is not 0".to_owned());
    }
    if !stdout.lines().any(|line| line == "subset=pass") {
        return Err("lockdiff subset did not pass".to_owned());
    }
    if added.len() != pins.len() {
        return Err(format!("added {} != pins {}", added.len(), pins.len()));
    }
    for triple in &added {
        let pin = pins
            .get(&triple.0)
            .ok_or_else(|| format!("added {} is not pinned", triple.0))?;
        if pin.version != triple.1 || pin.checksum != triple.2 {
            return Err(format!(
                "added {} {} {} != pin {} {}",
                triple.0, triple.1, triple.2, pin.version, pin.checksum
            ));
        }
    }
    let added_names: BTreeSet<&str> = added.iter().map(|triple| triple.0.as_str()).collect();
    let pin_names: BTreeSet<&str> = pins.keys().map(String::as_str).collect();
    if added_names != pin_names {
        return Err("added names differ from pins".to_owned());
    }
    Ok("subset_report=pass\n".to_owned())
}

fn parse_added(text: &str) -> Result<Vec<(String, String, String)>, String> {
    let mut lines = text.lines();
    let header = lines
        .find_map(|line| line.strip_prefix("added="))
        .ok_or("added= missing")?;
    let count: usize = header.parse().map_err(|_| format!("added count {header}"))?;
    let mut rows = Vec::new();
    for _ in 0..count {
        let line = lines.next().ok_or("added section truncated")?;
        let mut parts = line.split_whitespace();
        let name = parts.next().ok_or("added name")?.to_owned();
        let version = parts.next().ok_or("added version")?.to_owned();
        let checksum = parts.next().ok_or("added checksum")?.to_owned();
        rows.push((name, version, checksum));
    }
    Ok(rows)
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Pkg {
    name: String,
    version: String,
    checksum: String,
    source: String,
    deps: BTreeSet<String>,
}

fn edges(root_text: &str, probe_text: &str, pins_text: &str) -> Result<String, String> {
    let pins = parse_pins(pins_text)?;
    edge_report(&parse_packages(root_text)?, &parse_packages(probe_text)?, &pins)
}

fn edge_report(root: &[Pkg], probe: &[Pkg], pins: &BTreeMap<String, Pin>) -> Result<String, String> {
    let root_map = pkg_map(root)?;
    let probe_map = pkg_map(probe)?;
    for pkg in probe {
        if is_denied(&pkg.name) {
            return Err(format!("denied package {}", pkg.name));
        }
    }
    let mut added = Vec::new();
    let mut local = Vec::new();
    for (key, pkg) in &probe_map {
        if root_map.contains_key(key) {
            continue;
        }
        if pkg.source.starts_with("registry+") {
            added.push(pkg.clone());
        } else if pkg.source.is_empty() && pkg.checksum.is_empty() {
            local.push(pkg.clone());
        } else {
            return Err(format!(
                "added {} {} has unexpected source {}",
                pkg.name, pkg.version, pkg.source
            ));
        }
    }
    added.sort();
    local.sort();
    if local.len() != 1
        || local[0].name != "oo-gateway-http-baseline-probe"
        || local[0].version != "0.0.0"
    {
        return Err(format!(
            "probe lock local packages are not only the baseline probe ({})",
            local.len()
        ));
    }
    if added.len() != pins.len() {
        return Err(format!("probe adds {} packages, pins {}", added.len(), pins.len()));
    }
    for pkg in &added {
        if !pkg.source.starts_with("registry+") {
            return Err(format!("added {} is not a registry package", pkg.name));
        }
        let pin = pins
            .get(&pkg.name)
            .ok_or_else(|| format!("added {} is outside the five pins", pkg.name))?;
        if pin.version != pkg.version || pin.checksum != pkg.checksum {
            return Err(format!(
                "added {} {} {} != pin {} {}",
                pkg.name, pkg.version, pkg.checksum, pin.version, pin.checksum
            ));
        }
    }
    for name in pins.keys() {
        if !added.iter().any(|pkg| &pkg.name == name) {
            return Err(format!("pin {name} was not added"));
        }
    }
    let mut out = String::new();
    out.push_str(&format!("added={}\n", added.len()));
    for pkg in &added {
        out.push_str(&format!("{} {} {}\n", pkg.name, pkg.version, pkg.checksum));
    }
    let mut diffs = 0usize;
    for (key, probe_pkg) in &probe_map {
        if let Some(root_pkg) = root_map.get(key) {
            if probe_pkg.deps != root_pkg.deps {
                diffs += 1;
                out.push_str(&format!(
                    "edge-diff {} {} root={} probe={}\n",
                    probe_pkg.name,
                    probe_pkg.version,
                    join_deps(&root_pkg.deps),
                    join_deps(&probe_pkg.deps)
                ));
            }
        }
    }
    out.push_str(&format!("edge_diff_count={diffs}\nedges=pass\n"));
    Ok(out)
}

fn join_deps(deps: &BTreeSet<String>) -> String {
    let mut out = String::from("[");
    for (index, dep) in deps.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(dep);
    }
    out.push(']');
    out
}

fn pkg_map(pkgs: &[Pkg]) -> Result<BTreeMap<(String, String, String, String), Pkg>, String> {
    let mut map = BTreeMap::new();
    for pkg in pkgs {
        let key = (
            pkg.name.clone(),
            pkg.version.clone(),
            pkg.checksum.clone(),
            pkg.source.clone(),
        );
        if map.insert(key, pkg.clone()).is_some() {
            return Err(format!("duplicate package {} {}", pkg.name, pkg.version));
        }
    }
    Ok(map)
}

fn parse_packages(text: &str) -> Result<Vec<Pkg>, String> {
    let mut pkgs = Vec::new();
    for chunk in text.split("[[package]]").skip(1) {
        pkgs.push(parse_chunk(chunk)?);
    }
    Ok(pkgs)
}

fn parse_chunk(chunk: &str) -> Result<Pkg, String> {
    let mut name = None;
    let mut version = None;
    let mut checksum = String::new();
    let mut source = String::new();
    let mut deps = BTreeSet::new();
    let mut in_deps = false;
    for raw in chunk.lines() {
        let line = raw.trim();
        if in_deps {
            if line == "]" {
                in_deps = false;
                continue;
            }
            if line.is_empty() {
                continue;
            }
            deps.insert(unquote_dep(line)?);
            continue;
        }
        if line == "dependencies = [" {
            in_deps = true;
            continue;
        }
        if let Some(value) = line.strip_prefix("name = ") {
            name = Some(unquote_toml(value));
        } else if let Some(value) = line.strip_prefix("version = ") {
            if name.is_none() {
                continue;
            }
            version = Some(unquote_toml(value));
        } else if let Some(value) = line.strip_prefix("checksum = ") {
            checksum = unquote_toml(value);
        } else if let Some(value) = line.strip_prefix("source = ") {
            source = unquote_toml(value);
        }
    }
    if in_deps {
        return Err("unclosed dependencies".to_owned());
    }
    Ok(Pkg {
        name: name.ok_or("package missing name")?,
        version: version.ok_or("package missing version")?,
        checksum,
        source,
        deps,
    })
}

fn unquote_dep(line: &str) -> Result<String, String> {
    let trim = line.trim().trim_end_matches(',');
    unquote_toml(trim.trim())
        .strip_prefix('"')
        .map(str::to_owned)
        .or_else(|| {
            let value = unquote_toml(trim.trim());
            if value.starts_with('"') {
                None
            } else {
                Some(value)
            }
        })
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("bad dependency {line}"))
}

fn is_denied(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["jwt", "jose", "oidc", "oauth", "openid", "aliri", "paseto", "jsonwebtoken"]
        .iter()
        .any(|token| lower.contains(token))
}

fn index_newest(json: &str, name: &str) -> Result<String, String> {
    let top = take_object(json.trim())?;
    let fields = fields_depth1(top)?;
    let crate_object = match fields.get("crate") {
        Some(JsonValue::Raw(value)) => value.clone(),
        _ => return Err("crate index missing crate object".to_owned()),
    };
    let crate_fields = fields_depth1(&crate_object)?;
    let got = field_string(&crate_fields, "name")?;
    if got != name {
        return Err(format!("index name {got} != {name}"));
    }
    let max_stable = field_string(&crate_fields, "max_stable_version")?;
    let versions = match fields.get("versions") {
        Some(JsonValue::Raw(value)) => value.clone(),
        _ => return Err("crate index missing versions".to_owned()),
    };
    let first = first_array_object(&versions)?;
    let newest = fields_depth1(first)?;
    let num = field_string(&newest, "num")?;
    let created = field_string(&newest, "created_at")?;
    let yanked = field_bool(&newest, "yanked")?;
    Ok(format!(
        "name={got}\nmax_stable_version={max_stable}\nnewest_num={num}\nnewest_created_at={created}\nnewest_yanked={yanked}\n"
    ))
}

fn lock_sha(lock: &str, name: &str, version: &str, sha_file: &str) -> Result<String, String> {
    let want = first_sha(sha_file)?;
    let mut found = 0usize;
    for pkg in parse_packages(lock)? {
        if pkg.name == name && pkg.version == version {
            found += 1;
            if pkg.checksum != want {
                return Err(format!(
                    "{name} {version} lock {} != crate {want}",
                    pkg.checksum
                ));
            }
        }
    }
    if found != 1 {
        return Err(format!("{name} {version} found {found} times"));
    }
    Ok(format!("lock_sha=pass\nname={name}\nversion={version}\nchecksum={want}\n"))
}

fn test_report(stdout: &str) -> Result<String, String> {
    const NAMES: [&str; 4] = [
        "echo_returns_body_and_ignores_auth_and_query",
        "body_at_limit_is_kept_and_one_extra_byte_is_413",
        "slow_route_returns_408_before_the_handler_finishes",
        "second_request_waits_for_the_single_permit",
    ];
    for name in NAMES {
        let needle = format!("{name} ... ok");
        if !stdout.contains(&needle) {
            return Err(format!("missing passing test {name}"));
        }
    }
    if !stdout.contains("4 passed") || !stdout.contains("0 failed") {
        return Err("test summary is not 4 passed and 0 failed".to_owned());
    }
    if stdout.contains("FAILED") {
        return Err("test stdout contains FAILED".to_owned());
    }
    Ok("test_report=pass\nhttp1_echo=pass\nbody_32_vs_33=pass\ntimeout_408=pass\nshared_global_permit=pass\ndistinct_route_permit=pass\nliteral_noauth_query_echo=pass\n".to_owned())
}

fn meta_value(text: &str, key: &str) -> Result<String, String> {
    let prefix = format!("{key}=");
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(&prefix) {
            return Ok(rest.to_owned());
        }
    }
    Err(format!("meta missing {key}"))
}

fn first_sha(text: &str) -> Result<String, String> {
    let line = text.lines().next().ok_or("empty sha manifest")?;
    let hex: String = line.chars().take(64).collect();
    if hex.len() != 64 || !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(format!("sha manifest line is not 64 hex digits: {line}"));
    }
    Ok(hex.to_ascii_lowercase())
}

fn msrv_at_most_194(declared: Option<&str>) -> Result<bool, String> {
    let Some(declared) = declared else {
        return Ok(true);
    };
    Ok(cmp_version(declared, "1.94.0")? != std::cmp::Ordering::Greater)
}

fn cmp_version(left: &str, right: &str) -> Result<std::cmp::Ordering, String> {
    let parse = |text: &str| -> Result<[u64; 3], String> {
        let mut parts = [0u64; 3];
        for (index, part) in text.split('.').enumerate() {
            if index > 2 {
                return Err(format!("version {text}"));
            }
            parts[index] = part.parse().map_err(|_| format!("version {text}"))?;
        }
        Ok(parts)
    };
    Ok(parse(left)?.cmp(&parse(right)?))
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum JsonValue {
    String(String),
    Bool(bool),
    Null,
    Raw(String),
}

fn object_after_key<'a>(text: &'a str, key: &str) -> Result<&'a str, String> {
    let pattern = format!("\"{key}\":");
    let pos = text
        .find(&pattern)
        .ok_or_else(|| format!("missing {key}"))?;
    let rest = text[pos + pattern.len()..].trim_start();
    if !rest.starts_with('{') {
        return Err(format!("{key} is not an object"));
    }
    take_object(rest)
}

fn fields_depth1(object: &str) -> Result<BTreeMap<String, JsonValue>, String> {
    let object = take_object(object.trim())?;
    let bytes = object.as_bytes();
    let mut index = 1usize;
    let mut fields = BTreeMap::new();
    loop {
        index = skip_ws(bytes, index);
        if index >= bytes.len() {
            return Err("unclosed object".to_owned());
        }
        if bytes[index] == b'}' {
            break;
        }
        let (key, next) = take_string(object, index)?;
        index = skip_ws(bytes, next);
        if bytes.get(index) != Some(&b':') {
            return Err(format!("expected colon after {key}"));
        }
        index = skip_ws(bytes, index + 1);
        let (value, next) = take_json_value(object, index)?;
        fields.insert(key, value);
        index = skip_ws(bytes, next);
        if bytes.get(index) == Some(&b',') {
            index += 1;
            continue;
        }
        if bytes.get(index) == Some(&b'}') {
            break;
        }
        return Err("expected comma or object end".to_owned());
    }
    Ok(fields)
}

fn take_json_value(text: &str, index: usize) -> Result<(JsonValue, usize), String> {
    let bytes = text.as_bytes();
    let index = skip_ws(bytes, index);
    match bytes.get(index).copied() {
        Some(b'"') => {
            let (value, next) = take_string(text, index)?;
            Ok((JsonValue::String(value), next))
        }
        Some(b'{') => {
            let object = take_object(&text[index..])?;
            Ok((JsonValue::Raw(object.to_owned()), index + object.len()))
        }
        Some(b'[') => {
            let array = take_array(&text[index..])?;
            Ok((JsonValue::Raw(array.to_owned()), index + array.len()))
        }
        Some(b't') => {
            expect_literal(text, index, "true")?;
            Ok((JsonValue::Bool(true), index + 4))
        }
        Some(b'f') => {
            expect_literal(text, index, "false")?;
            Ok((JsonValue::Bool(false), index + 5))
        }
        Some(b'n') => {
            expect_literal(text, index, "null")?;
            Ok((JsonValue::Null, index + 4))
        }
        Some(b'-') | Some(b'0'..=b'9') => {
            let start = index;
            let mut end = index + 1;
            while matches!(bytes.get(end), Some(b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')) {
                end += 1;
            }
            Ok((JsonValue::Raw(text[start..end].to_owned()), end))
        }
        _ => Err("bad json value".to_owned()),
    }
}

fn expect_literal(text: &str, index: usize, literal: &str) -> Result<(), String> {
    if text[index..].starts_with(literal) {
        Ok(())
    } else {
        Err(format!("expected {literal}"))
    }
}

fn take_string(text: &str, index: usize) -> Result<(String, usize), String> {
    let bytes = text.as_bytes();
    if bytes.get(index) != Some(&b'"') {
        return Err("expected string".to_owned());
    }
    let mut out = String::new();
    let mut cursor = index + 1;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if byte == b'\\' {
            let next = bytes.get(cursor + 1).copied().ok_or("bad escape")?;
            if next == b'u' {
                let hex = text.get(cursor + 2..cursor + 6).ok_or("bad unicode escape")?;
                let code = u32::from_str_radix(hex, 16).map_err(|_| "bad unicode escape".to_owned())?;
                out.push(char::from_u32(code).ok_or("bad unicode scalar")?);
                cursor += 6;
                continue;
            }
            out.push(match next {
                b'"' => '"',
                b'\\' => '\\',
                b'/' => '/',
                b'n' => '\n',
                b't' => '\t',
                _ => return Err("unsupported escape".to_owned()),
            });
            cursor += 2;
            continue;
        }
        if byte == b'"' {
            return Ok((out, cursor + 1));
        }
        out.push(byte as char);
        cursor += 1;
    }
    Err("unclosed string".to_owned())
}

fn take_object(text: &str) -> Result<&str, String> {
    take_nested(text, b'{', b'}')
}

fn take_array(text: &str) -> Result<&str, String> {
    take_nested(text, b'[', b']')
}

fn take_nested(text: &str, open: u8, close: u8) -> Result<&str, String> {
    let bytes = text.as_bytes();
    if bytes.first() != Some(&open) {
        return Err("nested value does not start at the cursor".to_owned());
    }
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for (index, byte) in bytes.iter().enumerate() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            if *byte == b'\\' {
                escape = true;
                continue;
            }
            if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match *byte {
            b'"' => in_string = true,
            value if value == open => depth += 1,
            value if value == close => {
                depth -= 1;
                if depth == 0 {
                    return Ok(&text[..=index]);
                }
            }
            _ => {}
        }
    }
    Err("unclosed nested value".to_owned())
}

fn first_array_object(array: &str) -> Result<&str, String> {
    let array = take_array(array.trim())?;
    let rest = array[1..].trim_start();
    if !rest.starts_with('{') {
        return Err("versions array does not start with an object".to_owned());
    }
    take_object(rest)
}

fn skip_ws(bytes: &[u8], mut index: usize) -> usize {
    while matches!(bytes.get(index), Some(b' ' | b'\n' | b'\r' | b'\t')) {
        index += 1;
    }
    index
}

fn field_string(fields: &BTreeMap<String, JsonValue>, key: &str) -> Result<String, String> {
    match fields.get(key) {
        Some(JsonValue::String(value)) => Ok(value.clone()),
        _ => Err(format!("missing string {key}")),
    }
}

fn field_bool(fields: &BTreeMap<String, JsonValue>, key: &str) -> Result<bool, String> {
    match fields.get(key) {
        Some(JsonValue::Bool(value)) => Ok(*value),
        _ => Err(format!("missing bool {key}")),
    }
}

fn parse_rfc3339(value: &str) -> Result<i64, String> {
    let value = value.trim().trim_end_matches('Z');
    let (date, time) = value
        .split_once('T')
        .ok_or_else(|| format!("time {value} is not RFC3339"))?;
    let mut date_parts = date.split('-');
    let year: i64 = date_parts
        .next()
        .ok_or("year")?
        .parse()
        .map_err(|_| format!("year in {value}"))?;
    let month: u32 = date_parts
        .next()
        .ok_or("month")?
        .parse()
        .map_err(|_| format!("month in {value}"))?;
    let day: u32 = date_parts
        .next()
        .ok_or("day")?
        .parse()
        .map_err(|_| format!("day in {value}"))?;
    if date_parts.next().is_some() {
        return Err(format!("date {date}"));
    }
    let time = time.split('.').next().ok_or("time")?;
    let mut time_parts = time.split(':');
    let hour: u32 = time_parts
        .next()
        .ok_or("hour")?
        .parse()
        .map_err(|_| format!("hour in {value}"))?;
    let minute: u32 = time_parts
        .next()
        .ok_or("minute")?
        .parse()
        .map_err(|_| format!("minute in {value}"))?;
    let second: u32 = time_parts
        .next()
        .ok_or("second")?
        .parse()
        .map_err(|_| format!("second in {value}"))?;
    if time_parts.next().is_some() || hour > 23 || minute > 59 || second > 60 {
        return Err(format!("time {value}"));
    }
    let days = civil_days(year, month, day)?;
    Ok(days * 86_400 + i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second))
}

fn civil_days(year: i64, month: u32, day: u32) -> Result<i64, String> {
    if !(1..=12).contains(&month) || day == 0 || day > 31 {
        return Err(format!("date {year}-{month}-{day}"));
    }
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month_index = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * month_index + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + i64::from(doy);
    Ok(era * 146_097 + doe - 719_468)
}

fn selfcheck() -> Result<(), String> {
    if parse_rfc3339("1970-01-01T00:00:00Z")? != 0 {
        return Err("unix epoch did not parse to 0".to_owned());
    }
    let as_of = parse_rfc3339(AS_OF_ADOPTION)?;
    let exact = parse_rfc3339("2026-09-29T12:47:28Z")?;
    if as_of - exact != SEVEN_DAYS {
        return Err("seven-day boundary moved".to_owned());
    }
    let young = sample_meta("2026-10-01T00:00:00Z", false, Some("1.80".to_owned()));
    let sha = sample_sha();
    if evaluate_meta(&young, "axum", "0.8.9", AS_OF_ADOPTION, &sha, &sha).is_ok() {
        return Err("a release under 7 days passed".to_owned());
    }
    let old = sample_meta("2026-04-14T07:55:20.258398Z", false, Some("1.80".to_owned()));
    evaluate_meta(&old, "axum", "0.8.9", AS_OF_ADOPTION, &sha, &sha)?;
    let yanked = sample_meta("2026-04-14T07:55:20.258398Z", true, Some("1.80".to_owned()));
    if evaluate_meta(&yanked, "axum", "0.8.9", AS_OF_ADOPTION, &sha, &sha).is_ok() {
        return Err("yanked crate passed".to_owned());
    }
    let high = sample_meta("2026-04-14T07:55:20.258398Z", false, Some("1.95.0".to_owned()));
    if evaluate_meta(&high, "axum", "0.8.9", AS_OF_ADOPTION, &sha, &sha).is_ok() {
        return Err("rust-version 1.95 passed".to_owned());
    }
    if !msrv_at_most_194(None)? || !msrv_at_most_194(Some("1.94.0"))? || msrv_at_most_194(Some("1.94.1"))? {
        return Err("msrv comparison failed".to_owned());
    }
    let parsed = parse_version_json(
        "{\"version\":{\"crate\":\"axum\",\"num\":\"0.8.9\",\"yanked\":false,\"license\":\"MIT\",\"created_at\":\"2026-04-14T07:55:20.258398Z\",\"checksum\":\"31b698c5f9a010f6573133b09e0de5408834d0c82f8d7475a89fc1867a71cd90\",\"rust_version\":\"1.80\"}}",
    )?;
    if parsed.name != "axum" || parsed.rust_version.as_deref() != Some("1.80") || parsed.yanked {
        return Err("version json parse failed".to_owned());
    }
    let undeclared = parse_version_json(
        "{\"version\":{\"crate\":\"mime\",\"num\":\"0.3.17\",\"yanked\":false,\"license\":\"MIT OR Apache-2.0\",\"created_at\":\"2023-03-20T15:05:04.272177Z\",\"checksum\":\"6877bb514081ee2a7ff5ef9de3281f14a4dd4bceac4c09388074a6b5df8a139a\",\"rust_version\":null}}",
    )?;
    if undeclared.rust_version.is_some() {
        return Err("null rust_version was kept".to_owned());
    }
    if !is_denied("jsonwebtoken") || !is_denied("oauth2") || is_denied("axum") || is_denied("tower") {
        return Err("denylist mismatch".to_owned());
    }
    let root = "version = 4\n\n[[package]]\nname = \"hyper\"\nversion = \"1.11.1\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\ndependencies = [\n \"h2\",\n]\n\n[[package]]\nname = \"kept\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"\n";
    let probe = "version = 4\n\n[[package]]\nname = \"hyper\"\nversion = \"1.11.1\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\ndependencies = [\n \"httpdate\",\n]\n\n[[package]]\nname = \"httpdate\"\nversion = \"1.0.3\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc\"\n\n[[package]]\nname = \"oo-gateway-http-baseline-probe\"\nversion = \"0.0.0\"\n";
    let pins = parse_pins(
        "httpdate 1.0.3 2023-08-13T10:55:54.391658Z cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc MIT OR Apache-2.0\n",
    )?;
    let report = edge_report(&parse_packages(root)?, &parse_packages(probe)?, &pins)?;
    if !report.contains("edge-diff hyper 1.11.1") || !report.contains("edges=pass") || !report.contains("added=1") {
        return Err(format!("edge selfcheck failed\n{report}"));
    }
    if !report.contains("added=1\nhttpdate 1.0.3 cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc\n") {
        return Err("httpdate was not the only added triple".to_owned());
    }
    let bad_pins = parse_pins(
        "axum 0.8.9 2026-04-14T07:55:20.258398Z dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd MIT\n",
    )?;
    if edge_report(&parse_packages(root)?, &parse_packages(probe)?, &bad_pins).is_ok() {
        return Err("wrong pin set passed the edge gate".to_owned());
    }
    let extra_local = format!("{probe}\n[[package]]\nname = \"other-local\"\nversion = \"0.0.0\"\n");
    if edge_report(&parse_packages(root)?, &parse_packages(&extra_local)?, &pins).is_ok() {
        return Err("an extra path package passed the edge gate".to_owned());
    }
    let stdout = "added=1\nhttpdate 1.0.3 cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc\nchanged=0\ndenied=0\nsubset=pass\n";
    let pin_text = "httpdate 1.0.3 2023-08-13T10:55:54.391658Z cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc MIT OR Apache-2.0\n";
    subset_report(stdout, pin_text)?;
    let tests = "test echo_returns_body_and_ignores_auth_and_query ... ok\ntest body_at_limit_is_kept_and_one_extra_byte_is_413 ... ok\ntest slow_route_returns_408_before_the_handler_finishes ... ok\ntest second_request_waits_for_the_single_permit ... ok\ntest result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n";
    if !test_report(tests)?.contains("literal_noauth_query_echo=pass") {
        return Err("test report missed the echo case".to_owned());
    }
    let inline = "[features]\nhttp1 = [\"hyper/http1\"]\nserver = [\"hyper/server\"]\n";
    if hyper_util_server(inline)? != "hyper_util_server=hyper/server\n" {
        return Err("inline feature array was not read".to_owned());
    }
    let multiline = "[features]\nhttp1 = [\n \"dep:httparse\",\n]\nserver = [\n \"dep:httpdate\",\n]\n";
    let dep = "[dependencies.httpdate]\nversion = \"1.0\"\noptional = true\n";
    if hyper_httpdate(&format!("{multiline}{dep}"))? != "hyper_httpdate=server\n" {
        return Err("multiline feature array was not read".to_owned());
    }
    Ok(())
}

fn sample_meta(created_at: &str, yanked: bool, rust_version: Option<String>) -> Meta {
    Meta {
        name: "axum".to_owned(),
        version: "0.8.9".to_owned(),
        yanked,
        license: "MIT".to_owned(),
        rust_version,
        checksum: "31b698c5f9a010f6573133b09e0de5408834d0c82f8d7475a89fc1867a71cd90".to_owned(),
        created_at: created_at.to_owned(),
    }
}

fn sample_sha() -> String {
    "31b698c5f9a010f6573133b09e0de5408834d0c82f8d7475a89fc1867a71cd90  ./crate\n".to_owned()
}
