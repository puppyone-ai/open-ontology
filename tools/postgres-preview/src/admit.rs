//! Session report and the coordinator record for the W38 budget producer.
//!
//! This module does not invent a known budget unit. `cost_model.unit` stays
//! `unrecorded` unless a coordinator-recorded commit is present. The admission
//! decision itself is the configured `oo-source-admission` binary's stdout.

use std::fs;
use std::path::Path;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::error::Fail;

pub const SESSION_PROTOCOL: &str = "oo.postgres-preview.session/v1";
#[cfg(test)]
pub const REPORT_PROTOCOL: &str = "oo.source-admission.report/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionFacts {
    pub current_user: String,
    pub session_user: String,
    pub database: String,
    pub server_version: String,
    pub server_version_num: i64,
    pub superuser: bool,
    pub create_role: bool,
    pub create_db: bool,
    pub replication: bool,
    pub bypass_rls: bool,
    pub can_login: bool,
    pub select_ok: bool,
    pub write_privilege: bool,
    pub statement_timeout: String,
    pub lock_timeout: String,
    pub default_transaction_read_only: String,
    pub negotiated_tls: String,
}

pub struct AdmissionInput<'a> {
    pub source_id: &'a str,
    pub secret_ref: &'a str,
    pub expected_user: &'a str,
    pub expected_database: &'a str,
    pub facts: &'a SessionFacts,
}

pub fn identity_ok(input: &AdmissionInput<'_>) -> bool {
    let facts = input.facts;
    facts.current_user == input.expected_user
        && facts.session_user == input.expected_user
        && facts.database == input.expected_database
        && facts.server_version_num == 170_010
        && facts.server_version.contains("17.10")
        && !facts.superuser
        && !facts.create_role
        && !facts.create_db
        && !facts.replication
        && !facts.bypass_rls
        && facts.can_login
        && facts.select_ok
        && !facts.write_privilege
        && facts.negotiated_tls == "TLSv1.3"
}

pub fn session_report(input: &AdmissionInput<'_>) -> Value {
    json!({
        "protocol": SESSION_PROTOCOL,
        "source_id": input.source_id,
        "secret_ref": input.secret_ref,
        "identity": {
            "current_user": input.facts.current_user,
            "session_user": input.facts.session_user,
            "database": input.facts.database,
            "aligned": input.facts.current_user == input.facts.session_user
        },
        "version": {
            "server_version": input.facts.server_version,
            "server_version_num": input.facts.server_version_num
        },
        "permission": {
            "select": input.facts.select_ok,
            "write_privilege": input.facts.write_privilege,
            "superuser": input.facts.superuser
        },
        "settings": {
            "statement_timeout": input.facts.statement_timeout,
            "lock_timeout": input.facts.lock_timeout,
            "default_transaction_read_only": input.facts.default_transaction_read_only
        },
        "tls": input.facts.negotiated_tls,
        "budget_producer": "unrecorded",
        "cost_model": {"unit": "unrecorded", "confidence": "unknown", "estimate": "not_collected"}
    })
}

/// Report passed to frozen `validate`. The unit is the observed fact, not a
/// filled-in `Rows` value. The product path does not call this.
#[cfg(test)]
pub fn admission_report(input: &AdmissionInput<'_>) -> Value {
    let granted = input.facts.select_ok && !input.facts.write_privilege;
    json!({
        "protocol": REPORT_PROTOCOL,
        "target_id": "postgres-17-native",
        "source_id": input.source_id,
        "secret_ref": input.secret_ref,
        "driver": {
            "kind": "in_process",
            "name": "tokio-postgres",
            "version": "0.7.18",
            "digest": "unrecorded"
        },
        "compat_mode": "postgres-native",
        "license_flags": [],
        "permissions": [
            {"name": "SELECT", "status": "known", "granted": granted},
            {"name": "USAGE", "status": "known", "granted": true},
            {"name": "CONNECT", "status": "known", "granted": true}
        ],
        "capabilities": [{
            "name": "BatchFull",
            "cursor": null,
            "feed": null,
            "cdc": null,
            "token_order": null,
            "budget_unit": null,
            "level": "designed",
            "write_class": "R0",
            "consistency": "S0"
        }],
        "capture_methods": [{
            "method": "C1",
            "write_class": "R0",
            "consistency": "S0",
            "token_order": "None",
            "retention_window": null,
            "cursor": null,
            "feed": null,
            "cdc": null
        }],
        "cost_model": {"unit": "unrecorded", "confidence": "unknown", "estimate": "not_collected"},
        "r1w": null
    })
}

