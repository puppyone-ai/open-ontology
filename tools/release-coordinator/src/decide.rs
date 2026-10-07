//! Pure gate admissibility. No database and no certificate minting.

use crate::canon::canonical_sha;
use serde_json::{json, Value};

pub const GATE_IDS: &[&str] = &[
    "G1", "G2", "G3", "G4", "G5a", "G5b", "G6", "G7", "G8", "G9", "G10", "G11", "G12", "G13",
];

pub const PRODUCTION_EXTERNAL: &[&str] = &[
    "credential",
    "integrity",
    "authenticated_owner",
    "customer",
    "runsc",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GateFact {
    pub gate_id: String,
    pub source_hash: String,
    pub issuer_class: String,
    pub status: String,
    pub blocking: bool,
    pub scope: String,
    pub report_type: String,
    pub authority_statement_sha256: Option<String>,
    pub certificate_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub gate_id: String,
    pub disposition: String,
    pub test_admissible: bool,
    pub production_admissible: bool,
}

pub fn is_gate(gate_id: &str) -> bool {
    GATE_IDS.contains(&gate_id)
}

pub fn is_external(gate_id: &str) -> bool {
    PRODUCTION_EXTERNAL.contains(&gate_id)
}

pub fn production_admissible(_fact: &GateFact) -> bool {
    false
}

pub fn production_outcome(reasons: &[String]) -> &'static str {
    if reasons.is_empty() { "ready" } else { "production_blocked" }
}

pub fn fact_disposition(fact: &GateFact, candidate_source: &str, authority_sha: &str, covered: &[String]) -> &'static str {
    if fact.source_hash != candidate_source {
        return "stale";
    }
    if fact.scope != "test_scope" {
        return "foreign_receipt";
    }
    if fact.issuer_class == "component_authority" {
        if fact.gate_id == "G8" && fact.report_type == "component_recompile" && fact.status == "pass" && !fact.blocking {
            return "admissible";
        }
        return "component_uncovered";
    }
    if fact.status == "not_run" {
        return "not_run";
    }
    if fact.status == "fail" || fact.blocking {
        return "failed";
    }
    match fact.issuer_class.as_str() {
        "test_issuer" => {
            if fact.report_type != "test_issuer" {
                return "foreign_receipt";
            }
            pass_or_warn(fact)
        }
        "fixture_authority" => {
            if fact.report_type != "fixture_certificate" {
                return "foreign_receipt";
            }
            match &fact.authority_statement_sha256 {
                Some(hash) if hash == authority_sha && covered.iter().any(|gate| gate == &fact.gate_id) => pass_or_warn(fact),
                _ => "foreign_receipt",
            }
        }
        _ => "foreign_receipt",
    }
}

fn pass_or_warn(fact: &GateFact) -> &'static str {
    let warn_ok = fact.status == "warn" && matches!(fact.gate_id.as_str(), "G3" | "G4");
    if fact.status == "pass" || warn_ok { "admissible" } else { "failed" }
}

pub fn test_admissible(disposition: &str) -> bool {
    disposition == "admissible"
}

pub fn summarize_gates(facts: &[GateFact], candidate_source: &str, authority_sha: &str, covered: &[String]) -> Vec<Summary> {
    let mut out = Vec::with_capacity(GATE_IDS.len());
    for gate_id in GATE_IDS {
        out.push(summarize_one(gate_id, facts, candidate_source, authority_sha, covered));
    }
    out
}

pub fn summarize_external(facts: &[GateFact], candidate_source: &str, authority_sha: &str, covered: &[String]) -> Vec<Summary> {
    PRODUCTION_EXTERNAL
        .iter()
        .map(|gate_id| summarize_one(gate_id, facts, candidate_source, authority_sha, covered))
        .collect()
}

