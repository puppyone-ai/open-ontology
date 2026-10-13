//! Period scope report for the ten source products.
//! Historical exits, author booleans, and CLI status 0 are not CC probes.
//! The matrix is not rewritten and no grade is promoted.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::audit::parse_audit;
use crate::cases::CASES;
use crate::evaluate::Finding;
use crate::gitobj::{git_ref, valid_commit, GitRef, ObjectStore};
use crate::matrix::{self, is_runtime};

pub const SCHEMA: &str = "oo-source-conformance/scope-report/v1";

const REVIEW_75: &str = "execution/reviews/dl01-wave1-publish-maintenance-fixed-review/REVIEW.md";
const ELF_75: &str = "evidence/67a28659-717e-417f-a26b-2f981ae5c64c/DL01/publish-maintenance-fixed-review/linux/rework-699ca96/probe-elf.file";
const EXIT_75: &str = "evidence/67a28659-717e-417f-a26b-2f981ae5c64c/DL01/publish-maintenance-fixed-review/linux/rework-699ca96/probe.exit";
const REVIEW_76: &str = "execution/reviews/dl01-wave1-public-gateway-fixed-review/REVIEW.md";
const ELF_76: &str = "evidence/72f2dc43-1371-481f-bbe2-85e9765847c6/DL01/public-gateway-fixed-review/identity/file.txt";
const EXIT_76: &str = "evidence/72f2dc43-1371-481f-bbe2-85e9765847c6/DL01/public-gateway-fixed-review/identity/build.exit";
const REVIEW_77: &str = "execution/reviews/dl01-wave1-native-loader-fixed-review/REVIEW.md";
const EXIT_77: &str = "evidence/4f3857b5-e329-44e9-89a6-0c8b1a97c51f/DL01/native-loader-fixed-review/raw/phases/core-source-bound-retest.exit";

struct ProductSpec {
    product: &'static str,
    task_id: &'static str,
    endpoint_gap: bool,
    keyword: &'static str,
}

const PRODUCTS: &[ProductSpec] = &[
    ProductSpec { product: "postgresql", task_id: "DL01-W1-46", endpoint_gap: false, keyword: "postgres" },
    ProductSpec { product: "mysql", task_id: "DL01-W1-06", endpoint_gap: false, keyword: "mysql" },
    ProductSpec { product: "mariadb", task_id: "DL01-W1-39", endpoint_gap: false, keyword: "mariadb" },
    ProductSpec { product: "sqlserver", task_id: "DL01-W1-09", endpoint_gap: true, keyword: "mssql" },
    ProductSpec { product: "oracle", task_id: "DL01-W1-10", endpoint_gap: false, keyword: "oracle" },
    ProductSpec { product: "clickhouse", task_id: "DL01-W1-11", endpoint_gap: false, keyword: "clickhouse" },
    ProductSpec { product: "doris", task_id: "DL01-W1-43", endpoint_gap: false, keyword: "doris" },
    ProductSpec { product: "starrocks", task_id: "DL01-W1-45", endpoint_gap: false, keyword: "starrocks" },
    ProductSpec { product: "mongodb", task_id: "DL01-W1-41", endpoint_gap: false, keyword: "mongodb" },
    ProductSpec { product: "opensearch", task_id: "DL01-W1-44", endpoint_gap: false, keyword: "opensearch" },
];

struct ReviewSpec {
    chain: &'static str,
    task_id: &'static str,
    review_path: &'static str,
    elf_path: &'static str,
    exit_path: &'static str,
}

const REVIEWS: &[ReviewSpec] = &[
    ReviewSpec {
        chain: "ontology_pack_versions",
        task_id: "DL01-W1-75",
        review_path: REVIEW_75,
        elf_path: ELF_75,
        exit_path: EXIT_75,
    },
    ReviewSpec {
        chain: "gateway_agent_multiturn",
        task_id: "DL01-W1-76",
        review_path: REVIEW_76,
        elf_path: ELF_76,
        exit_path: EXIT_76,
    },
    ReviewSpec {
        chain: "native_loader_rereview",
        task_id: "DL01-W1-77",
        review_path: REVIEW_77,
        elf_path: "",
        exit_path: EXIT_77,
    },
];

