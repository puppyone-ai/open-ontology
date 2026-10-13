//! JSON labels shared by every command. Cell shape matches query-pg.

use std::fmt;

use oo_gateway::query_executor::{Cell, UntrustedAdmission, UNTRUSTED_ADMISSION};
use serde_json::{json, Map, Value};

use crate::{SURFACE, EXIT_AUDIT, EXIT_GUARD, EXIT_IO, EXIT_NOT_SENT, EXIT_PACK, EXIT_POLICY, EXIT_QUERY, EXIT_REQUEST, EXIT_TLS, EXIT_USAGE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fail {
    Io,
    Usage,
    Request,
    Tls,
    Policy,
    Guard,
    Audit,
    Query,
    Pack,
    NotSent,
}

impl Fail {
    pub fn code(self) -> u8 {
        match self {
            Self::Io => EXIT_IO,
            Self::Usage => EXIT_USAGE,
            Self::Request => EXIT_REQUEST,
            Self::Tls => EXIT_TLS,
            Self::Policy => EXIT_POLICY,
            Self::Guard => EXIT_GUARD,
            Self::Audit => EXIT_AUDIT,
            Self::Query => EXIT_QUERY,
            Self::Pack => EXIT_PACK,
            Self::NotSent => EXIT_NOT_SENT,
        }
    }

    pub fn token(self) -> &'static str {
        match self {
            Self::Io => "io",
            Self::Usage => "usage",
            Self::Request => "request",
            Self::Tls => "tls",
            Self::Policy => "policy",
            Self::Guard => "guard",
            Self::Audit => "audit",
            Self::Query => "query",
            Self::Pack => "pack",
            Self::NotSent => "not-sent",
        }
    }
}

impl fmt::Display for Fail {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.token())
    }
}

pub struct Finished {
    pub code: u8,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub fn finished_error(fail: Fail) -> Finished {
    Finished {
        code: fail.code(),
        stdout: Vec::new(),
        stderr: format!("operator-preview: {}\n", fail.token()).into_bytes(),
    }
}

pub fn finished_value(fail: Option<Fail>, value: &Value) -> Finished {
    let mut bytes = match serde_json::to_vec_pretty(value) {
        Ok(bytes) => bytes,
        Err(_) => return finished_error(Fail::Io),
    };
    bytes.push(b'\n');
    if bytes_have_rows_when_refused(value, &bytes) {
        return finished_error(Fail::Io);
    }
    let fail = fail.unwrap_or(Fail::Io);
    let code = if fail_is_success(fail, value) {
        0
    } else {
        fail.code()
    };
    let stderr = if code == 0 {
        Vec::new()
    } else {
        format!("operator-preview: {}\n", fail.token()).into_bytes()
    };
    Finished {
        code,
        stdout: bytes,
        stderr,
    }
}

fn fail_is_success(_fail: Fail, value: &Value) -> bool {
    matches!(
        value.get("outcome").and_then(Value::as_str),
        Some("succeeded" | "described" | "sent" | "canceled")
    )
}

fn bytes_have_rows_when_refused(value: &Value, bytes: &[u8]) -> bool {
    let outcome = value.get("outcome").and_then(Value::as_str).unwrap_or("");
    if matches!(outcome, "succeeded" | "described") {
        return false;
    }
    value.get("rows").is_some() || bytes.windows(6).any(|window| window == b"\"rows\"")
}

pub fn shell(mut fields: Map<String, Value>) -> Value {
    fields.insert("surface".to_string(), json!(SURFACE));
    fields.insert(
        "public_untrusted_gate".to_string(),
        json!("pending"),
    );
    fields.insert(
        "untrusted_admission".to_string(),
        json!(admission_label()),
    );
    fields.insert("pi_session".to_string(), json!("not_launched"));
    fields.insert("publication".to_string(), json!("not_published"));
    fields.insert(
        "policy_source".to_string(),
        json!("trusted-operator-file"),
    );
    fields.insert("pack_text_is_policy".to_string(), json!(false));
    Value::Object(fields)
}

pub fn admission_label() -> &'static str {
    match UNTRUSTED_ADMISSION {
        UntrustedAdmission::Blocked => "blocked",
    }
}

pub fn cell_value(cell: &Cell) -> Value {
    match cell {
        Cell::Null => json!({"kind": "null"}),
        Cell::Bool(value) => json!({"kind": "bool", "value": value}),
        Cell::Int(value) => json!({"kind": "int", "value": value}),
        Cell::Text(value) => json!({"kind": "text", "value": value}),
        Cell::Decimal(value) => json!({
            "kind": "decimal",
            "negative": value.is_negative(),
            "coefficient": value.coefficient(),
            "scale": value.scale(),
        }),
    }
}

pub fn rows_value(rows: &[Vec<Cell>]) -> Value {
    Value::Array(
        rows.iter()
            .map(|row| Value::Array(row.iter().map(cell_value).collect()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Map};

    use super::{admission_label, finished_value, shell};

    #[test]
    fn admission_constant_stays_blocked() {
        assert_eq!(admission_label(), "blocked");
    }

    #[test]
    fn described_sent_and_canceled_exit_zero_without_rows() {
        for outcome in ["described", "sent", "canceled"] {
            let mut fields = Map::new();
            fields.insert("outcome".to_string(), json!(outcome));
            let finished = finished_value(None, &shell(fields));
            assert_eq!(finished.code, 0, "{outcome}");
            assert!(finished.stderr.is_empty(), "{outcome}");
        }
    }

    #[test]
    fn refused_document_with_rows_emits_nothing() {
        let mut fields = Map::new();
        fields.insert("outcome".to_string(), json!("refused"));
        fields.insert("rows".to_string(), json!([]));
        let finished = finished_value(None, &shell(fields));
        assert_eq!(finished.code, 1);
        assert!(finished.stdout.is_empty());
    }
}
