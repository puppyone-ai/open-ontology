//! Structural diff of two captured PostgreSQL draft artifacts.
//!
//! Catalogs are compiled with `compile_captured_pg_catalog`. The schema-hash
//! preimage below is a copy of `pg-catalog-hash/v1` and is locked to the shop
//! digests already checked in by the compiler. `compile_captured_pg_catalog`
//! is the acceptance oracle. This file does not open a database.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Map, Value, json};

use oo_builder::{CapturedPgDraft, compile_captured_pg_catalog};
use oo_core::api::payload_sha256;

const SHOP_SCHEMA: &str = "bcb019d92270bf12092a3f47697b25719cc087a1ac4b7c56a10cf6dd00294c4c";
const STALE_SCHEMA: &str = "31bbede923a28e58c8fef54240de5458533fa881e4d5898cdd31504b4aa0bd7e";
const COMMENT_BODY: &str = "a156-comment-body-do-not-carry";
const OWNER: &str = "owner_role_do_not_leak";
const CURATED_BY: &str = "secret-owner-name-a213";
const CURATED_LABEL: &str = "do-not-echo-label-a213";
const SECRET_PROTOCOL: &str = "secret-protocol-do-not-echo-a213";
const SECRET_STATUS: &str = "secret-status-do-not-echo-a213";
const SECRET_COMMIT: &str = "secret-commit-do-not-echo-a213";
const SECRET_FIELD: &str = "secret-field-do-not-echo-a213";
const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const COMMAND_PROTOCOL: &str = "oo.builder.captured-pg-draft-command/v1";
const USAGE: &str = "\
ontology-artifact-diff: usage: ontology-artifact-diff --before <file> --after <file>\n\
ontology-artifact-diff: reads two oo.builder.captured-pg-draft-command/v1 artifacts\n\
ontology-artifact-diff: writes one oo.builder.artifact-diff/v1 structural diff\n\
ontology-artifact-diff: does not connect, call a model, import rows, accept, grant, certify, or publish\n";

const FULL_FK: &[FkPair] = &[
    FkPair {
        attnum: 1,
        name: "tenant_id",
        ref_attnum: 1,
        ref_name: "tenant_id",
    },
    FkPair {
        attnum: 3,
        name: "customer_id",
        ref_attnum: 2,
        ref_name: "id",
    },
];

const SHORT_FK: &[FkPair] = &[FkPair {
    attnum: 3,
    name: "customer_id",
    ref_attnum: 2,
    ref_name: "id",
}];

#[derive(Clone, Copy)]
struct FkPair {
    attnum: i32,
    name: &'static str,
    ref_attnum: i32,
    ref_name: &'static str,
}

#[derive(Clone, Copy)]
struct Shop {
    email_name: &'static str,
    email_oid: i64,
    email_type: &'static str,
    email_not_null: bool,
    fk: &'static [FkPair],
}

impl Shop {
    fn base() -> Self {
        Self {
            email_name: "email",
            email_oid: 25,
            email_type: "text",
            email_not_null: false,
            fk: FULL_FK,
        }
    }

    fn not_null(self) -> Self {
        Self {
            email_not_null: true,
            ..self
        }
    }

    fn renamed(self) -> Self {
        Self {
            email_name: "contact_email",
            ..self
        }
    }

    fn varchar(self) -> Self {
        Self {
            email_oid: 1043,
            email_type: "varchar",
            ..self
        }
    }

    fn short_fk(self) -> Self {
        Self {
            fk: SHORT_FK,
            ..self
        }
    }
}

#[test]
fn preimage_locks_the_checked_in_shop_hashes() {
    assert_eq!(oo_core::limits::DOCUMENT_MAX_BYTES, 1_048_576);
    assert_eq!(schema_hash(&Shop::base()), SHOP_SCHEMA);
    assert_eq!(schema_hash(&Shop::base().not_null()), STALE_SCHEMA);
    let base = compile(Shop::base(), None, None);
    assert_eq!(base.schema_hash, SHOP_SCHEMA);
    assert!(!base.draft.contains(COMMENT_BODY));
    assert!(!base.draft.contains(OWNER));
    assert!(!base.wrapper.contains(COMMENT_BODY));
    assert!(!base.evidence_envelope.contains(COMMENT_BODY));
    let stale = compile(Shop::base().not_null(), None, None);
    assert_eq!(stale.schema_hash, STALE_SCHEMA);
}

