//! Evidence v1 checker. Exit 0 means the document is honest. It does not sign the phase off.
//!
//! Historical a001–a022 manifests stay on disk. Without `--adapter historical-draft`
//! they fail canonical v1. The adapter reports pending and does not rewrite them.

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::{Value, json};

use crate::report::{EXIT_FAIL, EXIT_PENDING, EXIT_USAGE, Issue, Report};
use crate::util::{
    UNIMPLEMENTED_SUITES, appointed_logins, join_inside, parse_rfc3339_utc, private_component,
    read_no_symlink, repo_root, security_owner_appointed, sha256_hex,
};

pub const SCHEMA_TEXT: &str = include_str!("../../../contracts/evidence/evidence-v1.json");
const GOLD_MARKERS: &[&[u8]] = &[b"OO-GOLD-TRUTH", b"BEGIN HOLDOUT"];

pub fn run(args: &[String]) -> Report {
    let mut path: Option<PathBuf> = None;
    let mut repo = repo_root();
    let mut adapter: Option<String> = None;
    let mut check = false;
    let mut self_test = false;
    let mut emit: Option<PathBuf> = None;
    let mut write = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--check" => check = true,
            "--self-test" => self_test = true,
            "--write" => write = true,
            "--path" => {
                index += 1;
                match args.get(index) {
                    Some(value) => path = Some(PathBuf::from(value)),
                    None => return usage("evidence --path requires a file"),
                }
            }
            "--repo" => {
                index += 1;
                match args.get(index) {
                    Some(value) => repo = PathBuf::from(value),
                    None => return usage("evidence --repo requires a path"),
                }
            }
            "--dir" => {
                index += 1;
                match args.get(index) {
                    Some(value) => emit = Some(PathBuf::from(value)),
                    None => return usage("evidence --dir requires a directory"),
                }
            }
            "--adapter" => {
                index += 1;
                match args.get(index) {
                    Some(value) => adapter = Some(value.clone()),
                    None => return usage("evidence --adapter requires a name"),
                }
            }
            "--emit" => {}
            "--format" => {
                index += 1;
                match args.get(index).map(String::as_str) {
                    Some("json" | "human") => {}
                    _ => return usage("evidence --format requires human or json"),
                }
            }
            "--help" | "-h" => return help(),
            other => return usage(&format!("unknown evidence argument {other}")),
        }
        index += 1;
    }
    if self_test {
        return self_test_at(&repo_root());
    }
    if args.iter().any(|arg| arg == "--emit") {
        let Some(dir) = emit else {
            return usage("evidence --emit requires --dir");
        };
        return emit_dir(&repo, &dir, write);
    }
    if !check {
        return usage("evidence requires --check, --emit, or --self-test");
    }
    let Some(path) = path else {
        return usage("evidence --check requires --path");
    };
    if let Some(name) = adapter.as_deref()
        && name != "historical-draft"
    {
        return usage("the only evidence adapter is historical-draft");
    }
    let manifest = match resolve_inside(&repo, &path) {
        Ok(path) => path,
        Err(issue) => {
            return Report::fail("evidence", "failed", EXIT_FAIL, vec![issue], json!({}));
        }
    };
    check_file(&repo, &manifest, adapter.as_deref())
}

fn help() -> Report {
    Report::ok(
        "evidence",
        "help",
        json!({
            "usage": "xtask evidence --check --path MANIFEST [--adapter historical-draft] | xtask evidence --self-test | xtask evidence --emit --dir DIR --write",
            "exits": {"0": "honest document", "1": "malformed or false claim", "2": "usage", "3": "historical draft adapter"},
            "passed": "requires an appointed reviewer from uncommented CODEOWNERS. That set is empty, so a project document cannot be passed.",
            "historical": "a001-a022 drafts are not rewritten and do not satisfy evidence/v1"
        }),
    )
}

fn usage(message: &str) -> Report {
    Report::fail(
        "evidence",
        "usage",
        EXIT_USAGE,
        vec![Issue::new("cli.usage", "evidence", message)],
        json!({}),
    )
}

pub fn check_file(repo: &Path, manifest_path: &Path, adapter: Option<&str>) -> Report {
    let relative = manifest_path.display().to_string();
    let metadata = match fs::symlink_metadata(manifest_path) {
        Ok(metadata) => metadata,
        Err(error) => {
            return Report::fail(
                "evidence",
                "failed",
                EXIT_FAIL,
                vec![Issue::new("path.read", relative, error.to_string())],
                json!({}),
            );
        }
    };
    if metadata.file_type().is_symlink() {
        return Report::fail(
            "evidence",
            "failed",
            EXIT_FAIL,
            vec![Issue::new(
                "path.symlink",
                relative,
                "the manifest is a symlink",
            )],
            json!({}),
        );
    }
    let bytes = match fs::read(manifest_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return Report::fail(
                "evidence",
                "failed",
                EXIT_FAIL,
                vec![Issue::new("path.read", relative, error.to_string())],
                json!({}),
            );
        }
    };
    let value = match parse_document(&bytes, &relative) {
        Ok(value) => value,
        Err(issues) => {
            return Report::fail("evidence", "failed", EXIT_FAIL, issues, json!({}));
        }
    };
    if is_historical(&value) {
        return if adapter == Some("historical-draft") {
            adapt_historical(manifest_path, &value)
        } else {
            Report::fail(
                "evidence",
                "failed",
                EXIT_FAIL,
                vec![Issue::new(
                    "evidence.historical_draft",
                    relative,
                    "this draft has no evidence_schema_version. Pass --adapter historical-draft. It is not canonical v1 and it was not rewritten.",
                )],
                json!({"canonical_v1": false, "rewritten": false}),
            )
        };
    }
    let validator = match load_validator() {
        Ok(validator) => validator,
        Err(issue) => return Report::fail("evidence", "failed", EXIT_FAIL, vec![issue], json!({})),
    };
    let mut issues: Vec<Issue> = validator
        .iter_errors(&value)
        .take(30)
        .map(|error| {
            Issue::new(
                "evidence.schema",
                error.to_string(),
                "schema rejected this document",
            )
        })
        .collect();
    if issues.is_empty() {
        let evidence_root = manifest_path.parent().unwrap_or(repo);
        issues.extend(semantic(repo, evidence_root, &value));
    }
    let status = value.get("status").and_then(Value::as_str).unwrap_or("");
    let project_acceptance = value
        .get("reviewers")
        .and_then(Value::as_array)
        .is_some_and(|rows| {
            rows.iter()
                .any(|row| row.get("project_acceptance") == Some(&Value::Bool(true)))
        });
    Report::from_issues(
        "evidence",
        "valid",
        "failed",
        EXIT_FAIL,
        issues,
        json!({
            "document_status": status,
            "canonical_v1": true,
            "project_acceptance": project_acceptance && status == "passed",
            "appointments": appointed_logins(repo).len(),
            "security_owner_appointed": security_owner_appointed(repo),
            "rewritten": false
        }),
    )
}

