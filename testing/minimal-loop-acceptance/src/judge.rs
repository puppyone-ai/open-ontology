//! Judgment protocol for MP-01 through MP-06.
//!
//! A pass requires measurements taken by the card. A filled-in observation that
//! the card did not measure cannot approve the delivery.

use crate::gold::{self, Cell, DEVELOPMENT_ACK, MODEL_ID, PI_COMMIT};
use crate::json::{self, Value};
use crate::sha256::sha256_hex;

#[derive(Clone, Debug)]
pub struct Observation {
    pub measured: bool,
    pub head: String,
    pub pinned_commit: String,
    pub producer_dirty: bool,
    pub producer_untracked: bool,
    pub a397_committed: bool,
    pub binary_present: bool,
    pub binary_from_archive: bool,
    pub binary_sha_matches: bool,
    pub pi_pins_match: bool,
    pub development_ack: String,
    pub publication: String,
    pub llm_review: String,
    pub rows_party: Option<i64>,
    pub rows_account: Option<i64>,
    pub rows_entry: Option<i64>,
    pub server_version: String,
    pub source_identity: String,
    pub target_identity: String,
    pub source_sha256: String,
    pub pack_source_sha256: String,
    pub pack_hash: String,
    pub pack_hash_second: String,
    pub profile_bound: bool,
    pub peak_connections: Option<i64>,
    pub source_access_after_seal: Option<i64>,
    pub typed_reconciliation: bool,
    pub copy_performed: bool,
    pub manifest_present: bool,
    pub mysql_rejected: Option<bool>,
    pub model_id: String,
    pub cache_first: String,
    pub cache_second: String,
    pub tokens_consistent: bool,
    pub cost_status: String,
    pub generation_outcome: String,
    pub run_draft_ready: bool,
    pub fresh_request: bool,
    pub operator_approval_as_tool: bool,
    pub readme: String,
    pub files: Vec<MeasuredFile>,
    pub questions: Vec<QuestionObs>,
    pub write_reject: FaultObs,
    pub bad_model: FaultObs,
    pub timeout_cancel: FaultObs,
    pub close_pid_absent: Option<bool>,
    pub database_dropped: Option<bool>,
    pub mapping_revoked: Option<bool>,
    pub residue: Option<bool>,
    pub config_secret_free: Option<bool>,
    pub doctor_ran: bool,
    pub run_ran: bool,
    pub ask_ran: bool,
    pub close_ran: bool,
    pub stream_incomplete: bool,
    pub raw_exits_present: bool,
    pub repro_command: String,
    pub inventory_matches: bool,
    pub moving_source_attested: bool,
    pub dependency_probe: bool,
    pub sealed_manifest_present: bool,
    pub copy_review_signed: bool,
    pub product_consumes_kit: bool,
    pub kit_pins_match: bool,
    pub support_plan_recorded: bool,
    pub config_redacted: bool,
    pub copy_from_connection_config: bool,
    pub target_was_preloaded: bool,
    pub source_writes: Option<i64>,
    pub typed_null: bool,
    pub typed_int8: bool,
    pub typed_decimal: bool,
    pub typed_time: Option<bool>,
    pub typed_binary: Option<bool>,
    pub typed_key_coverage: bool,
    pub canonical_hashes_match: bool,
    pub snapshot_mode: String,
    pub fault_cancel_clean: bool,
    pub fault_timeout_clean: bool,
    pub fault_drift_clean: bool,
    pub fault_replay_clean: bool,
    pub no_partial_visibility: bool,
    pub no_source_rewrite: bool,
    pub no_snapshot_splice: bool,
    pub no_duplicate_side_effect: bool,
    pub backend_stopped: bool,
    pub source_unreachable_after_seal: bool,
    pub source_secret_withheld: bool,
    pub local_chain_on_copy: bool,
    pub audit_generation_bound: bool,
    pub source_dropped: Option<bool>,
    pub copy_dropped: Option<bool>,
    pub fixture_source_identity: String,
    pub fixture_target_identity: String,
    pub fixture_identities_distinct: bool,
    pub fixture_source_rows_ok: bool,
    pub fixture_target_empty: bool,
    pub fixture_typed_samples_ok: bool,
    pub fixture_server_ok: bool,
    pub fixture_loader_connections: Option<i64>,
    pub import_hash: String,
    pub author_customer_complete: Option<bool>,
    pub binding_rows_ok: bool,
    pub binding_types_ok: bool,
    pub binding_samples_ok: bool,
    pub binding_partial_denied: bool,
    pub binding_source_connects: Option<i64>,
    pub copy_capability_reviewed: bool,
    pub campaign_provenance_ok: bool,
    pub precomplete_manifest: bool,
    pub precomplete_import_hash: String,
    pub copy_phase_import: String,
    pub copy_phase_snapshot: String,
    pub manifest_snapshot: String,
    pub agent_entry_runtime: bool,
    pub http_release_claimed: bool,
}

