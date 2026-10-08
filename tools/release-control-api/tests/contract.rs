//! Frozen contract files and the public permission rule.

use std::fs;
use std::path::PathBuf;

use release_control_api::{permission_compatible, sha256_hex, ApiError, Lineage, LineageFields, CONTRACT};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(name: &str) -> String {
    fs::read_to_string(root().join(name)).unwrap_or_else(|_| panic!("missing {name}"))
}

#[test]
fn openapi_binds_the_admin_operations() {
    let text = read("openapi/release-control-api.openapi.yaml");
    for operation in ["createRelease", "listReleases", "getActiveRelease", "getActiveRolePack", "getRelease", "rollbackRelease"] {
        assert!(text.contains(&format!("operationId: {operation}")), "{operation}");
    }
    assert!(text.contains("B05.publish"));
    assert!(!text.contains("operationId: force"));
    assert!(!text.contains("customer_approved"));
}

#[test]
fn schemas_reject_unknown_fields_and_production_claims() {
    for name in [
        "schema/error.schema.json",
        "schema/create-release.request.schema.json",
        "schema/rollback.request.schema.json",
        "schema/release.schema.json",
        "schema/active.schema.json",
        "schema/role-pack.schema.json",
    ] {
        let text = read(name);
        assert!(text.contains("\"additionalProperties\": false"), "{name}");
    }
    let release = read("schema/release.schema.json");
    assert!(release.contains("\"production_ready\": {\"const\": false}"));
    assert!(release.contains("\"half_published\": {\"const\": false}"));
    assert!(release.contains("test_scope_not_production"));
    let active = read("schema/active.schema.json");
    assert!(active.contains("\"reader\": {\"const\": \"oo_release_reader\"}"));
    let pack = read("schema/role-pack.schema.json");
    assert!(pack.contains("\"replica_production_admissible\": {\"const\": false}"));
}

#[test]
fn error_body_cannot_claim_production() {
    let body = ApiError::forbidden("scope").body();
    assert_eq!(body["contract"], CONTRACT);
    assert_eq!(body["production_ready"], false);
    assert_eq!(body["full_sp08_accepted"], false);
    assert_eq!(body["code"], "forbidden");
    assert_eq!(body["detail"], "scope");
}

#[test]
fn rollback_permission_is_a_column_subset() {
    let wide = lineage(&["public.customer.contact", "public.customer.id"]);
    let narrow = lineage(&["public.customer.contact"]);
    assert!(permission_compatible(Some(&narrow), &wide));
    assert!(!permission_compatible(Some(&wide), &narrow));
    assert!(permission_compatible(None, &wide));
}

#[test]
fn migration_hash_is_lowercase_hex() {
    let hash = release_control_api::migration_sha256();
    assert_eq!(hash.len(), 64);
    assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
    let bytes = fs::read(root().join("migrations/oo_release_api_v1.sql")).expect("migration");
    assert_eq!(hash, sha256_hex(&bytes));
}

fn lineage(columns: &[&str]) -> Lineage {
    let mut line = Lineage::from_columns(
        "",
        LineageFields {
            role: "oo_r_clerk",
            policy_id: "pol-shop",
            policy_version: "1",
            knowledge_id: "know-shop",
            knowledge_version: "1",
            catalog_version: "cat-1",
            review_admitted: true,
        },
    );
    line.columns = columns.iter().map(|item| (*item).to_owned()).collect();
    line
}
