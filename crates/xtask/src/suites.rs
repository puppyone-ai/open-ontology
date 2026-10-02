//! Suite, datagen, infra, docs, and release commands.
//!
//! Unimplemented product suites return exit 3. Fixture smoke uses its own suite ids.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use serde_json::{Value, json};

use crate::contracts;
use crate::report::{EXIT_FAIL, EXIT_PENDING, EXIT_USAGE, Issue, Report};
use crate::util::{UNIMPLEMENTED_SUITES, appointed_logins, read_no_symlink, repo_root, sha256_hex};

const IMPLEMENTED: &[&str] = &["sp00-help-smoke", "sp00-contract-fixture-smoke"];

const INFRA_ENTRIES: &[&str] = &[
    "testing/infra/builders/snapshot-inputs.sh",
    "testing/infra/builders/run-matrix.sh",
    "testing/infra/builders/cargo-locked.sh",
    "testing/infra/compose/up.sh",
    "testing/infra/compose/smoke.sh",
    "testing/infra/compose/fault.sh",
    "testing/infra/compose/cleanup.sh",
    "testing/infra/compose/debezium/pending.sh",
];

pub fn test_cmd(args: &[String]) -> Report {
    let mut suite = None;
    let mut parameters = Vec::new();
    let mut scale = None;
    let mut seed = None;
    let mut database_fallback = false;
    let mut runtime = None;
    let mut fault_every_step = false;
    let mut profile = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--suite" => {
                index += 1;
                match args.get(index) {
                    Some(value) => suite = Some(value.clone()),
                    None => return usage("test", "test --suite requires a name"),
                }
            }
            "--parameters" => {
                index += 1;
                match args.get(index) {
                    Some(value) => parameters.push(value.clone()),
                    None => return usage("test", "test --parameters requires key=value"),
                }
            }
            "--scale" => {
                index += 1;
                match args.get(index).map(String::as_str) {
                    Some(value @ ("s" | "m" | "l")) => scale = Some(value.to_owned()),
                    Some(value) => {
                        return usage("test", &format!("--scale must be s, m, or l, got {value}"));
                    }
                    None => return usage("test", "--scale requires s, m, or l"),
                }
            }
            "--seed" => {
                index += 1;
                match args.get(index).map(String::as_str) {
                    Some(value) => match parse_seed(value) {
                        Some(parsed) => seed = Some(parsed),
                        None => return usage("test", "--seed requires an integer"),
                    },
                    None => return usage("test", "--seed requires an integer"),
                }
            }
            "--database-fallback" => database_fallback = true,
            "--runtime" => {
                index += 1;
                match args.get(index).map(String::as_str) {
                    Some(value) if valid_runtime(value) => runtime = Some(value.to_owned()),
                    Some(_) => {
                        return usage(
                            "test",
                            "--runtime must be a 1-32 character token without a URL or slash",
                        );
                    }
                    None => return usage("test", "--runtime requires a token"),
                }
            }
            "--fault-every-step" => fault_every_step = true,
            "--profile" => {
                index += 1;
                match args.get(index).map(String::as_str) {
                    Some("standard") => profile = Some("standard".to_owned()),
                    Some(_) => return usage("test", "--profile must be standard"),
                    None => return usage("test", "--profile requires standard"),
                }
            }
            "--format" => {
                index += 1;
                if !matches!(args.get(index).map(String::as_str), Some("json" | "human")) {
                    return usage("test", "test --format requires human or json");
                }
            }
            "--help" | "-h" => {
                return Report::ok(
                    "test",
                    "help",
                    json!({
                        "implemented": IMPLEMENTED,
                        "unimplemented_exit": 3,
                        "spec_flags": ["--scale", "--seed", "--database-fallback", "--runtime", "--fault-every-step", "--profile"],
                        "production_faults": false,
                        "note": "sp00 fixture smoke is not integration-a, security-b, or runtime-c"
                    }),
                );
            }
            other => return usage("test", &format!("unknown test argument {other}")),
        }
        index += 1;
    }
    let Some(suite) = suite else {
        return usage("test", "test requires --suite");
    };
    let typed = json!({
        "scale": scale,
        "seed": seed,
        "database_fallback": database_fallback,
        "runtime": runtime,
        "fault_every_step": fault_every_step,
        "profile": profile,
        "parameters": parameters,
        "production_faults": false,
        "http_server": false
    });
    if UNIMPLEMENTED_SUITES.contains(&suite.as_str()) {
        return Report::fail(
            "test",
            "suite_not_implemented",
            EXIT_PENDING,
            vec![Issue::new(
                "suite.not_implemented",
                &suite,
                "no environment probe is claimed. DB, runsc, and credentials were not contacted.",
            )],
            json!({"suite": suite, "parameters": typed, "blocked": true}),
        );
    }
    if scale.is_some()
        || seed.is_some()
        || database_fallback
        || runtime.is_some()
        || fault_every_step
        || profile.is_some()
    {
        return usage(
            "test",
            "spec suite flags apply to named unimplemented suites, not the SP00 smoke suites",
        );
    }
    if !IMPLEMENTED.contains(&suite.as_str()) {
        return usage("test", &format!("unknown suite {suite}"));
    }
    if suite == "sp00-help-smoke" {
        let help = crate::dispatch(&[String::from("help")]);
        let version = crate::dispatch(&[String::from("version")]);
        let unknown = crate::dispatch(&[String::from("--disable-gates")]);
        let mut issues = Vec::new();
        if help.exit_code != 0 || version.exit_code != 0 || unknown.exit_code != 2 {
            issues.push(Issue::new(
                "suite.help_smoke",
                &suite,
                format!(
                    "help {} version {} disable {}",
                    help.exit_code, version.exit_code, unknown.exit_code
                ),
            ));
        }
        return Report::from_issues(
            "test",
            "pass",
            "failed",
            EXIT_FAIL,
            issues,
            json!({
                "suite": suite,
                "scope": "sp00-help-smoke",
                "covers": ["help", "version", "reject --disable-gates"],
                "does_not_cover": UNIMPLEMENTED_SUITES
            }),
        );
    }
    let report = contracts::check_root(&repo_root(), false);
    let narrow = report.exit_code == EXIT_PENDING
        && report.status == "library_only"
        && report.issues.len() == 1
        && report
            .issues
            .iter()
            .all(|issue| issue.code == "contract.library_only");
    let mut issues = Vec::new();
    if !narrow {
        issues.push(Issue::new(
            "suite.contract_fixture",
            &suite,
            "library contract comparison failed. Shared schemas were not overwritten.",
        ));
        issues.extend(report.issues);
    }
    Report::from_issues(
        "test",
        "pass",
        "failed",
        EXIT_FAIL,
        issues,
        json!({
            "suite": suite,
            "scope": "sp00-contract-fixture-smoke",
            "library_only": true,
            "full_gate": false,
            "at_core_03_http": false,
            "contract_status": report.status
        }),
    )
}

pub fn bench_cmd(args: &[String]) -> Report {
    let suite = flag(args, "--suite").unwrap_or_else(|| "public-e".to_owned());
    Report::fail(
        "bench",
        "suite_not_implemented",
        EXIT_PENDING,
        vec![Issue::new(
            "suite.not_implemented",
            suite,
            "benchmark E commands are not implemented. No fixture was written.",
        )],
        json!({"generator_invoked": false}),
    )
}