fn parse_document(bytes: &[u8], label: &str) -> Result<Value, Vec<Issue>> {
    if bytes.len() > oo_core::limits::DOCUMENT_MAX_BYTES {
        return Err(vec![Issue::new(
            "evidence.schema",
            label,
            format!(
                "document is {} bytes; limit is {}",
                bytes.len(),
                oo_core::limits::DOCUMENT_MAX_BYTES
            ),
        )]);
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| vec![Issue::new("evidence.json", label, "document is not utf-8")])?;
    let value = oo_core::json_scan::parse_json_value(text, "evidence").map_err(|list| {
        list.items
            .iter()
            .map(|item| {
                let code = if item.code.contains("duplicate_key") {
                    "evidence.duplicate_key"
                } else {
                    "evidence.schema"
                };
                Issue::new(code, label, item.message.clone())
            })
            .collect::<Vec<_>>()
    })?;
    let mut issues = Vec::new();
    reject_wide_numbers(&value, "/", &mut issues);
    if issues.is_empty() {
        Ok(value)
    } else {
        Err(issues)
    }
}

fn reject_wide_numbers(value: &Value, path: &str, issues: &mut Vec<Issue>) {
    match value {
        Value::Number(number) if number.as_i64().is_none() && number.as_u64().is_none() => {
            issues.push(Issue::new(
                "evidence.schema",
                path,
                "number is outside an exact integer range",
            ));
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                reject_wide_numbers(item, &format!("{path}{index}/"), issues);
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                reject_wide_numbers(item, &format!("{path}{key}/"), issues);
            }
        }
        _ => {}
    }
}

fn load_validator() -> Result<jsonschema::Validator, Issue> {
    let schema: Value = serde_json::from_str(SCHEMA_TEXT)
        .map_err(|error| Issue::new("evidence.schema", "evidence-v1.json", error.to_string()))?;
    if !jsonschema::draft202012::meta::is_valid(&schema) {
        return Err(Issue::new(
            "evidence.schema",
            "evidence-v1.json",
            "evidence schema is not valid draft 2020-12",
        ));
    }
    oo_core::schema_util::compile_schema(&schema).map_err(|error| {
        Issue::new(
            "evidence.schema",
            "evidence-v1.json",
            format!("compile_schema failed: {error}"),
        )
    })
}

fn is_historical(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    if object.contains_key("evidence_schema_version") {
        return false;
    }
    if object.get("spec_id").and_then(Value::as_str) != Some("OO-SPEC-001") {
        return false;
    }
    object.contains_key("worktree_note")
        || object.contains_key("plan_id")
        || object
            .get("artifacts")
            .and_then(Value::as_array)
            .is_some_and(|rows| {
                rows.iter()
                    .any(|row| row.get("bytes").is_some() && row.get("size").is_none())
            })
}

fn adapt_historical(manifest_path: &Path, value: &Value) -> Report {
    let mut escaped = Vec::new();
    if let Some(rows) = value.get("artifacts").and_then(Value::as_array) {
        for row in rows {
            if let Some(path) = row.get("path").and_then(Value::as_str)
                && path_is_closed(path)
            {
                escaped.push(path.to_owned());
            }
        }
    }
    if let Some(path) = value.get("environment_manifest").and_then(Value::as_str)
        && path_is_closed(path)
    {
        escaped.push(path.to_owned());
    }
    Report::fail(
        "evidence",
        "historical_draft",
        EXIT_PENDING,
        Vec::new(),
        json!({
            "canonical_v1": false,
            "adapter": "historical-draft",
            "rewritten": false,
            "opened_escaped_paths": false,
            "escaped_paths_not_opened": escaped,
            "manifest": manifest_path.display().to_string(),
            "note": "Draft evidence is pending. It is not a v1 pass and this command does not edit it."
        }),
    )
}

fn path_is_closed(path: &str) -> bool {
    path.starts_with('/')
        || path.starts_with('~')
        || path.split(['/', '\\']).any(|part| part == "..")
        || private_component(path)
}