#[test]
fn unchanged_catalogs_emit_a_deterministic_sealed_diff() {
    let dir = scratch("unchanged");
    let text = command_of(&compile(Shop::base(), None, None));
    let path = write_text(&dir, "shop.json", &text);
    let (first, doc) = ok(&path, &path);
    let (second, _) = ok(&path, &path);
    assert_eq!(first, second);
    assert_eq!(doc["facts"].as_array().unwrap().len(), 0);
    assert_header(&doc, true);
    assert_counts(&doc, 33);
    assert_sorted(&doc);
    assert_sealed(&first, &doc);
    assert_eq!(doc["before"]["schema_hash"], SHOP_SCHEMA);
    assert_eq!(doc["after"]["schema_hash"], SHOP_SCHEMA);
    assert_eq!(doc["before"]["source_id"], "shop_src");
    assert_eq!(doc["before"]["database"], "shop");
    let parsed: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(digest(&parsed["draft"]), doc["before"]["draft_hash"].as_str().unwrap());
}

#[test]
fn flag_order_and_whitespace_do_not_change_the_diff() {
    let dir = scratch("reorder");
    let before = command_of(&compile(Shop::base(), None, None));
    let after = command_of(&compile(Shop::base().renamed(), None, None));
    let before_path = write_text(&dir, "before.json", &before);
    let after_path = write_text(&dir, "after.json", &after);
    let forward = run(&[
        "--before",
        before_path.to_str().unwrap(),
        "--after",
        after_path.to_str().unwrap(),
    ]);
    let reverse = run(&[
        "--after",
        after_path.to_str().unwrap(),
        "--before",
        before_path.to_str().unwrap(),
    ]);
    assert_eq!(forward.status.code(), Some(0), "{}", stderr_text(&forward));
    assert_eq!(forward.stdout, reverse.stdout);

    let value: Value = serde_json::from_str(&before).unwrap();
    let pretty = serde_json::to_string_pretty(&value).unwrap();
    assert_ne!(pretty, before);
    let mut reordered = String::from("{\"wrapper\":");
    reordered.push_str(&canonical_json(&value["wrapper"]));
    reordered.push_str(",\"protocol\":");
    reordered.push_str(&canonical_json(&value["protocol"]));
    reordered.push_str(",\"evidence_envelope\":");
    reordered.push_str(&canonical_json(&value["evidence_envelope"]));
    reordered.push_str(",\"draft\":");
    reordered.push_str(&canonical_json(&value["draft"]));
    reordered.push('}');
    assert_ne!(reordered, before);
    let pretty_path = write_text(&dir, "pretty.json", &pretty);
    let reordered_path = write_text(&dir, "reordered.json", &reordered);
    let (plain, _) = ok(&before_path, &before_path);
    let (from_pretty, _) = ok(&pretty_path, &before_path);
    let (from_reordered, _) = ok(&reordered_path, &before_path);
    assert_eq!(plain, from_pretty);
    assert_eq!(plain, from_reordered);
}

#[test]
fn column_rename_adds_and_removes_column_identities() {
    let doc = diff_shops(Shop::base(), Shop::base().renamed(), "rename");
    assert_header(&doc, false);
    assert_counts(&doc, 10);
    assert_sorted(&doc);
    fact(
        &doc,
        "removed",
        "column",
        "column/public.customer/email",
        "physical",
    );
    fact(
        &doc,
        "added",
        "column",
        "column/public.customer/contact_email",
        "physical",
    );
    fact(
        &doc,
        "removed",
        "column",
        "column/public.customer/email",
        "generated",
    );
    fact(
        &doc,
        "added",
        "column",
        "column/public.customer/contact_email",
        "generated",
    );
    fact(
        &doc,
        "stale",
        "column",
        "column/public.customer/id",
        "physical",
    );
    fact(&doc, "stale", "table", "table/public.customer", "physical");
    fact(
        &doc,
        "stale",
        "primary_key",
        "primary_key/public.customer",
        "physical",
    );
    fact(
        &doc,
        "stale",
        "foreign_key",
        "foreign_key/public.purchase/purchase_customer_fk",
        "physical",
    );
    let source = fact(&doc, "changed", "source", "source/shop_src/shop", "physical");
    assert_ne!(source["before"]["schema_hash"], source["after"]["schema_hash"]);
    assert!(doc["facts"].as_array().unwrap().iter().all(|item| {
        item["kind"] != "classification" && item["layer"] != "curated"
    }));
}

