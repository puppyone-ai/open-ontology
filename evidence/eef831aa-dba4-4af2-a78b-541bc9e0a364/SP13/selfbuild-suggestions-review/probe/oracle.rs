//! Compare the captured query-pg bytes with the literal gold.
//! This program does not invent a receipt. It only reads files the run wrote.

use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    let root = env::args().nth(1).unwrap_or_else(|| "/out".to_string());
    match check(&root) {
        Ok(()) => {
            println!("oracle ok");
            ExitCode::SUCCESS
        }
        Err(reason) => {
            eprintln!("oracle: {reason}");
            ExitCode::from(1)
        }
    }
}

fn check(root: &str) -> Result<(), String> {
    let select_sql = fs::read(format!("{root}/gold/select.sql")).map_err(|err| err.to_string())?;
    let probe_sql = fs::read(format!("{root}/gold/probe.sql")).map_err(|err| err.to_string())?;
    let row_line = fs::read(format!("{root}/gold/row-line.txt")).map_err(|err| err.to_string())?;
    let select_sql = std::str::from_utf8(&select_sql).map_err(|_| "select sql utf8")?;
    let probe_sql = std::str::from_utf8(&probe_sql).map_err(|_| "probe sql utf8")?;
    let row_line = std::str::from_utf8(&row_line).map_err(|_| "row line utf8")?;
    if select_sql != "SELECT party_key FROM shop.account WHERE party_key IS NOT NULL LIMIT 1" {
        return Err("select gold drifted".to_string());
    }
    if probe_sql != "UPDATE shop.account SET label = label" {
        return Err("probe gold drifted".to_string());
    }
    if row_line
        != "{\"record\":\"row\",\"turn\":0,\"cells\":[{\"kind\":\"text\",\"value\":\"a364-mapped-row\"}]}"
    {
        return Err("row gold drifted".to_string());
    }
    for clause in [
        "SELECT party_key",
        "FROM shop.account",
        "WHERE party_key IS NOT NULL",
        "LIMIT 1",
    ] {
        if !select_sql.contains(clause) {
            return Err(format!("gold sql missing {clause}"));
        }
    }

    let hash_line = fs::read_to_string(format!("{root}/gold/select.sha256"))
        .map_err(|err| err.to_string())?;
    let hash = hash_line
        .split_whitespace()
        .next()
        .ok_or("empty select hash")?
        .to_string();
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("select hash is not 64 hex".to_string());
    }

    let probe_exit = fs::read_to_string(format!("{root}/capture/request-1.exit"))
        .map_err(|_| "missing probe exit")?;
    if probe_exit != "6\n" {
        return Err(format!("probe exit {probe_exit:?}"));
    }
    let probe_stdout = fs::read(format!("{root}/capture/request-1.stdout"))
        .map_err(|_| "missing probe stdout")?;
    if !probe_stdout.is_empty() {
        return Err("probe stdout was not empty".to_string());
    }
    if fs::metadata(format!("{root}/capture/request-1.audit.jsonl")).is_ok() {
        return Err("probe wrote an audit receipt".to_string());
    }
    let probe_request = fs::read_to_string(format!("{root}/capture/request-1.request.json"))
        .map_err(|_| "missing probe request")?;
    if !probe_request.contains(probe_sql) || probe_request.contains(select_sql) {
        return Err("probe request sql".to_string());
    }

    let select_exit = fs::read_to_string(format!("{root}/capture/request-2.exit"))
        .map_err(|_| "missing select exit")?;
    if select_exit != "0\n" {
        return Err(format!("select exit {select_exit:?}"));
    }
    let select_stdout = fs::read_to_string(format!("{root}/capture/request-2.stdout"))
        .map_err(|_| "missing select stdout")?;
    if !select_stdout.lines().any(|line| line == row_line) {
        return Err("typed row line missing".to_string());
    }
    if !select_stdout.contains("\"record\":\"summary\"")
        || !select_stdout.contains("\"status\":\"succeeded\"")
        || !select_stdout.contains("\"returned_rows\":1")
        || !select_stdout.contains("\"truncated\":false")
    {
        return Err("select summary fields".to_string());
    }
    let stdout_original = every_hex(&select_stdout, "original_sha256")?;
    if stdout_original != hash {
        return Err(format!("stdout original {stdout_original} != {hash}"));
    }
    let stdout_executed = every_hex(&select_stdout, "executed_sha256")?;
    let select_request = fs::read_to_string(format!("{root}/capture/request-2.request.json"))
        .map_err(|_| "missing select request")?;
    if !select_request.contains(select_sql) {
        return Err("select request is not the full statement".to_string());
    }
    for clause in [
        "SELECT party_key",
        "FROM shop.account",
        "WHERE party_key IS NOT NULL",
        "LIMIT 1",
    ] {
        if !select_request.contains(clause) {
            return Err(format!("request missing {clause}"));
        }
    }

    let audit = fs::read_to_string(format!("{root}/capture/request-2.audit.jsonl"))
        .map_err(|_| "missing select audit receipt")?;
    if !audit.contains("\"record\":\"admit\"") || !audit.contains("\"record\":\"complete\"") {
        return Err("audit records".to_string());
    }
    if !audit.contains("\"status\":\"succeeded\"") || !audit.contains("\"returned_rows\":1") {
        return Err("audit completion".to_string());
    }
    let audit_original = every_hex(&audit, "original_sha256")?;
    if audit_original != hash {
        return Err(format!("audit original {audit_original} != {hash}"));
    }
    let audit_executed = every_hex(&audit, "executed_sha256")?;
    if audit_executed != stdout_executed {
        return Err("summary and audit executed hashes differ".to_string());
    }
    if audit.contains("BEGIN ") || audit.contains("PRIVATE KEY") {
        return Err("audit contains unexpected secret material".to_string());
    }

    if fs::metadata(format!("{root}/capture/request-3.exit")).is_ok() {
        return Err("a third query was spawned".to_string());
    }

    let outcome = fs::read_to_string(format!("{root}/outcome.json")).map_err(|_| "missing outcome")?;
    for needle in [
        "\"probe\":\"refused\"",
        "\"probe_exit\":6",
        "\"admitted\":1",
        "\"spawned\":2",
        "\"validation\":\"admitted\"",
        "\"validation\":\"not_run\"",
        "\"published\":false",
        "\"grants_issued\":false",
        "\"curated_written\":false",
        "\"metrics_certified\":false",
        "\"auto_merge\":false",
        "\"knowledge_verified_count\":0",
        "\"knowledge_verified\":false",
        "\"configured\":true",
    ] {
        if !outcome.contains(needle) {
            return Err(format!("outcome missing {needle}"));
        }
    }
    if outcome.contains("\"knowledge_verified\":true") {
        return Err("outcome claims knowledge_verified".to_string());
    }
    if !outcome.contains(select_sql) {
        return Err("outcome missing full select".to_string());
    }
    if count(&outcome, "\"validation\":\"admitted\"") != 1
        || count(&outcome, "\"validation\":\"not_run\"") != 1
        || count(&outcome, "\"gateway_admitted\":true") != 1
    {
        return Err("partial validation counts".to_string());
    }

    let apply = fs::read_to_string(format!("{root}/apply.txt")).map_err(|_| "missing apply record")?;
    if !apply.contains("mapped_rows=1\n") && !apply.contains("mapped_rows=1\r") {
        if !apply.lines().any(|line| line == "mapped_rows=1") {
            return Err("mapped row count is not 1".to_string());
        }
    }
    if !apply.contains("mapped_value=a364-mapped-row") {
        return Err("mapped value record".to_string());
    }
    if apply.contains("mapped_rows=100000") {
        return Err("apply loaded 100000 rows".to_string());
    }
    Ok(())
}

fn every_hex(text: &str, key: &str) -> Result<String, String> {
    let pattern = format!("\"{key}\":\"");
    let mut found: Option<String> = None;
    let mut rest = text;
    let mut count = 0_usize;
    while let Some(index) = rest.find(&pattern) {
        let start = index + pattern.len();
        let bytes = rest.as_bytes();
        if start + 64 > bytes.len() {
            return Err(format!("{key} truncated"));
        }
        let value = rest[start..start + 64].to_string();
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(format!("{key} is not lowercase hex"));
        }
        if let Some(previous) = &found {
            if previous != &value {
                return Err(format!("{key} changed inside one document"));
            }
        }
        found = Some(value);
        count += 1;
        rest = &rest[start + 64..];
    }
    found
        .filter(|_| count >= 1)
        .ok_or_else(|| format!("missing {key}"))
}

fn count(text: &str, needle: &str) -> usize {
    text.matches(needle).count()
}
