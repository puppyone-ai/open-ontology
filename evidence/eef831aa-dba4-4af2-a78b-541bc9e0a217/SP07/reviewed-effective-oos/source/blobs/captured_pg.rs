//! Captured PostgreSQL catalog to a reviewable structure draft.
//!
//! The input is the serde document of `oo.source.pgsql/v1`, the same shape
//! `PgSession::catalog` returns. This module does not open a connection, import
//! rows, grant privileges, classify semantics, certify, or publish. A captured
//! catalog is not proof of a complete row import or of an authorized grant.

use serde_json::Value;

use crate::canonical::{canonical_json, canonical_sha256, sha256_hex};
use crate::draft::{
    self, COMPILER, DRAFT_PROTOCOL, PG_CONTRACT_COMMIT, catalog_bindings, evidence_envelope_bytes,
    manifest_preimage_hash,
};
use crate::error::DraftError;
use crate::pgsql::{self, Catalog};

pub const CAPTURED_PROTOCOL: &str = "oo.builder.captured-pg-draft/v1";
pub const CAPTURED_COMPILER: &str = "oo-builder/captured-pg-draft/v1";

/// One captured catalog compiled through the existing structure-draft compiler.
///
/// `evidence_envelope` and `draft` are canonical JSON with no trailing newline.
/// `wrapper` binds their hashes. `wrapper_hash` is the digest of that wrapper
/// with the `wrapper_hash` field omitted, then the field is inserted. That is
/// the same seal rule as `manifest_hash`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedPgDraft {
    pub protocol: &'static str,
    pub compiler: &'static str,
    pub wrapper: String,
    pub evidence_envelope: String,
    pub draft: String,
    pub source_id: String,
    pub database: String,
    pub captured_at: String,
    pub schema_hash: String,
    pub config_hash: String,
    pub type_hash: String,
    pub evidence_hash: String,
    pub manifest_hash: String,
    pub captured_catalog_hash: String,
    pub evidence_envelope_hash: String,
    pub draft_hash: String,
    pub wrapper_hash: String,
}

/// Compile one captured `oo.source.pgsql/v1` catalog into an evidence envelope
/// and a structure draft.
///
/// `curated_json` and `reviews_json` are the optional overlay and decision list
/// already accepted by `compile_structure_draft`. Hashes are recomputed. A
/// caller cannot supply `schema_hash`, `config_hash`, `type_hash`, or
/// `evidence_hash` except as the catalog's own captured `schema_hash`, which
/// is checked against `pg-catalog-hash/v1`.
pub fn compile_captured_pg_catalog(
    catalog_json: &str,
    curated_json: Option<&str>,
    reviews_json: Option<&str>,
) -> Result<CapturedPgDraft, DraftError> {
    let catalog_value = draft::parse_evidence_document(catalog_json)?;
    let (catalog_value, catalog) = pgsql::accept_captured_catalog(catalog_value)?;
    let curated = match curated_json {
        None => None,
        Some(text) => Some(draft::parse_evidence_document(text)?),
    };
    let reviews = match reviews_json {
        None => Value::Array(Vec::new()),
        Some(text) => draft::parse_evidence_document(text)?,
    };
    let (bound_config, bound_type, bound_evidence) = catalog_bindings(&catalog);
    let evidence_envelope =
        evidence_envelope_bytes(&catalog_value, &catalog, curated.as_ref(), &reviews);
    let draft = draft::compile_structure_draft(&evidence_envelope)?;
    let draft_value: Value = serde_json::from_str(&draft).map_err(|_| {
        DraftError::new(
            "schema.invalid",
            "/draft",
            "compiled draft is not the structure-draft document",
        )
    })?;
    require_historical(
        &draft_value,
        &catalog,
        &bound_config,
        &bound_type,
        &bound_evidence,
    )?;
    let manifest_hash = sealed_manifest(&draft_value)?;
    let captured_catalog_hash = canonical_sha256(&catalog_value);
    let evidence_envelope_hash = sha256_hex(evidence_envelope.as_bytes());
    let draft_hash = sha256_hex(draft.as_bytes());
    let (wrapper, wrapper_hash) = seal_wrapper(&WrapperParts {
        catalog: &catalog,
        config_hash: &bound_config,
        type_hash: &bound_type,
        evidence_hash: &bound_evidence,
        manifest_hash: &manifest_hash,
        captured_catalog_hash: &captured_catalog_hash,
        evidence_envelope_hash: &evidence_envelope_hash,
        draft_hash: &draft_hash,
    });
    Ok(CapturedPgDraft {
        protocol: CAPTURED_PROTOCOL,
        compiler: CAPTURED_COMPILER,
        wrapper,
        evidence_envelope,
        draft,
        source_id: catalog.source_id,
        database: catalog.database,
        captured_at: catalog.captured_at,
        schema_hash: catalog.schema_hash,
        config_hash: bound_config,
        type_hash: bound_type,
        evidence_hash: bound_evidence,
        manifest_hash,
        captured_catalog_hash,
        evidence_envelope_hash,
        draft_hash,
        wrapper_hash,
    })
}