#[cfg(test)]
pub fn admission_config(input: &AdmissionInput<'_>) -> Value {
    json!({
        "protocol": "oo.source-admission.config/v1",
        "target_id": "postgres-17-native",
        "source_id": input.source_id,
        "secret_ref": input.secret_ref,
        "compat_mode": "postgres-native",
        "permissions": [
            {"name": "SELECT", "status": "known", "granted": true},
            {"name": "USAGE", "status": "known", "granted": true},
            {"name": "CONNECT", "status": "known", "granted": true}
        ],
        "budget": {"unit": "unrecorded", "hard_limit": 0, "confidence": "unknown"},
        "write_options": {
            "signal_table": false,
            "heartbeat_write": false,
            "create_stream": false,
            "create_trigger": false
        },
        "debezium": {
            "signal_data_collection": null,
            "heartbeat_action_query": null,
            "snapshot_mode": "never",
            "create_stream": false,
            "create_trigger": false
        },
        "requested_write_class": "R0",
        "r1w": null
    })
}

pub fn sha256_json(value: &Value) -> String {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    sha256_bytes(&bytes)
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn hex40(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn read_w38_commit(path: &Path) -> Result<Option<String>, Fail> {
    let bytes = fs::read(path).map_err(|_| Fail::new("coordinator.read", 3))?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| Fail::new("coordinator.schema", 3))?;
    if let Some(commit) = value
        .pointer("/development_extensions/DBX-W02-BUDGET/fixed_commit")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| hex40(item))
    {
        return Ok(Some(commit.to_owned()));
    }
    let protocol = value.get("protocol").and_then(Value::as_str);
    let status = value.get("status").and_then(Value::as_str).unwrap_or("");
    let source = value.get("source_sha").and_then(Value::as_str).map(str::trim);
    if protocol == Some("oo.source.pgsql.budget/v1")
        && status.starts_with("author_complete")
        && source.is_some_and(hex40)
    {
        return Ok(source.map(str::to_owned));
    }
    Ok(None)
}