pub fn datagen_cmd(args: &[String]) -> Report {
    let mut dataset = None;
    let mut manifest_path = None;
    let mut receipt = None;
    let mut operation = "verify".to_owned();
    let mut scale = None;
    let mut seed = None;
    let mut output = None;
    let mut scope = None;
    let mut list_name = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--check" => {}
            "--dataset" => {
                index += 1;
                dataset = args.get(index).cloned();
            }
            "--manifest-path" => {
                index += 1;
                match args.get(index) {
                    Some(value) => manifest_path = Some(value.clone()),
                    None => return usage("datagen", "datagen --manifest-path requires a path"),
                }
            }
            "--receipt" => {
                index += 1;
                match args.get(index) {
                    Some(value) => receipt = Some(value.clone()),
                    None => return usage("datagen", "datagen --receipt requires a path"),
                }
            }
            "--operation" => {
                index += 1;
                match args.get(index).map(String::as_str) {
                    Some(value @ ("verify" | "generate" | "list" | "export-sources")) => {
                        operation = value.to_owned();
                    }
                    Some(value) => {
                        return usage("datagen", &format!("unknown datagen operation {value}"));
                    }
                    None => return usage("datagen", "datagen --operation requires a name"),
                }
            }
            "--scale" => {
                index += 1;
                match args.get(index).map(String::as_str) {
                    Some(value @ ("xs" | "s")) => scale = Some(value.to_owned()),
                    Some(value) => {
                        return usage(
                            "datagen",
                            &format!("datagen --scale accepts xs or s, not {value}"),
                        );
                    }
                    None => return usage("datagen", "datagen --scale requires xs or s"),
                }
            }
            "--seed" => {
                index += 1;
                match args.get(index).and_then(|value| value.parse::<u64>().ok()) {
                    Some(value) => seed = Some(value),
                    None => return usage("datagen", "datagen --seed requires an unsigned integer"),
                }
            }
            "--output" => {
                index += 1;
                match args.get(index) {
                    Some(value) => output = Some(value.clone()),
                    None => return usage("datagen", "datagen --output requires a path"),
                }
            }
            "--scope" => {
                index += 1;
                match args.get(index) {
                    Some(value) => scope = Some(value.clone()),
                    None => return usage("datagen", "datagen --scope requires a path"),
                }
            }
            "--list-name" => {
                index += 1;
                match args.get(index).map(String::as_str) {
                    Some(value @ ("schema" | "fixtures")) => list_name = Some(value.to_owned()),
                    Some(value) => {
                        return usage(
                            "datagen",
                            &format!("datagen --list-name accepts schema or fixtures, not {value}"),
                        );
                    }
                    None => {
                        return usage("datagen", "datagen --list-name requires schema or fixtures");
                    }
                }
            }
            "--format" => {
                index += 1;
                if !matches!(args.get(index).map(String::as_str), Some("json" | "human")) {
                    return usage("datagen", "datagen --format requires human or json");
                }
            }
            "--help" | "-h" => {
                return Report::ok(
                    "datagen",
                    "help",
                    json!({
                        "usage": "xtask datagen --check [--dataset DIR] [--manifest-path PATH] [--receipt PATH] [--operation verify|generate|list|export-sources]",
                        "ready": false,
                        "manifest_path_supported": true,
                        "note": "Cargo owns --manifest-path. REVIEW_FROZEN is not a receipt. Exit 3 until an appointed receipt exists."
                    }),
                );
            }
            other => return usage("datagen", &format!("unknown datagen argument {other}")),
        }
        index += 1;
    }
    let repo = repo_root();
    let author_running = ledger_running(&repo, "eef831aa-dba4-4af2-a78b-541bc9e0a021");
    let mut issues = Vec::new();
    if let Some(dataset) = &dataset {
        issues.extend(visibility_only(&repo, Path::new(dataset)));
    }
    let base = json!({
        "generator_ready": false,
        "author_running": author_running,
        "manifest_path_supported": true,
        "manifest_path_requested": manifest_path.is_some(),
        "manifest_path_invoked": false,
        "fixtures_written": false,
        "w05_datagen_integration": false,
        "review_frozen_is_receipt": false,
        "generator_invoked": false,
        "observed_commands": ["generate", "verify", "export-sources", "list"],
        "visibility_classes": ["source-input", "hidden-truth", "attack", "authorization"],
        "hidden_truth_bytes_opened": false,
        "depends_on": "SP00-W04",
        "dataset": dataset,
        "next_step": DATAGEN_NEXT
    });
    if !issues.is_empty() {
        return Report::fail("datagen", "failed", EXIT_FAIL, issues, base);
    }
    let Some(receipt_path) = receipt else {
        return Report::fail(
            "datagen",
            "pending",
            EXIT_PENDING,
            vec![Issue::new(
                "datagen.pending",
                "bench/datagen",
                "No trusted review receipt was supplied. The generator was not run and no fixture was written.",
            )],
            base,
        );
    };
    let loaded = match load_receipt(&repo, Path::new(&receipt_path), "datagen") {
        Ok(loaded) => loaded,
        Err(issue) => return Report::fail("datagen", "failed", EXIT_FAIL, vec![issue], base),
    };
    if loaded.context == "appointed" {
        let mut details = base;
        if let Some(object) = details.as_object_mut() {
            object.insert("release_acceptance".to_owned(), json!(false));
            object.insert("synthetic_authorized".to_owned(), json!(false));
            object.insert("generator_invoked".to_owned(), json!(false));
            if let Some(line) = ledger_running_line(&repo, DATAGEN_WRITER) {
                object.insert("ledger_line".to_owned(), json!(line));
            }
        }
        return Report::fail(
            "datagen",
            "pending",
            EXIT_PENDING,
            vec![Issue::new(
                "datagen.appointed_pending",
                "bench/datagen",
                "An appointed CODEOWNERS receipt does not authorize this local synthetic generator. No bench process was spawned.",
            )],
            details,
        );
    }
    if loaded.context == "development" {
        return development_datagen(&repo, &loaded, base);
    }
    let cargo = match std::env::var("OO_XTASK_CARGO") {
        Ok(value) if !value.is_empty() => value,
        _ => {
            return Report::fail(
                "datagen",
                "pending",
                EXIT_PENDING,
                vec![Issue::new(
                    "datagen.fixture_executor",
                    "OO_XTASK_CARGO",
                    "A fixture receipt needs OO_XTASK_CARGO set to an outside-repo hash-bound executor. Live cargo was not started.",
                )],
                base,
            );
        }
    };
    let Some(manifest) = manifest_path else {
        return usage(
            "datagen",
            "a fixture receipt requires --manifest-path outside the repository",
        );
    };
    let cargo_path = match bind_outside(&repo, Path::new(&cargo), &loaded) {
        Ok(path) => path,
        Err(issue) => return Report::fail("datagen", "failed", EXIT_FAIL, vec![issue], base),
    };
    let manifest_bound = match bind_outside(&repo, Path::new(&manifest), &loaded) {
        Ok(path) => path,
        Err(issue) => return Report::fail("datagen", "failed", EXIT_FAIL, vec![issue], base),
    };
    let (child_args, owned) = match fixture_datagen_plan(
        &repo,
        &operation,
        FixturePlan {
            dataset: dataset.as_deref(),
            scale: scale.as_deref(),
            seed,
            output: output.as_deref(),
            scope: scope.as_deref(),
            list_name: list_name.as_deref(),
        },
    ) {
        Ok(plan) => plan,
        Err(issue) => {
            let code = if issue.code == "cli.usage" {
                EXIT_USAGE
            } else {
                EXIT_FAIL
            };
            return Report::fail(
                "datagen",
                if code == EXIT_USAGE {
                    "usage"
                } else {
                    "failed"
                },
                code,
                vec![issue],
                base,
            );
        }
    };
    let deadline = match child_deadline() {
        Ok(seconds) => seconds,
        Err(issue) => return Report::fail("datagen", "usage", EXIT_USAGE, vec![issue], base),
    };
    let mut command = Command::new(&cargo_path);
    command
        .arg("run")
        .arg("--locked")
        .arg("--offline")
        .arg("--manifest-path")
        .arg(&manifest_bound)
        .arg("--bin")
        .arg("oo-datagen")
        .arg("--")
        .args(&child_args)
        .current_dir(manifest_bound.parent().unwrap_or(Path::new("/")));
    let child = run_bounded(command, Duration::from_secs(deadline));
    if let Some(stage) = &owned {
        cleanup_owned_stage(stage);
    }
    let mut argv = vec![
        cargo_path.display().to_string(),
        "run".to_owned(),
        "--locked".to_owned(),
        "--offline".to_owned(),
        "--manifest-path".to_owned(),
        manifest_bound.display().to_string(),
        "--bin".to_owned(),
        "oo-datagen".to_owned(),
        "--".to_owned(),
    ];
    argv.extend(child_args);
    let mut details = base;
    if let Some(object) = details.as_object_mut() {
        object.insert("manifest_path_invoked".to_owned(), json!(true));
        object.insert("generator_invoked".to_owned(), json!(true));
        object.insert("fixture_branch_executed".to_owned(), json!(true));
        object.insert("fixture_scope".to_owned(), json!(true));
        object.insert("w05_datagen_integration".to_owned(), json!(false));
        object.insert("child_exit".to_owned(), json!(child.exit_code));
        object.insert("child_timed_out".to_owned(), json!(child.timed_out));
        object.insert("argv".to_owned(), json!(argv));
        if let Some(error) = &child.error {
            object.insert("child_error".to_owned(), json!(error));
        }
    }
    if child.timed_out || child.exit_code != 0 {
        return Report::fail(
            "datagen",
            "failed",
            EXIT_FAIL,
            vec![Issue::new(
                if child.timed_out {
                    "datagen.deadline"
                } else {
                    "datagen.child"
                },
                "OO_XTASK_CARGO",
                child_failure_message(&child),
            )],
            details,
        );
    }
    Report::fail(
        "datagen",
        "fixture_branch_executed",
        EXIT_PENDING,
        vec![Issue::new(
            "datagen.fixture",
            "OO_XTASK_CARGO",
            "The isolated fixture process ran. This is not a W05 datagen integration pass and no fixture dataset was written.",
        )],
        details,
    )
}