#[derive(Clone, Debug)]
pub struct MeasuredFile {
    pub path: String,
    pub body: String,
    pub sha256: String,
}

#[derive(Clone, Debug)]
pub struct QuestionObs {
    pub id: String,
    pub prompt: String,
    pub status: String,
    pub matched: bool,
    pub query_id: String,
    pub answer: String,
    pub cells: Vec<Vec<Cell>>,
    pub tools: Vec<ToolObs>,
    pub proxy_marker_mtls: bool,
    pub proxy_presented_session: bool,
    pub audit_query_id: String,
    pub audit_http: Option<i64>,
    pub audit_database: String,
    pub tool_sql: String,
}

#[derive(Clone, Debug)]
pub struct ToolObs {
    pub name: String,
    pub args: String,
}

#[derive(Clone, Debug, Default)]
pub struct FaultObs {
    pub ran: bool,
    pub incomplete: bool,
    pub exit: Option<i32>,
    pub http_status: Option<i64>,
    pub status: String,
}

#[derive(Clone, Debug)]
pub struct Check {
    pub id: String,
    pub status: String,
    pub reason: String,
}

#[derive(Clone, Debug)]
pub struct Verdict {
    pub passed: bool,
    pub exit_code: u8,
    pub independent_approval: bool,
    pub parent_acceptance: bool,
    pub goal_acceptance: bool,
    pub runsc_certified: bool,
    pub arbitrary_database: bool,
    pub full_plan: bool,
    pub checks: Vec<Check>,
}

impl Observation {
    pub fn unmeasured() -> Self {
        Self {
            measured: false,
            head: String::new(),
            pinned_commit: String::new(),
            producer_dirty: false,
            producer_untracked: false,
            a397_committed: false,
            binary_present: false,
            binary_from_archive: false,
            binary_sha_matches: false,
            pi_pins_match: false,
            development_ack: String::new(),
            publication: String::new(),
            llm_review: String::new(),
            rows_party: None,
            rows_account: None,
            rows_entry: None,
            server_version: String::new(),
            source_identity: String::new(),
            target_identity: String::new(),
            source_sha256: String::new(),
            pack_source_sha256: String::new(),
            pack_hash: String::new(),
            pack_hash_second: String::new(),
            profile_bound: false,
            peak_connections: None,
            source_access_after_seal: None,
            typed_reconciliation: false,
            copy_performed: false,
            manifest_present: false,
            mysql_rejected: None,
            model_id: String::new(),
            cache_first: String::new(),
            cache_second: String::new(),
            tokens_consistent: false,
            cost_status: String::new(),
            generation_outcome: String::new(),
            run_draft_ready: false,
            fresh_request: false,
            operator_approval_as_tool: false,
            readme: String::new(),
            files: Vec::new(),
            questions: Vec::new(),
            write_reject: FaultObs::default(),
            bad_model: FaultObs::default(),
            timeout_cancel: FaultObs::default(),
            close_pid_absent: None,
            database_dropped: None,
            mapping_revoked: None,
            residue: None,
            config_secret_free: None,
            doctor_ran: false,
            run_ran: false,
            ask_ran: false,
            close_ran: false,
            stream_incomplete: false,
            raw_exits_present: false,
            repro_command: String::new(),
            inventory_matches: false,
            moving_source_attested: false,
            dependency_probe: false,
            sealed_manifest_present: false,
            copy_review_signed: false,
            product_consumes_kit: false,
            kit_pins_match: false,
            support_plan_recorded: false,
            config_redacted: false,
            copy_from_connection_config: false,
            target_was_preloaded: false,
            source_writes: None,
            typed_null: false,
            typed_int8: false,
            typed_decimal: false,
            typed_time: None,
            typed_binary: None,
            typed_key_coverage: false,
            canonical_hashes_match: false,
            snapshot_mode: String::new(),
            fault_cancel_clean: false,
            fault_timeout_clean: false,
            fault_drift_clean: false,
            fault_replay_clean: false,
            no_partial_visibility: false,
            no_source_rewrite: false,
            no_snapshot_splice: false,
            no_duplicate_side_effect: false,
            backend_stopped: false,
            source_unreachable_after_seal: false,
            source_secret_withheld: false,
            local_chain_on_copy: false,
            audit_generation_bound: false,
            source_dropped: None,
            copy_dropped: None,
            fixture_source_identity: String::new(),
            fixture_target_identity: String::new(),
            fixture_identities_distinct: false,
            fixture_source_rows_ok: false,
            fixture_target_empty: false,
            fixture_typed_samples_ok: false,
            fixture_server_ok: false,
            fixture_loader_connections: None,
            import_hash: String::new(),
            author_customer_complete: None,
            binding_rows_ok: false,
            binding_types_ok: false,
            binding_samples_ok: false,
            binding_partial_denied: false,
            binding_source_connects: None,
            copy_capability_reviewed: false,
            campaign_provenance_ok: false,
            precomplete_manifest: false,
            precomplete_import_hash: String::new(),
            copy_phase_import: String::new(),
            copy_phase_snapshot: String::new(),
            manifest_snapshot: String::new(),
            agent_entry_runtime: false,
            http_release_claimed: false,
        }
    }
}