fn semantic(repo: &Path, evidence_root: &Path, value: &Value) -> Vec<Issue> {
    let mut issues = Vec::new();
    let status = text(value, "status");
    let commit = text(value, "commit");
    let author = value
        .pointer("/provenance/author")
        .and_then(Value::as_str)
        .unwrap_or("");
    let signed = value.pointer("/provenance/signed_aggregate") == Some(&Value::Bool(true));
    if signed {
        issues.push(Issue::new(
            "evidence.signature_deferred",
            "provenance.signed_aggregate",
            "no signature verifier is linked. A true flag is not an approval.",
        ));
    }
    let suite = value
        .pointer("/scope/suite")
        .and_then(Value::as_str)
        .unwrap_or("");
    let kind = value
        .pointer("/scope/kind")
        .and_then(Value::as_str)
        .unwrap_or("");
    let claim = value
        .pointer("/scope/claim")
        .and_then(Value::as_str)
        .unwrap_or("");
    if status == "passed" && UNIMPLEMENTED_SUITES.contains(&suite) {
        issues.push(Issue::new(
            "evidence.suite_overclaim",
            suite,
            "this suite is not implemented and cannot be passed",
        ));
    }
    let source_rows = value
        .get("source_hashes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if source_rows.is_empty() {
        issues.push(Issue::new(
            "evidence.source",
            "source_hashes",
            "evidence needs a source hash for every scope input",
        ));
    }
    let mut hashed = std::collections::BTreeMap::<String, String>::new();
    for row in &source_rows {
        let path = row.get("path").and_then(Value::as_str).unwrap_or("");
        let expect = row.get("sha256").and_then(Value::as_str).unwrap_or("");
        match read_no_symlink(repo, path) {
            Ok(bytes) => {
                let actual = sha256_hex(&bytes);
                if actual != expect {
                    issues.push(Issue::new(
                        "evidence.hash",
                        path,
                        format!("source sha256 is {actual}"),
                    ));
                }
                hashed.insert(path.to_owned(), expect.to_owned());
            }
            Err(issue) => issues.push(issue),
        }
    }
    if let Some(inputs) = value.pointer("/scope/inputs").and_then(Value::as_array) {
        for input in inputs {
            let Some(input) = input.as_str() else {
                issues.push(Issue::new(
                    "evidence.source",
                    "scope.inputs",
                    "scope input is not a string",
                ));
                continue;
            };
            match inventory_input(repo, input) {
                Ok(files) => {
                    if files.is_empty() {
                        issues.push(Issue::new(
                            "evidence.source",
                            input,
                            "scope input expanded to no files",
                        ));
                    }
                    for file in files {
                        if !hashed.contains_key(&file) {
                            issues.push(Issue::new(
                                "evidence.source",
                                file,
                                "scope input has no matching source hash",
                            ));
                        }
                    }
                }
                Err(issue) => issues.push(issue),
            }
        }
    }
    issues.extend(commit_binding(repo, commit, status, &hashed));
    if status == "passed" {
        let tuple = value.get("version_tuple");
        for key in ["schema", "harness", "suite"] {
            let field = tuple
                .and_then(|item| item.get(key))
                .and_then(Value::as_str)
                .unwrap_or("");
            if field.is_empty() {
                issues.push(Issue::new(
                    "evidence.version_tuple",
                    key,
                    "passed evidence needs this version field",
                ));
            }
        }
        if let Some(map) = tuple.and_then(Value::as_object) {
            for (key, item) in map {
                if item.as_str().is_some_and(|text| text.is_empty()) {
                    issues.push(Issue::new(
                        "evidence.version_tuple",
                        key,
                        "version field is empty",
                    ));
                }
            }
        }
    }
    let mut finished_max = None;
    if let Some(rows) = value.get("commands").and_then(Value::as_array) {
        for row in rows {
            let started = row.get("started_at").and_then(Value::as_str).unwrap_or("");
            let finished = row.get("finished_at").and_then(Value::as_str).unwrap_or("");
            let cwd = row.get("cwd").and_then(Value::as_str).unwrap_or("");
            if cwd.split(['/', '\\']).any(|part| part == "..")
                || cwd.starts_with('/')
                || cwd.starts_with('~')
            {
                issues.push(Issue::new(
                    "path.escape",
                    cwd,
                    "command cwd escapes the repo",
                ));
            }
            match (parse_rfc3339_utc(started), parse_rfc3339_utc(finished)) {
                (Ok(start), Ok(end)) => {
                    if end < start {
                        issues.push(Issue::new(
                            "evidence.time_order",
                            started,
                            "finished_at is earlier than started_at",
                        ));
                    }
                    finished_max = Some(
                        finished_max
                            .map(|item: crate::util::UtcStamp| item.max(end))
                            .unwrap_or(end),
                    );
                }
                (Err(error), _) | (_, Err(error)) => {
                    issues.push(Issue::new("evidence.time", started, error));
                }
            }
            let exit_code = row.get("exit_code").and_then(Value::as_i64).unwrap_or(-1);
            if status == "passed" && exit_code != 0 {
                issues.push(Issue::new(
                    "evidence.command_exit",
                    "commands",
                    format!("passed evidence has command exit {exit_code}"),
                ));
            }
        }
    }
    let privacy = text(value, "privacy");
    let env_privacy = value
        .pointer("/environment/privacy")
        .and_then(Value::as_str)
        .unwrap_or("");
    if privacy != env_privacy {
        issues.push(Issue::new(
            "evidence.privacy",
            "environment.privacy",
            "top-level privacy and environment privacy differ",
        ));
    }
    let env_manifest = text(value, "environment_manifest");
    let embedded_env = value.get("environment").cloned().unwrap_or(Value::Null);
    let mut saw_env = false;
    if let Some(rows) = value.get("artifacts").and_then(Value::as_array) {
        for row in rows {
            let path = row.get("path").and_then(Value::as_str).unwrap_or("");
            let visibility = row.get("visibility").and_then(Value::as_str).unwrap_or("");
            let role = row.get("role").and_then(Value::as_str).unwrap_or("");
            let expect = row.get("sha256").and_then(Value::as_str).unwrap_or("");
            let size = row.get("size").and_then(Value::as_u64);
            if path == env_manifest {
                saw_env = true;
            }
            if visibility != "public-synthetic" || private_component(path) || role == "dataset-gold"
            {
                issues.push(Issue::new(
                    "evidence.private_refused",
                    path,
                    "private, customer, or gold paths are not opened by the public checker",
                ));
                continue;
            }
            match read_no_symlink(evidence_root, path) {
                Ok(bytes) => {
                    if sha256_hex(&bytes) != expect || size != Some(bytes.len() as u64) {
                        issues.push(Issue::new(
                            "evidence.hash",
                            path,
                            "artifact sha256 or size does not match the file",
                        ));
                    }
                    if GOLD_MARKERS
                        .iter()
                        .any(|marker| bytes.windows(marker.len()).any(|window| window == *marker))
                    {
                        issues.push(Issue::new(
                            "evidence.gold_leak",
                            path,
                            "public artifact contains a dataset-gold marker",
                        ));
                    }
                    if matches!(role, "report" | "log") {
                        issues.extend(artifact_binding(row, commit, suite, expect));
                        if bytes.first() == Some(&b'{')
                            && let Ok(text) = std::str::from_utf8(&bytes)
                            && let Ok(parsed) =
                                oo_core::json_scan::parse_json_value(text, "evidence")
                            && let Some(inner) = parsed.get("commit").and_then(Value::as_str)
                            && inner != commit
                        {
                            issues.push(Issue::new(
                                "evidence.replay_wrong_build",
                                path,
                                "embedded commit does not match the manifest commit",
                            ));
                        }
                    }
                    if path == env_manifest {
                        match serde_json::from_slice::<Value>(&bytes) {
                            Ok(parsed) if parsed == embedded_env => {}
                            Ok(_) => issues.push(Issue::new(
                                "evidence.environment",
                                path,
                                "environment file does not match the embedded object",
                            )),
                            Err(error) => issues.push(Issue::new(
                                "evidence.environment",
                                path,
                                error.to_string(),
                            )),
                        }
                    }
                }
                Err(issue) => issues.push(issue),
            }
        }
    }
    if !saw_env {
        issues.push(Issue::new(
            "evidence.environment",
            env_manifest,
            "environment_manifest is not an artifact",
        ));
    }
    if let Some(rows) = value.get("acceptance").and_then(Value::as_array) {
        for row in rows {
            let id = row.get("id").and_then(Value::as_str).unwrap_or("");
            let row_status = row.get("status").and_then(Value::as_str).unwrap_or("");
            let exit_code = row.get("exit_code");
            let full_case = row.get("full_case") == Some(&Value::Bool(true));
            if kind == "fixture" && full_case {
                issues.push(Issue::new(
                    "evidence.fixture_full_case",
                    id,
                    "a fixture scope cannot claim full_case",
                ));
            }
            if status == "passed"
                && (row_status != "passed" || exit_code.and_then(Value::as_i64) != Some(0))
            {
                issues.push(Issue::new(
                    "evidence.acceptance",
                    id,
                    "passed evidence needs every acceptance passed and exit 0",
                ));
            }
            if status == "passed" && kind != "fixture" && !full_case {
                issues.push(Issue::new(
                    "evidence.acceptance",
                    id,
                    "passed implementation evidence needs full_case",
                ));
            }
            if status == "in_review" && row_status == "passed" {
                let slice = kind == "slice" || claim.starts_with("slice:");
                if full_case || !slice {
                    issues.push(Issue::new(
                        "evidence.slice_overclaim",
                        id,
                        "in_review may record a passed slice only when full_case is false and the scope is a slice",
                    ));
                }
            }
        }
    }
    issues.extend(reviewers(repo, value, status, kind, author));
    if let Some(rows) = value.get("waivers").and_then(Value::as_array) {
        for row in rows {
            let kind_name = row.get("kind").and_then(Value::as_str).unwrap_or("");
            let expires = row.get("expires_at").and_then(Value::as_str).unwrap_or("");
            if kind_name == "security-exception" && !security_owner_appointed(repo) {
                issues.push(Issue::new(
                    "evidence.waiver_unappointed",
                    row.get("id").and_then(Value::as_str).unwrap_or("waiver"),
                    "a security exception is invalid while the security owner is unappointed",
                ));
            }
            if let (Ok(expires_at), Some(finished)) = (parse_rfc3339_utc(expires), finished_max)
                && expires_at < finished
            {
                issues.push(Issue::new(
                    "evidence.waiver_expired",
                    expires,
                    "waiver expired before the run finished",
                ));
            }
        }
    }
    issues
}

