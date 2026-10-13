//! Shop catalog used by the release-coordinator proof. The compiler is `oo-builder`.

use oo_builder::{
    REVIEWED_EFFECTIVE_COMMAND_PROTOCOL, compile_captured_pg_catalog, compile_reviewed_effective,
};
use release_control_api::sha256_hex;
use serde_json::{Value, json};

pub const POLICY_ID: &str = "pol-shop";
pub const POLICY_VERSION: &str = "1";
pub const KNOWLEDGE_ID: &str = "know-shop";
pub const KNOWLEDGE_VERSION: &str = "1";
pub const CATALOG_VERSION: &str = "cat-1";

pub fn shop(columns: &[&str], role: &str) -> (Value, Value) {
    let catalog = catalog_text();
    let compiled = compile_captured_pg_catalog(&catalog, None, None).expect("captured catalog");
    let draft: Value = serde_json::from_str(&compiled.draft).expect("draft");
    let schema = draft["provenance"]["schema_hash"]
        .as_str()
        .expect("schema")
        .to_owned();
    let mut decisions = Vec::new();
    for column in ["id", "contact"] {
        let item_id = format!("column/public.customer/{column}");
        let fingerprint = draft["review_queue"]
            .as_array()
            .expect("queue")
            .iter()
            .find(|item| item["item_id"] == item_id && item["kind"] == "structural_column")
            .expect("column fact")["fact_fingerprint"]
            .as_str()
            .expect("fingerprint")
            .to_owned();
        decisions.push(json!({
            "dataset": "public.customer",
            "column": column,
            "schema_hash": schema,
            "fact_fingerprint": fingerprint,
            "pii": "none",
            "semantic_type": "identifier",
            "reviewer": {"by": "ada.owner", "at": "2026-10-06"}
        }));
    }
    let artifact = json!({
        "draft": serde_json::from_str::<Value>(&compiled.draft).unwrap(),
        "evidence_envelope": serde_json::from_str::<Value>(&compiled.evidence_envelope).unwrap(),
        "protocol": "oo.builder.captured-pg-draft-command/v1",
        "wrapper": serde_json::from_str::<Value>(&compiled.wrapper).unwrap(),
    });
    let command = json!({
        "protocol": REVIEWED_EFFECTIVE_COMMAND_PROTOCOL,
        "artifact": artifact,
        "settings": {
            "project": "shop",
            "ontology_version": "ont-1",
            "catalog_version": CATALOG_VERSION,
            "policy": {"id": POLICY_ID, "version": POLICY_VERSION},
            "knowledge": {"id": KNOWLEDGE_ID, "version": KNOWLEDGE_VERSION},
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
        },
        "decisions": decisions,
    });
    let reviewed = compile_reviewed_effective(&serde_json::to_string(&command).expect("command"))
        .expect("reviewed effective");
    let reviewed: Value = serde_json::from_str(&reviewed).expect("reviewed json");
    let visibility = json!({
        "protocol": "oo.builder.role-visibility/v1",
        "project": "shop",
        "principal": "ada",
        "role": role,
        "ontology_version": "ont-1",
        "catalog_version": CATALOG_VERSION,
        "policy": {"id": POLICY_ID, "version": POLICY_VERSION},
        "knowledge": {"id": KNOWLEDGE_ID, "version": KNOWLEDGE_VERSION},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": [{
            "dataset": "public.customer",
            "row_security": "none",
            "global_stats": "denied",
            "columns": columns
        }],
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    });
    (reviewed["effective_oos"].clone(), visibility)
}

pub fn pack_digest(effective: &Value, visibility: &Value) -> String {
    let pack = oo_builder::compile_role_pack(
        &serde_json::to_string(effective).expect("effective"),
        &serde_json::to_string(visibility).expect("visibility"),
    )
    .expect("role pack");
    pack.pack_digest
}

fn column_preimage(attnum: i32, name: &str, type_oid: i64) -> String {
    format!(
        r#"{{"attnum":{attnum},"name":"{name}","type_oid":{type_oid},"typmod":-1,"not_null":true,"collation_oid":0,"typtype":"b","base_type_oid":null,"identity":"","generated":"","default_sha256":null,"enum_labels":[],"domain_checks":[]}}"#
    )
}

fn catalog_text() -> String {
    let columns = format!(
        "{},{}",
        column_preimage(1, "id", 23),
        column_preimage(2, "contact", 25)
    );
    let preimage = format!(
        r#"{{"tables":[{{"schema":"public","name":"customer","relkind":"r","replica_identity":"d","rls":false,"force_rls":false,"columns":[{columns}],"primary_key":[{{"attnum":1,"name":"id"}}],"primary_key_nulls_not_distinct":false,"unique_keys":[],"foreign_keys":[],"policies":[]}}]}}"#
    );
    let schema_hash = sha256_hex(preimage.as_bytes());
    serde_json::to_string(&json!({
        "protocol": "oo.source.pgsql/v1",
        "core_contract": "oo.config/v1",
        "source_id": "shop_src",
        "database": "shop",
        "captured_at": "2026-10-04T12:00:00.000000Z",
        "schema_hash": schema_hash,
        "hash_algorithm": "pg-catalog-hash/v1",
        "ident_algorithm": "pgident/v1",
        "tables": [{
            "schema": "public",
            "schema_ident": "public",
            "name": "customer",
            "name_ident": "customer",
            "relkind": "r",
            "replica_identity": "d",
            "rls_enabled": false,
            "rls_forced": false,
            "owner": "owner_role_do_not_leak",
            "comment": Value::Null,
            "columns": [column_json(1, "id", 23, "int4"), column_json(2, "contact", 25, "text")],
            "primary_key": [{"attnum": 1, "name": "id"}],
            "primary_key_nulls_not_distinct": false,
            "unique_keys": [],
            "foreign_keys": [],
            "policies": [],
            "privileges": [{"grantee": "owner_role_do_not_leak", "privilege": "SELECT", "grantable": false}]
        }],
        "type_conversion": "unsupported"
    }))
    .expect("catalog")
}

fn column_json(attnum: i32, name: &str, type_oid: i64, type_name: &str) -> Value {
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
        "not_null": true,
        "collation_oid": 0,
        "collation_name": Value::Null,
        "identity": "",
        "generated": "",
        "default_expr": Value::Null,
        "comment": Value::Null,
        "enum_labels": [],
        "domain_checks": [],
        "type_support": {"converted": false, "reason": "known_builtin_not_converted"},
        "current_privileges": {"select": true, "insert": false, "update": false}
    })
}