impl Verdict {
    pub fn json(&self) -> String {
        let checks = Value::Array(self.checks.iter().map(check_json).collect());
        json::stringify(&json::object(vec![
            ("protocol", json::string("oo.minimal-loop-acceptance.verdict/v1")),
            ("passed", Value::Bool(self.passed)),
            ("exit_code", Value::Number(self.exit_code.to_string())),
            ("independent_approval", Value::Bool(self.independent_approval)),
            ("parent_acceptance", Value::Bool(false)),
            ("goal_acceptance", Value::Bool(false)),
            ("runsc_certified", Value::Bool(false)),
            ("arbitrary_database", Value::Bool(false)),
            ("full_plan", Value::Bool(false)),
            ("development_only", json::string("operator-development")),
            ("http_release_acceptance", Value::Bool(false)),
            ("checks", checks),
        ]))
    }
}

pub fn judge(obs: &Observation) -> Verdict {
    if !obs.measured {
        return verdict(vec![check("archive", "not_run", "the card did not measure this observation")]);
    }
    if obs.stream_incomplete {
        return verdict(vec![check("archive", "incomplete", "a command stream broke before the card could finish")]);
    }
    let mp01a = mp01a(obs);
    let mp01b = mp01b(obs);
    let mp01c = mp01c(obs);
    let mp01d = mp01d(obs);
    let mp01 = mp01_from(&[&mp01a, &mp01b, &mp01c, &mp01d]);
    let checks = vec![archive(obs), mp01a, mp01b, mp01c, mp01d, mp01, mp02(obs), mp03(obs), mp04(obs), mp05(obs), mp06(obs)];
    verdict(checks)
}

fn verdict(checks: Vec<Check>) -> Verdict {
    let passed = checks.iter().all(|item| item.status == "pass");
    let exit_code = if checks.iter().any(|item| item.status == "fail") {
        1
    } else if checks.iter().any(|item| item.status == "incomplete" || item.status == "not_run" || item.status == "pending") {
        4
    } else if passed {
        0
    } else {
        1
    };
    Verdict {
        passed,
        exit_code,
        independent_approval: passed,
        parent_acceptance: false,
        goal_acceptance: false,
        runsc_certified: false,
        arbitrary_database: false,
        full_plan: false,
        checks,
    }
}

fn archive(obs: &Observation) -> Check {
    let mut problems = Vec::new();
    if obs.moving_source_attested {
        problems.push("moving source was attested");
    }
    if !commit_ok(&obs.pinned_commit) || obs.head != obs.pinned_commit {
        problems.push("the pinned commit is not the measured archive commit");
    }
    if !obs.inventory_matches {
        problems.push("the archive does not match the pinned product inventory");
    }
    if obs.agent_entry_runtime {
        problems.push("tools/agent-entry was treated as a runtime dependency");
    }
    if obs.http_release_claimed {
        problems.push("an HTTP release was claimed");
    }
    if !obs.binary_present || !obs.binary_from_archive || !obs.binary_sha_matches {
        problems.push("no binary built from the fixed archive was measured");
    }
    if problems.is_empty() {
        check("archive", "pass", "the immutable archive matches the selected product inventory and the binary")
    } else {
        check("archive", "fail", &problems.join("; "))
    }
}