#[test]
fn type_change_changes_the_physical_and_generated_column() {
    let doc = diff_shops(Shop::base(), Shop::base().varchar(), "type");
    assert_header(&doc, false);
    assert_counts(&doc, 12);
    let physical = fact(
        &doc,
        "changed",
        "column",
        "column/public.customer/email",
        "physical",
    );
    assert_eq!(physical["before"]["type_oid"].as_i64(), Some(25));
    assert_eq!(physical["after"]["type_oid"].as_i64(), Some(1043));
    assert_eq!(physical["before"]["type_name"], "text");
    assert_eq!(physical["after"]["type_name"], "varchar");
    assert_eq!(physical["before"]["type_reason"], "known_builtin_not_converted");
    assert_eq!(physical["after"]["type_reason"], "known_builtin_not_converted");
    assert_eq!(physical["before"]["not_null"].as_bool(), Some(false));
    let generated = fact(
        &doc,
        "changed",
        "column",
        "column/public.customer/email",
        "generated",
    );
    assert_eq!(generated["before"]["data_type"], "text");
    assert_eq!(generated["after"]["data_type"], "varchar");
    fact(
        &doc,
        "stale",
        "column",
        "column/public.customer/tenant_id",
        "physical",
    );
}

#[test]
fn foreign_key_change_keeps_the_constraint_identity() {
    let doc = diff_shops(Shop::base(), Shop::base().short_fk(), "fk");
    assert_header(&doc, false);
    assert_counts(&doc, 12);
    let physical = fact(
        &doc,
        "changed",
        "foreign_key",
        "foreign_key/public.purchase/purchase_customer_fk",
        "physical",
    );
    assert_eq!(physical["before"]["columns"].as_array().unwrap().len(), 2);
    assert_eq!(physical["after"]["columns"].as_array().unwrap().len(), 1);
    assert_eq!(physical["after"]["columns"][0]["name"], "customer_id");
    assert_eq!(physical["after"]["columns"][0]["ref_name"], "id");
    let generated = fact(
        &doc,
        "changed",
        "foreign_key",
        "foreign_key/public.purchase/purchase_customer_fk",
        "generated",
    );
    assert_eq!(generated["before"]["from_columns"].as_array().unwrap().len(), 2);
    assert_eq!(generated["after"]["from_columns"].as_array().unwrap().len(), 1);
    assert_eq!(generated["after"]["from_columns"][0], "customer_id");
    fact(&doc, "changed", "source", "source/shop_src/shop", "physical");
    fact(
        &doc,
        "stale",
        "column",
        "column/public.purchase/customer_id",
        "physical",
    );
}

