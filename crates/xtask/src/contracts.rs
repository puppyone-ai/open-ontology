//! Checked-in schema and fixture comparison. This command never writes schemas.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

use crate::report::{EXIT_FAIL, EXIT_USAGE, Issue, Report};
use crate::util::{read_no_symlink, repo_root, sha256_hex};

const SCHEMA_FILES: &[&str] = &[
    "config-v1.json",
    "oos-v1.json",
    "batch-v1.json",
    "api-v1.json",
    "error-v1.json",
    "openapi-v1.json",
];

const CORE_TESTS: &[&str] = &[
    "schema_contract",
    "canonical_golden",
    "fixtures_pg_mysql",
    "frozen_corrections",
    "boundary_corrections",
];

const CONFIG_EXAMPLES: &[&str] = &[
    "examples/config/oo.yaml",
    "examples/config/sources.yaml",
    "examples/config/files.yaml",
    "examples/config/roles.yaml",
    "examples/config/agent.yaml",
    "examples/config/hooks.yaml",
    "examples/config/eval-customer-core.yaml",
];

pub fn run(args: &[String]) -> Report {
    let mut root = repo_root();
    let mut library_only = false;
    let mut self_test = false;
    let mut check = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--check" => check = true,
            "--library-only" => library_only = true,
            "--self-test" => self_test = true,
            "--root" => {
                index += 1;
                match args.get(index) {
                    Some(path) => root = PathBuf::from(path),
                    None => return usage("contracts --root requires a path"),
                }
            }
            "--help" | "-h" => return help(),
            other => return usage(&format!("unknown contracts argument {other}")),
        }
        index += 1;
    }
    if self_test {
        return self_test_at(&repo_root());
    }
    if !check {
        return usage("contracts requires --check");
    }
    check_root(&root, !library_only)
}

fn help() -> Report {
    Report::ok(
        "contracts",
        "help",
        json!({
            "usage": "xtask contracts --check [--root PATH] [--library-only] | xtask contracts --self-test",
            "writes": false,
            "core_tests": CORE_TESTS,
            "note": "A cargo filter that runs zero tests is a failure. --library-only compares documents, returns exit 3, and does not satisfy the full gate."
        }),
    )
}

fn usage(message: &str) -> Report {
    Report::fail(
        "contracts",
        "usage",
        EXIT_USAGE,
        vec![Issue::new("cli.usage", "contracts", message)],
        json!({}),
    )
}

pub fn check_root(root: &Path, run_cargo: bool) -> Report {
    let mut issues = library_issues(root);
    let mut cargo_details = json!({"ran": false});
    if run_cargo {
        match run_core_tests(root) {
            Ok(details) => cargo_details = details,
            Err(issue) => issues.push(issue),
        }
    } else if issues.is_empty() {
        return Report::fail(
            "contracts",
            "library_only",
            crate::report::EXIT_PENDING,
            vec![Issue::new(
                "contract.library_only",
                "contracts",
                "document comparison is not the full maintained core test gate",
            )],
            json!({
                "root": root.display().to_string(),
                "writes": false,
                "cargo": cargo_details,
                "full_gate": false,
                "schema_version_note": "config/v1 oos/v1 batch/v1 api/v1 error/v1; OpenAPI 3.1 components only",
            }),
        );
    }
    Report::from_issues(
        "contracts",
        "pass",
        "failed",
        EXIT_FAIL,
        issues,
        json!({
            "root": root.display().to_string(),
            "writes": false,
            "cargo": cargo_details,
            "full_gate": run_cargo,
            "schema_version_note": "config/v1 oos/v1 batch/v1 api/v1 error/v1; OpenAPI 3.1 components only",
        }),
    )
}