fn visibility_only(repo: &Path, dataset: &Path) -> Vec<Issue> {
    let full = if dataset.is_absolute() {
        dataset.to_path_buf()
    } else {
        repo.join(dataset)
    };
    let manifest = full.join("dataset-manifest.json");
    if !manifest.is_file() {
        return Vec::new();
    }
    let Ok(bytes) = fs::read(&manifest) else {
        return vec![Issue::new(
            "datagen.manifest",
            manifest.display().to_string(),
            "cannot read manifest",
        )];
    };
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return vec![Issue::new(
            "datagen.manifest",
            "dataset-manifest.json",
            "manifest is not JSON",
        )];
    };
    let mut issues = Vec::new();
    let Some(files) = value.get("files").and_then(Value::as_array) else {
        return issues;
    };
    for item in files {
        let path = item.get("path").and_then(Value::as_str).unwrap_or("");
        let visibility = item.get("visibility").and_then(Value::as_str).unwrap_or("");
        let expected = if path.starts_with("source-inputs/") {
            "source-input"
        } else if path.starts_with("hidden-truth/") {
            "hidden-truth"
        } else if path.starts_with("attacks/") {
            "attack"
        } else if path.starts_with("authorization/") {
            "authorization"
        } else {
            ""
        };
        if expected.is_empty() || visibility != expected {
            issues.push(Issue::new(
                "datagen.visibility",
                path,
                format!("declared {visibility}, path class {expected}. Hidden-truth bytes were not opened."),
            ));
        }
    }
    issues
}

pub fn infra_cmd(args: &[String]) -> Report {
    let mut check = false;
    let mut receipt = None;
    let mut script = None;
    let mut run_id = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--check" => check = true,
            "--receipt" => {
                index += 1;
                match args.get(index) {
                    Some(value) => receipt = Some(value.clone()),
                    None => return usage("infra", "infra --receipt requires a path"),
                }
            }
            "--script" => {
                index += 1;
                match args.get(index) {
                    Some(value) => script = Some(value.clone()),
                    None => return usage("infra", "infra --script requires a path"),
                }
            }
            "--run-id" => {
                index += 1;
                match args.get(index).map(String::as_str) {
                    Some(value) if is_uuid(value) => run_id = Some(value.to_owned()),
                    Some(_) => return usage("infra", "--run-id requires a UUID"),
                    None => return usage("infra", "--run-id requires a UUID"),
                }
            }
            "--format" => {
                index += 1;
                if !matches!(args.get(index).map(String::as_str), Some("json" | "human")) {
                    return usage("infra", "infra --format requires human or json");
                }
            }
            "--help" | "-h" => {
                return Report::ok(
                    "infra",
                    "help",
                    json!({
                        "usage": "xtask infra --check [--receipt PATH --script PATH --run-id UUID]",
                        "executes": false,
                        "note": "REVIEW_FROZEN is not a receipt. Live scripts stay unexecuted until a coordinator freeze-approved ENV review."
                    }),
                );
            }
            other => return usage("infra", &format!("unknown infra argument {other}")),
        }
        index += 1;
    }
    if !check {
        return usage("infra", "infra requires --check");
    }
    let repo = repo_root();
    let mut issues = Vec::new();
    let mut inventory = Vec::new();
    for relative in INFRA_ENTRIES {
        match read_no_symlink(&repo, relative) {
            Ok(bytes) => inventory.push(
                json!({"path": relative, "sha256": sha256_hex(&bytes), "bytes": bytes.len()}),
            ),
            Err(issue) => issues.push(issue),
        }
    }
    let review_running = ledger_running(&repo, "eef831aa-dba4-4af2-a78b-541bc9e0a022");
    let frozen = repo.join("testing/infra/REVIEW_FROZEN").is_file();
    let base = json!({
        "executed": false,
        "fixture_executed": false,
        "docker": false,
        "w05_infra_integration": false,
        "review_running": review_running,
        "review_frozen": frozen,
        "review_frozen_is_receipt": false,
        "run_id_consumed": false,
        "inventory": inventory,
        "next_step": INFRA_NEXT
    });
    if !issues.is_empty() {
        return Report::fail("infra", "failed", EXIT_FAIL, issues, base);
    }
    let Some(receipt_path) = receipt else {
        return Report::fail(
            "infra",
            "source_checked_execution_pending",
            EXIT_PENDING,
            vec![Issue::new(
                "infra.pending",
                "testing/infra",
                "source inventory is recorded. Scripts were not executed. REVIEW_FROZEN is not a receipt.",
            )],
            base,
        );
    };
    let loaded = match load_receipt(&repo, Path::new(&receipt_path), "infra") {
        Ok(loaded) => loaded,
        Err(issue) => return Report::fail("infra", "failed", EXIT_FAIL, vec![issue], base),
    };
    if loaded.context == "appointed" {
        let mut details = base;
        if let Some(object) = details.as_object_mut() {
            object.insert("release_acceptance".to_owned(), json!(false));
            object.insert("synthetic_authorized".to_owned(), json!(false));
            object.insert("docker".to_owned(), json!(false));
            object.insert("executed".to_owned(), json!(false));
        }
        return Report::fail(
            "infra",
            "source_checked_execution_pending",
            EXIT_PENDING,
            vec![Issue::new(
                "infra.appointed_pending",
                "testing/infra",
                "An appointed CODEOWNERS receipt does not authorize local synthetic infra or Docker. No script was spawned.",
            )],
            details,
        );
    }
    if loaded.context == "development" {
        return development_infra(&repo, &loaded, base);
    }
    let (Some(script), Some(run_id)) = (script, run_id) else {
        return Report::fail(
            "infra",
            "source_checked_execution_pending",
            EXIT_PENDING,
            vec![Issue::new(
                "infra.fixture_args",
                "testing/infra",
                "A fixture receipt needs --script and --run-id. Live scripts were not started.",
            )],
            base,
        );
    };
    let script_path = match bind_outside(&repo, Path::new(&script), &loaded) {
        Ok(path) => path,
        Err(issue) => return Report::fail("infra", "failed", EXIT_FAIL, vec![issue], base),
    };
    let deadline = match child_deadline() {
        Ok(seconds) => seconds,
        Err(issue) => return Report::fail("infra", "usage", EXIT_USAGE, vec![issue], base),
    };
    let stage = std::env::temp_dir().join(format!("oo-xtask-infra-{}", std::process::id()));
    if let Err(issue) = create_owned_stage(&stage) {
        return Report::fail("infra", "failed", EXIT_FAIL, vec![issue], base);
    }
    let mut command = Command::new("bash");
    command
        .arg(&script_path)
        .arg("--run-id")
        .arg(&run_id)
        .current_dir(&stage);
    let child = run_bounded(command, Duration::from_secs(deadline));
    cleanup_owned_stage(&stage);
    let argv = vec![
        "bash".to_owned(),
        script_path.display().to_string(),
        "--run-id".to_owned(),
        run_id.clone(),
    ];
    let mut details = base;
    if let Some(object) = details.as_object_mut() {
        object.insert("fixture_executed".to_owned(), json!(true));
        object.insert("executed".to_owned(), json!(false));
        object.insert("docker".to_owned(), json!(false));
        object.insert("w05_infra_integration".to_owned(), json!(false));
        object.insert("run_id_consumed".to_owned(), json!(true));
        object.insert("child_exit".to_owned(), json!(child.exit_code));
        object.insert("child_timed_out".to_owned(), json!(child.timed_out));
        object.insert("argv".to_owned(), json!(argv));
        if let Some(error) = &child.error {
            object.insert("child_error".to_owned(), json!(error));
        }
    }
    if child.timed_out || child.exit_code != 0 {
        return Report::fail(
            "infra",
            "failed",
            EXIT_FAIL,
            vec![Issue::new(
                if child.timed_out {
                    "infra.deadline"
                } else {
                    "infra.child"
                },
                script_path.display().to_string(),
                child_failure_message(&child),
            )],
            details,
        );
    }
    Report::fail(
        "infra",
        "fixture_branch_executed",
        EXIT_PENDING,
        vec![Issue::new(
            "infra.fixture",
            script_path.display().to_string(),
            "The isolated fixture script ran under bash with only --run-id. Live infra, Docker, and W05 integration did not run.",
        )],
        details,
    )
}

