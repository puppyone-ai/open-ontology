//! Capability journey. Expected tokens are the handwritten fixture.
//! The catalog is compiled by oo-builder, and the pack page is parsed again
//! with `compile_role_pack`. This file does not open a database.

mod common;

use std::process::{Command, Stdio};

use serde_json::{Value, json};

use common::{OWNER, binding, catalog, column, command, fk, key_col, table};
use source_semantics::compile_source_semantics;

fn expected() -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/oracle-expected.json");
    let text = std::fs::read_to_string(path).expect("expected fixture");
    serde_json::from_str(&text).expect("expected json")
}

fn run(value: &Value) -> Result<Value, String> {
    let text = serde_json::to_string(value).expect("command");
    match compile_source_semantics(&text) {
        Ok(out) => Ok(serde_json::from_str(&out).expect("result json")),
        Err(err) => Err(format!("{} {} {}", err.code, err.path, err.message)),
    }
}

fn base_target(product: &str, family: &str) -> Value {
    json!({
        "family": family,
        "product": product,
        "version": "19c",
        "capture": "batch_full",
        "source_consistency": "B",
        "delete_detection": "reconcile",
        "source_kind": "oracle_table",
        "cursor": null,
        "chunk": {"key_kind": "integer", "binary_order": null, "hash_chunking": false},
        "flags": {
            "synthetic_binlog": false,
            "append_only": false,
            "eventual_consistency": false,
            "default_for_missing": false,
            "accent_insensitive": false,
            "soft_delete_hint": false
        },
        "columns": [],
        "constraints": []
    })
}

fn oracle_command() -> Value {
    let party = table("party", vec![column(1, "id", 23, "int4")], vec![key_col(1, "id")], vec![], vec![]);
    let account = table("account", vec![column(1, "id", 23, "int4")], vec![key_col(1, "id")], vec![], vec![]);
    let customer = table(
        "customer",
        vec![
            column(1, "id", 23, "int4"),
            column(2, "bill_to", 23, "int4"),
            column(3, "account_id", 23, "int4"),
            column(4, "name", 25, "text"),
            column(5, "code", 1042, "bpchar"),
            column(6, "opened", 1082, "date"),
            column(7, "amount", 1700, "numeric"),
            column(8, "touched", 1114, "timestamp"),
        ],
        vec![key_col(1, "id")],
        vec![],
        vec![
            fk("customer_account_fk", 3, "account_id", "account", 1, "id"),
            fk("customer_party_fk", 2, "bill_to", "party", 1, "id"),
        ],
    );
    let sealed = catalog(vec![account, customer, party]);
    let mut target = base_target("oracle", "oracle");
    target["source_kind"] = json!("oracle_table");
    target["columns"] = json!([
        {"schema": "public", "table": "customer", "name": "name", "native_type": "VARCHAR2", "empty_string_is_null": true, "collation": "linguistic_ci"},
        {"schema": "public", "table": "customer", "name": "code", "native_type": "CHAR", "empty_string_is_null": true, "trailing_space_insensitive": true},
        {"schema": "public", "table": "customer", "name": "opened", "native_type": "DATE"},
        {"schema": "public", "table": "customer", "name": "amount", "native_type": "NUMBER", "unconstrained_number": true},
        {"schema": "public", "table": "customer", "name": "touched", "native_type": "TIMESTAMP(9)", "fractional_seconds": 9}
    ]);
    target["constraints"] = json!([
        {"schema": "public", "table": "customer", "name": "customer_pkey", "kind": "primary_key", "columns": ["id"], "trust": "enforced"},
        {"schema": "public", "table": "customer", "name": "customer_account_fk", "kind": "foreign_key", "columns": ["account_id"], "trust": "enforced"},
        {"schema": "public", "table": "customer", "name": "customer_party_fk", "kind": "foreign_key", "columns": ["bill_to"], "trust": "declared_only", "inclusion": {"rows_checked": 1000, "ratio_thousandths": 980}}
    ]);
    let examples = json!([
        {"id": "empty-name", "question": "rows whose name is null", "sql": "select id from public.customer where name is null", "uses": ["dataset:public.customer"], "verified_by": "pg-gateway", "verified_at": "2026-10-09"},
        {"id": "oracle-nvl", "question": "skipped", "sql": "select NVL(name, id) from public.customer", "verified_by": "pg-gateway", "verified_at": "2026-10-09"},
        {"id": "oracle-top", "question": "skipped", "sql": "select top 10 id from public.customer", "verified_by": "pg-gateway", "verified_at": "2026-10-09"},
        {"id": "oracle-rownum", "question": "skipped", "sql": "select id from public.customer where rownum < 2", "verified_by": "pg-gateway", "verified_at": "2026-10-09"}
    ]);
    let semantics = [
        ("customer", "id", "identifier"),
        ("customer", "bill_to", "foreign_key"),
        ("customer", "account_id", "foreign_key"),
        ("customer", "name", "entity_name"),
        ("customer", "code", "code"),
        ("customer", "opened", "date"),
        ("customer", "amount", "money"),
        ("customer", "touched", "audit_time"),
    ];
    let bound = binding(&sealed, &semantics, &[("customer", "amount", "USD")], examples);
    command(sealed, target, Some(bound))
}

