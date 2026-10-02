//! Reader-log audit. A nonempty statement trace is the zero-write evidence.
//! Absence of write verbs in an empty log is not a pass.

use std::fs;
use std::io::{BufRead, BufReader};

const ALLOWED: &[&str] = &["SELECT", "START", "BEGIN", "ROLLBACK", "WITH"];
const FORBIDDEN: &[&str] = &[
    "INSERT", "UPDATE", "DELETE", "MERGE", "TRUNCATE", "COPY", "CREATE", "ALTER", "DROP",
    "GRANT", "REVOKE", "COMMENT", "VACUUM", "REINDEX", "CLUSTER", "LOCK", "CALL", "DO",
    "IMPORT", "SECURITY", "RESET", "SET",
];

fn sql_of(line: &str) -> Option<&str> {
    if let Some(index) = line.find("statement:") {
        return Some(line[index + "statement:".len()..].trim());
    }
    if let Some(index) = line.find("execute ") {
        let rest = &line[index + "execute ".len()..];
        if let Some(colon) = rest.find(':') {
            return Some(rest[colon + 1..].trim());
        }
    }
    None
}

fn keyword(sql: &str) -> String {
    let word = sql.split_whitespace().next().unwrap_or("");
    word.chars()
        .take_while(|ch| ch.is_ascii_alphabetic())
        .collect::<String>()
        .to_ascii_uppercase()
}

#[test]
#[ignore = "requires the copied PostgreSQL statement log"]
fn reader_statement_audit() {
    let path = std::env::var("OO_A110_LOG").expect("OO_A110_LOG");
    let password_file = std::env::var("OO_A110_PASSWORD_FILE").expect("OO_A110_PASSWORD_FILE");
    let report_path = std::env::var("OO_A110_AUDIT_REPORT").unwrap_or_else(|_| "/phases/log-audit.txt".to_string());
    let log = fs::read_to_string(&path).expect("log");
    let password = fs::read_to_string(&password_file).expect("password file");
    let password = password.trim();
    assert!(!password.is_empty(), "password file is empty");
    assert!(log.contains("A110-LOG-READY"), "logging marker is absent");
    let marker_at = log.find("A110-LOG-READY").unwrap();
    let reader_at = log.find("app=oo-pg-source");
    if let Some(reader_at) = reader_at {
        assert!(marker_at < reader_at, "reader connected before the logging marker");
    }
    let mut select_n = 0u64;
    let mut start_n = 0u64;
    let mut rollback_n = 0u64;
    let mut forbidden_n = 0u64;
    let mut reader_sql = 0u64;
    let mut unknown = Vec::new();
    for line in BufReader::new(log.as_bytes()).lines() {
        let line = line.unwrap();
        if !line.contains("app=oo-pg-source") || !line.contains("user=oo_a110_reader") {
            continue;
        }
        let Some(sql) = sql_of(&line) else {
            continue;
        };
        if sql.is_empty() {
            continue;
        }
        reader_sql += 1;
        let word = keyword(sql);
        match word.as_str() {
            "SELECT" | "WITH" => select_n += 1,
            "START" | "BEGIN" => start_n += 1,
            "ROLLBACK" => rollback_n += 1,
            other if FORBIDDEN.contains(&other) => forbidden_n += 1,
            other if ALLOWED.contains(&other) => {}
            other => unknown.push(other.to_string()),
        }
    }
    let after = &log[marker_at..];
    let canary = after.matches("CANARY").count();
    let touch = after.matches("touch_canary").count();
    let secret = after.matches(password).count();
    let report = format!(
        "reader_sql={reader_sql}\nselect={select_n}\nstart={start_n}\nrollback={rollback_n}\nforbidden={forbidden_n}\nunknown={unknown:?}\ncanary={canary}\ntouch_canary={touch}\npassword_hits={secret}\nbytes={}\n",
        log.len()
    );
    fs::write(&report_path, &report).expect("audit report");
    assert!(reader_sql > 0, "reader statement trace is empty\n{report}");
    assert!(select_n > 0, "{report}");
    assert!(start_n > 0, "{report}");
    assert!(rollback_n > 0, "{report}");
    assert_eq!(forbidden_n, 0, "{report}");
    assert!(unknown.is_empty(), "{report}");
    assert_eq!(canary, 0, "{report}");
    assert_eq!(touch, 0, "{report}");
    assert_eq!(secret, 0, "{report}");
}