fn development_infra(repo: &Path, receipt: &Receipt, mut base: Value) -> Report {
    let Some(approval) = receipt.approval.as_ref() else {
        return Report::fail(
            "infra",
            "failed",
            EXIT_FAIL,
            vec![Issue::new(
                "receipt.approval",
                "approval",
                "approval object is missing",
            )],
            base,
        );
    };
    if let Some(object) = base.as_object_mut() {
        object.insert("docker".to_owned(), json!(false));
        object.insert("executed".to_owned(), json!(false));
        object.insert("w05_infra_integration".to_owned(), json!(false));
    }
    match bind_development(repo, "infra", approval, INFRA_EVIDENCE, INFRA_SESSION) {
        Ok(binding) => {
            if let Some(object) = base.as_object_mut() {
                object.insert("source_sha256".to_owned(), json!(binding.source_sha256));
                object.insert(
                    "current_source_sha256".to_owned(),
                    json!(binding.current_source_sha256),
                );
                object.insert(
                    "identity_matches".to_owned(),
                    json!(binding.identity_matches),
                );
                if let Some(line) = &binding.ledger_line {
                    object.insert("ledger_line".to_owned(), json!(line));
                }
            }
            let message = if let Some(line) = binding.ledger_line {
                format!("infra identity was checked and Docker was not started. Ledger: {line}")
            } else if binding.identity_matches {
                "infra identity matches the a038 inventory. Docker was not started. Full integration is a later task.".to_owned()
            } else {
                "current infra bytes do not match the a038 inventory. Docker was not started."
                    .to_owned()
            };
            Report::fail(
                "infra",
                "pending",
                EXIT_PENDING,
                vec![Issue::new(
                    "infra.integration_deferred",
                    "testing/infra",
                    message,
                )],
                base,
            )
        }
        Err(issue) => Report::fail("infra", "failed", EXIT_FAIL, vec![issue], base),
    }
}

pub fn docs_cmd(args: &[String]) -> Report {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Report::ok("docs", "help", json!({"usage": "xtask docs --check"}));
    }
    if !args.iter().any(|arg| arg == "--check") {
        return usage("docs", "docs requires --check");
    }
    for arg in args {
        if arg != "--check" && arg != "--format" && arg != "json" && arg != "human" {
            return usage("docs", &format!("unknown docs argument {arg}"));
        }
    }
    let repo = repo_root();
    let mut issues = Vec::new();
    let mut checks = Vec::new();
    for (script, extra) in [
        ("specs/scripts/check_spec.py", None),
        ("specs/scripts/check_spec.py", Some("--self-test")),
        ("specs/scripts/check_plan.py", None),
        ("specs/scripts/check_plan.py", Some("--self-test")),
        ("governance/check_governance.py", None),
    ] {
        let mut command = Command::new("python3");
        command.arg(script).current_dir(&repo);
        if let Some(extra) = extra {
            command.arg(extra);
        }
        match command.output() {
            Ok(output) => {
                let exit = output.status.code().unwrap_or(1);
                checks.push(json!({"script": script, "arg": extra, "exit": exit}));
                if exit != 0 {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let tail = format!("{stdout}{stderr}");
                    let tail = tail
                        .chars()
                        .rev()
                        .take(1500)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect::<String>();
                    issues.push(Issue::new(
                        "docs.checker",
                        script,
                        format!("exit {exit}: {tail}"),
                    ));
                }
            }
            Err(error) => issues.push(Issue::new("docs.checker", script, error.to_string())),
        }
    }
    let (scan_issues, outside) = byte_scan(&repo);
    issues.extend(scan_issues);
    Report::from_issues(
        "docs",
        "pass",
        "failed",
        EXIT_FAIL,
        issues,
        json!({
            "checks": checks,
            "writes": false,
            "tree_whitespace_clean": outside.is_empty(),
            "outside_harness_trailing_whitespace": outside
        }),
    )
}

fn byte_scan(repo: &Path) -> (Vec<Issue>, Vec<String>) {
    let mut issues = Vec::new();
    let owned_prefixes = [
        "crates/xtask/",
        "ci/",
        ".github/workflows/",
        "testing/harness/",
        "contracts/evidence/",
        "supply-chain/harness-dependency-delta",
        "supply-chain/licenses/harness/",
    ];
    let mut files = Vec::new();
    if let Ok(output) = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(repo)
        .output()
    {
        files.extend(split_null(&output.stdout));
    }
    if let Ok(output) = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard", "-z"])
        .current_dir(repo)
        .output()
    {
        files.extend(split_null(&output.stdout));
    }
    let mut outside = Vec::new();
    for relative in files {
        if relative.starts_with("target/")
            || relative.starts_with("bench/datagen/target/")
            || relative.contains("/.git/")
            || relative == "testing/harness/containment-supervisor/bin/containment-supervisor"
            || relative
                .starts_with("testing/harness/containment-supervisor/provenance/licenses/")
        {
            // The supervisor is a pinned ELF. License files are upstream
            // redistribution text, not harness prose.
            continue;
        }
        let owned = owned_prefixes
            .iter()
            .any(|prefix| relative.starts_with(prefix));
        let Ok(bytes) = read_no_symlink(repo, &relative) else {
            continue;
        };
        if bytes.contains(&0) && owned {
            issues.push(Issue::new(
                "docs.nul",
                &relative,
                "NUL byte in a harness text path",
            ));
            continue;
        }
        if !is_text(&relative) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        for (number, line) in text.lines().enumerate() {
            if line.starts_with("<<<<<<<") || line.starts_with(">>>>>>>") {
                issues.push(Issue::new(
                    "docs.conflict",
                    &relative,
                    format!("line {}", number + 1),
                ));
            }
            if owned && (line.ends_with(' ') || line.ends_with('\t')) {
                issues.push(Issue::new(
                    "docs.whitespace",
                    &relative,
                    format!("line {}", number + 1),
                ));
            } else if !owned && (line.ends_with(' ') || line.ends_with('\t')) && outside.len() < 20
            {
                outside.push(format!("{relative}:{}", number + 1));
            }
        }
    }
    (issues, outside)
}