fn summarize_one(gate_id: &str, facts: &[GateFact], candidate_source: &str, authority_sha: &str, covered: &[String]) -> Summary {
    let rows: Vec<&GateFact> = facts.iter().filter(|fact| fact.gate_id == gate_id).collect();
    if rows.is_empty() {
        return Summary {
            gate_id: gate_id.to_owned(),
            disposition: "missing".to_owned(),
            test_admissible: false,
            production_admissible: false,
        };
    }
    let mut labels = Vec::new();
    for fact in &rows {
        labels.push(fact_disposition(fact, candidate_source, authority_sha, covered));
    }
    let admissible_count = labels.iter().filter(|label| **label == "admissible").count();
    let disagrees = labels.iter().any(|label| matches!(*label, "not_run" | "failed" | "component_uncovered"));
    let disposition = if admissible_count > 1 || (admissible_count == 1 && disagrees) {
        "conflict"
    } else if admissible_count == 1 {
        "admissible"
    } else if labels.len() > 1 {
        "conflict"
    } else {
        labels[0]
    };
    let production = rows.iter().any(|fact| production_admissible(fact));
    Summary {
        gate_id: gate_id.to_owned(),
        disposition: disposition.to_owned(),
        test_admissible: disposition == "admissible",
        production_admissible: production,
    }
}

pub fn certificate_sha(body: &Value) -> String {
    canonical_sha(body)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckpointFact {
    pub kind: String,
    pub source_hash: String,
    pub epoch: String,
    pub barrier: String,
    pub sessions_terminated: bool,
    pub issuer_class: String,
    pub authority_statement_sha256: Option<String>,
    pub certificate_sha256: String,
}

pub fn checkpoint_disposition(fact: &CheckpointFact, candidate_source: &str, authority_sha: &str) -> &'static str {
    if fact.sessions_terminated {
        return "fabricated_session";
    }
    if fact.source_hash != candidate_source {
        return "stale";
    }
    let issuer_ok = match fact.issuer_class.as_str() {
        "test_issuer" => true,
        "fixture_authority" => fact.authority_statement_sha256.as_deref() == Some(authority_sha),
        _ => false,
    };
    if !issuer_ok {
        return "foreign_receipt";
    }
    let fresh = match fact.kind.as_str() {
        "fence" => fact.barrier == "closed",
        "replica" => fact.barrier == "replayed",
        _ => false,
    };
    if fresh { "fresh" } else { "not_fresh" }
}

