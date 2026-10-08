//! Compiler identity shared with ontology-dialogue.
//!
//! A legal policy id is the existing slug or exactly 64 lowercase hex digits.
//! The compiler copies that id into the pack manifest. It does not rewrite it.
//! Knowledge stays on the existing slug, so a hyphen remains legal here.

use serde_json::{Value, json};

use oo_builder::{RolePack, RolePackError, compile_role_pack};

const DIGEST: &str = "e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e";

#[test]
fn approved_digest_and_intersection_knowledge_are_emitted_unchanged() {
    let pack = compile_pair(DIGEST, "know_a317").expect("digest policy");
    let artifact: Value = serde_json::from_str(&pack.artifact).expect("artifact");
    assert_eq!(artifact["protocol"], "oo.builder.role-pack/v1");
    assert_eq!(artifact["compiler"], "oo-builder/role-pack/v1");
    let manifest: Value = serde_json::from_str(pack.file("manifest.json").expect("manifest")).expect("manifest json");
    assert_eq!(manifest["protocol"], "oo.builder.role-pack-manifest/v1");
    assert_eq!(manifest["policy"]["id"], DIGEST);
    assert_eq!(manifest["knowledge"]["id"], "know_a317");
    assert_eq!(pack.pack_digest.len(), 64);
}

#[test]
fn hyphenated_knowledge_still_compiles_and_is_not_rewritten() {
    let pack = compile_pair("pol-shop", "know-a317").expect("hyphen knowledge");
    let manifest: Value = serde_json::from_str(pack.file("manifest.json").expect("manifest")).expect("manifest json");
    assert_eq!(manifest["policy"]["id"], "pol-shop");
    assert_eq!(manifest["knowledge"]["id"], "know-a317");
    let with_digest = compile_pair(DIGEST, "know-a317").expect("digest plus hyphen knowledge");
    let manifest: Value =
        serde_json::from_str(with_digest.file("manifest.json").expect("manifest")).expect("manifest json");
    assert_eq!(manifest["policy"]["id"], DIGEST);
    assert_eq!(manifest["knowledge"]["id"], "know-a317");
}

#[test]
fn dangerous_policy_ids_fail_before_a_pack_exists() {
    let policies = [
        DIGEST.to_ascii_uppercase(),
        "0".repeat(63),
        "a".repeat(65),
        "g".repeat(64),
        "../pol".to_string(),
        String::new(),
        "pol/shop".to_string(),
        "pol shop".to_string(),
        "pol\nshop".to_string(),
        "\u{0001}pol".to_string(),
    ];
    for policy in &policies {
        let err = compile_pair(policy, "know_a317").expect_err("refused policy");
        assert_eq!(err.code, "oo.builder.role-pack/v1/identity.invalid", "{policy:?}");
        assert_eq!(err.path, "/policy/id");
    }
}

fn compile_pair(policy: &str, knowledge: &str) -> Result<RolePack, RolePackError> {
    let (effective, visibility) = documents(policy, knowledge);
    compile_role_pack(
        &serde_json::to_string(&effective).expect("effective"),
        &serde_json::to_string(&visibility).expect("visibility"),
    )
}

fn documents(policy: &str, knowledge: &str) -> (Value, Value) {
    let identity = json!({
        "project": "shop",
        "ontology_version": "ont-1",
        "catalog_version": "cat-1",
        "policy": {"id": policy, "version": "1"},
        "knowledge": {"id": knowledge, "version": "1"}
    });
    let mut effective = identity.clone();
    effective["protocol"] = json!("oo.builder.effective-oos/v1");
    effective["ontology"] = json!({
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
    });
    effective["datasets"] = json!([{
        "schema_version": "oos/v1",
        "dataset": "public.entry",
        "label": "Entry",
        "tier": "raw",
        "columns": [{
            "name": "entry_count",
            "pii": "none",
            "classification": {"state": "confirmed"}
        }]
    }]);
    let mut visibility = identity;
    visibility["protocol"] = json!("oo.builder.role-visibility/v1");
    visibility["principal"] = json!("ada");
    visibility["role"] = json!("oo_r_clerk");
    visibility["issued_for_compiler"] = json!("oo-builder/role-pack/v1");
    visibility["datasets"] = json!([{
        "dataset": "public.entry",
        "row_security": "none",
        "global_stats": "denied",
        "columns": ["entry_count"]
    }]);
    visibility["limits"] = json!({
        "max_file_bytes": 262144,
        "max_total_bytes": 1048576,
        "max_entries": 128
    });
    (effective, visibility)
}
