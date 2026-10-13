//! Catalog, review command, explicit classification, effective OOS, role pack.
//!
//! The shop catalog is the locked captured-pg fixture. PII and semantic class
//! come only from the decisions this test writes. Schema hashes and column
//! fingerprints are read from `review-pg-draft` output.

use std::io::Write;
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

const SHOP_SCHEMA: &str = "bcb019d92270bf12092a3f47697b25719cc087a1ac4b7c56a10cf6dd00294c4c";
const COMMENT_BODY: &str = "a156-comment-body-do-not-carry";
const OWNER: &str = "owner_role_do_not_leak";
const CAPTURED_AT: &str = "2026-10-04T12:00:00.000000Z";
const COMMAND: &str = "oo.builder.reviewed-effective-oos-command/v1";

fn oo(args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_oo-builder"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("oo-builder");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn reviewed(stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_reviewed-effective-oos"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("reviewed-effective-oos");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn sha256_comment(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn comment(text: &str) -> Value {
    json!({"byte_len": text.len(), "sha256": sha256_comment(text)})
}

fn column(
    attnum: i32,
    name: &str,
    type_oid: i64,
    type_name: &str,
    not_null: bool,
    comment: Option<Value>,
) -> Value {
    json!({
        "attnum": attnum,
        "name": name,
        "name_ident": name,
        "type_oid": type_oid,
        "type_schema": "pg_catalog",
        "type_name": type_name,
        "typtype": "b",
        "typmod": -1,
        "base_type_oid": Value::Null,
        "numeric_precision": Value::Null,
        "numeric_scale": Value::Null,
        "not_null": not_null,
        "collation_oid": 0,
        "collation_name": Value::Null,
        "identity": "",
        "generated": "",
        "default_expr": Value::Null,
        "comment": comment,
        "enum_labels": [],
        "domain_checks": [],
        "type_support": {"converted": false, "reason": "known_builtin_not_converted"},
        "current_privileges": {"select": true, "insert": false, "update": false}
    })
}

fn shop() -> Value {
    json!({
        "protocol": "oo.source.pgsql/v1",
        "core_contract": "oo.config/v1",
        "source_id": "shop_src",
        "database": "shop",
        "captured_at": CAPTURED_AT,
        "schema_hash": SHOP_SCHEMA,
        "hash_algorithm": "pg-catalog-hash/v1",
        "ident_algorithm": "pgident/v1",
        "tables": [
            {
                "schema": "public",
                "schema_ident": "public",
                "name": "customer",
                "name_ident": "customer",
                "relkind": "r",
                "replica_identity": "d",
                "rls_enabled": false,
                "rls_forced": false,
                "owner": OWNER,
                "comment": Value::Null,
                "columns": [
                    column(1, "tenant_id", 23, "int4", true, None),
                    column(2, "id", 23, "int4", true, None),
                    column(3, "email", 25, "text", false, Some(comment(COMMENT_BODY)))
                ],
                "primary_key": [{"attnum": 1, "name": "tenant_id"}, {"attnum": 2, "name": "id"}],
                "primary_key_nulls_not_distinct": false,
                "unique_keys": [],
                "foreign_keys": [],
                "policies": [],
                "privileges": [
                    {"grantee": OWNER, "privilege": "INSERT", "grantable": true},
                    {"grantee": "reader", "privilege": "SELECT", "grantable": false}
                ]
            },
            {
                "schema": "public",
                "schema_ident": "public",
                "name": "purchase",
                "name_ident": "purchase",
                "relkind": "r",
                "replica_identity": "d",
                "rls_enabled": false,
                "rls_forced": false,
                "owner": OWNER,
                "comment": Value::Null,
                "columns": [
                    column(1, "tenant_id", 23, "int4", true, None),
                    column(2, "id", 23, "int4", true, None),
                    column(3, "customer_id", 23, "int4", true, None)
                ],
                "primary_key": [{"attnum": 1, "name": "tenant_id"}, {"attnum": 2, "name": "id"}],
                "primary_key_nulls_not_distinct": false,
                "unique_keys": [],
                "foreign_keys": [{
                    "name": "purchase_customer_fk",
                    "columns": [
                        {"attnum": 1, "name": "tenant_id", "ref_schema": "public", "ref_table": "customer", "ref_attnum": 1, "ref_name": "tenant_id"},
                        {"attnum": 3, "name": "customer_id", "ref_schema": "public", "ref_table": "customer", "ref_attnum": 2, "ref_name": "id"}
                    ]
                }],
                "policies": [],
                "privileges": [
                    {"grantee": OWNER, "privilege": "INSERT", "grantable": true},
                    {"grantee": "reader", "privilege": "SELECT", "grantable": false}
                ]
            }
        ],
        "type_conversion": "unsupported"
    })
}

fn settings() -> Value {
    json!({
        "project": "shop",
        "ontology_version": "ont-1",
        "catalog_version": "cat-1",
        "policy": {"id": "pol-shop", "version": "1"},
        "knowledge": {"id": "know-shop", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "shop",
            "locale": "en-US",
            "include_schemas": ["public"],
            "default_tier_preference": ["raw"],
            "publish": {"include_unreviewed": []},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        }
    })
}

fn review_body(catalog: &Value, curated: Option<Value>, reviews: Option<Value>) -> String {
    let mut body = json!({"catalog": catalog});
    if let Some(curated) = curated {
        body["curated"] = curated;
    }
    if let Some(reviews) = reviews {
        body["reviews"] = reviews;
    }
    serde_json::to_string(&body).unwrap()
}

fn artifact_from(body: &str) -> Value {
    let output = oo(&["review-pg-draft"], body);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn queue<'a>(artifact: &'a Value, item_id: &str, kind: &str) -> &'a Value {
    artifact["draft"]["review_queue"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["item_id"] == item_id && item["kind"] == kind)
        .unwrap_or_else(|| panic!("missing {item_id} {kind}"))
}

fn fingerprint(artifact: &Value, dataset: &str, column: &str) -> String {
    queue(
        artifact,
        &format!("column/{dataset}/{column}"),
        "structural_column",
    )["fact_fingerprint"]
        .as_str()
        .unwrap()
        .to_string()
}

fn bare() -> Value {
    artifact_from(&review_body(&shop(), None, None))
}

fn decision(
    artifact: &Value,
    dataset: &str,
    column: &str,
    pii: &str,
    semantic: &str,
) -> Value {
    json!({
        "dataset": dataset,
        "column": column,
        "schema_hash": artifact["draft"]["provenance"]["schema_hash"],
        "fact_fingerprint": fingerprint(artifact, dataset, column),
        "pii": pii,
        "semantic_type": semantic,
        "reviewer": {"by": "ada.owner", "at": "2026-10-06"}
    })
}

fn classified_columns(artifact: &Value) -> Vec<Value> {
    let mut decisions = Vec::new();
    for (dataset, column, semantic) in [
        ("public.customer", "tenant_id", "identifier"),
        ("public.customer", "id", "identifier"),
        ("public.purchase", "tenant_id", "identifier"),
        ("public.purchase", "id", "identifier"),
        ("public.purchase", "customer_id", "foreign_key"),
    ] {
        decisions.push(decision(artifact, dataset, column, "none", semantic));
    }
    decisions
}

fn command(artifact: &Value, decisions: Vec<Value>) -> String {
    serde_json::to_string(&json!({
        "protocol": COMMAND,
        "artifact": artifact,
        "settings": settings(),
        "decisions": decisions,
    }))
    .unwrap()
}

fn success_output() -> (String, Value) {
    let bare = bare();
    assert_eq!(bare["draft"]["provenance"]["schema_hash"], SHOP_SCHEMA);
    let schema = SHOP_SCHEMA;
    let mut reviews = Vec::new();
    for (dataset, column) in [
        ("public.customer", "tenant_id"),
        ("public.customer", "id"),
        ("public.purchase", "tenant_id"),
        ("public.purchase", "id"),
        ("public.purchase", "customer_id"),
    ] {
        reviews.push(json!({
            "item_id": format!("column/{dataset}/{column}"),
            "action": "accept",
            "schema_hash": schema,
            "fact_fingerprint": fingerprint(&bare, dataset, column),
        }));
    }
    let relationship = bare["draft"]["review_queue"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| {
            item["kind"] == "candidate_relationship"
                && item["item_id"]
                    .as_str()
                    .unwrap()
                    .contains("purchase_customer_fk")
        })
        .unwrap();
    reviews.push(json!({
        "item_id": relationship["item_id"],
        "action": "accept",
        "schema_hash": schema,
        "fact_fingerprint": relationship["fact_fingerprint"],
    }));
    let artifact = artifact_from(&review_body(&shop(), None, Some(json!(reviews))));
    let email = column_of(&artifact["draft"]["effective"], "public.customer", "email");
    assert!(email["pii"].is_null());
    assert!(email["semantic_type"].is_null());
    let text = command(&artifact, classified_columns(&artifact));
    let output = reviewed(&text);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.ends_with('\n'));
    assert!(!stdout.contains(OWNER));
    assert!(!stdout.contains(COMMENT_BODY));
    let value: Value = serde_json::from_str(&stdout).unwrap();
    (stdout, value)
}

