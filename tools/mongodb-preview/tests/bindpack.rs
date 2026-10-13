use mongodb_preview::bindpack::{self, PackInput};
use serde_json::json;

fn seal() -> serde_json::Value {
    json!({
        "rows": [
            {
                "outcome": "sealed",
                "extended_json": {
                    "_id": {"$oid": "c141d04141eef04100000001"},
                    "code": "S41-001",
                    "flag": 101,
                    "weight": {"$numberDouble": "42.25"}
                },
                "weight": "42.25"
            },
            {
                "outcome": "scope_refused",
                "code": "oo.document.bson-jsonb/v1/field.scope",
                "extended_json": {
                    "_id": {"$oid": "c141d04141eef04100000000"},
                    "code": "S41-000",
                    "hiddenNote": "not-approved-field",
                    "weight": {"$numberDouble": "41.25"}
                },
                "weight": "41.25"
            }
        ]
    })
}

fn input<'a>(seal: &'a serde_json::Value, seal_bytes: &'a [u8], audit: &'a [u8]) -> PackInput<'a> {
    PackInput {
        seal,
        seal_bytes,
        audit_bytes: audit,
        unclassified: None,
        unclassified_bytes: None,
        driver_bytes: b"mongodb-3.3.0-fixture",
        generation: 1,
        parent: None,
        full_snapshot: false,
        arrow_path: "/work/gen1/rows.arrow",
        receipt_path: "/work/gen1/receipt.json",
        registry_path: "/work/gen1/registry.json",
        catalog_path: "/work/gen1/catalog.json",
        audit_path: "/work/gen1/audit.json",
        driver_path: "/work/gen1/mongodb-driver",
        password_file: "/work/fixture.pw",
        database: "oo_w141",
        port: 5432,
        shard: "shard_w141",
        owner: "41a14141-1414-4414-8414-000000000041",
    }
}

#[test]
fn pack_keeps_shipments_and_drops_the_hidden_row() {
    let seal = seal();
    let seal_bytes = serde_json::to_vec(&seal).unwrap();
    let audit = br#"{"reader_dml":0,"rows":[]}"#;
    let packed = bindpack::build_pack(&input(&seal, &seal_bytes, audit)).expect("pack");
    assert_eq!(packed.summary["catalog_table"], "oo_w141.shipments");
    assert_eq!(packed.summary["sealed_rows"], 1);
    assert_eq!(packed.summary["source_acceptance"], false);
    assert_eq!(packed.summary["fulldoc_complete"], false);
    let receipt = serde_json::from_slice::<serde_json::Value>(&packed.receipt).unwrap();
    assert_eq!(receipt["schema"], "oo.source-admission.capture-receipt/v1");
    assert_eq!(receipt["source_engine"], "mongodb");
    assert_eq!(receipt["source_id"], "oo_w141");
    assert_eq!(receipt["consistency"], "B");
    assert_ne!(receipt["driver_digest"], mongodb_preview::FROZEN_DRIVER_DIGEST);
    assert_eq!(receipt["registry_sha256"], packed.summary["registry_sha256"]);
    assert_ne!(receipt["registry_sha256"], packed.summary["registry_file_sha256"]);
    let request = serde_json::from_slice::<serde_json::Value>(&packed.request).unwrap();
    assert_eq!(
        request["admission_expect"]["declared_receipt_sha256"],
        packed.summary["receipt_sha256"]
    );
    assert_eq!(request["logical_schema"], "oo_w141");
    assert_eq!(request["logical_table"], "shipments");
    assert_eq!(request["columns"][0]["name"], "_id");
    assert_eq!(request["registry_path"], "/work/gen1/registry.json");
    assert!(request.get("source_bound").is_none());
    assert!(request.get("lease_id").is_none());
    assert!(request.get("trusted").is_none());
    let registry = serde_json::from_slice::<serde_json::Value>(&packed.registry).unwrap();
    let stamp = registry["bindings"][0]["admission_receipt_sha256"].as_str().unwrap();
    assert_eq!(stamp, packed.summary["receipt_sha256"].as_str().unwrap());
    assert_eq!(stamp.matches(stamp).count(), 1);
    assert_eq!(
        String::from_utf8(packed.registry.clone()).unwrap().matches(stamp).count(),
        1
    );
    let catalog = String::from_utf8(packed.catalog.clone()).unwrap();
    assert!(!catalog.contains("public.docs"));
    assert!(catalog.contains("oo_w141"));
    let census = serde_json::from_slice::<serde_json::Value>(&packed.census).unwrap();
    assert_eq!(census["deleted_in_sealed"], false);
    assert_eq!(census["sealed_keys"][0], "c141d04141eef04100000001");
    assert_eq!(census["approved_field_count"], 19);
    assert_eq!(census["id_renamed"], false);
    assert_eq!(census["refused"][0]["raw_doc_authorized"], false);
    assert_eq!(census["refused"][0]["classification"], "unclassified");
    assert_eq!(census["refused"][0]["field"], "hiddenNote");
    assert_eq!(census["refused"][0]["value_type"], "string");
    assert_eq!(census["refused"][0]["loaded"], false);
    assert!(packed.arrow.starts_with(b"ARROW1"));
    assert!(packed.arrow.ends_with(b"ARROW1"));
}

#[test]
fn a_sealed_hidden_note_is_not_stripped() {
    let seal = json!({
        "rows": [{
            "outcome": "sealed",
            "extended_json": {
                "_id": {"$oid": "c141d04141eef04100000001"},
                "hiddenNote": "not-approved-field",
                "weight": {"$numberDouble": "42.25"}
            },
            "weight": "42.25"
        }]
    });
    let seal_bytes = serde_json::to_vec(&seal).unwrap();
    let audit = br#"{"reader_dml":0}"#;
    match bindpack::build_pack(&input(&seal, &seal_bytes, audit)) {
        Err(err) => assert_eq!(err.code, "field.scope"),
        Ok(_) => panic!("hiddenNote was sealed"),
    }
}

#[test]
fn jsonb_text_sorts_keys_and_keeps_the_double_token() {
    let value = json!({"weight": {"$numberDouble": "41.25"}, "b": 1, "aa": 2});
    assert_eq!(
        bindpack::pg_jsonb_text(&value),
        r#"{"b": 1, "aa": 2, "weight": {"$numberDouble": "41.25"}}"#
    );
}

#[test]
fn reader_dml_blocks_the_pack() {
    let seal = seal();
    let seal_bytes = serde_json::to_vec(&seal).unwrap();
    let audit = br#"{"reader_dml":1}"#;
    match bindpack::build_pack(&input(&seal, &seal_bytes, audit)) {
        Err(err) => assert_eq!(err.code, "no_audit"),
        Ok(_) => panic!("reader DML was accepted"),
    }
}
