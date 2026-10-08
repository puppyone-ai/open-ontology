//! Compare exporter files with the hand gold written before the chain.
//! The CSV bytes are not derived from query-pg JSON. The stdout is checked
//! only as the compiled receipt: row count, truncation flag, and the
//! coefficient literals the producer must have emitted.

use std::fs;
use std::process::{Command, ExitCode};

const SQL: &str = "SELECT entry_id, amount, wide FROM shop.entry ORDER BY entry_id";
const AMOUNT: &str = "123456789012345678";
const WIDE: &str = "12345678901234567890123456789012345678";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        eprintln!("usage: chain-oracle partial-request SRC DST | check KIND STDOUT CSV SIDECAR GOLD");
        return ExitCode::from(2);
    };
    match command.as_str() {
        "partial-request" => {
            let Some(src) = args.next() else {
                return usage();
            };
            let Some(dst) = args.next() else {
                return usage();
            };
            partial_request(&src, &dst)
        }
        "check" => {
            let Some(kind) = args.next() else {
                return usage();
            };
            let Some(stdout_path) = args.next() else {
                return usage();
            };
            let Some(csv_path) = args.next() else {
                return usage();
            };
            let Some(sidecar_path) = args.next() else {
                return usage();
            };
            let Some(gold_path) = args.next() else {
                return usage();
            };
            check(&kind, &stdout_path, &csv_path, &sidecar_path, &gold_path)
        }
        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: chain-oracle partial-request SRC DST | check KIND STDOUT CSV SIDECAR GOLD");
    ExitCode::from(2)
}

fn partial_request(src: &str, dst: &str) -> ExitCode {
    let bytes = match fs::read(src) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("read request: {err}");
            return ExitCode::from(1);
        }
    };
    let Ok(text) = String::from_utf8(bytes) else {
        eprintln!("request is not utf-8");
        return ExitCode::from(1);
    };
    if text.matches(SQL).count() != 1 {
        eprintln!("approved sql is not the single statement");
        return ExitCode::from(1);
    }
    if text.matches("\"max_rows\":20").count() != 1 {
        eprintln!("max_rows 20 is not unique");
        return ExitCode::from(1);
    }
    if text.matches("audits/amounts.jsonl").count() != 1 {
        eprintln!("amounts audit path is not unique");
        return ExitCode::from(1);
    }
    let text = text.replacen("\"max_rows\":20", "\"max_rows\":1", 1);
    let text = text.replacen("audits/amounts.jsonl", "audits/partial.jsonl", 1);
    if text.contains("\"max_rows\":20") || text.matches(SQL).count() != 1 {
        eprintln!("partial request drifted");
        return ExitCode::from(1);
    }
    if fs::write(dst, text).is_err() {
        eprintln!("write partial request");
        return ExitCode::from(1);
    }
    println!("partial_request=1");
    ExitCode::SUCCESS
}