fn artifact_binding(row: &Value, commit: &str, suite: &str, content_sha: &str) -> Vec<Issue> {
    let path = row
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("artifact");
    let Some(binding) = row.get("binding") else {
        return vec![Issue::new(
            "evidence.replay_wrong_build",
            path,
            "report or log artifact has no binding metadata",
        )];
    };
    let bound_commit = binding.get("commit").and_then(Value::as_str).unwrap_or("");
    let bound_suite = binding.get("suite").and_then(Value::as_str).unwrap_or("");
    let schema = binding
        .get("schema_version")
        .and_then(Value::as_str)
        .unwrap_or("");
    let hash = binding
        .get("content_sha256")
        .and_then(Value::as_str)
        .unwrap_or("");
    if bound_commit != commit
        || bound_suite != suite
        || schema != "evidence/v1"
        || hash != content_sha
    {
        return vec![Issue::new(
            "evidence.replay_wrong_build",
            path,
            "artifact binding does not match the manifest commit, suite, schema, or content hash",
        )];
    }
    Vec::new()
}

fn commit_binding(
    repo: &Path,
    commit: &str,
    status: &str,
    hashed: &std::collections::BTreeMap<String, String>,
) -> Vec<Issue> {
    if commit == "uncommitted" {
        if status == "passed" {
            return vec![Issue::new(
                "evidence.commit",
                "commit",
                "passed evidence must bind a 40-hex commit that exists",
            )];
        }
        return Vec::new();
    }
    if commit.len() != 40 || !commit.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return vec![Issue::new(
            "evidence.commit",
            "commit",
            "commit is not 40 hex or uncommitted",
        )];
    }
    let listed = crate::util::isolated_git(repo)
        .args(["cat-file", "-t", commit])
        .output();
    let Ok(listed) = listed else {
        return vec![Issue::new(
            "evidence.commit",
            commit,
            "git could not resolve the commit",
        )];
    };
    if !listed.status.success() || !listed.stdout.starts_with(b"commit") {
        return vec![Issue::new(
            "evidence.commit",
            commit,
            "commit is not a git object in this repo",
        )];
    }
    let mut issues = Vec::new();
    for (path, expect) in hashed {
        let shown = crate::util::isolated_git(repo)
            .args(["show", &format!("{commit}:{path}")])
            .output();
        match shown {
            Ok(output) if output.status.success() && sha256_hex(&output.stdout) == *expect => {}
            _ => issues.push(Issue::new(
                "evidence.commit",
                path,
                "source bytes are not the blob in this commit",
            )),
        }
    }
    issues
}

