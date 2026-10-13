//! Customer entry for one OpenSearch source.
//!
//! `decide` calls the frozen `source_admission::admit` at content commit
//! 5df40d24. A search grant is observed only when a live search on one exact
//! index returned HTTP 200. mget is granted only when that live call returned
//! HTTP 200. This crate does not relabel the source as PostgreSQL and it does
//! not start import, Builder, or operator preview from admission alone.
//!
//! Index, fields, budget, target, role, and the measured row count come from
//! the customer request. An index or field outside that approval is refused
//! before `admit`.

#![allow(clippy::uninlined_format_args, clippy::redundant_closure_for_method_calls)]

use serde_json::{json, Value};
use source_admission::{
    admit, Budget, CaptureMethod, CostModel, DebeziumOptions, Decision, DeclaredCapability, Driver,
    GrantScope, Permission, SourceConfig, TargetReport, WriteOptions, CONFIG_PROTOCOL, REPORT_PROTOCOL,
};

pub const OBSERVED_COMPAT: &str = "opensearch-native";
pub const SEARCH: &str = "indices:data/read/search";
pub const MGET: &str = "indices:data/read/mget";
pub const GAP: &str = "indices:data/read/search is admitted only for one observed exact index; mget stays ungranted unless that live call returns 200; wildcards, write, admin, and create-index stay denied; oo.source.pgsql.budget/v1 is native PostgreSQL only";

const BUDGET_UNITS: &[&str] = &["Rows", "Bytes", "Credits", "Dbu", "ReadUnits", "WallClock"];

pub struct CustomerInput {
    pub source_id: String,
    pub secret_ref: String,
    pub approved_index: String,
    pub approved_fields: Vec<String>,
    pub requested_index: String,
    pub requested_fields: Vec<String>,
    pub budget_unit: String,
    pub budget_hard_limit: u64,
    pub target_id: String,
    pub role: String,
    pub relation: String,
    pub measured_rows: u64,
    pub search_status: u16,
    pub mget_status: u16,
}

pub fn precheck(input: &CustomerInput) -> Result<(), String> {
    if input.source_id.is_empty() || input.source_id.contains('*') {
        return Err("source_id".into());
    }
    if !input.secret_ref.starts_with("secret://") || input.secret_ref.contains('*') {
        return Err("secret_ref must be a secret:// reference".into());
    }
    if !exact_index(&input.approved_index) {
        return Err("approved index".into());
    }
    if input.requested_index != input.approved_index {
        return Err(format!("unauthorized index {}", input.requested_index));
    }
    if input.approved_fields.is_empty() {
        return Err("approved fields".into());
    }
    for field in &input.approved_fields {
        if !field_name(field) {
            return Err(format!("approved field {field}"));
        }
    }
    if input.requested_fields.is_empty() {
        return Err("requested fields".into());
    }
    for field in &input.requested_fields {
        if !input.approved_fields.iter().any(|approved| approved == field) {
            return Err(format!("unauthorized field {field}"));
        }
    }
    if !BUDGET_UNITS.contains(&input.budget_unit.as_str()) || input.budget_hard_limit == 0 {
        return Err("budget".into());
    }
    if !target_id(&input.target_id) {
        return Err("target".into());
    }
    if !authority_role(&input.role) {
        return Err("role".into());
    }
    if !field_name(&input.relation) {
        return Err("relation".into());
    }
    if input.measured_rows == 0 {
        return Err("measured rows".into());
    }
    Ok(())
}

pub fn cost_estimate(input: &CustomerInput) -> String {
    format!("measured_rows={}", input.measured_rows)
}

pub fn decide(driver_digest: &str, input: &CustomerInput) -> Result<Decision, String> {
    precheck(input)?;
    let (report, config) = admission_pair(driver_digest, input);
    Ok(admit(&report, &config))
}

pub fn admission_documents(driver_digest: &str, input: &CustomerInput) -> Result<(Value, Value), String> {
    precheck(input)?;
    let (report, config) = admission_pair(driver_digest, input);
    let report = serde_json::to_value(&report).map_err(|error| error.to_string())?;
    let config = serde_json::to_value(&config).map_err(|error| error.to_string())?;
    Ok((report, config))
}

