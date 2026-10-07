//! Compare two Cargo.lock files and check newly added crates.
//! Std-only. The digest used elsewhere is sha256_manifest.rs. This file does not hash crates.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::process::ExitCode;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Triple {
    name: String,
    version: String,
    checksum: String,
    source: String,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("lockdiff: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let mode = args.next().ok_or("usage: lockdiff <exact|subset|selfcheck> <old.lock> <new.lock> [pins as_of]")?;
    if mode == "selfcheck" {
        if args.next().is_some() {
            return Err("selfcheck takes no arguments".to_owned());
        }
        selfcheck()?;
        print!("selfcheck=pass\n");
        return Ok(());
    }
    let old_path = args.next().ok_or("missing old lock")?;
    let new_path = args.next().ok_or("missing new lock")?;
    let old = parse_lock(&fs::read_to_string(&old_path).map_err(|err| format!("read {old_path}: {err}"))?)?;
    let new = parse_lock(&fs::read_to_string(&new_path).map_err(|err| format!("read {new_path}: {err}"))?)?;
    let report = match mode.as_str() {
        "exact" => exact(&old, &new)?,
        "subset" => {
            let pins_path = args.next().ok_or("subset requires pins and as_of")?;
            let as_of = args.next().ok_or("subset requires as_of RFC3339")?;
            if args.next().is_some() {
                return Err("too many arguments".to_owned());
            }
            subset(&old, &new, &pins_path, &as_of)?
        }
        other => return Err(format!("unknown mode {other}")),
    };
    print!("{report}");
    Ok(())
}

fn exact(old: &[Triple], new: &[Triple]) -> Result<String, String> {
    let mut out = String::new();
    let (added, removed, changed) = classify(old, new);
    push_section(&mut out, "added", &added);
    push_section(&mut out, "removed", &removed);
    push_changed(&mut out, &changed);
    if !added.is_empty() || !removed.is_empty() || !changed.is_empty() {
        return Err(format!("exact lock diff is not empty\n{out}"));
    }
    out.push_str("exact=pass\n");
    Ok(out)
}

fn subset(old: &[Triple], new: &[Triple], pins_path: &str, as_of: &str) -> Result<String, String> {
    let pins = parse_pins(&fs::read_to_string(pins_path).map_err(|err| format!("read {pins_path}: {err}"))?)?;
    let as_of_unix = parse_rfc3339(as_of)?;
    let (added, _removed, changed) = classify(old, new);
    let mut denied = Vec::new();
    for triple in new {
        if is_denied(&triple.name) {
            denied.push(triple.clone());
        }
    }
    let mut out = String::new();
    push_section(&mut out, "added", &added);
    push_changed(&mut out, &changed);
    out.push_str("denied=");
    out.push_str(&denied.len().to_string());
    out.push('\n');
    if !denied.is_empty() || !changed.is_empty() {
        return Err(format!("subset gate failed\n{out}"));
    }
    for triple in &added {
        let pin = pins.get(&triple.name).ok_or_else(|| {
            format!("added crate {} {} is not in the pin file\n{out}", triple.name, triple.version)
        })?;
        if pin.version != triple.version || pin.checksum != triple.checksum {
            return Err(format!(
                "added {} lock {} {} != pin {} {}\n{out}",
                triple.name, triple.version, triple.checksum, pin.version, pin.checksum
            ));
        }
        let published = parse_rfc3339(&pin.created_at)?;
        if as_of_unix < published {
            return Err(format!("{} adopted before publication", triple.name));
        }
        let age = as_of_unix - published;
        if age < 7 * 86_400 {
            return Err(format!("{} age {age}s is under 7 days", triple.name));
        }
        out.push_str(&format!(
            "sbom name={} version={} checksum={} source={} license_field={} published={} age_seconds={}\n",
            triple.name, triple.version, triple.checksum, triple.source, pin.license, pin.created_at, age
        ));
    }
    out.push_str("subset=pass\n");
    Ok(out)
}

struct Pin {
    version: String,
    created_at: String,
    checksum: String,
    license: String,
}

fn parse_pins(text: &str) -> Result<BTreeMap<String, Pin>, String> {
    let mut pins = BTreeMap::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let name = parts.next().ok_or("pin name")?.to_owned();
        let version = parts.next().ok_or("pin version")?.to_owned();
        let created_at = parts.next().ok_or("pin time")?.to_owned();
        let checksum = parts.next().ok_or("pin checksum")?.to_owned();
        let license = parts.collect::<Vec<_>>().join(" ");
        if license.is_empty() {
            return Err(format!("pin line {} missing license", index + 1));
        }
        if checksum.len() != 64 || !checksum.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Err(format!("pin {name} checksum is not 64 hex digits"));
        }
        pins.insert(
            name,
            Pin {
                version,
                created_at,
                checksum,
                license,
            },
        );
    }
    Ok(pins)
}