fn inventory_input(repo: &Path, input: &str) -> Result<Vec<String>, Issue> {
    if path_is_closed(input) {
        return Err(Issue::new(
            "path.escape",
            input,
            "scope input escapes the repo",
        ));
    }
    let full = join_inside(repo, input)?;
    let metadata = fs::symlink_metadata(&full)
        .map_err(|_| Issue::new("evidence.source", input, "scope input is missing"))?;
    if metadata.file_type().is_symlink() {
        return Err(Issue::new(
            "path.symlink",
            input,
            "scope input is a symlink",
        ));
    }
    if metadata.is_file() {
        return Ok(vec![input.to_owned()]);
    }
    if !metadata.is_dir() {
        return Err(Issue::new(
            "evidence.source",
            input,
            "scope input is not a file or directory",
        ));
    }
    let mut files = Vec::new();
    collect_input(repo, &full, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_input(repo: &Path, dir: &Path, files: &mut Vec<String>) -> Result<(), Issue> {
    let entries = fs::read_dir(dir).map_err(|_| {
        Issue::new(
            "evidence.source",
            dir.display().to_string(),
            "cannot list scope input",
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|_| {
            Issue::new(
                "evidence.source",
                dir.display().to_string(),
                "cannot read scope input entry",
            )
        })?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|_| {
            Issue::new(
                "evidence.source",
                path.display().to_string(),
                "cannot stat scope input",
            )
        })?;
        let shown = relative_repo(repo, &path);
        if metadata.file_type().is_symlink() {
            return Err(Issue::new(
                "path.symlink",
                shown,
                "scope input contains a symlink",
            ));
        }
        if metadata.is_dir() {
            collect_input(repo, &path, files)?;
        } else if metadata.is_file() {
            files.push(shown);
        } else {
            return Err(Issue::new(
                "evidence.source",
                shown,
                "scope input contains an unsupported file type",
            ));
        }
    }
    Ok(())
}

fn relative_repo(repo: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(repo).unwrap_or(path);
    let mut out = String::new();
    for component in relative.components() {
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(&component.as_os_str().to_string_lossy());
    }
    out
}

fn reviewers(repo: &Path, value: &Value, status: &str, kind: &str, author: &str) -> Vec<Issue> {
    let mut issues = Vec::new();
    let appointed = appointed_logins(repo);
    let rows = value.get("reviewers").and_then(Value::as_array);
    let empty = rows.map(Vec::is_empty).unwrap_or(true);
    if status == "passed" && empty {
        issues.push(Issue::new(
            "evidence.reviewer",
            "reviewers",
            "passed evidence needs an independent reviewer. A name is not an appointment.",
        ));
    }
    let Some(rows) = rows else {
        return issues;
    };
    for row in rows {
        let identity = row.get("identity").and_then(Value::as_str).unwrap_or("");
        let role = row.get("role").and_then(Value::as_str).unwrap_or("");
        let context = row.get("context").and_then(Value::as_str).unwrap_or("");
        let independent = row.get("independent") == Some(&Value::Bool(true));
        let project = row.get("project_acceptance") == Some(&Value::Bool(true));
        if identity == author && (independent || context != "Author") {
            issues.push(Issue::new(
                "evidence.author_reviewer",
                identity,
                "the author is not an independent reviewer",
            ));
        }
        if context == "Author" && project {
            issues.push(Issue::new(
                "evidence.author_reviewer",
                identity,
                "an author record cannot accept the project",
            ));
        }
        if role == "security-owner" && !security_owner_appointed(repo) {
            issues.push(Issue::new(
                "evidence.reviewer",
                identity,
                "security-owner is not appointed. The role string is not an appointment.",
            ));
        }
        if context == "Fixture"
            && (kind != "fixture" || !identity.starts_with("fixture-") || project)
        {
            issues.push(Issue::new(
                "evidence.fixture_reviewer",
                identity,
                "a fixture reviewer is only valid on fixture scope, with a fixture- identity and project_acceptance false",
            ));
        }
        if context == "Appointed" {
            let login = identity.strip_prefix('@').unwrap_or(identity);
            if !appointed
                .iter()
                .any(|item| item == login || item == identity)
            {
                issues.push(Issue::new(
                    "evidence.reviewer",
                    identity,
                    "identity is not an appointed login; a name is not a role",
                ));
            }
        }
        if status == "passed"
            && kind == "fixture"
            && (context != "Fixture" || project || !independent)
        {
            issues.push(Issue::new(
                "evidence.fixture_reviewer",
                identity,
                "fixture passed requires an independent fixture reviewer and project_acceptance false",
            ));
        }
        if status == "passed"
            && kind != "fixture"
            && (context != "Appointed" || !project || !independent)
        {
            issues.push(Issue::new(
                "evidence.reviewer",
                identity,
                "project passed requires an independent appointed reviewer",
            ));
        }
    }
    issues
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

pub fn emit_dir(repo: &Path, dir: &Path, write: bool) -> Report {
    let dir = match resolve_inside(repo, dir) {
        Ok(dir) => dir,
        Err(issue) => return Report::fail("evidence", "failed", EXIT_FAIL, vec![issue], json!({})),
    };
    let manifest_path = dir.join("manifest.json");
    let bytes = match fs::read(&manifest_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return Report::fail(
                "evidence",
                "failed",
                EXIT_FAIL,
                vec![Issue::new("path.read", "manifest.json", error.to_string())],
                json!({}),
            );
        }
    };
    let mut value = match parse_document(&bytes, "manifest.json") {
        Ok(value) => value,
        Err(issues) => {
            return Report::fail("evidence", "failed", EXIT_FAIL, issues, json!({}));
        }
    };
    if value.get("status").and_then(Value::as_str) == Some("passed") {
        return Report::fail(
            "evidence",
            "failed",
            EXIT_FAIL,
            vec![Issue::new(
                "evidence.passed_refused",
                "status",
                "emit will not write a passed manifest",
            )],
            json!({}),
        );
    }
    let mut issues = Vec::new();
    if let Some(rows) = value.get_mut("artifacts").and_then(Value::as_array_mut) {
        for row in rows {
            let path = row
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let visibility = row.get("visibility").and_then(Value::as_str).unwrap_or("");
            if visibility != "public-synthetic" || private_component(&path) {
                issues.push(Issue::new(
                    "evidence.private_refused",
                    path,
                    "emit does not open private or customer artifacts",
                ));
                continue;
            }
            match read_no_symlink(&dir, &path) {
                Ok(file_bytes) => {
                    if let Some(object) = row.as_object_mut() {
                        object.insert("sha256".to_owned(), Value::String(sha256_hex(&file_bytes)));
                        object.insert("size".to_owned(), json!(file_bytes.len()));
                    }
                }
                Err(issue) => issues.push(issue),
            }
        }
    }
    if let Some(rows) = value.get_mut("source_hashes").and_then(Value::as_array_mut) {
        for row in rows {
            let path = row
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            match read_no_symlink(repo, &path) {
                Ok(file_bytes) => {
                    if let Some(object) = row.as_object_mut() {
                        object.insert("sha256".to_owned(), Value::String(sha256_hex(&file_bytes)));
                    }
                }
                Err(issue) => issues.push(issue),
            }
        }
    }
    if !issues.is_empty() {
        return Report::fail(
            "evidence",
            "failed",
            EXIT_FAIL,
            issues,
            json!({"written": false}),
        );
    }
    if write {
        let mut text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_owned());
        text.push('\n');
        if let Err(error) = fs::write(&manifest_path, text) {
            return Report::fail(
                "evidence",
                "failed",
                EXIT_FAIL,
                vec![Issue::new("path.write", "manifest.json", error.to_string())],
                json!({}),
            );
        }
    }
    Report::ok(
        "evidence",
        "bound",
        json!({"written": write, "path": manifest_path.display().to_string()}),
    )
}

fn resolve_inside(repo: &Path, requested: &Path) -> Result<PathBuf, Issue> {
    let repo = lexical_norm(repo);
    let full = if requested.is_absolute() {
        lexical_norm(requested)
    } else {
        let relative = requested.to_string_lossy();
        join_inside(&repo, &relative)?
    };
    let full = lexical_norm(&full);
    if !full.starts_with(&repo) {
        return Err(Issue::new(
            "path.escape",
            requested.display().to_string(),
            "path is outside the repo root",
        ));
    }
    Ok(full)
}

fn lexical_norm(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::RootDir | Component::Normal(_) | Component::Prefix(_) => {
                out.push(component.as_os_str());
            }
        }
    }
    out
}

