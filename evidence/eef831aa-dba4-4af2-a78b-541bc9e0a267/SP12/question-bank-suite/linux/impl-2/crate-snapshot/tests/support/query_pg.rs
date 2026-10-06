//! Test double for query-pg. The suite compiler does not spawn this file.
//! sql-runner does, with argv `--request <path>`, a cleared environment, and
//! the trial directory as its working directory. Responses live next to this
//! executable because the working directory is not the directory that holds
//! the table.

#![forbid(unsafe_code)]

use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::process::ExitCode;

const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn main() -> ExitCode {
    match serve() {
        Ok(()) => ExitCode::from(0),
        Err(code) => ExitCode::from(code),
    }
}

fn serve() -> Result<(), u8> {
    let mut args = env::args();
    let _program = args.next();
    let flag = args.next();
    let path = args.next();
    if args.next().is_some() || flag.as_deref() != Some("--request") || path.is_none() {
        return Err(2);
    }
    if env::var_os("PGPASSWORD").is_some() {
        return Err(3);
    }
    let request = fs::read_to_string(path.expect("request path")).map_err(|_| 4_u8)?;
    let sql = json_string(&request, "sql").ok_or(5_u8)?;
    let audit = json_string(&request, "audit_file").ok_or(5_u8)?;
    let table = fs::read_to_string(responses_path()?).map_err(|_| 7_u8)?;
    let rows = find_rows(&table, sql).ok_or(8_u8)?;
    write_audit(audit)?;
    let mut stdout = io::stdout().lock();
    let mut result_bytes = 0_u64;
    for row in &rows {
        stdout.write_all(row.as_bytes()).map_err(|_| 1_u8)?;
        stdout.write_all(b"\n").map_err(|_| 1_u8)?;
        result_bytes = result_bytes
            .saturating_add(u64::try_from(row.len()).unwrap_or(u64::MAX))
            .saturating_add(1);
    }
    let summary = format!(
        "{{\"end_audit_ok\":true,\"executed_sha256\":\"{EMPTY_SHA}\",\"original_sha256\":\"{EMPTY_SHA}\",\"record\":\"summary\",\"result_bytes\":{result_bytes},\"returned_rows\":{},\"rollback_confirmed\":true,\"socket_dropped\":false,\"sqlstate\":null,\"status\":\"succeeded\",\"stop\":null,\"truncated\":false,\"turn\":0}}\n",
        rows.len()
    );
    stdout.write_all(summary.as_bytes()).map_err(|_| 1_u8)?;
    Ok(())
}

fn responses_path() -> Result<std::path::PathBuf, u8> {
    let exe = env::current_exe().map_err(|_| 7_u8)?;
    let parent = exe.parent().ok_or(7_u8)?;
    Ok(parent.join("responses.json"))
}

fn write_audit(path: &str) -> Result<(), u8> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| 6_u8)?;
    file.write_all(b"{\"protocol\":\"oo.gateway.query-pg-audit/v1\"}\n")
        .map_err(|_| 6_u8)?;
    Ok(())
}

fn json_string<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\":\"");
    let start = text.find(&needle)? + needle.len();
    let rest = text.get(start..)?;
    let end = rest.find('"')?;
    let value = rest.get(..end)?;
    if value.bytes().any(|byte| byte == b'\\' || byte < 0x20) {
        return None;
    }
    Some(value)
}

fn find_rows(table: &str, sql: &str) -> Option<Vec<String>> {
    let mut lines = table.lines();
    if lines.next()? != "OOQRESP1" {
        return None;
    }
    let count: usize = lines.next()?.parse().ok()?;
    for _ in 0..count {
        let found = lines.next()?;
        let rows: usize = lines.next()?.parse().ok()?;
        let mut body = Vec::with_capacity(rows);
        for _ in 0..rows {
            body.push(lines.next()?.to_owned());
        }
        if found == sql {
            return Some(body);
        }
    }
    None
}