fn library_issues(root: &Path) -> Vec<Issue> {
    let mut issues = Vec::new();
    let generated = match oo_core::schema_export::contract_documents() {
        Ok(docs) => docs,
        Err(error) => {
            issues.push(Issue::new("contract.export", "oo-core", error));
            return issues;
        }
    };
    for (name, value) in &generated {
        let relative = format!("contracts/schema/{name}");
        let pretty = oo_core::schema_export::to_pretty(value);
        match read_no_symlink(root, &relative) {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                if text.as_ref() != pretty {
                    issues.push(Issue::new(
                        "contract.schema.drift",
                        relative,
                        format!(
                            "checked-in bytes differ from oo_core::schema_export (checked-in sha256 {}, generated sha256 {}). Refusing to overwrite.",
                            sha256_hex(&bytes),
                            sha256_hex(pretty.as_bytes())
                        ),
                    ));
                }
            }
            Err(issue) => issues.push(issue),
        }
        if *name != "openapi-v1.json" {
            if !jsonschema::draft202012::meta::is_valid(value) {
                issues.push(Issue::new(
                    "contract.schema.meta",
                    name,
                    "document is not a valid draft 2020-12 schema",
                ));
            }
            if let Err(error) = oo_core::schema_util::compile_schema(value) {
                issues.push(Issue::new(
                    "contract.schema.compile",
                    name,
                    format!("compile_schema with x-utf8-max-bytes failed: {error}"),
                ));
            }
        }
    }
    issues.extend(utf8_keyword_probe());
    issues.extend(openapi_shape(&generated));
    issues.extend(validate_examples(root, &generated));
    issues.extend(validate_api_fixtures(root, &generated));
    issues.extend(source_cells(root));
    let present: Vec<&str> = generated.iter().map(|(name, _)| *name).collect();
    for required in SCHEMA_FILES {
        if !present.contains(required) {
            issues.push(Issue::new(
                "contract.schema.missing_export",
                *required,
                "oo-core did not export this schema",
            ));
        }
    }
    issues
}

fn utf8_keyword_probe() -> Vec<Issue> {
    let schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "sql": {"type": "string", "maxLength": 10, "x-utf8-max-bytes": 4}
        },
        "required": ["sql"]
    });
    let Ok(validator) = oo_core::schema_util::compile_schema(&schema) else {
        return vec![Issue::new(
            "contract.utf8_keyword",
            "x-utf8-max-bytes",
            "probe schema did not compile",
        )];
    };
    let mut issues = Vec::new();
    if !validator.is_valid(&json!({"sql": "abcd"})) {
        issues.push(Issue::new(
            "contract.utf8_keyword",
            "x-utf8-max-bytes",
            "4 ASCII bytes were rejected",
        ));
    }
    if validator.is_valid(&json!({"sql": "ééé"})) {
        issues.push(Issue::new(
            "contract.utf8_keyword",
            "x-utf8-max-bytes",
            "3 scalars / 6 UTF-8 bytes passed a 4-byte budget; maxLength alone is not the byte check",
        ));
    }
    issues
}

fn openapi_shape(docs: &[(&str, Value)]) -> Vec<Issue> {
    let Some((_, openapi)) = docs.iter().find(|(name, _)| *name == "openapi-v1.json") else {
        return vec![Issue::new(
            "contract.openapi.missing",
            "openapi-v1.json",
            "export has no OpenAPI document",
        )];
    };
    let mut issues = Vec::new();
    if openapi.get("openapi").and_then(Value::as_str) != Some("3.1.0") {
        issues.push(Issue::new(
            "contract.openapi.version",
            "openapi-v1.json",
            format!(
                "openapi field is {}",
                openapi.get("openapi").unwrap_or(&Value::Null)
            ),
        ));
    }
    let paths = openapi.get("paths");
    let empty = paths
        .and_then(Value::as_object)
        .is_none_or(|map| map.is_empty());
    if !empty {
        issues.push(Issue::new(
            "contract.openapi.paths",
            "openapi-v1.json",
            "SP00 OpenAPI must not declare operations",
        ));
    }
    let schemas = openapi
        .pointer("/components/schemas")
        .and_then(Value::as_object);
    if schemas.is_none_or(|map| map.is_empty()) {
        issues.push(Issue::new(
            "contract.openapi.components",
            "openapi-v1.json",
            "component schemas are empty",
        ));
    }
    issues
}