#[test]
fn stale_review_and_curated_classification_stay_unaccepted() {
    let before_compiled = compile(Shop::base(), None, None);
    let fingerprint = column_fingerprint(&before_compiled.draft);
    let reviews = serde_json::to_string(&json!([{
        "item_id": "column/public.customer/email",
        "action": "accept",
        "schema_hash": SHOP_SCHEMA,
        "fact_fingerprint": fingerprint,
    }]))
    .unwrap();
    let curated = serde_json::to_string(&json!({
        "datasets": [{
            "dataset": "public.customer",
            "columns": [{
                "name": "email",
                "label": CURATED_LABEL,
                "classification": {"state": "confirmed", "by": CURATED_BY}
            }]
        }]
    }))
    .unwrap();
    let after_compiled = compile(Shop::base().not_null(), Some(&curated), Some(&reviews));
    assert_eq!(after_compiled.schema_hash, STALE_SCHEMA);
    let queue = serde_json::from_str::<Value>(&after_compiled.draft).unwrap();
    let stale = queue["review_queue"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| {
            item["item_id"] == "column/public.customer/email" && item["kind"] == "stale_decision"
        })
        .unwrap();
    assert_eq!(stale["detail"], "stale_schema_hash");
    assert_eq!(stale["publishable"].as_bool(), Some(false));

    let dir = scratch("stale-review");
    let before = write_text(&dir, "before.json", &command_of(&before_compiled));
    let after = write_text(&dir, "after.json", &command_of(&after_compiled));
    let (stdout, doc) = ok(&before, &after);
    assert_header(&doc, false);
    assert_counts(&doc, 12);
    assert_eq!(doc["auto_accept"].as_bool(), Some(false));
    assert_eq!(doc["publication"], "unavailable");
    assert_eq!(doc["grants"], "not_authorized");
    assert_eq!(doc["certification"], "none");
    let column = fact(
        &doc,
        "changed",
        "column",
        "column/public.customer/email",
        "physical",
    );
    assert_eq!(column["before"]["not_null"].as_bool(), Some(false));
    assert_eq!(column["after"]["not_null"].as_bool(), Some(true));
    fact(
        &doc,
        "stale",
        "column",
        "column/public.customer/email",
        "generated",
    );
    let class = fact(
        &doc,
        "added",
        "classification",
        "classification/public.customer/email",
        "curated",
    );
    assert!(class["before"].is_null());
    assert_eq!(class["after"]["unknown"].as_bool(), Some(false));
    assert_eq!(class["after"]["state"], "confirmed");
    assert!(class["after"].get("by").is_none());
    assert_eq!(class["after"]["value_sha256"].as_str().unwrap().len(), 64);
    let decision = fact(
        &doc,
        "stale",
        "review",
        "column/public.customer/email",
        "decision",
    );
    assert!(decision["before"].is_null());
    assert_eq!(decision["after"]["action"], "accept");
    assert_eq!(decision["after"]["schema_hash"], SHOP_SCHEMA);
    let queued = fact(
        &doc,
        "stale",
        "review",
        "column/public.customer/email",
        "queue",
    );
    assert_eq!(queued["after"]["kind"], "stale_decision");
    assert_eq!(queued["after"]["detail"], "stale_schema_hash");
    assert_eq!(queued["after"]["publishable"].as_bool(), Some(false));
    assert!(doc["facts"].as_array().unwrap().iter().all(|item| {
        !(item["kind"] == "classification" && item["layer"] == "generated")
    }));
    assert_quiet(&stdout, "");
}

#[test]
fn resealed_duplicate_column_is_duplicate_fact() {
    let dir = scratch("duplicate-fact");
    let text = command_of(&compile(Shop::base(), None, None));
    let mut root: Value = serde_json::from_str(&text).unwrap();
    let columns = root["draft"]["physical"]["tables"][0]["columns"]
        .as_array_mut()
        .unwrap();
    let first = columns[0].clone();
    columns.push(first);
    let good = write_text(&dir, "good.json", &text);
    let unsealed = write_text(&dir, "unsealed.json", &canonical_json(&root));
    let sealed = reseal(root);
    let sealed_path = write_text(&dir, "sealed.json", &canonical_json(&sealed));
    let mismatch = run_paths(&unsealed, &good);
    assert_refuse(&mismatch, "before", "hash.mismatch", "artifact hash does not match the canonical bytes");
    let duplicate = run_paths(&sealed_path, &good);
    assert_refuse(&duplicate, "before", "duplicate.fact", "artifact repeats a structural identity");
}