fn is_text(path: &str) -> bool {
    matches!(
        Path::new(path).extension().and_then(|ext| ext.to_str()),
        Some("rs" | "toml" | "md" | "yml" | "yaml" | "json" | "py" | "sh" | "txt")
    )
}

fn split_null(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|item| !item.is_empty())
        .map(|item| String::from_utf8_lossy(item).into_owned())
        .collect()
}

pub fn release_cmd(args: &[String]) -> Report {
    let mut digest = None;
    let mut check = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--check" => check = true,
            "--digest" => {
                index += 1;
                digest = args.get(index).cloned();
            }
            "--help" | "-h" => {
                return Report::ok(
                    "release",
                    "help",
                    json!({"usage": "xtask release --check [--digest HEX]", "publishes": false}),
                );
            }
            other => return usage("release", &format!("unknown release argument {other}")),
        }
        index += 1;
    }
    if !check {
        return usage("release", "release requires --check");
    }
    if let Some(digest) = &digest
        && !digest.is_empty()
        && (digest.len() != 64 || !digest.chars().all(|ch| ch.is_ascii_hexdigit()))
    {
        return usage("release", "digest must be 64 hex characters");
    }
    Report::fail(
        "release",
        "blocked",
        EXIT_PENDING,
        vec![Issue::new(
            "release.blocked",
            "release",
            "no RC artifact is signed or promoted. This command does not push or deploy.",
        )],
        json!({
            "digest_recorded": digest.filter(|item| !item.is_empty()),
            "promoted": false,
            "published": false,
            "signed": false,
            "tag_pushed": false
        }),
    )
}

const DATAGEN_NEXT: &str = "Fixture execution is isolated and is not W05. The development path checks the a035 tree hash and the ledger, then returns datagen.live_ready with the generator unspawned. Live integration is a later task. REVIEW_FROZEN is not a receipt.";
const INFRA_NEXT: &str = "Fixture execution is isolated and is not W05. Development identity is checked against the a038 inventory. Docker is not started. Full infra integration is a later task. REVIEW_FROZEN is not a receipt.";
const DATAGEN_EVIDENCE: &str =
    "evidence/eef831aa-dba4-4af2-a78b-541bc9e0a035/SP00/datagen-review/findings.json";
const DATAGEN_SESSION: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a035";
const DATAGEN_WRITER: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a042";
const INFRA_EVIDENCE: &str =
    "evidence/eef831aa-dba4-4af2-a78b-541bc9e0a038/SP00/infra-review/findings.json";
const INFRA_SESSION: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a038";
const INFRA_INVENTORY: &str =
    "evidence/eef831aa-dba4-4af2-a78b-541bc9e0a038/SP00/infra-review/raw/infra-before.sha256";
const INFRA_WRITER: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a041";

struct Receipt {
    context: String,
    files: Vec<(String, String)>,
    approval: Option<Approval>,
}

struct Approval {
    evidence_session: String,
    evidence_sha256: String,
    decision: String,
    source_sha256: String,
    coordinator_binding: String,
}

fn load_receipt(repo: &Path, path: &Path, subject: &str) -> Result<Receipt, Issue> {
    if path.is_symlink() {
        return Err(Issue::new(
            "receipt.symlink",
            path.display().to_string(),
            "a symlink is not a review receipt",
        ));
    }
    let bytes = fs::read(path).map_err(|error| {
        Issue::new(
            "receipt.read",
            path.display().to_string(),
            error.to_string(),
        )
    })?;
    if bytes.len() > 1_048_576 {
        return Err(Issue::new(
            "receipt.read",
            path.display().to_string(),
            "receipt exceeds 1 MiB",
        ));
    }
    let text = String::from_utf8(bytes).map_err(|_| {
        Issue::new(
            "receipt.read",
            path.display().to_string(),
            "receipt is not utf-8",
        )
    })?;
    let value = oo_core::json_scan::parse_json_value(&text, "review-receipt").map_err(|list| {
        Issue::new(
            "receipt.parse",
            path.display().to_string(),
            list.items
                .first()
                .map(|item| item.message.clone())
                .unwrap_or_else(|| "receipt JSON was rejected".to_owned()),
        )
    })?;
    let Some(object) = value.as_object() else {
        return Err(Issue::new(
            "receipt.schema",
            path.display().to_string(),
            "receipt must be an object",
        ));
    };
    for key in object.keys() {
        if !matches!(
            key.as_str(),
            "schema_version"
                | "subject"
                | "author"
                | "reviewer"
                | "independent"
                | "context"
                | "files"
                | "approval"
        ) {
            return Err(Issue::new(
                "receipt.schema",
                key.clone(),
                "unknown receipt field",
            ));
        }
    }
    if object.get("schema_version").and_then(Value::as_str) != Some("review-receipt/v1") {
        return Err(Issue::new(
            "receipt.schema",
            "schema_version",
            "schema_version must be review-receipt/v1",
        ));
    }
    if object.get("subject").and_then(Value::as_str) != Some(subject) {
        return Err(Issue::new(
            "receipt.subject",
            subject,
            "receipt subject does not match this command",
        ));
    }
    let author = object
        .get("author")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let reviewer = object
        .get("reviewer")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if author.is_empty()
        || reviewer.is_empty()
        || author == reviewer
        || reviewer == "OWNER_PENDING"
        || reviewer.contains("OWNER_PENDING")
    {
        return Err(Issue::new(
            "receipt.identity",
            reviewer,
            "reviewer must differ from the author and must not be OWNER_PENDING",
        ));
    }
    if object.get("independent") != Some(&Value::Bool(true)) {
        return Err(Issue::new(
            "receipt.identity",
            "independent",
            "independent must be true",
        ));
    }
    let context = object.get("context").and_then(Value::as_str).unwrap_or("");
    if context != "fixture" && context != "appointed" && context != "development" {
        return Err(Issue::new(
            "receipt.context",
            context,
            "context must be fixture, appointed, or development",
        ));
    }
    if context != "development" && object.contains_key("approval") {
        return Err(Issue::new(
            "receipt.schema",
            "approval",
            "approval is only valid on a development receipt",
        ));
    }
    let approval = if context == "development" {
        Some(load_approval(object.get("approval"))?)
    } else {
        None
    };
    if context == "development" {
        let expected = if subject == "datagen" {
            DATAGEN_SESSION
        } else {
            INFRA_SESSION
        };
        if reviewer != expected {
            return Err(Issue::new(
                "receipt.reviewer",
                reviewer,
                "development reviewer must be the evidence session id. A GitHub login is not invented here.",
            ));
        }
    }
    if context == "appointed" && !appointed_logins(repo).iter().any(|login| login == reviewer) {
        return Err(Issue::new(
            "receipt.reviewer",
            reviewer,
            "no appointed CODEOWNERS login matches the reviewer. A filename or self-signature is not an appointment.",
        ));
    }
    let Some(files) = object.get("files").and_then(Value::as_array) else {
        return Err(Issue::new(
            "receipt.files",
            "files",
            "files must be an array",
        ));
    };
    if files.is_empty() {
        return Err(Issue::new(
            "receipt.files",
            "files",
            "receipt binds no files",
        ));
    }
    let mut bound = Vec::new();
    for item in files {
        let Some(file) = item.as_object() else {
            return Err(Issue::new(
                "receipt.files",
                "files",
                "each file must be an object",
            ));
        };
        for key in file.keys() {
            if key != "path" && key != "sha256" {
                return Err(Issue::new(
                    "receipt.files",
                    key.clone(),
                    "unknown file field",
                ));
            }
        }
        let path = file.get("path").and_then(Value::as_str).unwrap_or("");
        let sha = file.get("sha256").and_then(Value::as_str).unwrap_or("");
        if path.is_empty()
            || sha.len() != 64
            || !sha
                .chars()
                .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
        {
            return Err(Issue::new(
                "receipt.files",
                path,
                "file path and lowercase sha256 are required",
            ));
        }
        if bound.iter().any(|(seen, _)| seen == path) {
            return Err(Issue::new("receipt.files", path, "duplicate receipt path"));
        }
        bound.push((path.to_owned(), sha.to_owned()));
    }
    Ok(Receipt {
        context: context.to_owned(),
        files: bound,
        approval,
    })
}

