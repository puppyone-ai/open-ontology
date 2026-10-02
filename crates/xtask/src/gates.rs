//! Monotonic gate check. A PR manifest is not its own protected baseline.
//!
//! Exit 0 means required jobs matched the bootstrap rules. `phase_signoff` stays
//! `in_review` and `protected_github_policy` stays false until a real owner and
//! branch protection exist. This crate does not observe GitHub settings.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::report::{EXIT_FAIL, EXIT_PENDING, EXIT_USAGE, Issue, Report};
use crate::util::{appointed_logins, phase_rank, repo_root, security_owner_appointed, sha256_hex};

const BASELINE_PLACEHOLDER: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

const FOUNDATION: &[&str] = &[
    "gate-docs",
    "gate-fmt",
    "gate-clippy",
    "gate-unit",
    "gate-contracts",
    "gate-architecture",
    "gate-secrets",
    "gate-dependencies",
    "gate-evidence",
    "gate-aggregate",
];

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GateFile {
    schema_version: String,
    document: String,
    current_phase: String,
    provenance: Provenance,
    gates: Vec<Gate>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Provenance {
    kind: String,
    status: String,
    protected_github_policy: bool,
    owner_appointed: bool,
    base_sha: Option<String>,
    first_enabled_commit: Option<String>,
    note: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Gate {
    id: String,
    status: String,
    reason: String,
    requirements: Vec<String>,
    acceptance: Vec<String>,
    effective_phase: Option<String>,
    latest_phase: Option<String>,
    capability: Option<String>,
    path_prefix: Option<String>,
    job: Option<String>,
    evaluated_by: String,
    success_forbidden: bool,
    echo_forbidden: bool,
    closes_acceptance: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct JobsFile {
    commit: String,
    jobs: Vec<JobRecord>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct JobRecord {
    id: String,
    result: String,
    command: Vec<String>,
    commit: String,
    exit_code: i64,
    log_sha256: String,
    runner: String,
    #[serde(default)]
    log_path: Option<String>,
    #[serde(default)]
    step_script_sha256: String,
    #[serde(default)]
    started_at: String,
    #[serde(default)]
    finished_at: String,
    #[serde(default)]
    run_id: String,
    #[serde(default)]
    capture_path: Option<String>,
}

pub fn run(args: &[String]) -> Report {
    let mut manifest = repo_root().join("ci/gates.yaml");
    let mut baseline = repo_root().join("ci/baseline/gates-baseline.yaml");
    let mut jobs: Option<PathBuf> = None;
    let mut changed: Option<PathBuf> = None;
    let mut phase: Option<String> = None;
    let mut source = "bootstrap".to_owned();
    let mut base_sha = None;
    let mut event = None;
    let mut require_protected = false;
    let mut event_path: Option<PathBuf> = None;
    let mut trusted_base_root: Option<PathBuf> = None;
    let mut repo = repo_root();
    let mut check = false;
    let mut manifest_only = false;
    let mut self_test = false;
    let mut execute_local = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--check" => check = true,
            "--check-manifest" => {
                check = true;
                manifest_only = true;
            }
            "--self-test" => self_test = true,
            "--execute-local" => execute_local = true,
            "--manifest" => {
                index += 1;
                match args.get(index) {
                    Some(value) => manifest = PathBuf::from(value),
                    None => return usage("gates --manifest requires a path"),
                }
            }
            "--baseline" => {
                index += 1;
                match args.get(index) {
                    Some(value) => baseline = PathBuf::from(value),
                    None => return usage("gates --baseline requires a path"),
                }
            }
            "--jobs" => {
                index += 1;
                match args.get(index) {
                    Some(value) => jobs = Some(PathBuf::from(value)),
                    None => return usage("gates --jobs requires a path"),
                }
            }
            "--changed" => {
                index += 1;
                match args.get(index) {
                    Some(value) => changed = Some(PathBuf::from(value)),
                    None => return usage("gates --changed requires a path"),
                }
            }
            "--phase" => {
                index += 1;
                match args.get(index) {
                    Some(value) => phase = Some(value.clone()),
                    None => return usage("gates --phase requires a phase"),
                }
            }
            "--baseline-source" => {
                index += 1;
                match args.get(index) {
                    Some(value) => source = value.clone(),
                    None => {
                        return usage(
                            "gates --baseline-source requires bootstrap, git-object, or git-show",
                        );
                    }
                }
            }
            "--base-sha" => {
                index += 1;
                match args.get(index) {
                    Some(value) => base_sha = Some(value.clone()),
                    None => return usage("gates --base-sha requires a commit"),
                }
            }
            "--workflow-event" => {
                index += 1;
                match args.get(index) {
                    Some(value) => event = Some(value.clone()),
                    None => return usage("gates --workflow-event requires an event name"),
                }
            }
            "--require-protected-policy" => require_protected = true,
            "--event-path" => {
                index += 1;
                match args.get(index) {
                    Some(value) => event_path = Some(PathBuf::from(value)),
                    None => return usage("gates --event-path requires a path"),
                }
            }
            "--trusted-base-root" => {
                index += 1;
                match args.get(index) {
                    Some(value) => trusted_base_root = Some(PathBuf::from(value)),
                    None => return usage("gates --trusted-base-root requires a path"),
                }
            }
            "--repo" => {
                index += 1;
                match args.get(index) {
                    Some(value) => repo = PathBuf::from(value),
                    None => return usage("gates --repo requires a path"),
                }
            }
            "--format" => {
                index += 1;
                match args.get(index).map(String::as_str) {
                    Some("json" | "human") => {}
                    _ => return usage("gates --format requires human or json"),
                }
            }
            "--help" | "-h" => return help(),
            other => return usage(&format!("unknown gates argument {other}")),
        }
        index += 1;
    }
    if self_test {
        return self_test_at(&repo_root());
    }
    if execute_local {
        return execute_local_at(&repo);
    }
    if !check {
        return usage("gates requires --check, --check-manifest, --execute-local, or --self-test");
    }
    if !matches!(source.as_str(), "bootstrap" | "git-show" | "git-object") {
        return Report::fail(
            "gates",
            "failed",
            EXIT_FAIL,
            vec![Issue::new(
                "gate.provenance_claim",
                source,
                "baseline source must be bootstrap, git-object, or git-show. This harness does not claim branch protection.",
            )],
            json!({"protected_github_policy": false}),
        );
    }
    evaluate_request(CheckRequest {
        manifest_path: &manifest,
        baseline_path: &baseline,
        jobs_path: jobs.as_deref(),
        changed_path: changed.as_deref(),
        phase: phase.as_deref(),
        source: &source,
        manifest_only,
        base_sha: base_sha.as_deref(),
        event: event.as_deref(),
        require_protected,
        repo: &repo,
        event_path: event_path.as_deref(),
        trusted_base_root: trusted_base_root.as_deref(),
        local_event_omission: false,
    })
}

fn help() -> Report {
    Report::ok(
        "gates",
        "help",
        json!({
            "usage": "xtask gates --check --jobs JOBS.json | xtask gates --execute-local",
            "check_manifest": "validates structure, monotonicity, and workflow text. jobs are not_supplied. Exit 0 is not a CI pass.",
            "runner_capture": "imported captures and jobs are integrity records. A public binding and producer coordinator are not execution authority. required_satisfied for local engineering comes only from gates --execute-local, which spawns the allowlist in this process. Release authority and CI stay false.",
            "release_authority": false,
            "protected_github_policy": false,
            "note": "A head file is never its own protected baseline. The same path for --manifest and --baseline fails."
        }),
    )
}

fn usage(message: &str) -> Report {
    Report::fail(
        "gates",
        "usage",
        EXIT_USAGE,
        vec![Issue::new("cli.usage", "gates", message)],
        json!({}),
    )
}

struct CheckRequest<'a> {
    manifest_path: &'a Path,
    baseline_path: &'a Path,
    jobs_path: Option<&'a Path>,
    changed_path: Option<&'a Path>,
    phase: Option<&'a str>,
    source: &'a str,
    manifest_only: bool,
    base_sha: Option<&'a str>,
    event: Option<&'a str>,
    require_protected: bool,
    repo: &'a Path,
    event_path: Option<&'a Path>,
    trusted_base_root: Option<&'a Path>,
    local_event_omission: bool,
}

fn evaluate_request(request: CheckRequest<'_>) -> Report {
    if request.source != "git-object"
        && request.source != "git-show"
        && same_path(request.manifest_path, request.baseline_path)
    {
        return Report::fail(
            "gates",
            "failed",
            EXIT_FAIL,
            vec![Issue::new(
                "gate.baseline_is_manifest",
                request.manifest_path.display().to_string(),
                "the PR manifest cannot be the protected baseline",
            )],
            json!({"protected_github_policy": false, "trusted_base_checker": "pending_external"}),
        );
    }
    if let Some(base_root) = request.trusted_base_root
        && same_path(base_root, request.repo)
    {
        return Report::fail(
            "gates",
            "failed",
            EXIT_FAIL,
            vec![Issue::new(
                "gate.repo",
                request.repo.display().to_string(),
                "the trusted base archive is not the candidate. The checker refuses to judge its own root.",
            )],
            json!({"protected_github_policy": false, "trusted_base_checker": "pending_external"}),
        );
    }
    let manifest = match load_gates(request.manifest_path, "manifest") {
        Ok(file) => file,
        Err(issue) => return Report::fail("gates", "failed", EXIT_FAIL, vec![issue], json!({})),
    };
    let (baseline, baseline_bytes, trust) = match resolve_baseline(&request) {
        Ok(resolved) => resolved,
        Err(issue) => {
            let exit = if soft_code(issue.code) {
                EXIT_PENDING
            } else {
                EXIT_FAIL
            };
            return Report::fail(
                "gates",
                if exit == EXIT_PENDING {
                    "pending"
                } else {
                    "failed"
                },
                exit,
                vec![issue],
                json!({
                    "protected_github_policy": false,
                    "trusted_base_checker": "pending_external",
                    "phase_signoff": "in_review"
                }),
            );
        }
    };
    let jobs = match request.jobs_path {
        Some(path) => match load_jobs(path) {
            Ok(jobs) => Some(jobs),
            Err(issue) => {
                return Report::fail("gates", "failed", EXIT_FAIL, vec![issue], json!({}));
            }
        },
        None => None,
    };
    let changed = if let Some(path) = request.changed_path {
        match read_changed_list(path) {
            Ok(lines) => lines,
            Err(issue) => {
                return Report::fail(
                    "gates",
                    "failed",
                    EXIT_FAIL,
                    vec![issue],
                    json!({
                        "protected_github_policy": false,
                        "trusted_base_checker": "pending_external",
                        "phase_signoff": "in_review"
                    }),
                );
            }
        }
    } else {
        Vec::new()
    };
    evaluate(
        &manifest,
        &baseline,
        EvalArgs {
            jobs: jobs.as_ref(),
            changed: &changed,
            phase: request.phase,
            source: request.source,
            manifest_only: request.manifest_only,
            repo: request.repo,
            baseline_sha256: &sha256_hex(&baseline_bytes),
            trust: &trust,
            require_protected: request.require_protected,
        },
    )
}

fn resolve_baseline(request: &CheckRequest<'_>) -> Result<(GateFile, Vec<u8>, String), Issue> {
    if request.source == "git-object" || request.source == "git-show" {
        let sha = request.base_sha.ok_or_else(|| {
            Issue::new(
                "gate.base",
                request.source,
                "a caller string is not a git object. Pass --base-sha and the checker will read the object.",
            )
        })?;
        let (bytes, commit) = git_baseline_bytes(request.repo, sha, request.event)?;
        if let Some(event_path) = request.event_path {
            bind_event(
                request.repo,
                request.event.unwrap_or("pull_request"),
                &commit,
                event_path,
            )?;
        } else if !request.local_event_omission {
            return Err(Issue::new(
                "gate.event",
                request.source,
                "git baseline checks require --event-path. A caller digest is not an event.",
            ));
        }
        let text = String::from_utf8(bytes.clone())
            .map_err(|_| Issue::new("gate.base", sha, "baseline object is not utf-8"))?;
        let file = parse_gates(&text, request.baseline_path, "baseline")?;
        let trust = checker_trust(request.repo, &commit)?;
        return Ok((file, bytes, trust));
    }
    if request.base_sha.is_some() {
        return Err(Issue::new(
            "gate.base",
            "bootstrap",
            "bootstrap does not accept a caller base sha as proof",
        ));
    }
    let bytes = fs::read(request.baseline_path).map_err(|error| {
        Issue::new(
            "gate.read",
            request.baseline_path.display().to_string(),
            error.to_string(),
        )
    })?;
    let text = String::from_utf8(bytes.clone()).map_err(|_| {
        Issue::new(
            "gate.read",
            request.baseline_path.display().to_string(),
            "baseline is not utf-8",
        )
    })?;
    let file = parse_gates(&text, request.baseline_path, "baseline")?;
    Ok((file, bytes, "pending_external".to_owned()))
}

fn git_baseline_bytes(
    repo: &Path,
    sha: &str,
    event: Option<&str>,
) -> Result<(Vec<u8>, String), Issue> {
    if sha.len() != 40
        || !sha.chars().all(|ch| ch.is_ascii_hexdigit())
        || sha.chars().all(|ch| ch == '0')
    {
        return Err(Issue::new(
            "gate.base",
            sha,
            "BASE must be a non-zero 40-hex commit",
        ));
    }
    let commit = git_text(
        repo,
        &["rev-parse", "--verify", &format!("{sha}^{{commit}}")],
    )?;
    let head = git_text(repo, &["rev-parse", "HEAD"])?;
    match event.unwrap_or("pull_request") {
        "schedule" | "workflow_dispatch" => {
            if commit != head {
                return Err(Issue::new(
                    "gate.ancestry",
                    &commit,
                    "schedule and workflow_dispatch require BASE to equal HEAD",
                ));
            }
        }
        "pull_request" | "push" => {
            if commit == head {
                return Err(Issue::new(
                    "gate.ancestry",
                    &commit,
                    "pull_request and push require BASE to be a strict ancestor of the tested HEAD",
                ));
            }
            let output = git_raw(repo, &["merge-base", "--is-ancestor", &commit, "HEAD"])?;
            if !output.status.success() {
                return Err(Issue::new(
                    "gate.ancestry",
                    &commit,
                    "BASE is not an ancestor of HEAD",
                ));
            }
        }
        other => {
            return Err(Issue::new(
                "gate.base",
                other,
                "workflow event must be pull_request, push, schedule, or workflow_dispatch",
            ));
        }
    }
    let bytes = git_raw(
        repo,
        &["show", &format!("{commit}:ci/baseline/gates-baseline.yaml")],
    )?;
    if !bytes.status.success() {
        return Err(Issue::new(
            "gate.base",
            &commit,
            "base commit has no ci/baseline/gates-baseline.yaml. Working-tree baseline is not a fallback.",
        ));
    }
    Ok((bytes.stdout, commit))
}

fn checker_trust(repo: &Path, commit: &str) -> Result<String, Issue> {
    let blob = git_raw(
        repo,
        &["show", &format!("{commit}:crates/xtask/src/gates.rs")],
    )?;
    if !blob.status.success() {
        return Err(Issue::new(
            "gate.checker_absent",
            commit,
            "base commit has no crates/xtask/src/gates.rs. The candidate checker is not the authority.",
        ));
    }
    let disk = fs::read(repo.join("crates/xtask/src/gates.rs")).map_err(|_| {
        Issue::new(
            "gate.checker_untrusted",
            "crates/xtask/src/gates.rs",
            "working tree has no gate checker to compare with the base blob",
        )
    })?;
    if disk != blob.stdout {
        return Err(Issue::new(
            "gate.checker_untrusted",
            "crates/xtask/src/gates.rs",
            "the working-tree checker differs from the base commit. A same-PR checker edit cannot approve itself.",
        ));
    }
    Ok("git_object".to_owned())
}

fn git_text(repo: &Path, args: &[&str]) -> Result<String, Issue> {
    let output = git_raw(repo, args)?;
    if !output.status.success() {
        return Err(Issue::new(
            "gate.base",
            args.first().copied().unwrap_or("git"),
            String::from_utf8_lossy(&output.stderr).trim(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn git_raw(repo: &Path, args: &[&str]) -> Result<std::process::Output, Issue> {
    crate::util::isolated_git(repo)
        .args(args)
        .output()
        .map_err(|error| Issue::new("gate.base", "git", error.to_string()))
}

fn load_gates(path: &Path, expect_document: &str) -> Result<GateFile, Issue> {
    let text = fs::read_to_string(path)
        .map_err(|error| Issue::new("gate.read", path.display().to_string(), error.to_string()))?;
    parse_gates(&text, path, expect_document)
}

fn parse_gates(text: &str, path: &Path, expect_document: &str) -> Result<GateFile, Issue> {
    let file: GateFile = oo_core::yaml::parse_yaml_typed(text, "oo.gates/v1").map_err(|error| {
        Issue::new("gate.schema", path.display().to_string(), error.to_string())
    })?;
    if file.schema_version != "gates/v1" {
        return Err(Issue::new(
            "gate.schema",
            path.display().to_string(),
            "schema_version must be gates/v1",
        ));
    }
    if file.document != expect_document {
        return Err(Issue::new(
            "gate.schema",
            path.display().to_string(),
            format!(
                "document is {} and this argument expects {expect_document}",
                file.document
            ),
        ));
    }
    Ok(file)
}

fn load_jobs(path: &Path) -> Result<JobsFile, Issue> {
    let bytes = fs::read(path)
        .map_err(|error| Issue::new("gate.jobs", path.display().to_string(), error.to_string()))?;
    let text = std::str::from_utf8(&bytes).map_err(|_| {
        Issue::new(
            "gate.jobs",
            path.display().to_string(),
            "jobs file is not utf-8",
        )
    })?;
    let value = oo_core::json_scan::parse_json_value(text, "gates").map_err(|list| {
        Issue::new(
            "gate.jobs",
            path.display().to_string(),
            list.items
                .first()
                .map(|item| item.message.clone())
                .unwrap_or_else(|| "jobs JSON was rejected".to_owned()),
        )
    })?;
    let mut wide = Vec::new();
    reject_wide_job_numbers(&value, &mut wide);
    if let Some(issue) = wide.into_iter().next() {
        return Err(issue);
    }
    serde_json::from_value(value)
        .map_err(|error| Issue::new("gate.jobs", path.display().to_string(), error.to_string()))
}

fn reject_wide_job_numbers(value: &Value, issues: &mut Vec<Issue>) {
    match value {
        Value::Number(number) if number.as_i64().is_none() && number.as_u64().is_none() => {
            issues.push(Issue::new(
                "gate.jobs",
                "jobs",
                "job number is outside an exact integer range",
            ));
        }
        Value::Array(items) => {
            for item in items {
                reject_wide_job_numbers(item, issues);
            }
        }
        Value::Object(map) => {
            for item in map.values() {
                reject_wide_job_numbers(item, issues);
            }
        }
        _ => {}
    }
}

fn read_changed_list(path: &Path) -> Result<Vec<String>, Issue> {
    let metadata = fs::metadata(path).map_err(|_| {
        Issue::new(
            "gate.changed_missing",
            path.display().to_string(),
            "changed list cannot be read",
        )
    })?;
    if !metadata.is_file() {
        return Err(Issue::new(
            "gate.changed_missing",
            path.display().to_string(),
            "changed list cannot be read",
        ));
    }
    let text = fs::read_to_string(path).map_err(|_| {
        Issue::new(
            "gate.changed_missing",
            path.display().to_string(),
            "changed list cannot be read",
        )
    })?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

fn bind_event(repo: &Path, event: &str, base: &str, path: &Path) -> Result<(), Issue> {
    let bytes = fs::read(path).map_err(|_| {
        Issue::new(
            "gate.event",
            path.display().to_string(),
            "event file cannot be read",
        )
    })?;
    let text = std::str::from_utf8(&bytes).map_err(|_| {
        Issue::new(
            "gate.event",
            path.display().to_string(),
            "event file is not utf-8",
        )
    })?;
    let value: Value = serde_json::from_str(text).map_err(|_| {
        Issue::new(
            "gate.event",
            path.display().to_string(),
            "event file is not JSON",
        )
    })?;
    let head = git_text(repo, &["rev-parse", "HEAD"])?;
    let sha_at = |pointer: &str| value.pointer(pointer).and_then(Value::as_str).unwrap_or("");
    match event {
        "pull_request" => {
            if sha_at("/pull_request/base/sha") != base || sha_at("/pull_request/head/sha") != head
            {
                return Err(Issue::new(
                    "gate.event",
                    event,
                    "pull_request base and head do not match the tested commits",
                ));
            }
        }
        "push" => {
            if sha_at("/before") != base || sha_at("/after") != head {
                return Err(Issue::new(
                    "gate.event",
                    event,
                    "push before and after do not match the tested commits",
                ));
            }
        }
        "schedule" | "workflow_dispatch" => {
            if base != head {
                return Err(Issue::new(
                    "gate.event",
                    event,
                    "schedule and workflow_dispatch bind BASE to the tested HEAD",
                ));
            }
            if value.get("pull_request").is_some() {
                return Err(Issue::new(
                    "gate.event",
                    event,
                    "schedule and workflow_dispatch reject a pull_request payload",
                ));
            }
        }
        other => {
            return Err(Issue::new(
                "gate.event",
                other,
                "workflow event must be pull_request, push, schedule, or workflow_dispatch",
            ));
        }
    }
    Ok(())
}

struct EvalArgs<'a> {
    jobs: Option<&'a JobsFile>,
    changed: &'a [String],
    phase: Option<&'a str>,
    source: &'a str,
    manifest_only: bool,
    repo: &'a Path,
    baseline_sha256: &'a str,
    trust: &'a str,
    require_protected: bool,
}

fn evaluate(manifest: &GateFile, baseline: &GateFile, args: EvalArgs<'_>) -> Report {
    let EvalArgs {
        jobs,
        changed,
        phase,
        source,
        manifest_only,
        repo,
        baseline_sha256,
        trust,
        require_protected,
    } = args;
    let mut issues = Vec::new();
    issues.extend(provenance_issues(&manifest.provenance, "manifest"));
    issues.extend(provenance_issues(&baseline.provenance, "baseline"));
    if source == "git-show" && manifest.provenance.protected_github_policy {
        issues.push(Issue::new(
            "gate.protected_claim",
            "git-show",
            "loading a base SHA does not make the manifest protected policy",
        ));
    }
    if let Some(phase) = phase
        && phase != manifest.current_phase
    {
        issues.push(Issue::new(
            "gate.phase_mismatch",
            phase,
            format!("CLI phase differs from manifest {}", manifest.current_phase),
        ));
    }
    match (
        phase_rank(&manifest.current_phase),
        phase_rank(&baseline.current_phase),
    ) {
        (Some(manifest_rank), Some(baseline_rank)) if manifest_rank < baseline_rank => {
            issues.push(Issue::new(
                "gate.phase_regression",
                &manifest.current_phase,
                "current phase moved backward from the baseline",
            ));
        }
        (None, _) | (_, None) => issues.push(Issue::new(
            "gate.phase",
            &manifest.current_phase,
            "phase is not SP00 through SP15",
        )),
        _ => {}
    }
    if manifest.provenance.owner_appointed && appointed_logins(repo).is_empty() {
        issues.push(Issue::new(
            "gate.owner_claim",
            "owner_appointed",
            "CODEOWNERS has no uncommented login",
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for gate in &manifest.gates {
        if !seen.insert(gate.id.clone()) {
            issues.push(Issue::new("gate.duplicate", &gate.id, "duplicate gate id"));
        }
        issues.extend(gate_shape(gate, &manifest.current_phase));
        if FOUNDATION.contains(&gate.id.as_str()) && gate.status != "required" {
            issues.push(Issue::new(
                "gate.foundation",
                &gate.id,
                "foundation gates are required from the first code PR",
            ));
        }
    }
    for base in &baseline.gates {
        let Some(gate) = manifest.gates.iter().find(|item| item.id == base.id) else {
            issues.push(Issue::new(
                "gate.removed",
                &base.id,
                "baseline gate is missing",
            ));
            continue;
        };
        if rank(&gate.status) < rank(&base.status) {
            issues.push(Issue::new(
                "gate.downgrade",
                &gate.id,
                format!("{} became {}", base.status, gate.status),
            ));
        }
        if phase_weakened(&gate.latest_phase, &base.latest_phase) {
            issues.push(Issue::new(
                "gate.deadline_extended",
                &gate.id,
                "latest_phase moved later or was removed",
            ));
        }
        if phase_weakened(&gate.effective_phase, &base.effective_phase) {
            issues.push(Issue::new(
                "gate.deadline_extended",
                &gate.id,
                "effective_phase moved later or was removed",
            ));
        }
        if gate.path_prefix != base.path_prefix || gate.capability != base.capability {
            issues.push(Issue::new(
                "gate.scope",
                &gate.id,
                "path or capability scope changed from the baseline",
            ));
        }
        for requirement in &base.requirements {
            if !gate.requirements.contains(requirement) {
                issues.push(Issue::new(
                    "gate.requirement_dropped",
                    &gate.id,
                    requirement.clone(),
                ));
            }
        }
        for acceptance in &base.acceptance {
            if !gate.acceptance.contains(acceptance) {
                issues.push(Issue::new(
                    "gate.acceptance_dropped",
                    &gate.id,
                    acceptance.clone(),
                ));
            }
        }
    }
    issues.extend(changed_path_issues(repo, changed));
    let (workflow_issues, workflow_commands) = scan_workflows(repo);
    issues.extend(workflow_issues);
    if require_protected && !manifest.provenance.protected_github_policy {
        issues.push(Issue::new(
            "gate.policy_absent",
            "protected_github_policy",
            "the baseline does not record protected GitHub policy",
        ));
    }
    let caller_supplied_digest_ignored = source == "bootstrap"
        && !manifest_only
        && std::env::var("OO_HARNESS_APPROVED_BASELINE_SHA256")
            .ok()
            .is_some_and(|hash| !hash.is_empty());
    if source == "bootstrap" && !manifest_only {
        issues.push(Issue::new(
            "gate.baseline_approval",
            "baseline",
            "bootstrap has no external trusted appointment. A caller digest is not approval.",
        ));
    }
    let mut pending = Vec::new();
    let mut required_ok = jobs.is_some() && !manifest_only;
    let mut runner_authority = "absent";
    if !manifest_only && jobs.is_none() {
        issues.push(Issue::new(
            "gate.job_missing",
            "jobs",
            "--check requires a jobs file. Use --check-manifest when jobs are not supplied.",
        ));
    }
    if let Some(jobs) = jobs {
        if !jobs.commit.chars().all(|ch| ch.is_ascii_hexdigit()) || jobs.commit.len() != 40 {
            issues.push(Issue::new(
                "gate.wrong_build",
                &jobs.commit,
                "jobs commit is not 40 hex",
            ));
        }
        if matches!(source, "git-object" | "git-show") {
            match git_text(repo, &["rev-parse", "HEAD"]) {
                Ok(head) if head == jobs.commit => {}
                Ok(_) => issues.push(Issue::new(
                    "gate.wrong_build",
                    &jobs.commit,
                    "jobs commit is not the tested HEAD",
                )),
                Err(issue) => issues.push(issue),
            }
        }
        for job in &jobs.jobs {
            if job.commit != jobs.commit {
                issues.push(Issue::new(
                    "gate.wrong_build",
                    &job.id,
                    "job commit does not match the jobs file",
                ));
            }
            if !manifest
                .gates
                .iter()
                .any(|gate| gate.job.as_deref() == Some(job.id.as_str()) || gate.id == job.id)
            {
                issues.push(Issue::new(
                    "gate.unknown_job",
                    &job.id,
                    "job does not belong to a gate",
                ));
            }
        }
        let mut seen_bindings = BTreeSet::new();
        let mut coordinator_bound = false;
        let mut capture_runs = BTreeSet::new();
        for gate in &manifest.gates {
            if gate.evaluated_by == "checker" {
                if jobs
                    .jobs
                    .iter()
                    .any(|job| job.id == gate.id && job.result == "success")
                {
                    issues.push(Issue::new(
                        "gate.aggregate_echo",
                        &gate.id,
                        "the checker evaluates this gate. A success job is not accepted.",
                    ));
                }
                continue;
            }
            let introduced = capability_activated(gate, changed);
            let job = jobs
                .jobs
                .iter()
                .find(|job| Some(job.id.as_str()) == gate.job.as_deref() || job.id == gate.id);
            if introduced && gate.status != "required" {
                issues.push(Issue::new(
                    "gate.capability_inactive",
                    &gate.id,
                    format!(
                        "capability {} requires this gate to be required in the same change",
                        gate.capability.as_deref().unwrap_or("unnamed")
                    ),
                ));
            }
            let mut command_ok = false;
            if let Some(job) = job {
                match command_issue(
                    gate,
                    job,
                    workflow_commands.get(&gate.id).map(String::as_str),
                    repo,
                    &jobs.commit,
                    &mut seen_bindings,
                    &mut capture_runs,
                ) {
                    Some(issue) => {
                        required_ok = false;
                        issues.push(issue);
                    }
                    None if job.result == "success" => {
                        command_ok = true;
                        coordinator_bound = true;
                    }
                    None => {}
                }
            }
            match (gate.status.as_str(), job) {
                ("required", None) => {
                    required_ok = false;
                    issues.push(Issue::new(
                        "gate.job_missing",
                        &gate.id,
                        "required gate has no job",
                    ));
                }
                ("required", Some(job)) if job.result == "skipped" || job.result == "skip" => {
                    required_ok = false;
                    issues.push(Issue::new(
                        "gate.skip",
                        &gate.id,
                        "a required gate cannot be skipped",
                    ));
                }
                ("required", Some(job)) if job.result != "success" => {
                    required_ok = false;
                    issues.push(Issue::new(
                        "gate.job_failed",
                        &gate.id,
                        format!("result {}", job.result),
                    ));
                }
                (_, Some(job))
                    if job.result == "success"
                        && (gate.success_forbidden || shell_failure(&job.command).is_some()) =>
                {
                    required_ok = false;
                    let code = if gate.success_forbidden {
                        "gate.false_pass"
                    } else {
                        shell_failure(&job.command).unwrap_or("gate.echo")
                    };
                    issues.push(Issue::new(
                        code,
                        &gate.id,
                        "success is not allowed for this gate or command",
                    ));
                }
                ("not_applicable_until_phase", Some(job)) if job.result == "success" => {
                    issues.push(Issue::new(
                        "gate.not_applicable_green",
                        &gate.id,
                        "not_applicable cannot be reported as success",
                    ));
                }
                ("pending", Some(job)) if job.result == "success" => {
                    issues.push(Issue::new(
                        "gate.false_pass",
                        &gate.id,
                        "pending is not a pass",
                    ));
                }
                ("pending", _) => pending.push(gate.id.clone()),
                ("not_applicable_until_phase", _) => {}
                ("required", Some(_)) => {}
                _ => {}
            }
            if introduced && !(command_ok && gate.status == "required") {
                issues.push(Issue::new(
                    "gate.capability_unproven",
                    &gate.id,
                    "capability introduction needs a coordinator capture of the workflow command",
                ));
            }
        }
        if capture_runs.len() > 1 {
            issues.push(Issue::new(
                "gate.log",
                "run_id",
                "jobs do not share one run id",
            ));
        }
        runner_authority = if coordinator_bound {
            "local-public-test"
        } else {
            "absent"
        };
    } else {
        required_ok = false;
        for gate in &manifest.gates {
            if gate.status == "pending" {
                pending.push(gate.id.clone());
            }
        }
    }
    let workflow_clean = !issues.iter().any(|issue| issue.code == "gate.workflow");
    let status = if manifest_only && issues.is_empty() {
        "manifest_coherent"
    } else if jobs.is_some() && issues.is_empty() && required_ok {
        "required_satisfied"
    } else if issues.is_empty() {
        "manifest_coherent"
    } else {
        "failed"
    };
    let _ = workflow_clean;
    let soft = !issues.is_empty() && issues.iter().all(|issue| soft_code(issue.code));
    let details = json!({
        "aggregate": if soft { "pending" } else { status },
        "jobs": if jobs.is_some() && !manifest_only { "checked" } else { "not_supplied" },
        "baseline_source": source,
        "baseline_sha256": baseline_sha256,
        "protected_github_policy": false,
        "trusted_base_checker": trust,
        "bootstrap_approval": if source == "bootstrap" { if issues.iter().any(|issue| issue.code == "gate.baseline_approval") { "pending" } else { "recorded" } } else { "not_bootstrap" },
        "caller_supplied_digest_ignored": caller_supplied_digest_ignored,
        "phase_signoff": "in_review",
        "ci_passed": false,
        "release_authority": false,
        "runner_authority": runner_authority,
        "production_fallback": false,
        "pending_gates": pending,
        "acceptance_closed": [],
        "closes_acceptance": manifest.gates.iter().filter(|gate| gate.closes_acceptance).map(|gate| gate.id.clone()).collect::<Vec<_>>(),
        "security_owner_appointed": security_owner_appointed(repo),
        "current_phase": manifest.current_phase,
    });
    if issues.is_empty() {
        Report::ok("gates", status, details)
    } else if soft {
        Report::fail("gates", "pending", EXIT_PENDING, issues, details)
    } else {
        Report::fail("gates", "failed", EXIT_FAIL, issues, details)
    }
}

fn soft_code(code: &str) -> bool {
    code == "gate.baseline_approval" || code == "gate.checker_absent" || code == "gate.unverified"
}

fn provenance_issues(provenance: &Provenance, label: &str) -> Vec<Issue> {
    let mut issues = Vec::new();
    if provenance.protected_github_policy {
        issues.push(Issue::new(
            "gate.protected_claim",
            label,
            "protected_github_policy is not claimable. No branch protection is configured.",
        ));
    }
    if provenance.status != "in_review" {
        issues.push(Issue::new(
            "gate.provenance",
            label,
            "bootstrap provenance status must be in_review",
        ));
    }
    if provenance.kind != "bootstrap" && provenance.kind != "git-show" {
        issues.push(Issue::new(
            "gate.provenance",
            label,
            "provenance kind must be bootstrap or git-show",
        ));
    }
    if provenance.note.trim().is_empty() {
        issues.push(Issue::new(
            "gate.provenance",
            label,
            "provenance note is empty",
        ));
    }
    if let Some(sha) = &provenance.first_enabled_commit {
        if sha.len() != 40 || !sha.chars().all(|ch| ch.is_ascii_hexdigit()) {
            issues.push(Issue::new(
                "gate.provenance",
                label,
                "first_enabled_commit is not 40 hex",
            ));
        }
    } else if provenance.protected_github_policy {
        issues.push(Issue::new(
            "gate.protected_claim",
            label,
            "a protected claim with a null first_enabled_commit is rejected",
        ));
    }
    if provenance
        .base_sha
        .as_ref()
        .is_some_and(|sha| sha.len() != 40)
    {
        issues.push(Issue::new(
            "gate.provenance",
            label,
            "base_sha is not 40 hex",
        ));
    }
    issues
}

fn gate_shape(gate: &Gate, current: &str) -> Vec<Issue> {
    let mut issues = Vec::new();
    if rank(&gate.status) < 0 {
        issues.push(Issue::new(
            "gate.unknown_status",
            &gate.id,
            gate.status.clone(),
        ));
    }
    if gate.reason.trim().is_empty() {
        issues.push(Issue::new("gate.reason", &gate.id, "reason is empty"));
    }
    if !gate.echo_forbidden {
        issues.push(Issue::new(
            "gate.echo",
            &gate.id,
            "echo_forbidden must stay true",
        ));
    }
    for requirement in &gate.requirements {
        if !requirement.starts_with("REQ-") {
            issues.push(Issue::new(
                "gate.requirement",
                &gate.id,
                requirement.clone(),
            ));
        }
    }
    for acceptance in &gate.acceptance {
        if !acceptance.starts_with("AT-") {
            issues.push(Issue::new("gate.acceptance", &gate.id, acceptance.clone()));
        }
    }
    if gate.status == "not_applicable_until_phase" {
        if gate.effective_phase.is_none() || gate.latest_phase.is_none() {
            issues.push(Issue::new(
                "gate.deadline",
                &gate.id,
                "not_applicable needs effective_phase and latest_phase",
            ));
        }
        if let Some(effective) = gate.effective_phase.as_deref().and_then(phase_rank)
            && phase_rank(current).unwrap_or(0) >= effective
        {
            issues.push(Issue::new(
                "gate.effective_reached",
                &gate.id,
                format!("effective phase reached at {current}; not_applicable is no longer valid"),
            ));
        }
        if let Some(latest) = gate.latest_phase.as_deref().and_then(phase_rank)
            && phase_rank(current).unwrap_or(0) >= latest
        {
            issues.push(Issue::new(
                "gate.expired",
                &gate.id,
                format!("not_applicable expired at {current}"),
            ));
        }
    }
    if gate.status == "pending"
        && let Some(latest) = gate.latest_phase.as_deref().and_then(phase_rank)
        && phase_rank(current).unwrap_or(0) >= latest
    {
        issues.push(Issue::new(
            "gate.expired",
            &gate.id,
            "pending passed its latest phase",
        ));
    }
    if gate.status == "required" && gate.evaluated_by == "job" && gate.job.is_none() {
        issues.push(Issue::new(
            "gate.job_missing",
            &gate.id,
            "required job gate has no job id",
        ));
    }
    if gate.evaluated_by != "job" && gate.evaluated_by != "checker" {
        issues.push(Issue::new(
            "gate.evaluated_by",
            &gate.id,
            gate.evaluated_by.clone(),
        ));
    }
    issues
}

fn changed_path_issues(repo: &Path, changed: &[String]) -> Vec<Issue> {
    let mut issues = Vec::new();
    for path in changed {
        if path.is_empty()
            || path.starts_with('/')
            || path.starts_with('~')
            || path.split(['/', '\\']).any(|part| part == "..")
        {
            issues.push(Issue::new(
                "gate.changed_path",
                path,
                "changed path is empty, absolute, or escapes",
            ));
            continue;
        }
        if !repo.join(path).exists() {
            issues.push(Issue::new(
                "gate.changed_missing",
                path,
                "changed path does not exist. A missing path does not keep the gate inactive.",
            ));
        }
    }
    issues
}

fn capability_activated(gate: &Gate, changed: &[String]) -> bool {
    let Some(prefix) = gate.path_prefix.as_deref() else {
        return false;
    };
    if prefix.is_empty() {
        return false;
    }
    changed.iter().any(|path| {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path.as_str());
        if name == "CAPABILITY_INTRODUCED" {
            return false;
        }
        path == prefix.trim_end_matches('/') || path.starts_with(prefix)
    })
}

fn shell_failure(command: &[String]) -> Option<&'static str> {
    if command.is_empty() {
        return Some("gate.echo");
    }
    let base = command[0]
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(command[0].as_str());
    if matches!(base, "echo" | "true" | ":" | "printf") {
        return Some("gate.echo");
    }
    if !matches!(base, "bash" | "sh" | "dash" | "zsh" | "ksh") {
        return None;
    }
    let script = command
        .windows(2)
        .find(|pair| pair[0] == "-c")
        .map(|pair| pair[1].as_str())
        .unwrap_or("");
    let first = script.split_whitespace().next().unwrap_or("");
    let first = first.rsplit(['/', '\\']).next().unwrap_or(first);
    if matches!(first, "" | "echo" | "true" | ":" | "printf" | "command") {
        Some("gate.echo")
    } else {
        Some("gate.untrusted_shell")
    }
}

fn allowed_command(gate_id: &str) -> Option<&'static [&'static str]> {
    Some(match gate_id {
        "gate-docs" => &["cargo", "xtask", "docs", "--check", "--format", "json"],
        "gate-fmt" => &["cargo", "fmt", "--all", "--", "--check"],
        "gate-clippy" => &[
            "cargo",
            "clippy",
            "--workspace",
            "--locked",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
        "gate-unit" => &["cargo", "test", "--workspace", "--locked"],
        "gate-contracts" => &["cargo", "xtask", "contracts", "--check", "--format", "json"],
        "gate-architecture" => &[
            "cargo",
            "xtask",
            "architecture",
            "--check",
            "--format",
            "json",
        ],
        "gate-secrets" => &["cargo", "xtask", "secrets", "--check", "--format", "json"],
        "gate-dependencies" => &[
            "cargo",
            "xtask",
            "supply",
            "--run-tools",
            "--format",
            "json",
        ],
        "gate-evidence" => &[
            "cargo",
            "xtask",
            "evidence",
            "--self-test",
            "--format",
            "json",
        ],
        _ => return None,
    })
}

fn split_command(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_owned).collect()
}

fn command_issue(
    gate: &Gate,
    job: &JobRecord,
    extracted: Option<&str>,
    repo: &Path,
    source_commit: &str,
    seen_bindings: &mut BTreeSet<String>,
    capture_runs: &mut BTreeSet<String>,
) -> Option<Issue> {
    if job.command.iter().any(|arg| arg == "--library-only") {
        return Some(Issue::new(
            "gate.command",
            &gate.id,
            "library-only cannot satisfy a required full gate",
        ));
    }
    if let Some(code) = shell_failure(&job.command) {
        return Some(Issue::new(
            code,
            &gate.id,
            "a shell wrapper cannot mark this gate successful",
        ));
    }
    if job.runner == "xtask-self-test" {
        return Some(Issue::new(
            "gate.runner",
            &gate.id,
            "production does not accept runner xtask-self-test. A shaped digest is not a log.",
        ));
    }
    if job.result != "success" {
        return None;
    }
    let Some(capture) = job.capture_path.as_deref() else {
        return Some(Issue::new(
            "gate.runner",
            &gate.id,
            "runner capture is absent. A caller hash is not execution of the workflow command.",
        ));
    };
    let Some(line) = extracted else {
        return Some(Issue::new(
            "gate.command",
            &gate.id,
            "workflow step for this gate is missing",
        ));
    };
    let job_id = gate.job.as_deref().unwrap_or(gate.id.as_str());
    let verified = match crate::runner_capture::verify_capture(
        Path::new(capture),
        &crate::runner_capture::VerifyExpect {
            repo,
            source: source_commit,
            job: job_id,
            step: job_id,
            step_script: line,
            run_id: None,
        },
    ) {
        Ok(verified) => verified,
        Err(issue) => return Some(issue),
    };
    if !seen_bindings.insert(verified.binding.clone()) {
        return Some(Issue::new(
            "gate.log",
            &gate.id,
            "replayed receipt was already used for another gate",
        ));
    }
    capture_runs.insert(verified.run_id);
    if verified.exit_code != 0 {
        return Some(Issue::new(
            "gate.job_failed",
            &gate.id,
            "captured status is nonzero",
        ));
    }
    let executed = split_command(&verified.step_script);
    if let Some(code) = shell_failure(&executed) {
        return Some(Issue::new(
            code,
            &gate.id,
            "the captured command is a shell wrapper",
        ));
    }
    if let Some(expected) = allowed_command(&gate.id) {
        let expected: Vec<String> = expected.iter().map(|arg| (*arg).to_owned()).collect();
        if executed != expected || verified.step_script != line {
            return Some(Issue::new(
                "gate.command",
                &gate.id,
                "captured command is not the workflow step for this gate",
            ));
        }
    } else if executed.first().map(String::as_str) != Some("cargo") {
        return Some(Issue::new(
            "gate.command",
            &gate.id,
            "a new gate command must be cargo",
        ));
    }
    Some(Issue::new(
        "gate.unverified",
        &gate.id,
        "imported capture is an integrity record. A public binding and producer coordinator are not execution authority.",
    ))
}

fn rank(status: &str) -> i32 {
    match status {
        "optional" => 0,
        "not_applicable_until_phase" => 1,
        "pending" => 2,
        "required" => 3,
        _ => -1,
    }
}

fn phase_weakened(manifest: &Option<String>, baseline: &Option<String>) -> bool {
    match (manifest.as_deref(), baseline.as_deref()) {
        (None, Some(_)) => true,
        (Some(manifest), Some(baseline)) => match (phase_rank(manifest), phase_rank(baseline)) {
            (Some(manifest), Some(baseline)) => manifest > baseline,
            _ => true,
        },
        _ => false,
    }
}

fn scan_workflows(repo: &Path) -> (Vec<Issue>, BTreeMap<String, String>) {
    let mut issues = Vec::new();
    let mut commands = BTreeMap::new();
    let dir = repo.join(".github/workflows");
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => {
            issues.push(Issue::new(
                "gate.workflow",
                ".github/workflows",
                "workflow directory is missing",
            ));
            return (issues, commands);
        }
    };
    let mut found = 0;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                issues.push(Issue::new(
                    "gate.workflow",
                    ".github/workflows",
                    "workflow directory cannot be read",
                ));
                continue;
            }
        };
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|item| item.to_str())
            .unwrap_or("");
        if !name.ends_with(".yml") && !name.ends_with(".yaml") {
            continue;
        }
        found += 1;
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(_) => {
                issues.push(Issue::new("gate.workflow", name, "workflow cannot be read"));
                continue;
            }
        };
        for (id, command) in extract_job_commands(&text) {
            if let Some(existing) = commands.get(&id) {
                if existing != &command {
                    issues.push(Issue::new(
                        "gate.workflow",
                        &id,
                        "workflow files do not agree on the gate command",
                    ));
                }
            } else {
                commands.insert(id, command);
            }
        }
        for token in [
            "pull_request_target",
            "self-hosted",
            "id-token:",
            "contents: write",
            "secrets.",
        ] {
            if text.contains(token) {
                issues.push(Issue::new(
                    "gate.workflow",
                    name,
                    format!("forbidden workflow text {token}"),
                ));
            }
        }
        if text.contains("|| true") {
            issues.push(Issue::new(
                "gate.workflow",
                name,
                "workflow uses || true. A failed fetch or diff is not a fallback.",
            ));
        }
        let runs = text.matches("runs-on:").count();
        let timeouts = text.matches("timeout-minutes:").count();
        if timeouts < runs {
            issues.push(Issue::new(
                "gate.workflow",
                name,
                "a job is missing timeout-minutes",
            ));
        }
        if !text.contains("contents: read") {
            issues.push(Issue::new(
                "gate.workflow",
                name,
                "permissions contents: read is missing",
            ));
        }
        if !text.contains("ubuntu-24.04") {
            issues.push(Issue::new(
                "gate.workflow",
                name,
                "runner must be ubuntu-24.04",
            ));
        }
    }
    if found == 0 {
        issues.push(Issue::new(
            "gate.workflow",
            ".github/workflows",
            "no workflow files",
        ));
    }
    (issues, commands)
}

