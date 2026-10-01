//! Path, time, and digest helpers. Evidence reads stay inside an explicit root.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use crate::report::Issue;

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
        out.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
    }
    out
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    Ok(sha256_hex(&bytes))
}

static GIT_HOME_SEQ: AtomicU64 = AtomicU64::new(0);

/// One git invocation with a private empty home.
///
/// The home is outside the repo. Global and system config are an empty file,
/// so the caller's `HOME` and `~/.gitconfig` cannot supply an identity, a
/// signing rule, or an alias.
pub fn isolated_git(repo: &Path) -> Command {
    let seq = GIT_HOME_SEQ.fetch_add(1, Ordering::Relaxed);
    let home = std::env::temp_dir().join(format!("oo-xtask-git-home-{}-{seq}", std::process::id()));
    let _ = fs::create_dir_all(&home);
    let config = home.join("config");
    let _ = fs::write(&config, b"");
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(repo)
        .arg("-c")
        .arg("advice.defaultBranchName=false")
        .arg("-c")
        .arg("init.defaultBranch=master")
        .env("HOME", &home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", &config)
        .env("GIT_CONFIG_SYSTEM", &config)
        .env("GIT_AUTHOR_NAME", "xtask")
        .env("GIT_AUTHOR_EMAIL", "xtask@localhost")
        .env("GIT_COMMITTER_NAME", "xtask")
        .env("GIT_COMMITTER_EMAIL", "xtask@localhost")
        .env_remove("GIT_CONFIG")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_PREFIX")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_PARAMETERS");
    command
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct UtcStamp {
    pub unix_seconds: i64,
    pub nanos: u32,
}

pub fn parse_rfc3339_utc(text: &str) -> Result<UtcStamp, String> {
    let (body, zone) = if let Some(prefix) = text.strip_suffix('Z') {
        (prefix, "Z")
    } else if let Some(prefix) = text.strip_suffix("+00:00") {
        (prefix, "+00:00")
    } else {
        return Err("timestamp must be UTC with Z or +00:00".to_owned());
    };
    let (date, time) = body
        .split_once('T')
        .ok_or_else(|| format!("timestamp {text} has no T separator"))?;
    let mut date_parts = date.split('-');
    let year: i32 = parse_fixed(date_parts.next(), 4, "year")?;
    let month: u32 = parse_fixed(date_parts.next(), 2, "month")?;
    let day: u32 = parse_fixed(date_parts.next(), 2, "day")?;
    if date_parts.next().is_some() {
        return Err("timestamp date has extra fields".to_owned());
    }
    let (clock, frac) = time
        .split_once('.')
        .map(|(clock, frac)| (clock, Some(frac)))
        .unwrap_or((time, None));
    let mut clock_parts = clock.split(':');
    let hour: u32 = parse_fixed(clock_parts.next(), 2, "hour")?;
    let minute: u32 = parse_fixed(clock_parts.next(), 2, "minute")?;
    let second: u32 = parse_fixed(clock_parts.next(), 2, "second")?;
    if clock_parts.next().is_some() {
        return Err("timestamp time has extra fields".to_owned());
    }
    if !(1..=12).contains(&month) || hour > 23 || minute > 59 || second > 59 {
        return Err(format!("timestamp {text} is not a civil UTC time"));
    }
    let nanos = match frac {
        None => 0,
        Some(digits)
            if digits.is_empty()
                || digits.len() > 9
                || !digits.bytes().all(|b| b.is_ascii_digit()) =>
        {
            return Err("fractional seconds must be 1 to 9 digits".to_owned());
        }
        Some(digits) => {
            let mut padded = digits.to_owned();
            padded.extend(std::iter::repeat_n('0', 9 - digits.len()));
            padded
                .parse::<u32>()
                .map_err(|_| "fractional seconds".to_owned())?
        }
    };
    let days = civil_days(year, month, day)
        .ok_or_else(|| format!("timestamp {text} is not a civil date"))?;
    let _ = zone;
    Ok(UtcStamp {
        unix_seconds: days * 86_400
            + i64::from(hour) * 3_600
            + i64::from(minute) * 60
            + i64::from(second),
        nanos,
    })
}

fn parse_fixed<T: std::str::FromStr>(
    part: Option<&str>,
    width: usize,
    name: &str,
) -> Result<T, String> {
    let text = part.ok_or_else(|| format!("timestamp is missing {name}"))?;
    if text.len() != width || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("timestamp {name} must be {width} digits"));
    }
    text.parse::<T>()
        .map_err(|_| format!("timestamp {name} is not a number"))
}