fn load_approval(value: Option<&Value>) -> Result<Approval, Issue> {
    let Some(object) = value.and_then(Value::as_object) else {
        return Err(Issue::new(
            "receipt.approval",
            "approval",
            "a development receipt needs an approval object",
        ));
    };
    for key in object.keys() {
        if !matches!(
            key.as_str(),
            "evidence_session"
                | "evidence_sha256"
                | "decision"
                | "source_sha256"
                | "coordinator_binding"
        ) {
            return Err(Issue::new(
                "receipt.approval",
                key.clone(),
                "unknown approval field",
            ));
        }
    }
    let take = |key: &str| {
        object
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_owned()
    };
    let approval = Approval {
        evidence_session: take("evidence_session"),
        evidence_sha256: take("evidence_sha256"),
        decision: take("decision"),
        source_sha256: take("source_sha256"),
        coordinator_binding: take("coordinator_binding"),
    };
    for (label, value) in [
        ("evidence_session", approval.evidence_session.as_str()),
        ("evidence_sha256", approval.evidence_sha256.as_str()),
        ("decision", approval.decision.as_str()),
        ("source_sha256", approval.source_sha256.as_str()),
        ("coordinator_binding", approval.coordinator_binding.as_str()),
    ] {
        if value.is_empty() {
            return Err(Issue::new(
                "receipt.approval",
                label,
                "approval field is missing",
            ));
        }
    }
    for (label, value) in [
        ("evidence_sha256", approval.evidence_sha256.as_str()),
        ("source_sha256", approval.source_sha256.as_str()),
        ("coordinator_binding", approval.coordinator_binding.as_str()),
    ] {
        if value.len() != 64
            || !value
                .chars()
                .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
        {
            return Err(Issue::new(
                "receipt.approval",
                label,
                "approval digest must be 64 lowercase hex",
            ));
        }
    }
    Ok(approval)
}

fn bind_outside(repo: &Path, path: &Path, receipt: &Receipt) -> Result<PathBuf, Issue> {
    if !path.is_absolute() {
        return Err(Issue::new(
            "receipt.path",
            path.display().to_string(),
            "fixture paths must be absolute and outside the repository",
        ));
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        Issue::new(
            "receipt.path",
            path.display().to_string(),
            error.to_string(),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(Issue::new(
            "receipt.symlink",
            path.display().to_string(),
            "fixture path must be a regular file",
        ));
    }
    let canon = path.canonicalize().map_err(|error| {
        Issue::new(
            "receipt.path",
            path.display().to_string(),
            error.to_string(),
        )
    })?;
    reject_symlink_components(&canon)?;
    let repo_canon = repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf());
    if canon.starts_with(&repo_canon) {
        return Err(Issue::new(
            "receipt.path",
            canon.display().to_string(),
            "fixture path is inside the repository. Live bench and infra paths are not executed.",
        ));
    }
    let bytes = fs::read(&canon).map_err(|error| {
        Issue::new(
            "receipt.path",
            canon.display().to_string(),
            error.to_string(),
        )
    })?;
    let hash = sha256_hex(&bytes);
    let listed = receipt.files.iter().any(|(listed, sha)| {
        let listed_path = Path::new(listed);
        listed_path.is_absolute()
            && listed_path
                .canonicalize()
                .is_ok_and(|got| got == canon && sha == &hash)
    });
    if !listed {
        return Err(Issue::new(
            "receipt.hash",
            canon.display().to_string(),
            "file bytes are not bound by the receipt",
        ));
    }
    Ok(canon)
}

fn reject_symlink_components(path: &Path) -> Result<(), Issue> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        if current.is_symlink() {
            return Err(Issue::new(
                "receipt.symlink",
                current.display().to_string(),
                "symlink components are refused",
            ));
        }
    }
    Ok(())
}

fn parse_seed(value: &str) -> Option<i64> {
    if value.is_empty() || value.len() > 20 {
        return None;
    }
    let (negative, digits) = if let Some(rest) = value.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = value.strip_prefix('+') {
        (false, rest)
    } else {
        (false, value)
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let parsed = digits.parse::<i64>().ok()?;
    if negative {
        parsed.checked_neg()
    } else {
        Some(parsed)
    }
}

fn valid_runtime(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 32
        && !value.contains("://")
        && !value.contains('/')
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn is_uuid(value: &str) -> bool {
    let parts: Vec<&str> = value.split('-').collect();
    parts.len() == 5
        && [8, 4, 4, 4, 12]
            .iter()
            .zip(parts.iter())
            .all(|(width, part)| {
                part.len() == *width && part.chars().all(|ch| ch.is_ascii_hexdigit())
            })
}

struct ChildRun {
    exit_code: i32,
    timed_out: bool,
    error: Option<String>,
}

fn child_deadline() -> Result<u64, Issue> {
    const CAP: u64 = 120;
    match std::env::var("OO_XTASK_CHILD_DEADLINE_SECS") {
        Ok(value) if !value.is_empty() => match value.parse::<u64>() {
            Ok(seconds) if (1..=CAP).contains(&seconds) => Ok(seconds),
            _ => Err(Issue::new(
                "child.deadline",
                "OO_XTASK_CHILD_DEADLINE_SECS",
                "the child deadline may only be set from 1 to 120 seconds",
            )),
        },
        _ => Ok(CAP),
    }
}

fn child_failure_message(child: &ChildRun) -> String {
    match (&child.error, child.timed_out) {
        (Some(error), true) => format!(
            "the fixture child exceeded its deadline. Cleanup is incomplete: {error}. A same-group signal does not contain a descendant that left the group."
        ),
        (None, true) => "the fixture child exceeded its deadline. Recorded owned processes were signaled and the owned stage was removed. A same-group signal does not contain a descendant that left the group.".to_owned(),
        (Some(error), false) => format!("the fixture child did not finish cleanly: {error}"),
        (None, false) => {
            "the fixture child exited nonzero. This is not a pending result.".to_owned()
        }
    }
}

fn run_bounded(mut command: Command, deadline: Duration) -> ChildRun {
    if let Some(contained) = supervise_fixture(&command, deadline) {
        return contained;
    }
    #[cfg(unix)]
    {
        command.process_group(0);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return ChildRun {
                exit_code: -1,
                timed_out: false,
                error: Some(error.to_string()),
            };
        }
    };
    let mut tree = crate::owned_process::OwnedTree::capture(child.id());
    let started = Instant::now();
    let (mut exit_code, timed_out, early_error) = loop {
        tree.refresh();
        match child.try_wait() {
            Ok(Some(status)) => break (status.code().unwrap_or(1), false, None),
            Ok(None) if started.elapsed() >= deadline => break (-1, true, None),
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                let _ = tree.kill_recorded();
                let _ = child.kill();
                let _ = child.wait();
                break (-1, false, Some(error.to_string()));
            }
        }
    };
    if timed_out {
        let _ = tree.kill_recorded();
        let _ = child.kill();
        let _ = child.wait();
    }
    tree.note_reaped_root();
    let cleanup = tree.confirm(Duration::from_millis(1500));
    if cleanup.incomplete && exit_code == 0 {
        exit_code = -1;
    }
    let error = if cleanup.incomplete {
        Some(match early_error {
            Some(error) => format!("cleanup incomplete: {error}"),
            None => format!("cleanup incomplete; surviving pids {:?}", cleanup.survivors),
        })
    } else {
        early_error
    };
    ChildRun {
        exit_code,
        timed_out,
        error,
    }
}