#[test]
fn corrupt_version_and_shape_inputs_refuse() {
    let dir = scratch("refuse");
    let text = command_of(&compile(Shop::base(), None, None));
    let good = write_text(&dir, "good.json", &text);
    let mut root: Value = serde_json::from_str(&text).unwrap();

    assert_usage(&[]);
    assert_usage(&["--before", good.to_str().unwrap()]);
    assert_usage(&[
        "--before",
        good.to_str().unwrap(),
        "--before",
        good.to_str().unwrap(),
        "--after",
        good.to_str().unwrap(),
    ]);
    assert_usage(&["--before", "-secret", "--after", good.to_str().unwrap()]);
    assert_usage(&["--after", good.to_str().unwrap(), "extra"]);

    let missing = dir.join("missing.json");
    assert_refuse(
        &run_paths(&missing, &good),
        "before",
        "io.open",
        "file could not be opened",
    );
    let folder = dir.join("folder");
    fs::create_dir(&folder).unwrap();
    assert_refuse(
        &run_paths(&folder, &good),
        "before",
        "io.regular",
        "path is not a regular file",
    );
    let link = dir.join("link.json");
    std::os::unix::fs::symlink(&good, &link).unwrap();
    assert_refuse(
        &run_paths(&good, &link),
        "after",
        "io.regular",
        "path is not a regular file",
    );
    let oversize = write_text(&dir, "oversize.json", &"x".repeat(1_048_577));
    assert_refuse(
        &run_paths(&good, &oversize),
        "after",
        "json.budget.bytes",
        "document exceeds the byte budget",
    );
    let utf8 = dir.join("utf8.json");
    fs::write(&utf8, [0xff, 0xfe]).unwrap();
    assert_refuse(
        &run_paths(&utf8, &good),
        "before",
        "json.utf8",
        "document is not UTF-8",
    );
    let syntax = write_text(&dir, "syntax.json", "{\"draft\"");
    assert_refuse(
        &run_paths(&good, &syntax),
        "after",
        "json.syntax",
        "document is not valid JSON",
    );
    let duplicate_key = write_text(&dir, "dup-key.json", "{\"draft\":{},\"draft\":{}}");
    assert_refuse(
        &run_paths(&duplicate_key, &good),
        "before",
        "json.duplicate_key",
        "duplicate JSON object key",
    );

    root["protocol"] = json!(SECRET_PROTOCOL);
    let version = write_text(&dir, "version.json", &canonical_json(&root));
    let version_out = run_paths(&version, &good);
    assert_refuse(
        &version_out,
        "before",
        "version.unsupported",
        "artifact version is not the frozen captured-pg-draft set",
    );
    assert_quiet("", &stderr_text(&version_out));

    root["protocol"] = json!(COMMAND_PROTOCOL);
    root["wrapper"]["historical"]["pg_catalog_contract_commit"] = json!(SECRET_COMMIT);
    let commit = write_text(&dir, "commit.json", &canonical_json(&root));
    let commit_out = run_paths(&good, &commit);
    assert_refuse(
        &commit_out,
        "after",
        "version.unsupported",
        "artifact version is not the frozen captured-pg-draft set",
    );
    assert_quiet("", &stderr_text(&commit_out));

    root["wrapper"]["historical"]["pg_catalog_contract_commit"] =
        json!("b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74");
    root["wrapper"]["publication"] = json!(SECRET_STATUS);
    let status = write_text(&dir, "status.json", &canonical_json(&root));
    let status_out = run_paths(&status, &good);
    assert_refuse(
        &status_out,
        "before",
        "status.refused",
        "artifact claims a status this contract does not emit",
    );
    assert_quiet("", &stderr_text(&status_out));

    root["wrapper"]["publication"] = json!("unavailable");
    root["wrapper"]["draft_hash"] = json!(ZERO_HASH);
    let hash_path = write_text(&dir, "hash.json", &canonical_json(&root));
    let hash_out = run_paths(&good, &hash_path);
    assert_refuse(
        &hash_out,
        "after",
        "hash.mismatch",
        "artifact hash does not match the canonical bytes",
    );
    assert!(!stderr_text(&hash_out).contains(ZERO_HASH));

    root["wrapper"]["draft_hash"] = json!(text_hash(&text, "draft_hash"));
    root.as_object_mut().unwrap().insert(SECRET_FIELD.to_string(), json!(1));
    let unknown = write_text(&dir, "unknown.json", &canonical_json(&root));
    let unknown_out = run_paths(&unknown, &good);
    assert_refuse(
        &unknown_out,
        "before",
        "field.unknown",
        "artifact has an unknown field",
    );
    assert_quiet("", &stderr_text(&unknown_out));
}

fn text_hash(document: &str, field: &str) -> String {
    serde_json::from_str::<Value>(document).unwrap()["wrapper"][field]
        .as_str()
        .unwrap()
        .to_string()
}

fn diff_shops(before: Shop, after: Shop, name: &str) -> Value {
    let dir = scratch(name);
    let before_text = command_of(&compile(before, None, None));
    let after_text = command_of(&compile(after, None, None));
    let before_path = write_text(&dir, "before.json", &before_text);
    let after_path = write_text(&dir, "after.json", &after_text);
    let (stdout, doc) = ok(&before_path, &after_path);
    assert_sealed(&stdout, &doc);
    assert_quiet(&stdout, "");
    doc
}

fn compile(shop: Shop, curated: Option<&str>, reviews: Option<&str>) -> CapturedPgDraft {
    let catalog = catalog_json(&shop);
    compile_captured_pg_catalog(&catalog, curated, reviews)
        .unwrap_or_else(|err| panic!("compile failed: {err}"))
}