struct WrapperParts<'a> {
    catalog: &'a Catalog,
    config_hash: &'a str,
    type_hash: &'a str,
    evidence_hash: &'a str,
    manifest_hash: &'a str,
    captured_catalog_hash: &'a str,
    evidence_envelope_hash: &'a str,
    draft_hash: &'a str,
}

fn seal_wrapper(parts: &WrapperParts<'_>) -> (String, String) {
    let wrapper = serde_json::json!({
        "captured_at": &parts.catalog.captured_at,
        "captured_catalog_hash": parts.captured_catalog_hash,
        "certification": "none",
        "compiler": CAPTURED_COMPILER,
        "config_hash": parts.config_hash,
        "connector_protocol": pgsql::PROTOCOL,
        "database": &parts.catalog.database,
        "draft_hash": parts.draft_hash,
        "evidence_envelope_hash": parts.evidence_envelope_hash,
        "evidence_hash": parts.evidence_hash,
        "grants": "not_authorized",
        "historical": {
            "compiler": COMPILER,
            "pg_catalog_contract": pgsql::PROTOCOL,
            "pg_catalog_contract_commit": PG_CONTRACT_COMMIT,
            "pg_catalog_integration": "not_accepted",
            "protocol": DRAFT_PROTOCOL,
        },
        "live_catalog_review": "pending",
        "llm": "not_used",
        "manifest_hash": parts.manifest_hash,
        "protocol": CAPTURED_PROTOCOL,
        "publication": "unavailable",
        "row_import": "not_proven",
        "schema_hash": &parts.catalog.schema_hash,
        "source_connection": "not_used",
        "source_id": &parts.catalog.source_id,
        "type_hash": parts.type_hash,
    });
    let wrapper_hash = canonical_sha256(&wrapper);
    let mut sealed = wrapper;
    if let Some(object) = sealed.as_object_mut() {
        object.insert(
            "wrapper_hash".to_string(),
            Value::String(wrapper_hash.clone()),
        );
    }
    (canonical_json(&sealed), wrapper_hash)
}

fn require_historical(
    draft: &Value,
    catalog: &Catalog,
    config_hash: &str,
    type_hash: &str,
    evidence_hash: &str,
) -> Result<(), DraftError> {
    let provenance = draft.get("provenance").ok_or_else(historical_error)?;
    let same =
        |key: &str, expected: &str| provenance.get(key).and_then(Value::as_str) == Some(expected);
    if draft.get("protocol").and_then(Value::as_str) != Some(DRAFT_PROTOCOL)
        || draft.get("compiler").and_then(Value::as_str) != Some(COMPILER)
        || !same("pg_catalog_integration", "not_accepted")
        || !same("pg_catalog_contract_commit", PG_CONTRACT_COMMIT)
        || !same("pg_catalog_contract", pgsql::PROTOCOL)
        || !same("publication", "unavailable")
        || !same("certification", "none")
        || !same("llm", "not_used")
        || !same("source_connection", "not_used")
        || !same("schema_hash", &catalog.schema_hash)
        || !same("config_hash", config_hash)
        || !same("type_hash", type_hash)
        || !same("evidence_hash", evidence_hash)
        || !same("source_id", &catalog.source_id)
        || !same("database", &catalog.database)
        || !same("captured_at", &catalog.captured_at)
    {
        return Err(historical_error());
    }
    Ok(())
}

fn sealed_manifest(draft: &Value) -> Result<String, DraftError> {
    let found = draft
        .get("provenance")
        .and_then(|item| item.get("manifest_hash"))
        .and_then(Value::as_str)
        .ok_or_else(historical_error)?;
    if manifest_preimage_hash(draft).as_deref() != Some(found) {
        return Err(historical_error());
    }
    Ok(found.to_string())
}