fn mp01_from(parts: &[&Check]) -> Check {
    if parts.iter().all(|item| item.status == "pending") {
        return check("MP-01", "pending", "MP-01a through MP-01d are pending");
    }
    let tuples: Vec<(&str, String)> = parts.iter().map(|item| (item.status.as_str(), item.reason.clone())).collect();
    fold("MP-01", &tuples)
}

fn mp01a(obs: &Observation) -> Check {
    if let Some(early) = not_executed(obs, "MP-01a", "the source connection copy was not executed", obs.doctor_ran || obs.run_ran) {
        return early;
    }
    let parts = vec![
        flag(obs.mysql_rejected == Some(true), "mysql was not rejected"),
        flag(
            obs.support_plan_recorded && obs.config_redacted && obs.config_secret_free == Some(true),
            "support, plan, or the redacted config was not measured",
        ),
        flag(obs.copy_from_connection_config, "the copy did not start from the source connection config"),
        flag(!obs.target_was_preloaded, "the target was a preloaded fixture"),
        flag(identities_distinct(obs), "source and target identities are not distinct"),
        flag(obs.server_version.starts_with("17.10"), "server version is not 17.10"),
    ];
    fold("MP-01a", &parts)
}

fn mp01b(obs: &Observation) -> Check {
    if let Some(early) = not_executed(obs, "MP-01b", "the copy was not executed", obs.run_ran) {
        return early;
    }
    let parts = vec![
        flag(obs.copy_performed && obs.copy_from_connection_config, "the run did not show a copy from the source connection"),
        flag(!obs.target_was_preloaded, "a preloaded target fixture was used as the copy"),
        flag(row_counts_ok(obs), "row counts are not 2000/8000/90000"),
        flag(obs.peak_connections == Some(1), "source connection peak is not 1"),
        flag(obs.source_writes == Some(0), "source writes were not measured as zero"),
        flag(obs.snapshot_mode == "repeatable_read_read_only", "snapshot mode is not repeatable read read only"),
        flag(obs.typed_reconciliation, "typed reconciliation was not measured"),
        flag(obs.typed_null && obs.typed_int8 && obs.typed_decimal, "null, int8, or decimal reconciliation is missing"),
        flag(obs.typed_time == Some(true), "time type was not reconciled"),
        flag(obs.typed_binary == Some(true), "binary type was not reconciled"),
        flag(obs.typed_key_coverage && obs.canonical_hashes_match, "key coverage or canonical hashes do not match"),
        flag(obs.manifest_present, "the import manifest is absent"),
        flag(sha_ok(&obs.source_sha256) && obs.source_sha256 == obs.pack_source_sha256, "the source hash is not bound to the pack"),
    ];
    fold("MP-01b", &parts)
}

fn mp01c(obs: &Observation) -> Check {
    if let Some(early) = not_executed(obs, "MP-01c", "copy fault behavior was not executed", obs.run_ran) {
        return early;
    }
    let parts = vec![
        flag(obs.fault_cancel_clean, "cancel did not stay non-public"),
        flag(obs.fault_timeout_clean, "timeout did not stay non-public"),
        flag(obs.fault_drift_clean, "drift reused an old snapshot"),
        flag(obs.fault_replay_clean, "replay duplicated target rows"),
        flag(obs.no_partial_visibility, "partial rows were visible"),
        flag(obs.no_source_rewrite, "the source was rewritten"),
        flag(obs.no_snapshot_splice, "old and new snapshots were spliced"),
        flag(obs.no_duplicate_side_effect, "the target recorded a duplicate side effect"),
        flag(obs.backend_stopped, "the source backend was not shown to stop"),
    ];
    fold("MP-01c", &parts)
}

