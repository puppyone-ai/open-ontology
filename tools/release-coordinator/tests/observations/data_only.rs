//! Synthetic transition vectors, not actual capture receipts.
use super::*;
use serde_json::json;

fn rehash(context: &mut Value) {
    let receipt = &mut context["publication"]["receipt"];
    receipt["shards"][0]["receipt_sha256"] =
        json!(crate::canon::canonical_sha(&receipt["shards"][0]["receipt"]));
    context["publication"]["receipt_sha256"] = json!(crate::canon::canonical_sha(receipt));
}

fn context(generation: i64) -> Value {
    let mut receipt: Value = serde_json::from_str(include_str!("../fixtures/oracle_c1_library.json")).unwrap();
    let run = format!("00000000-0000-4000-8000-{generation:012}");
    receipt["shards"][0]["anchor_run"] = json!(run);
    let vector = &mut receipt["shards"][0]["receipt"];
    vector["run_id"] = json!(run);
    vector["run_ids"] = json!([run]);
    vector["source_generation"] = json!(generation);
    vector["target_generation"] = json!(generation);
    vector["applied_pos"] = json!(generation.to_string());
    vector["oracle_buffer"]["final_cursor"]["transaction_ordinal"] = json!(generation.to_string());
    for table in vector["tables"].as_array_mut().unwrap() {
        table["run_id"] = json!(run);
        table["source_generation"] = json!(generation);
        table["target_generation"] = json!(generation);
        table["applied_pos"] = json!(generation.to_string());
        table["accepted_rows"] = json!(23 + generation);
        table["canonical_digest"] = json!(format!("{generation:064x}"));
        table["physical_name"] = json!(format!("pt_{generation:060x}"));
    }
    let mut mapping = json!({
        "datasets":[{"dataset":"q_415050.q_4954454d53","source":"oracle_fixture",
                     "config_digest":"a".repeat(64),"generation":generation}],
        "publication":{"protocol":PROTOCOL,"run_id":run,"receipt":receipt,"receipt_sha256":""}
    });
    rehash(&mut mapping);
    mapping
}

#[test]
fn data_only_advances_original_c1_vector_without_changing_membership() {
    let old = context(1);
    let next = context(2);
    data_only_successor(&old, &next).unwrap();
    assert!(data_only_successor(&old, &old).is_err());
    assert!(data_only_successor(&next, &old).is_err());
}

#[test]
fn data_only_rejects_identity_types_rejects_and_transport_tampering() {
    let old = context(1);
    for (path, bad) in [
        ("/source_epoch", json!(2)),
        ("/oracle_buffer/typed_schema_sha256", json!("c".repeat(64))),
        ("/consistency_grade", json!("S0")),
        ("/cross_table_consistent", json!(false)),
        ("/tables/0/rejects", json!([{"error":"typed_value"}])),
        ("/tables/0/storage_table", json!("foreign")),
        ("/oracle_buffer/source_identity/dbid", json!("other")),
        ("/oracle_buffer/final_cursor/transaction_ordinal", json!("1")),
        ("/target_generation", json!(1)),
    ] {
        let mut next = context(2);
        *next["publication"]["receipt"]["shards"][0]["receipt"].pointer_mut(path).unwrap() = bad;
        rehash(&mut next);
        assert!(data_only_successor(&old, &next).is_err(), "{path}");
    }
    let mut next = context(2);
    next["datasets"][0]["config_digest"] = json!("b".repeat(64));
    assert!(data_only_successor(&old, &next).is_err());
    let mut next = context(2);
    next["publication"]["receipt_sha256"] = json!("0".repeat(64));
    assert!(data_only_successor(&old, &next).is_err());
}

#[test]
fn data_only_rejects_new_generation_without_new_capture_position() {
    let old = context(1);
    let mut next = context(2);
    let receipt = &mut next["publication"]["receipt"]["shards"][0]["receipt"];
    receipt["applied_pos"] = json!("1");
    receipt["tables"][0]["applied_pos"] = json!("1");
    receipt["oracle_buffer"]["final_cursor"]["transaction_ordinal"] = json!("1");
    rehash(&mut next);
    assert!(data_only_successor(&old, &next).is_err());
}

fn mysql_context(generation: i64) -> Value {
    let mut value = context(generation);
    let receipt = &mut value["publication"]["receipt"]["shards"][0]["receipt"];
    receipt.as_object_mut().unwrap().remove("oracle_buffer");
    receipt["position_codec"] = json!("mysql-gtid/v1");
    receipt["position_lineage"] = json!("mysql.11111111-1111-4111-8111-111111111111");
    rehash(&mut value);
    value
}

#[test]
fn mysql_data_only_advances_gtid_with_same_epoch_lineage_and_membership() {
    let old = mysql_context(1);
    let next = mysql_context(2);
    data_only_successor(&old, &next).unwrap();
    assert!(data_only_successor(&old, &old).is_err());
    assert!(data_only_successor(&next, &old).is_err());
    for (path, bad) in [
        ("/source_epoch", json!(2)),
        ("/position_lineage", json!("mysql.22222222-2222-4222-8222-222222222222")),
        ("/position_codec", json!("snapshot-zero/v1")),
        ("/applied_pos", json!("1")),
        ("/consistency_grade", json!("S0")),
        ("/cross_table_consistent", json!(false)),
        ("/target_generation", json!(1)),
        ("/tables/0/rejects", json!([{"error":"typed_value"}])),
    ] {
        let mut wrong = next.clone();
        *wrong["publication"]["receipt"]["shards"][0]["receipt"].pointer_mut(path).unwrap() = bad;
        rehash(&mut wrong);
        assert!(data_only_successor(&old, &wrong).is_err(), "{path}");
    }
    let mut wrong = next.clone();
    wrong["datasets"][0]["config_digest"] = json!("b".repeat(64));
    assert!(data_only_successor(&old, &wrong).is_err());
    assert!(data_only_successor(&old, &context(2)).is_err());
}