pub fn decision_admitted(stdout: &[u8]) -> Result<(bool, bool, String, String), Fail> {
    let value: Value = serde_json::from_slice(stdout).map_err(|_| Fail::new("admission.output", 2))?;
    if value.get("protocol").and_then(Value::as_str) != Some("oo.source-admission.decision/v1") {
        return Err(Fail::new("admission.protocol", 2));
    }
    let admitted = value.get("admitted").and_then(Value::as_bool).unwrap_or(false);
    let blocks = value.get("blocks").and_then(Value::as_array);
    let budget_unknown = blocks.is_some_and(|items| {
        items.iter().any(|item| item.get("code").and_then(Value::as_str) == Some("BUDGET_UNIT_UNKNOWN"))
    });
    let source_id = value
        .pointer("/identity/source_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let report_sha = value
        .pointer("/identity/source_report_sha256")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    Ok((admitted, budget_unknown, source_id, report_sha))
}

pub fn budget_block_is_missing_producer(budget_unknown: bool, commit: Option<&str>) -> bool {
    budget_unknown && commit.is_none()
}

pub fn required_w38_input() -> Value {
    json!({
        "dependency": "DL01-W1-38",
        "status": "pending",
        "required": {
            "record": "pg-budget-ready.json source_sha for protocol oo.source.pgsql.budget/v1 with status author_complete, or control-state fixed_commit",
            "commit": "40 lowercase hex git commit containing the PgSession oo.source.pgsql.budget/v1 producer and admit-pg-budget",
            "author_status": "author_complete",
            "not_sufficient": [
                "dirty workspace copies of crates/oo-connectors and tools/source-admission",
                "HANDOFF.md without receipt.json",
                "a legacy oo.source.pgsql/v1 report",
                "a hand-written admitted=true decision"
            ]
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> SessionFacts {
        SessionFacts {
            current_user: "reader".to_owned(),
            session_user: "reader".to_owned(),
            database: "src".to_owned(),
            server_version: "17.10 (Debian 17.10-1.pgdg12+1)".to_owned(),
            server_version_num: 170_010,
            superuser: false,
            create_role: false,
            create_db: false,
            replication: false,
            bypass_rls: false,
            can_login: true,
            select_ok: true,
            write_privilege: false,
            statement_timeout: "30000".to_owned(),
            lock_timeout: "0".to_owned(),
            default_transaction_read_only: "on".to_owned(),
            negotiated_tls: "TLSv1.3".to_owned(),
        }
    }

    #[test]
    fn report_does_not_fill_a_budget_unit() {
        let facts = facts();
        let input = AdmissionInput {
            source_id: "oo_dl01w1_46",
            secret_ref: "secret://oo-dl01w1-46/reader",
            expected_user: "reader",
            expected_database: "src",
            facts: &facts,
        };
        assert!(identity_ok(&input));
        let report = admission_report(&input);
        assert_eq!(report["cost_model"]["unit"], "unrecorded");
        assert!(report["capabilities"][0]["budget_unit"].is_null());
        let text = report.to_string();
        assert!(!text.contains("\"unit\":\"Rows\""));
        let legacy = admission_config(&input);
        assert_eq!(legacy["budget"]["unit"], "unrecorded");
        assert_eq!(legacy["write_options"]["signal_table"].as_bool(), Some(false));
    }

    #[test]
    fn older_postgres_and_write_privilege_fail() {
        let mut older = facts();
        older.server_version_num = 160_010;
        older.server_version = "16.10".to_owned();
        let older_input = AdmissionInput {
            source_id: "s",
            secret_ref: "secret://s",
            expected_user: "reader",
            expected_database: "src",
            facts: &older,
        };
        assert!(!identity_ok(&older_input));
        let mut writer = facts();
        writer.write_privilege = true;
        let writer_input = AdmissionInput {
            source_id: "s",
            secret_ref: "secret://s",
            expected_user: "reader",
            expected_database: "src",
            facts: &writer,
        };
        assert!(!identity_ok(&writer_input));
    }

    #[test]
    fn missing_fixed_commit_field_is_pending() {
        let value = json!({"development_extensions": {"DBX-W02-BUDGET": {"frozen_units": 1}}});
        let path = std::env::temp_dir().join(format!("oo-dl01w1-46-control-{}.json", std::process::id()));
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let commit = read_w38_commit(&path).unwrap();
        assert!(commit.is_none());
        let _ = fs::remove_file(path);
        assert!(budget_block_is_missing_producer(true, None));
    }

    #[test]
    fn ready_file_source_sha_is_the_producer_commit() {
        let value = json!({
            "protocol": "oo.source.pgsql.budget/v1",
            "status": "author_complete_review_deferred",
            "source_sha": "6590cff4acc1cb265d0ed05d460b0582251cf360"
        });
        let path = std::env::temp_dir().join(format!("oo-dl01w1-46-ready-{}.json", std::process::id()));
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let commit = read_w38_commit(&path).unwrap();
        let _ = fs::remove_file(path);
        assert_eq!(commit.as_deref(), Some("6590cff4acc1cb265d0ed05d460b0582251cf360"));
    }
}