fn command_of(compiled: &CapturedPgDraft) -> String {
    format!(
        "{{\"draft\":{},\"evidence_envelope\":{},\"protocol\":\"{COMMAND_PROTOCOL}\",\"wrapper\":{}}}",
        compiled.draft, compiled.evidence_envelope, compiled.wrapper
    )
}

fn column_fingerprint(draft: &str) -> String {
    let value: Value = serde_json::from_str(draft).unwrap();
    value["review_queue"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| {
            item["item_id"] == "column/public.customer/email" && item["kind"] == "structural_column"
        })
        .unwrap()["fact_fingerprint"]
        .as_str()
        .unwrap()
        .to_string()
}

fn catalog_json(shop: &Shop) -> String {
    let hash = schema_hash(shop);
    let comment = json!({
        "byte_len": COMMENT_BODY.len(),
        "sha256": payload_sha256(COMMENT_BODY.as_bytes()),
    });
    let customer = table_json(
        "customer",
        json!([
            column_json(1, "tenant_id", 23, "int4", true, Value::Null),
            column_json(2, "id", 23, "int4", true, Value::Null),
            column_json(3, shop.email_name, shop.email_oid, shop.email_type, shop.email_not_null, comment),
        ]),
        json!([]),
    );
    let mut pairs = Vec::new();
    for pair in shop.fk {
        pairs.push(json!({
            "attnum": pair.attnum,
            "name": pair.name,
            "ref_schema": "public",
            "ref_table": "customer",
            "ref_attnum": pair.ref_attnum,
            "ref_name": pair.ref_name,
        }));
    }
    let purchase = table_json(
        "purchase",
        json!([
            column_json(1, "tenant_id", 23, "int4", true, Value::Null),
            column_json(2, "id", 23, "int4", true, Value::Null),
            column_json(3, "customer_id", 23, "int4", true, Value::Null),
        ]),
        json!([{
            "name": "purchase_customer_fk",
            "columns": pairs,
        }]),
    );
    serde_json::to_string(&json!({
        "protocol": "oo.source.pgsql/v1",
        "core_contract": "oo.config/v1",
        "source_id": "shop_src",
        "database": "shop",
        "captured_at": "2026-10-04T12:00:00.000000Z",
        "schema_hash": hash,
        "hash_algorithm": "pg-catalog-hash/v1",
        "ident_algorithm": "pgident/v1",
        "tables": [customer, purchase],
        "type_conversion": "unsupported"
    }))
    .unwrap()
}

fn table_json(name: &str, columns: Value, foreign_keys: Value) -> Value {
    json!({
        "schema": "public",
        "schema_ident": "public",
        "name": name,
        "name_ident": name,
        "relkind": "r",
        "replica_identity": "d",
        "rls_enabled": false,
        "rls_forced": false,
        "owner": OWNER,
        "comment": Value::Null,
        "columns": columns,
        "primary_key": [{"attnum": 1, "name": "tenant_id"}, {"attnum": 2, "name": "id"}],
        "primary_key_nulls_not_distinct": false,
        "unique_keys": [],
        "foreign_keys": foreign_keys,
        "policies": [],
        "privileges": [{"grantee": "reader", "privilege": "SELECT", "grantable": false}]
    })
}