pub fn self_test_at(_real_repo: &Path) -> Report {
    let mut issues = Vec::new();
    let root = std::env::temp_dir().join(format!("oo-xtask-evidence-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    if fs::create_dir_all(&root).is_err() {
        return Report::fail(
            "evidence",
            "failed",
            EXIT_FAIL,
            vec![Issue::new(
                "evidence.self_test",
                "temp",
                "cannot create temp root",
            )],
            json!({}),
        );
    }
    if parse_document(br#"{"n":1e9999}"#, "huge").is_ok() {
        issues.push(Issue::new(
            "evidence.self_test",
            "huge-number",
            "1e9999 was accepted",
        ));
    }
    match parse_document(br#"{"exit_code":1.5}"#, "fraction") {
        Err(found) if found.iter().any(|issue| issue.code == "evidence.schema") => {}
        other => issues.push(Issue::new(
            "evidence.self_test",
            "fraction",
            format!("fractional exit was not rejected: {other:?}"),
        )),
    }
    match parse_document(br#"{"status":"failed","status":"passed"}"#, "duplicate") {
        Err(found)
            if found
                .iter()
                .any(|issue| issue.code == "evidence.duplicate_key") => {}
        other => issues.push(Issue::new(
            "evidence.self_test",
            "duplicate",
            format!("duplicate key was not rejected: {other:?}"),
        )),
    }
    let mut deep = String::new();
    for _ in 0..4000 {
        deep.push('[');
    }
    deep.push('0');
    for _ in 0..4000 {
        deep.push(']');
    }
    if parse_document(deep.as_bytes(), "deep").is_ok() {
        issues.push(Issue::new(
            "evidence.self_test",
            "deep",
            "4000-deep JSON was accepted",
        ));
    }
    let padded = vec![b' '; oo_core::limits::DOCUMENT_MAX_BYTES + 1];
    if parse_document(&padded, "padded").is_ok() {
        issues.push(Issue::new(
            "evidence.self_test",
            "padded",
            "oversized JSON was accepted",
        ));
    }
    let utf8 = json!({"type": "string", "x-utf8-max-bytes": 4});
    match oo_core::schema_util::compile_schema(&utf8) {
        Ok(validator) => {
            if validator.is_valid(&json!("ééé")) || !validator.is_valid(&json!("abcd")) {
                issues.push(Issue::new(
                    "evidence.self_test",
                    "x-utf8-max-bytes",
                    "UTF-8 byte keyword is not active",
                ));
            }
        }
        Err(error) => issues.push(Issue::new(
            "evidence.self_test",
            "x-utf8-max-bytes",
            error.to_string(),
        )),
    }
    expect(
        &check_case(&root, "control", |_, _| {}),
        0,
        None,
        "control",
        &mut issues,
    );
    expect(
        &check_case(&root, "tamper", |manifest, _| {
            manifest["artifacts"][0]["sha256"] = Value::String("0".repeat(64));
        }),
        1,
        Some("evidence.hash"),
        "tamper",
        &mut issues,
    );
    expect(
        &check_case(&root, "passed-unreviewed", |manifest, _| {
            manifest["status"] = json!("passed");
            manifest["commit"] = json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
            manifest["acceptance"][0]["status"] = json!("passed");
            manifest["acceptance"][0]["full_case"] = json!(true);
        }),
        1,
        Some("evidence.reviewer"),
        "passed-unreviewed",
        &mut issues,
    );
    expect(
        &check_case(&root, "time", |manifest, _| {
            manifest["commands"][0]["finished_at"] = json!("2026-10-01T00:00:00Z");
            manifest["commands"][0]["started_at"] = json!("2026-10-01T00:00:02Z");
        }),
        1,
        Some("evidence.time_order"),
        "time",
        &mut issues,
    );
    expect(
        &check_case(&root, "escape", |manifest, _| {
            manifest["artifacts"][0]["path"] = json!("nested/../../outside.json");
        }),
        1,
        Some("path.escape"),
        "escape",
        &mut issues,
    );
    expect(
        &check_case(&root, "private", |manifest, files| {
            files.insert("secret.bin".to_owned(), b"SENTINEL-PRIVATE".to_vec());
            manifest["artifacts"][0]["path"] = json!("secret.bin");
            manifest["artifacts"][0]["visibility"] = json!("private");
            manifest["artifacts"][0]["role"] = json!("note");
            manifest["environment_manifest"] = json!("secret.bin");
        }),
        1,
        Some("evidence.private_refused"),
        "private",
        &mut issues,
    );
    expect(
        &check_case(&root, "gold", |manifest, files| {
            let body = b"public header\nOO-GOLD-TRUTH\n".to_vec();
            manifest["artifacts"][0]["sha256"] = json!(sha256_hex(&body));
            manifest["artifacts"][0]["size"] = json!(body.len());
            files.insert("environment.json".to_owned(), body);
        }),
        1,
        Some("evidence.gold_leak"),
        "gold",
        &mut issues,
    );
    expect(
        &check_case(&root, "replay", |manifest, files| {
            let body = b"{\"commit\":\"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"}\n".to_vec();
            files.insert("report.json".to_owned(), body.clone());
            let artifact = json!({
                "path": "report.json",
                "sha256": sha256_hex(&body),
                "size": body.len(),
                "visibility": "public-synthetic",
                "role": "report"
            });
            manifest["artifacts"].as_array_mut().unwrap().push(artifact);
        }),
        1,
        Some("evidence.replay_wrong_build"),
        "replay",
        &mut issues,
    );
    expect(
        &check_case(&root, "signature", |manifest, _| {
            manifest["provenance"]["signed_aggregate"] = json!(true);
        }),
        1,
        Some("evidence.signature_deferred"),
        "signature",
        &mut issues,
    );
    expect(
        &check_case(&root, "waiver", |manifest, _| {
            manifest["waivers"] = json!([{
                "id": "sec-7",
                "requirement": "REQ-SEC-01",
                "reason": "seven day exception",
                "expires_at": "2099-01-01T00:00:00Z",
                "owner": "unappointed",
                "kind": "security-exception"
            }]);
        }),
        1,
        Some("evidence.waiver_unappointed"),
        "waiver",
        &mut issues,
    );
    expect(
        &check_case(&root, "overclaim", |manifest, _| {
            manifest["acceptance"][0]["status"] = json!("passed");
            manifest["acceptance"][0]["full_case"] = json!(true);
        }),
        1,
        Some("evidence.slice_overclaim"),
        "overclaim",
        &mut issues,
    );
    expect(
        &check_case(&root, "name-role", |manifest, _| {
            manifest["status"] = json!("passed");
            manifest["commit"] = json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
            manifest["acceptance"][0] =
                json!({"id": "AT-CORE-04", "status": "passed", "exit_code": 0, "full_case": true});
            manifest["reviewers"] = json!([{
                "identity": "security-owner",
                "role": "reviewer",
                "context": "Appointed",
                "independent": true,
                "project_acceptance": true
            }]);
        }),
        1,
        Some("evidence.reviewer"),
        "name-role",
        &mut issues,
    );
    expect(
        &check_case(&root, "fixture-pass", |manifest, _| {
            manifest["status"] = json!("passed");
            manifest["commit"] = json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
            manifest["scope"]["kind"] = json!("fixture");
            manifest["scope"]["suite"] = json!("sp00-contract-fixture-smoke");
            manifest["acceptance"][0] =
                json!({"id": "AT-CORE-04", "status": "passed", "exit_code": 0, "full_case": true});
            manifest["version_tuple"]["schema"] = json!("evidence/v1");
            manifest["version_tuple"]["harness"] = json!("xtask");
            manifest["version_tuple"]["suite"] = json!("sp00-contract-fixture-smoke");
            manifest["reviewers"] = json!([{
                "identity": "fixture-local",
                "role": "reviewer",
                "context": "Fixture",
                "independent": true,
                "project_acceptance": false
            }]);
        }),
        1,
        Some("evidence.fixture_full_case"),
        "fixture-pass",
        &mut issues,
    );
    expect(
        &check_case(&root, "directory", |manifest, files| {
            files.insert("repo:inputs/a.txt".to_owned(), b"a\n".to_vec());
            files.insert("repo:inputs/.hidden".to_owned(), b"hidden\n".to_vec());
            manifest["scope"]["inputs"] = json!(["inputs", "notes.txt"]);
            manifest["source_hashes"] = json!([
                {"path": "inputs/a.txt", "sha256": sha256_hex(b"a\n")},
                {"path": "notes.txt", "sha256": sha256_hex(b"public synthetic source\n")}
            ]);
        }),
        1,
        Some("evidence.source"),
        "directory",
        &mut issues,
    );
    expect(
        &check_case(&root, "bound-log", |manifest, files| {
            let body = b"plaintext log\n";
            files.insert("run.log".to_owned(), body.to_vec());
            manifest["artifacts"].as_array_mut().unwrap().push(json!({
                "path": "run.log",
                "sha256": sha256_hex(body),
                "size": body.len(),
                "visibility": "public-synthetic",
                "role": "log",
                "binding": {
                    "commit": "uncommitted",
                    "suite": "sp00-help-smoke",
                    "schema_version": "evidence/v1",
                    "content_sha256": sha256_hex(body)
                }
            }));
        }),
        0,
        None,
        "bound-log",
        &mut issues,
    );
    let _ = check_case(&root, "appointed", |manifest, _| {
        manifest["status"] = json!("passed");
        manifest["commit"] = json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        manifest["acceptance"][0] =
            json!({"id": "AT-CORE-04", "status": "passed", "exit_code": 0, "full_case": true});
        manifest["version_tuple"]["schema"] = json!("evidence/v1");
        manifest["version_tuple"]["harness"] = json!("xtask");
        manifest["version_tuple"]["suite"] = json!("sp00-help-smoke");
        manifest["reviewers"] = json!([{
            "identity": "gate-reviewer",
            "role": "reviewer",
            "context": "Appointed",
            "independent": true,
            "project_acceptance": true
        }]);
    });
    let _ = fs::create_dir_all(root.join("appointed/.github"));
    let _ = fs::write(
        root.join("appointed/.github/CODEOWNERS"),
        "* @gate-reviewer\n",
    );
    match commit_file(&root.join("appointed"), "notes.txt") {
        Ok(sha) => {
            if let Ok(text) = fs::read_to_string(root.join("appointed/case/manifest.json")) {
                let updated = text.replace("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", &sha);
                let _ = fs::write(root.join("appointed/case/manifest.json"), updated);
            }
        }
        Err(error) => issues.push(Issue::new(
            "evidence.self_test",
            "appointed-logic",
            format!("fixture commit failed: {error}"),
        )),
    }
    let appointed = check_file(
        &root.join("appointed"),
        &root.join("appointed/case/manifest.json"),
        None,
    );
    expect(&appointed, 0, None, "appointed-logic", &mut issues);
    let _ = check_case(&root, "author", |manifest, _| {
        manifest["status"] = json!("passed");
        manifest["commit"] = json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        manifest["acceptance"][0] =
            json!({"id": "AT-CORE-04", "status": "passed", "exit_code": 0, "full_case": true});
        manifest["version_tuple"]["schema"] = json!("evidence/v1");
        manifest["version_tuple"]["harness"] = json!("xtask");
        manifest["version_tuple"]["suite"] = json!("sp00-help-smoke");
        manifest["reviewers"] = json!([{
            "identity": "fixture-author",
            "role": "reviewer",
            "context": "Appointed",
            "independent": true,
            "project_acceptance": true
        }]);
    });
    let _ = fs::create_dir_all(root.join("author/.github"));
    let _ = fs::write(
        root.join("author/.github/CODEOWNERS"),
        "* @fixture-author\n",
    );
    let author = check_file(
        &root.join("author"),
        &root.join("author/case/manifest.json"),
        None,
    );
    expect(
        &author,
        1,
        Some("evidence.author_reviewer"),
        "author",
        &mut issues,
    );
    let historical_dir = root.join("historical");
    let _ = fs::create_dir_all(&historical_dir);
    let draft = b"{\"spec_id\":\"OO-SPEC-001\",\"plan_id\":\"OO-PLAN-001\",\"worktree_note\":\"draft\",\"artifacts\":[{\"path\":\"../outside.json\",\"bytes\":4}]}\n";
    let draft_path = historical_dir.join("manifest.json");
    let _ = fs::write(&draft_path, draft);
    let outside = historical_dir.join("outside.json");
    let _ = fs::write(&outside, b"OPENED-HISTORICAL");
    let before = sha256_hex(draft);
    expect(
        &check_file(&root, &draft_path, None),
        1,
        Some("evidence.historical_draft"),
        "historical-bare",
        &mut issues,
    );
    let adapted = check_file(&root, &draft_path, Some("historical-draft"));
    expect(&adapted, 3, None, "historical-adapter", &mut issues);
    if adapted.status != "historical_draft"
        || adapted.details.get("canonical_v1") != Some(&Value::Bool(false))
    {
        issues.push(Issue::new(
            "evidence.self_test",
            "historical-adapter",
            "adapter did not stay non-canonical",
        ));
    }
    if sha256_hex(&fs::read(&draft_path).unwrap_or_default()) != before {
        issues.push(Issue::new(
            "evidence.self_test",
            "historical-adapter",
            "adapter rewrote the draft",
        ));
    }
    if fs::read(&outside).ok().as_deref() != Some(b"OPENED-HISTORICAL".as_slice()) {
        issues.push(Issue::new(
            "evidence.self_test",
            "historical-adapter",
            "adapter changed a path outside the draft",
        ));
    }
    expect(
        &check_case(&root, "control-again", |_, _| {}),
        0,
        None,
        "control-again",
        &mut issues,
    );
    let symlink_repo = root.join("symlink");
    let _ = check_case(&root, "symlink", |_, _| {});
    let case = symlink_repo.join("case");
    let _ = fs::rename(
        case.join("environment.json"),
        case.join("environment-real.json"),
    );
    std::os::unix::fs::symlink("environment-real.json", case.join("environment.json")).ok();
    let linked = check_file(&symlink_repo, &case.join("manifest.json"), None);
    expect(&linked, 1, Some("path.symlink"), "symlink", &mut issues);
    let _ = fs::remove_dir_all(&root);
    Report::from_issues(
        "evidence",
        "self_test_pass",
        "failed",
        EXIT_FAIL,
        issues,
        json!({"adapter": "historical-draft"}),
    )
}