fn check(kind: &str, stdout_path: &str, csv_path: &str, sidecar_path: &str, gold_path: &str) -> ExitCode {
    let stdout = match fs::read(stdout_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("read stdout: {err}");
            return ExitCode::from(1);
        }
    };
    let csv = match fs::read(csv_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("read csv: {err}");
            return ExitCode::from(1);
        }
    };
    let gold = match fs::read(gold_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("read gold: {err}");
            return ExitCode::from(1);
        }
    };
    let sidecar = match fs::read(sidecar_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("read sidecar: {err}");
            return ExitCode::from(1);
        }
    };
    if csv != gold {
        eprintln!("csv does not match hand gold kind={kind} csv={} gold={}", csv.len(), gold.len());
        return ExitCode::from(1);
    }
    let Ok(side) = std::str::from_utf8(&sidecar) else {
        eprintln!("sidecar is not utf-8");
        return ExitCode::from(1);
    };
    let Ok(source) = std::str::from_utf8(&stdout) else {
        eprintln!("stdout is not utf-8");
        return ExitCode::from(1);
    };
    let source_copy = csv_path.replace("export.csv", "export.source.jsonl");
    let copied = match fs::read(&source_copy) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("read source copy: {err}");
            return ExitCode::from(1);
        }
    };
    if copied != stdout {
        eprintln!("source copy is not the compiled stdout");
        return ExitCode::from(1);
    }
    if !source.contains(&format!("\"coefficient\":\"{WIDE}\"")) || !source.contains(&format!("\"coefficient\":\"{AMOUNT}\"")) {
        eprintln!("compiled stdout is missing the hand coefficients");
        return ExitCode::from(1);
    }
    if !source.contains("\"kind\":\"decimal\"") {
        eprintln!("compiled stdout is missing a decimal cell");
        return ExitCode::from(1);
    }
    if kind != "partial" && !source.contains("\"negative\":true") {
        eprintln!("compiled stdout is missing a negative decimal cell");
        return ExitCode::from(1);
    }
    let rows = source.matches("\"record\":\"row\"").count();
    let (expect_rows, truncated) = match kind {
        "complete" | "formula" => (6_usize, false),
        "partial" => (1_usize, true),
        _ => {
            eprintln!("unknown kind");
            return ExitCode::from(2);
        }
    };
    if rows != expect_rows {
        eprintln!("stdout row count {rows} kind={kind}");
        return ExitCode::from(1);
    }
    let truncated_token = if truncated { "\"truncated\":true" } else { "\"truncated\":false" };
    if !source.contains(truncated_token) || !source.contains("\"record\":\"summary\"") {
        eprintln!("stdout summary flag mismatch");
        return ExitCode::from(1);
    }
    if kind != "partial" && !source.contains("\"kind\":\"null\"") {
        eprintln!("compiled stdout is missing the null cell");
        return ExitCode::from(1);
    }
    if kind != "partial" && !source.contains("\"coefficient\":\"0\"") {
        eprintln!("compiled stdout is missing the zero coefficient");
        return ExitCode::from(1);
    }
    let need = match kind {
        "complete" => complete_need(),
        "partial" => partial_need(),
        "formula" => formula_need(),
        _ => return ExitCode::from(2),
    };
    for needle in &need {
        if !side.contains(needle.as_str()) {
            eprintln!("sidecar missing {needle}");
            return ExitCode::from(1);
        }
    }
    if kind == "complete" && csv.contains(&b'\'') {
        eprintln!("complete csv contains an apostrophe");
        return ExitCode::from(1);
    }
    if kind == "formula" && csv.iter().filter(|byte| **byte == b'\'').count() != 1 {
        eprintln!("formula csv apostrophe count is not one header prefix");
        return ExitCode::from(1);
    }
    if kind == "formula" && (csv.windows(5).any(|window| window == b"'-123") || csv.windows(4).any(|window| window == b"'-0.")) {
        eprintln!("formula prefix landed on a typed decimal");
        return ExitCode::from(1);
    }
    let source_hash = match json_string(side, "source_sha256") {
        Some(value) => value,
        None => {
            eprintln!("source_sha256 missing");
            return ExitCode::from(1);
        }
    };
    let csv_hash = match json_string(side, "csv_sha256") {
        Some(value) => value,
        None => {
            eprintln!("csv_sha256 missing");
            return ExitCode::from(1);
        }
    };
    if !hex64(&source_hash) || !hex64(&csv_hash) {
        eprintln!("hash field is not 64 hex digits");
        return ExitCode::from(1);
    }
    let hashed_source = match file_sha256(stdout_path) {
        Some(value) => value,
        None => return ExitCode::from(1),
    };
    let hashed_csv = match file_sha256(csv_path) {
        Some(value) => value,
        None => return ExitCode::from(1),
    };
    if hashed_source != source_hash || hashed_csv != csv_hash {
        eprintln!("sidecar hash does not match the file bytes");
        return ExitCode::from(1);
    }
    println!("gold_match=1 kind={kind}");
    ExitCode::SUCCESS
}

