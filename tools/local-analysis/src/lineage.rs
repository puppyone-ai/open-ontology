use std::path::Path;

use crate::fault::Fault;
use crate::json::{self, Json, JsonError};
use crate::limits::{MAX_LINEAGE_BYTES, MAX_RECEIPT_LINE};
use crate::path;
use crate::sha256::{is_lower_hex, sha256_hex};
use crate::source::Facts;

#[derive(Clone, Debug)]
pub struct Lineage {
    pub query_id: String,
    pub role: String,
    pub project: String,
    pub receipt_sha256: String,
    pub facts: Facts,
}

pub fn load_lineage(path: &Path) -> Result<Lineage, Fault> {
    let bytes = path::read_regular(path, MAX_LINEAGE_BYTES, Fault::lineage())?;
    if bytes.contains(&b'\r') {
        return Err(Fault::lineage());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| Fault::lineage())?;
    let value = match json::parse_json(text) {
        Ok(value) => value,
        Err(JsonError::Budget) => return Err(Fault::budget()),
        Err(JsonError::Syntax) => return Err(Fault::lineage()),
    };
    let mut fields = json::object(value).map_err(|()| Fault::lineage())?;
    let version = take_string(&mut fields)?;
    if version != "oo.local-analysis-lineage/v1" {
        return Err(Fault::lineage());
    }
    let trust = take_keyed(&mut fields, "trust")?;
    if trust != "local_operator_declared" {
        return Err(Fault::lineage());
    }
    let query_id = take_keyed(&mut fields, "query_id")?;
    let receipt_sha256 = take_keyed(&mut fields, "receipt_sha256")?;
    let receipt_body = take_keyed(&mut fields, "receipt_body")?;
    json::finish(&fields).map_err(|()| Fault::lineage())?;
    if !is_lower_hex(&receipt_sha256, Some(64)) {
        return Err(Fault::lineage());
    }
    if sha256_hex(receipt_body.as_bytes()) != receipt_sha256 {
        return Err(Fault::hash());
    }
    if receipt_body.contains('\r') || receipt_body.is_empty() {
        return Err(Fault::lineage());
    }
    let pair = parse_pair(&receipt_body)?;
    let derived = derived_id(&pair.fingerprint).ok_or(Fault::lineage())?;
    if query_id != derived {
        return Err(Fault::lineage());
    }
    Ok(Lineage {
        query_id,
        role: pair.role,
        project: pair.project,
        receipt_sha256,
        facts: pair.facts,
    })
}

struct Pair {
    fingerprint: String,
    role: String,
    project: String,
    facts: Facts,
}