fn validate_examples(root: &Path, docs: &[(&str, Value)]) -> Vec<Issue> {
    let mut issues = Vec::new();
    let Some((_, config)) = docs.iter().find(|(name, _)| *name == "config-v1.json") else {
        return issues;
    };
    let Some((_, oos)) = docs.iter().find(|(name, _)| *name == "oos-v1.json") else {
        return issues;
    };
    let config_validator = match oo_core::schema_util::compile_schema(config) {
        Ok(validator) => validator,
        Err(error) => {
            issues.push(Issue::new(
                "contract.config.compile",
                "config-v1.json",
                error.to_string(),
            ));
            return issues;
        }
    };
    for relative in CONFIG_EXAMPLES {
        match read_text(root, relative) {
            Ok(text) => match oo_core::yaml::parse_yaml_value(&text, "oo.config/v1") {
                Ok(value) => {
                    if !config_validator.is_valid(&value) {
                        issues.push(Issue::new(
                            "contract.example.schema",
                            *relative,
                            "YAML example does not satisfy config-v1.json",
                        ));
                    }
                }
                Err(error) => issues.push(Issue::new(
                    "contract.example.parse",
                    *relative,
                    error.to_string(),
                )),
            },
            Err(issue) => issues.push(issue),
        }
    }
    let _ = oos;
    let oos_files = [
        ("examples/schema/ontology.yaml", "ontology"),
        ("examples/schema/dataset-kh.yaml", "dataset"),
        ("examples/schema/entity-customer.yaml", "entity"),
        ("examples/schema/relationships.yaml", "relationships"),
        ("examples/schema/enum-ysk-zt.yaml", "enum"),
        ("examples/schema/metrics-finance.yaml", "metrics"),
        ("examples/schema/glossary.yaml", "glossary"),
        ("examples/schema/examples-finance.yaml", "examples"),
        ("examples/schema/calendar.yaml", "calendar"),
        ("examples/schema/resolution-customer.yaml", "resolution"),
        ("examples/schema/decisions.yaml", "decisions"),
    ];
    for (relative, label) in oos_files {
        match read_text(root, relative) {
            Ok(text) => {
                let parsed = match label {
                    "ontology" => oo_core::oos::parse_ontology_yaml(&text).map(|_| ()),
                    "dataset" => oo_core::oos::parse_dataset_yaml(&text).map(|_| ()),
                    "entity" => oo_core::oos::parse_entity_yaml(&text).map(|_| ()),
                    "relationships" => oo_core::oos::parse_relationships_yaml(&text).map(|_| ()),
                    "enum" => oo_core::oos::parse_enum_yaml(&text).map(|_| ()),
                    "metrics" => oo_core::oos::parse_metrics_yaml(&text).map(|_| ()),
                    "glossary" => oo_core::oos::parse_glossary_yaml(&text).map(|_| ()),
                    "examples" => oo_core::oos::parse_examples_yaml(&text).map(|_| ()),
                    "calendar" => oo_core::oos::parse_calendar_yaml(&text).map(|_| ()),
                    "resolution" => oo_core::oos::parse_resolution_yaml(&text).map(|_| ()),
                    "decisions" => oo_core::oos::parse_decisions_yaml(&text).map(|_| ()),
                    _ => Ok(()),
                };
                if let Err(error) = parsed {
                    issues.push(Issue::new(
                        "contract.oos.parse",
                        relative,
                        error.to_string(),
                    ));
                }
            }
            Err(issue) => issues.push(issue),
        }
    }
    issues
}

