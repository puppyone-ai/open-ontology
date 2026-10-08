use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::analyze::Group;
use crate::columnar::Columns;
use crate::decimal::Decimal;
use crate::fault::Fault;
use crate::json::{self, Json};
use crate::limits::MAX_CLI_BYTES;
use crate::path;
use crate::sha256::{is_lower_hex, sha256_hex};

const MANIFEST: &str = include_str!("../native/duckdb-cli.json");
const AGGREGATE_TIMEOUT: Duration = Duration::from_secs(30);
const CANCEL_TIMEOUT: Duration = Duration::from_secs(120);

pub fn require_cli(path: &Path) -> Result<String, Fault> {
    let expected = manifest_field("unpacked_sha256").ok_or(Fault::engine())?;
    if !is_lower_hex(&expected, Some(64)) {
        return Err(Fault::engine());
    }
    let bytes = match path::read_capped(path, MAX_CLI_BYTES) {
        Ok(bytes) => bytes,
        Err(fault) if fault.token == "path" => return Err(Fault::engine()),
        Err(fault) => return Err(fault),
    };
    let sha256 = sha256_hex(&bytes);
    if sha256 != expected {
        return Err(Fault::engine());
    }
    Ok(sha256)
}

pub fn aggregate(
    cli: &Path,
    columns: &Columns,
    groups: &[Group],
    cancel: Option<&Path>,
) -> Result<(), Fault> {
    let temp = TempDir::new()?;
    let result = aggregate_in(cli, &temp.0, columns, groups, cancel);
    drop(temp);
    result
}

pub fn prove_native_cancel(cli: &Path, cancel: &Path) -> Result<(), Fault> {
    require_cli(cli)?;
    let temp = TempDir::new()?;
    let sql = slow_sql();
    let result = run_sql(cli, &temp.0, &sql, Some(cancel), CANCEL_TIMEOUT);
    drop(temp);
    match result {
        Err(fault) if fault.token == "canceled" => Ok(()),
        Ok(_) => Err(Fault::engine()),
        Err(fault) => Err(fault),
    }
}

fn aggregate_in(
    cli: &Path,
    dir: &Path,
    columns: &Columns,
    groups: &[Group],
    cancel: Option<&Path>,
) -> Result<(), Fault> {
    let mut file = File::create(dir.join("in.parquet")).map_err(|_| Fault::engine())?;
    file.write_all(&columns.parquet).map_err(|_| Fault::engine())?;
    file.sync_all().map_err(|_| Fault::engine())?;
    let sql = aggregate_sql(dir)?;
    let stdout = run_sql(cli, dir, &sql, cancel, AGGREGATE_TIMEOUT)?;
    let rows = parse_rows(&stdout)?;
    if !same_groups(&rows, groups) {
        return Err(Fault::engine());
    }
    Ok(())
}

fn aggregate_sql(dir: &Path) -> Result<String, Fault> {
    let text = dir.to_str().filter(|text| path_is_safe(text)).ok_or(Fault::engine())?;
    Ok(format!(
        "SET threads=1;\n\
SET memory_limit='256MB';\n\
SET autoinstall_known_extensions=false;\n\
SET autoload_known_extensions=false;\n\
SET allowed_directories=['{text}'];\n\
SET enable_external_access=false;\n\
SET lock_configuration=true;\n\
SELECT group_key, CAST(SUM(coefficient) AS VARCHAR) AS amount\n\
FROM read_parquet('{text}/in.parquet')\n\
GROUP BY group_key\n\
ORDER BY MIN(ordinal);"
    ))
}

fn slow_sql() -> String {
    "SET threads=1;\n\
SET memory_limit='512MB';\n\
SET autoinstall_known_extensions=false;\n\
SET autoload_known_extensions=false;\n\
SET enable_external_access=false;\n\
SELECT count(*) FROM range(250000) a, range(250000) b;"
        .to_string()
}

fn path_is_safe(text: &str) -> bool {
    !text.is_empty()
        && !text.contains('\'')
        && !text.contains('\n')
        && !text.contains('\r')
        && !text.contains('\\')
}

fn run_sql(
    cli: &Path,
    dir: &Path,
    sql: &str,
    cancel: Option<&Path>,
    timeout: Duration,
) -> Result<Vec<u8>, Fault> {
    let mut child = Command::new(cli)
        .env_clear()
        .env("HOME", "/tmp")
        .current_dir(dir)
        .arg("-bail")
        .arg("-batch")
        .arg("-json")
        .arg("-c")
        .arg(sql)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Fault::engine())?;
    let mut stdout = child.stdout.take().ok_or(Fault::engine())?;
    let reader: JoinHandle<std::io::Result<Vec<u8>>> = thread::spawn(move || {
        let mut buf = Vec::new();
        stdout.read_to_end(&mut buf)?;
        Ok(buf)
    });
    supervise(&mut child, cancel, timeout, reader)
}

