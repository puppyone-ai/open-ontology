//! Bind one exact query-pg audit file into oo.local-analysis-lineage/v1.
//! The receipt body is the file bytes. The digest is the caller-supplied
//! output of tools/sha256.rs. This program does not reimplement SHA-256.
//! local-analysis recomputes the digest and refuses a mismatch.
#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let (Some(audit), Some(digest), Some(out)) = (args.next(), args.next(), args.next()) else {
        eprintln!("usage: lineage-file AUDIT SHA256 OUT");
        return ExitCode::from(2);
    };
    if args.next().is_some() {
        eprintln!("usage: lineage-file AUDIT SHA256 OUT");
        return ExitCode::from(2);
    }
    if digest.len() != 64 || !digest.bytes().all(is_lower_hex) {
        eprintln!("lineage-file: digest");
        return ExitCode::from(2);
    }
    let bytes = match fs::read(&audit) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("lineage-file: read {err}");
            return ExitCode::from(1);
        }
    };
    if bytes.is_empty() || bytes.contains(&b'\r') {
        eprintln!("lineage-file: body");
        return ExitCode::from(2);
    }
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        Err(_) => {
            eprintln!("lineage-file: utf8");
            return ExitCode::from(2);
        }
    };
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    if lines.len() != 2 || lines.iter().any(|line| line.is_empty()) {
        eprintln!("lineage-file: lines");
        return ExitCode::from(2);
    }
    let Some(first) = fingerprint(lines[0]) else {
        eprintln!("lineage-file: fingerprint");
        return ExitCode::from(2);
    };
    let Some(second) = fingerprint(lines[1]) else {
        eprintln!("lineage-file: fingerprint");
        return ExitCode::from(2);
    };
    if first != second || !(6..=40).contains(&first.len()) || !first.bytes().all(is_lower_hex) {
        eprintln!("lineage-file: fingerprint");
        return ExitCode::from(2);
    }
    let query_id = format!("q_{first}");
    let mut json = String::new();
    json.push_str(
        "{\"version\":\"oo.local-analysis-lineage/v1\",\"trust\":\"local_operator_declared\",\"query_id\":",
    );
    push_string(&mut json, &query_id);
    json.push_str(",\"receipt_sha256\":");
    push_string(&mut json, &digest);
    json.push_str(",\"receipt_body\":");
    push_string(&mut json, text);
    json.push_str("}\n");
    if let Err(err) = fs::write(&out, json.as_bytes()) {
        eprintln!("lineage-file: write {err}");
        return ExitCode::from(1);
    }
    println!("query_id={query_id}");
    ExitCode::SUCCESS
}

fn fingerprint(line: &str) -> Option<&str> {
    let key = "\"fingerprint_hex\":\"";
    let start = line.find(key)? + key.len();
    let rest = line.get(start..)?;
    let end = rest.find('"')?;
    let value = rest.get(..end)?;
    if value.is_empty() { None } else { Some(value) }
}

fn push_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                let code = ch as u32;
                out.push_str(&format!("\\u{code:04x}"));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}