fn parse_pair(body: &str) -> Result<Pair, Fault> {
    let mut lines: Vec<&str> = body.split('\n').collect();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    if lines.len() != 2 || lines.iter().any(|line| line.is_empty() || line.len() > MAX_RECEIPT_LINE)
    {
        return Err(Fault::lineage());
    }
    let admit = parse_record(lines[0], "admit")?;
    let complete = parse_record(lines[1], "complete")?;
    if admit.identity != complete.identity {
        return Err(Fault::lineage());
    }
    let Some(facts) = complete.facts else {
        return Err(Fault::lineage());
    };
    Ok(Pair {
        fingerprint: admit.identity.fingerprint,
        role: admit.identity.role,
        project: admit.identity.project,
        facts,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Identity {
    protocol: String,
    original_sha256: String,
    executed_sha256: String,
    fingerprint: String,
    role: String,
    project: String,
    policy_id: String,
    backend_pid: u64,
    backend_start: String,
}

struct Record {
    identity: Identity,
    facts: Option<Facts>,
}

fn parse_record(line: &str, record: &str) -> Result<Record, Fault> {
    let value = match json::parse_json(line) {
        Ok(value) => value,
        Err(JsonError::Budget) => return Err(Fault::budget()),
        Err(JsonError::Syntax) => return Err(Fault::lineage()),
    };
    let mut fields = json::object(value).map_err(|()| Fault::lineage())?;
    let protocol = take_keyed(&mut fields, "protocol")?;
    if protocol != "oo.gateway.query-pg-audit/v1" {
        return Err(Fault::lineage());
    }
    let got = take_keyed(&mut fields, "record")?;
    if got != record {
        return Err(Fault::lineage());
    }
    let original_sha256 = take_hex(&mut fields, "original_sha256")?;
    let executed_sha256 = take_hex(&mut fields, "executed_sha256")?;
    let fingerprint = take_keyed(&mut fields, "fingerprint_hex")?;
    if !is_lower_hex(&fingerprint, None) || fingerprint.len() > 128 {
        return Err(Fault::lineage());
    }
    let role = take_keyed(&mut fields, "role")?;
    let project = take_keyed(&mut fields, "project")?;
    if !role_ok(&role) || !slug_ok(&project, 63) {
        return Err(Fault::lineage());
    }
    let policy_id = take_hex(&mut fields, "policy_id")?;
    let backend_pid = take_u64(&mut fields, "backend_pid")?;
    if !(1..=2_147_483_647).contains(&backend_pid) {
        return Err(Fault::lineage());
    }
    let backend_start = take_keyed(&mut fields, "backend_start")?;
    if backend_start.is_empty()
        || backend_start.len() > 64
        || !backend_start.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(Fault::lineage());
    }
    let identity = Identity {
        protocol,
        original_sha256: original_sha256.clone(),
        executed_sha256: executed_sha256.clone(),
        fingerprint,
        role,
        project,
        policy_id,
        backend_pid,
        backend_start,
    };
    let facts = if record == "complete" {
        let status = take_keyed(&mut fields, "status")?;
        if !matches!(status.as_str(), "succeeded" | "partial" | "failed") {
            return Err(Fault::lineage());
        }
        let truncated = take_bool(&mut fields, "truncated")?;
        let returned_rows = take_u64(&mut fields, "returned_rows")?;
        let result_bytes = take_u64(&mut fields, "result_bytes")?;
        let rollback_confirmed = take_bool(&mut fields, "rollback_confirmed")?;
        let socket_dropped = take_bool(&mut fields, "socket_dropped")?;
        let stop = take_stop(&mut fields)?;
        let sqlstate = take_sqlstate(&mut fields)?;
        Some(Facts {
            status,
            truncated,
            returned_rows,
            result_bytes,
            rollback_confirmed,
            socket_dropped,
            stop,
            sqlstate,
            original_sha256,
            executed_sha256,
        })
    } else {
        None
    };
    json::finish(&fields).map_err(|()| Fault::lineage())?;
    Ok(Record { identity, facts })
}

fn derived_id(fingerprint: &str) -> Option<String> {
    if (6..=40).contains(&fingerprint.len()) {
        Some(format!("q_{fingerprint}"))
    } else {
        None
    }
}

fn role_ok(role: &str) -> bool {
    let Some(rest) = role.strip_prefix("oo_r_") else {
        return false;
    };
    slug_ok(rest, 32)
}

fn slug_ok(text: &str, max: usize) -> bool {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() > max {
        return false;
    }
    let first = bytes[0];
    if !first.is_ascii_lowercase() && first != b'_' {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn take_string(fields: &mut Vec<(String, Json)>) -> Result<String, Fault> {
    take_keyed(fields, "version")
}

fn take_keyed(fields: &mut Vec<(String, Json)>, key: &str) -> Result<String, Fault> {
    let value = json::take(fields, key).map_err(|()| Fault::lineage())?;
    json::as_str(&value)
        .map(str::to_string)
        .map_err(|()| Fault::lineage())
}

fn take_hex(fields: &mut Vec<(String, Json)>, key: &str) -> Result<String, Fault> {
    let text = take_keyed(fields, key)?;
    if is_lower_hex(&text, Some(64)) {
        Ok(text)
    } else {
        Err(Fault::lineage())
    }
}

fn take_bool(fields: &mut Vec<(String, Json)>, key: &str) -> Result<bool, Fault> {
    let value = json::take(fields, key).map_err(|()| Fault::lineage())?;
    json::as_bool(&value).map_err(|()| Fault::lineage())
}

fn take_u64(fields: &mut Vec<(String, Json)>, key: &str) -> Result<u64, Fault> {
    let value = json::take(fields, key).map_err(|()| Fault::lineage())?;
    json::as_u64(&value).map_err(|()| Fault::lineage())
}

fn take_stop(fields: &mut Vec<(String, Json)>) -> Result<Option<String>, Fault> {
    let value = json::take(fields, "stop").map_err(|()| Fault::lineage())?;
    match value {
        Json::Null => Ok(None),
        Json::String(text)
            if matches!(
                text.as_str(),
                "timeout" | "canceled" | "type_refused" | "database" | "protocol"
            ) =>
        {
            Ok(Some(text))
        }
        _ => Err(Fault::lineage()),
    }
}

fn take_sqlstate(fields: &mut Vec<(String, Json)>) -> Result<Option<String>, Fault> {
    let value = json::take(fields, "sqlstate").map_err(|()| Fault::lineage())?;
    match value {
        Json::Null => Ok(None),
        Json::String(text) if sqlstate_ok(&text) => Ok(Some(text)),
        _ => Err(Fault::lineage()),
    }
}

fn sqlstate_ok(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 5
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
}