fn is_denied(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["jwt", "jose", "oidc", "oauth", "openid", "aliri", "paseto", "jsonwebtoken"]
        .iter()
        .any(|token| lower.contains(token))
}

fn classify(old: &[Triple], new: &[Triple]) -> (Vec<Triple>, Vec<Triple>, Vec<(Triple, Triple)>) {
    let old_names = by_name(old);
    let new_names = by_name(new);
    let mut names = BTreeSet::new();
    names.extend(old_names.keys().cloned());
    names.extend(new_names.keys().cloned());
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    for name in names {
        let empty = Vec::new();
        let old_rows = old_names.get(&name).unwrap_or(&empty);
        let new_rows = new_names.get(&name).unwrap_or(&empty);
        let old_ids: BTreeSet<_> = old_rows.iter().map(identity).collect();
        let new_ids: BTreeSet<_> = new_rows.iter().map(identity).collect();
        let mut dropped: Vec<_> = old_rows
            .iter()
            .filter(|triple| !new_ids.contains(&identity(triple)))
            .cloned()
            .collect();
        let mut arrived: Vec<_> = new_rows
            .iter()
            .filter(|triple| !old_ids.contains(&identity(triple)))
            .cloned()
            .collect();
        dropped.sort();
        arrived.sort();
        if dropped.is_empty() && arrived.is_empty() {
            continue;
        }
        if old_rows.is_empty() {
            added.extend(arrived);
            continue;
        }
        if arrived.is_empty() {
            removed.extend(dropped);
            continue;
        }
        let width = dropped.len().max(arrived.len());
        for index in 0..width {
            let before = dropped.get(index).cloned().unwrap_or_else(|| Triple {
                name: name.clone(),
                version: "(additional)".to_owned(),
                checksum: "-".to_owned(),
                source: String::new(),
            });
            let after = arrived.get(index).cloned().unwrap_or_else(|| Triple {
                name: name.clone(),
                version: "(removed)".to_owned(),
                checksum: "-".to_owned(),
                source: String::new(),
            });
            changed.push((before, after));
        }
    }
    (added, removed, changed)
}

fn by_name(triples: &[Triple]) -> BTreeMap<String, Vec<Triple>> {
    let mut map: BTreeMap<String, Vec<Triple>> = BTreeMap::new();
    for triple in triples {
        map.entry(triple.name.clone()).or_default().push(triple.clone());
    }
    for rows in map.values_mut() {
        rows.sort();
    }
    map
}

fn identity(triple: &Triple) -> (String, String, String) {
    (triple.name.clone(), triple.version.clone(), triple.checksum.clone())
}

fn triple(name: &str, version: &str, checksum: &str) -> Triple {
    Triple {
        name: name.to_owned(),
        version: version.to_owned(),
        checksum: checksum.to_owned(),
        source: "registry+https://github.com/rust-lang/crates.io-index".to_owned(),
    }
}

fn selfcheck() -> Result<(), String> {
    if parse_rfc3339("1970-01-01T00:00:00Z")? != 0 {
        return Err("unix epoch did not parse to 0".to_owned());
    }
    let published = parse_rfc3339("2026-04-14T07:55:20.258398Z")?;
    let as_of = parse_rfc3339("2026-10-06T12:47:28Z")?;
    let age = as_of - published;
    if age < 7 * 86_400 {
        return Err(format!("axum 0.8.9 age {age}s is under 7 days"));
    }
    let young = parse_rfc3339("2026-09-30T12:47:28Z")?;
    if as_of - young >= 7 * 86_400 {
        return Err("a six-day-old release passed the age gate".to_owned());
    }
    let checksum_a = "a".repeat(64);
    let checksum_b = "b".repeat(64);
    let checksum_c = "c".repeat(64);
    let old = vec![triple("foo", "1.0.0", &checksum_a), triple("foo", "2.0.0", &checksum_b)];
    let kept = vec![triple("foo", "1.0.0", &checksum_a)];
    let (added, removed, changed) = classify(&old, &kept);
    if !added.is_empty() || !changed.is_empty() || removed.len() != 1 || removed[0].version != "2.0.0" {
        return Err("keeping one of two versions was not a removal of the other".to_owned());
    }
    let extra = vec![triple("foo", "1.0.0", &checksum_a), triple("foo", "3.0.0", &checksum_c)];
    let (added, _, changed) = classify(&old, &extra);
    if !added.is_empty() || changed.is_empty() {
        return Err("a new version of an existing name was not a change".to_owned());
    }
    let brand_new = vec![triple("mime", "0.3.17", &checksum_c)];
    let (added, removed, changed) = classify(&[], &brand_new);
    if added.len() != 1 || !removed.is_empty() || !changed.is_empty() {
        return Err("a new name was not classified as added".to_owned());
    }
    let (added, removed, changed) = classify(&old, &old);
    if !added.is_empty() || !removed.is_empty() || !changed.is_empty() {
        return Err("identical locks differed".to_owned());
    }
    let pins = parse_pins(
        "# comment\nmime 0.3.17 2023-03-20T15:05:04.272177Z dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd MIT OR Apache-2.0\n",
    )?;
    let pin = pins.get("mime").ok_or("mime pin missing")?;
    if pin.license != "MIT OR Apache-2.0" {
        return Err(format!("license parse {}", pin.license));
    }
    if !is_denied("jsonwebtoken") || !is_denied("oauth2") || is_denied("axum") || is_denied("tower") {
        return Err("denylist mismatch".to_owned());
    }
    let parsed = parse_lock(
        "version = 4\n\n[[package]]\nname = \"mime\"\nversion = \"0.3.17\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd\"\n",
    )?;
    if parsed.len() != 1 || parsed[0].name != "mime" || parsed[0].version != "0.3.17" {
        return Err("lockfile header version was treated as a package".to_owned());
    }
    Ok(())
}