fn mp01d(obs: &Observation) -> Check {
    if let Some(early) = not_executed(obs, "MP-01d", "the sealed local chain was not executed", obs.ask_ran || obs.close_ran) {
        return early;
    }
    if !(obs.ask_ran && obs.close_ran) {
        return check("MP-01d", "fail", "the sealed local chain did not finish");
    }
    let audits_local = !obs.questions.is_empty()
        && obs.questions.iter().all(|item| item.audit_database == obs.target_identity && !item.audit_database.is_empty());
    let typed_binding = obs.binding_rows_ok
        && obs.binding_types_ok
        && obs.binding_samples_ok
        && obs.binding_partial_denied
        && obs.binding_source_connects == Some(0);
    let parts = vec![
        flag(obs.copy_capability_reviewed, "the independent copy capability review is absent"),
        flag(obs.sealed_manifest_present, "the selected campaign has no sealed manifest"),
        flag(obs.campaign_provenance_ok, "the selected campaign provenance is not the sealed snapshot"),
        flag(typed_binding, "the selected campaign typed binding is absent"),
        flag(sha_ok(&obs.import_hash), "the campaign import hash was not measured"),
        flag(obs.source_unreachable_after_seal, "the source was still reachable after seal"),
        flag(obs.source_secret_withheld, "the source secret was still available to a later process"),
        flag(obs.local_chain_on_copy, "self-build and the Pi chain did not use the sealed local copy"),
        flag(obs.source_access_after_seal == Some(0), "source access after seal is not 0"),
        flag(obs.audit_generation_bound, "query audits are not bound to one import generation"),
        flag(audits_local, "a query audit does not name the local target"),
        flag(obs.source_dropped == Some(false) && obs.copy_dropped == Some(false), "close dropped the source or the persisted copy"),
        flag(obs.database_dropped == Some(false), "close dropped the database"),
    ];
    fold("MP-01d", &parts)
}

fn mp02(obs: &Observation) -> Check {
    if let Some(early) = not_executed(obs, "MP-02", "run was not executed", obs.run_ran) {
        return early;
    }
    let parts = vec![
        flag(obs.model_id == MODEL_ID, "the model is not grok-4.7"),
        flag(obs.fresh_request && obs.cache_first == "miss", "the first model call is not a recorded miss"),
        flag(obs.cache_second == "hit", "the second run did not reuse the cache"),
        flag(obs.tokens_consistent, "token counts are absent or inconsistent"),
        flag(obs.cost_status == "unknown" || decimal_ok(&obs.cost_status), "cost status is absent"),
        flag(obs.generation_outcome == "validated" || obs.run_draft_ready, "the generation is not validated"),
        flag(obs.llm_review == "unreviewed" && obs.publication == "not_published", "the draft is marked reviewed or published"),
        flag(!obs.operator_approval_as_tool, "operator approval was used as a tool"),
        flag(obs.profile_bound && obs.development_ack == DEVELOPMENT_ACK, "the profile is not bound to this development database"),
        flag(pack_files_ok(obs), "the pack README, YAML, or file hash does not match"),
        flag(!obs.pack_hash.is_empty() && obs.pack_hash == obs.pack_hash_second, "the two runs do not share one pack hash"),
    ];
    fold("MP-02", &parts)
}

fn mp03(obs: &Observation) -> Check {
    if let Some(early) = not_executed(obs, "MP-03", "ask was not executed", obs.ask_ran) {
        return early;
    }
    if obs.questions.is_empty() {
        return check("MP-03", "fail", "ask produced no Pi tool record");
    }
    let mut parts = vec![flag(
        obs.pi_pins_match && obs.questions.iter().all(|item| item.tools.iter().any(|tool| tool.name == "read")),
        "Pi read of the pack was not recorded",
    )];
    for question in &obs.questions {
        parts.push(question_tools(question));
    }
    fold("MP-03", &parts)
}

fn mp04(obs: &Observation) -> Check {
    if !gold::sealed_prompts() {
        return check("MP-04", "fail", "the frozen prompts contain SQL or a gold number");
    }
    if let Some(early) = not_executed(obs, "MP-04", "ask was not executed", obs.ask_ran) {
        return early;
    }
    let mut parts = Vec::new();
    for expected in gold::frozen_questions() {
        let Some(question) = obs.questions.iter().find(|item| item.id == expected.id) else {
            parts.push(flag(false, "a frozen question is missing"));
            continue;
        };
        parts.push(flag(answer_cited(question, expected.id), "a frozen answer does not match gold or its query id"));
    }
    fold("MP-04", &parts)
}