fn civil_days(year: i32, month: u32, day: u32) -> Option<i64> {
    let leap = |year: i32| year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let mdays = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if month == 0 || month > 12 {
        return None;
    }
    let mut max = mdays[month as usize];
    if month == 2 && leap(year) {
        max = 29;
    }
    if day == 0 || day > max {
        return None;
    }
    let mut y = year;
    let m = month as i32;
    y -= if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + day as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy as u32;
    Some(i64::from(era) * 146_097 + i64::from(doe) - 719_468)
}

/// Top-level keys of one JSON object, in order. Nested keys are not returned.
/// A duplicate key appears twice so callers can fail closed.
pub fn top_level_keys(text: &str) -> Result<Vec<String>, String> {
    let bytes = text.as_bytes();
    let mut index = 0;
    skip_ws(bytes, &mut index);
    if index >= bytes.len() || bytes[index] != b'{' {
        return Err("json object expected".to_owned());
    }
    index += 1;
    let mut keys = Vec::new();
    loop {
        skip_ws(bytes, &mut index);
        if index >= bytes.len() {
            return Err("unterminated object".to_owned());
        }
        if bytes[index] == b'}' {
            return Ok(keys);
        }
        if bytes[index] != b'"' {
            return Err("object key expected".to_owned());
        }
        let key = parse_json_string(bytes, &mut index)?;
        skip_ws(bytes, &mut index);
        if index >= bytes.len() || bytes[index] != b':' {
            return Err("colon expected".to_owned());
        }
        index += 1;
        skip_json_value(bytes, &mut index)?;
        keys.push(key);
        skip_ws(bytes, &mut index);
        if index >= bytes.len() {
            return Err("unterminated object".to_owned());
        }
        if bytes[index] == b',' {
            index += 1;
            continue;
        }
        if bytes[index] == b'}' {
            return Ok(keys);
        }
        return Err("object separator expected".to_owned());
    }
}

pub fn duplicate_key(keys: &[String]) -> Option<&str> {
    let mut seen = std::collections::BTreeSet::new();
    for key in keys {
        if !seen.insert(key.as_str()) {
            return Some(key.as_str());
        }
    }
    None
}

fn skip_ws(bytes: &[u8], index: &mut usize) {
    while *index < bytes.len() && matches!(bytes[*index], b' ' | b'\t' | b'\n' | b'\r') {
        *index += 1;
    }
}

fn parse_json_string(bytes: &[u8], index: &mut usize) -> Result<String, String> {
    if *index >= bytes.len() || bytes[*index] != b'"' {
        return Err("string expected".to_owned());
    }
    *index += 1;
    let mut out = Vec::new();
    while *index < bytes.len() {
        let byte = bytes[*index];
        *index += 1;
        match byte {
            b'"' => {
                return String::from_utf8(out).map_err(|_| "string is not utf-8".to_owned());
            }
            b'\\' => {
                if *index >= bytes.len() {
                    return Err("bad escape".to_owned());
                }
                let escaped = bytes[*index];
                *index += 1;
                match escaped {
                    b'"' | b'\\' | b'/' => out.push(escaped),
                    b'b' => out.push(0x08),
                    b'f' => out.push(0x0c),
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'u' => {
                        if *index + 4 > bytes.len() {
                            return Err("bad unicode escape".to_owned());
                        }
                        let hex = std::str::from_utf8(&bytes[*index..*index + 4])
                            .map_err(|_| "bad unicode escape".to_owned())?;
                        *index += 4;
                        let code = u32::from_str_radix(hex, 16)
                            .map_err(|_| "bad unicode escape".to_owned())?;
                        let ch =
                            char::from_u32(code).ok_or_else(|| "bad unicode escape".to_owned())?;
                        let mut buf = [0; 4];
                        out.extend(ch.encode_utf8(&mut buf).as_bytes());
                    }
                    _ => return Err("bad escape".to_owned()),
                }
            }
            other => out.push(other),
        }
    }
    Err("unterminated string".to_owned())
}

