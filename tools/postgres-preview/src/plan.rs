//! Fail-closed stage plan. A later stage is not scheduled after a stop.

use sha2::{Digest, Sha256};

use crate::error::Fail;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Extract,
    Import,
    Build,
    Preview,
    Detach,
    PreviewLocal,
    Close,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    Stop(Fail),
    Proceed { run_id: String, stages: Vec<Stage> },
}

pub struct PlanInput<'a> {
    pub run_id: &'a str,
    pub prior_run_id: Option<&'a str>,
    pub partial_open: bool,
    pub identity_ok: bool,
    pub type_ok: bool,
    pub rows_observed: u64,
    pub bytes_observed: u64,
    pub max_rows: u64,
    pub max_bytes: u64,
    pub admitted: bool,
    pub budget_unit_unknown: bool,
    pub w38_commit: Option<&'a str>,
    pub decision_source_id: &'a str,
    pub config_source_id: &'a str,
    pub decision_report_sha: &'a str,
    pub session_report_sha: &'a str,
}

pub fn plan_run(input: PlanInput<'_>) -> Plan {
    if input.partial_open {
        return Plan::Stop(Fail::new("partial_not_cleaned", 9));
    }
    if input.prior_run_id.is_some_and(|prior| prior != input.run_id) {
        return Plan::Stop(Fail::new("foreign_run", 9));
    }
    if !input.identity_ok {
        return Plan::Stop(Fail::new("identity", 6));
    }
    if !input.type_ok {
        return Plan::Stop(Fail::new("type_gate", 5));
    }
    if input.rows_observed == 0 {
        return Plan::Stop(Fail::new("no_data", 8));
    }
    if input.rows_observed > input.max_rows || input.bytes_observed > input.max_bytes {
        return Plan::Stop(Fail::new("budget_cap", 10));
    }
    let bound = input.admitted
        && input.decision_source_id == input.config_source_id
        && input.decision_report_sha == input.session_report_sha
        && !input.decision_report_sha.is_empty();
    if !bound {
        if input.budget_unit_unknown || input.w38_commit.is_none() {
            return Plan::Stop(Fail::new("dependency_pending", 3));
        }
        return Plan::Stop(Fail::new("admission_denied", 2));
    }
    let Some(commit) = input.w38_commit else {
        return Plan::Stop(Fail::new("dependency_pending", 3));
    };
    if !is_commit(commit) {
        return Plan::Stop(Fail::new("dependency_pending", 3));
    }
    Plan::Proceed {
        run_id: input.run_id.to_owned(),
        stages: vec![
            Stage::Extract,
            Stage::Import,
            Stage::Build,
            Stage::Preview,
            Stage::Detach,
            Stage::PreviewLocal,
            Stage::Close,
        ],
    }
}

pub fn same_run(seal_run: &str, detach_run: &str) -> bool {
    !seal_run.is_empty() && seal_run == detach_run
}

pub fn model_cache_key(source: &str, import: &str, schema: &str, config: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    hasher.update(b"\n");
    hasher.update(import.as_bytes());
    hasher.update(b"\n");
    hasher.update(schema.as_bytes());
    hasher.update(b"\n");
    hasher.update(config.as_bytes());
    hex(&hasher.finalize())
}

/// HTTP status is not a generation success. Unknown cost stays unknown.
pub fn model_http(status: u16, timed_out: bool, cost_known: bool) -> &'static str {
    if timed_out {
        return "timeout";
    }
    if status == 401 {
        return "unauthorized";
    }
    if !cost_known {
        return "cost_unknown";
    }
    "not_a_generation_success"
}

fn is_commit(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> PlanInput<'static> {
        PlanInput {
            run_id: "run-new",
            prior_run_id: None,
            partial_open: false,
            identity_ok: true,
            type_ok: true,
            rows_observed: 100_000,
            bytes_observed: 1_000,
            max_rows: 100_000,
            max_bytes: 50_000_000,
            admitted: false,
            budget_unit_unknown: true,
            w38_commit: None,
            decision_source_id: "src",
            config_source_id: "src",
            decision_report_sha: "abc",
            session_report_sha: "abc",
        }
    }

    #[test]
    fn missing_producer_does_not_schedule_import() {
        let plan = plan_run(base());
        match plan {
            Plan::Stop(fail) => {
                assert_eq!(fail.token, "dependency_pending");
                assert_eq!(fail.exit, 3);
            }
            Plan::Proceed { .. } => panic!("proceeded without a producer"),
        }
    }

    #[test]
    fn admitted_flag_without_commit_stays_pending() {
        let mut input = base();
        input.admitted = true;
        input.budget_unit_unknown = false;
        assert!(matches!(plan_run(input), Plan::Stop(fail) if fail.token == "dependency_pending"));
    }

    #[test]
    fn historical_runs_do_not_join() {
        assert!(!same_run(
            "3b70b151552ad5d5e905a6148860812a3f84ac82",
            "9187802b6526683e6a8a4980e4bcf2413ed3ffeb"
        ));
        assert!(same_run("run-new", "run-new"));
        let mut input = base();
        input.prior_run_id = Some("3b70b151552ad5d5e905a6148860812a3f84ac82");
        assert!(matches!(plan_run(input), Plan::Stop(fail) if fail.token == "foreign_run"));
    }

    #[test]
    fn caps_identity_and_types_stop() {
        let mut rows = base();
        rows.max_rows = 1;
        assert!(matches!(plan_run(rows), Plan::Stop(fail) if fail.token == "budget_cap"));
        let mut empty = base();
        empty.rows_observed = 0;
        assert!(matches!(plan_run(empty), Plan::Stop(fail) if fail.token == "no_data"));
        let mut identity = base();
        identity.identity_ok = false;
        assert!(matches!(plan_run(identity), Plan::Stop(fail) if fail.token == "identity"));
        let mut kinds = base();
        kinds.type_ok = false;
        assert!(matches!(plan_run(kinds), Plan::Stop(fail) if fail.token == "type_gate"));
        let mut partial = base();
        partial.partial_open = true;
        assert!(matches!(plan_run(partial), Plan::Stop(fail) if fail.token == "partial_not_cleaned"));
    }

    #[test]
    fn cache_key_tracks_import_identity() {
        let left = model_cache_key("source", "import-a", "schema", "config");
        let right = model_cache_key("source", "import-b", "schema", "config");
        assert_ne!(left, right);
        assert_eq!(left.len(), 64);
    }

    #[test]
    fn http_status_is_not_success() {
        assert_eq!(model_http(200, false, false), "cost_unknown");
        assert_eq!(model_http(200, false, true), "not_a_generation_success");
        assert_eq!(model_http(401, false, true), "unauthorized");
        assert_eq!(model_http(200, true, true), "timeout");
    }

    #[test]
    fn recorded_commit_still_requires_admission_bind() {
        let mut input = base();
        input.w38_commit = Some("0123456789abcdef0123456789abcdef01234567");
        input.admitted = true;
        input.budget_unit_unknown = false;
        input.decision_report_sha = "other";
        assert!(matches!(plan_run(input), Plan::Stop(fail) if fail.token == "admission_denied"));
    }
}