fn mp05(obs: &Observation) -> Check {
    let mut parts = vec![
        fault_rejected(obs, &obs.write_reject, "write"),
        fault_rejected(obs, &obs.bad_model, "bad model"),
        fault_rejected(obs, &obs.timeout_cancel, "timeout"),
    ];
    if !obs.close_ran {
        parts.push(if obs.dependency_probe {
            ("pending", pending_reason(obs))
        } else {
            ("not_run", "close was not executed".to_owned())
        });
    } else {
        parts.push(match obs.close_pid_absent {
            Some(true) => flag(true, "pid"),
            Some(false) => flag(false, "the pi process was still present"),
            None => flag(false, "close did not report the process"),
        });
        parts.push(match obs.database_dropped {
            Some(false) => flag(true, "database"),
            Some(true) => flag(false, "close dropped the database"),
            None => flag(false, "close did not report the database"),
        });
        parts.push(match obs.mapping_revoked {
            Some(true) => flag(true, "mapping"),
            _ => flag(false, "the proxy mapping was not revoked"),
        });
        parts.push(match obs.residue {
            Some(false) => flag(true, "residue"),
            _ => flag(false, "close left a session residue"),
        });
        parts.push(match (obs.source_dropped, obs.copy_dropped) {
            (Some(false), Some(false)) => flag(true, "kept"),
            (Some(true), _) => flag(false, "close dropped the source"),
            (_, Some(true)) => flag(false, "close dropped the persisted copy"),
            _ => flag(false, "close did not report the source and the persisted copy"),
        });
    }
    fold("MP-05", &parts)
}

fn mp06(obs: &Observation) -> Check {
    let any_ran = obs.doctor_ran || obs.run_ran || obs.ask_ran || obs.close_ran;
    if let Some(early) = not_executed(obs, "MP-06", "the command set was not executed", any_ran) {
        return early;
    }
    let parts = vec![
        flag(obs.doctor_ran && obs.run_ran && obs.ask_ran && obs.close_ran, "a command was not executed"),
        flag(obs.raw_exits_present, "a raw exit is missing"),
        flag(!obs.repro_command.is_empty() && !obs.repro_command.contains("password="), "the repro command is missing or carries a secret"),
        flag(obs.config_secret_free == Some(true), "the config was not shown to be free of secret values"),
        flag(obs.cache_second == "hit" && obs.pack_hash == obs.pack_hash_second && !obs.pack_hash.is_empty(), "the rerun did not reuse the same pack"),
        flag(commit_ok(&obs.pinned_commit) && obs.head == obs.pinned_commit, "the commit was not pinned"),
        flag(obs.development_ack == DEVELOPMENT_ACK, "the development acknowledgement is absent"),
    ];
    fold("MP-06", &parts)
}

fn question_tools(question: &QuestionObs) -> (&'static str, String) {
    if gold::prompt_leaks(&question.prompt)
        || (question.prompt.contains(&question.tool_sql) && !question.tool_sql.is_empty())
    {
        return flag(false, "the prompt contains SQL or a gold number");
    }
    if question.proxy_presented_session || !question.proxy_marker_mtls {
        return flag(false, "the proxy did not inject over mTLS, or the client presented the session token");
    }
    if question.audit_query_id != question.query_id || question.audit_http != Some(200) || question.audit_database.is_empty() {
        return flag(false, "the gateway audit does not match the query id");
    }
    let read_at = question.tools.iter().position(|tool| tool.name == "read" && tool.args.contains("README"));
    let bash_at = question.tools.iter().position(|tool| tool.name == "bash" && tool.args.contains("query"));
    let ordered = match (read_at, bash_at) {
        (Some(read), Some(bash)) => read < bash,
        _ => false,
    };
    let fake = question.tools.iter().any(|tool| {
        let name = tool.name.to_ascii_lowercase();
        name.contains("approval") || name.contains("proposal")
    });
    let sql_ok = question.tool_sql.to_ascii_lowercase().contains("select") && !question.tool_sql.trim().is_empty();
    flag(ordered && !fake && sql_ok, "Pi did not read the pack and then run its own SQL through oo")
}