fn supervise(
    child: &mut std::process::Child,
    cancel: Option<&Path>,
    timeout: Duration,
    reader: JoinHandle<std::io::Result<Vec<u8>>>,
) -> Result<Vec<u8>, Fault> {
    let started = Instant::now();
    loop {
        if let Some(path) = cancel {
            match path::cancel_requested(path) {
                Ok(true) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader.join();
                    return Err(Fault::canceled());
                }
                Ok(false) => {}
                Err(fault) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader.join();
                    return Err(fault);
                }
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                let bytes = reader.join().map_err(|_| Fault::engine())?.map_err(|_| Fault::engine())?;
                if status.success() {
                    return Ok(bytes);
                }
                return Err(Fault::engine());
            }
            Ok(None) => {
                if started.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader.join();
                    return Err(Fault::budget());
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = reader.join();
                return Err(Fault::engine());
            }
        }
    }
}

fn parse_rows(bytes: &[u8]) -> Result<Vec<(String, String)>, Fault> {
    let text = std::str::from_utf8(bytes).map_err(|_| Fault::engine())?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let value = json::parse_json(trimmed).map_err(|_| Fault::engine())?;
    let items = match value {
        Json::Array(items) => items,
        _ => return Err(Fault::engine()),
    };
    let mut rows = Vec::with_capacity(items.len());
    for item in items {
        let mut fields = json::object(item).map_err(|()| Fault::engine())?;
        let key = take_string(&mut fields, "group_key")?;
        let amount = take_string(&mut fields, "amount")?;
        json::finish(&fields).map_err(|()| Fault::engine())?;
        rows.push((key, amount));
    }
    Ok(rows)
}

fn take_string(fields: &mut Vec<(String, Json)>, key: &str) -> Result<String, Fault> {
    let value = json::take(fields, key).map_err(|()| Fault::engine())?;
    json::as_str(&value).map(str::to_string).map_err(|()| Fault::engine())
}

fn same_groups(rows: &[(String, String)], groups: &[Group]) -> bool {
    if rows.len() != groups.len() {
        return false;
    }
    rows.iter().zip(groups).all(|(row, group)| row.0 == group.key && same_number(&row.1, &group.value))
}

fn same_number(text: &str, value: &Decimal) -> bool {
    if text == value.text {
        return true;
    }
    let Some(parsed) = parse_decimal(text) else {
        return false;
    };
    aligned_eq(&parsed, value)
}

fn parse_decimal(text: &str) -> Option<Decimal> {
    if text.is_empty() || text.contains(['e', 'E', '+']) {
        return None;
    }
    let (negative, rest) = if let Some(rest) = text.strip_prefix('-') {
        (true, rest)
    } else {
        (false, text)
    };
    if rest.is_empty() || rest.starts_with('.') || rest.ends_with('.') {
        return None;
    }
    let (whole, frac) = match rest.split_once('.') {
        Some((whole, frac)) => (whole, frac),
        None => (rest, ""),
    };
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !frac.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let mut coefficient = format!("{whole}{frac}");
    let scale = i32::try_from(frac.len()).ok()?;
    coefficient = coefficient.trim_start_matches('0').to_string();
    if coefficient.is_empty() {
        coefficient.push('0');
    }
    let negative = negative && coefficient != "0";
    Some(Decimal {
        negative,
        coefficient,
        scale,
        text: text.to_string(),
    })
}

fn aligned_eq(left: &Decimal, right: &Decimal) -> bool {
    let scale = left.scale.max(right.scale);
    match (
        pad(&left.coefficient, left.scale, scale),
        pad(&right.coefficient, right.scale, scale),
    ) {
        (Some(left_digits), Some(right_digits)) => {
            left_digits == right_digits && left.negative == right.negative
        }
        _ => false,
    }
}

fn pad(coefficient: &str, from: i32, to: i32) -> Option<String> {
    if to < from {
        return None;
    }
    let mut out = coefficient.to_string();
    out.extend(std::iter::repeat_n('0', usize::try_from(to - from).ok()?));
    Some(out)
}

fn manifest_field(key: &str) -> Option<String> {
    let pattern = format!("\"{key}\"");
    let index = MANIFEST.find(&pattern)?;
    let rest = MANIFEST[index + pattern.len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, Fault> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Fault::io())?
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("oo-a386-duck-{nanos}-{}", std::process::id()));
        fs::create_dir(&dir).map_err(|_| Fault::io())?;
        Ok(Self(dir))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