fn historical_error() -> DraftError {
    DraftError::new(
        "version.unsupported",
        "/provenance",
        "draft provenance is not the historical compiler contract",
    )
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::compile_captured_pg_catalog;
    use crate::canonical::sha256_hex;
    use crate::pgsql::parse_catalog;

    const SHOP_SCHEMA: &str = "bcb019d92270bf12092a3f47697b25719cc087a1ac4b7c56a10cf6dd00294c4c";
    const STALE_SCHEMA: &str = "31bbede923a28e58c8fef54240de5458533fa881e4d5898cdd31504b4aa0bd7e";

    fn column(attnum: i32, name: &str, type_oid: i64, type_name: &str, not_null: bool) -> Value {
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
            "comment": Value::Null,
            "enum_labels": [],
            "domain_checks": [],
            "type_support": {"converted": false, "reason": "known_builtin_not_converted"},
            "current_privileges": {"select": true, "insert": false, "update": false}
        })
    }

    fn shop_value(email_not_null: bool) -> Value {
        let key = json!([{"attnum": 1, "name": "tenant_id"}, {"attnum": 2, "name": "id"}]);
        let table = |name: &str, columns: Vec<Value>, foreign_keys: Value| {
            json!({
                "schema": "public",
                "schema_ident": "public",
                "name": name,
                "name_ident": name,
                "relkind": "r",
                "replica_identity": "d",
                "rls_enabled": false,
                "rls_forced": false,
                "owner": "owner_role",
                "comment": Value::Null,
                "columns": columns,
                "primary_key": key.clone(),
                "primary_key_nulls_not_distinct": false,
                "unique_keys": [],
                "foreign_keys": foreign_keys,
                "policies": [],
                "privileges": [{"grantee": "reader", "privilege": "SELECT", "grantable": false}]
            })
        };
        json!({
            "protocol": "oo.source.pgsql/v1",
            "core_contract": "oo.config/v1",
            "source_id": "shop_src",
            "database": "shop",
            "captured_at": "2026-10-04T12:00:00.000000Z",
            "schema_hash": "0000000000000000000000000000000000000000000000000000000000000000",
            "hash_algorithm": "pg-catalog-hash/v1",
            "ident_algorithm": "pgident/v1",
            "tables": [
                table(
                    "customer",
                    vec![
                        column(1, "tenant_id", 23, "int4", true),
                        column(2, "id", 23, "int4", true),
                        column(3, "email", 25, "text", email_not_null),
                    ],
                    json!([])
                ),
                table(
                    "purchase",
                    vec![
                        column(1, "tenant_id", 23, "int4", true),
                        column(2, "id", 23, "int4", true),
                        column(3, "customer_id", 23, "int4", true),
                    ],
                    json!([{
                        "name": "purchase_customer_fk",
                        "columns": [
                            {"attnum": 1, "name": "tenant_id", "ref_schema": "public", "ref_table": "customer", "ref_attnum": 1, "ref_name": "tenant_id"},
                            {"attnum": 3, "name": "customer_id", "ref_schema": "public", "ref_table": "customer", "ref_attnum": 2, "ref_name": "id"}
                        ]
                    }])
                )
            ],
            "type_conversion": "unsupported"
        })
    }

    #[test]
    fn parser_locks_the_shop_schema_hash() {
        let base = parse_catalog(&shop_value(false)).unwrap();
        assert_eq!(base.schema_hash, SHOP_SCHEMA);
        let stale = parse_catalog(&shop_value(true)).unwrap();
        assert_eq!(stale.schema_hash, STALE_SCHEMA);
        let mut value = shop_value(false);
        value["schema_hash"] = json!(base.schema_hash);
        value["tables"][0]["columns"][2]["comment"] = json!({
            "byte_len": 30,
            "sha256": sha256_hex(b"a156-comment-body-do-not-carry")
        });
        let text = serde_json::to_string(&value).unwrap();
        let compiled = compile_captured_pg_catalog(&text, None, None).unwrap();
        assert_eq!(compiled.schema_hash, SHOP_SCHEMA);
        assert!(!compiled.draft.contains("a156-comment-body-do-not-carry"));
        assert!(!compiled.wrapper.contains("a156-comment-body-do-not-carry"));
    }
}