fn fault_rejected<'a>(obs: &'a Observation, fault: &FaultObs, label: &str) -> (&'a str, String) {
    if !fault.ran {
        return if obs.dependency_probe {
            ("pending", pending_reason(obs))
        } else {
            ("not_run", format!("{label} was not executed"))
        };
    }
    if fault.incomplete {
        return ("incomplete", format!("{label} stream broke"));
    }
    if gold::false_success(&fault.status, fault.exit, fault.http_status) {
        ("fail", format!("{label} was reported as success"))
    } else if fault.exit.is_some_and(|code| code != 0) {
        ("pass", format!("{label} failed closed"))
    } else {
        ("fail", format!("{label} had no failing exit"))
    }
}

fn not_executed(obs: &Observation, id: &str, reason: &str, ran: bool) -> Option<Check> {
    if ran {
        None
    } else if obs.dependency_probe {
        Some(check(id, "pending", &pending_reason(obs)))
    } else {
        Some(check(id, "not_run", reason))
    }
}

fn pending_reason(obs: &Observation) -> String {
    let mut parts = Vec::new();
    if !obs.copy_capability_reviewed {
        parts.push("the independent copy capability review is absent");
    }
    if parts.is_empty() {
        parts.push("the selected campaign was not executed");
    }
    parts.join("; ")
}

/// A fresh user-CLI copy is a discover begin and a copy begin, both from the
/// source connection, followed by a copy complete for a new import and a
/// non-empty snapshot. A replay token does not count. A hash that was already
/// complete does not count. The card still has to bind that pair to the
/// sealed manifest re-read after `run`.
pub fn fresh_copy_phase(document: &Value, precomplete_hash: &str) -> Option<(String, String)> {
    let phases = document.get("phases").or_else(|| document.get("copy_phases")).and_then(Value::as_array)?;
    let mut discover_begin = false;
    let mut copy_begin = false;
    for phase in phases {
        let name = phase.get("name").and_then(Value::as_str).unwrap_or("");
        let event = phase.get("event").and_then(Value::as_str).unwrap_or("");
        let from = phase.get("from").and_then(Value::as_str).unwrap_or("");
        let failure = phase.get("failure").and_then(Value::as_str).unwrap_or("");
        if failure == "replay_complete" || event == "replay_complete" {
            continue;
        }
        if event == "begin" && from == "source_connection" && name == "discover" {
            discover_begin = true;
        }
        if event == "begin" && from == "source_connection" && name == "copy" {
            copy_begin = true;
        }
        if name == "copy" && event == "complete" && discover_begin && copy_begin {
            let hash = phase.get("import_hash").and_then(Value::as_str).unwrap_or("");
            let snapshot = phase.get("snapshot_id").and_then(Value::as_str).unwrap_or("");
            let state = phase.get("mirror_state").and_then(Value::as_str).unwrap_or("");
            let customer = phase.get("customer_complete").and_then(Value::as_bool);
            if failure == "none"
                && state == "complete"
                && customer == Some(false)
                && sha_ok(hash)
                && hash != precomplete_hash
                && !snapshot.is_empty()
            {
                return Some((hash.to_owned(), snapshot.to_owned()));
            }
        }
    }
    None
}

pub fn fresh_copy_performed(document: &Value, precomplete_hash: &str) -> bool {
    fresh_copy_phase(document, precomplete_hash).is_some()
}

fn answer_cited(question: &QuestionObs, id: &str) -> bool {
    let Some(token) = gold::answer_token(id) else {
        return false;
    };
    if question.answer.is_empty() || question.query_id.is_empty() {
        return false;
    }
    if !question.answer.contains(&question.query_id) || !question.answer.contains(token) {
        return false;
    }
    if question.audit_query_id != question.query_id || question.audit_http != Some(200) || question.audit_database.is_empty() {
        return false;
    }
    if gold::expected_cells(id).is_some_and(|cells| cells == question.cells) {
        return true;
    }
    id == "id-overlap" && id_overlap_join_result(&question.tool_sql, &question.cells)
}

/// `id-overlap` may be a count cell of 2000, a wider joined row that still
/// carries that count, or a 2000-row join listing. A single joined pair does
/// not. The SQL has to be a join when the cells are not the exact gold shape.
pub fn id_overlap_join_result(sql: &str, cells: &[Vec<Cell>]) -> bool {
    if !sql.to_ascii_lowercase().contains("join") {
        return false;
    }
    let Some(token) = gold::answer_token("id-overlap") else {
        return false;
    };
    let carries_count = cells.iter().any(|row| row.iter().any(|cell| matches!(cell, Cell::Text(text) if text == token)));
    let listing = cells.len() == 2000 && cells.iter().all(|row| !row.is_empty());
    carries_count || listing
}

