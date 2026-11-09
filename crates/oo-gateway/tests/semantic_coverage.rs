use oo_gateway::{pack_catalog::PackCatalog, semantic_coverage::CoverageIndex};
use serde_json::{Value, json};
use std::fs;

#[test]
fn compiled_role_pack_indexes_semantics_without_hidden_or_unreviewed_terms() {
    let mut effective: Value = serde_json::from_str(include_str!(
        "../../../testing/real-db/gateway-compiled-pack/fixtures/effective.json"
    ))
    .unwrap();
    let visibility: Value = serde_json::from_str(include_str!(
        "../../../testing/real-db/gateway-compiled-pack/fixtures/visibility-picker.json"
    ))
    .unwrap();
    effective["datasets"][0]["columns"][0]["data_type"] = json!("bigint");
    effective["datasets"][0]["columns"][1]["data_type"] = json!("text");
    effective["datasets"][0]["columns"][1]["label"] = json!("Warehouse annotation");
    effective["datasets"][0]["columns"][1]["description"] = json!("Dispatch commentary");
    effective["datasets"][0]["columns"][2]["label"] = json!("Privatecanary");
    effective["entities"] = json!([{"schema_version":"oos/v1","entity":"parcel","label":"Package",
        "synonyms":["Freight parcel", "托运货物"],
        "primary_dataset":"shop.orders","key":"order_id","display":"picker_note",
        "datasets":["shop.orders"],"members":[]}]);
    effective["metrics"] = json!([{"metric":"order_total","label":"Booking volume",
        "owner":"fixture","certified":true,"status":"active","synonyms":["parcel tally"],
        "default_anchor":"month_start","tests":[{"params":{},"expect":{"rows":1}}],
        "control_total":{"as_of":"month_start","source":"shop",
            "sql":"SELECT count(order_id) AS total FROM shop.orders","tolerance":{"abs":"0"}},
        "sql":"SELECT count(order_id) AS total FROM shop.orders"},
        {"metric":"future_total","label":"Forecastcanary","owner":"fixture",
        "certified":false,"status":"draft","sql":"SELECT count(order_id) AS total FROM shop.orders"}]);
    effective["enums"] = json!([{"schema_version":"oos/v1","enum":"shop.orders.picker_note",
        "classes":{"ready":"prepared","unexplained":"unconfirmed"},
        "values":[{"code":"R","label":"Dispatched","class":"ready","status":"approved"},
            {"code":"U","label":"Unknowncanary","class":"unexplained","status":"unexplained"}]}]);
    effective["glossary"] = json!([{"term":"Consignment","definition":"A shipment booking",
        "status":"approved","maps_to":{"dataset":"shop.orders"}}]);
    let compiled =
        oo_builder::compile_role_pack(&effective.to_string(), &visibility.to_string()).unwrap();
    let dir = std::env::temp_dir().join(format!("oo-coverage-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("pack.json"), &compiled.artifact).unwrap();
    let catalog = PackCatalog::load_dir(&dir).unwrap();
    let pack = catalog.find("depot", "oo_r_picker", "pv1", "kv1").unwrap();
    let index = CoverageIndex::from_pack(&catalog, &pack);
    for question in [
        "Warehouse annotation",
        "Package",
        "Booking volume",
        "parcel tally",
        "Dispatched",
        "Consignment",
        "Dispatch commentary",
        "Freight parcel",
        "托运货物",
    ] {
        assert_eq!(index.assess(question)["coverage"], "covered", "{question}");
    }
    for question in [
        "Privatecanary",
        "Forecastcanary",
        "Unknowncanary",
        "auditor phone",
    ] {
        assert_ne!(index.assess(question)["coverage"], "covered", "{question}");
    }
    assert!(
        catalog
            .find("depot", "oo_r_auditor", "pv1", "kv1")
            .is_none()
    );
    let mut wrong_role = pack;
    wrong_role.role = "oo_r_auditor".into();
    wrong_role.columns.clear();
    assert_eq!(
        CoverageIndex::from_pack(&catalog, &wrong_role).assess("Package")["coverage"],
        "not_covered"
    );
    fs::remove_dir_all(dir).unwrap();
}
