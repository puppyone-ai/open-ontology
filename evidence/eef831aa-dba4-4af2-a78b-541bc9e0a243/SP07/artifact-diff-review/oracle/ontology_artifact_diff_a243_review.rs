//! Independent CLI oracle for the a243 review of ontology-artifact-diff.
//!
//! Catalogs are a ledger fixture, not the shop fixture. `oo-builder
//! captured-pg-draft` and `oo-builder review-pg-draft` compile them. This
//! file does not call `compile_captured_pg_catalog`. The schema preimage is
//! locked to the published empty and one-column vectors before any compile.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{Map, Value, json};

use oo_core::api::payload_sha256;

const EMPTY_SCHEMA: &str = "cb2b03b89678ec1fb9db96f116e8e8ad81a81f6b9bbc2594bcb7d941e5234ecc";
const ONE_COLUMN_SCHEMA: &str = "60f7939195cc539644d11132ae4a604c3b025167f1fe657d121167feb8463499";
const ONE_COLUMN_PREIMAGE: &str = r#"{"tables":[{"schema":"public","name":"customer","relkind":"r","replica_identity":"d","rls":false,"force_rls":false,"columns":[{"attnum":1,"name":"id","type_oid":23,"typmod":-1,"not_null":true,"collation_oid":0,"typtype":"b","base_type_oid":null,"identity":"","generated":"","default_sha256":null,"enum_labels":[],"domain_checks":[]}],"primary_key":[{"attnum":1,"name":"id"}],"primary_key_nulls_not_distinct":false,"unique_keys":[],"foreign_keys":[],"policies":[]}]}"#;
const COMMENT: &str = "ledger-comment-do-not-carry-a243";
const OWNER: &str = "ledger_owner_a243";
const CURATED_BY: &str = "secret-curator-name-a243";
const CURATED_LABEL: &str = "do-not-echo-label-a243";
const SECRET_PROTOCOL: &str = "secret-protocol-do-not-echo-a243";
const PATH_SENTINEL: &str = "secret-path-do-not-echo-a243";
const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const CAPTURED_AT: &str = "2026-10-06T08:30:00.000000Z";
const COMMAND_PROTOCOL: &str = "oo.builder.captured-pg-draft-command/v1";
const USAGE: &str = "\
ontology-artifact-diff: usage: ontology-artifact-diff --before <file> --after <file>\n\
ontology-artifact-diff: reads two oo.builder.captured-pg-draft-command/v1 artifacts\n\
ontology-artifact-diff: writes one oo.builder.artifact-diff/v1 structural diff\n\
ontology-artifact-diff: does not connect, call a model, import rows, accept, grant, certify, or publish\n";

const BUILDER: &str = env!("CARGO_BIN_EXE_oo-builder");
const DIFF_BIN: &str = env!("CARGO_BIN_EXE_ontology-artifact-diff");

struct Col {
    attnum: i32,
    name: &'static str,
    type_oid: i64,
    type_name: &'static str,
    not_null: bool,
    comment: bool,
}

struct FkCol {
    attnum: i32,
    name: &'static str,
    ref_attnum: i32,
    ref_name: &'static str,
}

struct Spec {
    code_name: &'static str,
    code_oid: i64,
    code_type: &'static str,
    code_not_null: bool,
    full_fk: bool,
    entry_first: bool,
}

impl Spec {
    fn base() -> Self {
        Self {
            code_name: "code",
            code_oid: 25,
            code_type: "text",
            code_not_null: false,
            full_fk: true,
            entry_first: false,
        }
    }
}