fn extract_job_commands(text: &str) -> BTreeMap<String, String> {
    let mut jobs = BTreeMap::new();
    let mut in_jobs = false;
    let mut current: Option<String> = None;
    for line in text.lines() {
        if !in_jobs {
            if line.starts_with("jobs:") {
                in_jobs = true;
            }
            continue;
        }
        if !line.is_empty() && !line.starts_with(' ') && !line.starts_with('#') {
            break;
        }
        if let Some(name) = job_heading(line) {
            current = Some(gate_id_for_job(name));
            continue;
        }
        let Some(job) = current.as_deref() else {
            continue;
        };
        let Some(command) = tee_gate_command(line) else {
            continue;
        };
        jobs.entry(job.to_owned()).or_insert(command);
    }
    jobs
}

fn job_heading(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("  ")?;
    if rest.starts_with(' ') || rest.starts_with('#') {
        return None;
    }
    let (name, tail) = rest.split_once(':')?;
    if !tail.trim().is_empty() || name.is_empty() {
        return None;
    }
    if name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    {
        Some(name)
    } else {
        None
    }
}

fn gate_id_for_job(name: &str) -> String {
    if name == "secret-scan" {
        "gate-secrets".to_owned()
    } else {
        name.to_owned()
    }
}

fn tee_gate_command(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let (left, _) = trimmed.split_once("| tee")?;
    let left = left.trim();
    let first = left.split_whitespace().next()?;
    let base = first.rsplit(['/', '\\']).next().unwrap_or(first);
    if base == "rustc" || left.is_empty() {
        None
    } else {
        Some(left.to_owned())
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

pub fn self_test_at(repo: &Path) -> Report {
    let mut issues = Vec::new();
    let manifest_path = repo.join("ci/gates.yaml");
    let baseline_path = repo.join("ci/baseline/gates-baseline.yaml");
    let coherent = evaluate_request(CheckRequest {
        manifest_path: &manifest_path,
        baseline_path: &baseline_path,
        jobs_path: None,
        changed_path: None,
        phase: Some("SP00"),
        source: "bootstrap",
        manifest_only: true,
        base_sha: None,
        event: None,
        require_protected: false,
        repo,
        event_path: None,
        trusted_base_root: None,
        local_event_omission: true,
    });
    if coherent.exit_code != 0 {
        issues.push(Issue::new(
            "gate.self_test",
            "manifest",
            format!(
                "checked-in gates are not coherent: {}",
                coherent.render("json")
            ),
        ));
    }
    let manifest = match load_gates(&manifest_path, "manifest") {
        Ok(file) => file,
        Err(issue) => return Report::fail("gates", "failed", EXIT_FAIL, vec![issue], json!({})),
    };
    let baseline = match load_gates(&baseline_path, "baseline") {
        Ok(file) => file,
        Err(issue) => return Report::fail("gates", "failed", EXIT_FAIL, vec![issue], json!({})),
    };
    let jobs = synthetic_jobs(&manifest, false);
    let control = evaluate(
        &manifest,
        &baseline,
        EvalArgs {
            jobs: Some(&jobs),
            changed: &[],
            phase: Some("SP00"),
            source: "bootstrap",
            manifest_only: false,
            repo,
            baseline_sha256: BASELINE_PLACEHOLDER,
            trust: "pending_external",
            require_protected: false,
        },
    );
    if control.exit_code == 0
        || !control
            .issues
            .iter()
            .any(|issue| issue.code == "gate.runner")
        || !control
            .issues
            .iter()
            .any(|issue| issue.code == "gate.baseline_approval")
    {
        issues.push(Issue::new(
            "gate.self_test",
            "control",
            control.render("json"),
        ));
    }
    let mut missing = jobs.jobs.clone();
    missing.pop();
    let missing_jobs = JobsFile {
        commit: jobs.commit.clone(),
        jobs: missing,
    };
    let missing_report = evaluate(
        &manifest,
        &baseline,
        EvalArgs {
            jobs: Some(&missing_jobs),
            changed: &[],
            phase: Some("SP00"),
            source: "bootstrap",
            manifest_only: false,
            repo,
            baseline_sha256: BASELINE_PLACEHOLDER,
            trust: "pending_external",
            require_protected: false,
        },
    );
    expect_code(
        &missing_report,
        "gate.job_missing",
        "job-omission",
        &mut issues,
    );
    let mut skipped = jobs.jobs.clone();
    if let Some(job) = skipped.first_mut() {
        job.result = "skipped".to_owned();
    }
    let skipped_report = evaluate(
        &manifest,
        &baseline,
        EvalArgs {
            jobs: Some(&JobsFile {
                commit: jobs.commit.clone(),
                jobs: skipped,
            }),
            changed: &[],
            phase: Some("SP00"),
            source: "bootstrap",
            manifest_only: false,
            repo,
            baseline_sha256: BASELINE_PLACEHOLDER,
            trust: "pending_external",
            require_protected: false,
        },
    );
    expect_code(&skipped_report, "gate.skip", "skip", &mut issues);
    let mut echo = jobs.jobs.clone();
    if let Some(job) = echo.first_mut() {
        job.command = vec!["echo".to_owned(), "success".to_owned()];
    }
    let echo_report = evaluate(
        &manifest,
        &baseline,
        EvalArgs {
            jobs: Some(&JobsFile {
                commit: jobs.commit.clone(),
                jobs: echo,
            }),
            changed: &[],
            phase: Some("SP00"),
            source: "bootstrap",
            manifest_only: false,
            repo,
            baseline_sha256: BASELINE_PLACEHOLDER,
            trust: "pending_external",
            require_protected: false,
        },
    );
    expect_code(&echo_report, "gate.echo", "echo", &mut issues);
    let mut wrapped = jobs.jobs.clone();
    if let Some(job) = wrapped.first_mut() {
        job.command = vec![
            "bash".to_owned(),
            "-c".to_owned(),
            "echo success".to_owned(),
        ];
    }
    let wrapped_report = evaluate(
        &manifest,
        &baseline,
        EvalArgs {
            jobs: Some(&JobsFile {
                commit: jobs.commit.clone(),
                jobs: wrapped,
            }),
            changed: &[],
            phase: Some("SP00"),
            source: "bootstrap",
            manifest_only: false,
            repo,
            baseline_sha256: BASELINE_PLACEHOLDER,
            trust: "pending_external",
            require_protected: false,
        },
    );
    expect_code(&wrapped_report, "gate.echo", "bash-echo", &mut issues);
    let mut library = jobs.jobs.clone();
    if let Some(job) = library.first_mut() {
        job.command = vec![
            "cargo".to_owned(),
            "xtask".to_owned(),
            "contracts".to_owned(),
            "--check".to_owned(),
            "--library-only".to_owned(),
            "--format".to_owned(),
            "json".to_owned(),
        ];
    }
    let library_report = evaluate(
        &manifest,
        &baseline,
        EvalArgs {
            jobs: Some(&JobsFile {
                commit: jobs.commit.clone(),
                jobs: library,
            }),
            changed: &[],
            phase: Some("SP00"),
            source: "bootstrap",
            manifest_only: false,
            repo,
            baseline_sha256: BASELINE_PLACEHOLDER,
            trust: "pending_external",
            require_protected: false,
        },
    );
    expect_code(&library_report, "gate.command", "library-only", &mut issues);
    let caller_string = evaluate_request(CheckRequest {
        manifest_path: &manifest_path,
        baseline_path: &baseline_path,
        jobs_path: None,
        changed_path: None,
        phase: Some("SP00"),
        source: "git-show",
        manifest_only: true,
        base_sha: None,
        event: Some("pull_request"),
        require_protected: false,
        repo,
        event_path: None,
        trusted_base_root: None,
        local_event_omission: true,
    });
    expect_code(&caller_string, "gate.base", "git-show-string", &mut issues);
    let mut na = jobs.jobs.clone();
    if let Some(gate) = manifest
        .gates
        .iter()
        .find(|gate| gate.status == "not_applicable_until_phase")
    {
        na.push(JobRecord {
            id: gate.job.clone().unwrap_or_else(|| gate.id.clone()),
            result: "success".to_owned(),
            command: vec![
                "cargo".to_owned(),
                "xtask".to_owned(),
                "test".to_owned(),
                "--suite".to_owned(),
                "integration-a".to_owned(),
            ],
            commit: jobs.commit.clone(),
            exit_code: 0,
            log_sha256: "ab".repeat(32),
            runner: "xtask-self-test".to_owned(),
            log_path: None,
            step_script_sha256: String::new(),
            started_at: String::new(),
            finished_at: String::new(),
            run_id: String::new(),
            capture_path: None,
        });
    }
    let na_report = evaluate(
        &manifest,
        &baseline,
        EvalArgs {
            jobs: Some(&JobsFile {
                commit: jobs.commit.clone(),
                jobs: na,
            }),
            changed: &[],
            phase: Some("SP00"),
            source: "bootstrap",
            manifest_only: false,
            repo,
            baseline_sha256: BASELINE_PLACEHOLDER,
            trust: "pending_external",
            require_protected: false,
        },
    );
    if !na_report.issues.iter().any(|issue| {
        issue.code == "gate.not_applicable_green"
            || issue.code == "gate.echo"
            || issue.code == "gate.false_pass"
    }) {
        issues.push(Issue::new(
            "gate.self_test",
            "not-applicable",
            na_report.render("json"),
        ));
    }
    let mut downgraded = manifest.clone();
    if let Some(gate) = downgraded
        .gates
        .iter_mut()
        .find(|gate| gate.id == "gate-fmt")
    {
        gate.status = "optional".to_owned();
    }
    let down = evaluate(
        &downgraded,
        &baseline,
        EvalArgs {
            jobs: Some(&jobs),
            changed: &[],
            phase: Some("SP00"),
            source: "bootstrap",
            manifest_only: false,
            repo,
            baseline_sha256: BASELINE_PLACEHOLDER,
            trust: "pending_external",
            require_protected: false,
        },
    );
    expect_code(&down, "gate.downgrade", "downgrade", &mut issues);
    let mut claimed = manifest.clone();
    claimed.provenance.protected_github_policy = true;
    let claim = evaluate(
        &claimed,
        &baseline,
        EvalArgs {
            jobs: None,
            changed: &[],
            phase: Some("SP00"),
            source: "bootstrap",
            manifest_only: true,
            repo,
            baseline_sha256: BASELINE_PLACEHOLDER,
            trust: "pending_external",
            require_protected: false,
        },
    );
    expect_code(&claim, "gate.protected_claim", "protected", &mut issues);
    let sentinel = evaluate(
        &manifest,
        &baseline,
        EvalArgs {
            jobs: Some(&jobs),
            changed: &["crates/oo-pipeline/CAPABILITY_INTRODUCED".to_owned()],
            phase: Some("SP00"),
            source: "bootstrap",
            manifest_only: false,
            repo,
            baseline_sha256: BASELINE_PLACEHOLDER,
            trust: "pending_external",
            require_protected: false,
        },
    );
    if sentinel
        .issues
        .iter()
        .any(|issue| issue.code == "gate.capability_inactive")
    {
        issues.push(Issue::new(
            "gate.self_test",
            "sentinel",
            "a sentinel file activated a capability gate",
        ));
    }
    let introduced = evaluate(
        &manifest,
        &baseline,
        EvalArgs {
            jobs: Some(&jobs),
            changed: &["crates/oo-pipeline/src/lib.rs".to_owned()],
            phase: Some("SP00"),
            source: "bootstrap",
            manifest_only: false,
            repo,
            baseline_sha256: BASELINE_PLACEHOLDER,
            trust: "pending_external",
            require_protected: false,
        },
    );
    expect_code(
        &introduced,
        "gate.capability_inactive",
        "capability",
        &mut issues,
    );
    let missing_path = evaluate(
        &manifest,
        &baseline,
        EvalArgs {
            jobs: Some(&jobs),
            changed: &["crates/oo-pipeline/missing-source.rs".to_owned()],
            phase: Some("SP00"),
            source: "bootstrap",
            manifest_only: false,
            repo,
            baseline_sha256: BASELINE_PLACEHOLDER,
            trust: "pending_external",
            require_protected: false,
        },
    );
    expect_code(
        &missing_path,
        "gate.changed_missing",
        "missing-path",
        &mut issues,
    );
    let mut late = manifest.clone();
    late.current_phase = "SP11".to_owned();
    let reached = evaluate(
        &late,
        &baseline,
        EvalArgs {
            jobs: None,
            changed: &[],
            phase: Some("SP11"),
            source: "bootstrap",
            manifest_only: true,
            repo,
            baseline_sha256: BASELINE_PLACEHOLDER,
            trust: "pending_external",
            require_protected: false,
        },
    );
    expect_code(
        &reached,
        "gate.effective_reached",
        "effective-phase",
        &mut issues,
    );
    let same = evaluate_request(CheckRequest {
        manifest_path: &manifest_path,
        baseline_path: &manifest_path,
        jobs_path: None,
        changed_path: None,
        phase: Some("SP00"),
        source: "bootstrap",
        manifest_only: true,
        base_sha: None,
        event: None,
        require_protected: false,
        repo,
        event_path: None,
        trusted_base_root: None,
        local_event_omission: true,
    });
    expect_code(&same, "gate.baseline_is_manifest", "same-path", &mut issues);
    let sealed_root = std::env::temp_dir().join(format!("oo-xtask-sealed-{}", std::process::id()));
    let _ = fs::remove_dir_all(&sealed_root);
    match seal_foundation_jobs(repo, &manifest, &"a".repeat(40), "5150", &sealed_root) {
        Ok(sealed) => {
            let restored = evaluate(
                &manifest,
                &baseline,
                EvalArgs {
                    jobs: Some(&sealed),
                    changed: &[],
                    phase: Some("SP00"),
                    source: "bootstrap",
                    manifest_only: false,
                    repo,
                    baseline_sha256: BASELINE_PLACEHOLDER,
                    trust: "pending_external",
                    require_protected: false,
                },
            );
            let authority = restored
                .details
                .get("runner_authority")
                .and_then(|item| item.as_str());
            if restored.exit_code != EXIT_PENDING
                || restored.status == "required_satisfied"
                || !restored
                    .issues
                    .iter()
                    .any(|issue| issue.code == "gate.baseline_approval")
                || !restored
                    .issues
                    .iter()
                    .any(|issue| issue.code == "gate.unverified")
                || restored.issues.iter().any(|issue| {
                    issue.code != "gate.baseline_approval" && issue.code != "gate.unverified"
                })
                || authority != Some("absent")
                || restored.details.get("release_authority") != Some(&json!(false))
                || restored.details.get("ci_passed") != Some(&json!(false))
                || restored.details.get("protected_github_policy") != Some(&json!(false))
            {
                issues.push(Issue::new(
                    "gate.self_test",
                    "restored",
                    restored.render("json"),
                ));
            }
        }
        Err(error) => issues.push(Issue::new("gate.self_test", "restored", error)),
    }
    let _ = fs::remove_dir_all(&sealed_root);
    let missing_list = repo.join("ci/no-such-changed-list.txt");
    let missing_changed = evaluate_request(CheckRequest {
        manifest_path: &manifest_path,
        baseline_path: &baseline_path,
        jobs_path: None,
        changed_path: Some(&missing_list),
        phase: Some("SP00"),
        source: "bootstrap",
        manifest_only: true,
        base_sha: None,
        event: None,
        require_protected: false,
        repo,
        event_path: None,
        trusted_base_root: None,
        local_event_omission: true,
    });
    expect_code(
        &missing_changed,
        "gate.changed_missing",
        "changed-file-missing",
        &mut issues,
    );
    let changed_dir = std::env::temp_dir().join(format!("oo-xtask-changed-{}", std::process::id()));
    let _ = fs::create_dir_all(&changed_dir);
    let unreadable_changed = evaluate_request(CheckRequest {
        manifest_path: &manifest_path,
        baseline_path: &baseline_path,
        jobs_path: None,
        changed_path: Some(&changed_dir),
        phase: Some("SP00"),
        source: "bootstrap",
        manifest_only: true,
        base_sha: None,
        event: None,
        require_protected: false,
        repo,
        event_path: None,
        trusted_base_root: None,
        local_event_omission: true,
    });
    expect_code(
        &unreadable_changed,
        "gate.changed_missing",
        "changed-file-unreadable",
        &mut issues,
    );
    let _ = fs::remove_dir_all(&changed_dir);
    let self_root = evaluate_request(CheckRequest {
        manifest_path: &manifest_path,
        baseline_path: &baseline_path,
        jobs_path: None,
        changed_path: None,
        phase: Some("SP00"),
        source: "bootstrap",
        manifest_only: true,
        base_sha: None,
        event: None,
        require_protected: false,
        repo,
        event_path: None,
        trusted_base_root: Some(repo),
        local_event_omission: true,
    });
    expect_code(
        &self_root,
        "gate.repo",
        "trusted-base-is-candidate",
        &mut issues,
    );
    command_binding_probe(repo, &manifest, &baseline, &mut issues);
    git_object_proof(repo, &mut issues);
    Report::from_issues(
        "gates",
        "self_test_pass",
        "failed",
        EXIT_FAIL,
        issues,
        json!({"protected_github_policy": false, "phase_signoff": "in_review"}),
    )
}

fn synthetic_jobs(manifest: &GateFile, echo: bool) -> JobsFile {
    let commit = "a".repeat(40);
    let jobs = manifest
        .gates
        .iter()
        .filter(|gate| gate.status == "required" && gate.evaluated_by == "job")
        .map(|gate| JobRecord {
            id: gate.job.clone().unwrap_or_else(|| gate.id.clone()),
            result: "success".to_owned(),
            command: if echo {
                vec!["echo".to_owned(), "success".to_owned()]
            } else {
                allowed_command(&gate.id)
                    .unwrap_or(&[])
                    .iter()
                    .map(|arg| (*arg).to_owned())
                    .collect()
            },
            commit: commit.clone(),
            exit_code: 0,
            log_sha256: "ab".repeat(32),
            runner: "xtask-self-test".to_owned(),
            log_path: None,
            step_script_sha256: String::new(),
            started_at: String::new(),
            finished_at: String::new(),
            run_id: String::new(),
            capture_path: None,
        })
        .collect();
    JobsFile { commit, jobs }
}

fn command_binding_probe(
    repo: &Path,
    manifest: &GateFile,
    baseline: &GateFile,
    issues: &mut Vec<Issue>,
) {
    let Some(gate) = manifest.gates.iter().find(|gate| gate.id == "gate-docs") else {
        issues.push(Issue::new(
            "gate.self_test",
            "command-binding",
            "gate-docs is missing",
        ));
        return;
    };
    let (workflow_issues, commands) = scan_workflows(repo);
    if !workflow_issues.is_empty() {
        issues.push(Issue::new(
            "gate.self_test",
            "command-binding",
            workflow_issues[0].message.clone(),
        ));
        return;
    }
    let Some(line) = commands.get("gate-docs").cloned() else {
        issues.push(Issue::new(
            "gate.self_test",
            "command-binding",
            "gate-docs workflow step is missing",
        ));
        return;
    };
    let source = "a".repeat(40);
    let other_source = "b".repeat(40);
    let run_id = "5150";
    let job_id = gate.job.as_deref().unwrap_or("gate-docs");
    let command = split_command(&line);
    let root = std::env::temp_dir().join(format!("oo-xtask-capture-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    if fs::create_dir_all(&root).is_err() {
        issues.push(Issue::new(
            "gate.self_test",
            "command-binding",
            "cannot create the capture fixture",
        ));
        return;
    }
    let fix = SealFix {
        repo,
        source: &source,
        run_id,
    };

    let absent = probe_job(job_id, "github-actions", None, &command, &source);
    let (mut seen, mut runs) = empty_sets();
    expect_issue(
        command_issue(
            gate,
            &absent,
            Some(&line),
            repo,
            &source,
            &mut seen,
            &mut runs,
        ),
        "gate.runner",
        "absent",
        "absent-runner",
        issues,
    );

    let forged = probe_job(
        job_id,
        "xtask-self-test",
        Some(root.join("unused").display().to_string()),
        &command,
        &source,
    );
    let (mut seen, mut runs) = empty_sets();
    expect_issue(
        command_issue(
            gate,
            &forged,
            Some(&line),
            repo,
            &source,
            &mut seen,
            &mut runs,
        ),
        "gate.runner",
        "xtask-self-test",
        "forged-runner",
        issues,
    );

    let positive_dir = root.join("positive");
    if let Err(error) = seal_stdout(
        &positive_dir,
        &fix,
        job_id,
        job_id,
        &line,
        b"bound-by-coordinator\n",
    ) {
        issues.push(Issue::new("gate.self_test", "positive", error));
        let _ = fs::remove_dir_all(&root);
        return;
    }
    let positive = probe_job(
        job_id,
        "coordinator",
        Some(positive_dir.display().to_string()),
        &command,
        &source,
    );
    let (mut seen, mut runs) = empty_sets();
    match command_issue(
        gate,
        &positive,
        Some(&line),
        repo,
        &source,
        &mut seen,
        &mut runs,
    ) {
        Some(issue)
            if issue.code == "gate.unverified" && issue.message.contains("integrity record") => {}
        Some(issue) => issues.push(Issue::new(
            "gate.self_test",
            "positive",
            format!(
                "imported coordinator capture wanted gate.unverified, got {} {}",
                issue.code, issue.message
            ),
        )),
        None => issues.push(Issue::new(
            "gate.self_test",
            "positive",
            "imported coordinator capture was accepted as execution",
        )),
    }
    expect_issue(
        command_issue(
            gate,
            &positive,
            Some(&line),
            repo,
            &source,
            &mut seen,
            &mut runs,
        ),
        "gate.log",
        "replayed",
        "replayed-receipt",
        issues,
    );

    let missing_dir = root.join("missing-log");
    if seal_stdout(
        &missing_dir,
        &fix,
        job_id,
        job_id,
        &line,
        b"bound-by-coordinator\n",
    )
    .is_ok()
    {
        let _ = fs::remove_file(missing_dir.join("stdout"));
        let job = probe_job(
            job_id,
            "coordinator",
            Some(missing_dir.display().to_string()),
            &command,
            &source,
        );
        let (mut seen, mut runs) = empty_sets();
        expect_issue(
            command_issue(gate, &job, Some(&line), repo, &source, &mut seen, &mut runs),
            "gate.log",
            "missing",
            "missing-log",
            issues,
        );
    } else {
        issues.push(Issue::new(
            "gate.self_test",
            "missing-log",
            "cannot seal the capture",
        ));
    }

    let echo_dir = root.join("echo");
    if seal_stdout(&echo_dir, &fix, job_id, job_id, &line, b"echo ok\n").is_ok() {
        let job = probe_job(
            job_id,
            "coordinator",
            Some(echo_dir.display().to_string()),
            &command,
            &source,
        );
        let (mut seen, mut runs) = empty_sets();
        expect_issue(
            command_issue(gate, &job, Some(&line), repo, &source, &mut seen, &mut runs),
            "gate.echo",
            "echo",
            "echo-transcript",
            issues,
        );
    } else {
        issues.push(Issue::new(
            "gate.self_test",
            "echo-transcript",
            "cannot seal the capture",
        ));
    }

    let script_dir = root.join("other-script");
    if seal_stdout(
        &script_dir,
        &fix,
        job_id,
        job_id,
        "echo ok",
        b"bound-by-coordinator\n",
    )
    .is_ok()
    {
        let job = probe_job(
            job_id,
            "coordinator",
            Some(script_dir.display().to_string()),
            &command,
            &source,
        );
        let (mut seen, mut runs) = empty_sets();
        expect_issue(
            command_issue(gate, &job, Some(&line), repo, &source, &mut seen, &mut runs),
            "gate.command",
            "step script",
            "different-script",
            issues,
        );
    } else {
        issues.push(Issue::new(
            "gate.self_test",
            "different-script",
            "cannot seal the capture",
        ));
    }

    let step_dir = root.join("other-step");
    if seal_stdout(
        &step_dir,
        &fix,
        "other-step",
        "other-step",
        &line,
        b"bound-by-coordinator\n",
    )
    .is_ok()
    {
        let job = probe_job(
            job_id,
            "coordinator",
            Some(step_dir.display().to_string()),
            &command,
            &source,
        );
        let (mut seen, mut runs) = empty_sets();
        expect_issue(
            command_issue(gate, &job, Some(&line), repo, &source, &mut seen, &mut runs),
            "gate.log",
            "different job or step",
            "different-step",
            issues,
        );
    } else {
        issues.push(Issue::new(
            "gate.self_test",
            "different-step",
            "cannot seal the capture",
        ));
    }

    let source_dir = root.join("wrong-source");
    let other = SealFix {
        repo,
        source: &other_source,
        run_id,
    };
    if seal_stdout(
        &source_dir,
        &other,
        job_id,
        job_id,
        &line,
        b"bound-by-coordinator\n",
    )
    .is_ok()
    {
        let job = probe_job(
            job_id,
            "coordinator",
            Some(source_dir.display().to_string()),
            &command,
            &source,
        );
        let (mut seen, mut runs) = empty_sets();
        expect_issue(
            command_issue(gate, &job, Some(&line), repo, &source, &mut seen, &mut runs),
            "gate.log",
            "does not match this source",
            "wrong-source",
            issues,
        );
    } else {
        issues.push(Issue::new(
            "gate.self_test",
            "wrong-source",
            "cannot seal the capture",
        ));
    }

    let run_dir = root.join("wrong-run");
    if seal_stdout(
        &run_dir,
        &fix,
        job_id,
        job_id,
        &line,
        b"bound-by-coordinator\n",
    )
    .is_ok()
    {
        match crate::runner_capture::verify_capture(
            &run_dir,
            &crate::runner_capture::VerifyExpect {
                repo,
                source: &source,
                job: job_id,
                step: job_id,
                step_script: &line,
                run_id: Some("9999"),
            },
        ) {
            Err(issue)
                if issue.code == "gate.log"
                    && issue.message.contains("does not match this run") => {}
            Err(issue) => issues.push(Issue::new(
                "gate.self_test",
                "wrong-run",
                format!("{} {}", issue.code, issue.message),
            )),
            Ok(_) => issues.push(Issue::new(
                "gate.self_test",
                "wrong-run",
                "a receipt from another run was accepted",
            )),
        }
    } else {
        issues.push(Issue::new(
            "gate.self_test",
            "wrong-run",
            "cannot seal the capture",
        ));
    }

    let tamper_dir = root.join("tamper");
    if seal_stdout(
        &tamper_dir,
        &fix,
        job_id,
        job_id,
        &line,
        b"bound-by-coordinator\n",
    )
    .is_ok()
        && fs::write(tamper_dir.join("stdout"), b"tampered\n").is_ok()
    {
        let job = probe_job(
            job_id,
            "coordinator",
            Some(tamper_dir.display().to_string()),
            &command,
            &source,
        );
        let (mut seen, mut runs) = empty_sets();
        expect_issue(
            command_issue(gate, &job, Some(&line), repo, &source, &mut seen, &mut runs),
            "gate.log",
            "tampered",
            "tampered-bytes",
            issues,
        );
    } else {
        issues.push(Issue::new(
            "gate.self_test",
            "tampered-bytes",
            "cannot seal the capture",
        ));
    }

    let forged_dir = root.join("forged-producer");
    if seal_stdout(
        &forged_dir,
        &fix,
        job_id,
        job_id,
        &line,
        b"bound-by-coordinator\n",
    )
    .is_ok()
        && rewrite_producer(&forged_dir, "xtask-self-test").is_ok()
    {
        let job = probe_job(
            job_id,
            "coordinator",
            Some(forged_dir.display().to_string()),
            &command,
            &source,
        );
        let (mut seen, mut runs) = empty_sets();
        expect_issue(
            command_issue(gate, &job, Some(&line), repo, &source, &mut seen, &mut runs),
            "gate.runner",
            "forged runner",
            "forged-producer",
            issues,
        );
    } else {
        issues.push(Issue::new(
            "gate.self_test",
            "forged-producer",
            "cannot seal the capture",
        ));
    }

    let actions_dir = root.join("github-actions");
    if seal_stdout(
        &actions_dir,
        &fix,
        job_id,
        job_id,
        &line,
        b"bound-by-coordinator\n",
    )
    .is_ok()
        && rewrite_producer(&actions_dir, "github-actions").is_ok()
    {
        let job = probe_job(
            job_id,
            "github-actions",
            Some(actions_dir.display().to_string()),
            &command,
            &source,
        );
        let (mut seen, mut runs) = empty_sets();
        expect_issue(
            command_issue(gate, &job, Some(&line), repo, &source, &mut seen, &mut runs),
            "gate.runner",
            "does not invent GitHub credentials",
            "unauthenticated-actions",
            issues,
        );
    } else {
        issues.push(Issue::new(
            "gate.self_test",
            "unauthenticated-actions",
            "cannot seal the capture",
        ));
    }

    let cargo_text = b"cargo xtask docs --check --format json\nFinished dev\n";
    let mut lied = probe_job(job_id, "github-actions", None, &command, &source);
    lied.log_path = Some(root.join("cargo-text.log").display().to_string());
    lied.log_sha256 = sha256_hex(cargo_text);
    let _ = fs::write(root.join("cargo-text.log"), cargo_text);
    let (mut seen, mut runs) = empty_sets();
    expect_issue(
        command_issue(
            gate,
            &lied,
            Some(&line),
            repo,
            &source,
            &mut seen,
            &mut runs,
        ),
        "gate.runner",
        "absent",
        "cargo-text-without-capture",
        issues,
    );
    let text_dir = root.join("cargo-text-sealed");
    if seal_stdout(&text_dir, &fix, job_id, job_id, &line, cargo_text).is_ok() {
        let job = probe_job(
            job_id,
            "coordinator",
            Some(text_dir.display().to_string()),
            &command,
            &source,
        );
        let (mut seen, mut runs) = empty_sets();
        match command_issue(gate, &job, Some(&line), repo, &source, &mut seen, &mut runs) {
            Some(issue) if issue.code == "gate.unverified" => {}
            Some(issue) => issues.push(Issue::new(
                "gate.self_test",
                "cargo-text-sealed",
                format!(
                    "sealed cargo text wanted gate.unverified, got {} {}",
                    issue.code, issue.message
                ),
            )),
            None => issues.push(Issue::new(
                "gate.self_test",
                "cargo-text-sealed",
                "sealed cargo text was accepted as execution",
            )),
        }
    } else {
        issues.push(Issue::new(
            "gate.self_test",
            "cargo-text-sealed",
            "cannot seal the capture",
        ));
    }

    let true_dir = root.join("true-exec");
    let true_argv = vec!["/usr/bin/true".to_owned()];
    match crate::runner_capture::execute_and_seal(
        &true_dir, repo, &source, run_id, job_id, job_id, &true_argv,
    ) {
        Ok(0) => {
            match crate::runner_capture::verify_capture(
                &true_dir,
                &crate::runner_capture::VerifyExpect {
                    repo,
                    source: &source,
                    job: job_id,
                    step: job_id,
                    step_script: "/usr/bin/true",
                    run_id: Some(run_id),
                },
            ) {
                Ok(verified) if verified.exit_code == 0 && verified.stdout.is_empty() => {}
                Ok(_) => issues.push(Issue::new(
                    "gate.self_test",
                    "executed-true",
                    "true did not seal an empty successful transcript",
                )),
                Err(issue) => issues.push(Issue::new(
                    "gate.self_test",
                    "executed-true",
                    format!("{} {}", issue.code, issue.message),
                )),
            }
            let job = probe_job(
                job_id,
                "coordinator",
                Some(true_dir.display().to_string()),
                &command,
                &source,
            );
            let (mut seen, mut runs) = empty_sets();
            expect_issue(
                command_issue(gate, &job, Some(&line), repo, &source, &mut seen, &mut runs),
                "gate.command",
                "step script",
                "executed-true-is-not-docs",
                issues,
            );
        }
        Ok(code) => issues.push(Issue::new(
            "gate.self_test",
            "executed-true",
            format!("true exited {code}"),
        )),
        Err(error) => {
            let platform_limit = !cfg!(target_os = "linux") && error.contains("containment");
            if !platform_limit {
                issues.push(Issue::new("gate.self_test", "executed-true", error));
            }
        }
    }

    let mixed_root = root.join("mixed-runs");
    match seal_foundation_jobs(repo, manifest, &source, "5150", &mixed_root) {
        Ok(mut jobs) => {
            let fmt_dir = root.join("fmt-other-run");
            let fmt_line = commands.get("gate-fmt").map(String::as_str).unwrap_or("");
            let other_run = SealFix {
                repo,
                source: &source,
                run_id: "9999",
            };
            if seal_stdout(
                &fmt_dir,
                &other_run,
                "gate-fmt",
                "gate-fmt",
                fmt_line,
                b"bound-by-coordinator\n",
            )
            .is_ok()
            {
                if !jobs.jobs.iter().any(|job| job.id == "gate-fmt") {
                    issues.push(Issue::new(
                        "gate.self_test",
                        "different-run",
                        "gate-fmt was not sealed",
                    ));
                    let _ = fs::remove_dir_all(&root);
                    return;
                }
                if let Some(job) = jobs.jobs.iter_mut().find(|job| job.id == "gate-fmt") {
                    job.capture_path = Some(fmt_dir.display().to_string());
                }
                let report = evaluate(
                    manifest,
                    baseline,
                    EvalArgs {
                        jobs: Some(&jobs),
                        changed: &[],
                        phase: Some("SP00"),
                        source: "bootstrap",
                        manifest_only: false,
                        repo,
                        baseline_sha256: BASELINE_PLACEHOLDER,
                        trust: "pending_external",
                        require_protected: false,
                    },
                );
                if !report
                    .issues
                    .iter()
                    .any(|issue| issue.code == "gate.log" && issue.message.contains("one run"))
                {
                    issues.push(Issue::new(
                        "gate.self_test",
                        "different-run",
                        report.render("json"),
                    ));
                }
            } else {
                issues.push(Issue::new(
                    "gate.self_test",
                    "different-run",
                    "cannot reseal gate-fmt",
                ));
            }
        }
        Err(error) => issues.push(Issue::new("gate.self_test", "different-run", error)),
    }
    let _ = fs::remove_dir_all(&root);
}

fn git_object_proof(repo: &Path, issues: &mut Vec<Issue>) {
    let temp = std::env::temp_dir().join(format!("oo-xtask-gates-{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp);
    let manifest_path = temp.join("ci/gates.yaml");
    let baseline_path = temp.join("ci/baseline/gates-baseline.yaml");
    let checker_path = temp.join("crates/xtask/src/gates.rs");
    if fs::create_dir_all(baseline_path.parent().unwrap()).is_err()
        || fs::create_dir_all(checker_path.parent().unwrap()).is_err()
        || fs::create_dir_all(temp.join(".github/workflows")).is_err()
    {
        issues.push(Issue::new(
            "gate.self_test",
            "git-object",
            "cannot create the temp git fixture",
        ));
        return;
    }
    let copies = [
        ("ci/gates.yaml", manifest_path.as_path()),
        ("ci/baseline/gates-baseline.yaml", baseline_path.as_path()),
        ("crates/xtask/src/gates.rs", checker_path.as_path()),
    ];
    for (relative, dest) in copies {
        match fs::read(repo.join(relative)) {
            Ok(bytes) => {
                if fs::write(dest, bytes).is_err() {
                    issues.push(Issue::new("gate.self_test", "git-object", "copy failed"));
                    return;
                }
            }
            Err(error) => {
                issues.push(Issue::new("gate.self_test", relative, error.to_string()));
                return;
            }
        }
    }
    if let Ok(entries) = fs::read_dir(repo.join(".github/workflows")) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let dest = temp.join(".github/workflows").join(name);
            if fs::copy(entry.path(), &dest).is_ok() {
                loosen_file(&dest);
            }
        }
    }
    if let Err(error) = git_ok(&temp, &["init"])
        .and_then(|_| git_ok(&temp, &["add", "--", "."]))
        .and_then(|_| {
            git_ok(
                &temp,
                &[
                    "-c",
                    "user.email=xtask@localhost",
                    "-c",
                    "user.name=xtask",
                    "-c",
                    "commit.gpgsign=false",
                    "commit",
                    "-m",
                    "baseline",
                ],
            )
        })
    {
        issues.push(Issue::new(
            "gate.self_test",
            "git-object",
            format!("temp baseline commit failed: {error}"),
        ));
        return;
    }
    let parent = match git_text(&temp, &["rev-parse", "HEAD"]) {
        Ok(sha) => sha,
        Err(issue) => {
            issues.push(issue);
            return;
        }
    };
    let workflow = temp.join(".github/workflows/pr.yml");
    let mut workflow_text = match fs::read_to_string(&workflow) {
        Ok(text) => text,
        Err(error) => {
            issues.push(Issue::new(
                "gate.self_test",
                "git-object",
                format!("temp workflow is missing: {error}"),
            ));
            return;
        }
    };
    workflow_text
        .push_str("\n# candidate note. Protected gate commands stay on their tee lines.\n");
    if let Err(error) = fs::write(&workflow, &workflow_text) {
        issues.push(Issue::new(
            "gate.self_test",
            "git-object",
            format!("temp candidate workflow write failed: {error}"),
        ));
        return;
    }
    if let Err(error) = git_ok(&temp, &["add", "--", ".github/workflows/pr.yml"]) {
        issues.push(Issue::new(
            "gate.self_test",
            "git-object",
            format!("temp candidate add failed: {error}"),
        ));
        return;
    }
    if let Err(error) = git_ok(
        &temp,
        &[
            "-c",
            "user.email=xtask@localhost",
            "-c",
            "user.name=xtask",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "candidate",
        ],
    ) {
        issues.push(Issue::new(
            "gate.self_test",
            "git-object",
            format!("temp candidate commit failed: {error}"),
        ));
        return;
    }
    let head = match git_text(&temp, &["rev-parse", "HEAD"]) {
        Ok(sha) => sha,
        Err(issue) => {
            issues.push(issue);
            return;
        }
    };
    let manifest = match load_gates(&manifest_path, "manifest") {
        Ok(file) => file,
        Err(issue) => {
            issues.push(issue);
            return;
        }
    };
    let jobs = synthetic_jobs(&manifest, false);
    let jobs_path = temp.join("jobs.json");
    let jobs_json = json!({
        "commit": head,
        "jobs": jobs.jobs.iter().map(|job| json!({
            "id": job.id,
            "result": job.result,
            "command": job.command,
            "commit": head,
            "exit_code": job.exit_code,
            "log_sha256": job.log_sha256,
            "runner": job.runner
        })).collect::<Vec<_>>()
    });
    if fs::write(&jobs_path, jobs_json.to_string()).is_err() {
        issues.push(Issue::new(
            "gate.self_test",
            "git-object",
            "cannot write jobs",
        ));
        return;
    }
    let event_path = temp.join("event.json");
    let pull_request_event = format!(
        "{{\"pull_request\":{{\"base\":{{\"sha\":\"{parent}\"}},\"head\":{{\"sha\":\"{head}\"}}}}}}"
    );
    if fs::write(&event_path, &pull_request_event).is_err() {
        issues.push(Issue::new(
            "gate.self_test",
            "git-object",
            "cannot write the event",
        ));
        return;
    }
    let proof = evaluate_request(CheckRequest {
        manifest_path: &manifest_path,
        baseline_path: &baseline_path,
        jobs_path: Some(&jobs_path),
        changed_path: None,
        phase: Some("SP00"),
        source: "git-object",
        manifest_only: false,
        base_sha: Some(&parent),
        event: Some("pull_request"),
        require_protected: false,
        repo: &temp,
        event_path: Some(&event_path),
        trusted_base_root: None,
        local_event_omission: false,
    });
    if proof.exit_code == 0
        || !proof.issues.iter().any(|issue| issue.code == "gate.runner")
        || proof
            .details
            .get("trusted_base_checker")
            .and_then(|item| item.as_str())
            != Some("git_object")
        || proof.details.get("protected_github_policy") != Some(&json!(false))
        || proof.details.get("release_authority") != Some(&json!(false))
        || proof.details.get("ci_passed") != Some(&json!(false))
        || proof
            .details
            .get("runner_authority")
            .and_then(|item| item.as_str())
            != Some("absent")
    {
        issues.push(Issue::new(
            "gate.self_test",
            "git-object",
            proof.render("json"),
        ));
    }
    let capture_root = temp.join("captures");
    let sealed = match seal_foundation_jobs(&temp, &manifest, &head, "5150", &capture_root) {
        Ok(file) => file,
        Err(error) => {
            issues.push(Issue::new("gate.self_test", "coordinator-capture", error));
            let _ = fs::remove_dir_all(&temp);
            return;
        }
    };
    if let Err(error) = write_jobs_file(&jobs_path, &sealed) {
        issues.push(Issue::new("gate.self_test", "coordinator-capture", error));
        let _ = fs::remove_dir_all(&temp);
        return;
    }
    let bound = evaluate_request(CheckRequest {
        manifest_path: &manifest_path,
        baseline_path: &baseline_path,
        jobs_path: Some(&jobs_path),
        changed_path: None,
        phase: Some("SP00"),
        source: "git-object",
        manifest_only: false,
        base_sha: Some(&parent),
        event: Some("pull_request"),
        require_protected: false,
        repo: &temp,
        event_path: Some(&event_path),
        trusted_base_root: None,
        local_event_omission: false,
    });
    if bound.exit_code == 0
        || bound.status == "required_satisfied"
        || !bound
            .issues
            .iter()
            .any(|issue| issue.code == "gate.unverified")
        || bound
            .details
            .get("runner_authority")
            .and_then(|item| item.as_str())
            != Some("absent")
        || bound.details.get("release_authority") != Some(&json!(false))
        || bound.details.get("ci_passed") != Some(&json!(false))
        || bound.details.get("protected_github_policy") != Some(&json!(false))
        || bound
            .details
            .get("trusted_base_checker")
            .and_then(|item| item.as_str())
            != Some("git_object")
        || bound
            .details
            .get("phase_signoff")
            .and_then(|item| item.as_str())
            != Some("in_review")
    {
        issues.push(Issue::new(
            "gate.self_test",
            "coordinator-capture",
            bound.render("json"),
        ));
    }
    let same_head = evaluate_request(CheckRequest {
        manifest_path: &manifest_path,
        baseline_path: &baseline_path,
        jobs_path: Some(&jobs_path),
        changed_path: None,
        phase: Some("SP00"),
        source: "git-object",
        manifest_only: false,
        base_sha: Some(&head),
        event: Some("pull_request"),
        require_protected: false,
        repo: &temp,
        event_path: None,
        trusted_base_root: None,
        local_event_omission: true,
    });
    expect_code(&same_head, "gate.ancestry", "self-base", issues);
    let dispatch_path = temp.join("dispatch.json");
    if fs::write(&dispatch_path, "{\"workflow_dispatch\":true}\n").is_err() {
        issues.push(Issue::new(
            "gate.self_test",
            "dispatch",
            "cannot write the dispatch event",
        ));
    } else {
        let dispatch = evaluate_request(CheckRequest {
            manifest_path: &manifest_path,
            baseline_path: &baseline_path,
            jobs_path: Some(&jobs_path),
            changed_path: None,
            phase: Some("SP00"),
            source: "git-object",
            manifest_only: false,
            base_sha: Some(&head),
            event: Some("workflow_dispatch"),
            require_protected: false,
            repo: &temp,
            event_path: Some(&dispatch_path),
            trusted_base_root: None,
            local_event_omission: false,
        });
        if dispatch.exit_code == 0
            || dispatch.status == "required_satisfied"
            || !dispatch
                .issues
                .iter()
                .any(|issue| issue.code == "gate.unverified")
            || dispatch.details.get("release_authority") != Some(&json!(false))
            || dispatch.details.get("ci_passed") != Some(&json!(false))
            || dispatch.details.get("protected_github_policy") != Some(&json!(false))
            || dispatch
                .details
                .get("runner_authority")
                .and_then(|item| item.as_str())
                != Some("absent")
        {
            issues.push(Issue::new(
                "gate.self_test",
                "dispatch-same-head",
                dispatch.render("json"),
            ));
        }
    }
    let _ = fs::write(
        &event_path,
        format!(
            "{{\"pull_request\":{{\"base\":{{\"sha\":\"{parent}\"}},\"head\":{{\"sha\":\"{}\"}}}}}}",
            "dead".repeat(10)
        ),
    );
    let event_tamper = evaluate_request(CheckRequest {
        manifest_path: &manifest_path,
        baseline_path: &baseline_path,
        jobs_path: Some(&jobs_path),
        changed_path: None,
        phase: Some("SP00"),
        source: "git-object",
        manifest_only: false,
        base_sha: Some(&parent),
        event: Some("pull_request"),
        require_protected: false,
        repo: &temp,
        event_path: Some(&event_path),
        trusted_base_root: None,
        local_event_omission: false,
    });
    expect_code(&event_tamper, "gate.event", "event-tamper", issues);
    let _ = fs::write(&event_path, &pull_request_event);
    let original_manifest = fs::read_to_string(&manifest_path).unwrap_or_default();
    let mutated = original_manifest.replacen(
        "  - id: gate-fmt\n    status: required\n",
        "  - id: gate-fmt\n    status: optional\n",
        1,
    );
    if mutated == original_manifest {
        issues.push(Issue::new(
            "gate.self_test",
            "dual-edit",
            "gate-fmt status anchor was not found in the temp manifest",
        ));
    } else {
        let _ = fs::write(&manifest_path, &mutated);
        let _ = fs::write(
            &baseline_path,
            "document: baseline\ncurrent_phase: SP00\ngates: []\n",
        );
        let downgraded = evaluate_request(CheckRequest {
            manifest_path: &manifest_path,
            baseline_path: &baseline_path,
            jobs_path: Some(&jobs_path),
            changed_path: None,
            phase: Some("SP00"),
            source: "git-object",
            manifest_only: false,
            base_sha: Some(&parent),
            event: Some("pull_request"),
            require_protected: false,
            repo: &temp,
            event_path: Some(&event_path),
            trusted_base_root: None,
            local_event_omission: false,
        });
        if downgraded.exit_code == 0
            || !downgraded
                .issues
                .iter()
                .any(|issue| issue.code == "gate.downgrade")
            || downgraded
                .issues
                .iter()
                .any(|issue| issue.code == "gate.schema")
        {
            issues.push(Issue::new(
                "gate.self_test",
                "dual-edit",
                downgraded.render("json"),
            ));
        }
        let _ = fs::write(&manifest_path, &original_manifest);
    }
    let _ = fs::write(&checker_path, b"fn tampered() {}\n");
    let tampered = evaluate_request(CheckRequest {
        manifest_path: &manifest_path,
        baseline_path: &baseline_path,
        jobs_path: Some(&jobs_path),
        changed_path: None,
        phase: Some("SP00"),
        source: "git-object",
        manifest_only: false,
        base_sha: Some(&parent),
        event: Some("pull_request"),
        require_protected: false,
        repo: &temp,
        event_path: Some(&event_path),
        trusted_base_root: None,
        local_event_omission: false,
    });
    if tampered.exit_code == 0
        || !tampered
            .issues
            .iter()
            .any(|issue| issue.code == "gate.checker_untrusted")
    {
        issues.push(Issue::new(
            "gate.self_test",
            "checker-tamper",
            tampered.render("json"),
        ));
    }
    let _ = fs::remove_dir_all(&temp);
}

fn git_ok(repo: &Path, args: &[&str]) -> Result<(), String> {
    let output = crate::util::isolated_git(repo)
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(git_stderr(&output))
    }
}

fn git_stderr(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if stderr.is_empty() {
        format!("git exited {}", output.status.code().unwrap_or(-1))
    } else {
        stderr
    }
}

fn loosen_file(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o644);
    let _ = fs::set_permissions(path, permissions);
}

fn expect_code(report: &Report, code: &str, label: &str, issues: &mut Vec<Issue>) {
    if report.exit_code == 0 || !report.issues.iter().any(|issue| issue.code == code) {
        issues.push(Issue::new(
            "gate.self_test",
            label,
            format!("wanted {code}, got {}", report.render("json")),
        ));
    }
}

fn execute_local_at(repo: &Path) -> Report {
    match crate::runner_capture::gate_deadline_from_env() {
        Ok(deadline) => execute_local_with_deadline(repo, deadline),
        Err(message) => Report::fail(
            "gates",
            "usage",
            EXIT_USAGE,
            vec![Issue::new(
                "gate.deadline",
                "OO_XTASK_GATE_DEADLINE_SECS",
                message,
            )],
            json!({
                "protected_github_policy": false,
                "release_authority": false,
                "ci_passed": false,
                "runner_authority": "absent",
                "authenticated_provenance": "pending",
                "phase_signoff": "in_review",
                "spawned": false
            }),
        ),
    }
}

fn execute_local_with_deadline(repo: &Path, deadline: std::time::Duration) -> Report {
    let head = match git_text(repo, &["rev-parse", "HEAD"]) {
        Ok(sha) => sha,
        Err(issue) => return local_fail(vec![issue], false),
    };
    let porcelain = match git_text(repo, &["status", "--porcelain"]) {
        Ok(text) => text,
        Err(issue) => return local_fail(vec![issue], false),
    };
    if !porcelain.is_empty() {
        return local_fail(
            vec![Issue::new(
                "gate.wrong_build",
                &head,
                "local execution refuses to bind a dirty worktree to HEAD. The source commit is not the tree that would run.",
            )],
            false,
        );
    }
    let manifest = match load_gates(&repo.join("ci/gates.yaml"), "manifest") {
        Ok(file) => file,
        Err(issue) => return local_fail(vec![issue], true),
    };
    let baseline = match load_gates(&repo.join("ci/baseline/gates-baseline.yaml"), "baseline") {
        Ok(file) => file,
        Err(issue) => return local_fail(vec![issue], true),
    };
    let structural = evaluate(
        &manifest,
        &baseline,
        EvalArgs {
            jobs: None,
            changed: &[],
            phase: None,
            source: "bootstrap",
            manifest_only: true,
            repo,
            baseline_sha256: BASELINE_PLACEHOLDER,
            trust: "local_execution",
            require_protected: false,
        },
    );
    if !structural.issues.is_empty() {
        return local_fail(structural.issues, true);
    }
    let (workflow_issues, commands) = scan_workflows(repo);
    if !workflow_issues.is_empty() {
        return local_fail(workflow_issues, true);
    }
    let mut planned = Vec::new();
    let mut refused = Vec::new();
    for gate in manifest
        .gates
        .iter()
        .filter(|gate| gate.status == "required" && gate.evaluated_by == "job")
    {
        let Some(line) = commands.get(&gate.id) else {
            refused.push(Issue::new(
                "gate.command",
                &gate.id,
                "workflow step is missing. It was not spawned.",
            ));
            continue;
        };
        let argv = split_command(line);
        let Some(allowed) = allowed_command(&gate.id) else {
            refused.push(Issue::new(
                "gate.command",
                &gate.id,
                "local execution only spawns the foundation allowlist. It was not spawned.",
            ));
            continue;
        };
        let expected: Vec<String> = allowed.iter().map(|arg| (*arg).to_owned()).collect();
        if argv != expected {
            refused.push(Issue::new(
                "gate.command",
                &gate.id,
                "workflow step is not the allowlisted command. It was not spawned.",
            ));
            continue;
        }
        let id = gate.job.clone().unwrap_or_else(|| gate.id.clone());
        planned.push((id, argv));
    }
    if !refused.is_empty() || planned.len() != 9 {
        if planned.len() != 9 && refused.is_empty() {
            refused.push(Issue::new(
                "gate.command",
                "allowlist",
                "local execution did not find the nine foundation commands. Nothing was spawned.",
            ));
        }
        return local_fail(refused, true);
    }
    let run_id = format!("local-{}", std::process::id());
    let root = std::env::temp_dir().join(format!(
        "oo-xtask-execute-{}-{}",
        std::process::id(),
        &head[..8]
    ));
    let _ = fs::remove_dir_all(&root);
    let mut executed = Vec::new();
    let mut issues = Vec::new();
    for (id, argv) in &planned {
        let run = crate::runner_capture::execute_command(repo, argv, deadline, id, &head);
        let dir = root.join(id);
        if let Err(error) = crate::runner_capture::seal_executed(&dir, repo, &run_id, id, &run) {
            issues.push(Issue::new(
                "gate.runner",
                id,
                format!("integrity record was not written: {error}"),
            ));
        }
        if crate::containment::blocks_pass(
            run.containment(),
            run.spawned(),
            run.cleanup_incomplete(),
            run.containment_note(),
        ) {
            issues.push(Issue::new(
                "gate.containment",
                id,
                format!(
                    "containment is {} (note: {}). A process listing, process group, or start token does not prove descendants are gone. This capture cannot seal a pass.",
                    run.containment(),
                    run.containment_note()
                ),
            ));
        } else if let Some(error) = run.spawn_error() {
            issues.push(Issue::new("gate.runner", id, error));
        } else if run.timed_out() {
            issues.push(Issue::new(
                "gate.deadline",
                id,
                "the command exceeded its deadline. Recorded owned processes were signaled. A same-group signal does not contain a descendant that left the group. A timed-out child cannot seal a pass.",
            ));
        } else if run.output_limited() {
            issues.push(Issue::new(
                "gate.output_limit",
                id,
                "command output exceeded the capture limit. The retained prefix was hashed and the command cannot seal a pass.",
            ));
        } else if run.cleanup_incomplete() {
            issues.push(Issue::new(
                "gate.cleanup",
                id,
                "owned descendants were still alive after the command. Cleanup is incomplete. This capture is not reused and cannot seal a pass.",
            ));
        } else if run.exit_code() != 0 {
            issues.push(Issue::new(
                "gate.job_failed",
                id,
                format!("exit {}", run.exit_code()),
            ));
        } else if run.argv() != argv.as_slice() || run.source_commit() != head {
            issues.push(Issue::new(
                "gate.command",
                id,
                "the spawned argv or source commit does not match the allowlisted command",
            ));
        }
        if run.spawn_error().is_none() && run.timed_out() && run.cleanup_incomplete() {
            issues.push(Issue::new(
                "gate.cleanup",
                id,
                "owned descendants were still alive after the deadline. Cleanup is incomplete. This capture is not reused and cannot seal a pass.",
            ));
        }
        if run.spawn_error().is_none() && run.output_limited() && run.timed_out() {
            issues.push(Issue::new(
                "gate.output_limit",
                id,
                "command output exceeded the capture limit. The retained prefix was hashed and the command cannot seal a pass.",
            ));
        }
        executed.push(run);
    }
    let head_after = git_text(repo, &["rev-parse", "HEAD"]).unwrap_or_default();
    let porcelain_after = git_text(repo, &["status", "--porcelain"]).unwrap_or_else(|issue| {
        issues.push(issue);
        "unreadable".to_owned()
    });
    let drifted = head_after != head || !porcelain_after.is_empty();
    if drifted {
        issues.push(Issue::new(
            "gate.wrong_build",
            &head,
            "source drifted after the run. The executed results were kept and do not seal a pass.",
        ));
    }
    let satisfied = issues.is_empty();
    let any_spawned = executed.iter().any(|run| run.spawned());
    let details = json!({
        "aggregate": if satisfied { "required_satisfied" } else { "failed" },
        "baseline_source": "local-execution",
        "protected_github_policy": false,
        "trusted_base_checker": "local_execution",
        "release_authority": false,
        "ci_passed": false,
        "runner_authority": if any_spawned { "local-engineering" } else { "absent" },
        "authenticated_provenance": "pending",
        "phase_signoff": "in_review",
        "spawned": any_spawned,
        "worktree_matches_source": !drifted,
        "integrity_root": root.display().to_string(),
        "executed": executed.iter().map(|run| json!({
            "job": run.job(),
            "exit_code": run.exit_code(),
            "timed_out": run.timed_out(),
            "cleanup_incomplete": run.cleanup_incomplete(),
            "output_limited": run.output_limited(),
            "containment": run.containment(),
            "spawned": run.spawned(),
            "containment_note": run.containment_note(),
            "direct_pid": run.direct_pid(),
            "stdout_sha256": sha256_hex(run.stdout()),
            "stderr_sha256": sha256_hex(run.stderr()),
            "spawn_error": run.spawn_error(),
        })).collect::<Vec<_>>(),
    });
    if satisfied {
        Report::ok("gates", "required_satisfied", details)
    } else {
        Report::fail("gates", "failed", EXIT_FAIL, issues, details)
    }
}

fn local_fail(issues: Vec<Issue>, worktree_matches_source: bool) -> Report {
    Report::fail(
        "gates",
        "failed",
        EXIT_FAIL,
        issues,
        json!({
            "protected_github_policy": false,
            "release_authority": false,
            "ci_passed": false,
            "runner_authority": "absent",
            "authenticated_provenance": "pending",
            "phase_signoff": "in_review",
            "spawned": false,
            "worktree_matches_source": worktree_matches_source
        }),
    )
}

struct SealFix<'a> {
    repo: &'a Path,
    source: &'a str,
    run_id: &'a str,
}

