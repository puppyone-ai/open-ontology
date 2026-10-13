use mongodb_preview::capture::{
    catch_up_json, converge, parse_request, periods_are_distinct, plan, SnapRow, APPROVED_BUDGET_ROWS,
    APPROVED_FIELDS,
};
use serde_json::{json, Value};

fn fields() -> Vec<&'static str> {
    APPROVED_FIELDS.to_vec()
}

fn base(method: &str) -> Value {
    let names = fields();
    json!({
        "protocol": "oo.mongodb-preview.sync-plan/v1",
        "method": method,
        "every_ms": 3000,
        "delete_mode": if method == "C1" { "native" } else { "reconcile" },
        "consistency": if method == "C1" { "T" } else { "B" },
        "token_order": if method == "C1" { "Total" } else { "None" },
        "quality": if method == "C1" { Vec::<&str>::new() } else { vec!["DELETES_LAGGED"] },
        "budget_rows": APPROVED_BUDGET_ROWS,
        "approved_fields": names,
        "projected_fields": names,
        "secret_ref": "secret://oo-dl01w1-41/reader",
        "secret_path_present": true,
        "topology": if method == "C1" { "replica" } else { "standalone" },
        "change_stream_privilege": method == "C1",
        "change_stream_server_code": if method == "C1" { json!(null) } else { json!(40573) },
        "resume_supported": method == "C1",
        "oplog_window_known": method == "C1",
        "delete_event_observed": method == "C1",
        "delete_event_keys": if method == "C1" { vec!["_id"] } else { Vec::<&str>::new() },
        "transaction_metadata": method == "C1",
        "write_class": if method == "C1" { "R1" } else { "R0" },
        "fulldoc_requested": false,
        "fulldoc_contains_double": true,
        "fulldoc_claim_complete": false,
        "observed_fields": names,
        "field_types": { "weight": ["double"], "lane": ["string", "long"] },
        "row_count": 16,
        "snapshot_token": null,
    })
}

fn decide(value: &Value) -> Value {
    let request = parse_request(value).expect("request");
    plan(&request).to_json()
}

fn codes(decision: &Value) -> Vec<String> {
    decision["blocks"]
        .as_array()
        .expect("blocks")
        .iter()
        .map(|item| item["code"].as_str().unwrap_or("").to_string())
        .collect()
}

#[test]
fn standalone_c1_is_refused_and_not_rewritten_as_c5() {
    let mut value = base("C1");
    value["topology"] = json!("standalone");
    value["change_stream_privilege"] = json!(false);
    value["resume_supported"] = json!(false);
    value["oplog_window_known"] = json!(false);
    value["delete_event_observed"] = json!(false);
    value["delete_event_keys"] = json!([]);
    value["transaction_metadata"] = json!(false);
    value["change_stream_server_code"] = json!(40573);
    let decision = decide(&value);
    assert_eq!(decision["outcome"], "cdc_refused");
    assert_eq!(decision["every_ms"], 3000);
    assert!(decision["capture_method"].is_null());
    assert_eq!(decision["cdc_claimed"], json!(false));
    assert_eq!(decision["cdc_read_allowed"], json!(false));
    assert_eq!(decision["read_allowed"], json!(false));
    assert_eq!(codes(&decision), vec!["CDC_TOPOLOGY_UNSUPPORTED".to_string()]);
    assert_eq!(decision["source_acceptance"], json!(false));
    assert_eq!(decision["import_invoked"], json!(false));
    assert_eq!(decision["typed_weight_retained"], json!(true));
}

#[test]
fn replica_without_transaction_metadata_does_not_downgrade() {
    let mut value = base("C1");
    value["transaction_metadata"] = json!(false);
    let decision = decide(&value);
    assert_eq!(decision["outcome"], "cdc_refused");
    assert!(decision["capture_method"].is_null());
    assert_eq!(codes(&decision), vec!["CDC_TRANSACTION_METADATA_ABSENT".to_string()]);
}

#[test]
fn replica_c1_admits_only_after_the_capability_checks() {
    let decision = decide(&base("C1"));
    assert_eq!(decision["outcome"], "c1_admitted");
    assert_eq!(decision["capture_method"], "C1");
    assert_eq!(decision["cdc_claimed"], json!(true));
    assert_eq!(decision["consistency"], "T");
    assert_eq!(decision["fulldoc_complete"], json!(false));
    assert_eq!(decision["fulldoc_produced"], json!(false));
    assert_eq!(decision["gaps"][0]["code"], "JSONB_DOUBLE_UNENCODED");
    assert_eq!(decision["gaps"][0]["owner"], "DL01-W1-05");
    assert_eq!(decision["gaps"][0]["also_owner"], "DL01-W1-06");
    assert_eq!(decision["source_acceptance"], json!(false));
}

#[test]
fn explicit_c5_discloses_grade_b_and_keeps_weight() {
    let mut value = base("C5");
    value["observed_fields"] = json!(fields().into_iter().chain(["hiddenNote"]).collect::<Vec<_>>());
    value["field_types"]["hiddenNote"] = json!(["int", "string"]);
    let decision = decide(&value);
    assert_eq!(decision["outcome"], "c5_admitted");
    assert_eq!(decision["every_ms"], 3000);
    assert_eq!(decision["capture_method"], "C5");
    assert_eq!(decision["cdc_claimed"], json!(false));
    assert_eq!(decision["consistency"], "B");
    assert_eq!(decision["delete_detection"], "reconcile");
    assert_eq!(decision["quality"][0], "DELETES_LAGGED");
    assert_eq!(decision["typed_weight_retained"], json!(true));
    assert_eq!(decision["approved_field_count"], 19);
    assert_eq!(decision["unclassified"][0]["field"], "hiddenNote");
    assert_eq!(decision["unclassified"][0]["visible"], json!(false));
    assert_eq!(decision["unclassified"][0]["projected"], json!(false));
    assert_eq!(decision["unclassified"][0]["raw_doc_authorized"], json!(false));
    assert_eq!(decision["unclassified"][0]["mixed"], json!(true));
    assert_eq!(decision["generation_created"], json!(false));
}