#[test]
fn oracle_cases_pack_and_the_page_is_parsed_again() {
    let expect = expected();
    let want = &expect["oracle"];
    let command = oracle_command();
    let catalog_text = serde_json::to_string(&command["catalog"]).unwrap();
    oo_builder::compile_captured_pg_catalog(&catalog_text, None, None).expect("builder accepts the sealed catalog");
    let result = run(&command).expect("packed oracle command");
    assert!(!result.to_string().contains(OWNER));
    assert_eq!(result["status"], want["status"]);
    assert_eq!(result["semantic_caveats"], want["semantic_caveats"]);
    assert_eq!(result["native_notes"], want["native_notes"]);
    assert_eq!(result["source_semantics"]["contract"], want["contract"]);
    assert_eq!(result["chunk_key_order"]["decision"], want["chunk_decision"]);
    assert_eq!(result["chunk_key_order"]["live_binary_sort_proof"], want["live_binary_sort_proof"]);
    assert_eq!(result["live_database"], want["live_database"]);
    assert_eq!(result["catalog_proof"]["source_connection"], want["source_connection"]);
    assert_eq!(result["catalog_proof"]["status"], "accepted");
    assert_eq!(result["freshness_projection"]["gateway_get_freshness"], want["gateway_get_freshness"]);
    for (name, caveats) in want["column_caveats"].as_object().unwrap() {
        let found = result["columns"]
            .as_array()
            .unwrap()
            .iter()
            .find(|column| column["column"].as_str() == Some(name.as_str()))
            .unwrap();
        assert_eq!(&found["caveats"], caveats, "{name}");
    }
    for (name, trust) in want["trust"].as_object().unwrap() {
        let found = result["constraint_trust"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"].as_str() == Some(name.as_str()))
            .unwrap();
        assert_eq!(found["trust"], trust["trust"], "{name}");
        assert_eq!(found["disposition"], trust["disposition"], "{name}");
        if trust.get("ratio_thousandths").is_some() {
            assert_eq!(found["inclusion"]["ratio_thousandths"], trust["ratio_thousandths"]);
        }
    }
    let page = result["pack"]["pages"]["tables/public.customer.md"].as_str().unwrap();
    for needle in want["page_contains"].as_array().unwrap() {
        let text = needle.as_str().unwrap();
        assert!(page.contains(text), "page missing {text}");
    }
    for needle in want["page_absent"].as_array().unwrap() {
        let text = needle.as_str().unwrap();
        assert!(!page.contains(text), "page contains {text}");
    }
    let verified = result["pack"]["verified_sql"].as_str().unwrap();
    assert!(verified.contains(want["verified_contains"].as_str().unwrap()));
    for needle in want["verified_absent"].as_array().unwrap() {
        let text = needle.as_str().unwrap();
        assert!(!verified.contains(text), "verified.sql contains {text}");
    }
    let rejected: Vec<&str> = result["examples_rejected"].as_array().unwrap().iter().map(|item| item["id"].as_str().unwrap()).collect();
    for id in want["rejected_ids"].as_array().unwrap() {
        assert!(rejected.contains(&id.as_str().unwrap()));
    }
    assert_eq!(result["pack"]["readme_contains_pg_dialect_sentence"], want["readme_contains_pg_dialect_sentence"]);
    assert_eq!(result["pack"]["dedicated_heading_emitted"], want["dedicated_heading_emitted"]);
    assert_eq!(result["pack"]["caveat_fence_contains_heading"], want["caveat_fence_contains_heading"]);
    let readme = result["pack"]["readme"].as_str().unwrap();
    assert!(readme.contains("所有 SQL 为 PostgreSQL 方言，无论数据来自何种源库"));
    assert!(!readme.contains("consistency_grade: A"));
    let relationships = page.split("## 来源与语义").next().unwrap();
    let relationships = relationships.split("## relationships").nth(1).unwrap();
    assert!(!relationships.contains("customer_party_fk"));
    assert!(page.split("## 来源与语义").nth(1).unwrap().contains("customer_party_fk"));
    assert!(page.split("## 来源与语义").nth(1).unwrap().contains("declared_only"));
    let effective = result["role_pack_inputs"]["effective_json"].as_str().unwrap();
    let visibility = result["role_pack_inputs"]["visibility_json"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(effective).unwrap();
    let customer = parsed["datasets"].as_array().unwrap().iter().find(|item| item["dataset"] == "public.customer").unwrap();
    assert_eq!(customer["source_semantics"]["grade"], "B");
    assert!(customer["source_semantics"].get("contract").is_none());
    assert!(customer["source_semantics"].get("endpoint").is_none());
    let amount = customer["columns"].as_array().unwrap().iter().find(|item| item["name"] == "amount").unwrap();
    assert_eq!(amount["semantic_type"], "money");
    assert_eq!(amount["unit"], "USD");
    assert!(customer["source_semantics"]["constraint_trust"].as_array().unwrap().iter().any(|row| {
        row["name"] == "customer_party_fk" && row["trust"] == "declared_only" && row["disposition"] == "candidate"
    }));
    assert_eq!(result["fixed_renderer_gaps"], json!(["gateway_freshness_fields_absent"]));
    assert_eq!(result["freshness_projection"]["gateway_get_freshness"], "not_called");
    assert_eq!(result["freshness_projection"]["contract"], "pending_gateway_fields");
    assert_eq!(result["freshness_projection"]["consistency_grade"], "B");
    assert_eq!(result["freshness_projection"]["consistency_source"], "compiler_normalized_not_gateway");
    assert_ne!(result["source_semantics"]["grade"], "A");
    let pack = oo_builder::compile_role_pack(effective, visibility).expect("independent role pack");
    assert_eq!(pack.file("tables/public.customer.md").unwrap(), page);
    assert_eq!(pack.file("examples/verified.sql").unwrap(), verified);
    assert!(pack.file("README.md").unwrap().contains("所有 SQL 为 PostgreSQL 方言，无论数据来自何种源库"));
    assert!(result["builder_calls"].as_array().unwrap().iter().any(|item| item == "compile_role_pack"));
}

#[test]
fn precision_loss_and_declared_only_blocks_still_consume_the_catalog() {
    let expect = expected();
    let codes = &expect["blocks"];
    assert_block(&lossy_key(false), codes["precision_loss_key"].as_str().unwrap(), &["precision_loss_cursor"]);
    assert_block(&lossy_key(true), codes["precision_loss_key_raw_column_present"].as_str().unwrap(), &["precision_loss_cursor"]);
    let cursor = run(&cursor_loss()).unwrap();
    assert!(cursor["blocks"].as_array().unwrap().iter().any(|item| item["code"] == codes["precision_loss_cursor"]));
    assert!(cursor["blocks"].as_array().unwrap().iter().all(|item| item["code"] != "precision_loss_key"));
    assert_eq!(cursor["pack"], Value::Null);
    assert_eq!(cursor["catalog_proof"]["status"], "accepted");
    assert_block(&fk_case(None), codes["declared_only_fk_untested"].as_str().unwrap(), &[]);
    assert_block(&fk_case(Some((0, 1000))), codes["declared_only_fk_empty_sample"].as_str().unwrap(), &[]);
    assert_block(&fk_case(Some((100, 979))), codes["declared_only_fk_below_threshold"].as_str().unwrap(), &[]);
    assert_block(&unique_loss(), codes["unique_precision_loss"].as_str().unwrap(), &["precision_loss_cursor"]);
    let snow = run(&snowflake_fk()).unwrap();
    assert!(snow["blocks"].as_array().unwrap().iter().any(|item| item["code"] == codes["snowflake_enforced_becomes_declared_only"]));
    let row = snow["constraint_trust"].as_array().unwrap().iter().find(|item| item["name"] == "item_ref_fk").unwrap();
    assert_eq!(row["claimed_trust"], "enforced");
    assert_eq!(row["trust"], "declared_only");
    assert!(snow["semantic_caveats"].as_array().unwrap().iter().any(|item| item == "unenforced_constraints"));
    assert_eq!(snow["pack"], Value::Null);
    let allowed = run(&raw_key_allowed()).unwrap();
    assert_eq!(allowed["status"], expect["allows"]["raw_text_key"]);
    assert!(allowed["blocks"].as_array().unwrap().is_empty());
    assert!(allowed["semantic_caveats"].as_array().unwrap().iter().any(|item| item == "precision_loss"));
    assert!(!allowed["pack"].is_null());
}

#[test]
fn string_chunk_order_never_claims_a_live_binary_proof() {
    let expect = expected();
    let cases = [
        ("forced", json!({"key_kind": "string", "binary_order": "forced", "hash_chunking": false}), "forced_binary_order"),
        ("unavailable_hash", json!({"key_kind": "string", "binary_order": "unavailable", "hash_chunking": true}), "binary_order_unavailable"),
        ("index_unusable", json!({"key_kind": "binary", "binary_order": "index_unusable", "hash_chunking": false}), "index_unusable"),
        ("omitted", json!({"key_kind": "string", "hash_chunking": false}), "omitted_does_not_claim_binary"),
    ];
    for (name, chunk, reason) in cases {
        let result = run(&chunk_command(chunk)).unwrap();
        assert_eq!(result["status"], "packed", "{name}");
        assert_eq!(result["chunk_key_order"]["decision"], expect["chunks"][name], "{name}");
        assert_eq!(result["chunk_key_order"]["reason"], reason, "{name}");
        assert_eq!(result["chunk_key_order"]["live_binary_sort_proof"], "not_run");
    }
}

#[test]
fn cli_packed_blocked_and_usage_exits() {
    let bin = binary_path();
    let packed = cli(&bin, &oracle_command(), &[]);
    assert_eq!(packed.0, 0, "{}", packed.2);
    assert!(packed.1.contains("\"status\":\"packed\"") || packed.1.contains("\"status\": \"packed\""));
    let blocked = cli(&bin, &lossy_key(false), &[]);
    assert_eq!(blocked.0, 3, "{}", blocked.2);
    assert!(blocked.1.contains("precision_loss_key"));
    let usage = Command::new(&bin).arg("--help").output().unwrap();
    assert_eq!(usage.status.code(), Some(2));
    let empty = Command::new(&bin).stdin(Stdio::null()).output().unwrap();
    assert_eq!(empty.status.code(), Some(2));
}

fn binary_path() -> String {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_source_semantics")
        && !path.is_empty()
    {
        return path;
    }
    let fallback = "/target/debug/source-semantics";
    if std::path::Path::new(fallback).is_file() {
        return fallback.to_string();
    }
    panic!("source-semantics binary was not exposed");
}

fn cli(bin: &str, command: &Value, args: &[&str]) -> (i32, String, String) {
    let mut child = Command::new(bin);
    child.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut process = child.spawn().unwrap();
    {
        use std::io::Write;
        let stdin = process.stdin.as_mut().unwrap();
        stdin.write_all(serde_json::to_string(command).unwrap().as_bytes()).unwrap();
    }
    let output = process.wait_with_output().unwrap();
    (
        output.status.code().unwrap_or(1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

fn assert_block(command: &Value, code: &str, absent: &[&str]) {
    let result = run(command).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(result["status"], "blocked");
    assert_eq!(result["pack"], Value::Null);
    assert_eq!(result["catalog_proof"]["status"], "accepted");
    assert_eq!(result["builder_calls"], json!(["compile_captured_pg_catalog"]));
    assert!(result["blocks"].as_array().unwrap().iter().any(|item| item["code"] == code), "{code} in {}", result["blocks"]);
    for other in absent {
        assert!(result["blocks"].as_array().unwrap().iter().all(|item| item["code"] != *other));
    }
    let catalog_text = serde_json::to_string(&command["catalog"]).unwrap();
    oo_builder::compile_captured_pg_catalog(&catalog_text, None, None).expect("blocked catalog still compiles");
}

fn item_table(columns: Vec<Value>, primary: Vec<Value>, unique: Vec<Value>, foreign: Vec<Value>) -> Value {
    catalog(vec![table("item", columns, primary, unique, foreign)])
}

fn lossy_key(raw_present: bool) -> Value {
    let mut columns = vec![column(1, "id", 1700, "numeric")];
    if raw_present {
        columns.push(column(2, "id_raw", 25, "text"));
    }
    let sealed = item_table(columns, vec![key_col(1, "id")], vec![], vec![]);
    let mut target = base_target("oracle", "oracle");
    let mut facts = vec![json!({"schema": "public", "table": "item", "name": "id", "native_type": "NUMBER", "precision_loss": true})];
    if raw_present {
        facts.push(json!({"schema": "public", "table": "item", "name": "id_raw", "native_type": "VARCHAR2"}));
    }
    target["columns"] = Value::Array(facts);
    target["constraints"] = json!([
        {"schema": "public", "table": "item", "name": "item_pkey", "kind": "primary_key", "columns": ["id"], "trust": "enforced"}
    ]);
    command(sealed, target, None)
}

fn raw_key_allowed() -> Value {
    let sealed = item_table(
        vec![column(1, "id", 1700, "numeric"), column(2, "id_raw", 25, "text")],
        vec![key_col(2, "id_raw")],
        vec![],
        vec![],
    );
    let mut target = base_target("oracle", "oracle");
    target["columns"] = json!([
        {"schema": "public", "table": "item", "name": "id", "native_type": "NUMBER", "precision_loss": true},
        {"schema": "public", "table": "item", "name": "id_raw", "native_type": "VARCHAR2"}
    ]);
    target["constraints"] = json!([
        {"schema": "public", "table": "item", "name": "item_pkey", "kind": "primary_key", "columns": ["id_raw"], "trust": "enforced"}
    ]);
    let bound = binding(&sealed, &[], &[], json!([]));
    command(sealed, target, Some(bound))
}

fn cursor_loss() -> Value {
    let sealed = item_table(
        vec![column(1, "id", 23, "int4"), column(2, "touched", 1114, "timestamp")],
        vec![key_col(1, "id")],
        vec![],
        vec![],
    );
    let mut target = base_target("oracle", "oracle");
    target["columns"] = json!([
        {"schema": "public", "table": "item", "name": "touched", "native_type": "TIMESTAMP(9)", "fractional_seconds": 9}
    ]);
    target["cursor"] = json!({"schema": "public", "table": "item", "columns": ["touched"]});
    target["constraints"] = json!([
        {"schema": "public", "table": "item", "name": "item_pkey", "kind": "primary_key", "columns": ["id"], "trust": "enforced"}
    ]);
    command(sealed, target, None)
}

fn unique_loss() -> Value {
    let sealed = item_table(
        vec![column(1, "id", 23, "int4"), column(2, "touched", 1114, "timestamp")],
        vec![key_col(1, "id")],
        vec![json!({"name": "item_touched_uk", "columns": [{"attnum": 2, "name": "touched"}], "nulls_not_distinct": false})],
        vec![],
    );
    let mut target = base_target("oracle", "oracle");
    target["columns"] = json!([
        {"schema": "public", "table": "item", "name": "touched", "native_type": "TIMESTAMP(9)"}
    ]);
    target["constraints"] = json!([
        {"schema": "public", "table": "item", "name": "item_pkey", "kind": "primary_key", "columns": ["id"], "trust": "enforced"},
        {"schema": "public", "table": "item", "name": "item_touched_uk", "kind": "unique", "columns": ["touched"], "trust": "enforced"}
    ]);
    command(sealed, target, None)
}

fn fk_case(inclusion: Option<(u64, u32)>) -> Value {
    let parent = table("refitem", vec![column(1, "id", 23, "int4")], vec![key_col(1, "id")], vec![], vec![]);
    let child = table(
        "item",
        vec![column(1, "id", 23, "int4"), column(2, "ref_id", 23, "int4")],
        vec![key_col(1, "id")],
        vec![],
        vec![fk("item_ref_fk", 2, "ref_id", "refitem", 1, "id")],
    );
    let sealed = catalog(vec![parent, child]);
    let mut target = base_target("oracle", "oracle");
    let mut constraint = json!({
        "schema": "public",
        "table": "item",
        "name": "item_ref_fk",
        "kind": "foreign_key",
        "columns": ["ref_id"],
        "trust": "declared_only"
    });
    if let Some((rows, ratio)) = inclusion {
        constraint["inclusion"] = json!({"rows_checked": rows, "ratio_thousandths": ratio});
    }
    target["constraints"] = json!([constraint]);
    command(sealed, target, None)
}

fn snowflake_fk() -> Value {
    let mut command = fk_case(None);
    command["target"]["product"] = json!("snowflake");
    command["target"]["family"] = json!("snowflake");
    command["target"]["constraints"][0]["trust"] = json!("enforced");
    command["target"]["constraints"][0].as_object_mut().unwrap().remove("inclusion");
    command
}

fn chunk_command(chunk: Value) -> Value {
    let sealed = item_table(vec![column(1, "id", 23, "int4"), column(2, "label", 25, "text")], vec![key_col(1, "id")], vec![], vec![]);
    let mut target = base_target("postgres", "postgres");
    target["source_kind"] = json!("postgres_table");
    target["chunk"] = chunk;
    let bound = binding(&sealed, &[], &[], json!([]));
    command(sealed, target, Some(bound))
}

#[test]
fn profile_grade_is_not_a_consistency_grade() {
    let mut command = oracle_command();
    command["target"]["source_consistency"] = json!("A");
    let err = run(&command).expect_err("profile grade");
    assert!(err.contains("profile quality"), "{err}");
    assert!(!err.contains(OWNER));
}

#[test]
fn money_needs_an_explicit_unit_and_unknown_is_not_one() {
    let mut missing = oracle_command();
    let decisions = missing["binding"]["decisions"].as_array_mut().unwrap();
    let amount = decisions.iter_mut().find(|item| item["column"] == "amount").unwrap();
    amount.as_object_mut().unwrap().remove("unit");
    let err = run(&missing).expect_err("money without unit");
    assert!(
        err.contains("oos.invalid") || err.contains("effective OOS validation failed"),
        "{err}"
    );

    let mut unknown = oracle_command();
    let decisions = unknown["binding"]["decisions"].as_array_mut().unwrap();
    let amount = decisions.iter_mut().find(|item| item["column"] == "amount").unwrap();
    amount["unit"] = json!("unknown");
    let err = run(&unknown).expect_err("unknown unit");
    assert!(err.contains("decision.invalid"), "{err}");

    let mut upper = oracle_command();
    let decisions = upper["binding"]["decisions"].as_array_mut().unwrap();
    let amount = decisions.iter_mut().find(|item| item["column"] == "amount").unwrap();
    amount["unit"] = json!("UNKNOWN");
    let err = run(&upper).expect_err("UNKNOWN unit");
    assert!(err.contains("decision.invalid"), "{err}");
}

#[test]
fn hidden_column_and_hidden_dataset_semantics_do_not_leak() {
    let secret = table(
        "secret_table",
        vec![column(1, "id", 23, "int4"), column(2, "marker", 25, "text")],
        vec![key_col(1, "id")],
        vec![],
        vec![],
    );
    let customer = table(
        "customer",
        vec![column(1, "id", 23, "int4"), column(2, "leak_col", 25, "text")],
        vec![key_col(1, "id")],
        vec![],
        vec![],
    );
    let sealed = catalog(vec![customer, secret]);
    let mut target = base_target("oracle", "oracle");
    target["columns"] = json!([
        {"schema": "public", "table": "customer", "name": "leak_col", "native_type": "VARCHAR2", "trailing_space_insensitive": true},
        {"schema": "public", "table": "secret_table", "name": "marker", "native_type": "VARCHAR2", "lossy_type_fallback": true}
    ]);
    let mut bound = binding(&sealed, &[], &[], json!([]));
    let datasets = bound["visibility"]["datasets"].as_array_mut().unwrap();
    datasets.retain(|item| item["dataset"] == "public.customer");
    datasets[0]["columns"] = json!(["id"]);
    let result = run(&command(sealed, target, Some(bound))).expect("packed hidden case");
    assert_eq!(result["status"], "packed");
    let page = result["pack"]["pages"]["tables/public.customer.md"].as_str().unwrap();
    assert!(page.contains("## 来源与语义"));
    assert!(page.contains("consistency_grade: B"));
    for secret_text in [
        "leak_col",
        "trailing_space_insensitive",
        "secret_table",
        "marker",
        "lossy_type_fallback",
        "NVL",
    ] {
        assert!(!page.contains(secret_text), "page leaked {secret_text}");
        assert!(
            !result["pack"]["readme"].as_str().unwrap().contains(secret_text),
            "readme leaked {secret_text}"
        );
        assert!(
            !result["pack"]["verified_sql"].as_str().unwrap().contains(secret_text),
            "sql leaked {secret_text}"
        );
    }
    let effective: Value =
        serde_json::from_str(result["role_pack_inputs"]["effective_json"].as_str().unwrap()).unwrap();
    let hidden = effective["datasets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["dataset"] == "public.secret_table")
        .unwrap();
    assert!(
        hidden["source_semantics"]["columns"]
            .as_array()
            .unwrap()
            .iter()
            .any(|column| column["column"] == "marker")
    );
}