fn skip_json_value(bytes: &[u8], index: &mut usize) -> Result<(), String> {
    skip_ws(bytes, index);
    if *index >= bytes.len() {
        return Err("value expected".to_owned());
    }
    match bytes[*index] {
        b'"' => {
            parse_json_string(bytes, index)?;
            Ok(())
        }
        b'{' => skip_container(bytes, index, b'{', b'}'),
        b'[' => skip_container(bytes, index, b'[', b']'),
        b't' => skip_literal(bytes, index, b"true"),
        b'f' => skip_literal(bytes, index, b"false"),
        b'n' => skip_literal(bytes, index, b"null"),
        b'-' | b'0'..=b'9' => skip_number(bytes, index),
        _ => Err("value expected".to_owned()),
    }
}

fn skip_literal(bytes: &[u8], index: &mut usize, literal: &[u8]) -> Result<(), String> {
    if bytes[*index..].starts_with(literal) {
        *index += literal.len();
        Ok(())
    } else {
        Err("literal expected".to_owned())
    }
}

fn skip_number(bytes: &[u8], index: &mut usize) -> Result<(), String> {
    let start = *index;
    if bytes[*index] == b'-' {
        *index += 1;
    }
    if *index >= bytes.len() || !bytes[*index].is_ascii_digit() {
        return Err("number expected".to_owned());
    }
    while *index < bytes.len() && bytes[*index].is_ascii_digit() {
        *index += 1;
    }
    if *index < bytes.len() && bytes[*index] == b'.' {
        *index += 1;
        while *index < bytes.len() && bytes[*index].is_ascii_digit() {
            *index += 1;
        }
    }
    if *index < bytes.len() && matches!(bytes[*index], b'e' | b'E') {
        *index += 1;
        if *index < bytes.len() && matches!(bytes[*index], b'+' | b'-') {
            *index += 1;
        }
        while *index < bytes.len() && bytes[*index].is_ascii_digit() {
            *index += 1;
        }
    }
    if *index == start {
        return Err("number expected".to_owned());
    }
    Ok(())
}