#[test]
fn presence_null_on_memo_and_pack_stays_admitted() {
    let mut value = base("C5");
    value["every_ms"] = json!(8000);
    value["field_types"]["memo"] = json!(["null", "string"]);
    value["field_types"]["pack"] = json!(["null", "object"]);
    value["field_types"]["lane"] = json!(["long", "string"]);
    value["observed_fields"] = json!(fields().into_iter().chain(["hiddenNote"]).collect::<Vec<_>>());
    value["field_types"]["hiddenNote"] = json!(["int", "string"]);
    let decision = decide(&value);
    assert_eq!(decision["outcome"], "c5_admitted");
    assert_eq!(decision["every_ms"], 8000);
    assert!(codes(&decision).is_empty());
    assert_eq!(decision["unclassified"][0]["field"], "hiddenNote");
    assert_eq!(decision["typed_weight_retained"], json!(true));
}

#[test]
fn mixed_approved_scalar_is_blocked() {
    let mut value = base("C5");
    value["field_types"]["code"] = json!(["int", "string"]);
    let decision = decide(&value);
    assert_eq!(decision["outcome"], "blocked");
    let found = codes(&decision);
    assert!(found.contains(&"TYPE_DRIFT".to_string()));
    assert!(found.contains(&"MIXED_TYPE_PROJECTED".to_string()));
}

#[test]
fn c5_without_disclosure_or_with_a_shrunk_field_list_is_blocked() {
    let mut quiet = base("C5");
    quiet["consistency"] = json!("T");
    quiet["quality"] = json!([]);
    let decision = decide(&quiet);
    assert_eq!(decision["outcome"], "blocked");
    assert!(codes(&decision).contains(&"CONSISTENCY_OVERCLAIM".to_string()));
    assert!(codes(&decision).contains(&"QUALITY_MISMATCH".to_string()));

    let mut shrunk = base("C5");
    let mut names = fields();
    names.retain(|field| *field != "weight");
    shrunk["approved_fields"] = json!(names.clone());
    shrunk["projected_fields"] = json!(names);
    shrunk["fulldoc_contains_double"] = json!(false);
    let decision = decide(&shrunk);
    assert_eq!(decision["outcome"], "blocked");
    assert!(codes(&decision).contains(&"FIELD_SET_CHANGED".to_string()));
    assert_eq!(decision["typed_weight_retained"], json!(false));
}

#[test]
fn complete_fulldoc_with_a_double_is_a_shared_block() {
    let mut value = base("C5");
    value["fulldoc_requested"] = json!(true);
    value["fulldoc_claim_complete"] = json!(true);
    let decision = decide(&value);
    assert_eq!(decision["outcome"], "blocked");
    assert_eq!(decision["fulldoc_complete"], json!(false));
    assert_eq!(decision["fulldoc_produced"], json!(false));
    let found = codes(&decision);
    assert!(found.contains(&"FULLDOC_NOT_PRODUCED".to_string()));
    assert!(found.contains(&"PRECISION_LOSS_CONCEALED".to_string()));
    assert_eq!(decision["blocks"][0]["owner"], "DL01-W1-05");
}

#[test]
fn missing_secret_and_budget_stop_before_a_read() {
    let mut value = base("C5");
    value["secret_path_present"] = json!(false);
    value["row_count"] = json!(17);
    let decision = decide(&value);
    assert_eq!(decision["read_allowed"], json!(false));
    let found = codes(&decision);
    assert!(found.contains(&"SECRET_PATH_MISSING".to_string()));
    assert!(found.contains(&"SOURCE_BUDGET_EXHAUSTED".to_string()));
}

#[test]
fn catch_up_tracks_insert_update_and_delete_without_resurrection() {
    let before = vec![
        SnapRow { key: "a".to_string(), canonical: "one".to_string() },
        SnapRow { key: "b".to_string(), canonical: "old".to_string() },
        SnapRow { key: "c".to_string(), canonical: "keep".to_string() },
    ];
    let after = vec![
        SnapRow { key: "a".to_string(), canonical: "one".to_string() },
        SnapRow { key: "b".to_string(), canonical: "new".to_string() },
        SnapRow { key: "d".to_string(), canonical: "added".to_string() },
    ];
    let caught = converge(&before, &after, &["c".to_string()]).expect("diff");
    assert_eq!(caught.inserts, vec!["d".to_string()]);
    assert_eq!(caught.updates, vec!["b".to_string()]);
    assert_eq!(caught.deletes, vec!["c".to_string()]);
    assert_eq!(caught.published, vec!["a".to_string(), "b".to_string(), "d".to_string()]);
    assert!(caught.converged);
    assert!(!caught.published.iter().any(|key| key == "c"));
    let body = catch_up_json(&caught);
    assert_eq!(body["import_invoked"], json!(false));
    assert_eq!(converge(&before, &after, &["b".to_string()]).unwrap_err(), "resurrected");
    assert!(periods_are_distinct(3000, 8000));
    assert!(!periods_are_distinct(3000, 3000));
    assert!(!periods_are_distinct(0, 8000));
}
