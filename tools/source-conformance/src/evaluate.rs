//! One rule path for the library and the CLI. Grades are never written back.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::cases::{required_cases, CASES, CERTIFIED_CASES};
use crate::historical::{self, Record};
use crate::identity::{self, Verdict};
use crate::matrix::{self, is_runtime, managed_variant, version_exact, Matrix, Target, LEVELS};
use crate::nightly::{self, Streak};
use crate::package::{self, hash_detail, Loaded};
use crate::sha256_hex;

const BASE_SHA: &str = "577160c22de63adb9b38e0147f3bc2da893c4d9d";
const SUPPORT_SHA: &str = "29597079f7feacc6e3934b5c803686fc80567a93";
const SUPPORT_REVIEW: &str = "824b20735bd958c4298913838fb832058d17e512";
const PROPOSED_API: &str = "source_support::matrix::parse_support_matrix(yaml: &str) -> Result<SupportMatrix, MatrixError>; Target { id, family, product, versions, variant, capabilities: BTreeMap<canonical id, Level> }. Fixed source-support 29597079f7feacc6e3934b5c803686fc80567a93 exports matrix::load, not parse_support_matrix. Review 824b20735bd958c4298913838fb832058d17e512 accepted that tree. This crate does not call the mutable workspace and does not stub a successful consumer.";

