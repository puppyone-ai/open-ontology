//! Sealed import through source semantics, effective OOS, and the role pack.
//!
//! The catalog fixture is the captured producer document. Target facts are
//! independent of that capture. This file does not open a database or call a model.

use serde_json::{Value, json};

use source_semantics::compile_source_semantics;

const FILE_SHA256: &str = "d9af109ea2f8cd63497771d960a8c396b959501b575b07aa9668fb04a61db844";
const SHADOW: &str = "pt_9cb24f0ac8a48d5f4f4f6d93f5ebe7f2c4162c9ae5237040365d3f2047ce";

fn fixture_text() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/w06-sealed-import-catalog.json");
    std::fs::read_to_string(path).expect("fixture")
}

fn run(value: &Value) -> Result<Value, String> {
    let text = serde_json::to_string(value).expect("command");
    match compile_source_semantics(&text) {
        Ok(out) => Ok(serde_json::from_str(&out).expect("result")),
        Err(err) => Err(format!("{} {} {}", err.code, err.path, err.message)),
    }
}

#[test]
fn sealed_import_packs_money_hidden_columns_and_a_declared_fk() {
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/w06-sealed-import-catalog.json"),
    )
    .unwrap();
    assert_eq!(oo_core::api::payload_sha256(&bytes), FILE_SHA256);
    let catalog: Value = serde_json::from_str(&fixture_text()).unwrap();
    let pg = oo_builder::compile_captured_pg_catalog(&fixture_text(), None, None).expect_err("pg");
    assert!(pg.code.ends_with("/field.unknown"));
    assert_eq!(pg.message, "catalog evidence is not oo.source.pgsql/v1");
    let draft = oo_builder::compile_sealed_import_catalog(&fixture_text()).expect("compiler");
    let command = packed_command(&catalog, &draft.identity_hash, true);
    let result = run(&command).expect("packed");
    assert_eq!(result["status"], "packed");
    assert_eq!(
        result["builder_calls"],
        json!([
            "compile_sealed_import_catalog",
            "compile_reviewed_sealed_import",
            "compile_role_pack"
        ])
    );
    assert_eq!(result["catalog_proof"]["status"], "accepted");
    assert_eq!(result["catalog_proof"]["source_engine"], "mysql");
    assert_eq!(result["catalog_proof"]["normalized_representation"], "postgresql_target_storage");
    assert_eq!(result["catalog_proof"]["source_connection"], "not_used");
    assert_eq!(result["live_database"], "not_connected");
    assert_eq!(result["freshness_projection"]["gateway_get_freshness"], "not_called");
    assert_eq!(result["source_semantics"]["grade"], "B");
    assert_ne!(result["source_semantics"]["grade"], "A");
    let rendered = result.to_string();
    assert!(!rendered.contains("oo.source.pgsql/v1"));
    assert!(!rendered.contains(SHADOW));
    let id = result["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|column| column["column"] == "erp.parcel.id")
        .unwrap();
    assert_eq!(id["native_type"], "absent");
    let rate = result["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|column| column["column"] == "erp.parcel.rate")
        .unwrap();
    assert!(rate["caveats"].as_array().unwrap().iter().any(|token| token == "precision_loss"));
    let page = result["pack"]["pages"]["tables/erp.parcel.md"].as_str().unwrap();
    for needle in [
        "amount",
        "USD",
        "## 来源与语义",
        "consistency_grade: B",
        "postgresql_target_storage",
        "tinyint1_as_int2",
        "enum_as_text",
        "parcel_key",
        "parcel_box_fk",
        "declared_only",
        "family: mysql",
        "capture: sealed_import",
    ] {
        assert!(page.contains(needle), "page missing {needle}");
    }
    for needle in ["json_not_jsonb", "blob_col", "mysql_time_not_interval", SHADOW, "pc_", "pn_", "NVL"] {
        assert!(!page.contains(needle), "page contains {needle}");
    }
    let semantics = page.split("## 来源与语义").nth(1).unwrap();
    assert!(semantics.contains("parcel_box_fk"));
    assert!(semantics.contains("rows 40 ratio_thousandths 990"));
    let relationships = page.split("## 来源与语义").next().unwrap();
    let relationships = relationships.split("## relationships").nth(1).unwrap();
    assert!(!relationships.contains("parcel_box_fk"));
    let readme = result["pack"]["readme"].as_str().unwrap();
    assert!(readme.contains("所有 SQL 为 PostgreSQL 方言，无论数据来自何种源库"));
    let verified = result["pack"]["verified_sql"].as_str().unwrap();
    // empty_box.id is outside the grant. The role pack drops any example
    // whose SQL names that hidden token, so the kept example does not use id.
    assert!(verified.contains("select amount from erp.parcel where amount is not null"));
    assert!(!verified.contains("NVL"));
    let rejected: Vec<&str> = result["examples_rejected"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect();
    assert!(rejected.contains(&"source-nvl"));
    let effective = result["role_pack_inputs"]["effective_json"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(effective).unwrap();
    let parcel = parsed["datasets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["dataset"] == "erp.parcel")
        .unwrap();
    let amount = parcel["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "amount")
        .unwrap();
    assert_eq!(amount["semantic_type"], "money");
    assert_eq!(amount["unit"], "USD");
    assert_eq!(amount["data_type"], "numeric");
    assert_eq!(parcel["source_semantics"]["grade"], "B");
    assert!(parcel["source_semantics"]["caveats"]
        .as_array()
        .unwrap()
        .iter()
        .any(|token| token == "postgresql_target_storage"));
    let visibility = result["role_pack_inputs"]["visibility_json"].as_str().unwrap();
    let pack = oo_builder::compile_role_pack(effective, visibility).expect("consumer");
    assert_eq!(pack.file("tables/erp.parcel.md").unwrap(), page);
    assert!(pack.cache_key.contains(&draft.identity_hash));
    assert!(!pack.cache_key.contains("cat-1"));
}

#[test]
fn unit_fk_precision_and_identity_failures_stay_on_the_same_path() {
    let catalog: Value = serde_json::from_str(&fixture_text()).unwrap();
    let draft = oo_builder::compile_sealed_import_catalog(&fixture_text()).unwrap();
    let mut missing = packed_command(&catalog, &draft.identity_hash, true);
    let decisions = missing["binding"]["decisions"].as_array_mut().unwrap();
    let amount = decisions.iter_mut().find(|item| item["column"] == "amount").unwrap();
    amount.as_object_mut().unwrap().remove("unit");
    let err = run(&missing).expect_err("money");
    assert!(err.contains("oos.invalid"), "{err}");
    let mut unknown = packed_command(&catalog, &draft.identity_hash, true);
    let decisions = unknown["binding"]["decisions"].as_array_mut().unwrap();
    let amount = decisions.iter_mut().find(|item| item["column"] == "amount").unwrap();
    amount["unit"] = json!("unknown");
    let err = run(&unknown).expect_err("unknown");
    assert!(err.contains("decision.invalid"), "{err}");
    let untested = packed_command(&catalog, &draft.identity_hash, false);
    let blocked = run(&untested).expect("fk block");
    assert_eq!(blocked["status"], "blocked");
    assert_eq!(blocked["pack"], Value::Null);
    assert_eq!(blocked["catalog_proof"]["status"], "accepted");
    assert_eq!(blocked["builder_calls"], json!(["compile_sealed_import_catalog"]));
    assert!(blocked["blocks"].as_array().unwrap().iter().any(|item| item["code"] == "declared_only_fk_untested"));
    let mut lossy = catalog.clone();
    lossy["tables"][0]["columns"][0]["oid"] = json!(700);
    lossy["schema_digest"] = json!(schema_digest(&lossy["tables"]));
    let lossy_draft = oo_builder::compile_sealed_import_catalog(&lossy.to_string()).expect_err("key");
    assert!(lossy_draft.code.ends_with("/precision_loss_key"));
    let lossy_command = packed_command(&lossy, &draft.identity_hash, true);
    let lossy_result = run(&lossy_command).expect("blocked key");
    assert_eq!(lossy_result["status"], "blocked");
    assert_eq!(lossy_result["pack"], Value::Null);
    assert_eq!(lossy_result["catalog_proof"]["status"], "refused");
    assert_eq!(lossy_result["builder_calls"], json!(["compile_sealed_import_catalog"]));
    let mut stale = packed_command(&catalog, &draft.identity_hash, true);
    stale["binding"]["settings"]["catalog_version"] = json!("cat-1");
    let err = run(&stale).expect_err("stale version");
    assert!(err.contains("binding.catalog_version"), "{err}");
    let mut changed = catalog.clone();
    let mut digest = changed["input_digest"].as_str().unwrap().to_string().into_bytes();
    digest[0] = if digest[0] == b'a' { b'b' } else { b'a' };
    changed["input_digest"] = json!(String::from_utf8(digest).unwrap());
    let changed_draft = oo_builder::compile_sealed_import_catalog(&changed.to_string()).unwrap();
    assert_ne!(changed_draft.identity_hash, draft.identity_hash);
    let changed_result = run(&packed_command(&changed, &changed_draft.identity_hash, true)).unwrap();
    let changed_pack = oo_builder::compile_role_pack(
        changed_result["role_pack_inputs"]["effective_json"].as_str().unwrap(),
        changed_result["role_pack_inputs"]["visibility_json"].as_str().unwrap(),
    )
    .unwrap();
    let first_result = run(&packed_command(&catalog, &draft.identity_hash, true)).unwrap();
    let first = oo_builder::compile_role_pack(
        first_result["role_pack_inputs"]["effective_json"].as_str().unwrap(),
        first_result["role_pack_inputs"]["visibility_json"].as_str().unwrap(),
    )
    .unwrap();
    assert_ne!(first.cache_key, changed_pack.cache_key);
    assert!(first.cache_key.contains(&draft.identity_hash));
    assert!(changed_pack.cache_key.contains(&changed_draft.identity_hash));
    assert!(!changed_pack.cache_key.contains(&draft.identity_hash));
}

#[test]
fn captured_postgresql_uuid_catalog_packs_without_rewriting_the_source_type() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/oo06-pg-sealed-import-catalog.json");
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes.len(), 8488);
    assert_eq!(
        oo_core::api::payload_sha256(&bytes),
        "51ee055b40bc478308152b17d78e36ff2864e0c30df640d930553cdd75401cac"
    );
    let text = std::fs::read_to_string(&path).unwrap();
    let pg = oo_builder::compile_captured_pg_catalog(&text, None, None).expect_err("pg abi");
    assert!(pg.code.ends_with("/field.unknown"));
    assert_eq!(pg.message, "catalog evidence is not oo.source.pgsql/v1");
    let catalog: Value = serde_json::from_str(&text).unwrap();
    let draft = oo_builder::compile_sealed_import_catalog(&text).expect("compiler");
    assert_eq!(draft.source_engine, "postgresql");
    assert_eq!(draft.source_id, "oo_dl01w1_46");
    let hub = draft
        .columns
        .iter()
        .find(|column| column.dataset == "public.hubs" && column.name == "hub_token")
        .expect("hub_token");
    assert_eq!(hub.target_type, "uuid");
    assert_eq!(hub.target_oid, 2950);
    assert!(hub.source_type.is_none());
    let result = run(&pg_packed(&catalog, &draft.identity_hash)).expect("packed");
    assert_eq!(result["status"], "packed");
    assert_eq!(
        result["builder_calls"],
        json!([
            "compile_sealed_import_catalog",
            "compile_reviewed_sealed_import",
            "compile_role_pack"
        ])
    );
    assert_eq!(result["catalog_proof"]["status"], "accepted");
    assert_eq!(result["catalog_proof"]["source_engine"], "postgresql");
    assert_eq!(result["catalog_proof"]["source_id"], "oo_dl01w1_46");
    assert_eq!(result["catalog_proof"]["schema_digest"], draft.schema_digest);
    assert_eq!(result["live_database"], "not_connected");
    assert_eq!(result["freshness_projection"]["gateway_get_freshness"], "not_called");
    let native = result["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|column| column["column"] == "public.hubs.hub_token")
        .unwrap();
    assert_eq!(native["native_type"], "absent");
    assert!(native["native_note"].is_null());
    let page = result["pack"]["pages"]["tables/public.hubs.md"].as_str().unwrap();
    assert!(page.contains("| `hub_token` |  | uuid | identifier | none |  |"));
    assert!(!page.contains("| `hub_token` |  | text |"));
    assert!(page.contains("## 来源与语义"));
    assert!(page.contains("postgresql_target_storage"));
    assert!(page.contains("family: postgresql"));
    let readme = result["pack"]["readme"].as_str().unwrap();
    assert!(readme.contains("所有 SQL 为 PostgreSQL 方言，无论数据来自何种源库"));
    let verified = result["pack"]["verified_sql"].as_str().unwrap();
    assert!(verified.contains("select hub_token from public.hubs where hub_token is not null"));
    assert!(!verified.contains("NVL"));
    let rejected: Vec<&str> = result["examples_rejected"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect();
    assert!(rejected.contains(&"source-nvl"));
    let effective = result["role_pack_inputs"]["effective_json"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(effective).unwrap();
    assert_eq!(parsed["catalog_version"], draft.identity_hash);
    let file = parsed["datasets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["dataset"] == "public.hubs")
        .unwrap();
    let column = file["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "hub_token")
        .unwrap();
    assert_eq!(column["data_type"], "uuid");
    assert_eq!(column["semantic_type"], "identifier");
    let mut named = catalog.clone();
    set_pg_column(&mut named, "hubs", "hub_token", "source_type", json!("uuid"));
    let named_draft = oo_builder::compile_sealed_import_catalog(&named.to_string()).expect("named");
    let named_hub = named_draft
        .columns
        .iter()
        .find(|column| column.name == "hub_token")
        .unwrap();
    assert_eq!(named_hub.source_type.as_deref(), Some("uuid"));
    assert_eq!(named_hub.target_type, "uuid");
    assert_eq!(named_draft.identity_hash, draft.identity_hash);
    let named_result = run(&pg_packed(&named, &named_draft.identity_hash)).expect("named pack");
    let named_native = named_result["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|column| column["column"] == "public.hubs.hub_token")
        .unwrap();
    assert_eq!(named_native["native_type"], "uuid");
    let mut text_type = catalog.clone();
    set_pg_column(&mut text_type, "hubs", "hub_token", "source_type", json!("text"));
    let err = run(&pg_packed(&text_type, &draft.identity_hash)).expect_err("text");
    assert!(err.contains("type.ambiguous"), "{err}");
    let mut unknown = catalog.clone();
    set_pg_column(&mut unknown, "hubs", "hub_token", "oid", json!(2951));
    unknown["schema_digest"] = json!(schema_digest(&unknown["tables"]));
    let err = run(&pg_packed(&unknown, &draft.identity_hash)).expect_err("oid");
    assert!(err.contains("type.ambiguous"), "{err}");
    let mut digest = catalog.clone();
    digest["schema_digest"] = json!("ab".repeat(32));
    let err = run(&pg_packed(&digest, &draft.identity_hash)).expect_err("digest");
    assert!(err.contains("digest.mismatch"), "{err}");
    let mut clickhouse = catalog.clone();
    clickhouse["engine"] = json!("clickhouse");
    let clickhouse_draft = oo_builder::compile_sealed_import_catalog(&clickhouse.to_string()).unwrap();
    assert_eq!(
        clickhouse_draft.columns.iter().find(|column| column.name == "hub_token").unwrap().target_type,
        "uuid"
    );
    let mut mongo = catalog.clone();
    mongo["engine"] = json!("mongodb");
    let mongo_draft = oo_builder::compile_sealed_import_catalog(&mongo.to_string()).unwrap();
    assert_eq!(
        mongo_draft.columns.iter().find(|column| column.name == "event_token").unwrap().target_type,
        "uuid"
    );
    assert_ne!(mongo_draft.identity_hash, draft.identity_hash);
}

fn set_pg_column(doc: &mut Value, table: &str, column: &str, field: &str, value: Value) {
    for item in doc["tables"].as_array_mut().expect("tables") {
        if item["logical_table"] != table {
            continue;
        }
        for entry in item["columns"].as_array_mut().expect("columns") {
            if entry["logical_name"] == column {
                entry[field] = value;
                return;
            }
        }
    }
    panic!("missing {table}.{column}");
}

fn pg_packed(catalog: &Value, identity: &str) -> Value {
    let decisions = [
        ("public.hubs", "hub_token"),
        ("public.shipments", "shipment_token"),
        ("public.tracking_events", "event_token"),
    ];
    let mut rows = Vec::new();
    for (dataset, column) in decisions {
        rows.push(json!({
            "dataset": dataset,
            "column": column,
            "pii": "none",
            "semantic_type": "identifier",
            "reviewer": {"by": "ada.owner", "at": "2026-10-11"}
        }));
    }
    json!({
        "protocol": "oo.source-semantics.command/v1",
        "catalog": catalog,
        "target": {
            "family": catalog["engine"].as_str().unwrap_or("postgresql"),
            "product": catalog["engine"].as_str().unwrap_or("postgresql"),
            "version": "16",
            "capture": "sealed_import",
            "source_consistency": "B",
            "delete_detection": "reconcile",
            "source_kind": "postgres_table",
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
        },
        "binding": {
            "settings": pg_settings(identity),
            "decisions": rows,
            "visibility": {
                "protocol": "oo.builder.role-visibility/v1",
                "project": "lane",
                "principal": "ada",
                "role": "oo_r_clerk",
                "ontology_version": "ont-1",
                "catalog_version": identity,
                "policy": {"id": "pol-lane", "version": "1"},
                "knowledge": {"id": "know-lane", "version": "1"},
                "issued_for_compiler": "oo-builder/role-pack/v1",
                "datasets": [
                    {"dataset": "public.hubs", "row_security": "none", "global_stats": "denied", "columns": ["hub_token"]},
                    {"dataset": "public.shipments", "row_security": "none", "global_stats": "denied", "columns": ["shipment_token"]},
                    {"dataset": "public.tracking_events", "row_security": "none", "global_stats": "denied", "columns": ["event_token"]}
                ],
                "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
            },
            "examples": [
                {
                    "id": "token-present",
                    "question": "present rows",
                    "sql": "select hub_token from public.hubs where hub_token is not null",
                    "uses": ["dataset:public.hubs"],
                    "verified_by": "pg-gateway",
                    "verified_at": "2026-10-11"
                },
                {
                    "id": "source-nvl",
                    "question": "skipped",
                    "sql": "select NVL(hub_token, hub_token) from public.hubs",
                    "verified_by": "pg-gateway",
                    "verified_at": "2026-10-11"
                }
            ]
        }
    })
}

fn pg_settings(identity: &str) -> Value {
    json!({
        "project": "lane",
        "ontology_version": "ont-1",
        "catalog_version": identity,
        "policy": {"id": "pol-lane", "version": "1"},
        "knowledge": {"id": "know-lane", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "lane",
            "locale": "en-US",
            "include_schemas": ["public"],
            "default_tier_preference": ["raw"],
            "publish": {"include_unreviewed": []},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 4000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        }
    })
}

#[test]
fn oracle_engine_uses_the_same_import_path() {
    let mut catalog: Value = serde_json::from_str(&fixture_text()).unwrap();
    catalog["engine"] = json!("oracle");
    let draft = oo_builder::compile_sealed_import_catalog(&catalog.to_string()).unwrap();
    assert_eq!(draft.source_engine, "oracle");
    assert_eq!(draft.normalized_representation, "postgresql_target_storage");
    let mut command = packed_command(&catalog, &draft.identity_hash, true);
    command["target"]["family"] = json!("oracle");
    command["target"]["product"] = json!("oracle");
    command["target"]["source_kind"] = json!("oracle_table");
    let result = run(&command).expect("oracle path");
    assert_eq!(result["status"], "packed");
    assert_eq!(result["catalog_proof"]["source_engine"], "oracle");
    assert_eq!(result["builder_calls"][0], "compile_sealed_import_catalog");
    assert_ne!(result["builder_calls"][1], "compile_reviewed_effective");
    let mut conflict = command.clone();
    conflict["target"]["family"] = json!("mysql");
    let err = run(&conflict).expect_err("family");
    assert!(err.contains("provenance.conflict"), "{err}");
}

fn packed_command(catalog: &Value, identity: &str, inclusion: bool) -> Value {
    let mut constraint = json!({
        "schema": "erp",
        "table": "parcel",
        "name": "parcel_box_fk",
        "kind": "foreign_key",
        "columns": ["units"],
        "trust": "declared_only"
    });
    if inclusion {
        constraint["inclusion"] = json!({"rows_checked": 40, "ratio_thousandths": 990});
    }
    let decisions = [
        ("id", "identifier", None),
        ("amount", "money", Some("USD")),
        ("flag", "boolean_flag", None),
        ("lane", "category", None),
        ("units", "quantity", None),
    ];
    let mut rows = Vec::new();
    for (column, semantic, unit) in decisions {
        let mut row = json!({
            "dataset": "erp.parcel",
            "column": column,
            "pii": "none",
            "semantic_type": semantic,
            "reviewer": {"by": "ada.owner", "at": "2026-10-06"}
        });
        if let Some(unit) = unit {
            row["unit"] = json!(unit);
        }
        rows.push(row);
    }
    json!({
        "protocol": "oo.source-semantics.command/v1",
        "catalog": catalog,
        "target": {
            "family": "mysql",
            "product": "mysql",
            "version": "8.0",
            "capture": "sealed_import",
            "source_consistency": "B",
            "delete_detection": "reconcile",
            "source_kind": "mysql_table",
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
            "constraints": [constraint]
        },
        "binding": {
            "settings": settings(identity),
            "decisions": rows,
            "visibility": {
                "protocol": "oo.builder.role-visibility/v1",
                "project": "shop",
                "principal": "ada",
                "role": "oo_r_clerk",
                "ontology_version": "ont-1",
                "catalog_version": identity,
                "policy": {"id": "pol-shop", "version": "1"},
                "knowledge": {"id": "know-shop", "version": "1"},
                "issued_for_compiler": "oo-builder/role-pack/v1",
                "datasets": [{
                    "dataset": "erp.parcel",
                    "row_security": "none",
                    "global_stats": "denied",
                    "columns": ["id", "amount", "flag", "lane", "units"]
                }],
                "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
            },
            "examples": [
                {
                    "id": "amount-present",
                    "question": "rows with an amount",
                    "sql": "select amount from erp.parcel where amount is not null",
                    "uses": ["dataset:erp.parcel"],
                    "verified_by": "pg-gateway",
                    "verified_at": "2026-10-09"
                },
                {
                    "id": "source-nvl",
                    "question": "skipped",
                    "sql": "select NVL(amount, 0) from erp.parcel",
                    "verified_by": "pg-gateway",
                    "verified_at": "2026-10-09"
                }
            ]
        }
    })
}

fn settings(identity: &str) -> Value {
    json!({
        "project": "shop",
        "ontology_version": "ont-1",
        "catalog_version": identity,
        "policy": {"id": "pol-shop", "version": "1"},
        "knowledge": {"id": "know-shop", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "shop",
            "locale": "en-US",
            "include_schemas": ["erp"],
            "default_tier_preference": ["raw"],
            "publish": {"include_unreviewed": []},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 4000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        }
    })
}

fn schema_digest(tables: &Value) -> String {
    let mut out = String::new();
    for table in tables.as_array().unwrap() {
        oo_core::sealed_import::push_schema_table(
            &mut out,
            table["logical_schema"].as_str().unwrap(),
            table["logical_table"].as_str().unwrap(),
        );
        for column in table["columns"].as_array().unwrap() {
            oo_core::sealed_import::push_schema_column(
                &mut out,
                column["logical_name"].as_str().unwrap(),
                column["oid"].as_i64().unwrap(),
                i32::try_from(column["typmod"].as_i64().unwrap()).unwrap(),
                column["caveat"].as_str(),
            );
        }
    }
    oo_core::api::payload_sha256(out.as_bytes())
}

#[test]
fn captured_oracle_bpchar_catalog_packs_without_rewriting_the_source_type() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/oo04-oracle-sealed-import-catalog.json");
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes.len(), 3754);
    assert_eq!(
        oo_core::api::payload_sha256(&bytes),
        "75baa479e1a79cfff11126ce55d11c667f0b69231179d18e863b4a2e666269d6"
    );
    let text = std::fs::read_to_string(&path).unwrap();
    let pg = oo_builder::compile_captured_pg_catalog(&text, None, None).expect_err("pg abi");
    assert!(pg.code.ends_with("/field.unknown"));
    assert_eq!(pg.message, "catalog evidence is not oo.source.pgsql/v1");
    let catalog: Value = serde_json::from_str(&text).unwrap();
    let draft = oo_builder::compile_sealed_import_catalog(&text).expect("compiler");
    assert_eq!(draft.source_engine, "oracle");
    assert_eq!(draft.source_id, "oo04_t_batch");
    assert_eq!(draft.schema_digest, "59a1d30cfedb66d93fa8277124851ed6fcb3504c697688ddcf92dcadfcc289dc");
    let c6 = draft
        .columns
        .iter()
        .find(|column| column.dataset == "batch.t_batch" && column.name == "c6")
        .expect("c6");
    assert_eq!(c6.target_type, "bpchar");
    assert_eq!(c6.target_oid, 1042);
    assert_eq!(c6.target_typmod, 10);
    assert_eq!(c6.target_typmod - 4, 6);
    assert!(c6.source_type.is_none());
    assert!(!c6.precision_loss);
    let result = run(&oracle_packed(&catalog, &draft.identity_hash)).expect("packed");
    assert_eq!(result["status"], "packed");
    assert_eq!(
        result["builder_calls"],
        json!([
            "compile_sealed_import_catalog",
            "compile_reviewed_sealed_import",
            "compile_role_pack"
        ])
    );
    assert_eq!(result["catalog_proof"]["status"], "accepted");
    assert_eq!(result["catalog_proof"]["source_engine"], "oracle");
    assert_eq!(result["catalog_proof"]["source_id"], "oo04_t_batch");
    assert_eq!(result["catalog_proof"]["schema_digest"], draft.schema_digest);
    assert_eq!(result["live_database"], "not_connected");
    assert_eq!(result["freshness_projection"]["gateway_get_freshness"], "not_called");
    let native = result["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|column| column["column"] == "batch.t_batch.c6")
        .unwrap();
    assert_eq!(native["native_type"], "absent");
    assert!(native["native_note"].is_null());
    let page = result["pack"]["pages"]["tables/batch.t_batch.md"].as_str().unwrap();
    assert!(page.contains("| `c6` |  | bpchar | code | none |  |"));
    assert!(!page.contains("| `c6` |  | text |"));
    assert!(!page.contains("| `c6` |  | varchar |"));
    assert!(page.contains("primary key: not visible"));
    assert!(page.contains("## 来源与语义"));
    assert!(page.contains("postgresql_target_storage"));
    assert!(page.contains("family: oracle"));
    let readme = result["pack"]["readme"].as_str().unwrap();
    assert!(readme.contains("所有 SQL 为 PostgreSQL 方言，无论数据来自何种源库"));
    let verified = result["pack"]["verified_sql"].as_str().unwrap();
    assert!(verified.contains("select c6 from batch.t_batch where c6 is not null"));
    assert!(!verified.contains("NVL"));
    let rejected: Vec<&str> = result["examples_rejected"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect();
    assert!(rejected.contains(&"source-nvl"));
    let effective = result["role_pack_inputs"]["effective_json"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(effective).unwrap();
    assert_eq!(parsed["catalog_version"], draft.identity_hash);
    let file = parsed["datasets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["dataset"] == "batch.t_batch")
        .unwrap();
    let column = file["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "c6")
        .unwrap();
    assert_eq!(column["data_type"], "bpchar");
    assert_eq!(column["semantic_type"], "code");
    let mut named = catalog.clone();
    set_pg_column(&mut named, "t_batch", "c6", "source_type", json!("bpchar"));
    let named_draft = oo_builder::compile_sealed_import_catalog(&named.to_string()).expect("named");
    let named_c6 = named_draft.columns.iter().find(|column| column.name == "c6").unwrap();
    assert_eq!(named_c6.source_type.as_deref(), Some("bpchar"));
    assert_eq!(named_c6.target_type, "bpchar");
    assert_eq!(named_c6.target_typmod, 10);
    assert_eq!(named_draft.identity_hash, draft.identity_hash);
    let named_result = run(&oracle_packed(&named, &named_draft.identity_hash)).expect("named pack");
    let named_native = named_result["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|column| column["column"] == "batch.t_batch.c6")
        .unwrap();
    assert_eq!(named_native["native_type"], "bpchar");
    for source in ["text", "varchar"] {
        let mut wrong = catalog.clone();
        set_pg_column(&mut wrong, "t_batch", "c6", "source_type", json!(source));
        let err = run(&oracle_packed(&wrong, &draft.identity_hash)).expect_err(source);
        assert!(err.contains("type.ambiguous"), "{err}");
    }
    let mut unknown = catalog.clone();
    set_pg_column(&mut unknown, "t_batch", "c6", "oid", json!(1041));
    unknown["schema_digest"] = json!(schema_digest(&unknown["tables"]));
    let err = run(&oracle_packed(&unknown, &draft.identity_hash)).expect_err("oid");
    assert!(err.contains("type.ambiguous"), "{err}");
    let mut typmod = catalog.clone();
    set_pg_column(&mut typmod, "t_batch", "c6", "typmod", json!(4));
    typmod["schema_digest"] = json!(schema_digest(&typmod["tables"]));
    let err = run(&oracle_packed(&typmod, &draft.identity_hash)).expect_err("typmod");
    assert!(err.contains("type.ambiguous"), "{err}");
    let mut digest = catalog.clone();
    digest["schema_digest"] = json!("ab".repeat(32));
    let err = run(&oracle_packed(&digest, &draft.identity_hash)).expect_err("digest");
    assert!(err.contains("digest.mismatch"), "{err}");
    let mut stale_id = catalog.clone();
    set_pg_column(&mut stale_id, "t_batch", "id", "oid", json!(23));
    let err = run(&oracle_packed(&stale_id, &draft.identity_hash)).expect_err("stale id");
    assert!(err.contains("digest.mismatch"), "{err}");
    let mut retired = catalog.clone();
    retired["state"] = json!("retired");
    let err = run(&oracle_packed(&retired, &draft.identity_hash)).expect_err("seal");
    assert!(err.contains("seal.unsealed"), "{err}");
    let mut stale_version = oracle_packed(&catalog, &draft.identity_hash);
    stale_version["binding"]["settings"]["catalog_version"] = json!("cat-1");
    let err = run(&stale_version).expect_err("stale version");
    assert!(err.contains("binding.catalog_version"), "{err}");
    for engine in ["mysql", "clickhouse", "mongodb", "postgresql"] {
        let mut copy = catalog.clone();
        copy["engine"] = json!(engine);
        let compiled = oo_builder::compile_sealed_import_catalog(&copy.to_string()).expect(engine);
        assert_eq!(compiled.source_engine, engine);
        assert_ne!(compiled.identity_hash, draft.identity_hash);
        let copied = compiled.columns.iter().find(|column| column.name == "c6").unwrap();
        assert_eq!(copied.target_type, "bpchar");
        assert_eq!(copied.target_oid, 1042);
        assert_eq!(copied.target_typmod, 10);
    }
    let again = std::fs::read(&path).unwrap();
    assert_eq!(again, bytes);
}

fn oracle_packed(catalog: &Value, identity: &str) -> Value {
    json!({
        "protocol": "oo.source-semantics.command/v1",
        "catalog": catalog,
        "target": {
            "family": catalog["engine"].as_str().unwrap_or("oracle"),
            "product": catalog["engine"].as_str().unwrap_or("oracle"),
            "version": "19",
            "capture": "sealed_import",
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
        },
        "binding": {
            "settings": oracle_settings(identity),
            "decisions": [{
                "dataset": "batch.t_batch",
                "column": "c6",
                "pii": "none",
                "semantic_type": "code",
                "reviewer": {"by": "ada.owner", "at": "2026-10-11"}
            }],
            "visibility": {
                "protocol": "oo.builder.role-visibility/v1",
                "project": "batch",
                "principal": "ada",
                "role": "oo_r_clerk",
                "ontology_version": "ont-1",
                "catalog_version": identity,
                "policy": {"id": "pol-batch", "version": "1"},
                "knowledge": {"id": "know-batch", "version": "1"},
                "issued_for_compiler": "oo-builder/role-pack/v1",
                "datasets": [{
                    "dataset": "batch.t_batch",
                    "row_security": "none",
                    "global_stats": "denied",
                    "columns": ["c6"]
                }],
                "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
            },
            "examples": [
                {
                    "id": "c6-present",
                    "question": "present rows",
                    "sql": "select c6 from batch.t_batch where c6 is not null",
                    "uses": ["dataset:batch.t_batch"],
                    "verified_by": "pg-gateway",
                    "verified_at": "2026-10-11"
                },
                {
                    "id": "source-nvl",
                    "question": "skipped",
                    "sql": "select NVL(c6, c6) from batch.t_batch",
                    "verified_by": "pg-gateway",
                    "verified_at": "2026-10-11"
                }
            ]
        }
    })
}

#[test]
fn jsonb_3802_omits_a_document_until_raw_bson_is_sealed_into_the_page() {
    let raw = include_bytes!("../../../crates/oo-core/tests/fixtures/jsonb3802/canonical-bson.bin");
    let pg = include_bytes!("../../../crates/oo-core/tests/fixtures/jsonb3802/canonical.bin");
    let text = include_str!("../../../crates/oo-core/tests/fixtures/jsonb3802/canonical.txt");
    let catalog = docs_catalog("mongodb", 23, 3802, -1, Some("jsonb"));
    let draft = oo_builder::compile_sealed_import_catalog(&catalog.to_string()).expect("catalog");
    let plain = docs_command(&catalog, &draft.identity_hash, None);
    let result = run(&plain).expect("type only");
    assert_eq!(result["status"], "packed");
    assert!(result.get("document_profile").is_none());
    assert_eq!(
        result["builder_calls"],
        json!([
            "compile_sealed_import_catalog",
            "compile_reviewed_sealed_import",
            "compile_role_pack"
        ])
    );
    let page = result["pack"]["pages"]["tables/public.docs.md"].as_str().unwrap();
    assert!(page.contains("| `doc` |  | jsonb | json | none |  |"));
    assert!(page.contains("| `id` |  | int4 | identifier | none |  |"));
    assert!(!page.contains("json_not_jsonb"));
    assert!(!page.contains("document_storage:"));
    let native = result["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|column| column["column"] == "public.docs.doc")
        .unwrap();
    assert_eq!(native["native_type"], "jsonb");
    let effective: Value = serde_json::from_str(result["role_pack_inputs"]["effective_json"].as_str().unwrap()).unwrap();
    let column = effective["datasets"][0]["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "doc")
        .unwrap();
    assert_eq!(column["data_type"], "jsonb");
    assert_eq!(column["semantic_type"], "json");
    let document = bson_document(raw, pg, text);
    let sealed = run(&docs_command(&catalog, &draft.identity_hash, Some(document.clone()))).expect("sealed");
    assert_eq!(sealed["status"], "packed");
    assert_eq!(
        sealed["builder_calls"],
        json!([
            "compile_sealed_import_catalog",
            "compile_reviewed_sealed_import",
            "compile_role_pack",
            "compile_jsonb_document_profile"
        ])
    );
    let profile = &sealed["document_profile"];
    assert_eq!(profile["protocol"], "oo.builder.document-jsonb-profile/v1");
    assert_eq!(profile["compiler"], "compile_jsonb_document_profile");
    assert_eq!(profile["source_engine"], "mongodb");
    assert_eq!(profile["normalized_representation"], "postgresql_target_storage");
    assert_eq!(profile["schema_digest"], draft.schema_digest);
    assert_eq!(profile["extended_json"]["d"]["$numberDecimal"], "1.2300");
    assert_eq!(profile["extended_json"]["fits"]["$numberLong"], "5");
    assert_eq!(profile["extended_json"]["big"]["$numberLong"], "9223372036854775807");
    assert_eq!(profile["arrow"]["metadata"]["oo.storage.oid"], "3802");
    assert_eq!(profile["arrow"]["metadata"]["oo.document.missing"], "absent_not_null");
    assert_eq!(profile["precision"]["float"], "rejected");
    assert_eq!(profile["quality"]["loss_concealed"], false);
    let sealed_page = sealed["pack"]["pages"]["tables/public.docs.md"].as_str().unwrap();
    assert!(sealed_page.contains("document_storage: jsonb"));
    assert!(sealed_page.contains("document_oid: 3802"));
    assert!(sealed_page.contains("document_typmod: -1"));
    assert!(sealed_page.contains("document_kind: bson_extended_json"));
    assert!(sealed_page.contains("missing: absent_not_null"));
    assert!(sealed_page.contains("uuid: rfc4122_lowercase"));
    assert!(sealed_page.contains("date: unix_millis_utc"));
    assert!(sealed_page.contains("bytes: base64_subtype"));
    assert!(sealed_page.contains("decimal: exact_decimal_string"));
    assert!(sealed_page.contains("double: canonical_number_double"));
    assert!(sealed_page.contains("double_negative_zero: -0.0"));
    assert!(sealed_page.contains("double_non_finite: NaN|Infinity|-Infinity"));
    assert!(sealed_page.contains("double_nan_payload: not_preserved"));
    assert!(sealed_page.contains("field_scope: unchecked"));
    assert!(sealed_page.contains("float: rejected"));
    let mut root_names: Vec<String> = profile["extended_json"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    root_names.sort();
    assert!(sealed_page.contains(&format!("document_root_keys: {}", root_names.join(","))));
    assert!(!sealed_page.lines().any(|line| line.starts_with("weight:")));
    assert!(sealed_page.contains(&format!("document_raw_bson_sha256: {}", profile["raw_bson_sha256"].as_str().unwrap())));
    assert!(sealed_page.contains(&format!("document_pg_jsonb_sha256: {}", profile["pg_jsonb_sha256"].as_str().unwrap())));
    assert!(!sealed_page.contains("NVL"));
    let mut dropped = document.clone();
    dropped["raw_bson_dropped"] = json!(true);
    dropped["raw_bson_hex"] = json!("");
    let err = run(&docs_command(&catalog, &draft.identity_hash, Some(dropped.clone()))).expect_err("dropped");
    assert!(err.contains("raw_bson_dropped"), "{err}");
    let bad = docs_catalog("mongodb", 23, 3803, -1, None);
    let err = run(&docs_command(&bad, &draft.identity_hash, Some(dropped))).expect_err("bad oid");
    assert!(err.contains("type.ambiguous"), "{err}");
    assert!(!err.contains("raw_bson_dropped"), "{err}");
    let mut as_json = document.clone();
    as_json["oid"] = json!(114);
    let err = run(&docs_command(&catalog, &draft.identity_hash, Some(as_json))).expect_err("114");
    assert!(err.contains("json114_not_jsonb"), "{err}");
    let json_column = docs_catalog("mongodb", 23, 114, -1, Some("json"));
    let json_draft = oo_builder::compile_sealed_import_catalog(&json_column.to_string()).expect("json");
    assert_eq!(json_draft.columns.iter().find(|column| column.name == "doc").unwrap().target_type, "json");
    let err = run(&docs_command(&json_column, &json_draft.identity_hash, Some(document.clone()))).expect_err("json column");
    assert!(err.contains("document column is not jsonb oid 3802 typmod -1"), "{err}");
    let loss = docs_catalog("mongodb", 700, 3802, -1, Some("jsonb"));
    let blocked = docs_command_target(&loss, None);
    let blocked_result = run(&blocked).expect("blocked");
    assert_eq!(blocked_result["status"], "blocked");
    assert_eq!(blocked_result["builder_calls"], json!(["compile_sealed_import_catalog"]));
    assert!(blocked_result.get("document_profile").is_none());
    let refused = docs_command_target(&loss, Some(document));
    let err = run(&refused).expect_err("refused document");
    assert!(err.contains("document.refused"), "{err}");
    assert!(!err.contains("raw_bson_dropped"), "{err}");
}

#[test]
fn jsonb_page_prints_a_present_number_double_and_leaves_a_missing_key_absent() {
    // The send file is the version byte plus this decoder's JSON text. It is
    // not a PostgreSQL jsonb_send capture.
    let raw = append_root_double(&[5, 0, 0, 0, 0], "weight", 41.25_f64.to_bits());
    let extended = oo_core::jsonb_doc::bson_to_extended_json(&raw).expect("weight");
    assert_eq!(extended["weight"]["$numberDouble"], "41.25");
    assert!(extended.get("code").is_none());
    let text = serde_json::to_string(&extended).expect("text");
    let mut pg = vec![oo_core::jsonb_doc::JSONB_SEND_VERSION];
    pg.extend(text.as_bytes());
    let mut document = bson_document(&raw, &pg, &text);
    document["approved_fields"] = json!(["_id", "code", "weight", "whCode"]);
    let catalog = docs_catalog("mongodb", 23, 3802, -1, Some("jsonb"));
    let draft = oo_builder::compile_sealed_import_catalog(&catalog.to_string()).expect("catalog");
    let sealed = run(&docs_command(&catalog, &draft.identity_hash, Some(document))).expect("page");
    let page = sealed["pack"]["pages"]["tables/public.docs.md"].as_str().unwrap();
    assert!(page.contains("double: canonical_number_double"));
    assert!(page.contains("field_scope: approved_root_keys"));
    assert!(page.contains("document_root_keys: weight"));
    assert!(page.contains("weight: {\"$numberDouble\":\"41.25\"}"));
    assert!(!page.lines().any(|line| line.starts_with("code:")));
    assert_eq!(sealed["document_profile"]["extended_json"]["weight"]["$numberDouble"], "41.25");
}

fn append_root_double(raw: &[u8], key: &str, bits: u64) -> Vec<u8> {
    assert_eq!(raw.last().copied(), Some(0));
    let mut body = raw[4..raw.len() - 1].to_vec();
    body.push(0x01);
    body.extend(key.as_bytes());
    body.push(0);
    body.extend(bits.to_le_bytes());
    body.push(0);
    let mut out = i32::try_from(body.len() + 4).unwrap().to_le_bytes().to_vec();
    out.extend(body);
    out
}

fn docs_catalog(engine: &str, id_oid: i64, doc_oid: i64, doc_typmod: i32, source_type: Option<&str>) -> Value {
    let mut preimage = String::new();
    oo_core::sealed_import::push_schema_table(&mut preimage, "public", "docs");
    oo_core::sealed_import::push_schema_column(&mut preimage, "id", id_oid, -1, None);
    oo_core::sealed_import::push_schema_column(&mut preimage, "doc", doc_oid, doc_typmod, None);
    let schema_digest = oo_core::api::payload_sha256(preimage.as_bytes());
    let digest = "a".repeat(64);
    json!({
        "capture": "sealed_import",
        "config_digest": digest,
        "engine": engine,
        "hook_checked": false,
        "input_digest": digest,
        "protocol": "oo.pipeline.sealed-import-catalog/v1",
        "quality": {
            "loss_concealed": false,
            "manifest_reject_count": 0,
            "payload_includes_reject_lines": false,
            "stored_omits_manifest_rejects": false,
            "stored_rejected_rows": 0,
            "stored_rows": [{
                "accepted_rows": 1,
                "attnum": null,
                "batch_id": "t0.b0",
                "key_digest": null,
                "kind": "batch",
                "physical_name": format!("pt_{}", "44".repeat(30)),
                "reason": null,
                "rejected_rows": 0,
                "row_index": null,
                "value_len": null
            }]
        },
        "run_id": "beecbbc7-8dda-4779-81ba-6ff0383f72ec",
        "schema_digest": schema_digest,
        "shadows_queried": false,
        "source_generation": 1,
        "source_id": "mongo_docs",
        "source_introspection": "not_used",
        "state": "awaiting_publish",
        "tables": [{
            "accepted_rows": 1,
            "canonical_digest": digest,
            "columns": [
                {
                    "caveat": null,
                    "key": true,
                    "logical_name": "id",
                    "oid": id_oid,
                    "source_type": null,
                    "stored_physical": format!("pc_{}", "11".repeat(30)),
                    "typmod": -1
                },
                {
                    "caveat": null,
                    "key": false,
                    "logical_name": "doc",
                    "oid": doc_oid,
                    "source_type": source_type,
                    "stored_physical": format!("pc_{}", "22".repeat(30)),
                    "typmod": doc_typmod
                }
            ],
            "logical_schema": "public",
            "logical_table": "docs",
            "rejects": [],
            "stored_namespace": format!("pn_{}", "33".repeat(30)),
            "stored_physical": format!("pt_{}", "44".repeat(30))
        }],
        "target_generation": 1,
        "tool_hook_digest": digest,
        "type_digest": digest
    })
}

fn bson_document(raw: &[u8], pg: &[u8], text: &str) -> Value {
    json!({
        "protocol": "oo.document.bson-jsonb/v1",
        "dataset": "public.docs",
        "column": "doc",
        "oid": 3802,
        "typmod": -1,
        "source_type": "jsonb",
        "raw_bson_dropped": false,
        "raw_bson_hex": docs_hex(raw),
        "raw_bson_sha256": oo_core::api::payload_sha256(raw),
        "pg_jsonb_hex": docs_hex(pg),
        "pg_jsonb_text": text,
        "loss_concealed": false,
        "missing_is_null": false
    })
}

fn docs_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn docs_command(catalog: &Value, identity: &str, document: Option<Value>) -> Value {
    let mut command = docs_command_target(catalog, document);
    command["binding"] = json!({
        "settings": docs_settings(identity),
        "decisions": [
            {"dataset": "public.docs", "column": "id", "pii": "none", "semantic_type": "identifier", "reviewer": {"by": "ada.owner", "at": "2026-10-11"}},
            {"dataset": "public.docs", "column": "doc", "pii": "none", "semantic_type": "json", "reviewer": {"by": "ada.owner", "at": "2026-10-11"}}
        ],
        "visibility": {
            "protocol": "oo.builder.role-visibility/v1",
            "project": "docs",
            "principal": "ada",
            "role": "oo_r_clerk",
            "ontology_version": "ont-1",
            "catalog_version": identity,
            "policy": {"id": "pol-docs", "version": "1"},
            "knowledge": {"id": "know-docs", "version": "1"},
            "issued_for_compiler": "oo-builder/role-pack/v1",
            "datasets": [{
                "dataset": "public.docs",
                "row_security": "none",
                "global_stats": "denied",
                "columns": ["id", "doc"]
            }],
            "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
        },
        "examples": [{
            "id": "doc-present",
            "question": "present rows",
            "sql": "select id from public.docs where id is not null",
            "uses": ["dataset:public.docs"],
            "verified_by": "pg-gateway",
            "verified_at": "2026-10-11"
        }]
    });
    command
}

fn docs_command_target(catalog: &Value, document: Option<Value>) -> Value {
    let mut command = json!({
        "protocol": "oo.source-semantics.command/v1",
        "catalog": catalog,
        "target": {
            "family": "mongodb",
            "product": "mongodb",
            "version": "7.0",
            "capture": "sealed_import",
            "source_consistency": "B",
            "delete_detection": "reconcile",
            "source_kind": "mongodb_collection",
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
        }
    });
    if let Some(document) = document {
        command["document"] = document;
    }
    command
}

fn docs_settings(identity: &str) -> Value {
    json!({
        "project": "docs",
        "ontology_version": "ont-1",
        "catalog_version": identity,
        "policy": {"id": "pol-docs", "version": "1"},
        "knowledge": {"id": "know-docs", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "docs",
            "locale": "en-US",
            "include_schemas": ["public"],
            "default_tier_preference": ["raw"],
            "publish": {"include_unreviewed": []},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 4000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        }
    })
}

fn oracle_settings(identity: &str) -> Value {
    json!({
        "project": "batch",
        "ontology_version": "ont-1",
        "catalog_version": identity,
        "policy": {"id": "pol-batch", "version": "1"},
        "knowledge": {"id": "know-batch", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "batch",
            "locale": "en-US",
            "include_schemas": ["batch"],
            "default_tier_preference": ["raw"],
            "publish": {"include_unreviewed": []},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 4000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        }
    })
}
