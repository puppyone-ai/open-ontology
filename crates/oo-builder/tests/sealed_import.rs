//! Producer catalog to the sealed-import compiler.
//!
//! The fixture is the captured `oo.pipeline.sealed-import-catalog/v1` document.
//! These tests do not open a database, call a model, or relabel the document
//! as `oo.source.pgsql/v1`.

use serde_json::{Value, json};

const FILE_SHA256: &str = "d9af109ea2f8cd63497771d960a8c396b959501b575b07aa9668fb04a61db844";
const SCHEMA_DIGEST: &str = "bac1dde9dc73fc98cf5531621818af2a5d64832a25b6dd60fb52d76942f5cea8";
const SHADOW: &str = "pt_9cb24f0ac8a48d5f4f4f6d93f5ebe7f2c4162c9ae5237040365d3f2047ce";

fn fixture() -> &'static str {
    include_str!("fixtures/w06-sealed-import-catalog.json")
}

fn compile(text: &str) -> oo_builder::SealedImportDraft {
    oo_builder::compile_sealed_import_catalog(text).expect("sealed catalog")
}

#[test]
fn captured_mysql_catalog_compiles_without_becoming_a_pg_source() {
    let bytes = include_bytes!("fixtures/w06-sealed-import-catalog.json");
    assert_eq!(oo_core::api::payload_sha256(bytes), FILE_SHA256);
    let draft = compile(fixture());
    assert_eq!(draft.schema_digest, SCHEMA_DIGEST);
    assert_eq!(draft.source_engine, "mysql");
    assert_eq!(draft.normalized_representation, "postgresql_target_storage");
    assert_eq!(draft.protocol, "oo.builder.sealed-import-draft/v1");
    assert_eq!(draft.compiler, "oo-builder/sealed-import-draft/v1");
    assert_eq!(draft.source_id, "mysqlpreview");
    assert_eq!(draft.source_generation, 1);
    assert_eq!(draft.target_generation, 3);
    assert!(!draft.document.contains(SHADOW));
    assert!(!draft.document.contains("pc_"));
    assert!(!draft.document.contains("pn_"));
    assert!(!draft.document.contains("oo.source.pgsql/v1"));
    assert!(draft.document.contains("oo.pipeline.sealed-import-catalog/v1"));
    assert!(draft.document.contains("\"source_connection\":\"not_used\""));
    assert!(draft.document.contains("\"live_database\":\"not_connected\""));
    let amount = draft
        .columns
        .iter()
        .find(|column| column.dataset == "erp.parcel" && column.name == "amount")
        .expect("amount");
    assert_eq!(amount.target_type, "numeric");
    assert_eq!(amount.target_oid, 1700);
    assert_eq!(amount.target_typmod, 786438);
    assert!(!amount.precision_loss);
    assert!(!amount.key);
    let flag = draft
        .columns
        .iter()
        .find(|column| column.name == "flag")
        .expect("flag");
    assert_eq!(flag.caveat.as_deref(), Some("tinyint1_as_int2"));
    assert_eq!(flag.target_type, "int2");
    let rate = draft
        .columns
        .iter()
        .find(|column| column.name == "rate")
        .expect("rate");
    assert!(rate.precision_loss);
    assert!(!rate.key);
    let again = compile(fixture());
    assert_eq!(again.identity_hash, draft.identity_hash);
    assert_eq!(
        again.fact_fingerprint("erp.parcel", "amount"),
        Some(amount.fact_fingerprint.as_str())
    );
}

#[test]
fn pg_catalog_entry_still_refuses_the_sealed_document() {
    let err = oo_builder::compile_captured_pg_catalog(fixture(), None, None).expect_err("pg");
    assert!(err.code.ends_with("/field.unknown"), "{}", err.code);
    assert_eq!(err.message, "catalog evidence is not oo.source.pgsql/v1");
}

#[test]
fn engine_is_not_the_success_switch_and_a_changed_digest_changes_identity() {
    let mysql = compile(fixture());
    let mut oracle = serde_json::from_str::<Value>(fixture()).unwrap();
    oracle["engine"] = json!("oracle");
    let oracle = compile(&oracle.to_string());
    assert_eq!(oracle.source_engine, "oracle");
    assert_eq!(oracle.normalized_representation, "postgresql_target_storage");
    assert_eq!(oracle.schema_digest, mysql.schema_digest);
    assert_ne!(oracle.identity_hash, mysql.identity_hash);
    let mut changed = serde_json::from_str::<Value>(fixture()).unwrap();
    let digest = changed["input_digest"].as_str().unwrap().to_string();
    let mut chars: Vec<u8> = digest.into_bytes();
    chars[0] = if chars[0] == b'a' { b'b' } else { b'a' };
    changed["input_digest"] = json!(String::from_utf8(chars).unwrap());
    let changed = compile(&changed.to_string());
    assert_ne!(changed.identity_hash, mysql.identity_hash);
    assert_eq!(changed.schema_digest, mysql.schema_digest);
}

