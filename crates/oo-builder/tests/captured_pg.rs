//! Public captured-catalog API. Schema hashes are locked by
//! `captured_pg::tests::parser_locks_the_shop_schema_hash`, which calls the
//! existing catalog parser rather than a second preimage implementation.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use oo_builder::{
    CAPTURED_COMPILER, CAPTURED_PROTOCOL, COMPILER, DRAFT_PROTOCOL, EMPTY_SCHEMA_HASH,
    EVIDENCE_PROTOCOL, compile_captured_pg_catalog, compile_structure_draft,
};

const SHOP_SCHEMA: &str = "bcb019d92270bf12092a3f47697b25719cc087a1ac4b7c56a10cf6dd00294c4c";
const STALE_SCHEMA: &str = "31bbede923a28e58c8fef54240de5458533fa881e4d5898cdd31504b4aa0bd7e";
const COMMENT_BODY: &str = "a156-comment-body-do-not-carry";
const OWNER: &str = "owner_role_do_not_leak";
const CAPTURED_AT: &str = "2026-10-04T12:00:00.000000Z";

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn comment(text: &str) -> Value {
    json!({"byte_len": text.len(), "sha256": sha256_hex(text.as_bytes())})
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

fn privileges(first_owner: bool) -> Value {
    let owner = json!({"grantee": OWNER, "privilege": "INSERT", "grantable": true});
    let reader = json!({"grantee": "reader", "privilege": "SELECT", "grantable": false});
    if first_owner {
        json!([owner, reader])
    } else {
        json!([reader, owner])
    }
}

fn customer(email_not_null: bool, columns_forward: bool, owner_privilege_first: bool) -> Value {
    let mut columns = vec![
        column(1, "tenant_id", 23, "int4", true, None),
        column(2, "id", 23, "int4", true, None),
        column(
            3,
            "email",
            25,
            "text",
            email_not_null,
            Some(comment(COMMENT_BODY)),
        ),
    ];
    if !columns_forward {
        columns.reverse();
    }
    json!({
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
        "columns": columns,
        "primary_key": [{"attnum": 1, "name": "tenant_id"}, {"attnum": 2, "name": "id"}],
        "primary_key_nulls_not_distinct": false,
        "unique_keys": [],
        "foreign_keys": [],
        "policies": [],
        "privileges": privileges(owner_privilege_first)
    })
}

fn purchase() -> Value {
    json!({
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
        "privileges": privileges(true)
    })
}

fn shop(email_not_null: bool, customer_before_purchase: bool, columns_forward: bool) -> String {
    let schema = if email_not_null {
        STALE_SCHEMA
    } else {
        SHOP_SCHEMA
    };
    let customer = customer(email_not_null, columns_forward, customer_before_purchase);
    let purchase = purchase();
    let tables = if customer_before_purchase {
        json!([customer, purchase])
    } else {
        json!([purchase, customer])
    };
    serde_json::to_string(&json!({
        "protocol": "oo.source.pgsql/v1",
        "core_contract": "oo.config/v1",
        "source_id": "shop_src",
        "database": "shop",
        "captured_at": CAPTURED_AT,
        "schema_hash": schema,
        "hash_algorithm": "pg-catalog-hash/v1",
        "ident_algorithm": "pgident/v1",
        "tables": tables,
        "type_conversion": "unsupported"
    }))
    .unwrap()
}

fn empty_catalog(schema_hash: &str) -> String {
    serde_json::to_string(&json!({
        "protocol": "oo.source.pgsql/v1",
        "core_contract": "oo.config/v1",
        "source_id": "shop_src",
        "database": "shop",
        "captured_at": CAPTURED_AT,
        "schema_hash": schema_hash,
        "hash_algorithm": "pg-catalog-hash/v1",
        "ident_algorithm": "pgident/v1",
        "tables": [],
        "type_conversion": "unsupported"
    }))
    .unwrap()
}

fn dataset<'a>(draft: &'a Value, layer: &str, name: &str) -> &'a Value {
    draft[layer]["datasets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["dataset"] == name)
        .unwrap()
}

fn column_of<'a>(dataset: &'a Value, name: &str) -> &'a Value {
    dataset["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == name)
        .unwrap()
}

