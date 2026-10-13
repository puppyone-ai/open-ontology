//! Source-statement audit parsers. A parsed file is not a product acceptance.
//! Platform self-claims, empty leases, and class/SQL contradictions stay errors.

use serde_json::Value;

pub const SCHEMA: &str = "oo-source-conformance/audit/v1";

const ENGINES: &[&str] = &[
    "postgres",
    "mysql",
    "mariadb",
    "mssql",
    "oracle",
    "db2",
    "hana",
    "snowflake",
    "bigquery",
    "databricks",
    "redshift",
    "clickhouse",
    "doris",
    "starrocks",
    "mongodb",
    "opensearch",
    "dynamodb",
];

const WRITE_VERBS: &[&str] = &[
    "INSERT", "UPDATE", "DELETE", "MERGE", "CREATE", "ALTER", "DROP", "TRUNCATE", "GRANT", "REVOKE", "COPY", "REPLACE", "UPSERT", "REMOVE",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditOutcome {
    pub ok: bool,
    pub zero_write: bool,
    pub product_accepted: bool,
    pub counts_as_cc02_pass: bool,
    pub statements: u32,
    pub codes: Vec<String>,
}

pub fn parse_audit(bytes: &[u8]) -> AuditOutcome {
    let mut codes = Vec::new();
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        codes.push("audit.parser_error: UTF-8 BOM".to_owned());
        return finish(false, false, 0, codes);
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        codes.push("audit.parser_error: not UTF-8".to_owned());
        return finish(false, false, 0, codes);
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        codes.push("audit.parser_error: empty".to_owned());
        return finish(false, false, 0, codes);
    }
    if trimmed.starts_with('{') {
        return parse_self_claim(trimmed);
    }
    let mut lines = text.lines();
    let Some(header) = lines.next() else {
        codes.push("audit.parser_error: empty".to_owned());
        return finish(false, false, 0, codes);
    };
    if header.trim() != "oo-source-audit/v1" {
        codes.push("audit.parser_error: missing oo-source-audit/v1 header".to_owned());
        return finish(false, false, 0, codes);
    }
    let mut statements = 0u32;
    let mut zero_write = true;
    for (index, line) in lines.enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        statements += 1;
        let mut parts = line.splitn(3, char::is_whitespace);
        let engine = parts.next().unwrap_or("");
        let class = parts.next().unwrap_or("");
        let statement = parts.next().unwrap_or("").trim();
        if !ENGINES.contains(&engine) {
            codes.push(format!("audit.unknown_engine: line {} {engine}", index + 2));
            zero_write = false;
            continue;
        }
        if !matches!(class, "R0" | "R1" | "R1w" | "R2" | "R3") || statement.is_empty() {
            codes.push(format!("audit.parser_error: line {} malformed", index + 2));
            zero_write = false;
            continue;
        }
        let write = statement_is_write(statement);
        match class {
            "R0" | "R1" if write => {
                codes.push(format!("audit.class_contradicts_statement: line {} {class} starts with a write", index + 2));
                zero_write = false;
            }
            "R1w" => {
                codes.push(format!("audit.r1w_not_zero_write: line {}", index + 2));
                zero_write = false;
            }
            "R2" | "R3" => {
                codes.push(format!("audit.write_detected: line {} {class}", index + 2));
                zero_write = false;
            }
            _ => {}
        }
    }
    if statements == 0 {
        codes.push("audit.parser_error: no statements".to_owned());
        zero_write = false;
    }
    let ok = codes.is_empty();
    finish(ok, ok && zero_write, statements, codes)
}

fn parse_self_claim(text: &str) -> AuditOutcome {
    let mut codes = Vec::new();
    let parsed: Result<Value, _> = serde_json::from_str(text);
    let Ok(value) = parsed else {
        return finish(false, false, 0, vec!["audit.parser_error: JSON is not an audit record".to_owned()]);
    };
    let Some(map) = value.as_object() else {
        return finish(false, false, 0, vec!["audit.parser_error: JSON is not an audit record".to_owned()]);
    };
    if map.contains_key("source_bound") || map.contains_key("zero_write") || map.contains_key("trusted") {
        codes.push("trust.producer_self_trust: author boolean is not a source audit".to_owned());
    }
    let empty_lease = json_null(map.get("lease"))
        || json_null(map.get("jobs_lease_receipt"))
        || json_null(map.get("jobs_lease"));
    if empty_lease || map.contains_key("lease") || map.contains_key("jobs_lease_receipt") {
        codes.push("trust.empty_lease_not_pass: empty lease is not a fence".to_owned());
    }
    if codes.is_empty() {
        codes.push("audit.parser_error: JSON object is not oo-source-audit/v1".to_owned());
    }
    finish(false, false, 0, codes)
}

fn json_null(value: Option<&Value>) -> bool {
    value.is_some_and(Value::is_null)
}

fn statement_is_write(statement: &str) -> bool {
    let token = statement.split_whitespace().next().unwrap_or("").trim_matches('(');
    WRITE_VERBS.iter().any(|verb| token.eq_ignore_ascii_case(verb))
}

fn finish(ok: bool, zero_write: bool, statements: u32, codes: Vec<String>) -> AuditOutcome {
    AuditOutcome {
        ok,
        zero_write,
        product_accepted: false,
        counts_as_cc02_pass: false,
        statements,
        codes,
    }
}