fn column_json(
    attnum: i32,
    name: &str,
    type_oid: i64,
    type_name: &str,
    not_null: bool,
    comment: Value,
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

fn schema_hash(shop: &Shop) -> String {
    payload_sha256(schema_preimage(shop).as_bytes())
}

fn schema_preimage(shop: &Shop) -> String {
    let mut out = String::from("{\"tables\":[");
    write_table(
        &mut out,
        "customer",
        &[
            (1, "tenant_id", 23, true),
            (2, "id", 23, true),
            (3, shop.email_name, shop.email_oid, shop.email_not_null),
        ],
        &[],
    );
    out.push(',');
    write_table(
        &mut out,
        "purchase",
        &[
            (1, "tenant_id", 23, true),
            (2, "id", 23, true),
            (3, "customer_id", 23, true),
        ],
        shop.fk,
    );
    out.push_str("]}");
    out
}

fn write_table(
    out: &mut String,
    name: &str,
    columns: &[(i32, &str, i64, bool)],
    fk: &[FkPair],
) {
    out.push('{');
    write_str(out, "schema", "public");
    out.push(',');
    write_str(out, "name", name);
    out.push(',');
    write_str(out, "relkind", "r");
    out.push(',');
    write_str(out, "replica_identity", "d");
    out.push(',');
    write_bool(out, "rls", false);
    out.push(',');
    write_bool(out, "force_rls", false);
    out.push_str(",\"columns\":[");
    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write_column(out, column.0, column.1, column.2, column.3);
    }
    out.push_str("],\"primary_key\":[{\"attnum\":1,\"name\":\"tenant_id\"},{\"attnum\":2,\"name\":\"id\"}],\"primary_key_nulls_not_distinct\":false,\"unique_keys\":[],\"foreign_keys\":[");
    if !fk.is_empty() {
        out.push_str("{\"name\":\"purchase_customer_fk\",\"columns\":[");
        for (index, pair) in fk.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push('{');
            write_i32(out, "attnum", pair.attnum);
            out.push(',');
            write_str(out, "name", pair.name);
            out.push(',');
            write_str(out, "ref_schema", "public");
            out.push(',');
            write_str(out, "ref_table", "customer");
            out.push(',');
            write_i32(out, "ref_attnum", pair.ref_attnum);
            out.push(',');
            write_str(out, "ref_name", pair.ref_name);
            out.push('}');
        }
        out.push_str("]}");
    }
    out.push_str("],\"policies\":[]}");
}

fn write_column(out: &mut String, attnum: i32, name: &str, type_oid: i64, not_null: bool) {
    out.push('{');
    write_i32(out, "attnum", attnum);
    out.push(',');
    write_str(out, "name", name);
    out.push(',');
    write_i64(out, "type_oid", type_oid);
    out.push_str(",\"typmod\":-1,");
    write_bool(out, "not_null", not_null);
    out.push_str(",\"collation_oid\":0,");
    write_str(out, "typtype", "b");
    out.push_str(",\"base_type_oid\":null,");
    write_str(out, "identity", "");
    out.push(',');
    write_str(out, "generated", "");
    out.push_str(",\"default_sha256\":null,\"enum_labels\":[],\"domain_checks\":[]}");
}

fn write_str(out: &mut String, name: &str, value: &str) {
    push_string(out, name);
    out.push(':');
    push_string(out, value);
}

fn write_bool(out: &mut String, name: &str, value: bool) {
    push_string(out, name);
    out.push(':');
    out.push_str(if value { "true" } else { "false" });
}

fn write_i32(out: &mut String, name: &str, value: i32) {
    push_string(out, name);
    out.push(':');
    out.push_str(&value.to_string());
}

fn write_i64(out: &mut String, name: &str, value: i64) {
    push_string(out, name);
    out.push(':');
    out.push_str(&value.to_string());
}

fn reseal(mut root: Value) -> Value {
    let mut bare = root["draft"].clone();
    bare["provenance"]
        .as_object_mut()
        .unwrap()
        .remove("manifest_hash");
    let manifest = digest(&bare);
    root["draft"]["provenance"]["manifest_hash"] = json!(manifest);
    let draft_hash = digest(&root["draft"]);
    root["wrapper"]["manifest_hash"] = json!(manifest);
    root["wrapper"]["draft_hash"] = json!(draft_hash);
    let mut wrapper = root["wrapper"].clone();
    wrapper.as_object_mut().unwrap().remove("wrapper_hash");
    let wrapper_hash = digest(&wrapper);
    root["wrapper"]["wrapper_hash"] = json!(wrapper_hash);
    assert_eq!(
        digest(&root["evidence_envelope"]),
        root["wrapper"]["evidence_envelope_hash"].as_str().unwrap()
    );
    root
}

fn ok(before: &Path, after: &Path) -> (String, Value) {
    let output = run_paths(before, after);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    assert!(output.stderr.is_empty(), "{}", stderr_text(&output));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_quiet(&stdout, "");
    let doc: Value = serde_json::from_str(&stdout).unwrap();
    (stdout, doc)
}

fn run_paths(before: &Path, after: &Path) -> Output {
    run(&[
        "--before",
        before.to_str().unwrap(),
        "--after",
        after.to_str().unwrap(),
    ])
}

