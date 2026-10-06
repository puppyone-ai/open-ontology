//! Review fixture for query-pg, the result adapter, and the comparator.
//!
//! Compiled with rustc. The three names are hard links. This file is not the
//! product under review. It never reads a password and it never opens a socket.

#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;

const HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn main() {
    let exe = std::env::args().next().unwrap_or_default();
    let name = std::path::Path::new(&exe)
        .file_name()
        .and_then(|text| text.to_str())
        .unwrap_or("");
    match name {
        "query-pg" => query(),
        "query-result-adapter" => adapter(),
        "result-comparator" => comparator(),
        _ => fail(2, "stub: usage\n"),
    }
}

fn query() -> ! {
    if std::env::var_os("PGPASSWORD").is_some() {
        fail(3, "query-pg: request\n");
    }
    let mut args = std::env::args().skip(1);
    let flag = args.next().unwrap_or_default();
    let path = args.next().unwrap_or_default();
    if flag != "--request" || path.is_empty() || args.next().is_some() {
        fail(2, "query-pg: usage\n");
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) => fail(3, "query-pg: request\n"),
    };
    let kind = marker(&text);
    let index = trial_index(&text);
    if refuses(kind, index) {
        fail(6, "query-pg: guard\n");
    }
    let audit = match field(&text, "audit_file") {
        Some(path) => path,
        None => fail(3, "query-pg: request\n"),
    };
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&audit)
    {
        Ok(_) => {}
        Err(_) => fail(7, "query-pg: audit\n"),
    }
    match (kind, index) {
        ("SLOW", _) | ("GATE", 1) | ("QUAD", 0) => {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            succeed();
        }
        ("SPLIT", 1) => miss("A255-MISS"),
        ("QUAD", 2) => {
            emit(&row("A255-MISS"));
            emit(&summary());
            disarm();
            std::process::exit(0);
        }
        ("DISARM", _) => {
            succeed_body();
            disarm();
            std::process::exit(0);
        }
        ("PASS", _) | ("NOTE", _) | ("SPLIT", 0) | ("GATE", _) | ("QUAD", _) => succeed(),
        _ => fail(3, "query-pg: request\n"),
    }
}

fn refuses(kind: &str, index: u8) -> bool {
    matches!((kind, index), ("GATE", 0) | ("QUAD", 1))
}

fn adapter() -> ! {
    if std::env::args().len() != 1 {
        fail(2, "query-result-adapter: usage\n");
    }
    let mut buf = Vec::new();
    if std::io::stdin().read_to_end(&mut buf).is_err() || buf.is_empty() {
        fail(2, "query-result-adapter: protocol /1\n");
    }
    let text = String::from_utf8_lossy(&buf);
    let value = if text.contains("A255-MISS") {
        "GOLD-MISS"
    } else {
        "full"
    };
    let line = format!(
        "{{\"columns\":[{{\"name\":\"note\",\"type\":\"text\"}}],\"completion\":\"complete\",\"kind\":\"rows\",\"rows\":[[{{\"presence\":\"value\",\"value\":\"{value}\"}}]],\"warnings\":[]}}\n"
    );
    emit(&line);
    std::process::exit(0);
}

fn comparator() -> ! {
    if std::env::args().len() != 1 {
        fail(2, "result-comparator: usage\n");
    }
    let mut buf = Vec::new();
    if std::io::stdin().read_to_end(&mut buf).is_err() {
        fail(2, "result-comparator: stdin\n");
    }
    let text = String::from_utf8_lossy(&buf);
    if !text.contains("oo.benchmark.result-comparison/v1") {
        fail(2, "result-comparator: schema /version\n");
    }
    if text.contains("GOLD-MISS") {
        emit(
            "{\"differences\":[{\"path\":\"rows/0/0\",\"reason\":\"value\"}],\"verdict\":\"mismatch\",\"version\":\"oo.benchmark.result-comparison-report/v1\"}\n",
        );
        std::process::exit(1);
    }
    emit(
        "{\"differences\":[],\"verdict\":\"match\",\"version\":\"oo.benchmark.result-comparison-report/v1\"}\n",
    );
    std::process::exit(0);
}

fn disarm() {
    let exe = std::env::args().next().unwrap_or_default();
    let path = std::path::Path::new(&exe);
    if path.file_name().and_then(|text| text.to_str()) != Some("query-pg") {
        fail(9, "query-pg: guard\n");
    }
    if std::fs::remove_file(path).is_err() {
        fail(9, "query-pg: guard\n");
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true).mode(0o644);
    if options.open(path).is_err() {
        fail(9, "query-pg: guard\n");
    }
}

fn marker(text: &str) -> &'static str {
    if text.contains("A255-DISARM") {
        "DISARM"
    } else if text.contains("A255-SPLIT") {
        "SPLIT"
    } else if text.contains("A255-GATE") {
        "GATE"
    } else if text.contains("A255-SLOW") {
        "SLOW"
    } else if text.contains("A255-NOTE") {
        "NOTE"
    } else if text.contains("A255-QUAD") {
        "QUAD"
    } else if text.contains("A255-PASS") {
        "PASS"
    } else {
        ""
    }
}

fn trial_index(text: &str) -> u8 {
    let Some(audit) = field(text, "audit_file") else {
        fail(3, "query-pg: request\n");
    };
    for index in 0..8 {
        let needle = format!("/trial-{index}/");
        if audit.contains(&needle) {
            return index;
        }
    }
    fail(3, "query-pg: request\n");
}

fn field(text: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\":\"");
    let start = text.find(&pattern)? + pattern.len();
    let rest = text.get(start..)?;
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

fn succeed() -> ! {
    succeed_body();
    std::process::exit(0);
}

fn succeed_body() {
    emit(&row("full"));
    emit(&summary());
}

fn miss(value: &str) -> ! {
    emit(&row(value));
    emit(&summary());
    std::process::exit(0);
}

fn row(value: &str) -> String {
    format!(
        "{{\"record\":\"row\",\"turn\":0,\"cells\":[{{\"kind\":\"text\",\"value\":\"{value}\"}}]}}\n"
    )
}

fn summary() -> String {
    format!(
        "{{\"record\":\"summary\",\"turn\":0,\"status\":\"succeeded\",\"truncated\":false,\"returned_rows\":1,\"result_bytes\":1,\"rollback_confirmed\":true,\"socket_dropped\":false,\"end_audit_ok\":true,\"original_sha256\":\"{HASH}\",\"executed_sha256\":\"{HASH}\",\"stop\":null,\"sqlstate\":null}}\n"
    )
}

fn emit(text: &str) {
    let _ = std::io::stdout().write_all(text.as_bytes());
    let _ = std::io::stdout().flush();
}

fn fail(code: i32, line: &str) -> ! {
    let _ = std::io::stderr().write_all(line.as_bytes());
    let _ = std::io::stderr().flush();
    std::process::exit(code);
}