pub fn customer_document(input: &CustomerInput) -> Value {
    json!({
        "source_id": input.source_id,
        "secret_ref": input.secret_ref,
        "approved_index": input.approved_index,
        "approved_fields": input.approved_fields,
        "requested_index": input.requested_index,
        "requested_fields": input.requested_fields,
        "budget_unit": input.budget_unit,
        "budget_hard_limit": input.budget_hard_limit,
        "target_id": input.target_id,
        "role": input.role,
        "relation": input.relation,
        "measured_rows": input.measured_rows,
        "cost_estimate": cost_estimate(input),
    })
}

fn admission_pair(driver_digest: &str, input: &CustomerInput) -> (TargetReport, SourceConfig) {
    let permissions = vec![
        permission(SEARCH, input.search_status == 200, &input.approved_index),
        permission(MGET, input.mget_status == 200, &input.approved_index),
    ];
    let report = TargetReport {
        protocol: REPORT_PROTOCOL.to_string(),
        target_id: input.target_id.clone(),
        source_id: input.source_id.clone(),
        secret_ref: input.secret_ref.clone(),
        driver: Driver {
            kind: "native-http".to_string(),
            name: "os-reader".to_string(),
            version: "3.9.0".to_string(),
            digest: driver_digest.to_string(),
        },
        compat_mode: OBSERVED_COMPAT.to_string(),
        license_flags: vec!["Apache-2.0".to_string()],
        permissions: permissions.clone(),
        capabilities: vec![capability()],
        capture_methods: vec![capture()],
        cost_model: CostModel {
            unit: input.budget_unit.clone(),
            confidence: "unknown".to_string(),
            estimate: cost_estimate(input),
        },
        r1w: None,
        native: None,
    };
    let config = SourceConfig {
        protocol: CONFIG_PROTOCOL.to_string(),
        target_id: report.target_id.clone(),
        source_id: report.source_id.clone(),
        secret_ref: report.secret_ref.clone(),
        compat_mode: OBSERVED_COMPAT.to_string(),
        permissions,
        budget: Budget {
            unit: input.budget_unit.clone(),
            hard_limit: input.budget_hard_limit,
            confidence: "unknown".to_string(),
        },
        write_options: WriteOptions {
            signal_table: false,
            heartbeat_write: false,
            create_stream: false,
            create_trigger: false,
        },
        debezium: DebeziumOptions {
            signal_data_collection: None,
            heartbeat_action_query: None,
            snapshot_mode: "never".to_string(),
            create_stream: false,
            create_trigger: false,
        },
        requested_write_class: "R0".to_string(),
        r1w: None,
        native: None,
    };
    (report, config)
}

fn permission(name: &str, granted: bool, index: &str) -> Permission {
    Permission {
        name: name.to_string(),
        status: "known".to_string(),
        granted,
        scope: Some(GrantScope {
            database: None,
            table: None,
            index: Some(index.to_string()),
            observed: Some(granted),
        }),
    }
}

pub fn import_allowed(decision: &Decision) -> bool {
    decision.admitted
}

pub fn gap_document(decision: &Decision) -> Value {
    json!({
        "admitted": decision.admitted,
        "blocks": decision.blocks,
        "gap": GAP,
        "import_started": false,
        "builder_started": false,
        "operator_preview_started": false,
        "pg_budget_protocol_used": false,
        "compat_observed": OBSERVED_COMPAT,
        "independent_approval": false,
    })
}

fn capability() -> DeclaredCapability {
    DeclaredCapability {
        name: "BatchFull".to_string(),
        cursor: None,
        feed: None,
        cdc: None,
        token_order: Some("None".to_string()),
        budget_unit: Some("Rows".to_string()),
        level: "experimental".to_string(),
        write_class: "R0".to_string(),
        consistency: "S0".to_string(),
    }
}

fn capture() -> CaptureMethod {
    CaptureMethod {
        method: "C5".to_string(),
        write_class: "R0".to_string(),
        consistency: "S0".to_string(),
        token_order: "None".to_string(),
        retention_window: None,
        cursor: None,
        feed: None,
        cdc: None,
    }
}

fn exact_index(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    value.len() <= 255
        && value != "_all"
        && (first.is_ascii_lowercase() || first.is_ascii_digit())
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
}