fn supervise_fixture(command: &Command, deadline: Duration) -> Option<ChildRun> {
    let dir = std::env::temp_dir().join(format!(
        "oo-xtask-bounded-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0)
    ));
    if fs::create_dir_all(&dir).is_err() {
        return None;
    }
    let contained = crate::containment::supervise(
        command,
        deadline,
        &dir.join("stdout"),
        &dir.join("stderr"),
        &dir.join("status"),
        crate::runner_capture::OUTPUT_LIMIT_BYTES as u64,
    );
    let _ = fs::remove_dir_all(&dir);
    // Only a platform with no ownership primitive falls through. A missing or
    // untrusted helper on Linux must not become an ordinary process-group run.
    if !contained.spawned
        && contained.containment == crate::containment::UNAVAILABLE
        && contained.note == "platform"
    {
        return None;
    }
    let error = if contained.containment != crate::containment::VERIFIED
        || contained.cleanup_incomplete
        || !contained.spawned
    {
        Some(format!(
            "containment {} ({})",
            contained.containment, contained.note
        ))
    } else if contained.output_limited {
        Some("command output exceeded the capture limit".to_owned())
    } else {
        None
    };
    Some(ChildRun {
        exit_code: contained.exit_code,
        timed_out: contained.timed_out,
        error,
    })
}

fn outside_path(repo: &Path, path: &Path, must_exist: bool) -> Result<PathBuf, Issue> {
    if !path.is_absolute() {
        return Err(Issue::new(
            "datagen.path",
            path.display().to_string(),
            "generator paths must be absolute and outside the repository",
        ));
    }
    if path
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(Issue::new(
            "datagen.path",
            path.display().to_string(),
            "generator paths cannot contain ..",
        ));
    }
    let repo_canon = repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf());
    if path.is_symlink() {
        return Err(Issue::new(
            "receipt.symlink",
            path.display().to_string(),
            "generator path is a symlink",
        ));
    }
    if path.exists() {
        let canon = path.canonicalize().map_err(|error| {
            Issue::new(
                "datagen.path",
                path.display().to_string(),
                error.to_string(),
            )
        })?;
        if canon.starts_with(&repo_canon) {
            return Err(Issue::new(
                "datagen.path",
                canon.display().to_string(),
                "generator path is inside the repository",
            ));
        }
        return Ok(canon);
    }
    if must_exist {
        return Err(Issue::new(
            "datagen.path",
            path.display().to_string(),
            "generator path does not exist",
        ));
    }
    let Some(parent) = path.parent() else {
        return Err(Issue::new(
            "datagen.path",
            path.display().to_string(),
            "generator path has no parent",
        ));
    };
    let parent_canon = parent.canonicalize().map_err(|error| {
        Issue::new(
            "datagen.path",
            parent.display().to_string(),
            error.to_string(),
        )
    })?;
    if parent_canon.starts_with(&repo_canon) {
        return Err(Issue::new(
            "datagen.path",
            path.display().to_string(),
            "generator path is inside the repository",
        ));
    }
    Ok(parent_canon.join(path.file_name().unwrap_or_default()))
}

fn create_owned_stage(path: &Path) -> Result<(), Issue> {
    if path.exists() {
        return Err(Issue::new(
            "datagen.output",
            path.display().to_string(),
            "output path already exists and was not replaced",
        ));
    }
    fs::create_dir_all(path).map_err(|error| {
        Issue::new(
            "datagen.output",
            path.display().to_string(),
            error.to_string(),
        )
    })?;
    fs::write(
        path.join(".oo-owned-stage"),
        format!("{}\n", std::process::id()),
    )
    .map_err(|error| {
        Issue::new(
            "datagen.output",
            path.display().to_string(),
            error.to_string(),
        )
    })?;
    Ok(())
}

fn cleanup_owned_stage(path: &Path) {
    let marker = path.join(".oo-owned-stage");
    let Ok(text) = fs::read_to_string(&marker) else {
        return;
    };
    if text.trim() == std::process::id().to_string() {
        let _ = fs::remove_dir_all(path);
    }
}

struct FixturePlan<'a> {
    dataset: Option<&'a str>,
    scale: Option<&'a str>,
    seed: Option<u64>,
    output: Option<&'a str>,
    scope: Option<&'a str>,
    list_name: Option<&'a str>,
}

fn fixture_datagen_plan(
    repo: &Path,
    operation: &str,
    plan: FixturePlan<'_>,
) -> Result<(Vec<String>, Option<PathBuf>), Issue> {
    let FixturePlan {
        dataset,
        scale,
        seed,
        output,
        scope,
        list_name,
    } = plan;
    let usage_issue = |message: &str| Issue::new("cli.usage", "datagen", message);
    match operation {
        "verify" => {
            let Some(dataset) = dataset else {
                return Err(usage_issue("verify requires --dataset"));
            };
            let path = outside_path(repo, Path::new(dataset), true)?;
            Ok((
                vec![
                    "verify".to_owned(),
                    "--dataset".to_owned(),
                    path.display().to_string(),
                ],
                None,
            ))
        }
        "generate" => {
            let Some(scale) = scale else {
                return Err(usage_issue("generate requires --scale xs|s"));
            };
            let Some(seed) = seed else {
                return Err(usage_issue("generate requires --seed"));
            };
            let Some(output) = output else {
                return Err(usage_issue("generate requires --output"));
            };
            let Some(scope) = scope else {
                return Err(usage_issue("generate requires --scope"));
            };
            let output = outside_path(repo, Path::new(output), false)?;
            let scope = outside_path(repo, Path::new(scope), true)?;
            create_owned_stage(&output)?;
            Ok((
                vec![
                    "generate".to_owned(),
                    "--scale".to_owned(),
                    scale.to_owned(),
                    "--seed".to_owned(),
                    seed.to_string(),
                    "--output".to_owned(),
                    output.display().to_string(),
                    "--scope".to_owned(),
                    scope.display().to_string(),
                ],
                Some(output),
            ))
        }
        "export-sources" => {
            let Some(dataset) = dataset else {
                return Err(usage_issue("export-sources requires --dataset"));
            };
            let Some(output) = output else {
                return Err(usage_issue("export-sources requires --output"));
            };
            let dataset = outside_path(repo, Path::new(dataset), true)?;
            let output = outside_path(repo, Path::new(output), false)?;
            create_owned_stage(&output)?;
            let mut args = vec![
                "export-sources".to_owned(),
                "--dataset".to_owned(),
                dataset.display().to_string(),
                "--output".to_owned(),
                output.display().to_string(),
            ];
            if let Some(scope) = scope {
                let scope = outside_path(repo, Path::new(scope), true)?;
                args.push("--scope".to_owned());
                args.push(scope.display().to_string());
            }
            Ok((args, Some(output)))
        }
        "list" => {
            let Some(name) = list_name else {
                return Err(usage_issue("list requires --list-name schema|fixtures"));
            };
            Ok((vec!["list".to_owned(), name.to_owned()], None))
        }
        other => Err(usage_issue(&format!("unknown operation {other}"))),
    }
}