#[derive(Debug, Clone)]
pub struct Request {
    pub matrix_path: PathBuf,
    pub package_dirs: Vec<PathBuf>,
    pub nightly_path: Option<PathBuf>,
    pub historical_pg: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub schema: String,
    pub ok: bool,
    pub exit_code: u8,
    pub promoted: bool,
    pub matrix_rewritten: bool,
    pub matrix_sha256: String,
    pub counts: BTreeMap<String, u32>,
    pub findings: Vec<Finding>,
    pub cells: Vec<CellOut>,
    pub nightly: Vec<NightlyOut>,
    pub historical_pg: Option<Record>,
    pub source_support_consumer: Consumer,
    pub case_map: Vec<CaseMapOut>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub severity: String,
    pub code: String,
    pub target_id: String,
    pub capability: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CellOut {
    pub target_id: String,
    pub capability: String,
    pub family: String,
    pub matrix_level: String,
    pub recommended_level: String,
    pub downgrade_recommended: bool,
    pub status: String,
    pub required_tests: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NightlyOut {
    pub target_id: String,
    pub capability: String,
    pub trailing_consecutive_success_days: u32,
    pub longest_consecutive_success_days: u32,
    pub missed_days: Vec<String>,
    pub consecutive_failures: u32,
    pub manual_runs: u32,
    pub suite_runs: u32,
    pub downgrade_recommended: bool,
    pub recommended_level: String,
    pub matrix_level: String,
    pub matrix_rewritten: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Consumer {
    pub status: String,
    pub protocol_commit: String,
    pub mutable_workspace: String,
    pub proposed_api: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseMapOut {
    pub id: String,
    pub name: String,
    pub applies: String,
}

pub fn run_paths(request: &Request) -> Report {
    let mut findings = Vec::new();
    let historical = match &request.historical_pg {
        None => None,
        Some(path) => match historical::load(path) {
            Ok(record) => Some(record),
            Err(error) => {
                push_coded(&mut findings, &error, "", "");
                None
            }
        },
    };
    let matrix_bytes = match read_capped(&request.matrix_path, 2_097_152, "matrix.unreadable") {
        Ok(bytes) => bytes,
        Err(error) => {
            push_coded(&mut findings, &error, "", "");
            return finish(String::new(), Vec::new(), Vec::new(), findings, historical);
        }
    };
    let matrix_sha256 = sha256_hex(&matrix_bytes);
    let text = match std::str::from_utf8(&matrix_bytes) {
        Ok(text) if !text.starts_with('\u{feff}') => text,
        Ok(_) => {
            push(&mut findings, "error", "matrix.parse", "", "", "UTF-8 BOM");
            return finish(matrix_sha256, Vec::new(), Vec::new(), findings, historical);
        }
        Err(_) => {
            push(&mut findings, "error", "matrix.parse", "", "", "not UTF-8");
            return finish(matrix_sha256, Vec::new(), Vec::new(), findings, historical);
        }
    };
    let matrix = match matrix::parse_matrix(text) {
        Ok(matrix) => matrix,
        Err(error) => {
            push_coded(&mut findings, &error, "", "");
            return finish(matrix_sha256, Vec::new(), Vec::new(), findings, historical);
        }
    };
    let mut packages = Vec::new();
    let mut blocked = BTreeSet::new();
    for dir in &request.package_dirs {
        match package::load_dir(dir) {
            Ok(loaded) => packages.push(loaded),
            Err(error) => {
                push_coded(&mut findings, &error, "", "");
                if let Some(evidence_id) = package::cited_evidence_id(dir) {
                    blocked.insert(evidence_id);
                }
            }
        }
    }
    let registry = match &request.nightly_path {
        None => None,
        Some(path) => match read_capped(path, 2_097_152, "nightly.unreadable").and_then(|bytes| {
            let text = std::str::from_utf8(&bytes).map_err(|_| "nightly.parse: not UTF-8".to_owned())?;
            nightly::parse_registry(text)
        }) {
            Ok(registry) => Some(registry),
            Err(error) => {
                push_coded(&mut findings, &error, "", "");
                None
            }
        },
    };
    evaluate(&matrix, &packages, registry.as_ref(), matrix_sha256, findings, historical, blocked)
}

fn evaluate(
    matrix: &Matrix,
    packages: &[Loaded],
    registry: Option<&nightly::Registry>,
    matrix_sha256: String,
    mut findings: Vec<Finding>,
    historical: Option<Record>,
    mut blocked: BTreeSet<String>,
) -> Report {
    let mut present_by_target: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut support_by_target: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut credited: BTreeMap<String, usize> = BTreeMap::new();
    for (index, loaded) in packages.iter().enumerate() {
        let evidence_id = loaded.package.evidence_id.clone();
        let Some(target) = matrix.target(&loaded.package.target_id) else {
            push(
                &mut findings,
                "error",
                "evidence.target_mismatch",
                &loaded.package.target_id,
                "",
                format!("package target is not in the matrix ({})", loaded.label),
            );
            deny_credit(&mut blocked, &mut credited, evidence_id);
            continue;
        };
        if let Some(variant) = &target.variant
            && loaded.package.variant != *variant
        {
            let got = &loaded.package.variant;
            push(
                &mut findings,
                "error",
                "evidence.variant_mismatch",
                &target.id,
                "",
                format!("package variant {got} != matrix variant {variant}"),
            );
            deny_credit(&mut blocked, &mut credited, evidence_id);
            continue;
        }
        note_hashes(&mut findings, loaded);
        present_by_target.entry(target.id.clone()).or_default().push(index);
        if let Some(key) = &loaded.duplicate_identity_key {
            note_identity(&mut findings, target, "version.identity_duplicate", key);
            deny_credit(&mut blocked, &mut credited, evidence_id);
        } else if identity_supports(&mut findings, target, loaded) {
            support_by_target.entry(target.id.clone()).or_default().push(index);
            if !blocked.contains(&evidence_id) {
                credited.insert(evidence_id, index);
            }
        } else {
            deny_credit(&mut blocked, &mut credited, evidence_id);
        }
    }
    let mut streaks: BTreeMap<(String, String), Streak> = BTreeMap::new();
    if let Some(registry) = registry {
        for entry in &registry.entries {
            let Some(target) = matrix.target(&entry.target_id) else {
                push(&mut findings, "error", "nightly.target_unknown", &entry.target_id, &entry.capability, "target is not in the matrix");
                continue;
            };
            if !target.caps.iter().any(|cap| cap.canonical == entry.capability) {
                push(&mut findings, "error", "nightly.target_unknown", &entry.target_id, &entry.capability, "capability is not on the target");
                continue;
            }
            let key = (entry.target_id.clone(), entry.capability.clone());
            if streaks.contains_key(&key) {
                push(&mut findings, "error", "nightly.duplicate", &entry.target_id, &entry.capability, "repeated target and capability");
                continue;
            }
            let counted = retain_credited_days(entry, &blocked, &credited, packages, matrix);
            match nightly::streak_of(&counted) {
                Ok(streak) => {
                    streaks.insert(key, streak);
                }
                Err(error) => push_coded(&mut findings, &error, &entry.target_id, &entry.capability),
            }
        }
    }
    let mut cells = Vec::new();
    for target in &matrix.targets {
        let present_indexes = present_by_target.get(&target.id).cloned().unwrap_or_default();
        let support_indexes = support_by_target.get(&target.id).cloned().unwrap_or_default();
        let present: Vec<&Loaded> = present_indexes.iter().map(|index| &packages[*index]).collect();
        let support: Vec<&Loaded> = support_indexes.iter().map(|index| &packages[*index]).collect();
        let any_runtime = target.caps.iter().any(|cap| is_runtime(&cap.level));
        let any_static = target.caps.iter().any(|cap| !is_runtime(&cap.level));
        if any_runtime && any_static && !support.is_empty() && !has_valid_rejection(target, &support) {
            for cap in target.caps.iter().filter(|cap| is_runtime(&cap.level)) {
                push(&mut findings, "error", "claim.missing_rejection", &target.id, &cap.canonical, "no valid CAPABILITY_UNVERIFIED rejection");
            }
        }
        note_bad_rejections(&mut findings, target, &support);
        for cap in &target.caps {
            cells.push(eval_cell(target, &cap.canonical, &cap.level, &present, &support, &streaks, &mut findings));
        }
    }
    let nightly_rows = nightly_rows(&cells, &streaks);
    finish(matrix_sha256, cells, nightly_rows, findings, historical)
}

fn eval_cell(
    target: &Target,
    capability: &str,
    level: &str,
    present: &[&Loaded],
    support: &[&Loaded],
    streaks: &BTreeMap<(String, String), Streak>,
    findings: &mut Vec<Finding>,
) -> CellOut {
    let runtime = is_runtime(level);
    let required = if runtime { required_cases(&target.family, capability) } else { Vec::new() };
    let mut recommended = level.to_owned();
    let mut downgrade = false;
    if runtime {
        if present.is_empty() {
            push(findings, "error", "claim.manual_assertion", &target.id, capability, "runtime level has no usable package");
            push(findings, "error", "claim.missing_test", &target.id, capability, "no package");
        } else if support.is_empty() {
            push(findings, "error", "claim.missing_test", &target.id, capability, "no identity-matched package");
        } else {
            let missing: Vec<&str> = required.iter().copied().filter(|id| !case_covered(support, id, capability)).collect();
            if !missing.is_empty() {
                push(findings, "error", "claim.missing_test", &target.id, capability, format!("missing {}", missing.join(",")));
            }
            check_versions(target, level, support, capability, findings);
            if matches!(level, "verified" | "certified") {
                for package in support {
                    match package::reviewers_ok(&package.package) {
                        Ok(()) => {}
                        Err(reason) if reason.starts_with("placeholder") => {
                            push(findings, "error", "reviewer.placeholder", &target.id, capability, reason);
                        }
                        Err(reason) => push(findings, "error", "reviewer.missing", &target.id, capability, reason),
                    }
                    if package.package.deployment_scope != "public" {
                        push(findings, "error", "cloud.deployment_scope", &target.id, capability, package.package.deployment_scope.clone());
                    }
                    if package.package.budget_exceeded || package.package.cycle_status == "not_verified_this_cycle" {
                        push(findings, "error", "cloud.cycle_unverified", &target.id, capability, package.package.cycle_status.clone());
                        recommended = "experimental".to_owned();
                    }
                }
                let streak = streaks.get(&(target.id.clone(), capability.to_owned())).cloned().unwrap_or(Streak {
                    trailing_success_days: 0,
                    longest_success_days: 0,
                    missed_days: Vec::new(),
                    consecutive_failures: 0,
                    manual_runs: 0,
                    suite_runs: 0,
                });
                if streak.trailing_success_days < 14 {
                    push(findings, "error", "nightly.streak_short", &target.id, capability, format!("trailing {}", streak.trailing_success_days));
                }
                if streak.consecutive_failures >= 2 {
                    push(findings, "error", "nightly.failure_downgrade", &target.id, capability, format!("consecutive_failures {}", streak.consecutive_failures));
                    recommended = "experimental".to_owned();
                    downgrade = true;
                }
            }
            if let Some(variant) = &target.variant
                && managed_variant(variant)
            {
                for package in support {
                    if package.package.evidence_type == "container" {
                        push(findings, "error", "cloud.container_scope", &target.id, capability, variant.clone());
                    }
                }
            }
            if level == "certified" {
                let missing: Vec<&str> = CERTIFIED_CASES.iter().copied().filter(|id| !id_covered(support, id)).collect();
                if !missing.is_empty() {
                    push(findings, "error", "certified.fault_drill_missing", &target.id, capability, missing.join(","));
                }
                let strong = support.iter().any(|package| {
                    matches!(package.package.evidence_type.as_str(), "customer" | "managed_cloud")
                });
                if !strong {
                    push(findings, "error", "certified.incomplete", &target.id, capability, "container evidence is not customer or managed_cloud");
                }
            }
        }
    } else if let Some(streak) = streaks.get(&(target.id.clone(), capability.to_owned()))
        && streak.trailing_success_days >= 14
    {
        let days = streak.trailing_success_days;
        push(findings, "info", "nightly.not_promoted", &target.id, capability, format!("trailing {days} stays {level}"));
    }
    if !runtime {
        for package in present {
            for case in &package.package.cases {
                if case.capability == capability && case.disposition == "pass" && case.raw_exit == 0 && hash_detail(package, &case.raw_path, &case.raw_sha256).is_none() {
                    if level == "excluded" {
                        push(findings, "error", "claim.excluded_passed", &target.id, capability, case.id.clone());
                    } else {
                        push(findings, "error", "claim.silent_fallback", &target.id, capability, format!("pass on {level} {}", case.id));
                    }
                }
            }
        }
        if !present.is_empty() && !cell_has_error(findings, &target.id, capability) {
            push(findings, "info", "package.not_applied", &target.id, capability, "package does not promote this cell");
        }
    }
    recommended = not_above(&recommended, level);
    let status = if cell_has_error(findings, &target.id, capability) {
        "failed"
    } else if runtime {
        "passed"
    } else {
        "not_required"
    };
    CellOut {
        target_id: target.id.clone(),
        capability: capability.to_owned(),
        family: target.family.clone(),
        matrix_level: level.to_owned(),
        recommended_level: recommended,
        downgrade_recommended: downgrade,
        status: status.to_owned(),
        required_tests: required.into_iter().map(str::to_owned).collect(),
    }
}

fn identity_supports(findings: &mut Vec<Finding>, target: &Target, loaded: &Loaded) -> bool {
    let claimed = loaded.package.exact_version.as_str();
    let service = loaded.package.service_version.as_str();
    match identity::classify(&target.family, &target.product, claimed, service) {
        Verdict::Match { .. } => true,
        Verdict::Mismatch { service, canonical, claimed } => {
            let detail = format!("service_version {service} canonical {canonical} != exact_version {claimed}");
            note_identity(findings, target, "version.identity_mismatch", &detail);
            false
        }
        Verdict::Unknown { service, reason } => {
            let detail = format!("service_version {service}: {reason}");
            note_identity(findings, target, "version.identity_unknown", &detail);
            false
        }
    }
}

fn note_identity(findings: &mut Vec<Finding>, target: &Target, code: &str, detail: &str) {
    let runtime: Vec<&str> = target.caps.iter().filter(|cap| is_runtime(&cap.level)).map(|cap| cap.canonical.as_str()).collect();
    let caps: Vec<&str> = if runtime.is_empty() {
        target.caps.iter().map(|cap| cap.canonical.as_str()).collect()
    } else {
        runtime
    };
    if caps.is_empty() {
        push(findings, "error", code, &target.id, "", detail);
        return;
    }
    for capability in caps {
        push(findings, "error", code, &target.id, capability, detail);
    }
}

fn deny_credit(blocked: &mut BTreeSet<String>, credited: &mut BTreeMap<String, usize>, evidence_id: String) {
    blocked.insert(evidence_id.clone());
    credited.remove(&evidence_id);
}

fn retain_credited_days(
    entry: &nightly::Entry,
    blocked: &BTreeSet<String>,
    credited: &BTreeMap<String, usize>,
    packages: &[Loaded],
    matrix: &Matrix,
) -> nightly::Entry {
    let target = matrix.target(&entry.target_id);
    nightly::Entry {
        target_id: entry.target_id.clone(),
        capability: entry.capability.clone(),
        manual_runs: entry.manual_runs,
        days: entry
            .days
            .iter()
            .filter(|day| day.raw_exit != 0 || night_success_counts(day.evidence_id.as_str(), &entry.capability, blocked, credited, packages, target))
            .cloned()
            .collect(),
    }
}

fn night_success_counts(
    evidence_id: &str,
    capability: &str,
    blocked: &BTreeSet<String>,
    credited: &BTreeMap<String, usize>,
    packages: &[Loaded],
    target: Option<&Target>,
) -> bool {
    if blocked.contains(evidence_id) {
        return false;
    }
    let Some(&index) = credited.get(evidence_id) else {
        return true;
    };
    let Some(target) = target else {
        return false;
    };
    if packages[index].package.target_id != target.id {
        return false;
    }
    raw_supports_capability(target, &packages[index], capability)
}

fn raw_supports_capability(target: &Target, loaded: &Loaded, capability: &str) -> bool {
    let Some(cap) = target.caps.iter().find(|cap| cap.canonical == capability) else {
        return false;
    };
    if !is_runtime(&cap.level) {
        return loaded.package.cases.iter().filter(|case| case.capability == capability).all(|case| {
            hash_detail(loaded, &case.raw_path, &case.raw_sha256).is_none() && (case.disposition != "pass" || case.raw_exit == 0)
        });
    }
    let one = [loaded];
    required_cases(&target.family, capability).into_iter().all(|id| case_covered(&one, id, capability))
}

fn check_versions(target: &Target, level: &str, loaded: &[&Loaded], capability: &str, findings: &mut Vec<Finding>) {
    if target.versions.is_empty() {
        push(findings, "error", "version.matrix_missing", &target.id, capability, "runtime cell has no versions");
        return;
    }
    let exact_listed: Vec<&str> = target.versions.iter().filter(|version| version_exact(version)).map(String::as_str).collect();
    if matches!(level, "verified" | "certified") {
        let mut ranged = false;
        for version in &target.versions {
            if !version_exact(version) {
                push(findings, "error", "version.not_exact", &target.id, capability, version.clone());
                ranged = true;
            }
        }
        if ranged {
            return;
        }
        let covered: BTreeSet<&str> = loaded.iter().map(|package| package.package.exact_version.as_str()).filter(|version| version_exact(version)).collect();
        for version in &exact_listed {
            if !covered.contains(version) {
                push(findings, "error", "version.untested", &target.id, capability, format!("missing {version}"));
            }
        }
        for version in &covered {
            if !exact_listed.contains(version) {
                push(findings, "error", "version.untested", &target.id, capability, format!("extra {version}"));
            }
        }
        return;
    }
    if exact_listed.is_empty() {
        for package in loaded {
            if !version_exact(&package.package.exact_version) {
                push(findings, "error", "version.not_exact", &target.id, capability, package.package.exact_version.clone());
            }
        }
        return;
    }
    for package in loaded {
        if !version_exact(&package.package.exact_version) {
            push(findings, "error", "version.not_exact", &target.id, capability, package.package.exact_version.clone());
        } else if !exact_listed.contains(&package.package.exact_version.as_str()) {
            push(findings, "error", "version.untested", &target.id, capability, package.package.exact_version.clone());
        }
    }
}

fn note_hashes(findings: &mut Vec<Finding>, loaded: &Loaded) {
    for case in &loaded.package.cases {
        if let Some(detail) = hash_detail(loaded, &case.raw_path, &case.raw_sha256) {
            push(findings, "error", "evidence.hash_mismatch", &loaded.package.target_id, &case.capability, detail);
        }
    }
    for rejection in &loaded.package.rejections {
        if let Some(detail) = hash_detail(loaded, &rejection.raw_path, &rejection.raw_sha256) {
            push(findings, "error", "evidence.hash_mismatch", &loaded.package.target_id, &rejection.capability, detail);
        }
    }
}

fn note_bad_rejections(findings: &mut Vec<Finding>, target: &Target, loaded: &[&Loaded]) {
    for package in loaded {
        for rejection in &package.package.rejections {
            if hash_detail(package, &rejection.raw_path, &rejection.raw_sha256).is_some() {
                continue;
            }
            let shaped = rejection.code == "CAPABILITY_UNVERIFIED" && !rejection.silent_fallback && rejection.raw_exit != 0;
            if !shaped {
                push(findings, "error", "claim.silent_fallback", &target.id, &rejection.capability, format!("rejection {} silent_fallback={}", rejection.code, rejection.silent_fallback));
            }
        }
    }
}

fn has_valid_rejection(target: &Target, loaded: &[&Loaded]) -> bool {
    loaded.iter().any(|package| {
        package.package.rejections.iter().any(|rejection| {
            let level = target.caps.iter().find(|cap| cap.canonical == rejection.capability).map(|cap| cap.level.as_str());
            let Some(level) = level else {
                return false;
            };
            !is_runtime(level)
                && rejection.code == "CAPABILITY_UNVERIFIED"
                && !rejection.silent_fallback
                && rejection.raw_exit != 0
                && hash_detail(package, &rejection.raw_path, &rejection.raw_sha256).is_none()
        })
    })
}

fn case_covered(loaded: &[&Loaded], id: &str, capability: &str) -> bool {
    loaded.iter().any(|package| {
        package.package.cases.iter().any(|case| {
            case.id == id
                && case.capability == capability
                && case.disposition == "pass"
                && case.raw_exit == 0
                && hash_detail(package, &case.raw_path, &case.raw_sha256).is_none()
        })
    })
}

fn id_covered(loaded: &[&Loaded], id: &str) -> bool {
    loaded.iter().any(|package| {
        package.package.cases.iter().any(|case| {
            case.id == id && case.disposition == "pass" && case.raw_exit == 0 && hash_detail(package, &case.raw_path, &case.raw_sha256).is_none()
        })
    })
}

fn nightly_rows(cells: &[CellOut], streaks: &BTreeMap<(String, String), Streak>) -> Vec<NightlyOut> {
    let mut rows = Vec::new();
    for ((target_id, capability), streak) in streaks {
        let cell = cells.iter().find(|cell| cell.target_id == *target_id && cell.capability == *capability);
        rows.push(NightlyOut {
            target_id: target_id.clone(),
            capability: capability.clone(),
            trailing_consecutive_success_days: streak.trailing_success_days,
            longest_consecutive_success_days: streak.longest_success_days,
            missed_days: streak.missed_days.clone(),
            consecutive_failures: streak.consecutive_failures,
            manual_runs: streak.manual_runs,
            suite_runs: streak.suite_runs,
            downgrade_recommended: cell.is_some_and(|cell| cell.downgrade_recommended),
            recommended_level: cell.map(|cell| cell.recommended_level.clone()).unwrap_or_default(),
            matrix_level: cell.map(|cell| cell.matrix_level.clone()).unwrap_or_default(),
            matrix_rewritten: false,
        });
    }
    rows
}

fn finish(matrix_sha256: String, cells: Vec<CellOut>, nightly: Vec<NightlyOut>, mut findings: Vec<Finding>, historical: Option<Record>) -> Report {
    let mut counts = BTreeMap::new();
    for level in LEVELS {
        counts.insert((*level).to_owned(), 0);
    }
    for cell in &cells {
        *counts.entry(cell.matrix_level.clone()).or_insert(0) += 1;
    }
    let ok = !findings.iter().any(|finding| finding.severity == "error");
    let truncated = findings.len() > 200;
    if truncated {
        findings.truncate(200);
        push(&mut findings, "info", "report.truncated", "", "", "findings capped at 200");
    }
    Report {
        schema: "oo-source-conformance/report/v1".to_owned(),
        ok,
        exit_code: if ok { 0 } else { 1 },
        promoted: false,
        matrix_rewritten: false,
        matrix_sha256,
        counts,
        findings,
        cells,
        nightly,
        historical_pg: historical,
        source_support_consumer: Consumer {
            status: "pending".to_owned(),
            protocol_commit: format!(
                "parse_support_matrix-absent; source-support-{SUPPORT_SHA}; review-{SUPPORT_REVIEW}; archive-{BASE_SHA}"
            ),
            mutable_workspace: "not-read".to_owned(),
            proposed_api: PROPOSED_API.to_owned(),
        },
        case_map: case_map(),
        truncated,
    }
}

fn case_map() -> Vec<CaseMapOut> {
    CASES.iter().map(|case| {
        let applies = match case.applies {
            crate::cases::Applies::Always => "all".to_owned(),
            crate::cases::Applies::Caps(names) => names.join(","),
            crate::cases::Applies::DocumentFamily => "document".to_owned(),
        };
        CaseMapOut { id: case.id.to_owned(), name: case.name.to_owned(), applies }
    }).collect()
}

fn cell_has_error(findings: &[Finding], target: &str, capability: &str) -> bool {
    findings.iter().any(|finding| finding.severity == "error" && finding.target_id == target && finding.capability == capability)
}

fn not_above(recommended: &str, matrix_level: &str) -> String {
    if rank(recommended) > rank(matrix_level) { matrix_level.to_owned() } else { recommended.to_owned() }
}

fn rank(level: &str) -> u8 {
    match level {
        "excluded" => 0,
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

fn push_coded(findings: &mut Vec<Finding>, error: &str, target: &str, capability: &str) {
    let (code, detail) = split_code(error);
    push(findings, "error", &code, target, capability, detail);
}

fn split_code(error: &str) -> (String, String) {
    if let Some((code, detail)) = error.split_once(':') {
        let token = code.trim();
        if token.contains('.') && token.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '_') {
            return (token.to_owned(), detail.trim().to_owned());
        }
    }
    ("evidence.malformed".to_owned(), error.to_owned())
}

fn read_capped(path: &Path, max_bytes: usize, code: &str) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| format!("{code}: {}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("path.symlink: {}", path.display()));
    }
    if !metadata.is_file() {
        return Err(format!("{code}: {}", path.display()));
    }
    if metadata.len() > max_bytes as u64 {
        return Err(format!("{code}: {} exceeds {max_bytes}", path.display()));
    }
    let bytes = fs::read(path).map_err(|_| format!("{code}: {}", path.display()))?;
    if bytes.len() > max_bytes {
        return Err(format!("{code}: {} exceeds {max_bytes}", path.display()));
    }
    Ok(bytes)
}