fn empty_sets() -> (BTreeSet<String>, BTreeSet<String>) {
    (BTreeSet::new(), BTreeSet::new())
}

fn probe_job(
    id: &str,
    runner: &str,
    capture: Option<String>,
    command: &[String],
    commit: &str,
) -> JobRecord {
    JobRecord {
        id: id.to_owned(),
        result: "success".to_owned(),
        command: command.to_vec(),
        commit: commit.to_owned(),
        exit_code: 0,
        log_sha256: "ab".repeat(32),
        runner: runner.to_owned(),
        log_path: None,
        step_script_sha256: "00".repeat(32),
        started_at: String::new(),
        finished_at: String::new(),
        run_id: String::new(),
        capture_path: capture,
    }
}

fn expect_issue(
    found: Option<Issue>,
    code: &str,
    fragment: &str,
    label: &str,
    issues: &mut Vec<Issue>,
) {
    match found {
        Some(issue) if issue.code == code && issue.message.contains(fragment) => {}
        Some(issue) => issues.push(Issue::new(
            "gate.self_test",
            label,
            format!(
                "wanted {code} containing {fragment}, got {} {}",
                issue.code, issue.message
            ),
        )),
        None => issues.push(Issue::new(
            "gate.self_test",
            label,
            format!("wanted {code}, got acceptance"),
        )),
    }
}