fn column_of<'a>(effective: &'a Value, dataset: &str, name: &str) -> &'a Value {
    effective["datasets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["dataset"] == dataset)
        .unwrap()["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|column| column["name"] == name)
        .unwrap()
}

fn hidden_reason(value: &Value, dataset: &str, column: &str) -> String {
    value["hidden"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["dataset"] == dataset && item["column"] == column)
        .unwrap()["reason"]
        .as_str()
        .unwrap()
        .to_string()
}

fn assert_static_failure(output: &Output, code: &str) {
    assert_eq!(output.status.code(), Some(1), "{code}");
    assert!(output.stdout.is_empty());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains(code), "{err}");
    assert!(!err.contains(OWNER));
    assert!(!err.contains(COMMENT_BODY));
}

fn visible(dataset: &str, columns: &[&str]) -> Value {
    json!({
        "dataset": dataset,
        "row_security": "none",
        "global_stats": "denied",
        "columns": columns
    })
}

fn role_pack(effective: &Value, role: &str, datasets: Value) -> Output {
    let command = json!({
        "protocol": "oo.builder.role-pack-command/v1",
        "effective_oos": effective,
        "visibility": {
            "protocol": "oo.builder.role-visibility/v1",
            "project": "shop",
            "principal": "ada",
            "role": role,
            "ontology_version": "ont-1",
            "catalog_version": "cat-1",
            "policy": {"id": "pol-shop", "version": "1"},
            "knowledge": {"id": "know-shop", "version": "1"},
            "issued_for_compiler": "oo-builder/role-pack/v1",
            "datasets": datasets,
            "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
        }
    });
    oo(&["role-pack"], &serde_json::to_string(&command).unwrap())
}

