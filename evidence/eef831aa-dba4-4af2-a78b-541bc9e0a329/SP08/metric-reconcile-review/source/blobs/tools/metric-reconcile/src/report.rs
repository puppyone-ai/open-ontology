//! Operator report. Field order is the receipt order.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const CONTRACT: &str = "metric-reconcile/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub contract: String,
    pub outcome: String,
    pub owner_authenticated: bool,
    pub metric_certified: bool,
    pub publication: bool,
    pub full_gates: bool,
    pub database_used: bool,
    pub source_modified: bool,
    pub reason: String,
    pub gates: Vec<Gate>,
    pub omitted_gates: Vec<OmittedGate>,
    pub lifecycle: Lifecycle,
    pub provenance: Provenance,
    pub enum_resolution: EnumResolution,
    pub semantic_diff: SemanticDiff,
    pub metric_query: QueryReport,
    pub control_query: QueryReport,
    pub comparison: Comparison,
    pub tests: Vec<TestReport>,
    pub explain: ExplainReport,
    pub values: Values,
    pub receipt_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gate {
    pub gate_id: String,
    pub status: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OmittedGate {
    pub gate_id: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lifecycle {
    pub status: Option<String>,
    pub owner: Option<String>,
    pub synonyms: Vec<String>,
    pub replaced_by: Option<String>,
    pub replaced_by_core_field: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub owner: Option<String>,
    pub source: String,
    pub anchor_name: String,
    pub anchor_date: Option<String>,
    pub project: String,
    pub role: String,
    pub policy_id: String,
    pub knowledge_id: String,
    pub password_logged: bool,
    pub host: Option<String>,
    pub database: Option<String>,
    pub user: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnumResolution {
    pub certification_allowed: bool,
    pub classes: Vec<ClassReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassReport {
    pub enum_name: String,
    pub class_name: String,
    pub codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticDiff {
    pub owner_review_required: bool,
    pub changed: Vec<String>,
    pub metric_sql_sha256: String,
    pub previous_metric_sql_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryReport {
    pub raw_sql_sha256: String,
    pub bound_sql: String,
    pub guard_fingerprint: String,
    pub executed_sql_sha256: String,
    pub executed_sql: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Comparison {
    pub status: String,
    pub reason: String,
    pub difference: Option<String>,
    pub tolerance_abs: Option<String>,
    pub tolerance_rel: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestReport {
    pub index: u32,
    pub status: String,
    pub reason: String,
    pub expect_rows: Option<u64>,
    pub actual_rows: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExplainReport {
    pub status: String,
    pub reason: String,
    pub rows: u32,
    pub text_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Values {
    pub metric: Option<String>,
    pub control: Option<String>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

pub fn omitted_gates() -> Vec<OmittedGate> {
    ["G1", "G2", "G3", "G4", "G6", "G7", "G8", "G9", "G10", "G11", "G12", "G13"]
        .into_iter()
        .map(|gate_id| OmittedGate {
            gate_id: gate_id.to_owned(),
            status: "not_run".to_owned(),
        })
        .collect()
}

pub fn blank() -> Report {
    Report {
        contract: CONTRACT.to_owned(),
        outcome: "refused".to_owned(),
        owner_authenticated: false,
        metric_certified: false,
        publication: false,
        full_gates: false,
        database_used: false,
        source_modified: false,
        reason: "ok".to_owned(),
        gates: vec![
            Gate {
                gate_id: "G5a".to_owned(),
                status: "block".to_owned(),
                reason: "not_executed".to_owned(),
            },
            Gate {
                gate_id: "G5b".to_owned(),
                status: "block".to_owned(),
                reason: "not_executed".to_owned(),
            },
        ],
        omitted_gates: omitted_gates(),
        lifecycle: Lifecycle {
            status: None,
            owner: None,
            synonyms: Vec::new(),
            replaced_by: None,
            replaced_by_core_field: false,
        },
        provenance: Provenance {
            owner: None,
            source: String::new(),
            anchor_name: String::new(),
            anchor_date: None,
            project: String::new(),
            role: String::new(),
            policy_id: String::new(),
            knowledge_id: String::new(),
            password_logged: false,
            host: None,
            database: None,
            user: None,
        },
        enum_resolution: EnumResolution {
            certification_allowed: false,
            classes: Vec::new(),
        },
        semantic_diff: SemanticDiff {
            owner_review_required: false,
            changed: Vec::new(),
            metric_sql_sha256: String::new(),
            previous_metric_sql_sha256: None,
        },
        metric_query: QueryReport::blank(),
        control_query: QueryReport::blank(),
        comparison: Comparison {
            status: "block".to_owned(),
            reason: "not_executed".to_owned(),
            difference: None,
            tolerance_abs: None,
            tolerance_rel: None,
        },
        tests: Vec::new(),
        explain: ExplainReport {
            status: "not_executed".to_owned(),
            reason: "not_executed".to_owned(),
            rows: 0,
            text_sha256: String::new(),
        },
        values: Values {
            metric: None,
            control: None,
        },
        receipt_sha256: String::new(),
    }
}

impl QueryReport {
    pub fn blank() -> Self {
        Self {
            raw_sql_sha256: String::new(),
            bound_sql: String::new(),
            guard_fingerprint: String::new(),
            executed_sql_sha256: String::new(),
            executed_sql: String::new(),
        }
    }
}

pub fn seal(report: &mut Report) {
    report.receipt_sha256.clear();
    let bytes = serde_json::to_vec(report).expect("report serializes");
    report.receipt_sha256 = sha256_hex(&bytes);
}

pub fn fold_status(parts: &[(&'static str, &'static str)]) -> (&'static str, &'static str) {
    for (status, reason) in parts {
        if *status == "failed" {
            return ("failed", reason);
        }
    }
    for (status, reason) in parts {
        if *status == "block" {
            return ("block", reason);
        }
    }
    ("passed", "ok")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_is_the_empty_field_hash() {
        let mut report = blank();
        report.outcome = "compiled".to_owned();
        report.reason = "ok".to_owned();
        seal(&mut report);
        let bytes = serde_json::to_vec(&report).expect("bytes");
        let mut again: Report = serde_json::from_slice(&bytes).expect("parse");
        assert_eq!(again.receipt_sha256, report.receipt_sha256);
        again.receipt_sha256.clear();
        let cleared = serde_json::to_vec(&again).expect("cleared");
        assert_eq!(sha256_hex(&cleared), report.receipt_sha256);
        assert!(report.receipt_sha256.len() == 64);
    }
}