fn skip_container(bytes: &[u8], index: &mut usize, open: u8, close: u8) -> Result<(), String> {
    if *index >= bytes.len() || bytes[*index] != open {
        return Err("container expected".to_owned());
    }
    *index += 1;
    let mut depth = 1;
    let mut in_string = false;
    let mut escape = false;
    while *index < bytes.len() {
        let byte = bytes[*index];
        *index += 1;
        if in_string {
            if escape {
                escape = false;
            } else if byte == b'\\' {
                escape = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        if byte == b'"' {
            in_string = true;
        } else if byte == open {
            depth += 1;
        } else if byte == close {
            depth -= 1;
            if depth == 0 {
                return Ok(());
            }
        }
    }
    Err("unterminated container".to_owned())
}

pub fn now_rfc3339() -> String {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format_unix(elapsed.as_secs() as i64, elapsed.subsec_nanos())
}

fn format_unix(seconds: i64, nanos: u32) -> String {
    let days = seconds.div_euclid(86_400);
    let sod = seconds.rem_euclid(86_400) as u32;
    let (year, month, day) = civil_from_days(days);
    let hour = sod / 3_600;
    let minute = (sod % 3_600) / 60;
    let second = sod % 60;
    if nanos == 0 {
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
    } else {
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{nanos:09}Z")
    }
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i32 + era as i32 * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    (year, month, day)
}

pub fn read_no_symlink(root: &Path, relative: &str) -> Result<Vec<u8>, Issue> {
    let path = join_inside(root, relative)?;
    let metadata = fs::symlink_metadata(&path).map_err(|error| {
        Issue::new(
            "path.read",
            relative,
            format!("cannot stat {}: {error}", path.display()),
        )
    })?;
    if metadata.file_type().is_symlink() {
        return Err(Issue::new("path.symlink", relative, "symlinks are refused"));
    }
    if !metadata.file_type().is_file() {
        return Err(Issue::new(
            "path.not_file",
            relative,
            "path is not a regular file",
        ));
    }
    const MAX: u64 = 64 * 1024 * 1024;
    if metadata.len() > MAX {
        return Err(Issue::new(
            "path.too_large",
            relative,
            format!("file is {} bytes; limit is {MAX}", metadata.len()),
        ));
    }
    fs::read(&path).map_err(|error| Issue::new("path.read", relative, error.to_string()))
}

pub fn join_inside(root: &Path, relative: &str) -> Result<PathBuf, Issue> {
    if relative.is_empty()
        || relative.starts_with('/')
        || relative.starts_with('\\')
        || relative.starts_with('~')
        || relative.contains('\0')
    {
        return Err(Issue::new(
            "path.escape",
            relative,
            "absolute, empty, home, or NUL paths are refused",
        ));
    }
    let relative_path = Path::new(relative);
    if relative_path.is_absolute() {
        return Err(Issue::new(
            "path.escape",
            relative,
            "absolute paths are refused",
        ));
    }
    let mut out = root.to_path_buf();
    for component in relative_path.components() {
        match component {
            Component::Normal(part) => {
                let name = part.to_string_lossy();
                if name == ".." || name == "." || name.contains('/') || name.contains('\\') {
                    return Err(Issue::new(
                        "path.escape",
                        relative,
                        "path component is refused",
                    ));
                }
                out.push(part);
                if out.exists() {
                    let meta = fs::symlink_metadata(&out)
                        .map_err(|error| Issue::new("path.read", relative, error.to_string()))?;
                    if meta.file_type().is_symlink() {
                        return Err(Issue::new(
                            "path.symlink",
                            relative,
                            "a path component is a symlink",
                        ));
                    }
                }
            }
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => {
                return Err(Issue::new(
                    "path.escape",
                    relative,
                    "path escapes the allowed root",
                ));
            }
        }
    }
    Ok(out)
}

pub fn private_component(relative: &str) -> bool {
    Path::new(relative)
        .components()
        .any(|component| match component {
            Component::Normal(part) => {
                let name = part.to_string_lossy().to_ascii_lowercase();
                matches!(
                    name.as_str(),
                    "gold"
                        | "holdout"
                        | "customer"
                        | "customer-private"
                        | ".ssh"
                        | ".gnupg"
                        | ".aws"
                )
            }
            _ => false,
        })
}

pub const UNIMPLEMENTED_SUITES: &[&str] = &[
    "integration-a",
    "security-b",
    "runtime-c",
    "release-state",
    "operational",
    "public-e",
    "ontology-d",
    "performance-f",
];

pub const PHASES: &[&str] = &[
    "SP00", "SP01", "SP02", "SP03", "SP04", "SP05", "SP06", "SP07", "SP08", "SP09", "SP10", "SP11",
    "SP12", "SP13", "SP14", "SP15",
];

pub fn phase_rank(phase: &str) -> Option<usize> {
    PHASES.iter().position(|item| *item == phase)
}

/// Uncommented `@login` and `@org/team` tokens. Commented lines and
/// `OWNER_PENDING` are not appointments. The current file yields none.
pub fn appointed_logins(repo: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(repo.join(".github/CODEOWNERS")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.contains("OWNER_PENDING") {
            continue;
        }
        for token in trimmed.split_whitespace() {
            let Some(login) = token.strip_prefix('@') else {
                continue;
            };
            if login.is_empty() || out.iter().any(|item| item == login) {
                continue;
            }
            out.push(login.to_owned());
        }
    }
    out
}

/// True only when SECURITY.md no longer says the owner is unappointed and
/// CODEOWNERS has at least one real login. A missing file is unappointed.
pub fn security_owner_appointed(repo: &Path) -> bool {
    let Ok(text) = fs::read_to_string(repo.join("SECURITY.md")) else {
        return false;
    };
    if text
        .to_ascii_lowercase()
        .contains("security owner is not appointed")
    {
        return false;
    }
    !appointed_logins(repo).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_epoch_and_known_civil_day() {
        let epoch = parse_rfc3339_utc("1970-01-01T00:00:00Z").unwrap();
        assert_eq!(epoch.unix_seconds, 0);
        let later = parse_rfc3339_utc("2026-10-01T00:00:00Z").unwrap();
        assert!(later > epoch);
        let offset = parse_rfc3339_utc("2026-10-01T00:00:00+00:00").unwrap();
        assert_eq!(later, offset);
        assert!(parse_rfc3339_utc("2026-10-01T00:00:00+08:00").is_err());
        assert!(parse_rfc3339_utc("2026-10-01 00:00:00Z").is_err());
        assert!(parse_rfc3339_utc("2026-02-29T00:00:00Z").is_err());
    }

    #[test]
    fn finished_before_started_orders() {
        let start = parse_rfc3339_utc("2026-10-01T00:00:01.5Z").unwrap();
        let earlier = parse_rfc3339_utc("2026-10-01T00:00:01.499Z").unwrap();
        assert!(earlier < start);
        let now = now_rfc3339();
        assert!(parse_rfc3339_utc(&now).is_ok(), "{now}");
    }

    #[test]
    fn top_level_keys_ignore_nested_and_report_duplicates() {
        let keys = top_level_keys(
            r#"{"name":"serde","deps":[{"name":"inner"}],"yanked":false,"yanked":true}"#,
        )
        .unwrap();
        assert_eq!(
            keys,
            vec!["name", "deps", "yanked", "yanked"]
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        );
        assert_eq!(duplicate_key(&keys), Some("yanked"));
        let nested = top_level_keys(r#"{"name":"libc","cksum":"ab"}"#).unwrap();
        assert!(duplicate_key(&nested).is_none());
    }

    #[test]
    fn join_rejects_escape_and_home() {
        let root = std::env::temp_dir().join("oo-xtask-path-test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("nested")).unwrap();
        assert!(join_inside(&root, "nested/file.json").is_ok());
        assert!(join_inside(&root, "../secret").is_err());
        assert!(join_inside(&root, "/etc/passwd").is_err());
        assert!(join_inside(&root, "~/id_rsa").is_err());
        assert!(join_inside(&root, "nested/../../.ssh/id_rsa").is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn isolated_git_commits_without_the_caller_home() {
        let repo = std::env::temp_dir().join(format!("oo-a058-git-{}", std::process::id()));
        let _ = fs::remove_dir_all(&repo);
        fs::create_dir_all(&repo).unwrap();
        let run = |args: &[&str]| {
            let output = isolated_git(&repo).args(args).output().expect("git");
            assert!(
                output.status.success(),
                "{args:?}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        run(&["init"]);
        fs::write(repo.join("note.txt"), b"public\n").unwrap();
        run(&["add", "--", "note.txt"]);
        run(&[
            "-c",
            "user.email=xtask@localhost",
            "-c",
            "user.name=xtask",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "fixture",
        ]);
        let status = isolated_git(&repo)
            .args(["status", "--porcelain"])
            .output()
            .expect("git status");
        assert!(status.status.success());
        assert!(
            status.stdout.is_empty(),
            "private git home dirtied the worktree: {}",
            String::from_utf8_lossy(&status.stdout)
        );
        let _ = fs::remove_dir_all(&repo);
    }
}