fn pack_text(pack: &Value) -> String {
    pack["artifact"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["text"].as_str().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn shop_review_classifies_into_two_role_packs_and_keeps_email_hidden() {
    let (first, value) = success_output();
    let (second, _) = success_output();
    assert_eq!(first, second);
    assert_eq!(value["protocol"], COMMAND);
    assert_eq!(value["compiler"], "oo-builder/reviewed-effective-oos/v1");
    assert_eq!(value["effective_oos"]["protocol"], "oo.builder.effective-oos/v1");
    assert!(value["effective_oos"].get("decisions").is_none());
    assert!(value["omitted_relationships"].as_array().unwrap().is_empty());
    assert_eq!(
        hidden_reason(&value, "public.customer", "email"),
        "classification.absent"
    );
    assert_eq!(value["hidden"].as_array().unwrap().len(), 1);
    let email = column_of(&value["effective_oos"], "public.customer", "email");
    assert!(email["pii"].is_null());
    assert!(email["semantic_type"].is_null());
    assert!(email["classification"].is_null());
    assert_eq!(email["data_type"], "text");
    let id = column_of(&value["effective_oos"], "public.customer", "id");
    assert_eq!(id["pii"], "none");
    assert_eq!(id["semantic_type"], "identifier");
    assert_eq!(id["classification"]["state"], "confirmed");
    assert_eq!(id["classification"]["by"], "ada.owner");
    assert_eq!(id["classification"]["at"], "2026-10-06");
    assert!(id["review"].is_null());
    let customer_id = column_of(&value["effective_oos"], "public.purchase", "customer_id");
    assert_eq!(customer_id["semantic_type"], "foreign_key");
    assert_eq!(customer_id["pii"], "none");
    let relationship = &value["effective_oos"]["relationships"][0];
    assert_eq!(relationship["name"], "purchase_customer_fk");
    assert_eq!(relationship["provenance"], "declared_fk");
    assert_eq!(relationship["status"], "draft");
    assert!(relationship["review"].is_null());
    let provenance = &value["provenance"];
    assert_ne!(provenance["generated_sha256"], provenance["curated_sha256"]);
    for key in [
        "decisions_sha256",
        "sealed_source_sha256",
        "generated_sha256",
        "curated_sha256",
        "draft_hash",
        "wrapper_hash",
        "schema_hash",
    ] {
        let hex = provenance[key].as_str().unwrap();
        assert_eq!(hex.len(), 64, "{key}");
        assert!(hex.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
    assert_eq!(provenance["schema_hash"], SHOP_SCHEMA);
    for key in [
        "authentication",
        "authorization",
        "grants",
        "publication",
        "semantic_publication",
    ] {
        assert_eq!(provenance[key], "not_claimed");
    }
    assert_eq!(
        value["effective_oos"]["ontology"]["classification"]["auto_accept"],
        "rule_high_confidence"
    );

    let effective = &value["effective_oos"];
    let clerk = role_pack(
        effective,
        "oo_r_clerk",
        json!([
            visible("public.customer", &["tenant_id", "id"]),
            visible("public.purchase", &["tenant_id", "id", "customer_id"])
        ]),
    );
    assert_eq!(clerk.status.code(), Some(0), "{}", String::from_utf8_lossy(&clerk.stderr));
    let clerk_value: Value = serde_json::from_slice(&clerk.stdout).unwrap();
    let clerk_text = pack_text(&clerk_value);
    assert!(clerk_text.contains("purchase_customer_fk"));
    assert!(clerk_text.contains("declared_fk (source-declared, not a grant)"));
    assert!(!clerk_text.contains("email"));
    assert!(!clerk_text.contains(OWNER));
    assert_eq!(clerk_value["manifest"]["authentication"], "not_claimed");
    assert_eq!(clerk_value["manifest"]["publication"], "not_claimed");
    assert_eq!(clerk_value["manifest"]["row_security_reproven"], false);

    let peek = role_pack(
        effective,
        "oo_r_peek",
        json!([visible("public.customer", &["tenant_id", "id"])]),
    );
    assert_eq!(peek.status.code(), Some(0), "{}", String::from_utf8_lossy(&peek.stderr));
    let peek_text = pack_text(&serde_json::from_slice::<Value>(&peek.stdout).unwrap());
    assert!(!peek_text.contains("purchase_customer_fk"));
    assert!(!peek_text.contains("email"));

    let ledger = role_pack(
        effective,
        "oo_r_ledger",
        json!([
            visible("public.customer", &["tenant_id", "id"]),
            visible("public.purchase", &["tenant_id", "id"])
        ]),
    );
    assert_eq!(ledger.status.code(), Some(0), "{}", String::from_utf8_lossy(&ledger.stderr));
    let ledger_text = pack_text(&serde_json::from_slice::<Value>(&ledger.stdout).unwrap());
    assert!(!ledger_text.contains("purchase_customer_fk"));
    assert!(!ledger_text.contains("customer_id"));
    assert!(!ledger_text.contains("email"));

    let refused = role_pack(
        effective,
        "oo_r_clerk",
        json!([visible("public.customer", &["email"])]),
    );
    assert_eq!(refused.status.code(), Some(1));
    assert!(refused.stdout.is_empty());
    let err = String::from_utf8_lossy(&refused.stderr);
    assert!(err.contains("classification.unknown_pii"), "{err}");
}

#[test]
fn stale_unknown_duplicate_invalid_and_renamed_stay_explicit() {
    let artifact = bare();
    let mut decisions = classified_columns(&artifact);
    for item in &mut decisions {
        if item["dataset"] == "public.customer" && item["column"] == "id" {
            item["fact_fingerprint"] = json!("0".repeat(64));
        }
    }
    let output = reviewed(&command(&artifact, decisions));
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        hidden_reason(&value, "public.customer", "id"),
        "decision.stale_fact"
    );
    assert!(column_of(&value["effective_oos"], "public.customer", "id")["pii"].is_null());
    let stale_pack = role_pack(
        &value["effective_oos"],
        "oo_r_clerk",
        json!([visible("public.customer", &["id"])]),
    );
    assert!(String::from_utf8_lossy(&stale_pack.stderr).contains("classification.unknown_pii"));
    assert!(stale_pack.stdout.is_empty());

    let mut unknown = classified_columns(&artifact);
    let mut renamed_column = decision(&artifact, "public.customer", "id", "none", "identifier");
    renamed_column["column"] = json!("contact");
    unknown.push(renamed_column);
    assert_static_failure(&reviewed(&command(&artifact, unknown)), "/decision.unknown");

    let mut duplicated = classified_columns(&artifact);
    duplicated.push(decision(
        &artifact,
        "public.customer",
        "tenant_id",
        "none",
        "identifier",
    ));
    assert_static_failure(
        &reviewed(&command(&artifact, duplicated)),
        "/decision.duplicate",
    );

    let mut invalid = classified_columns(&artifact);
    invalid[0]["pii"] = json!("secret");
    assert_static_failure(&reviewed(&command(&artifact, invalid)), "/decision.invalid");

    let mut renamed = shop();
    for table in renamed["tables"].as_array_mut().unwrap() {
        if table["name"] == "customer" {
            for column in table["columns"].as_array_mut().unwrap() {
                if column["name"] == "email" {
                    column["name"] = json!("contact");
                    column["name_ident"] = json!("contact");
                }
            }
        }
    }
    let output = oo(&["review-pg-draft"], &review_body(&renamed, None, None));
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("hash.schema"), "{err}");
    assert!(!err.contains(OWNER));
    assert!(!err.contains(COMMENT_BODY));
}

#[test]
fn curated_pii_is_not_copied_and_money_needs_an_existing_unit() {
    let artifact = bare();
    let mut money = vec![decision(&artifact, "public.customer", "email", "none", "money")];
    money[0]["schema_hash"] = artifact["draft"]["provenance"]["schema_hash"].clone();
    assert_static_failure(&reviewed(&command(&artifact, money)), "/oos.invalid");

    let curated = json!({
        "datasets": [{
            "dataset": "public.customer",
            "columns": [{
                "name": "email",
                "unit": "CNY",
                "pii": "personal",
                "semantic_type": "email"
            }]
        }],
        "relationships": []
    });
    let reviews = json!([{
        "item_id": "column/public.customer/email",
        "action": "override",
        "schema_hash": SHOP_SCHEMA,
        "fact_fingerprint": fingerprint(&artifact, "public.customer", "email")
    }]);
    let overridden = artifact_from(&review_body(&shop(), Some(curated), Some(reviews)));
    let mut decisions = classified_columns(&overridden);
    decisions.push(decision(
        &overridden,
        "public.customer",
        "email",
        "none",
        "money",
    ));
    let output = reviewed(&command(&overridden, decisions));
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains("personal"));
    assert!(!stdout.contains(OWNER));
    let value: Value = serde_json::from_str(&stdout).unwrap();
    let email = column_of(&value["effective_oos"], "public.customer", "email");
    assert_eq!(email["pii"], "none");
    assert_eq!(email["semantic_type"], "money");
    assert_eq!(email["unit"], "CNY");
    assert_ne!(
        value["provenance"]["generated_sha256"],
        value["provenance"]["curated_sha256"]
    );
}

#[test]
fn explicit_usd_classifies_money_and_unknown_or_absent_units_fail() {
    let artifact = bare();
    let mut usd_decisions = classified_columns(&artifact);
    let mut usd = decision(&artifact, "public.customer", "email", "none", "money");
    usd["unit"] = json!("USD");
    usd_decisions.push(usd);
    let output = reviewed(&command(&artifact, usd_decisions));
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let email = column_of(&value["effective_oos"], "public.customer", "email");
    assert_eq!(email["semantic_type"], "money");
    assert_eq!(email["unit"], "USD");

    for unit in ["unknown", "UNKNOWN", "Unknown"] {
        let mut decisions = classified_columns(&artifact);
        let mut bad = decision(&artifact, "public.customer", "email", "none", "money");
        bad["unit"] = json!(unit);
        decisions.push(bad);
        assert_static_failure(&reviewed(&command(&artifact, decisions)), "/decision.invalid");
    }

    let mut absent = classified_columns(&artifact);
    absent.push(decision(
        &artifact,
        "public.customer",
        "email",
        "none",
        "money",
    ));
    assert_static_failure(&reviewed(&command(&artifact, absent)), "/oos.invalid");
}

#[test]
fn rejected_column_leaves_the_effective_draft_and_the_decision_is_unknown() {
    let bare = bare();
    let reviews = json!([{
        "item_id": "column/public.customer/email",
        "action": "reject",
        "schema_hash": SHOP_SCHEMA,
        "fact_fingerprint": fingerprint(&bare, "public.customer", "email")
    }]);
    let artifact = artifact_from(&review_body(&shop(), None, Some(reviews)));
    assert!(
        artifact["draft"]["effective"]["datasets"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|dataset| dataset["dataset"] == "public.customer")
            .flat_map(|dataset| dataset["columns"].as_array().unwrap())
            .all(|column| column["name"] != "email")
    );
    let mut named = classified_columns(&artifact);
    named.push(decision(
        &artifact,
        "public.customer",
        "email",
        "none",
        "identifier",
    ));
    assert_static_failure(&reviewed(&command(&artifact, named)), "/decision.unknown");

    let output = reviewed(&command(&artifact, classified_columns(&artifact)));
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        value["effective_oos"]["datasets"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|dataset| dataset["columns"].as_array().unwrap())
            .all(|column| column["name"] != "email")
    );
    assert!(
        value["hidden"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["column"] != "email")
    );
    let refused = role_pack(
        &value["effective_oos"],
        "oo_r_clerk",
        json!([visible("public.customer", &["email"])]),
    );
    assert_eq!(refused.status.code(), Some(1));
    assert!(refused.stdout.is_empty());
    let err = String::from_utf8_lossy(&refused.stderr);
    assert!(err.contains("visibility.unknown_column"), "{err}");
    assert!(!err.contains(OWNER));
    assert!(!err.contains(COMMENT_BODY));
}