fn field_name(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    value.len() <= 63
        && first.is_ascii_lowercase()
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

fn target_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.contains('*')
        && value.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn authority_role(value: &str) -> bool {
    let Some(slug) = value.strip_prefix("oo_r_") else {
        return false;
    };
    let mut chars = slug.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    slug.len() <= 58
        && first.is_ascii_lowercase()
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(index: &str, search_status: u16, mget_status: u16) -> CustomerInput {
        CustomerInput {
            source_id: "oo_dl01w1_44".to_string(),
            secret_ref: "secret://oo-dl01w1-44/opensearch".to_string(),
            approved_index: index.to_string(),
            approved_fields: vec!["doc_key".to_string(), "event_ms".to_string(), "payload_json".to_string()],
            requested_index: index.to_string(),
            requested_fields: vec!["doc_key".to_string(), "event_ms".to_string()],
            budget_unit: "Rows".to_string(),
            budget_hard_limit: 40,
            target_id: "oo_dl01w1_44_ledger_pg".to_string(),
            role: "oo_r_ledger".to_string(),
            relation: "ledger_rows".to_string(),
            measured_rows: 5,
            search_status,
            mget_status,
        }
    }

    fn observed(index: &str, search_status: u16, mget_status: u16) -> Decision {
        decide(&"ab".repeat(32), &sample(index, search_status, mget_status)).expect("precheck")
    }

    #[test]
    fn observed_search_200_and_mget_403_is_admitted() {
        let input = sample("oo-dl01w1-44-ledger", 200, 403);
        let decision = decide(&"ab".repeat(32), &input).expect("precheck");
        assert!(decision.admitted, "{:?}", decision.blocks);
        assert!(import_allowed(&decision));
        let (report, config) = admission_documents(&"ab".repeat(32), &input).expect("documents");
        assert_eq!(report.get("target_id").and_then(Value::as_str), Some("oo_dl01w1_44_ledger_pg"));
        assert_eq!(
            report.pointer("/cost_model/estimate").and_then(Value::as_str),
            Some("measured_rows=5")
        );
        assert_ne!(report.pointer("/cost_model/estimate").and_then(Value::as_str), Some("8"));
        assert_eq!(config.pointer("/budget/hard_limit").and_then(Value::as_u64), Some(40));
        assert_eq!(config.pointer("/budget/confidence").and_then(Value::as_str), Some("unknown"));
        let customer = customer_document(&input);
        assert_eq!(customer.get("role").and_then(Value::as_str), Some("oo_r_ledger"));
        assert_eq!(customer.get("relation").and_then(Value::as_str), Some("ledger_rows"));
        let gap = gap_document(&decision);
        assert_eq!(gap.get("import_started").and_then(Value::as_bool), Some(false));
        assert_eq!(gap.get("builder_started").and_then(Value::as_bool), Some(false));
        assert_eq!(gap.get("operator_preview_started").and_then(Value::as_bool), Some(false));
        assert_eq!(gap.get("pg_budget_protocol_used").and_then(Value::as_bool), Some(false));
    }

    #[test]
    fn search_that_did_not_succeed_is_not_a_grant() {
        let decision = observed("oo-dl01w1-44-ledger", 403, 403);
        assert!(!decision.admitted);
        assert!(!import_allowed(&decision));
        let codes: Vec<&str> = decision.blocks.iter().map(|block| block.code.as_str()).collect();
        assert!(codes.contains(&"PERMISSION_UNKNOWN"));
    }

    #[test]
    fn wildcard_index_is_denied() {
        let mut input = sample("oo-dl01w1-44-ledger", 200, 403);
        input.approved_index = "oo-*".to_string();
        input.requested_index = "oo-*".to_string();
        let error = decide(&"ab".repeat(32), &input).expect_err("wildcard");
        assert!(error.contains("index"), "{error}");
    }

    #[test]
    fn unauthorized_index_is_refused_before_admit() {
        let mut input = sample("oo-dl01w1-44-ledger", 200, 403);
        input.requested_index = "oo-dl01w1-44-synthetic".to_string();
        let error = precheck(&input).expect_err("index");
        assert!(error.contains("unauthorized index"), "{error}");
    }

    #[test]
    fn unauthorized_field_is_refused_before_admit() {
        let mut input = sample("oo-dl01w1-44-ledger", 200, 403);
        input.requested_fields.push("raw_admin".to_string());
        let error = precheck(&input).expect_err("field");
        assert!(error.contains("unauthorized field"), "{error}");
    }

    #[test]
    fn live_mget_200_is_a_separate_observed_grant() {
        let decision = observed("oo-dl01w1-44-ledger", 403, 200);
        assert!(decision.admitted, "{:?}", decision.blocks);
    }
}