#[test]
fn wrong_digest_unsealed_ambiguous_sql_and_precision_key_fail() {
    let mut wrong = serde_json::from_str::<Value>(fixture()).unwrap();
    wrong["schema_digest"] = json!("ab".repeat(32));
    let err = oo_builder::compile_sealed_import_catalog(&wrong.to_string()).expect_err("digest");
    assert!(err.code.ends_with("/digest.mismatch"), "{}", err.code);
    wrong = serde_json::from_str(fixture()).unwrap();
    wrong["state"] = json!("retired");
    let err = oo_builder::compile_sealed_import_catalog(&wrong.to_string()).expect_err("state");
    assert!(err.code.ends_with("/seal.unsealed"), "{}", err.code);
    wrong = serde_json::from_str(fixture()).unwrap();
    wrong["source_generation"] = json!(0);
    let err = oo_builder::compile_sealed_import_catalog(&wrong.to_string()).expect_err("stale");
    assert!(err.code.ends_with("/seal.stale"), "{}", err.code);
    wrong = serde_json::from_str(fixture()).unwrap();
    wrong.as_object_mut().unwrap().insert("password_file".to_string(), json!("x"));
    let err = oo_builder::compile_sealed_import_catalog(&wrong.to_string()).expect_err("secret");
    assert!(err.code.ends_with("/secret.rejected"), "{}", err.code);
    wrong = serde_json::from_str(fixture()).unwrap();
    wrong["tables"][0]["columns"][0]["source_type"] = json!("varchar(10)");
    let err = oo_builder::compile_sealed_import_catalog(&wrong.to_string()).expect_err("sql");
    assert!(err.code.ends_with("/sql.rejected"), "{}", err.code);
    wrong = serde_json::from_str(fixture()).unwrap();
    wrong["tables"][0]["columns"][2]["oid"] = json!(23);
    let err = oo_builder::compile_sealed_import_catalog(&wrong.to_string()).expect_err("caveat");
    assert!(err.code.ends_with("/type.ambiguous"), "{}", err.code);
    let mut lossy = serde_json::from_str::<Value>(fixture()).unwrap();
    lossy["tables"][0]["columns"][0]["oid"] = json!(700);
    lossy["schema_digest"] = json!(schema_digest(&lossy["tables"]));
    let err = oo_builder::compile_sealed_import_catalog(&lossy.to_string()).expect_err("key");
    assert!(err.code.ends_with("/precision_loss_key"), "{}", err.code);
}

#[test]
fn reviewed_import_keeps_money_unit_and_feeds_the_role_pack() {
    let draft = compile(fixture());
    let reviewed = oo_builder::compile_reviewed_sealed_import(&command(&draft, Some("USD")))
        .expect("reviewed");
    let reviewed: Value = serde_json::from_str(&reviewed).unwrap();
    assert_eq!(reviewed["protocol"], "oo.builder.reviewed-sealed-import-command/v1");
    assert_eq!(reviewed["provenance"]["source_engine"], "mysql");
    assert_eq!(reviewed["provenance"]["normalized_representation"], "postgresql_target_storage");
    assert_eq!(reviewed["provenance"]["schema_digest"], SCHEMA_DIGEST);
    assert_eq!(reviewed["provenance"]["identity_hash"], draft.identity_hash);
    assert!(reviewed["provenance"].get("connector_protocol").is_none());
    assert!(!reviewed.to_string().contains(SHADOW));
    let effective = &reviewed["effective_oos"];
    assert_eq!(effective["protocol"], "oo.builder.effective-oos/v1");
    assert_eq!(effective["catalog_version"], draft.identity_hash);
    let parcel = effective["datasets"]
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
    assert_eq!(amount["provenance"], "generated");
    let visibility = visibility(&draft.identity_hash);
    let pack = oo_builder::compile_role_pack(
        &serde_json::to_string(effective).unwrap(),
        &serde_json::to_string(&visibility).unwrap(),
    )
    .expect("role pack");
    let page = pack.file("tables/erp.parcel.md").unwrap();
    assert!(page.contains("amount"));
    assert!(page.contains("USD"));
    assert!(!page.contains(SHADOW));
    assert!(!page.contains("pc_"));
    assert!(pack.cache_key.contains(&draft.identity_hash));
    let missing = oo_builder::compile_reviewed_sealed_import(&command(&draft, None)).expect_err("unit");
    assert!(missing.code.ends_with("/oos.invalid"), "{}", missing.code);
    let unknown = command(&draft, Some("unknown"));
    let unknown = oo_builder::compile_reviewed_sealed_import(&unknown).expect_err("unknown");
    assert!(unknown.code.ends_with("/decision.invalid"), "{}", unknown.code);
    let mut stale = serde_json::from_str::<Value>(&command(&draft, Some("USD"))).unwrap();
    stale["settings"]["catalog_version"] = json!("cat-1");
    let stale = oo_builder::compile_reviewed_sealed_import(&stale.to_string()).expect_err("stale");
    assert!(stale.code.ends_with("/binding.catalog_version"), "{}", stale.code);
}

const PG_FILE_SHA256: &str = "51ee055b40bc478308152b17d78e36ff2864e0c30df640d930553cdd75401cac";
const PG_SCHEMA_DIGEST: &str = "4a6806e71757ef4681145298a1c2305fe3ffc245340ada043ce290c890aa66bc";
const PG_CONFIG_DIGEST: &str = "26b5658d240e527fb3d27045f8909910b4fb4ab3ef2705de720ab6dc00ad2e4d";
const PG_INPUT_DIGEST: &str = "397d2eb1ac0fc87799fa033238533a6c20ee05cfa976226f54fddb96526924c7";
const PG_TYPE_DIGEST: &str = "b26ad0bc075bfc1e0f52264dc469b9f1d732d7fea29e2792fcd32688fbedb404";

fn pg_fixture() -> &'static str {
    include_str!("fixtures/oo06-pg-sealed-import-catalog.json")
}