fn queue_item<'a>(draft: &'a Value, item_id: &str, kind: &str) -> &'a Value {
    draft["review_queue"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["item_id"] == item_id && item["kind"] == kind)
        .unwrap_or_else(|| panic!("missing {item_id} {kind}"))
}

fn assert_shop_structure(draft: &Value) {
    assert_eq!(
        dataset(draft, "generated", "public.customer")["primary_key"],
        json!(["tenant_id", "id"])
    );
    assert_eq!(
        dataset(draft, "generated", "public.purchase")["primary_key"],
        json!(["tenant_id", "id"])
    );
    let relationship = &draft["generated"]["relationships"][0];
    assert_eq!(relationship["name"], "purchase_customer_fk");
    assert_eq!(relationship["provenance"], "declared_fk");
    assert_eq!(relationship["status"], "draft");
    assert!(relationship["confidence"].is_null());
    assert_eq!(relationship["join_hint"], "left");
    assert_eq!(relationship["cardinality"], "many_to_one");
    assert_eq!(
        relationship["from"]["columns"],
        json!(["tenant_id", "customer_id"])
    );
    assert_eq!(relationship["to"]["columns"], json!(["tenant_id", "id"]));
    assert_eq!(relationship["to"]["dataset"], "public.customer");
    let email = column_of(dataset(draft, "effective", "public.customer"), "email");
    assert!(email["pii"].is_null());
    assert!(email["semantic_type"].is_null());
    assert_eq!(email["data_type"], "text");
    assert_eq!(
        queue_item(draft, "pii/public.customer/email", "pii_unknown")["state"],
        "pending"
    );
    assert_eq!(
        draft["provenance"]["pg_catalog_integration"],
        "not_accepted"
    );
    assert_eq!(
        draft["provenance"]["pg_catalog_contract_commit"],
        "b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74"
    );
    assert_eq!(draft["provenance"]["publication"], "unavailable");
    assert_eq!(draft["provenance"]["certification"], "none");
    assert_eq!(draft["provenance"]["llm"], "not_used");
    assert_eq!(draft["provenance"]["source_connection"], "not_used");
}

fn assert_no_secret(bytes: &str) {
    assert!(!bytes.contains(COMMENT_BODY));
    assert!(!bytes.contains(OWNER));
}