fn push_section(out: &mut String, title: &str, rows: &[Triple]) {
    out.push_str(title);
    out.push('=');
    out.push_str(&rows.len().to_string());
    out.push('\n');
    for row in rows {
        out.push_str(&format!("{} {} {}\n", row.name, row.version, row.checksum));
    }
}

fn push_changed(out: &mut String, rows: &[(Triple, Triple)]) {
    out.push_str("changed=");
    out.push_str(&rows.len().to_string());
    out.push('\n');
    for (before, after) in rows {
        out.push_str(&format!(
            "{} {} {} -> {} {}\n",
            before.name, before.version, before.checksum, after.version, after.checksum
        ));
    }
}

fn parse_lock(text: &str) -> Result<Vec<Triple>, String> {
    let mut triples = Vec::new();
    let mut name = None;
    let mut version = None;
    let mut checksum = None;
    let mut source = None;
    let flush = |name: &mut Option<String>,
                 version: &mut Option<String>,
                 checksum: &mut Option<String>,
                 source: &mut Option<String>,
                 triples: &mut Vec<Triple>|
     -> Result<(), String> {
        if let Some(name) = name.take() {
            let version = version.take().ok_or_else(|| format!("{name} missing version"))?;
            let source = source.take().unwrap_or_default();
            let checksum = checksum.take().unwrap_or_default();
            if source.starts_with("registry+") {
                if checksum.len() != 64 {
                    return Err(format!("{name} {version} missing registry checksum"));
                }
                triples.push(Triple {
                    name,
                    version,
                    checksum,
                    source,
                });
            }
        }
        Ok(())
    };
    for raw in text.lines() {
        let line = raw.trim();
        if line == "[[package]]" {
            flush(&mut name, &mut version, &mut checksum, &mut source, &mut triples)?;
            continue;
        }
        if let Some(value) = line.strip_prefix("name = ") {
            name = Some(unquote(value)?);
        } else if let Some(value) = line.strip_prefix("version = ") {
            if name.is_none() {
                continue;
            }
            version = Some(unquote(value)?);
        } else if let Some(value) = line.strip_prefix("checksum = ") {
            checksum = Some(unquote(value)?);
        } else if let Some(value) = line.strip_prefix("source = ") {
            source = Some(unquote(value)?);
        }
    }
    flush(&mut name, &mut version, &mut checksum, &mut source, &mut triples)?;
    Ok(triples)
}

fn unquote(value: &str) -> Result<String, String> {
    let value = value.trim();
    if let Some(inner) = value.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')) {
        return Ok(inner.to_owned());
    }
    Err(format!("expected quoted value, got {value}"))
}

fn parse_rfc3339(value: &str) -> Result<i64, String> {
    let value = value.trim().trim_end_matches('Z');
    let (date, time) = value
        .split_once('T')
        .ok_or_else(|| format!("time {value} is not RFC3339"))?;
    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next().ok_or("year")?.parse().map_err(|_| format!("year in {value}"))?;
    let month: u32 = date_parts.next().ok_or("month")?.parse().map_err(|_| format!("month in {value}"))?;
    let day: u32 = date_parts.next().ok_or("day")?.parse().map_err(|_| format!("day in {value}"))?;
    if date_parts.next().is_some() {
        return Err(format!("date {date}"));
    }
    let time = time.split('.').next().ok_or("time")?;
    let mut time_parts = time.split(':');
    let hour: u32 = time_parts.next().ok_or("hour")?.parse().map_err(|_| format!("hour in {value}"))?;
    let minute: u32 = time_parts.next().ok_or("minute")?.parse().map_err(|_| format!("minute in {value}"))?;
    let second: u32 = time_parts.next().ok_or("second")?.parse().map_err(|_| format!("second in {value}"))?;
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