#[derive(Debug, Clone)]
pub struct ScopeInput {
    pub dispatch_json: String,
    pub intake_json: String,
    pub matrix_yaml: String,
    pub frontier_json: Option<String>,
    pub claim_json: Option<String>,
    pub package_ok: Option<bool>,
    pub package_error_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScopeReport {
    pub schema: String,
    pub ok: bool,
    pub exit_code: u8,
    pub report_complete: bool,
    pub promoted: bool,
    pub matrix_rewritten: bool,
    pub sources_signed_off: bool,
    pub source_acceptance: String,
    pub accepted_sources: u32,
    pub cli_exit_is_not_acceptance: bool,
    pub manufactured_nightly_days: u32,
    pub any_level_raised: bool,
    pub parent: ParentOut,
    pub products: Vec<ProductOut>,
    pub chains: Vec<ChainOut>,
    pub degradation: Vec<DegradeOut>,
    pub ci_activation: ActivationOut,
    pub case_map: Vec<CaseOut>,
    pub rejected_days: Vec<String>,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParentOut {
    pub id: String,
    pub tool_surface_present: bool,
    pub parent_accepted: bool,
    pub reason: String,
    pub pending_external: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProductOut {
    pub product: String,
    pub task_id: String,
    pub dispatch_sha: Option<String>,
    pub intake_sha: Option<String>,
    pub sha_agreement: String,
    pub commit_opened: bool,
    pub whole_source_accepted: bool,
    pub controlled_preview_not_whole_source: bool,
    pub endpoint: String,
    pub cli_exit: Option<i64>,
    pub cc: Vec<CcOut>,
    pub siblings: Vec<SiblingOut>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CcOut {
    pub id: String,
    pub name: String,
    pub status: String,
    pub reason: String,
    pub counts_as_probe: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SiblingOut {
    pub task_id: String,
    pub name: String,
    pub status: String,
    pub cli_exit: Option<i64>,
    pub counted_as_acceptance: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChainOut {
    pub id: String,
    pub task_id: String,
    pub commit: Option<String>,
    pub commit_opened: bool,
    pub upgraded: bool,
    pub original_exit: String,
    pub counts_as_probe: bool,
    pub exit_zero_is_not_acceptance: bool,
    pub review: Option<GitRef>,
    pub elf: Option<GitRef>,
    pub raw_exit: Option<GitRef>,
    pub raw_exit_value: Option<i32>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DegradeOut {
    pub target_id: String,
    pub capability: String,
    pub before: String,
    pub after: String,
    pub downgrade_recommended: bool,
    pub unverified_this_cycle: bool,
    pub matrix_rewritten: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActivationOut {
    pub schema: String,
    pub activation_count: u32,
    pub empty_run_refused: bool,
    pub empty_run_is_not_a_suite: bool,
    pub cli_exit_is_not_acceptance: bool,
    pub command_template: String,
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseOut {
    pub id: String,
    pub name: String,
}

struct TaskView {
    task_id: String,
    name: String,
    status: String,
    sha: Option<String>,
    cli_exit: Option<i64>,
    pending: Option<String>,
}

struct IntakeView {
    sha: Option<String>,
    product_acceptance: Option<bool>,
    empty_inventory_flag: bool,
    manifests: Vec<(String, u64)>,
}

pub fn run_scope(input: &ScopeInput, store: &dyn ObjectStore) -> ScopeReport {
    let mut findings = Vec::new();
    let mut rejected_days = Vec::new();
    let tasks = match parse_tasks(&input.dispatch_json) {
        Ok(tasks) => tasks,
        Err(error) => {
            push(&mut findings, "error", "scope.dispatch_unreadable", "", "", error);
            return finish(false, Vec::new(), Vec::new(), Vec::new(), false, rejected_days, findings);
        }
    };
    let intake = match parse_intake(&input.intake_json) {
        Ok(intake) => intake,
        Err(error) => {
            push(&mut findings, "error", "scope.intake_unreadable", "", "", error);
            return finish(false, Vec::new(), Vec::new(), Vec::new(), false, rejected_days, findings);
        }
    };
    let FrontierView { preview, acceptance_count: ledger_count, sqlserver_gap: frontier_sqlserver } = match &input.frontier_json {
        None => FrontierView::default(),
        Some(text) => match parse_frontier(text) {
            Ok(value) => value,
            Err(error) => {
                push(&mut findings, "error", "scope.frontier_unreadable", "", "", error);
                FrontierView::default()
            }
        },
    };
    if let Some(count) = ledger_count
        && count != 0
    {
        push(&mut findings, "error", "claim.ledger_count_not_acceptance", "", "", format!("frontier source_acceptance_count {count}"));
    }
    if let Some(text) = frontier_sqlserver {
        push(&mut findings, "info", "endpoint.gap_text", "sqlserver", "", text);
    }
    apply_claim(input.claim_json.as_deref(), store, &mut rejected_days, &mut findings);
    apply_package(input, &mut findings);

    let mut products = Vec::new();
    for spec in PRODUCTS {
        products.push(eval_product(spec, &tasks, &intake, &preview, store, &mut findings));
    }
    let mut chains = Vec::new();
    for spec in REVIEWS {
        chains.push(eval_review(spec, &tasks, &intake, store, &mut findings));
    }
    chains.push(eval_dispatch_chain("updates_deletes_intervals", "DL01-W1-74", &tasks, &intake, store, &mut findings));
    chains.push(eval_dispatch_chain("selfbuild_s0_s10_yaml", "DL01-W1-71", &tasks, &intake, store, &mut findings));
    let degradation = degrade(&input.matrix_yaml, &mut findings);
    let any_level_raised = degradation.iter().any(|row| rank(&row.after) > rank(&row.before));
    if any_level_raised {
        push(&mut findings, "error", "degrade.raised", "", "", "a proposal raised a matrix level");
    }
    finish(true, products, chains, degradation, any_level_raised, rejected_days, findings)
}

fn eval_product(spec: &ProductSpec, tasks: &BTreeMap<String, TaskView>, intake: &BTreeMap<String, IntakeView>, preview: &[String], store: &dyn ObjectStore, findings: &mut Vec<Finding>) -> ProductOut {
    let task = tasks.get(spec.task_id);
    let dispatch_sha = task.and_then(|task| task.sha.clone());
    let cli_exit = task.and_then(|task| task.cli_exit);
    let intake_row = intake.get(spec.task_id);
    let intake_sha = intake_row.and_then(|row| row.sha.clone());
    let sha_agreement = agreement(dispatch_sha.as_deref(), intake_sha.as_deref());
    if sha_agreement == "drift" {
        push(findings, "error", "evidence.sha_drift", spec.product, "", format!("dispatch {dispatch_sha:?} intake {intake_sha:?}"));
    }
    if let Some(row) = intake_row {
        note_intake(spec.product, row, findings);
    }
    let commit_opened = dispatch_sha.as_deref().is_some_and(|sha| open_commit(store, spec.product, sha, findings));
    if dispatch_sha.is_none() && spec.endpoint_gap {
        push(findings, "error", "endpoint.notrun", spec.product, "", "SQL Server actual endpoint is absent");
    } else if dispatch_sha.is_none() {
        push(findings, "error", "scope.delivery_missing", spec.product, "", spec.task_id);
    }
    if cli_exit == Some(0) {
        push(findings, "error", "claim.cli_exit_not_a_test", spec.product, "", spec.task_id);
    }
    let controlled = preview.iter().any(|name| name == spec.product);
    if controlled {
        push(findings, "info", "preview.not_whole_source", spec.product, "", "controlled preview is not a whole source");
    }
    let siblings = siblings_for(spec, tasks);
    let (status, reason) = if spec.endpoint_gap {
        ("notrun", "endpoint_gap")
    } else {
        ("not_probed", "historical bytes are not a probe of this capability")
    };
    let cc = CASES
        .iter()
        .map(|case| CcOut {
            id: case.id.to_owned(),
            name: case.name.to_owned(),
            status: status.to_owned(),
            reason: reason.to_owned(),
            counts_as_probe: false,
        })
        .collect();
    if let Some(row) = intake_row {
        open_manifests(spec.product, intake_sha.as_deref().or(dispatch_sha.as_deref()), &row.manifests, store, findings);
    }
    ProductOut {
        product: spec.product.to_owned(),
        task_id: spec.task_id.to_owned(),
        dispatch_sha,
        intake_sha,
        sha_agreement,
        commit_opened,
        whole_source_accepted: false,
        controlled_preview_not_whole_source: controlled,
        endpoint: if spec.endpoint_gap { "notrun" } else { "not_claimed" }.to_owned(),
        cli_exit,
        cc,
        siblings,
    }
}

fn eval_review(spec: &ReviewSpec, tasks: &BTreeMap<String, TaskView>, intake: &BTreeMap<String, IntakeView>, store: &dyn ObjectStore, findings: &mut Vec<Finding>) -> ChainOut {
    let task = tasks.get(spec.task_id);
    let commit = task.and_then(|task| task.sha.clone());
    let cli_exit = task.and_then(|task| task.cli_exit);
    if let Some(row) = intake.get(spec.task_id) {
        note_intake(spec.chain, row, findings);
        if let (Some(left), Some(right)) = (commit.as_deref(), row.sha.as_deref())
            && !left.eq_ignore_ascii_case(right)
        {
            push(findings, "error", "evidence.sha_drift", spec.chain, "", format!("dispatch {left} intake {right}"));
        }
        open_manifests(spec.chain, row.sha.as_deref().or(commit.as_deref()), &row.manifests, store, findings);
    }
    if cli_exit == Some(0) {
        push(findings, "error", "claim.cli_exit_not_a_test", spec.chain, "", spec.task_id);
    }
    let mut notes = Vec::new();
    let mut review = None;
    let mut elf = None;
    let mut raw_exit = None;
    let mut raw_exit_value = None;
    let mut exit_zero = false;
    let commit_opened = match &commit {
        None => {
            push(findings, "error", "scope.required_review_missing", spec.chain, "", spec.task_id);
            false
        }
        Some(sha) => {
            let opened = open_commit(store, spec.chain, sha, findings);
            if opened {
                review = take_required(store, sha, spec.review_path, spec.chain, findings);
                if !spec.elf_path.is_empty() {
                    elf = take_required(store, sha, spec.elf_path, spec.chain, findings);
                }
                raw_exit = take_required(store, sha, spec.exit_path, spec.chain, findings);
            }
            opened
        }
    };
    if let Some(pointer) = &review
        && let Ok(opened) = store.open(&pointer.commit, &pointer.path)
    {
        let text = String::from_utf8_lossy(&opened.bytes);
        notes.push("historical_report_not_probe".to_owned());
        if text.contains("REQUEST_CHANGES") {
            notes.push("request_changes".to_owned());
            push(findings, "error", "review.request_changes_not_acceptance", spec.chain, "", pointer.blob.clone());
        }
        if spec.chain == "ontology_pack_versions" && text.to_ascii_lowercase().contains("notrun") {
            notes.push("publication_notrun".to_owned());
            push(findings, "error", "ontology.pack_version_not_accepted", spec.chain, "", "review records notrun");
        }
        if spec.chain == "gateway_agent_multiturn" && text.contains("没有 Agent 对话") {
            notes.push("agent_dialogue_absent".to_owned());
            push(findings, "error", "gateway.agent_multiturn_notrun", spec.chain, "", "review records no agent dialogue");
        }
        if spec.chain == "native_loader_rereview" && text.contains("source_bound true") {
            push(findings, "error", "trust.producer_self_trust_not_pass", spec.chain, "", "source_bound author boolean");
        }
        if spec.chain == "native_loader_rereview" && text.contains("jobs_lease_receipt") && text.contains("null") {
            push(findings, "error", "trust.empty_lease_not_pass", spec.chain, "", "null lease");
        }
        if spec.chain == "native_loader_rereview" && (text.contains("expired-pos") || text.contains("POSITION_EXPIRED")) {
            push(findings, "error", "position.expired_not_pass", spec.chain, "", "expired position is not a current success");
        }
        if spec.chain == "native_loader_rereview" && text.contains("无新批次") {
            push(findings, "error", "result.known_failure_not_pass", spec.chain, "", "failure with no new batch is not a pass");
        }
    }
    if let Some(pointer) = &elf
        && let Ok(opened) = store.open(&pointer.commit, &pointer.path)
    {
        let text = String::from_utf8_lossy(&opened.bytes);
        if !text.contains("ELF") {
            push(findings, "error", "scope.elf_identity_missing", spec.chain, "", pointer.path.clone());
        }
        if text.contains("debug_review_not_author_release") {
            notes.push("elf_is_not_author_release".to_owned());
        }
    }
    if let Some(pointer) = &raw_exit
        && let Ok(opened) = store.open(&pointer.commit, &pointer.path)
    {
        raw_exit_value = parse_exit(&opened.bytes);
        if raw_exit_value == Some(0) {
            exit_zero = true;
            notes.push("exit_zero_is_not_acceptance".to_owned());
        }
    }
    let original_exit = if notes.iter().any(|note| note == "request_changes") {
        "request_changes"
    } else if commit.is_none() {
        "missing"
    } else {
        "opened_not_accepted"
    };
    ChainOut {
        id: spec.chain.to_owned(),
        task_id: spec.task_id.to_owned(),
        commit,
        commit_opened,
        upgraded: false,
        original_exit: original_exit.to_owned(),
        counts_as_probe: false,
        exit_zero_is_not_acceptance: exit_zero,
        review,
        elf,
        raw_exit,
        raw_exit_value,
        notes,
    }
}

fn eval_dispatch_chain(id: &str, task_id: &str, tasks: &BTreeMap<String, TaskView>, intake: &BTreeMap<String, IntakeView>, store: &dyn ObjectStore, findings: &mut Vec<Finding>) -> ChainOut {
    let task = tasks.get(task_id);
    let commit = task.and_then(|task| task.sha.clone());
    let status = task.map(|task| task.status.clone()).unwrap_or_else(|| "missing".to_owned());
    let pending = task.and_then(|task| task.pending.clone());
    if task.and_then(|task| task.cli_exit) == Some(0) {
        push(findings, "error", "claim.cli_exit_not_a_test", id, "", task_id);
    }
    if let Some(row) = intake.get(task_id) {
        note_intake(id, row, findings);
        if let (Some(left), Some(right)) = (commit.as_deref(), row.sha.as_deref())
            && !left.eq_ignore_ascii_case(right)
        {
            push(findings, "error", "evidence.sha_drift", id, "", format!("dispatch {left} intake {right}"));
        }
        open_manifests(id, row.sha.as_deref().or(commit.as_deref()), &row.manifests, store, findings);
    }
    let commit_opened = commit.as_deref().is_some_and(|sha| open_commit(store, id, sha, findings));
    let mut notes = vec!["not_upgraded".to_owned()];
    if let Some(text) = &pending {
        notes.push(text.clone());
        push(findings, "error", "chain.original_exit_not_upgraded", id, "", text.clone());
    } else if status != "accepted" {
        push(findings, "info", "chain.original_exit_not_upgraded", id, "", status.clone());
    }
    ChainOut {
        id: id.to_owned(),
        task_id: task_id.to_owned(),
        commit,
        commit_opened,
        upgraded: false,
        original_exit: pending.unwrap_or(status),
        counts_as_probe: false,
        exit_zero_is_not_acceptance: false,
        review: None,
        elf: None,
        raw_exit: None,
        raw_exit_value: None,
        notes,
    }
}

fn degrade(matrix_yaml: &str, findings: &mut Vec<Finding>) -> Vec<DegradeOut> {
    let matrix = match matrix::parse_matrix(matrix_yaml) {
        Ok(matrix) => matrix,
        Err(error) => {
            push(findings, "error", "matrix.parse", "", "", error);
            return Vec::new();
        }
    };
    let mut rows = Vec::new();
    for target in matrix.targets {
        for cap in target.caps {
            let before = cap.level;
            let runtime_high = matches!(before.as_str(), "verified" | "certified");
            let after = if runtime_high { "experimental".to_owned() } else { before.clone() };
            let mssql = target.family == "mssql" || target.id.starts_with("sqlserver");
            let reason = if runtime_high {
                "no passing conformance package this cycle"
            } else if mssql {
                "endpoint_gap_notrun"
            } else if is_runtime(&before) {
                "experimental_not_promoted"
            } else {
                "not_verified_this_cycle"
            };
            rows.push(DegradeOut {
                target_id: target.id.clone(),
                capability: cap.canonical,
                before,
                after,
                downgrade_recommended: runtime_high,
                unverified_this_cycle: true,
                matrix_rewritten: false,
                reason: reason.to_owned(),
            });
        }
    }
    rows
}

fn apply_package(input: &ScopeInput, findings: &mut Vec<Finding>) {
    match input.package_ok {
        Some(false) => {
            let detail = if input.package_error_codes.is_empty() {
                "package check failed".to_owned()
            } else {
                input.package_error_codes.join(",")
            };
            push(findings, "error", "package.not_accepted", "", "", detail);
        }
        Some(true) => push(findings, "info", "package.passed_not_promoted", "", "", "a package pass does not sign off the ten sources"),
        None => {}
    }
}

fn apply_claim(claim_json: Option<&str>, store: &dyn ObjectStore, rejected_days: &mut Vec<String>, findings: &mut Vec<Finding>) {
    let Some(text) = claim_json else {
        return;
    };
    let value: Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(error) => {
            push(findings, "error", "claim.malformed", "", "", error.to_string());
            return;
        }
    };
    let Some(map) = value.as_object() else {
        push(findings, "error", "claim.malformed", "", "", "claim is not an object");
        return;
    };
    if map.get("schema").and_then(Value::as_str) != Some("oo-source-conformance/scope-claim/v1") {
        push(findings, "error", "claim.schema", "", "", "expected oo-source-conformance/scope-claim/v1");
        return;
    }
    if map.get("assert_all_sources_accepted").and_then(Value::as_bool) == Some(true) || map.get("assert_accepted_count").and_then(Value::as_u64).unwrap_or(0) > 0 {
        push(findings, "error", "claim.all_sources_refused", "", "", "ten-source acceptance is not established");
    }
    if map.get("activate_without_tests").and_then(Value::as_bool) == Some(true) {
        push(findings, "error", "ci.empty_run_refused", "", "", "CI must not enable a capability before its tests pass");
    }
    if map.get("external_attestation_inline").is_some() && !map.get("external_attestation_inline").is_some_and(Value::is_null) {
        push(findings, "error", "external.forged_attestation", "", "", "inline owner/DBA/14-day attestation is not external evidence");
    }
    let Some(witnesses) = map.get("witnesses") else {
        return;
    };
    let Some(list) = witnesses.as_array() else {
        push(findings, "error", "claim.malformed", "", "", "witnesses is not a list");
        return;
    };
    for witness in list {
        apply_witness(witness, store, rejected_days, findings);
    }
}

fn apply_witness(witness: &Value, store: &dyn ObjectStore, rejected_days: &mut Vec<String>, findings: &mut Vec<Finding>) {
    let Some(map) = witness.as_object() else {
        push(findings, "error", "claim.malformed", "", "", "witness is not an object");
        return;
    };
    let kind = map.get("kind").and_then(Value::as_str).unwrap_or("");
    match kind {
        "unknown_source" => {
            let product = map.get("product").and_then(Value::as_str).unwrap_or("");
            let code = if PRODUCTS.iter().any(|spec| spec.product == product) { "source.known_not_accepted" } else { "source.unknown" };
            push(findings, "error", code, product, "", "unknown or unsigned source is not a pass");
        }
        "unverified_day" => {
            let day = map.get("calendar_day").and_then(Value::as_str).unwrap_or("");
            rejected_days.push(day.to_owned());
            push(findings, "error", "nightly.unverified_day", "", "", day);
        }
        "raw_exit" | "missing" | "self_trust" | "expired_position" | "audit" | "tamper" => {
            let commit = map.get("commit").and_then(Value::as_str).unwrap_or("");
            let path = map.get("path").and_then(Value::as_str).unwrap_or("");
            let disposition = map.get("disposition").and_then(Value::as_str).unwrap_or("");
            match store.open(commit, path) {
                Err(error) => push(findings, "error", "claim.missing_object", "", "", format!("{path} {error}")),
                Ok(opened) => {
                    if let Some(expected) = map.get("expect_blob").and_then(Value::as_str)
                        && !expected.is_empty()
                        && expected != opened.blob
                    {
                        push(findings, "error", "evidence.hash_mismatch", "", "", format!("blob {expected} != {}", opened.blob));
                    }
                    let text = String::from_utf8_lossy(&opened.bytes);
                    if kind == "raw_exit" && disposition == "pass" && parse_exit(&opened.bytes) != Some(0) {
                        push(findings, "error", "raw.failure_impersonates_pass", "", "", path);
                    }
                    if kind == "self_trust" || (disposition == "pass" && text.contains("source_bound")) {
                        push(findings, "error", "trust.producer_self_trust_not_pass", "", "", path);
                    }
                    if kind == "expired_position" || (disposition == "pass" && (text.contains("expired-pos") || text.contains("POSITION_EXPIRED"))) {
                        push(findings, "error", "position.expired_not_pass", "", "", path);
                    }
                    if kind == "audit" {
                        let outcome = parse_audit(&opened.bytes);
                        if outcome.ok && outcome.zero_write {
                            push(findings, "info", "audit.zero_write_not_cc02_pass", "", "", path);
                        } else if outcome.codes.is_empty() {
                            push(findings, "error", "audit.rejected", "", "", path);
                        } else {
                            for code in outcome.codes {
                                let head = code.split(':').next().unwrap_or("audit.rejected").to_owned();
                                push(findings, "error", &head, "", "", code);
                            }
                        }
                    }
                }
            }
        }
        _ => push(findings, "error", "claim.witness_kind", "", "", kind),
    }
}

fn open_manifests(target: &str, sha: Option<&str>, manifests: &[(String, u64)], store: &dyn ObjectStore, findings: &mut Vec<Finding>) {
    for (path, checked) in manifests {
        if *checked == 0 {
            continue;
        }
        match sha {
            Some(commit) => match store.open(commit, path) {
                Ok(opened) => {
                    let pointer = git_ref(commit, path, &opened);
                    push(findings, "info", "intake.inventory_not_cc_probe", target, "", format!("{} {}", pointer.blob, pointer.sha256));
                }
                Err(error) => push(findings, "error", "intake.manifest_unopened", target, "", format!("{path} {error}")),
            },
            None => push(findings, "error", "intake.manifest_unopened", target, "", path.clone()),
        }
    }
}

fn note_intake(product: &str, row: &IntakeView, findings: &mut Vec<Finding>) {
    match row.product_acceptance {
        Some(true) => push(findings, "error", "intake.product_acceptance_refused", product, "", "author or intake boolean is not acceptance"),
        Some(false) => push(findings, "info", "intake.product_acceptance_false", product, "", "intake says product_acceptance false"),
        None => push(findings, "error", "intake.product_acceptance_untrusted", product, "", "missing product_acceptance"),
    }
    if row.empty_inventory_flag && row.manifests.is_empty() {
        push(findings, "error", "intake.empty_inventory_not_proof", product, "", "empty inventory is not a new delivery proof");
    }
}

fn siblings_for(spec: &ProductSpec, tasks: &BTreeMap<String, TaskView>) -> Vec<SiblingOut> {
    let mut out = Vec::new();
    for task in tasks.values() {
        if task.task_id == spec.task_id {
            continue;
        }
        let name = task.name.to_ascii_lowercase();
        let hit = if spec.product == "mysql" {
            name == "mysql" || name.starts_with("mysql-")
        } else if spec.product == "sqlserver" {
            name.contains("mssql") || name.contains("sqlserver")
        } else {
            name.contains(spec.keyword)
        };
        if !hit {
            continue;
        }
        out.push(SiblingOut {
            task_id: task.task_id.clone(),
            name: task.name.clone(),
            status: task.status.clone(),
            cli_exit: task.cli_exit,
            counted_as_acceptance: false,
        });
        if out.len() == 12 {
            break;
        }
    }
    out
}

fn open_commit(store: &dyn ObjectStore, target: &str, sha: &str, findings: &mut Vec<Finding>) -> bool {
    if !valid_commit(sha) {
        push(findings, "error", "scope.sha_malformed", target, "", sha);
        return false;
    }
    if store.has_commit(sha) {
        true
    } else {
        push(findings, "error", "scope.commit_unopened", target, "", sha);
        false
    }
}

fn take_required(store: &dyn ObjectStore, commit: &str, path: &str, target: &str, findings: &mut Vec<Finding>) -> Option<GitRef> {
    match store.open(commit, path) {
        Ok(opened) => Some(git_ref(commit, path, &opened)),
        Err(error) => {
            push(findings, "error", "scope.object_missing", target, "", format!("{path} {error}"));
            None
        }
    }
}

fn agreement(dispatch: Option<&str>, intake: Option<&str>) -> String {
    match (dispatch, intake) {
        (Some(left), Some(right)) if left.eq_ignore_ascii_case(right) => "match".to_owned(),
        (Some(_), Some(_)) => "drift".to_owned(),
        (Some(_), None) => "intake_absent".to_owned(),
        (None, Some(_)) => "dispatch_absent".to_owned(),
        (None, None) => "both_absent".to_owned(),
    }
}

fn parse_exit(bytes: &[u8]) -> Option<i32> {
    let text = String::from_utf8_lossy(bytes);
    let token = text.trim().lines().next()?.trim();
    token.parse().ok()
}

fn parse_tasks(text: &str) -> Result<BTreeMap<String, TaskView>, String> {
    let value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let list = value.get("tasks").and_then(Value::as_array).ok_or_else(|| "dispatch.tasks missing".to_owned())?;
    let mut out = BTreeMap::new();
    for item in list {
        let Some(map) = item.as_object() else {
            continue;
        };
        let Some(task_id) = map.get("task_id").and_then(Value::as_str) else {
            continue;
        };
        let sha = match map.get("latest_author_delivery").and_then(Value::as_str) {
            Some(sha) if valid_commit(sha) => Some(sha.to_owned()),
            _ => None,
        };
        out.insert(
            task_id.to_owned(),
            TaskView {
                task_id: task_id.to_owned(),
                name: map.get("name").and_then(Value::as_str).unwrap_or("").to_owned(),
                status: map.get("status").and_then(Value::as_str).unwrap_or("").to_owned(),
                sha,
                cli_exit: map.get("cli_exit").and_then(Value::as_i64),
                pending: map.get("complete_capability_pending").and_then(Value::as_str).map(str::to_owned),
            },
        );
    }
    Ok(out)
}

fn parse_intake(text: &str) -> Result<BTreeMap<String, IntakeView>, String> {
    let value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let list = value.as_array().ok_or_else(|| "intake is not a list".to_owned())?;
    let mut out = BTreeMap::new();
    for item in list {
        let map = item.as_object().ok_or_else(|| "intake row is not an object".to_owned())?;
        let task = map.get("task").and_then(Value::as_str).ok_or_else(|| "intake.task missing".to_owned())?;
        let task_id = if task.starts_with("DL01-") { task.to_owned() } else { format!("DL01-W1-{task}") };
        let sha = match map.get("fixed_sha").and_then(Value::as_str) {
            Some(sha) if valid_commit(sha) => Some(sha.to_owned()),
            Some(sha) => return Err(format!("intake.sha_malformed: {task} {sha}")),
            None => None,
        };
        let manifests = map
            .get("changed_manifests")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let object = item.as_object()?;
                        let path = object.get("manifest").and_then(Value::as_str)?.to_owned();
                        let checked = object.get("checked").and_then(Value::as_u64).unwrap_or(0);
                        Some((path, checked))
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.insert(
            task_id,
            IntakeView {
                sha,
                product_acceptance: map.get("product_acceptance").and_then(Value::as_bool),
                empty_inventory_flag: map.get("inventory_new_delivery_not_inferred_from_old").and_then(Value::as_bool).unwrap_or(false),
                manifests,
            },
        );
    }
    Ok(out)
}

#[derive(Default)]
struct FrontierView {
    preview: Vec<String>,
    acceptance_count: Option<i64>,
    sqlserver_gap: Option<String>,
}

fn parse_frontier(text: &str) -> Result<FrontierView, String> {
    let value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let map = value.as_object().ok_or_else(|| "frontier is not an object".to_owned())?;
    let preview = map
        .get("controlled_preview_accepted_source_products")
        .and_then(Value::as_object)
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default();
    let acceptance_count = map.get("source_acceptance_count").and_then(Value::as_i64);
    let sqlserver_gap = map
        .get("external_target_gaps")
        .and_then(Value::as_object)
        .and_then(|object| object.get("sqlserver"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok(FrontierView { preview, acceptance_count, sqlserver_gap })
}

fn finish(report_complete: bool, products: Vec<ProductOut>, chains: Vec<ChainOut>, degradation: Vec<DegradeOut>, any_level_raised: bool, rejected_days: Vec<String>, findings: Vec<Finding>) -> ScopeReport {
    let errors = findings.iter().any(|finding| finding.severity == "error");
    let empty_refused = findings.iter().any(|finding| finding.code == "ci.empty_run_refused");
    ScopeReport {
        schema: SCHEMA.to_owned(),
        ok: report_complete && !errors,
        exit_code: if report_complete && !errors { 0 } else { 1 },
        report_complete,
        promoted: false,
        matrix_rewritten: false,
        sources_signed_off: false,
        source_acceptance: "0/10".to_owned(),
        accepted_sources: 0,
        cli_exit_is_not_acceptance: true,
        manufactured_nightly_days: 0,
        any_level_raised,
        parent: ParentOut {
            id: "DBX-W14".to_owned(),
            tool_surface_present: report_complete,
            parent_accepted: false,
            reason: "external 14-day, owner, DBA, SQL Server endpoint, and ten-source signoff stay pending".to_owned(),
            pending_external: vec![
                "sqlserver_endpoint".to_owned(),
                "external_14_day".to_owned(),
                "external_owner".to_owned(),
                "external_dba".to_owned(),
                "managed_cloud_budget".to_owned(),
            ],
        },
        products,
        chains,
        degradation,
        ci_activation: ActivationOut {
            schema: "oo-source-conformance/ci-activation/v1".to_owned(),
            activation_count: 0,
            empty_run_refused: empty_refused,
            empty_run_is_not_a_suite: true,
            cli_exit_is_not_acceptance: true,
            command_template: "source-conformance check --matrix FILE --package DIR".to_owned(),
            commands: Vec::new(),
        },
        case_map: CASES.iter().map(|case| CaseOut { id: case.id.to_owned(), name: case.name.to_owned() }).collect(),
        rejected_days,
        findings,
    }
}

fn rank(level: &str) -> u8 {
    match level {
        "candidate" => 1,
        "designed" => 2,
        "experimental" => 3,
        "verified" => 4,
        "certified" => 5,
        _ => 0,
    }
}

fn push(findings: &mut Vec<Finding>, severity: &str, code: &str, target: &str, capability: &str, detail: impl Into<String>) {
    findings.push(Finding {
        severity: severity.to_owned(),
        code: code.to_owned(),
        target_id: target.to_owned(),
        capability: capability.to_owned(),
        detail: detail.into(),
    });
}