#[test]
fn preimage_locks_the_published_vectors() {
    assert_eq!(preimage(&[]), r#"{"tables":[]}"#);
    assert_eq!(payload_sha256(br#"{"tables":[]}"#), EMPTY_SCHEMA);
    let customer = [table(
        "customer",
        vec![Col {
            attnum: 1,
            name: "id",
            type_oid: 23,
            type_name: "int4",
            not_null: true,
            comment: false,
        }],
        vec![(1, "id")],
        Vec::new(),
    )];
    assert_eq!(preimage(&customer), ONE_COLUMN_PREIMAGE);
    assert_eq!(
        payload_sha256(ONE_COLUMN_PREIMAGE.as_bytes()),
        ONE_COLUMN_SCHEMA
    );
    let forward = schema_hash(&Spec::base());
    let mut reversed = Spec::base();
    reversed.entry_first = true;
    assert_eq!(schema_hash(&reversed), forward);
    assert_ne!(schema_hash(&code_renamed()), forward);
    assert_ne!(schema_hash(&code_varchar()), forward);
    assert_ne!(schema_hash(&short_fk()), forward);
    assert_ne!(schema_hash(&code_not_null()), forward);
}

#[test]
fn cli_identical_ledger_is_sealed_and_omits_secrets() {
    let forward = capture(&Spec::base());
    let mut reversed_spec = Spec::base();
    reversed_spec.entry_first = true;
    let reversed = capture(&reversed_spec);
    assert_eq!(forward, reversed);
    assert!(!forward["draft"].to_string().contains(COMMENT));
    assert!(!forward["wrapper"].to_string().contains(OWNER));
    let dir = scratch("identical");
    let path = write_json(&dir, "ledger.json", &forward);
    let pretty = write_text(
        &dir,
        "pretty.json",
        &serde_json::to_string_pretty(&forward).unwrap(),
    );
    let reordered = write_text(&dir, "reordered.json", &reordered_command(&forward));
    let (first, doc) = ok(&path, &path);
    let (second, _) = ok(&path, &path);
    let (from_pretty, _) = ok(&pretty, &path);
    let (from_reordered, _) = ok(&reordered, &path);
    assert_eq!(first, second);
    assert_eq!(first, from_pretty);
    assert_eq!(first, from_reordered);
    assert_header(&doc, true);
    assert_eq!(doc["facts"].as_array().unwrap().len(), 0);
    assert!(doc["counts"]["unchanged"].as_u64().unwrap() > 0);
    assert_sorted(&doc);
    assert_counts(&doc);
    assert_sealed(&first, &doc);
    assert_eq!(doc["before"]["source_id"], "ledger_src");
    assert_eq!(doc["before"]["database"], "ledger");
    assert_eq!(doc["before"]["schema_hash"], doc["after"]["schema_hash"]);
    assert_quiet(&first, "");
}

#[test]
fn cli_rename_type_and_fk_keep_identity_rules() {
    let before = capture(&Spec::base());
    let renamed = diff_specs(&before, &capture(&code_renamed()), "rename");
    assert_header(&renamed, false);
    assert_sorted(&renamed);
    assert_counts(&renamed);
    fact(
        &renamed,
        "removed",
        "column",
        "column/public.account/code",
        "physical",
    );
    fact(
        &renamed,
        "added",
        "column",
        "column/public.account/reference_code",
        "physical",
    );
    fact(
        &renamed,
        "removed",
        "column",
        "column/public.account/code",
        "generated",
    );
    fact(
        &renamed,
        "added",
        "column",
        "column/public.account/reference_code",
        "generated",
    );
    fact(
        &renamed,
        "stale",
        "column",
        "column/public.account/id",
        "physical",
    );
    fact(&renamed, "changed", "source", "source/ledger_src/ledger", "physical");
    assert!(
        renamed["facts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["kind"] != "classification")
    );

    let typed = diff_specs(&before, &capture(&code_varchar()), "type");
    let physical = fact(
        &typed,
        "changed",
        "column",
        "column/public.account/code",
        "physical",
    );
    assert_eq!(physical["before"]["type_oid"], json!(25));
    assert_eq!(physical["after"]["type_oid"], json!(1043));
    assert_eq!(physical["before"]["type_name"], "text");
    assert_eq!(physical["after"]["type_name"], "varchar");
    assert_eq!(
        physical["before"]["type_reason"],
        "known_builtin_not_converted"
    );
    assert_eq!(
        physical["after"]["type_reason"],
        "known_builtin_not_converted"
    );
    let generated = fact(
        &typed,
        "changed",
        "column",
        "column/public.account/code",
        "generated",
    );
    assert_ne!(generated["before"]["data_type"], generated["after"]["data_type"]);

    let shrunk = diff_specs(&before, &capture(&short_fk()), "fk");
    let foreign = fact(
        &shrunk,
        "changed",
        "foreign_key",
        "foreign_key/public.entry/entry_account_fk",
        "physical",
    );
    assert_eq!(foreign["before"]["columns"].as_array().unwrap().len(), 2);
    assert_eq!(foreign["after"]["columns"].as_array().unwrap().len(), 1);
    fact(
        &shrunk,
        "changed",
        "foreign_key",
        "foreign_key/public.entry/entry_account_fk",
        "generated",
    );
    fact(
        &shrunk,
        "stale",
        "column",
        "column/public.entry/id",
        "physical",
    );
}

#[test]
fn cli_curated_classification_and_stale_decision_stay_unaccepted() {
    let before = capture(&Spec::base());
    let before_hash = before["draft"]["provenance"]["schema_hash"]
        .as_str()
        .unwrap()
        .to_string();
    let fingerprint = structural_fingerprint(&before);
    let after_spec = code_not_null();
    let catalog = catalog_json(&after_spec);
    let curated = json!({
        "datasets": [{
            "dataset": "public.account",
            "columns": [{
                "name": "code",
                "label": CURATED_LABEL,
                "classification": {"state": "confirmed", "by": CURATED_BY}
            }]
        }]
    });
    let reviews = json!([{
        "item_id": "column/public.account/code",
        "action": "accept",
        "schema_hash": before_hash,
        "fact_fingerprint": fingerprint,
    }]);
    let after = review(&catalog, &curated, &reviews);
    assert_ne!(
        after["draft"]["provenance"]["schema_hash"].as_str().unwrap(),
        before_hash
    );
    let dir = scratch("curated");
    let before_path = write_json(&dir, "before.json", &before);
    let after_path = write_json(&dir, "after.json", &after);
    let (stdout, doc) = ok(&before_path, &after_path);
    assert_header(&doc, false);
    assert_sorted(&doc);
    assert_counts(&doc);
    assert_sealed(&stdout, &doc);
    let column = fact(
        &doc,
        "changed",
        "column",
        "column/public.account/code",
        "physical",
    );
    assert_eq!(column["before"]["not_null"], json!(false));
    assert_eq!(column["after"]["not_null"], json!(true));
    fact(
        &doc,
        "stale",
        "column",
        "column/public.account/code",
        "generated",
    );
    let class = fact(
        &doc,
        "added",
        "classification",
        "classification/public.account/code",
        "curated",
    );
    assert!(class["before"].is_null());
    assert_eq!(class["after"]["unknown"], json!(false));
    assert_eq!(class["after"]["state"], "confirmed");
    assert!(class["after"].get("by").is_none());
    let class_bytes = canonical_json(&json!({"by": CURATED_BY, "state": "confirmed"}));
    assert_eq!(
        class["after"]["value_sha256"],
        payload_sha256(class_bytes.as_bytes())
    );
    let decision = fact(
        &doc,
        "stale",
        "review",
        "column/public.account/code",
        "decision",
    );
    assert!(decision["before"].is_null());
    assert_eq!(decision["after"]["action"], "accept");
    assert_eq!(decision["after"]["schema_hash"], before_hash);
    let queued = fact(
        &doc,
        "stale",
        "review",
        "column/public.account/code",
        "queue",
    );
    assert_eq!(queued["after"]["kind"], "stale_decision");
    assert_eq!(queued["after"]["detail"], "stale_schema_hash");
    assert_eq!(queued["after"]["publishable"], json!(false));
    assert!(
        doc["facts"].as_array().unwrap().iter().all(|item| {
            !(item["kind"] == "classification" && item["layer"] == "generated")
        })
    );
    assert_quiet(&stdout, "");
    assert!(
        !stdout.contains(CURATED_LABEL),
        "curated column label was copied into the diff"
    );
    assert!(class.get("label").is_none());
}

#[test]
fn cli_tamper_and_usage_refuse_without_echo() {
    let good_doc = capture(&Spec::base());
    let dir = scratch("tamper");
    let good = write_json(&dir, "good.json", &good_doc);
    let usage = run_diff(&[]);
    assert_eq!(usage.status.code(), Some(2));
    assert!(usage.stdout.is_empty());
    assert_eq!(std_text(&usage.stderr), USAGE);
    let missing = dir.join("secret-missing-do-not-echo-a243.json");
    assert_refuse(
        &run_paths(&missing, &good),
        "before",
        "io.open",
        "file could not be opened",
    );
    let folder = dir.join(PATH_SENTINEL);
    fs::create_dir(&folder).unwrap();
    assert_refuse(
        &run_paths(&folder, &good),
        "before",
        "io.regular",
        "path is not a regular file",
    );
    let link = dir.join("secret-link-do-not-echo-a243.json");
    std::os::unix::fs::symlink(&good, &link).unwrap();
    assert_refuse(
        &run_paths(&good, &link),
        "after",
        "io.regular",
        "path is not a regular file",
    );
    let duplicate = write_text(&dir, "dup.json", "{\"draft\":{},\"draft\":{}}");
    assert_refuse(
        &run_paths(&duplicate, &good),
        "before",
        "json.duplicate_key",
        "duplicate JSON object key",
    );
    let mut root = good_doc.clone();
    root["protocol"] = json!(SECRET_PROTOCOL);
    let version = write_json(&dir, "version.json", &root);
    let version_out = run_paths(&version, &good);
    assert_refuse(
        &version_out,
        "before",
        "version.unsupported",
        "artifact version is not the frozen captured-pg-draft set",
    );
    root["protocol"] = json!(COMMAND_PROTOCOL);
    root["wrapper"]["draft_hash"] = json!(ZERO_HASH);
    let hashed = write_json(&dir, "hash.json", &root);
    let hash_out = run_paths(&good, &hashed);
    assert_refuse(
        &hash_out,
        "after",
        "hash.mismatch",
        "artifact hash does not match the canonical bytes",
    );
    assert!(!std_text(&hash_out.stderr).contains(ZERO_HASH));
    root["wrapper"]["draft_hash"] = good_doc["wrapper"]["draft_hash"].clone();
    root.as_object_mut()
        .unwrap()
        .insert("secret-field-do-not-echo-a243".to_string(), json!(1));
    let unknown = write_json(&dir, "unknown.json", &root);
    assert_refuse(
        &run_paths(&unknown, &good),
        "before",
        "field.unknown",
        "artifact has an unknown field",
    );
}

fn code_renamed() -> Spec {
    let mut spec = Spec::base();
    spec.code_name = "reference_code";
    spec
}

fn code_varchar() -> Spec {
    let mut spec = Spec::base();
    spec.code_oid = 1043;
    spec.code_type = "varchar";
    spec
}

fn short_fk() -> Spec {
    let mut spec = Spec::base();
    spec.full_fk = false;
    spec
}

fn code_not_null() -> Spec {
    let mut spec = Spec::base();
    spec.code_not_null = true;
    spec
}

fn capture(spec: &Spec) -> Value {
    let text = serde_json::to_string(&catalog_json(spec)).unwrap();
    let output = run_stdin(BUILDER, &["captured-pg-draft"], &text);
    assert_eq!(
        output.status.code(),
        Some(0),
        "captured-pg-draft {}",
        std_text(&output.stderr)
    );
    parse_stdout(&output)
}

fn review(catalog: &Value, curated: &Value, reviews: &Value) -> Value {
    let text = serde_json::to_string(&json!({
        "catalog": catalog,
        "curated": curated,
        "reviews": reviews,
    }))
    .unwrap();
    let output = run_stdin(BUILDER, &["review-pg-draft"], &text);
    assert_eq!(
        output.status.code(),
        Some(0),
        "review-pg-draft {}",
        std_text(&output.stderr)
    );
    parse_stdout(&output)
}

fn diff_specs(before: &Value, after: &Value, name: &str) -> Value {
    let dir = scratch(name);
    let before_path = write_json(&dir, "before.json", before);
    let after_path = write_json(&dir, "after.json", after);
    let forward = run_diff(&[
        "--before",
        before_path.to_str().unwrap(),
        "--after",
        after_path.to_str().unwrap(),
    ]);
    let reverse = run_diff(&[
        "--after",
        after_path.to_str().unwrap(),
        "--before",
        before_path.to_str().unwrap(),
    ]);
    assert_eq!(forward.status.code(), Some(0), "{}", std_text(&forward.stderr));
    assert_eq!(forward.stdout, reverse.stdout);
    let stdout = std_text(&forward.stdout);
    let doc: Value = serde_json::from_str(&stdout).unwrap();
    assert_sealed(&stdout, &doc);
    assert_quiet(&stdout, &std_text(&forward.stderr));
    doc
}

fn ok(before: &Path, after: &Path) -> (String, Value) {
    let output = run_paths(before, after);
    assert_eq!(output.status.code(), Some(0), "{}", std_text(&output.stderr));
    let stdout = std_text(&output.stdout);
    let doc: Value = serde_json::from_str(&stdout).unwrap();
    assert_quiet(&stdout, &std_text(&output.stderr));
    (stdout, doc)
}

fn run_paths(before: &Path, after: &Path) -> Output {
    run_diff(&[
        "--before",
        before.to_str().unwrap(),
        "--after",
        after.to_str().unwrap(),
    ])
}

fn run_diff(args: &[&str]) -> Output {
    Command::new(DIFF_BIN)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap_or_else(|err| panic!("spawn diff: {err}"))
}

fn run_stdin(bin: &str, args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {bin}: {err}"));
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn parse_stdout(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "stdout is not JSON: {err}; stderr {}",
            std_text(&output.stderr)
        )
    })
}

fn assert_refuse(output: &Output, side: &str, code: &str, message: &str) {
    assert_eq!(output.status.code(), Some(1), "{}", std_text(&output.stderr));
    assert!(output.stdout.is_empty());
    let expected = format!(
        "ontology-artifact-diff: {side}: oo.builder.artifact-diff/v1/{code} {message}\n"
    );
    assert_eq!(std_text(&output.stderr), expected);
    assert_quiet("", &std_text(&output.stderr));
}

fn assert_header(doc: &Value, equal: bool) {
    assert_eq!(doc["protocol"], "oo.builder.artifact-diff/v1");
    assert_eq!(doc["compiler"], "ontology-artifact-diff/v1");
    assert_eq!(doc["auto_accept"], json!(false));
    assert_eq!(doc["publication"], "unavailable");
    assert_eq!(doc["certification"], "none");
    assert_eq!(doc["grants"], "not_authorized");
    assert_eq!(doc["schema_hash_equal"], json!(equal));
}

fn assert_counts(doc: &Value) {
    let facts = doc["facts"].as_array().unwrap();
    for status in ["added", "changed", "removed", "stale"] {
        let seen = facts
            .iter()
            .filter(|item| item["status"] == status)
            .count() as u64;
        assert_eq!(doc["counts"][status].as_u64().unwrap(), seen, "{status}");
    }
}

fn assert_sorted(doc: &Value) {
    let facts = doc["facts"].as_array().unwrap();
    let keys: Vec<_> = facts
        .iter()
        .map(|item| {
            (
                item["status"].as_str().unwrap().to_string(),
                item["kind"].as_str().unwrap().to_string(),
                item["id"].as_str().unwrap().to_string(),
                item["layer"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let mut ordered = keys.clone();
    ordered.sort();
    assert_eq!(keys, ordered, "{facts:?}");
}

fn assert_sealed(stdout: &str, doc: &Value) {
    assert!(!stdout.ends_with('\n'));
    assert!(!stdout.is_empty());
    assert_eq!(stdout, canonical_json(doc));
    let mut bare = doc.clone();
    assert!(bare.as_object_mut().unwrap().remove("diff_hash").is_some());
    assert_eq!(
        doc["diff_hash"],
        payload_sha256(canonical_json(&bare).as_bytes())
    );
}

fn assert_quiet(stdout: &str, stderr: &str) {
    for secret in [
        COMMENT,
        OWNER,
        CURATED_BY,
        CURATED_LABEL,
        SECRET_PROTOCOL,
        PATH_SENTINEL,
        "secret-missing-do-not-echo-a243",
        "secret-link-do-not-echo-a243",
        "secret-field-do-not-echo-a243",
    ] {
        assert!(!stdout.contains(secret), "{secret} leaked to stdout");
        assert!(!stderr.contains(secret), "{secret} leaked to stderr");
    }
}

fn fact<'a>(doc: &'a Value, status: &str, kind: &str, id: &str, layer: &str) -> &'a Value {
    let facts = doc["facts"].as_array().unwrap();
    let found: Vec<_> = facts
        .iter()
        .filter(|item| item["id"] == id && item["layer"] == layer)
        .collect();
    assert_eq!(
        found.len(),
        1,
        "missing one {id} {layer}; facts={facts:?}"
    );
    assert_eq!(found[0]["status"], status, "{id}");
    assert_eq!(found[0]["kind"], kind, "{id}");
    found[0]
}

fn structural_fingerprint(command: &Value) -> String {
    command["draft"]["review_queue"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| {
            item["item_id"] == "column/public.account/code" && item["kind"] == "structural_column"
        })
        .unwrap_or_else(|| panic!("queue {}", command["draft"]["review_queue"]))["fact_fingerprint"]
        .as_str()
        .unwrap()
        .to_string()
}

fn schema_hash(spec: &Spec) -> String {
    payload_sha256(preimage(&tables(spec)).as_bytes())
}

fn catalog_json(spec: &Spec) -> Value {
    let tables = tables(spec);
    let hash = payload_sha256(preimage(&tables).as_bytes());
    let mut rendered = Vec::new();
    for table in &tables {
        rendered.push(table_json(table));
    }
    if spec.entry_first {
        rendered.reverse();
    }
    json!({
        "protocol": "oo.source.pgsql/v1",
        "core_contract": "oo.config/v1",
        "source_id": "ledger_src",
        "database": "ledger",
        "captured_at": CAPTURED_AT,
        "schema_hash": hash,
        "hash_algorithm": "pg-catalog-hash/v1",
        "ident_algorithm": "pgident/v1",
        "tables": rendered,
        "type_conversion": "unsupported"
    })
}

fn tables(spec: &Spec) -> Vec<Table> {
    let account_columns = vec![
        Col {
            attnum: 1,
            name: "tenant_id",
            type_oid: 23,
            type_name: "int4",
            not_null: true,
            comment: false,
        },
        Col {
            attnum: 2,
            name: "id",
            type_oid: 23,
            type_name: "int4",
            not_null: true,
            comment: false,
        },
        Col {
            attnum: 3,
            name: spec.code_name,
            type_oid: spec.code_oid,
            type_name: spec.code_type,
            not_null: spec.code_not_null,
            comment: true,
        },
    ];
    let mut fk = vec![FkCol {
        attnum: 3,
        name: "account_id",
        ref_attnum: 2,
        ref_name: "id",
    }];
    if spec.full_fk {
        fk.insert(
            0,
            FkCol {
                attnum: 1,
                name: "tenant_id",
                ref_attnum: 1,
                ref_name: "tenant_id",
            },
        );
    }
    vec![
        table(
            "account",
            account_columns,
            vec![(1, "tenant_id"), (2, "id")],
            Vec::new(),
        ),
        table(
            "entry",
            vec![
                Col {
                    attnum: 1,
                    name: "tenant_id",
                    type_oid: 23,
                    type_name: "int4",
                    not_null: true,
                    comment: false,
                },
                Col {
                    attnum: 2,
                    name: "id",
                    type_oid: 23,
                    type_name: "int4",
                    not_null: true,
                    comment: false,
                },
                Col {
                    attnum: 3,
                    name: "account_id",
                    type_oid: 23,
                    type_name: "int4",
                    not_null: true,
                    comment: false,
                },
            ],
            vec![(1, "tenant_id"), (2, "id")],
            vec![Fk {
                name: "entry_account_fk",
                columns: fk,
            }],
        ),
    ]
}

struct Table {
    name: &'static str,
    columns: Vec<Col>,
    primary_key: Vec<(i32, &'static str)>,
    foreign_keys: Vec<Fk>,
}

struct Fk {
    name: &'static str,
    columns: Vec<FkCol>,
}

fn table(
    name: &'static str,
    columns: Vec<Col>,
    primary_key: Vec<(i32, &'static str)>,
    foreign_keys: Vec<Fk>,
) -> Table {
    Table {
        name,
        columns,
        primary_key,
        foreign_keys,
    }
}

fn table_json(table: &Table) -> Value {
    let columns: Vec<_> = table
        .columns
        .iter()
        .map(|column| {
            json!({
                "attnum": column.attnum,
                "name": column.name,
                "name_ident": column.name,
                "type_oid": column.type_oid,
                "type_schema": "pg_catalog",
                "type_name": column.type_name,
                "typtype": "b",
                "typmod": -1,
                "base_type_oid": Value::Null,
                "numeric_precision": Value::Null,
                "numeric_scale": Value::Null,
                "not_null": column.not_null,
                "collation_oid": 0,
                "collation_name": Value::Null,
                "identity": "",
                "generated": "",
                "default_expr": Value::Null,
                "comment": if column.comment {
                    json!({"byte_len": COMMENT.len(), "sha256": payload_sha256(COMMENT.as_bytes())})
                } else {
                    Value::Null
                },
                "enum_labels": [],
                "domain_checks": [],
                "type_support": {"converted": false, "reason": "known_builtin_not_converted"},
                "current_privileges": {"select": true, "insert": false, "update": false}
            })
        })
        .collect();
    let primary_key: Vec<_> = table
        .primary_key
        .iter()
        .map(|(attnum, name)| json!({"attnum": attnum, "name": name}))
        .collect();
    let foreign_keys: Vec<_> = table
        .foreign_keys
        .iter()
        .map(|key| {
            let columns: Vec<_> = key
                .columns
                .iter()
                .map(|column| {
                    json!({
                        "attnum": column.attnum,
                        "name": column.name,
                        "ref_schema": "public",
                        "ref_table": "account",
                        "ref_attnum": column.ref_attnum,
                        "ref_name": column.ref_name
                    })
                })
                .collect();
            json!({"name": key.name, "columns": columns})
        })
        .collect();
    json!({
        "schema": "public",
        "schema_ident": "public",
        "name": table.name,
        "name_ident": table.name,
        "relkind": "r",
        "replica_identity": "d",
        "rls_enabled": false,
        "rls_forced": false,
        "owner": OWNER,
        "comment": Value::Null,
        "columns": columns,
        "primary_key": primary_key,
        "primary_key_nulls_not_distinct": false,
        "unique_keys": [],
        "foreign_keys": foreign_keys,
        "policies": [],
        "privileges": [
            {"grantee": OWNER, "privilege": "INSERT", "grantable": true},
            {"grantee": "reader", "privilege": "SELECT", "grantable": false}
        ]
    })
}

fn preimage(tables: &[Table]) -> String {
    let mut ordered: Vec<&Table> = tables.iter().collect();
    ordered.sort_by(|left, right| left.name.cmp(right.name));
    let mut buf = String::from("{\"tables\":[");
    for (index, table) in ordered.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        write_table(&mut buf, table);
    }
    buf.push_str("]}");
    buf
}

fn write_table(buf: &mut String, table: &Table) {
    buf.push('{');
    write_str(buf, "schema", "public");
    buf.push(',');
    write_str(buf, "name", table.name);
    buf.push_str(",\"relkind\":\"r\",\"replica_identity\":\"d\",\"rls\":false,\"force_rls\":false,\"columns\":[");
    let mut columns = table.columns.iter().collect::<Vec<_>>();
    columns.sort_by_key(|column| column.attnum);
    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        write_column(buf, column);
    }
    buf.push_str("],\"primary_key\":[");
    for (index, (attnum, name)) in table.primary_key.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push('{');
        write_i32(buf, "attnum", *attnum);
        buf.push(',');
        write_str(buf, "name", name);
        buf.push('}');
    }
    buf.push_str("],\"primary_key_nulls_not_distinct\":false,\"unique_keys\":[],\"foreign_keys\":[");
    for (index, key) in table.foreign_keys.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push('{');
        write_str(buf, "name", key.name);
        buf.push_str(",\"columns\":[");
        for (col_index, column) in key.columns.iter().enumerate() {
            if col_index > 0 {
                buf.push(',');
            }
            buf.push('{');
            write_i32(buf, "attnum", column.attnum);
            buf.push(',');
            write_str(buf, "name", column.name);
            buf.push(',');
            write_str(buf, "ref_schema", "public");
            buf.push(',');
            write_str(buf, "ref_table", "account");
            buf.push(',');
            write_i32(buf, "ref_attnum", column.ref_attnum);
            buf.push(',');
            write_str(buf, "ref_name", column.ref_name);
            buf.push('}');
        }
        buf.push_str("]}");
    }
    buf.push_str("],\"policies\":[]}");
}

fn write_column(buf: &mut String, column: &Col) {
    buf.push('{');
    write_i32(buf, "attnum", column.attnum);
    buf.push(',');
    write_str(buf, "name", column.name);
    buf.push(',');
    write_i64(buf, "type_oid", column.type_oid);
    buf.push_str(",\"typmod\":-1,");
    write_bool(buf, "not_null", column.not_null);
    buf.push_str(",\"collation_oid\":0,\"typtype\":\"b\",\"base_type_oid\":null,\"identity\":\"\",\"generated\":\"\",\"default_sha256\":null,\"enum_labels\":[],\"domain_checks\":[]}");
}

fn write_str(buf: &mut String, name: &str, value: &str) {
    push_raw(buf, name);
    buf.push(':');
    push_raw(buf, value);
}

fn write_bool(buf: &mut String, name: &str, value: bool) {
    push_raw(buf, name);
    buf.push(':');
    buf.push_str(if value { "true" } else { "false" });
}

fn write_i32(buf: &mut String, name: &str, value: i32) {
    push_raw(buf, name);
    buf.push(':');
    buf.push_str(&value.to_string());
}

fn write_i64(buf: &mut String, name: &str, value: i64) {
    push_raw(buf, name);
    buf.push(':');
    buf.push_str(&value.to_string());
}

fn push_raw(buf: &mut String, value: &str) {
    buf.push('"');
    buf.push_str(value);
    buf.push('"');
}

fn reordered_command(value: &Value) -> String {
    let mut out = String::from("{\"wrapper\":");
    out.push_str(&canonical_json(&value["wrapper"]));
    out.push_str(",\"protocol\":");
    out.push_str(&canonical_json(&value["protocol"]));
    out.push_str(",\"evidence_envelope\":");
    out.push_str(&canonical_json(&value["evidence_envelope"]));
    out.push_str(",\"draft\":");
    out.push_str(&canonical_json(&value["draft"]));
    out.push('}');
    out
}

fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => push_string(out, text),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => write_object(out, map),
    }
}

fn write_object(out: &mut String, map: &Map<String, Value>) {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    out.push('{');
    for (index, key) in keys.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        push_string(out, key);
        out.push(':');
        write_value(out, &map[*key]);
    }
    out.push('}');
}

fn push_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", u32::from(ch)));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("oo-a243-artifact-diff-{name}"));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn write_json(dir: &Path, name: &str, value: &Value) -> PathBuf {
    write_text(dir, name, &serde_json::to_string(value).unwrap())
}

fn write_text(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, text).unwrap();
    path
}

fn std_text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).unwrap_or_else(|err| panic!("non-utf8 output: {err}"))
}