fn seal_stdout(
    dir: &Path,
    fix: &SealFix<'_>,
    job: &str,
    step: &str,
    step_script: &str,
    stdout: &[u8],
) -> Result<(), String> {
    crate::runner_capture::seal_capture(
        dir,
        &crate::runner_capture::CaptureParts {
            repo: fix.repo,
            source: fix.source,
            run_id: fix.run_id,
            job,
            step,
            step_script,
            exit_code: 0,
            started_at: "2026-10-01T00:00:00Z",
            finished_at: "2026-10-01T00:00:01Z",
            stdout,
            stderr: b"",
        },
    )
    .map(|_| ())
}

fn seal_foundation_jobs(
    repo: &Path,
    manifest: &GateFile,
    source: &str,
    run_id: &str,
    root: &Path,
) -> Result<JobsFile, String> {
    let (workflow_issues, commands) = scan_workflows(repo);
    if let Some(issue) = workflow_issues.first() {
        return Err(issue.message.clone());
    }
    let fix = SealFix {
        repo,
        source,
        run_id,
    };
    let mut jobs = Vec::new();
    for gate in manifest
        .gates
        .iter()
        .filter(|gate| gate.status == "required" && gate.evaluated_by == "job")
    {
        let id = gate.job.clone().unwrap_or_else(|| gate.id.clone());
        let line = commands
            .get(&gate.id)
            .ok_or_else(|| format!("workflow step for {} is missing", gate.id))?;
        let dir = root.join(&id);
        seal_stdout(&dir, &fix, &id, &id, line, b"bound-by-coordinator\n")?;
        jobs.push(probe_job(
            &id,
            "coordinator",
            Some(dir.display().to_string()),
            &split_command(line),
            source,
        ));
    }
    if jobs.len() != 9 {
        return Err(format!("expected 9 foundation jobs, sealed {}", jobs.len()));
    }
    Ok(JobsFile {
        commit: source.to_owned(),
        jobs,
    })
}

fn write_jobs_file(path: &Path, jobs: &JobsFile) -> Result<(), String> {
    let body = json!({
        "commit": jobs.commit,
        "jobs": jobs.jobs.iter().map(|job| json!({
            "id": job.id,
            "result": job.result,
            "command": job.command,
            "commit": job.commit,
            "exit_code": job.exit_code,
            "log_sha256": job.log_sha256,
            "runner": job.runner,
            "log_path": job.log_path,
            "step_script_sha256": job.step_script_sha256,
            "started_at": job.started_at,
            "finished_at": job.finished_at,
            "run_id": job.run_id,
            "capture_path": job.capture_path,
        })).collect::<Vec<_>>()
    });
    fs::write(path, body.to_string()).map_err(|error| error.to_string())
}

fn rewrite_producer(dir: &Path, producer: &str) -> Result<(), String> {
    let path = dir.join("capture.json");
    let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let mut value: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    let Some(object) = value.as_object_mut() else {
        return Err("capture is not an object".to_owned());
    };
    object.insert("producer".to_owned(), Value::String(producer.to_owned()));
    fs::write(path, value.to_string()).map_err(|error| error.to_string())
}