#[test]
fn captured_postgresql_uuid_catalog_compiles_through_draft_oos_and_role_pack() {
    let bytes = include_bytes!("fixtures/oo06-pg-sealed-import-catalog.json");
    assert_eq!(bytes.len(), 8488);
    assert_eq!(oo_core::api::payload_sha256(bytes), PG_FILE_SHA256);
    let draft = oo_builder::compile_sealed_import_catalog(pg_fixture()).expect("pg catalog");
    assert_eq!(draft.schema_digest, PG_SCHEMA_DIGEST);
    assert_eq!(draft.config_digest, PG_CONFIG_DIGEST);
    assert_eq!(draft.input_digest, PG_INPUT_DIGEST);
    assert_eq!(draft.type_digest, PG_TYPE_DIGEST);
    assert_eq!(draft.source_engine, "postgresql");
    assert_eq!(draft.source_id, "oo_dl01w1_46");
    assert_eq!(draft.source_generation, 1);
    assert_eq!(draft.target_generation, 1);
    assert_eq!(draft.normalized_representation, "postgresql_target_storage");
    assert_eq!(draft.protocol, "oo.builder.sealed-import-draft/v1");
    assert_eq!(draft.compiler, "oo-builder/sealed-import-draft/v1");
    assert!(draft.document.contains("oo.pipeline.sealed-import-catalog/v1"));
    assert!(!draft.document.contains("oo.source.pgsql/v1"));
    assert!(!draft.document.contains("pc_"));
    assert!(!draft.document.contains("pn_"));
    assert!(!draft.document.contains("pt_"));
    let rows = [
        ("public.hubs", 2000_u64),
        ("public.shipments", 8000),
        ("public.tracking_events", 90000),
    ];
    for (dataset, accepted) in rows {
        let table = draft.tables.iter().find(|table| table.dataset == dataset).expect(dataset);
        assert_eq!(table.accepted_rows, accepted);
    }
    let tokens = [
        ("public.hubs", "hub_token"),
        ("public.shipments", "shipment_token"),
        ("public.tracking_events", "event_token"),
    ];
    for (dataset, name) in tokens {
        let column = draft
            .columns
            .iter()
            .find(|column| column.dataset == dataset && column.name == name)
            .expect(name);
        assert_eq!(column.target_type, "uuid");
        assert_eq!(column.target_oid, 2950);
        assert_eq!(column.target_typmod, -1);
        assert!(column.source_type.is_none());
        assert!(column.caveat.is_none());
        assert!(!column.key);
        assert!(!column.precision_loss);
    }
    let latitude = draft
        .columns
        .iter()
        .find(|column| column.dataset == "public.hubs" && column.name == "latitude")
        .expect("latitude");
    assert_eq!(latitude.target_type, "numeric");
    assert_eq!(latitude.target_oid, 1700);
    assert_eq!(latitude.target_typmod, 2490376);
    assert!(!latitude.precision_loss);
    let again = oo_builder::compile_sealed_import_catalog(pg_fixture()).expect("again");
    assert_eq!(again.identity_hash, draft.identity_hash);
    let refused = oo_builder::compile_captured_pg_catalog(pg_fixture(), None, None).expect_err("pg abi");
    assert!(refused.code.ends_with("/field.unknown"), "{}", refused.code);
    assert_eq!(refused.message, "catalog evidence is not oo.source.pgsql/v1");
    let reviewed = oo_builder::compile_reviewed_sealed_import(&pg_reviewed(&draft)).expect("reviewed");
    let reviewed: Value = serde_json::from_str(&reviewed).unwrap();
    assert_eq!(reviewed["protocol"], "oo.builder.reviewed-sealed-import-command/v1");
    assert_eq!(reviewed["provenance"]["source_engine"], "postgresql");
    assert_eq!(reviewed["provenance"]["source_id"], "oo_dl01w1_46");
    assert_eq!(reviewed["provenance"]["schema_digest"], PG_SCHEMA_DIGEST);
    assert_eq!(reviewed["provenance"]["identity_hash"], draft.identity_hash);
    assert_eq!(reviewed["provenance"]["normalized_representation"], "postgresql_target_storage");
    let effective = &reviewed["effective_oos"];
    assert_eq!(effective["protocol"], "oo.builder.effective-oos/v1");
    assert_eq!(effective["catalog_version"], draft.identity_hash);
    for (dataset, name) in tokens {
        let file = effective["datasets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["dataset"] == dataset)
            .expect(dataset);
        let column = file["columns"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["name"] == name)
            .expect(name);
        assert_eq!(column["data_type"], "uuid");
        assert_eq!(column["semantic_type"], "identifier");
        assert_eq!(column["provenance"], "generated");
        assert!(column.get("unit").is_none() || column["unit"].is_null());
    }
    let visibility = pg_visibility(&draft.identity_hash);
    let pack = oo_builder::compile_role_pack(
        &serde_json::to_string(effective).unwrap(),
        &serde_json::to_string(&visibility).unwrap(),
    )
    .expect("role pack");
    let page = pack.file("tables/public.hubs.md").unwrap();
    assert!(page.contains("| `hub_token` |  | uuid | identifier | none |  |"));
    assert!(!page.contains("| `hub_token` |  | text |"));
    let shipments = pack.file("tables/public.shipments.md").unwrap();
    assert!(shipments.contains("| `shipment_token` |  | uuid | identifier | none |  |"));
    let events = pack.file("tables/public.tracking_events.md").unwrap();
    assert!(events.contains("| `event_token` |  | uuid | identifier | none |  |"));
    assert!(pack.cache_key.contains(&draft.identity_hash));
    assert!(!pack.cache_key.contains("cat-1"));
    let mut oracle = serde_json::from_str::<Value>(pg_fixture()).unwrap();
    oracle["engine"] = json!("oracle");
    let oracle = oo_builder::compile_sealed_import_catalog(&oracle.to_string()).expect("oracle engine");
    assert_eq!(oracle.source_engine, "oracle");
    assert_eq!(oracle.normalized_representation, "postgresql_target_storage");
    assert_ne!(oracle.identity_hash, draft.identity_hash);
    let oracle_token = oracle
        .columns
        .iter()
        .find(|column| column.name == "hub_token")
        .expect("oracle hub_token");
    assert_eq!(oracle_token.target_type, "uuid");
    assert_eq!(oracle_token.target_oid, 2950);
}

#[test]
fn postgresql_uuid_catalog_rejects_type_and_digest_errors() {
    let mut wrong = serde_json::from_str::<Value>(pg_fixture()).unwrap();
    wrong["schema_digest"] = json!("ab".repeat(32));
    let err = oo_builder::compile_sealed_import_catalog(&wrong.to_string()).expect_err("digest");
    assert!(err.code.ends_with("/digest.mismatch"), "{}", err.code);
    wrong = serde_json::from_str(pg_fixture()).unwrap();
    set_pg_column(&mut wrong, "hubs", "hub_token", "source_type", json!("text"));
    let err = oo_builder::compile_sealed_import_catalog(&wrong.to_string()).expect_err("text");
    assert!(err.code.ends_with("/type.ambiguous"), "{}", err.code);
    assert_eq!(err.message, "source type token names a different normalized type");
    wrong = serde_json::from_str(pg_fixture()).unwrap();
    set_pg_column(&mut wrong, "hubs", "hub_token", "source_type", json!("uuid"));
    let kept = oo_builder::compile_sealed_import_catalog(&wrong.to_string()).expect("named uuid");
    let hub = kept
        .columns
        .iter()
        .find(|column| column.name == "hub_token")
        .expect("hub_token");
    assert_eq!(hub.source_type.as_deref(), Some("uuid"));
    assert_eq!(hub.target_type, "uuid");
    assert_eq!(hub.target_oid, 2950);
    let original = oo_builder::compile_sealed_import_catalog(pg_fixture()).unwrap();
    assert_eq!(kept.identity_hash, original.identity_hash);
    assert_ne!(hub.fact_fingerprint, original.fact_fingerprint("public.hubs", "hub_token").unwrap());
    wrong = serde_json::from_str(pg_fixture()).unwrap();
    set_pg_column(&mut wrong, "hubs", "hub_token", "oid", json!(2951));
    wrong["schema_digest"] = json!(schema_digest(&wrong["tables"]));
    let err = oo_builder::compile_sealed_import_catalog(&wrong.to_string()).expect_err("unknown oid");
    assert!(err.code.ends_with("/type.ambiguous"), "{}", err.code);
    assert_eq!(err.message, "native type has no normalized target representation");
    wrong = serde_json::from_str(pg_fixture()).unwrap();
    set_pg_column(&mut wrong, "hubs", "hub_id", "oid", json!(23));
    let err = oo_builder::compile_sealed_import_catalog(&wrong.to_string()).expect_err("stale digest");
    assert!(err.code.ends_with("/digest.mismatch"), "{}", err.code);
    wrong = serde_json::from_str(pg_fixture()).unwrap();
    set_pg_column(&mut wrong, "hubs", "hub_token", "typmod", json!(0));
    wrong["schema_digest"] = json!(schema_digest(&wrong["tables"]));
    let err = oo_builder::compile_sealed_import_catalog(&wrong.to_string()).expect_err("typmod");
    assert!(err.code.ends_with("/type.ambiguous"), "{}", err.code);
    wrong = serde_json::from_str(pg_fixture()).unwrap();
    wrong["state"] = json!("retired");
    let err = oo_builder::compile_sealed_import_catalog(&wrong.to_string()).expect_err("seal");
    assert!(err.code.ends_with("/seal.unsealed"), "{}", err.code);
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

fn pg_reviewed(draft: &oo_builder::SealedImportDraft) -> String {
    let tokens = [
        ("public.hubs", "hub_token"),
        ("public.shipments", "shipment_token"),
        ("public.tracking_events", "event_token"),
    ];
    let mut decisions = Vec::new();
    for (dataset, name) in tokens {
        let column = draft
            .columns
            .iter()
            .find(|column| column.dataset == dataset && column.name == name)
            .expect(name);
        decisions.push(json!({
            "column": name,
            "dataset": dataset,
            "fact_fingerprint": column.fact_fingerprint,
            "pii": "none",
            "reviewer": {"at": "2026-10-11", "by": "ada.owner"},
            "schema_hash": draft.schema_digest,
            "semantic_type": "identifier",
        }));
    }
    serde_json::to_string(&json!({
        "catalog": serde_json::from_str::<Value>(pg_fixture()).unwrap(),
        "decisions": decisions,
        "protocol": "oo.builder.reviewed-sealed-import-command/v1",
        "settings": pg_settings(&draft.identity_hash),
    }))
    .unwrap()
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

fn pg_visibility(identity: &str) -> Value {
    json!({
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
    })
}

fn command(draft: &oo_builder::SealedImportDraft, unit: Option<&str>) -> String {
    let mut decisions = Vec::new();
    for column in &draft.columns {
        let semantic = if column.name == "amount" && column.dataset == "erp.parcel" {
            "money"
        } else {
            "identifier"
        };
        let mut row = json!({
            "column": column.name,
            "dataset": column.dataset,
            "fact_fingerprint": column.fact_fingerprint,
            "pii": "none",
            "reviewer": {"at": "2026-10-06", "by": "ada.owner"},
            "schema_hash": draft.schema_digest,
            "semantic_type": semantic,
        });
        if semantic == "money"
            && let Some(unit) = unit
        {
            row["unit"] = json!(unit);
        }
        decisions.push(row);
    }
    serde_json::to_string(&json!({
        "catalog": serde_json::from_str::<Value>(fixture()).unwrap(),
        "decisions": decisions,
        "protocol": "oo.builder.reviewed-sealed-import-command/v1",
        "settings": settings(&draft.identity_hash),
    }))
    .unwrap()
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
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        }
    })
}