fn commit_file(repo: &Path, relative: &str) -> Result<String, String> {
    let run = |args: &[&str]| {
        let output = crate::util::isolated_git(repo)
            .args(args)
            .output()
            .map_err(|error| error.to_string())?;
        if output.status.success() {
            Ok(output)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            if stderr.is_empty() {
                Err(format!("git exited {}", output.status.code().unwrap_or(-1)))
            } else {
                Err(stderr)
            }
        }
    };
    run(&["init"])?;
    run(&["add", "--", relative])?;
    run(&[
        "-c",
        "user.email=xtask@localhost",
        "-c",
        "user.name=xtask",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-m",
        "fixture",
    ])?;
    let sha = run(&["rev-parse", "HEAD"])?;
    let text = String::from_utf8(sha.stdout).map_err(|error| error.to_string())?;
    let text = text.trim();
    if text.len() == 40 {
        Ok(text.to_owned())
    } else {
        Err(format!("rev-parse returned {text}"))
    }
}

fn expect(report: &Report, exit: i32, code: Option<&str>, label: &str, issues: &mut Vec<Issue>) {
    let rendered = report.render("json");
    if rendered.contains("OO-GOLD-TRUTH")
        || rendered.contains("SENTINEL-PRIVATE")
        || rendered.contains("OPENED-HISTORICAL")
    {
        issues.push(Issue::new(
            "evidence.self_test",
            label,
            "a closed artifact body was copied into the report",
        ));
    }
    if report.exit_code != exit {
        issues.push(Issue::new(
            "evidence.self_test",
            label,
            format!("exit {} wanted {exit}: {rendered}", report.exit_code),
        ));
        return;
    }
    if let Some(code) = code
        && !report.issues.iter().any(|issue| issue.code == code)
    {
        issues.push(Issue::new(
            "evidence.self_test",
            label,
            format!("missing {code}: {rendered}"),
        ));
    }
}

