use selfbuild_control_api::library::projection;
use serde_json::json;

#[test]
fn library_overlay_uses_physical_types_and_never_invents_money_units() {
    let facts = json!({"tables":[{"dataset":"app.items","columns":[
        {"name":"amount","data_type":"numeric(18,2)","classification_candidate":{"semantic_type":"money","pii":"none"}},
        {"name":"phone","data_type":"text","classification_candidate":{"semantic_type":"phone","pii":"personal"}}
    ]}],"relationship_candidates":[]});
    let (catalog, rules) = projection(&facts).unwrap();
    assert_eq!(
        catalog["tables"][0]["columns"][0]["data_type"],
        "numeric(18,2)"
    );
    assert_eq!(rules["columns"][0]["semantic_type"], "quantity");
    assert_eq!(rules["columns"][1]["pii"], "personal");
    assert!(rules["columns"][0].get("unit").is_none());
}

#[test]
fn missing_global_inventory_is_refused() {
    assert!(projection(&json!({"tables":[],"relationship_candidates":false})).is_err());
    assert!(
        projection(
            &json!({"tables":[{"dataset":"unqualified","columns":[]}],"relationship_candidates":[]})
        )
        .is_err()
    );
}