fn complete_need() -> Vec<String> {
    vec![
        "\"complete\":true,\"partial\":false".to_string(),
        "\"status\":\"succeeded\",\"truncated\":false".to_string(),
        "\"returned_rows\":6,\"result_bytes\":162".to_string(),
        "\"parsed_rows\":6,\"parsed_result_bytes\":162".to_string(),
        "\"formula_prefixed\":0,\"formula_cells\":[]".to_string(),
        "\"null_count\":2".to_string(),
        "\"null_cells\":[[5,1],[5,2]]".to_string(),
        "\"int_count\":6".to_string(),
        int_cells(6),
        "\"decimal_count\":10".to_string(),
        decimal_cells(10),
        columns("wide", 1),
        "\"xlsx\":\"unsupported\"".to_string(),
        "\"http_auth\":false".to_string(),
        "\"end_audit_ok\":true".to_string(),
        "\"rollback_confirmed\":true".to_string(),
        "\"socket_dropped\":false".to_string(),
        "\"stop\":null".to_string(),
        "\"sqlstate\":null".to_string(),
    ]
}

fn partial_need() -> Vec<String> {
    vec![
        "\"complete\":false,\"partial\":true".to_string(),
        "\"status\":\"succeeded\",\"truncated\":true".to_string(),
        "\"returned_rows\":1,\"result_bytes\":59".to_string(),
        "\"parsed_rows\":1,\"parsed_result_bytes\":59".to_string(),
        "\"formula_prefixed\":0,\"formula_cells\":[]".to_string(),
        "\"null_count\":0".to_string(),
        "\"null_cells\":[]".to_string(),
        "\"int_count\":1".to_string(),
        int_cells(1),
        "\"decimal_count\":2".to_string(),
        decimal_cells(2),
        columns("wide", 0),
        "\"stop\":null".to_string(),
        "\"sqlstate\":null".to_string(),
    ]
}

fn formula_need() -> Vec<String> {
    let mut need = complete_need();
    need.retain(|item| !item.contains("\"name\":\"wide\""));
    need.push(columns("-wide", 1));
    need
}

fn columns(wide_name: &str, nulls: u32) -> String {
    format!(
        "[{{\"name\":\"entry_id\",\"kind\":\"int\",\"nulls\":0,\"empty_text\":0}},{{\"name\":\"amount\",\"kind\":\"decimal\",\"nulls\":{nulls},\"empty_text\":0}},{{\"name\":\"{wide_name}\",\"kind\":\"decimal\",\"nulls\":{nulls},\"empty_text\":0}}]"
    )
}

fn int_cells(count: usize) -> String {
    let mut out = String::from("[");
    for index in 0..count {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"row\":{index},\"column\":0,\"text\":\"{}\"}}",
            index + 1
        ));
    }
    out.push(']');
    out
}

fn decimal_cells(count: usize) -> String {
    let cells = [
        cell(0, 1, false, AMOUNT, "12345678901234.5678"),
        cell(0, 2, false, WIDE, "1234567890123456789012345678901234.5678"),
        cell(1, 1, true, AMOUNT, "-12345678901234.5678"),
        cell(1, 2, true, WIDE, "-1234567890123456789012345678901234.5678"),
        cell(2, 1, false, "1", "0.0001"),
        cell(2, 2, false, "1", "0.0001"),
        cell(3, 1, true, "1", "-0.0001"),
        cell(3, 2, true, "1", "-0.0001"),
        cell(4, 1, false, "0", "0.0000"),
        cell(4, 2, false, "0", "0.0000"),
    ];
    let mut out = String::from("[");
    for (index, cell) in cells.iter().take(count).enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(cell);
    }
    out.push(']');
    out
}

fn cell(row: u32, column: u32, negative: bool, coefficient: &str, text: &str) -> String {
    let flag = if negative { "true" } else { "false" };
    format!(
        "{{\"row\":{row},\"column\":{column},\"negative\":{flag},\"coefficient\":\"{coefficient}\",\"scale\":4,\"text\":\"{text}\"}}"
    )
}

fn json_string<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let pattern = format!("\"{key}\":\"");
    let start = text.find(&pattern)? + pattern.len();
    let rest = text.get(start..)?;
    let end = rest.find('"')?;
    Some(&rest[..end])
}

fn hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn file_sha256(path: &str) -> Option<String> {
    let output = Command::new("/cargo-home/bin/sha256").args(["file", path]).output().ok()?;
    if !output.status.success() {
        eprintln!("sha256 file failed");
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    text.split_whitespace().next().map(str::to_string)
}
