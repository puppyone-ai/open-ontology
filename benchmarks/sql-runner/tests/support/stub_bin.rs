//! Stand-in for query-pg, the adapter, and the comparator.
//!
//! The test binary compiles this file with rustc and hard-links the three
//! names. It is not the product. It never reads a password from the environment.

#![forbid(unsafe_code)]

use std::io::{Read, Write};

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
    let mut args = std::env::args().skip(1);
    let flag = args.next().unwrap_or_default();
    let path = args.next().unwrap_or_default();
    if flag != "--request" || path.is_empty() || args.next().is_some() {
        fail(2, "query-pg: usage\n");
    }
    if std::env::var_os("PGPASSWORD").is_some() {
        fail(3, "query-pg: request\n");
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) => fail(3, "query-pg: request\n"),
    };
    let kind = marker(&text);
    if kind != "HIDE" {
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
    }
    match kind {
        "HIDE" => fail(6, "query-pg: guard\n"),
        "SLOW" => {
            std::thread::sleep(std::time::Duration::from_secs(20));
            emit(&summary("succeeded", false, true, "null"));
            std::process::exit(0);
        }
        "FLOOD" => {
            let buf = vec![b'a'; 1_048_577];
            let _ = std::io::stdout().write_all(&buf);
            let _ = std::io::stdout().flush();
            std::process::exit(0);
        }
        "NOEND" => {
            emit(&row("full"));
            let _ = std::io::stderr().write_all(b"query-pg: query\n");
            std::process::exit(8);
        }
        "TRUNC" => {
            emit(&row("prefix"));
            emit(&summary("succeeded", true, true, "null"));
            std::process::exit(0);
        }
        "PARTIAL" => {
            emit(&summary("partial", false, true, "null"));
            let _ = std::io::stderr().write_all(b"query-pg: query\n");
            std::process::exit(8);
        }
        "AUDITFALSE" => {
            emit(&summary("succeeded", false, false, "null"));
            let _ = std::io::stderr().write_all(b"query-pg: audit\n");
            std::process::exit(7);
        }
        "MATCH" => {
            emit(&row("full"));
            emit(&summary("succeeded", false, true, "null"));
            std::process::exit(0);
        }
        _ => fail(3, "query-pg: request\n"),
    }
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
    let value = if text.contains("\"truncated\":true") {
        "prefix"
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
    if text.contains("\"row_comparator\":\"set\"") {
        fail(2, "result-comparator: schema /row_comparator\n");
    }
    if text.contains("GOLD-CANARY-a231-9f3c") {
        emit(
            "{\"differences\":[{\"actual\":\"CANARY-CELL-other\",\"expected\":\"GOLD-CANARY-a231-9f3c\",\"path\":\"rows/0/0\",\"reason\":\"value\"}],\"verdict\":\"mismatch\",\"version\":\"oo.benchmark.result-comparison-report/v1\"}\n",
        );
        std::process::exit(1);
    }
    emit(
        "{\"differences\":[],\"verdict\":\"match\",\"version\":\"oo.benchmark.result-comparison-report/v1\"}\n",
    );
    std::process::exit(0);
}

fn marker(text: &str) -> &'static str {
    if text.contains("A231-HIDE") {
        "HIDE"
    } else if text.contains("A231-SLOW") {
        "SLOW"
    } else if text.contains("A231-FLOOD") {
        "FLOOD"
    } else if text.contains("A231-NOEND") {
        "NOEND"
    } else if text.contains("A231-TRUNC") {
        "TRUNC"
    } else if text.contains("A231-PARTIAL") {
        "PARTIAL"
    } else if text.contains("A231-AUDITFALSE") {
        "AUDITFALSE"
    } else if text.contains("A231-MATCH") {
        "MATCH"
    } else {
        ""
    }
}

fn field(text: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\":\"");
    let start = text.find(&pattern)? + pattern.len();
    let rest = text.get(start..)?;
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

fn row(value: &str) -> String {
    format!(
        "{{\"record\":\"row\",\"turn\":0,\"cells\":[{{\"kind\":\"text\",\"value\":\"{value}\"}}]}}\n"
    )
}

fn summary(status: &str, truncated: bool, end_ok: bool, stop_json: &str) -> String {
    let truncated = if truncated { "true" } else { "false" };
    let end_ok = if end_ok { "true" } else { "false" };
    format!(
        "{{\"record\":\"summary\",\"turn\":0,\"status\":\"{status}\",\"truncated\":{truncated},\"returned_rows\":1,\"result_bytes\":1,\"rollback_confirmed\":true,\"socket_dropped\":false,\"end_audit_ok\":{end_ok},\"original_sha256\":\"{HASH}\",\"executed_sha256\":\"{HASH}\",\"stop\":{stop_json},\"sqlstate\":null}}\n"
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