fn validate_api_fixtures(root: &Path, docs: &[(&str, Value)]) -> Vec<Issue> {
    let mut issues = Vec::new();
    let Some((_, api)) = docs.iter().find(|(name, _)| *name == "api-v1.json") else {
        return issues;
    };
    let Some((_, error_schema)) = docs.iter().find(|(name, _)| *name == "error-v1.json") else {
        return issues;
    };
    let fixtures = [
        (
            "contracts/fixtures/api/query-request.json",
            "QueryRequest",
            false,
        ),
        (
            "contracts/fixtures/api/query-response.json",
            "QueryResponse",
            false,
        ),
        (
            "contracts/fixtures/api/accepted.json",
            "AcceptedOperation",
            false,
        ),
        ("contracts/fixtures/api/page.json", "PageRequest", false),
        (
            "contracts/fixtures/api/export.json",
            "ExportCompletion",
            false,
        ),
        (
            "contracts/fixtures/api/error-rate-limited.json",
            "ErrorEnvelope",
            true,
        ),
    ];
    for (relative, def, is_error) in fixtures {
        let bytes = match read_no_symlink(root, relative) {
            Ok(bytes) => bytes,
            Err(issue) => {
                issues.push(issue);
                continue;
            }
        };
        let instance: Value = match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(error) => {
                issues.push(Issue::new(
                    "contract.fixture.json",
                    relative,
                    error.to_string(),
                ));
                continue;
            }
        };
        let schema = if is_error {
            error_schema.clone()
        } else {
            schema_for_def(api, def)
        };
        match oo_core::schema_util::compile_schema(&schema) {
            Ok(validator) => {
                if !validator.is_valid(&instance) {
                    let errors: Vec<String> = validator
                        .iter_errors(&instance)
                        .map(|error| error.to_string())
                        .collect();
                    issues.push(Issue::new(
                        "contract.fixture.schema",
                        relative,
                        format!("{def} rejected the fixture: {}", errors.join("; ")),
                    ));
                }
            }
            Err(error) => issues.push(Issue::new(
                "contract.fixture.compile",
                relative,
                error.to_string(),
            )),
        }
    }
    issues
}

fn schema_for_def(bundle: &Value, name: &str) -> Value {
    let mut schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$ref": format!("#/$defs/{name}")
    });
    if let Some(defs) = bundle.get("$defs") {
        schema["$defs"] = defs.clone();
    }
    schema
}

fn source_cells(root: &Path) -> Vec<Issue> {
    let relative = "contracts/fixtures/pg-mysql/cases.json";
    let bytes = match read_no_symlink(root, relative) {
        Ok(bytes) => bytes,
        Err(issue) => return vec![issue],
    };
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(error) => {
            return vec![Issue::new(
                "contract.cells.json",
                relative,
                error.to_string(),
            )];
        }
    };
    if value.get("driver").and_then(Value::as_str) != Some("none") {
        return vec![Issue::new(
            "contract.cells.driver",
            relative,
            "driver must stay none; this fixture is not a database execution",
        )];
    }
    Vec::new()
}

fn read_text(root: &Path, relative: &str) -> Result<String, Issue> {
    let bytes = read_no_symlink(root, relative)?;
    String::from_utf8(bytes).map_err(|_| Issue::new("path.utf8", relative, "file is not UTF-8"))
}

pub fn run_core_tests(root: &Path) -> Result<Value, Issue> {
    let mut ran = Vec::new();
    for name in CORE_TESTS {
        let output = Command::new("cargo")
            .arg("test")
            .arg("-p")
            .arg("oo-core")
            .arg("--locked")
            .arg("--offline")
            .arg("--manifest-path")
            .arg(root.join("Cargo.toml"))
            .arg("--test")
            .arg(name)
            .arg("--")
            .arg("--test-threads")
            .arg("8")
            .output()
            .map_err(|error| Issue::new("contract.cargo.spawn", *name, error.to_string()))?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let passed = accept_cargo_test(output.status.success(), &stdout, &stderr)
            .map_err(|message| Issue::new("contract.cargo.test", *name, message))?;
        ran.push(json!({"test": name, "passed": passed, "exit": output.status.code()}));
    }
    Ok(json!({"ran": true, "targets": ran}))
}

pub fn accept_cargo_test(success: bool, stdout: &str, stderr: &str) -> Result<usize, String> {
    if !success {
        return Err(format!("cargo test exited nonzero\n{stdout}\n{stderr}"));
    }
    let text = format!("{stdout}\n{stderr}");
    if text.contains("test result: FAILED") {
        return Err("cargo reported test result FAILED".to_owned());
    }
    let mut passed = 0usize;
    let mut saw_ok = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("test result: ok.") {
            saw_ok = true;
            let count = rest
                .split_whitespace()
                .next()
                .unwrap_or("0")
                .parse::<usize>()
                .unwrap_or(0);
            passed += count;
        }
    }
    if !saw_ok || passed == 0 {
        return Err(
            "cargo exited 0 without running a test. A missing or nonsensical --exact name is not a pass."
                .to_owned(),
        );
    }
    Ok(passed)
}