fn check_case(
    root: &Path,
    name: &str,
    mutate: impl FnOnce(&mut Value, &mut std::collections::BTreeMap<String, Vec<u8>>),
) -> Report {
    let repo = root.join(name);
    let case = repo.join("case");
    let _ = fs::create_dir_all(&case);
    let environment = json!({
        "privacy": "public-synthetic",
        "secret_material": false,
        "os": "fixture",
        "rustc": "1.98.1"
    });
    let mut env_bytes = serde_json::to_vec_pretty(&environment).unwrap_or_default();
    env_bytes.push(b'\n');
    let source = b"public synthetic source\n".to_vec();
    let mut files =
        std::collections::BTreeMap::from([("environment.json".to_owned(), env_bytes.clone())]);
    let mut manifest = json!({
        "evidence_schema_version": "evidence/v1",
        "spec_id": "OO-SPEC-001",
        "spec_version": "1.0.0",
        "plan_id": "OO-PLAN-001",
        "plan_version": "1.0.0",
        "run_id": "fixture-evidence-self-test",
        "phase": "SP00",
        "task_id": "SP00-W05",
        "commit": "uncommitted",
        "source_hashes": [{"path": "notes.txt", "sha256": sha256_hex(&source)}],
        "scope": {
            "kind": "implementation",
            "suite": "sp00-help-smoke",
            "claim": "honest in-review fixture",
            "inputs": ["notes.txt"]
        },
        "environment_manifest": "environment.json",
        "environment": environment,
        "requirements": ["REQ-OPS-07"],
        "acceptance": [{"id": "AT-CORE-04", "status": "in_review", "exit_code": 0, "full_case": false}],
        "version_tuple": {
            "dataset": null, "snapshot": null, "suite": "sp00-help-smoke", "scorer": null,
            "policy": null, "knowledge": null, "config": null, "type_policy": null,
            "hook": null, "schema": "evidence/v1", "model": null, "harness": "xtask",
            "artifact_digest": null
        },
        "commands": [{
            "argv": ["xtask", "evidence", "--self-test"],
            "started_at": "2026-10-01T00:00:00Z",
            "finished_at": "2026-10-01T00:00:01Z",
            "exit_code": 0,
            "cwd": "."
        }],
        "artifacts": [{
            "path": "environment.json",
            "sha256": sha256_hex(&env_bytes),
            "size": env_bytes.len(),
            "visibility": "public-synthetic",
            "role": "environment"
        }],
        "privacy": "public-synthetic",
        "reviewers": [],
        "waivers": [],
        "status": "in_review",
        "provenance": {
            "kind": "fixture",
            "author": "fixture-author",
            "signed_aggregate": false,
            "signature_artifact": null
        }
    });
    mutate(&mut manifest, &mut files);
    let _ = fs::write(repo.join("notes.txt"), source);
    for (name, bytes) in &files {
        if let Some(relative) = name.strip_prefix("repo:") {
            let path = repo.join(relative);
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(path, bytes);
        } else {
            let _ = fs::write(case.join(name), bytes);
        }
    }
    let mut text = serde_json::to_string_pretty(&manifest).unwrap_or_else(|_| "{}".to_owned());
    text.push('\n');
    let manifest_path = case.join("manifest.json");
    let _ = fs::write(&manifest_path, text);
    check_file(&repo, &manifest_path, None)
}