fn visibility(identity: &str) -> Value {
    json!({
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
            "columns": ["id", "amount"]
        }],
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
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

const ORACLE_FILE_SHA256: &str = "75baa479e1a79cfff11126ce55d11c667f0b69231179d18e863b4a2e666269d6";
const ORACLE_SCHEMA_DIGEST: &str = "59a1d30cfedb66d93fa8277124851ed6fcb3504c697688ddcf92dcadfcc289dc";
const ORACLE_CONFIG_DIGEST: &str = "d8eadf809a0942e2043810a67a15e197fd5239c2c81155640a337bcd716d744e";
const ORACLE_INPUT_DIGEST: &str = "5064f81e25162f8b183b9fe2c515907a29387d9509f1a7ea3d7b129976556f08";
const ORACLE_TYPE_DIGEST: &str = "ae32553abc785f74a5b84b6afac4a4f5008df340455b7eabc1e0c23fb47a1f11";

fn oracle_fixture() -> &'static str {
    include_str!("fixtures/oo04-oracle-sealed-import-catalog.json")
}

#[test]
fn captured_oracle_bpchar_catalog_compiles_through_draft_oos_and_role_pack() {
    let bytes = include_bytes!("fixtures/oo04-oracle-sealed-import-catalog.json");
    assert_eq!(bytes.len(), 3754);
    assert_eq!(oo_core::api::payload_sha256(bytes), ORACLE_FILE_SHA256);
    let draft = oo_builder::compile_sealed_import_catalog(oracle_fixture()).expect("oracle catalog");
    assert_eq!(draft.schema_digest, ORACLE_SCHEMA_DIGEST);
    assert_eq!(draft.config_digest, ORACLE_CONFIG_DIGEST);
    assert_eq!(draft.input_digest, ORACLE_INPUT_DIGEST);
    assert_eq!(draft.type_digest, ORACLE_TYPE_DIGEST);
    assert_eq!(draft.source_engine, "oracle");
    assert_eq!(draft.source_id, "oo04_t_batch");
    assert_eq!(draft.source_generation, 1);
    assert_eq!(draft.target_generation, 2);
    assert_eq!(draft.normalized_representation, "postgresql_target_storage");
    assert_eq!(draft.protocol, "oo.builder.sealed-import-draft/v1");
    assert_eq!(draft.compiler, "oo-builder/sealed-import-draft/v1");
    assert!(draft.document.contains("oo.pipeline.sealed-import-catalog/v1"));
    assert!(!draft.document.contains("oo.source.pgsql/v1"));
    assert!(!draft.document.contains("pc_"));
    assert!(!draft.document.contains("pn_"));
    assert!(!draft.document.contains("pt_"));
    assert!(!draft.document.contains("bpchar(6)"));
    assert!(!draft.document.contains("trim("));
    assert!(!draft.document.contains("cast("));
    let table = draft.tables.iter().find(|table| table.dataset == "batch.t_batch").expect("table");
    assert_eq!(table.accepted_rows, 5);
    let expected = [
        ("id", "int2", 21, -1, true),
        ("n16", "numeric", 1700, 1048586, false),
        ("n1", "int2", 21, -1, false),
        ("d", "timestamp", 1114, -1, false),
        ("c6", "bpchar", 1042, 10, false),
        ("v2", "varchar", 1043, -1, false),
        ("r", "bytea", 17, -1, false),
    ];
    for (name, target, oid, typmod, key) in expected {
        let column = draft
            .columns
            .iter()
            .find(|column| column.dataset == "batch.t_batch" && column.name == name)
            .expect(name);
        assert_eq!(column.target_type, target);
        assert_eq!(column.target_oid, oid);
        assert_eq!(column.target_typmod, typmod);
        assert_eq!(column.key, key);
        assert!(column.source_type.is_none());
        assert!(column.caveat.is_none());
        assert!(!column.precision_loss);
    }
    let c6 = draft
        .columns
        .iter()
        .find(|column| column.name == "c6")
        .expect("c6");
    assert_eq!(c6.target_typmod - 4, 6);
    let again = oo_builder::compile_sealed_import_catalog(oracle_fixture()).expect("again");
    assert_eq!(again.identity_hash, draft.identity_hash);
    assert_eq!(oo_core::api::payload_sha256(bytes), ORACLE_FILE_SHA256);
    let refused = oo_builder::compile_captured_pg_catalog(oracle_fixture(), None, None).expect_err("pg abi");
    assert!(refused.code.ends_with("/field.unknown"), "{}", refused.code);
    assert_eq!(refused.message, "catalog evidence is not oo.source.pgsql/v1");
    let reviewed = oo_builder::compile_reviewed_sealed_import(&oracle_reviewed(&draft)).expect("reviewed");
    let reviewed: Value = serde_json::from_str(&reviewed).unwrap();
    assert_eq!(reviewed["protocol"], "oo.builder.reviewed-sealed-import-command/v1");
    assert_eq!(reviewed["provenance"]["source_engine"], "oracle");
    assert_eq!(reviewed["provenance"]["source_id"], "oo04_t_batch");
    assert_eq!(reviewed["provenance"]["schema_digest"], ORACLE_SCHEMA_DIGEST);
    assert_eq!(reviewed["provenance"]["identity_hash"], draft.identity_hash);
    assert_eq!(reviewed["provenance"]["normalized_representation"], "postgresql_target_storage");
    let effective = &reviewed["effective_oos"];
    assert_eq!(effective["protocol"], "oo.builder.effective-oos/v1");
    assert_eq!(effective["catalog_version"], draft.identity_hash);
    let file = effective["datasets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["dataset"] == "batch.t_batch")
        .expect("dataset");
    for (name, target, _, _, _) in expected {
        let column = file["columns"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["name"] == name)
            .expect(name);
        assert_eq!(column["data_type"], target);
    }
    let c6_file = file["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "c6")
        .expect("c6");
    assert_eq!(c6_file["data_type"], "bpchar");
    assert_eq!(c6_file["semantic_type"], "code");
    assert_eq!(c6_file["provenance"], "generated");
    assert!(c6_file.get("unit").is_none() || c6_file["unit"].is_null());
    let visibility = oracle_visibility(&draft.identity_hash);
    let pack = oo_builder::compile_role_pack(
        &serde_json::to_string(effective).unwrap(),
        &serde_json::to_string(&visibility).unwrap(),
    )
    .expect("role pack");
    let page = pack.file("tables/batch.t_batch.md").unwrap();
    assert!(page.contains("| `c6` |  | bpchar | code | none |  |"));
    assert!(!page.contains("| `c6` |  | text |"));
    assert!(!page.contains("| `c6` |  | varchar |"));
    assert!(page.contains("primary key: not visible"));
    assert!(!page.contains("| `id` |"));
    assert!(!page.contains("| `v2` |"));
    assert!(!page.contains("pc_"));
    assert!(pack.cache_key.contains(&draft.identity_hash));
    for engine in ["mysql", "clickhouse", "mongodb", "postgresql"] {
        let mut copy = serde_json::from_str::<Value>(oracle_fixture()).unwrap();
        copy["engine"] = json!(engine);
        let compiled = oo_builder::compile_sealed_import_catalog(&copy.to_string()).expect(engine);
        assert_eq!(compiled.source_engine, engine);
        assert_eq!(compiled.schema_digest, draft.schema_digest);
        assert_ne!(compiled.identity_hash, draft.identity_hash);
        let copied = compiled.columns.iter().find(|column| column.name == "c6").expect("c6");
        assert_eq!(copied.target_type, "bpchar");
        assert_eq!(copied.target_oid, 1042);
        assert_eq!(copied.target_typmod, 10);
        assert!(copied.source_type.is_none());
    }
}

#[test]
fn oracle_bpchar_catalog_rejects_type_and_digest_errors() {
    let original = oo_builder::compile_sealed_import_catalog(oracle_fixture()).unwrap();
    let mut named = serde_json::from_str::<Value>(oracle_fixture()).unwrap();
    set_pg_column(&mut named, "t_batch", "c6", "source_type", json!("bpchar"));
    let kept = oo_builder::compile_sealed_import_catalog(&named.to_string()).expect("named bpchar");
    let c6 = kept.columns.iter().find(|column| column.name == "c6").expect("c6");
    assert_eq!(c6.source_type.as_deref(), Some("bpchar"));
    assert_eq!(c6.target_type, "bpchar");
    assert_eq!(c6.target_oid, 1042);
    assert_eq!(c6.target_typmod, 10);
    assert_eq!(kept.identity_hash, original.identity_hash);
    assert_ne!(
        c6.fact_fingerprint,
        original.fact_fingerprint("batch.t_batch", "c6").unwrap()
    );
    let mut token = serde_json::from_str::<Value>(oracle_fixture()).unwrap();
    set_pg_column(&mut token, "t_batch", "c6", "source_type", json!("char"));
    let token = oo_builder::compile_sealed_import_catalog(&token.to_string()).expect("char token");
    let c6 = token.columns.iter().find(|column| column.name == "c6").expect("c6");
    assert_eq!(c6.source_type.as_deref(), Some("char"));
    assert_eq!(c6.target_type, "bpchar");
    assert_eq!(c6.target_oid, 1042);
    for source in ["text", "varchar"] {
        let mut wrong = serde_json::from_str::<Value>(oracle_fixture()).unwrap();
        set_pg_column(&mut wrong, "t_batch", "c6", "source_type", json!(source));
        let err = oo_builder::compile_sealed_import_catalog(&wrong.to_string()).expect_err(source);
        assert!(err.code.ends_with("/type.ambiguous"), "{}", err.code);
        assert_eq!(err.message, "source type token names a different normalized type");
    }
    for typmod in [-1, 0, 4] {
        let mut wrong = serde_json::from_str::<Value>(oracle_fixture()).unwrap();
        set_pg_column(&mut wrong, "t_batch", "c6", "typmod", json!(typmod));
        wrong["schema_digest"] = json!(schema_digest(&wrong["tables"]));
        let err = oo_builder::compile_sealed_import_catalog(&wrong.to_string()).expect_err("typmod");
        assert!(err.code.ends_with("/type.ambiguous"), "{}", err.code);
        assert_eq!(err.message, "native type has no normalized target representation");
    }
    let mut unknown = serde_json::from_str::<Value>(oracle_fixture()).unwrap();
    set_pg_column(&mut unknown, "t_batch", "c6", "oid", json!(1041));
    unknown["schema_digest"] = json!(schema_digest(&unknown["tables"]));
    let err = oo_builder::compile_sealed_import_catalog(&unknown.to_string()).expect_err("oid");
    assert!(err.code.ends_with("/type.ambiguous"), "{}", err.code);
    assert_eq!(err.message, "native type has no normalized target representation");
    let mut stale = serde_json::from_str::<Value>(oracle_fixture()).unwrap();
    set_pg_column(&mut stale, "t_batch", "id", "oid", json!(23));
    let err = oo_builder::compile_sealed_import_catalog(&stale.to_string()).expect_err("stale digest");
    assert!(err.code.ends_with("/digest.mismatch"), "{}", err.code);
    let mut digest = serde_json::from_str::<Value>(oracle_fixture()).unwrap();
    digest["schema_digest"] = json!("ab".repeat(32));
    let err = oo_builder::compile_sealed_import_catalog(&digest.to_string()).expect_err("digest");
    assert!(err.code.ends_with("/digest.mismatch"), "{}", err.code);
    let mut retired = serde_json::from_str::<Value>(oracle_fixture()).unwrap();
    retired["state"] = json!("retired");
    let err = oo_builder::compile_sealed_import_catalog(&retired.to_string()).expect_err("seal");
    assert!(err.code.ends_with("/seal.unsealed"), "{}", err.code);
    let mut generation = serde_json::from_str::<Value>(oracle_fixture()).unwrap();
    generation["source_generation"] = json!(0);
    let err = oo_builder::compile_sealed_import_catalog(&generation.to_string()).expect_err("stale");
    assert!(err.code.ends_with("/seal.stale"), "{}", err.code);
}

fn oracle_reviewed(draft: &oo_builder::SealedImportDraft) -> String {
    let column = draft
        .columns
        .iter()
        .find(|column| column.dataset == "batch.t_batch" && column.name == "c6")
        .expect("c6");
    serde_json::to_string(&json!({
        "catalog": serde_json::from_str::<Value>(oracle_fixture()).unwrap(),
        "decisions": [{
            "column": "c6",
            "dataset": "batch.t_batch",
            "fact_fingerprint": column.fact_fingerprint,
            "pii": "none",
            "reviewer": {"at": "2026-10-11", "by": "ada.owner"},
            "schema_hash": draft.schema_digest,
            "semantic_type": "code",
        }],
        "protocol": "oo.builder.reviewed-sealed-import-command/v1",
        "settings": oracle_settings(&draft.identity_hash),
    }))
    .unwrap()
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

fn oracle_visibility(identity: &str) -> Value {
    json!({
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
    })
}

#[test]
fn jsonb_3802_document_profile_reaches_the_role_pack_and_refuses_bad_documents() {
    let raw = include_bytes!("../../oo-core/tests/fixtures/jsonb3802/canonical-bson.bin");
    let pg = include_bytes!("../../oo-core/tests/fixtures/jsonb3802/canonical.bin");
    let text = include_str!("../../oo-core/tests/fixtures/jsonb3802/canonical.txt");
    let catalog = docs_catalog("mongodb", 23, 3802, -1, Some("jsonb"), None);
    let draft = oo_builder::compile_sealed_import_catalog(&catalog.to_string()).expect("catalog");
    let doc = draft.columns.iter().find(|column| column.name == "doc").expect("doc");
    assert_eq!(doc.target_type, "jsonb");
    assert_eq!(doc.target_oid, 3802);
    assert_eq!(doc.target_typmod, -1);
    assert!(!doc.precision_loss);
    assert!(!doc.key);
    let id = draft.columns.iter().find(|column| column.name == "id").expect("id");
    assert_eq!(id.target_type, "int4");
    assert!(id.key);
    let mysql = docs_catalog("mysql", 23, 3802, -1, Some("jsonb"), None);
    let mysql_draft = oo_builder::compile_sealed_import_catalog(&mysql.to_string()).expect("mysql");
    assert_eq!(mysql_draft.schema_digest, draft.schema_digest);
    assert_ne!(mysql_draft.identity_hash, draft.identity_hash);
    assert_eq!(
        mysql_draft.columns.iter().find(|column| column.name == "doc").unwrap().target_type,
        "jsonb"
    );
    let document = bson_document(raw, pg, text);
    let profile = oo_builder::compile_jsonb_document_profile(&catalog.to_string(), &document.to_string())
        .expect("profile");
    assert_eq!(profile.protocol, "oo.builder.document-jsonb-profile/v1");
    assert_eq!(profile.compiler, "compile_jsonb_document_profile");
    assert_eq!(profile.dataset, "public.docs");
    assert_eq!(profile.column, "doc");
    assert_eq!(profile.schema_digest, draft.schema_digest);
    assert_eq!(profile.source_engine, "mongodb");
    assert_eq!(profile.normalized_representation, "postgresql_target_storage");
    assert_eq!(profile.raw_bson_sha256, oo_core::api::payload_sha256(raw));
    assert_eq!(profile.pg_jsonb_sha256, oo_core::api::payload_sha256(pg));
    let again = oo_builder::compile_jsonb_document_profile(&catalog.to_string(), &document.to_string())
        .expect("again");
    assert_eq!(again.document_identity_hash, profile.document_identity_hash);
    assert_eq!(again.document, profile.document);
    let parsed: Value = serde_json::from_str(&profile.document).unwrap();
    assert_eq!(parsed["arrow"]["type"], "utf8");
    assert_eq!(parsed["arrow"]["metadata"]["oo.storage.type"], "jsonb");
    assert_eq!(parsed["arrow"]["metadata"]["oo.storage.oid"], "3802");
    assert_eq!(parsed["arrow"]["metadata"]["oo.storage.typmod"], "-1");
    assert_eq!(parsed["arrow"]["metadata"]["oo.document.kind"], "bson_extended_json");
    assert_eq!(parsed["arrow"]["metadata"]["oo.document.missing"], "absent_not_null");
    assert_eq!(parsed["arrow"]["metadata"]["oo.document.uuid"], "rfc4122_lowercase");
    assert_eq!(parsed["arrow"]["metadata"]["oo.document.date"], "unix_millis_utc");
    assert_eq!(parsed["arrow"]["metadata"]["oo.document.bytes"], "base64_subtype");
    assert_eq!(parsed["arrow"]["metadata"]["oo.document.decimal"], "exact_decimal_string");
    assert_eq!(parsed["arrow"]["metadata"]["oo.precision.float"], "rejected");
    assert_eq!(parsed["arrow"]["metadata"]["oo.document.double"], "canonical_number_double");
    assert_eq!(parsed["arrow"]["metadata"]["oo.document.double.negative_zero"], "-0.0");
    assert_eq!(parsed["arrow"]["metadata"]["oo.document.double.non_finite"], "NaN|Infinity|-Infinity");
    assert_eq!(parsed["arrow"]["metadata"]["oo.document.double.nan_payload"], "not_preserved");
    assert_eq!(parsed["arrow"]["metadata"]["oo.document.field_scope"], "unchecked");
    assert_eq!(parsed["arrow"]["metadata"]["oo.quality.loss_concealed"], "false");
    assert_eq!(parsed["quality"]["loss_concealed"], false);
    assert_eq!(parsed["precision"]["float"], "rejected");
    assert_eq!(parsed["double"]["encoding"], "canonical_number_double");
    assert_eq!(parsed["double"]["negative_zero"], "-0.0");
    assert_eq!(parsed["double"]["non_finite"], "NaN|Infinity|-Infinity");
    assert_eq!(parsed["double"]["nan_payload"], "not_preserved");
    assert_eq!(parsed["extended_json"]["big"]["$numberLong"], "9223372036854775807");
    assert_eq!(parsed["extended_json"]["fits"]["$numberLong"], "5");
    assert_eq!(parsed["extended_json"]["d"]["$numberDecimal"], "1.2300");
    assert_eq!(parsed["extended_json"]["negd"]["$numberDecimal"], "-1.50");
    assert_eq!(parsed["extended_json"]["u"]["$uuid"], "00112233-4455-4677-8899-aabbccddeeff");
    assert_eq!(parsed["extended_json"]["when"]["$date"]["$numberLong"], "-61000");
    assert_eq!(parsed["extended_json"]["b"]["$binary"]["base64"], "AA==");
    assert_eq!(parsed["extended_json"]["b"]["$binary"]["subType"], "00");
    assert_eq!(parsed["extended_json"]["_id"]["$oid"], "507f1f77bcf86cd799439011");
    assert!(parsed["extended_json"]["note"].is_null());
    assert!(parsed["extended_json"].get("gone").is_none());
    assert!(!profile.document.contains(&hex_encode(raw)));
    let reviewed = oo_builder::compile_reviewed_sealed_import(&docs_reviewed(&catalog, &draft)).expect("reviewed");
    let reviewed: Value = serde_json::from_str(&reviewed).unwrap();
    let effective = &reviewed["effective_oos"];
    let file = effective["datasets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["dataset"] == "public.docs")
        .unwrap();
    let column = file["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "doc")
        .unwrap();
    assert_eq!(column["data_type"], "jsonb");
    assert_eq!(column["semantic_type"], "json");
    let pack = oo_builder::compile_role_pack(
        &serde_json::to_string(effective).unwrap(),
        &serde_json::to_string(&docs_visibility(&draft.identity_hash)).unwrap(),
    )
    .expect("role pack");
    let page = pack.file("tables/public.docs.md").unwrap();
    assert!(page.contains("| `doc` |  | jsonb | json | none |  |"));
    assert!(page.contains("| `id` |  | int4 | identifier | none |  |"));
    assert!(!page.contains("json_not_jsonb"));
    let mut dropped = document.clone();
    dropped["raw_bson_dropped"] = json!(true);
    dropped["raw_bson_hex"] = json!("");
    let err = oo_builder::compile_jsonb_document_profile(&catalog.to_string(), &dropped.to_string())
        .expect_err("dropped");
    assert!(err.code.ends_with("/raw_bson_dropped"), "{}", err.code);
    let bad = docs_catalog("mongodb", 23, 3803, -1, None, None);
    let err = oo_builder::compile_jsonb_document_profile(&bad.to_string(), &dropped.to_string())
        .expect_err("oid");
    assert!(err.code.ends_with("/type.ambiguous"), "{}", err.code);
    assert_eq!(err.message, "native type has no normalized target representation");
    let mut as_json = document.clone();
    as_json["oid"] = json!(114);
    let err = oo_builder::compile_jsonb_document_profile(&catalog.to_string(), &as_json.to_string())
        .expect_err("114");
    assert!(err.code.ends_with("/json114_not_jsonb"), "{}", err.code);
    as_json["oid"] = json!(3802);
    as_json["typmod"] = json!(0);
    let err = oo_builder::compile_jsonb_document_profile(&catalog.to_string(), &as_json.to_string())
        .expect_err("typmod");
    assert!(err.code.ends_with("/typmod.rejected"), "{}", err.code);
    let json_column = docs_catalog("postgresql", 23, 114, -1, Some("json"), None);
    let err = oo_builder::compile_jsonb_document_profile(&json_column.to_string(), &document.to_string())
        .expect_err("json column");
    assert!(err.code.ends_with("/type.ambiguous"), "{}", err.code);
    assert_eq!(err.message, "document column is not jsonb oid 3802 typmod -1");
}

fn docs_catalog(
    engine: &str,
    id_oid: i64,
    doc_oid: i64,
    doc_typmod: i32,
    source_type: Option<&str>,
    caveat: Option<&str>,
) -> Value {
    let mut preimage = String::new();
    oo_core::sealed_import::push_schema_table(&mut preimage, "public", "docs");
    oo_core::sealed_import::push_schema_column(&mut preimage, "id", id_oid, -1, None);
    oo_core::sealed_import::push_schema_column(&mut preimage, "doc", doc_oid, doc_typmod, caveat);
    let schema_digest = oo_core::api::payload_sha256(preimage.as_bytes());
    let digest = "a".repeat(64);
    let id_shadow = format!("pc_{}", "11".repeat(30));
    let doc_shadow = format!("pc_{}", "22".repeat(30));
    let namespace = format!("pn_{}", "33".repeat(30));
    let physical = format!("pt_{}", "44".repeat(30));
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
                "physical_name": physical,
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
                    "stored_physical": id_shadow,
                    "typmod": -1
                },
                {
                    "caveat": caveat,
                    "key": false,
                    "logical_name": "doc",
                    "oid": doc_oid,
                    "source_type": source_type,
                    "stored_physical": doc_shadow,
                    "typmod": doc_typmod
                }
            ],
            "logical_schema": "public",
            "logical_table": "docs",
            "rejects": [],
            "stored_namespace": namespace,
            "stored_physical": physical
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
        "raw_bson_hex": hex_encode(raw),
        "raw_bson_sha256": oo_core::api::payload_sha256(raw),
        "pg_jsonb_hex": hex_encode(pg),
        "pg_jsonb_text": text,
        "loss_concealed": false,
        "missing_is_null": false
    })
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn docs_reviewed(catalog: &Value, draft: &oo_builder::SealedImportDraft) -> String {
    let mut decisions = Vec::new();
    for (name, semantic) in [("id", "identifier"), ("doc", "json")] {
        let column = draft.columns.iter().find(|column| column.name == name).expect(name);
        decisions.push(json!({
            "column": name,
            "dataset": "public.docs",
            "fact_fingerprint": column.fact_fingerprint,
            "pii": "none",
            "reviewer": {"at": "2026-10-11", "by": "ada.owner"},
            "schema_hash": draft.schema_digest,
            "semantic_type": semantic,
        }));
    }
    serde_json::to_string(&json!({
        "catalog": catalog,
        "decisions": decisions,
        "protocol": "oo.builder.reviewed-sealed-import-command/v1",
        "settings": docs_settings(&draft.identity_hash),
    }))
    .unwrap()
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

fn docs_visibility(identity: &str) -> Value {
    json!({
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
    })
}

#[test]
fn curated_label_is_bound_to_native_fact_and_preserves_classification() {
    let draft = compile(fixture());
    let mut input: Value = serde_json::from_str(&command(&draft, Some("USD"))).unwrap();
    let column = draft.columns.iter().find(|c| c.dataset == "erp.parcel" && c.name == "amount").unwrap();
    input["curated_labels"] = json!([{"dataset":"erp.parcel","column":"amount","fact_fingerprint":column.fact_fingerprint,"label":"Reviewed amount"}]);
    let result: Value = serde_json::from_str(&oo_builder::compile_reviewed_sealed_import(&input.to_string()).unwrap()).unwrap();
    let dataset = result["effective_oos"]["datasets"].as_array().unwrap().iter().find(|d| d["dataset"] == "erp.parcel").unwrap();
    let amount = dataset["columns"].as_array().unwrap().iter().find(|c| c["name"] == "amount").unwrap();
    assert_eq!(amount["label"], "Reviewed amount");
    assert_eq!(amount["provenance"], "human");
    assert_eq!(amount["unit"], "USD");
    assert_eq!(result["provenance"]["identity_hash"], draft.identity_hash);
    input["curated_labels"][0]["fact_fingerprint"] = json!("00".repeat(32));
    assert!(oo_builder::compile_reviewed_sealed_import(&input.to_string()).unwrap_err().code.ends_with("/curated.stale"));
}