fn identities_distinct(obs: &Observation) -> bool {
    !obs.source_identity.is_empty() && !obs.target_identity.is_empty() && obs.source_identity != obs.target_identity
}

fn row_counts_ok(obs: &Observation) -> bool {
    obs.rows_party == Some(gold::PARTY_ROWS) && obs.rows_account == Some(gold::ACCOUNT_ROWS) && obs.rows_entry == Some(gold::ENTRY_ROWS)
}

fn pack_files_ok(obs: &Observation) -> bool {
    if !obs.readme.starts_with("# DRAFT — unreviewed — not published") {
        return false;
    }
    if !obs.readme.contains(&obs.source_sha256) || !obs.readme.contains("unreviewed") || !obs.readme.contains("not_published") {
        return false;
    }
    let needed = ["README.md", "ontology.yaml", "party.yaml", "account.yaml", "entry.yaml"];
    needed.iter().all(|name| {
        obs.files.iter().any(|file| {
            file.path.ends_with(name)
                && file.sha256 == sha256_hex(file.body.as_bytes())
                && file.body.contains(&obs.source_sha256)
                && file.body.contains("unreviewed")
                && file.body.contains("not_published")
        })
    })
}

fn fold(id: &str, parts: &[(&str, String)]) -> Check {
    if parts.iter().any(|(status, _)| *status == "fail") {
        let reason = parts.iter().filter(|(status, _)| *status == "fail").map(|(_, reason)| reason.as_str()).collect::<Vec<_>>().join("; ");
        return check(id, "fail", &reason);
    }
    if parts.iter().any(|(status, _)| *status == "incomplete") {
        let reason = parts.iter().filter(|(status, _)| *status == "incomplete").map(|(_, reason)| reason.as_str()).collect::<Vec<_>>().join("; ");
        return check(id, "incomplete", &reason);
    }
    if parts.iter().any(|(status, _)| *status == "pending") {
        let reason = parts.iter().filter(|(status, _)| *status == "pending").map(|(_, reason)| reason.as_str()).collect::<Vec<_>>().join("; ");
        return check(id, "pending", &reason);
    }
    if parts.iter().any(|(status, _)| *status == "not_run") {
        let reason = parts.iter().filter(|(status, _)| *status == "not_run").map(|(_, reason)| reason.as_str()).collect::<Vec<_>>().join("; ");
        return check(id, "not_run", &reason);
    }
    if !parts.is_empty() && parts.iter().all(|(status, _)| *status == "pass") {
        return check(id, "pass", "measured evidence matches");
    }
    check(id, "fail", "the measurement is incomplete")
}

fn flag(ok: bool, reason: &str) -> (&'static str, String) {
    if ok { ("pass", reason.to_owned()) } else { ("fail", reason.to_owned()) }
}

fn check(id: &str, status: &str, reason: &str) -> Check {
    Check { id: id.to_owned(), status: status.to_owned(), reason: reason.to_owned() }
}

fn commit_ok(text: &str) -> bool {
    text.len() == 40 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn sha_ok(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn decimal_ok(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let rest = if first == '-' { chars.as_str() } else { text };
    let mut dot = false;
    !rest.is_empty()
        && rest.chars().all(|char| {
            if char == '.' && !dot {
                dot = true;
                true
            } else {
                char.is_ascii_digit()
            }
        })
}

fn check_json(item: &Check) -> Value {
    json::object(vec![
        ("id", json::string(&item.id)),
        ("status", json::string(&item.status)),
        ("reason", json::string(&item.reason)),
    ])
}

pub fn config_is_secret_free(text: &str) -> bool {
    let Ok(value) = json::parse(text) else {
        return false;
    };
    secret_free(&value)
}

fn secret_free(value: &Value) -> bool {
    const FORBIDDEN: &[&str] = &["api_key", "bearer", "password", "secret", "token"];
    match value {
        Value::Object(pairs) => pairs.iter().all(|(key, child)| !FORBIDDEN.contains(&key.as_str()) && secret_free(child)),
        Value::Array(items) => items.iter().all(secret_free),
        _ => true,
    }
}

pub fn pi_commit_ok(commit: &str) -> bool {
    commit == PI_COMMIT
}