fn run(args: &[&str]) -> Output {
    // Cargo injects the hyphenated binary path at compile time for `cargo test`.
    // `std::env::var` does not see that rustc env, and overlay-2 panicked on it.
    Command::new(env!("CARGO_BIN_EXE_ontology-artifact-diff"))
        .args(args)
        .output()
        .unwrap()
}

fn assert_usage(args: &[&str]) {
    let output = run(args);
    assert_eq!(output.status.code(), Some(2), "{}", stderr_text(&output));
    assert!(output.stdout.is_empty());
    assert_eq!(stderr_text(&output), USAGE);
}

fn assert_refuse(output: &Output, side: &str, code: &str, message: &str) {
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(output));
    assert!(output.stdout.is_empty());
    let expected = format!(
        "ontology-artifact-diff: {side}: oo.builder.artifact-diff/v1/{code} {message}\n"
    );
    assert_eq!(stderr_text(output), expected);
}

fn assert_header(doc: &Value, equal: bool) {
    assert_eq!(doc["protocol"], "oo.builder.artifact-diff/v1");
    assert_eq!(doc["compiler"], "ontology-artifact-diff/v1");
    assert_eq!(doc["auto_accept"].as_bool(), Some(false));
    assert_eq!(doc["publication"], "unavailable");
    assert_eq!(doc["certification"], "none");
    assert_eq!(doc["grants"], "not_authorized");
    assert_eq!(doc["schema_hash_equal"].as_bool(), Some(equal));
}

fn assert_counts(doc: &Value, unchanged: u64) {
    let facts = doc["facts"].as_array().unwrap();
    let mut added = 0u64;
    let mut changed = 0u64;
    let mut removed = 0u64;
    let mut stale = 0u64;
    for fact in facts {
        match fact["status"].as_str().unwrap() {
            "added" => added += 1,
            "changed" => changed += 1,
            "removed" => removed += 1,
            "stale" => stale += 1,
            other => panic!("unexpected status {other}"),
        }
    }
    assert_eq!(doc["counts"]["added"].as_u64(), Some(added));
    assert_eq!(doc["counts"]["changed"].as_u64(), Some(changed));
    assert_eq!(doc["counts"]["removed"].as_u64(), Some(removed));
    assert_eq!(doc["counts"]["stale"].as_u64(), Some(stale));
    assert_eq!(doc["counts"]["unchanged"].as_u64(), Some(unchanged));
}

fn assert_sorted(doc: &Value) {
    let facts = doc["facts"].as_array().unwrap();
    let keys: Vec<_> = facts
        .iter()
        .map(|fact| {
            (
                fact["status"].as_str().unwrap(),
                fact["kind"].as_str().unwrap(),
                fact["id"].as_str().unwrap(),
                fact["layer"].as_str().unwrap(),
            )
        })
        .collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    assert_eq!(keys, sorted);
}

fn assert_sealed(stdout: &str, doc: &Value) {
    assert!(!stdout.ends_with('\n'));
    let mut bare = doc.clone();
    let found = bare.as_object_mut().unwrap().remove("diff_hash").unwrap();
    let digest = digest(&bare);
    assert_eq!(found.as_str(), Some(digest.as_str()));
    bare.as_object_mut()
        .unwrap()
        .insert("diff_hash".to_string(), Value::String(digest));
    assert_eq!(canonical_json(&bare), stdout);
}

fn assert_quiet(stdout: &str, stderr: &str) {
    for secret in [
        COMMENT_BODY,
        OWNER,
        CURATED_BY,
        CURATED_LABEL,
        SECRET_PROTOCOL,
        SECRET_STATUS,
        SECRET_COMMIT,
        SECRET_FIELD,
    ] {
        assert!(!stdout.contains(secret), "{secret} leaked to stdout");
        assert!(!stderr.contains(secret), "{secret} leaked to stderr");
    }
}

fn fact<'a>(doc: &'a Value, status: &str, kind: &str, id: &str, layer: &str) -> &'a Value {
    doc["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| {
            item["status"] == status
                && item["kind"] == kind
                && item["id"] == id
                && item["layer"] == layer
        })
        .unwrap_or_else(|| panic!("missing {status} {kind} {id} {layer}"))
}

fn digest(value: &Value) -> String {
    payload_sha256(canonical_json(value).as_bytes())
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oo-a213-artifact-diff-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_text(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, text).unwrap();
    path
}

fn stderr_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
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