#[test]
fn empty_catalog_seals_historical_not_accepted() {
    let compiled = compile_captured_pg_catalog(&empty_catalog(EMPTY_SCHEMA_HASH), None, None)
        .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(compiled.protocol, CAPTURED_PROTOCOL);
    assert_eq!(compiled.compiler, CAPTURED_COMPILER);
    assert_eq!(compiled.schema_hash, EMPTY_SCHEMA_HASH);
    assert!(!compiled.draft.ends_with('\n'));
    assert!(!compiled.evidence_envelope.ends_with('\n'));
    assert!(!compiled.wrapper.ends_with('\n'));
    assert_eq!(
        compile_structure_draft(&compiled.evidence_envelope).unwrap(),
        compiled.draft
    );
    assert_eq!(compiled.draft_hash, sha256_hex(compiled.draft.as_bytes()));
    assert_eq!(
        compiled.evidence_envelope_hash,
        sha256_hex(compiled.evidence_envelope.as_bytes())
    );
    let draft: Value = serde_json::from_str(&compiled.draft).unwrap();
    assert_eq!(draft["protocol"], DRAFT_PROTOCOL);
    assert_eq!(draft["compiler"], COMPILER);
    assert_eq!(
        draft["provenance"]["pg_catalog_integration"],
        "not_accepted"
    );
    assert_eq!(draft["provenance"]["manifest_hash"], compiled.manifest_hash);
    assert!(
        draft["generated"]["datasets"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let wrapper: Value = serde_json::from_str(&compiled.wrapper).unwrap();
    assert_eq!(wrapper["protocol"], CAPTURED_PROTOCOL);
    assert_eq!(wrapper["live_catalog_review"], "pending");
    assert_eq!(wrapper["row_import"], "not_proven");
    assert_eq!(wrapper["grants"], "not_authorized");
    assert_eq!(wrapper["publication"], "unavailable");
    assert_eq!(
        wrapper["historical"]["pg_catalog_integration"],
        "not_accepted"
    );
    assert_eq!(wrapper["wrapper_hash"], compiled.wrapper_hash);
    assert_eq!(wrapper["connector_protocol"], "oo.source.pgsql/v1");
    assert!(compiled.evidence_envelope.contains(EVIDENCE_PROTOCOL));
    let again =
        compile_captured_pg_catalog(&empty_catalog(EMPTY_SCHEMA_HASH), None, Some("[]")).unwrap();
    assert_eq!(again.evidence_envelope, compiled.evidence_envelope);
    assert_eq!(again.draft, compiled.draft);
    assert_eq!(again.wrapper, compiled.wrapper);
}

#[test]
fn shop_draft_keeps_composite_key_fk_and_unknown_pii() {
    let compiled = compile_captured_pg_catalog(&shop(false, true, true), None, None).unwrap();
    let draft: Value = serde_json::from_str(&compiled.draft).unwrap();
    assert_shop_structure(&draft);
    assert_eq!(compiled.schema_hash, SHOP_SCHEMA);
    assert_no_secret(&compiled.draft);
    assert_no_secret(&compiled.wrapper);
    assert!(!compiled.evidence_envelope.contains(COMMENT_BODY));
    assert!(compiled.evidence_envelope.contains(OWNER));
    assert_eq!(
        compile_structure_draft(&compiled.evidence_envelope).unwrap(),
        compiled.draft
    );
    let wrapper: Value = serde_json::from_str(&compiled.wrapper).unwrap();
    assert_eq!(wrapper["schema_hash"], SHOP_SCHEMA);
    assert_eq!(wrapper["live_catalog_review"], "pending");
    assert_eq!(wrapper["row_import"], "not_proven");
    assert_eq!(wrapper["grants"], "not_authorized");
    assert_eq!(wrapper["draft_hash"], compiled.draft_hash);
    assert_eq!(
        wrapper["captured_catalog_hash"],
        compiled.captured_catalog_hash
    );
}

#[test]
fn equivalent_catalog_reorder_keeps_the_same_bytes() {
    let forward = compile_captured_pg_catalog(&shop(false, true, true), None, None).unwrap();
    let reversed = compile_captured_pg_catalog(&shop(false, false, false), None, None).unwrap();
    assert_ne!(shop(false, true, true), shop(false, false, false));
    assert_eq!(forward.draft, reversed.draft);
    assert_eq!(forward.evidence_envelope, reversed.evidence_envelope);
    assert_eq!(forward.wrapper, reversed.wrapper);
    assert_eq!(
        forward.captured_catalog_hash,
        reversed.captured_catalog_hash
    );
}

#[test]
fn fresh_curated_label_stays_out_of_pii_and_accept_does_not_approve_the_catalog() {
    let first = compile_captured_pg_catalog(&shop(false, true, true), None, None).unwrap();
    let draft: Value = serde_json::from_str(&first.draft).unwrap();
    let fingerprint =
        queue_item(&draft, "column/public.customer/email", "structural_column")["fact_fingerprint"]
            .as_str()
            .unwrap();
    let curated = json!({
        "datasets": [{
            "dataset": "public.customer",
            "columns": [{"name": "email", "label": "Email address", "pii": "email"}]
        }],
        "relationships": []
    });
    let reviews = json!([{
        "item_id": "column/public.customer/email",
        "action": "override",
        "schema_hash": first.schema_hash,
        "fact_fingerprint": fingerprint
    }]);
    let compiled = compile_captured_pg_catalog(
        &shop(false, true, true),
        Some(&curated.to_string()),
        Some(&reviews.to_string()),
    )
    .unwrap();
    let draft: Value = serde_json::from_str(&compiled.draft).unwrap();
    let effective = column_of(dataset(&draft, "effective", "public.customer"), "email");
    assert_eq!(effective["label"], "Email address");
    assert!(effective["pii"].is_null());
    assert_eq!(
        draft["curated"]["datasets"][0]["columns"][0]["pii"],
        "email"
    );
    assert_eq!(
        queue_item(
            &draft,
            "pii/public.customer/email",
            "semantic_field_not_applied"
        )["detail"],
        "pii"
    );
    assert_eq!(
        queue_item(&draft, "pii/public.customer/email", "pii_unknown")["state"],
        "pending"
    );
    let wrapper: Value = serde_json::from_str(&compiled.wrapper).unwrap();
    assert_eq!(wrapper["live_catalog_review"], "pending");
    assert_eq!(
        wrapper["historical"]["pg_catalog_integration"],
        "not_accepted"
    );
    assert_eq!(
        draft["provenance"]["pg_catalog_integration"],
        "not_accepted"
    );
    assert!(!compiled.draft.contains(COMMENT_BODY));
}

#[test]
fn schema_change_stales_the_prior_decision_and_keeps_the_curated_echo() {
    let first = compile_captured_pg_catalog(&shop(false, true, true), None, None).unwrap();
    let draft: Value = serde_json::from_str(&first.draft).unwrap();
    let fingerprint =
        queue_item(&draft, "column/public.customer/email", "structural_column")["fact_fingerprint"]
            .as_str()
            .unwrap();
    let curated = r#"{"datasets":[{"dataset":"public.customer","columns":[{"name":"email","label":"Email address","pii":"email"}]}],"relationships":[]}"#;
    let reviews = json!([{
        "item_id": "column/public.customer/email",
        "action": "override",
        "schema_hash": first.schema_hash,
        "fact_fingerprint": fingerprint
    }]);
    let compiled = compile_captured_pg_catalog(
        &shop(true, true, true),
        Some(curated),
        Some(&reviews.to_string()),
    )
    .unwrap();
    assert_eq!(compiled.schema_hash, STALE_SCHEMA);
    assert_ne!(compiled.schema_hash, first.schema_hash);
    let draft: Value = serde_json::from_str(&compiled.draft).unwrap();
    let effective = column_of(dataset(&draft, "effective", "public.customer"), "email");
    assert_eq!(effective["label"], "email");
    assert!(effective["pii"].is_null());
    assert_eq!(
        draft["curated"]["datasets"][0]["columns"][0]["label"],
        "Email address"
    );
    assert_eq!(
        queue_item(&draft, "column/public.customer/email", "stale_decision")["detail"],
        "stale_schema_hash"
    );
    assert_eq!(
        queue_item(&draft, "column/public.customer/email", "stale_decision")["state"],
        "pending"
    );
    assert_eq!(
        queue_item(&draft, "column/public.customer/email", "curated_unbound")["state"],
        "pending"
    );
    let wrapper: Value = serde_json::from_str(&compiled.wrapper).unwrap();
    assert_eq!(wrapper["live_catalog_review"], "pending");
    assert_eq!(
        draft["provenance"]["pg_catalog_integration"],
        "not_accepted"
    );
}

#[test]
fn captured_catalog_rejects_bad_hash_conversion_secrets_and_size() {
    let err =
        compile_captured_pg_catalog(&empty_catalog(&"ab".repeat(32)), None, None).unwrap_err();
    assert_eq!(err.code, format!("{EVIDENCE_PROTOCOL}/hash.schema"));
    assert_eq!(err.path, "/catalog/schema_hash");

    let mut converted: Value = serde_json::from_str(&shop(false, true, true)).unwrap();
    converted["tables"][0]["columns"][2]["type_support"]["converted"] = json!(true);
    let err = compile_captured_pg_catalog(&converted.to_string(), None, None).unwrap_err();
    assert_eq!(err.code, format!("{EVIDENCE_PROTOCOL}/type.converted"));

    let mut body: Value = serde_json::from_str(&shop(false, true, true)).unwrap();
    body["tables"][0]["columns"][2]["comment"]["body"] = json!(COMMENT_BODY);
    let err = compile_captured_pg_catalog(&body.to_string(), None, None).unwrap_err();
    assert_eq!(err.code, format!("{EVIDENCE_PROTOCOL}/field.unknown"));

    let mut protocol: Value = serde_json::from_str(&empty_catalog(EMPTY_SCHEMA_HASH)).unwrap();
    protocol["protocol"] = json!("oo.source.pgsql/v0");
    let err = compile_captured_pg_catalog(&protocol.to_string(), None, None).unwrap_err();
    assert_eq!(err.code, format!("{EVIDENCE_PROTOCOL}/version.unsupported"));

    let oversized = "a".repeat(1_048_577);
    let err = compile_captured_pg_catalog(&oversized, None, None).unwrap_err();
    assert_eq!(err.code, format!("{EVIDENCE_PROTOCOL}/json.budget.bytes"));
}