fn development_datagen(repo: &Path, receipt: &Receipt, mut base: Value) -> Report {
    let approval = match receipt.approval.as_ref() {
        Some(approval) => approval,
        None => {
            return Report::fail(
                "datagen",
                "failed",
                EXIT_FAIL,
                vec![Issue::new(
                    "receipt.approval",
                    "approval",
                    "approval object is missing",
                )],
                base,
            );
        }
    };
    match bind_development(repo, "datagen", approval, DATAGEN_EVIDENCE, DATAGEN_SESSION) {
        Ok(binding) => {
            if let Some(object) = base.as_object_mut() {
                object.insert("source_sha256".to_owned(), json!(binding.source_sha256));
                object.insert(
                    "current_source_sha256".to_owned(),
                    json!(binding.current_source_sha256),
                );
                object.insert(
                    "identity_matches".to_owned(),
                    json!(binding.identity_matches),
                );
                object.insert("w05_datagen_integration".to_owned(), json!(false));
                object.insert("at_eval_01".to_owned(), json!(false));
                object.insert("generator_invoked".to_owned(), json!(false));
                if let Some(line) = &binding.ledger_line {
                    object.insert("ledger_line".to_owned(), json!(line));
                }
            }
            if !binding.identity_matches {
                return Report::fail(
                    "datagen",
                    "pending",
                    EXIT_PENDING,
                    vec![Issue::new(
                        "datagen.identity",
                        "bench/datagen",
                        "the current bench/datagen tree does not match the a035 approval. The generator was not spawned.",
                    )],
                    base,
                );
            }
            if let Some(line) = binding.ledger_line {
                return Report::fail(
                    "datagen",
                    "pending",
                    EXIT_PENDING,
                    vec![Issue::new(
                        "datagen.writer",
                        DATAGEN_WRITER,
                        format!("the ledger still records a running writer: {line}"),
                    )],
                    base,
                );
            }
            Report::fail(
                "datagen",
                "pending",
                EXIT_PENDING,
                vec![Issue::new(
                    "datagen.live_ready",
                    "bench/datagen",
                    "identity matches and no writer is running. This command does not treat that as W05 or AT-EVAL-01. Live spawn stays deferred until the integration task.",
                )],
                base,
            )
        }
        Err(issue) => Report::fail("datagen", "failed", EXIT_FAIL, vec![issue], base),
    }
}

struct DevelopmentBinding {
    source_sha256: String,
    current_source_sha256: String,
    identity_matches: bool,
    ledger_line: Option<String>,
}

fn bind_development(
    repo: &Path,
    subject: &str,
    approval: &Approval,
    evidence_rel: &str,
    session: &str,
) -> Result<DevelopmentBinding, Issue> {
    if approval.evidence_session != session {
        return Err(Issue::new(
            "receipt.approval",
            "evidence_session",
            "approval session does not match the reviewed evidence",
        ));
    }
    if approval.decision != "approve" {
        return Err(Issue::new(
            "receipt.approval",
            "decision",
            "approval decision must be approve",
        ));
    }
    let evidence = repo.join(evidence_rel);
    let bytes = fs::read(&evidence)
        .map_err(|error| Issue::new("receipt.approval", evidence_rel, error.to_string()))?;
    let digest = sha256_hex(&bytes);
    if digest != approval.evidence_sha256 {
        return Err(Issue::new(
            "receipt.approval",
            evidence_rel,
            "evidence bytes do not match the approval digest",
        ));
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
        Issue::new(
            "receipt.approval",
            evidence_rel,
            "evidence file is not JSON",
        )
    })?;
    if value.get("decision").and_then(Value::as_str) != Some("approve") {
        return Err(Issue::new(
            "receipt.approval",
            evidence_rel,
            "evidence decision is not approve",
        ));
    }
    let (source_sha256, current) = if subject == "datagen" {
        if value.get("signs_at_eval_01") != Some(&Value::Bool(false)) {
            return Err(Issue::new(
                "receipt.approval",
                evidence_rel,
                "a035 does not sign AT-EVAL-01",
            ));
        }
        let approved = value
            .get("datagen_tree_sha256")
            .and_then(Value::as_str)
            .unwrap_or("");
        if approved != approval.source_sha256 {
            return Err(Issue::new(
                "receipt.approval",
                "source_sha256",
                "approval source hash does not match the a035 evidence",
            ));
        }
        let current = datagen_tree_sha256(repo)
            .map_err(|error| Issue::new("datagen.identity", "bench/datagen", error))?;
        (approved.to_owned(), current)
    } else {
        let approved = value
            .pointer("/infra_hash/list_sha256")
            .and_then(Value::as_str)
            .unwrap_or("");
        if approved != approval.source_sha256 {
            return Err(Issue::new(
                "receipt.approval",
                "source_sha256",
                "approval source hash does not match the a038 evidence",
            ));
        }
        let current = infra_inventory_identity(repo)?;
        (approved.to_owned(), current)
    };
    let preimage = format!(
        "open-ontology-development-approval/v1\nsubject={subject}\nevidence_session={session}\nevidence_sha256={digest}\ndecision=approve\nsource_sha256={source_sha256}\nat_eval_01=false\nw05=false\n"
    );
    let binding = sha256_hex(preimage.as_bytes());
    if approval.coordinator_binding != binding {
        return Err(Issue::new(
            "receipt.approval",
            "coordinator_binding",
            "coordinator binding does not match the evidence bytes and source hash",
        ));
    }
    let writer = if subject == "datagen" {
        DATAGEN_WRITER
    } else {
        INFRA_WRITER
    };
    Ok(DevelopmentBinding {
        identity_matches: current == source_sha256,
        source_sha256,
        current_source_sha256: current,
        ledger_line: ledger_running_line(repo, writer),
    })
}

fn datagen_tree_sha256(repo: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let root = repo.join("bench/datagen");
    let mut files = Vec::new();
    collect_files(&root, &root, &mut files)?;
    files.sort();
    let mut hasher = Sha256::new();
    for relative in files {
        let bytes = fs::read(root.join(&relative)).map_err(|error| error.to_string())?;
        let digest = Sha256::digest(&bytes);
        hasher.update(relative.as_bytes());
        hasher.update([0]);
        hasher.update(digest);
        hasher.update([0]);
        hasher.update(bytes.len().to_string().as_bytes());
        hasher.update(b"\n");
    }
    Ok(sha256_hex(&hasher.finalize()))
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|error| error.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|item| item.to_str())
            .unwrap_or("");
        if name == "target" {
            continue;
        }
        if path.is_dir() {
            collect_files(root, &path, out)?;
            continue;
        }
        if path.is_file() {
            let relative = path.strip_prefix(root).map_err(|error| error.to_string())?;
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

fn infra_inventory_identity(repo: &Path) -> Result<String, Issue> {
    let inventory = repo.join(INFRA_INVENTORY);
    let bytes = fs::read(&inventory)
        .map_err(|error| Issue::new("infra.identity", INFRA_INVENTORY, error.to_string()))?;
    let text = String::from_utf8(bytes.clone())
        .map_err(|_| Issue::new("infra.identity", INFRA_INVENTORY, "inventory is not utf-8"))?;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(expected) = parts.next() else {
            continue;
        };
        let Some(relative) = parts.next() else {
            continue;
        };
        let file = repo.join(relative);
        let actual = fs::read(&file)
            .map(|bytes| sha256_hex(&bytes))
            .unwrap_or_default();
        if actual != expected {
            return Ok(sha256_hex(&bytes) + "-stale");
        }
    }
    Ok(sha256_hex(&bytes))
}

fn ledger_running_line(repo: &Path, session: &str) -> Option<String> {
    let bytes = read_no_symlink(repo, "specs/open-ontology-development-ledger.md").ok()?;
    let text = String::from_utf8_lossy(&bytes);
    text.lines()
        .find(|line| line.contains(session) && line.contains("running"))
        .map(|line| line.trim().to_owned())
}

pub fn ledger_running(repo: &Path, session: &str) -> bool {
    let Ok(bytes) = read_no_symlink(repo, "specs/open-ontology-development-ledger.md") else {
        return true;
    };
    let text = String::from_utf8_lossy(&bytes);
    text.lines()
        .any(|line| line.contains(session) && line.contains("running"))
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn usage(command: &str, message: &str) -> Report {
    Report::fail(
        command,
        "usage",
        EXIT_USAGE,
        vec![Issue::new("cli.usage", command, message)],
        json!({}),
    )
}