fn self_test_at(repo: &Path) -> Report {
    let mut issues = Vec::new();
    let generated = match oo_core::schema_export::contract_documents() {
        Ok(docs) => docs,
        Err(error) => {
            return Report::fail(
                "contracts",
                "failed",
                EXIT_FAIL,
                vec![Issue::new("contract.export", "oo-core", error)],
                json!({}),
            );
        }
    };
    let temp = std::env::temp_dir().join(format!("oo-xtask-contracts-{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp);
    let schema_dir = temp.join("contracts/schema");
    if fs::create_dir_all(&schema_dir).is_err() {
        return Report::fail(
            "contracts",
            "failed",
            EXIT_FAIL,
            vec![Issue::new(
                "contract.self_test",
                "temp",
                "cannot create temp root",
            )],
            json!({}),
        );
    }
    for (name, value) in &generated {
        let pretty = oo_core::schema_export::to_pretty(value);
        if fs::write(schema_dir.join(name), &pretty).is_err() {
            issues.push(Issue::new(
                "contract.self_test",
                *name,
                "cannot write temp schema",
            ));
        }
    }
    let before = hash_tree(repo, &["contracts/schema/api-v1.json"]);
    let control = library_issues_schemas_only(&temp, &generated);
    if !control.is_empty() {
        issues.push(Issue::new(
            "contract.self_test.control",
            "temp",
            format!("unmodified generated schemas were rejected: {control:?}"),
        ));
    }
    let target = schema_dir.join("api-v1.json");
    if let Ok(mut bytes) = fs::read(&target) {
        if let Some(byte) = bytes.last_mut() {
            *byte = byte.wrapping_add(1);
        }
        let _ = fs::write(&target, bytes);
        let tampered = library_issues_schemas_only(&temp, &generated);
        if !tampered
            .iter()
            .any(|issue| issue.code == "contract.schema.drift")
        {
            issues.push(Issue::new(
                "contract.self_test.tamper",
                "contracts/schema/api-v1.json",
                "byte tamper was not reported as drift",
            ));
        }
    }
    let after = hash_tree(repo, &["contracts/schema/api-v1.json"]);
    if before != after {
        issues.push(Issue::new(
            "contract.self_test.shared_mutation",
            "contracts/schema/api-v1.json",
            "self-test changed the shared schema",
        ));
    }
    let ignored = accept_cargo_test(
        true,
        "running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n",
        "",
    );
    if ignored.is_ok() {
        issues.push(Issue::new(
            "contract.self_test.zero_tests",
            "cargo",
            "a zero-test cargo result was accepted",
        ));
    }
    let failed_exit = accept_cargo_test(false, "test result: ok. 3 passed; 0 failed\n", "");
    if failed_exit.is_ok() {
        issues.push(Issue::new(
            "contract.self_test.exit",
            "cargo",
            "nonzero cargo exit was ignored",
        ));
    }
    let _ = fs::remove_dir_all(&temp);
    Report::from_issues(
        "contracts",
        "self_test_pass",
        "failed",
        EXIT_FAIL,
        issues,
        json!({"shared_schema_unchanged": before == after}),
    )
}

fn library_issues_schemas_only(root: &Path, generated: &[(&str, Value)]) -> Vec<Issue> {
    let mut issues = Vec::new();
    for (name, value) in generated {
        let relative = format!("contracts/schema/{name}");
        let pretty = oo_core::schema_export::to_pretty(value);
        match read_no_symlink(root, &relative) {
            Ok(bytes) => {
                if String::from_utf8_lossy(&bytes).as_ref() != pretty {
                    issues.push(Issue::new(
                        "contract.schema.drift",
                        relative,
                        "bytes differ",
                    ));
                }
            }
            Err(issue) => issues.push(issue),
        }
    }
    issues
}

fn hash_tree(root: &Path, files: &[&str]) -> String {
    let mut acc = String::new();
    for file in files {
        match read_no_symlink(root, file) {
            Ok(bytes) => acc.push_str(&sha256_hex(&bytes)),
            Err(_) => acc.push_str("missing"),
        }
    }
    acc
}