#[test]
fn usage_scanner_and_settings_failures_do_not_echo_the_body() {
    let output = Command::new(env!("CARGO_BIN_EXE_reviewed-effective-oos"))
        .arg("help")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage"));

    let artifact = bare();
    let mut extra = serde_json::from_str::<Value>(&command(&artifact, vec![])).unwrap();
    extra["extra"] = json!(OWNER);
    assert_static_failure(&reviewed(&serde_json::to_string(&extra).unwrap()), "/field.unknown");

    let bad_protocol = command(&artifact, vec![]).replacen(COMMAND, "oo.builder.other/v1", 1);
    assert_static_failure(&reviewed(&bad_protocol), "/protocol.mismatch");

    let mut bad_settings = serde_json::from_str::<Value>(&command(&artifact, vec![])).unwrap();
    bad_settings["settings"]["project"] = json!("Shop");
    assert_static_failure(
        &reviewed(&serde_json::to_string(&bad_settings).unwrap()),
        "/settings.invalid",
    );

    let duplicated = format!(
        "{{\"protocol\":\"{COMMAND}\",\"protocol\":\"{COMMAND}\",\"artifact\":{{}},\"settings\":{{}},\"decisions\":[]}}"
    );
    assert_static_failure(&reviewed(&duplicated), "/json.duplicate_key");
}