pub fn plan_document(source_hash: &str, scope: &str, epoch: &str, expected: Option<&str>, certificates: &[String]) -> Value {
    json!({
        "protocol": "oo.release-coordinator/plan-body/v1",
        "source_hash": source_hash,
        "scope": scope,
        "epoch": epoch,
        "expected_version": expected,
        "certificates": certificates,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        checkpoint_disposition, fact_disposition, production_admissible, production_outcome, summarize_gates, CheckpointFact,
        GateFact, GATE_IDS,
    };

    fn fact(gate: &str, status: &str, issuer: &str) -> GateFact {
        GateFact {
            gate_id: gate.to_owned(),
            source_hash: "ab".repeat(32),
            issuer_class: issuer.to_owned(),
            status: status.to_owned(),
            blocking: false,
            scope: "test_scope".to_owned(),
            report_type: if issuer == "test_issuer" { "test_issuer" } else { "fixture_certificate" }.to_owned(),
            authority_statement_sha256: None,
            certificate_sha256: format!("{gate}-{status}"),
        }
    }

    #[test]
    fn missing_not_run_stale_and_foreign_cannot_pass() {
        let source = "ab".repeat(32);
        let empty = summarize_gates(&[], &source, &source, &[]);
        assert!(empty.iter().all(|item| item.disposition == "missing" && !item.test_admissible));
        let mut not_run = fact("G6", "not_run", "test_issuer");
        assert_eq!(fact_disposition(&not_run, &source, &source, &[]), "not_run");
        not_run.source_hash = "cd".repeat(32);
        assert_eq!(fact_disposition(&not_run, &source, &source, &[]), "stale");
        let mut foreign = fact("G9", "pass", "fixture_authority");
        foreign.authority_statement_sha256 = Some("00".repeat(32));
        assert_eq!(fact_disposition(&foreign, &source, &source, &["G9".to_owned()]), "foreign_receipt");
        assert!(!production_admissible(&fact("G8", "pass", "component_authority")));
        assert_eq!(GATE_IDS.len(), 14);
    }

    #[test]
    fn two_passes_conflict_and_one_test_issuer_passes_only_the_test_scope() {
        let source = "ab".repeat(32);
        let mut first = fact("G1", "pass", "test_issuer");
        let mut second = fact("G1", "pass", "test_issuer");
        second.certificate_sha256 = "other".to_owned();
        let rows = vec![first.clone(), second];
        let summary = summarize_gates(&rows, &source, &source, &[]);
        let g1 = summary.iter().find(|item| item.gate_id == "G1").expect("g1");
        assert_eq!(g1.disposition, "conflict");
        assert!(!g1.test_admissible);
        assert!(!g1.production_admissible);
        first.certificate_sha256 = "only".to_owned();
        let one = summarize_gates(&[first], &source, &source, &[]);
        let g1 = one.iter().find(|item| item.gate_id == "G1").expect("g1");
        assert!(g1.test_admissible);
        assert!(!g1.production_admissible);
    }

    #[test]
    fn warn_is_limited_to_g3_and_g4() {
        let source = "ab".repeat(32);
        let g3 = fact("G3", "warn", "test_issuer");
        let g1 = fact("G1", "warn", "test_issuer");
        assert_eq!(fact_disposition(&g3, &source, &source, &[]), "admissible");
        assert_eq!(fact_disposition(&g1, &source, &source, &[]), "failed");
    }

    #[test]
    fn an_empty_production_list_is_ready_and_a_named_gap_is_not() {
        assert_eq!(production_outcome(&[]), "ready");
        assert_eq!(production_outcome(&["G8_not_production_admissible".to_owned()]), "production_blocked");
        assert_eq!(production_outcome(&["credential_missing".to_owned()]), "production_blocked");
        let component = GateFact {
            gate_id: "G8".to_owned(),
            source_hash: "ab".repeat(32),
            issuer_class: "component_authority".to_owned(),
            status: "not_run".to_owned(),
            blocking: false,
            scope: "test_scope".to_owned(),
            report_type: "component_recompile".to_owned(),
            authority_statement_sha256: None,
            certificate_sha256: "g8".to_owned(),
        };
        assert_eq!(fact_disposition(&component, &component.source_hash, "aa", &[]), "component_uncovered");
        assert!(!production_admissible(&component));
    }

    #[test]
    fn not_run_beside_a_pass_is_a_conflict() {
        let source = "ab".repeat(32);
        let pass = fact("G4", "pass", "test_issuer");
        let mut skipped = fact("G4", "not_run", "test_issuer");
        skipped.certificate_sha256 = "skip".to_owned();
        let summary = summarize_gates(&[pass, skipped], &source, &source, &[]);
        let g4 = summary.iter().find(|item| item.gate_id == "G4").expect("g4");
        assert_eq!(g4.disposition, "conflict");
        assert!(!g4.test_admissible);
    }

    #[test]
    fn a_terminated_session_checkpoint_is_not_fresh() {
        let source = "ab".repeat(32);
        let fact = CheckpointFact {
            kind: "fence".to_owned(),
            source_hash: source.clone(),
            epoch: "1".to_owned(),
            barrier: "closed".to_owned(),
            sessions_terminated: true,
            issuer_class: "test_issuer".to_owned(),
            authority_statement_sha256: None,
            certificate_sha256: "fence".to_owned(),
        };
        assert_eq!(checkpoint_disposition(&fact, &source, &source), "fabricated_session");
    }
}
