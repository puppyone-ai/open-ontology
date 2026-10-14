use std::collections::BTreeMap;

use mongodb_preview::c1::{
    BatchRef, Event, Store, apply_interface_gap, catalog_adapter, cluster_decimal, cmp_cluster,
    cmp_watermark, expiry_record, may_pull, op_index, position_expired, producer_wire,
    public_doc, question_bank, seal_delta, seal_initial, snapshot_before, source_gold,
    token_cluster, CLUSTER_CODEC,
};
use serde_json::json;

fn event(op: &str, key: Option<&str>, doc: Option<serde_json::Value>, op_index: u32) -> Event {
    Event {
        op: op.to_string(),
        key: key.map(str::to_string),
        doc,
        pre: None,
        cluster: 50,
        lsid: Some("sid".to_string()),
        txn: Some(7),
        op_index,
    }
}

#[test]
fn cluster_time_orders_and_the_resume_token_is_only_hashed() {
    assert!(cmp_cluster(cluster_decimal(2, 0), cluster_decimal(1, 99)).is_gt());
    assert_eq!(cluster_decimal(1, 1).to_string(), "4294967297");
    assert_eq!(token_cluster("826ACF682700000001").expect("token"), cluster_decimal(0x6ACF6827, 1));
    assert!(token_cluster("00").is_err());
    assert!(position_expired(286));
    assert!(position_expired(136));
    assert!(!position_expired(280));
    assert!(may_pull(1));
    assert!(!may_pull(2));
}

#[test]
fn snapshot_does_not_resurrect_a_deleted_key() {
    let mut store = Store::new();
    let mut rows = BTreeMap::new();
    rows.insert("a".to_string(), json!({"_id": "a", "memo": "old"}));
    store.load_snapshot(rows, 10);
    store
        .observe(Event {
            op: "delete".to_string(),
            key: Some("a".to_string()),
            doc: None,
            pre: Some(json!({"_id": "a"})),
            cluster: 20,
            lsid: None,
            txn: None,
            op_index: 1,
        })
        .expect("delete");
    let mut again = BTreeMap::new();
    again.insert("a".to_string(), json!({"_id": "a", "memo": "old"}));
    store.load_snapshot(again, 10);
    let body = store.applied_json("abc");
    assert!(body["rows"].as_array().expect("rows").is_empty());
    assert_eq!(body["tombstones"][0], "a");
    assert_eq!(body["position_codec"], CLUSTER_CODEC);
    assert_eq!(body["resume_token_compared"], false);
}

#[test]
fn a_multi_document_commit_is_atomic_and_replay_does_not_duplicate() {
    let mut store = Store::new();
    store.load_snapshot(BTreeMap::new(), 1);
    store
        .observe(event("update", Some("a"), Some(json!({"_id": "a", "memo": null, "secretNote": "hide-me"})), 1))
        .expect("buffer");
    assert!(store.applied_json("abc")["rows"].as_array().expect("rows").is_empty());
    assert_eq!(store.grade(), "S0");
    store
        .observe(event("update", Some("b"), Some(json!({"_id": "b"})), 2))
        .expect("buffer");
    store
        .observe(Event {
            op: "commit".to_string(),
            key: None,
            doc: None,
            pre: None,
            cluster: 60,
            lsid: Some("sid".to_string()),
            txn: Some(7),
            op_index: 3,
        })
        .expect("commit");
    assert_eq!(store.grade(), "T");
    assert!(store.null_memo);
    assert!(store.hidden.contains("secretNote"));
    assert_eq!(store.applied_json("abc")["rows"].as_array().expect("rows").len(), 2);
    let again = event("update", Some("a"), Some(json!({"_id": "a", "memo": null, "secretNote": "hide-me"})), 1);
    store.replay(&[again]).expect("replay");
    assert_eq!(store.applied_json("abc")["rows"].as_array().expect("rows").len(), 2);
    assert_eq!(store.applied_json("abc")["applied_pos"], "60");
}

#[test]
fn a_group_without_a_commit_event_applies_atomically_at_s0() {
    let mut store = Store::new();
    store
        .observe(event("update", Some("a"), Some(json!({"_id": "a", "memo": null})), 1))
        .expect("buffer");
    store
        .observe(event("update", Some("b"), Some(json!({"_id": "b"})), 2))
        .expect("buffer");
    assert!(store.applied_json("abc")["rows"].as_array().expect("rows").is_empty());
    store
        .observe(Event {
            op: "insert".to_string(),
            key: Some("c".to_string()),
            doc: Some(json!({"_id": "c"})),
            pre: None,
            cluster: 70,
            lsid: None,
            txn: None,
            op_index: 3,
        })
        .expect("close");
    assert_eq!(store.grade(), "S0");
    assert_eq!(store.txn_groups, 1);
    assert_eq!(store.multi_commits, 0);
    assert_eq!(store.applied_json("abc")["rows"].as_array().expect("rows").len(), 3);
    assert_eq!(store.applied_json("abc")["applied_pos"], "70");
    assert_eq!(store.applied_json("abc")["commit_event_seen"], false);
}

#[test]
fn abort_drops_the_buffer_and_the_apply_gap_names_the_closed_codec() {
    let mut store = Store::new();
    store
        .observe(event("insert", Some("a"), Some(json!({"_id": "a"})), 1))
        .expect("buffer");
    store
        .observe(Event {
            op: "abort".to_string(),
            key: None,
            doc: None,
            pre: None,
            cluster: 40,
            lsid: Some("sid".to_string()),
            txn: Some(7),
            op_index: 2,
        })
        .expect("abort");
    assert!(store.applied_json("abc")["rows"].as_array().expect("rows").is_empty());
    assert_eq!(store.grade(), "S0");
    let gap = apply_interface_gap();
    assert_eq!(gap["error"], "closed");
    assert_eq!(gap["policy_error"], "c1_requires_t");
    assert_eq!(gap["position_codec"], CLUSTER_CODEC);
    assert!(gap["closed_allow"]
        .as_array()
        .expect("allow")
        .iter()
        .all(|item| item != "mongo-cluster-time/v1"));
    assert_eq!(gap["loader_invoked"], false);
}

#[test]
fn same_cluster_time_uses_the_server_op_index() {
    let first = "826ACF6FE3000000012B042C010029";
    let second = "826ACF6FE3000000012B042C01002B02";
    assert_eq!(token_cluster(first).expect("cluster"), token_cluster(second).expect("cluster"));
    assert_eq!(op_index(first).expect("index"), 0);
    assert_eq!(op_index(second).expect("index"), 1);
    assert!(op_index("00").is_err());
    assert!(cmp_watermark("rs0", 1, 10, "rs0", 1, 11).expect("order").is_lt());
    assert!(cmp_watermark("rs0", 1, 10, "rs1", 1, 11).is_err());
    assert!(cmp_watermark("rs0", 1, 10, "rs0", 2, 11).is_err());
    let expired = expiry_record(286, "ChangeStreamHistoryLost", "lost", false);
    assert_eq!(expired["status"], "POSITION_EXPIRED");
    assert_eq!(expired["ack"], false);
    assert_eq!(expired["skipped_gap"], false);
    assert_eq!(expired["continued"], false);
    assert_eq!(expired["resumed_from_latest"], false);
    assert_eq!(expired["recovery"], "rebaseline_cdc_first");
    assert_eq!(expired["full_snapshot"], false);
    assert_eq!(expiry_record(280, "ChangeStreamFatalError", "bad", false)["status"], "not_expired");

    let mut store = Store::new();
    store.lineage = "rs0".to_string();
    store.epoch = 1;
    let newer = Event {
        op: "update".to_string(),
        key: Some("a".to_string()),
        doc: Some(json!({"_id": "a", "memo": "newer"})),
        pre: Some(json!({"_id": "a", "memo": "older"})),
        cluster: 50,
        lsid: Some("sid".to_string()),
        txn: Some(7),
        op_index: 1,
    };
    let older = Event {
        op: "update".to_string(),
        key: Some("a".to_string()),
        doc: Some(json!({"_id": "a", "memo": "older"})),
        pre: Some(json!({"_id": "a", "memo": "base"})),
        cluster: 50,
        lsid: Some("sid".to_string()),
        txn: Some(7),
        op_index: 0,
    };
    store.observe(newer.clone()).expect("buffer");
    store.observe(older.clone()).expect("buffer");
    store
        .observe(Event {
            op: "insert".to_string(),
            key: Some("c".to_string()),
            doc: Some(json!({"_id": "c"})),
            pre: None,
            cluster: 70,
            lsid: None,
            txn: None,
            op_index: 0,
        })
        .expect("close");
    let body = store.applied_json("abc");
    assert_eq!(body["grade"], "S0");
    assert_eq!(body["commit_event_seen"], false);
    assert_eq!(body["position_lineage"], "rs0");
    assert_eq!(body["source_epoch"], 1);
    assert_eq!(body["txn_groups"], 1);
    let row = body["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|item| item["key"] == "a")
        .expect("a");
    assert_eq!(row["doc"]["memo"], "newer");
    store.replay(&[newer, older]).expect("replay");
    assert_eq!(store.applied_json("abc")["rows"].as_array().expect("rows").len(), 2);
}

#[test]
fn sealed_wire_stays_c1_s0_and_drops_secret_note() {
    let mut current = BTreeMap::new();
    current.insert(
        "a".to_string(),
        json!({"_id": "a", "memo": "newer", "secretNote": "hide-me"}),
    );
    current.insert("b".to_string(), json!({"_id": "b", "code": "C1-MISS"}));
    let events = vec![Event {
        op: "update".to_string(),
        key: Some("a".to_string()),
        doc: Some(json!({"_id": "a", "memo": "newer", "secretNote": "hide-me"})),
        pre: Some(json!({"_id": "a", "memo": null, "secretNote": "hide-me"})),
        cluster: 80,
        lsid: None,
        txn: None,
        op_index: 0,
    }];
    let initial = snapshot_before(&current, &events).expect("snapshot");
    assert_eq!(initial["a"]["memo"], json!(null));
    assert!(!initial.contains_key("b") || initial["b"]["code"] == "C1-MISS");
    assert!(public_doc(&initial["a"]).get("secretNote").is_none());
    let arrow = seal_initial(&initial).expect("initial");
    assert!(!arrow.bytes.windows(10).any(|window| window == b"secretNote"));
    assert!(!arrow.bytes.windows(7).any(|window| window == b"hide-me"));
    let delta = seal_delta(&events, "rs0", 1).expect("delta");
    assert!(!delta.bytes.windows(10).any(|window| window == b"secretNote"));
    let docs = vec![public_doc(&initial["a"]), public_doc(&current["b"])];
    let catalog = catalog_adapter(&docs, "S0", false).expect("catalog");
    assert_eq!(catalog["collections"], 1);
    assert_eq!(catalog["customer_writes"], false);
    assert_eq!(catalog["txn_grouping"], "S0");
    assert_eq!(catalog["grade_t_claimed"], false);
    assert!(catalog["ontology_columns"]
        .as_array()
        .expect("columns")
        .iter()
        .all(|column| column["name"] != "secretNote"));
    assert_eq!(catalog["variants"]["memo"].as_array().expect("memo").len(), 2);
    assert!(catalog_adapter(&docs, "T", false).is_err());
    let catalog_sha = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let resume = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let initial_ref = BatchRef {
        arrow_sha256: &arrow.file_sha256,
        schema_sha256: &arrow.schema_sha256,
        rows: 2,
        arrow_path: "/tmp/oo-mongodb-connection-whole/c1-prepare/initial.arrow",
    };
    let delta_ref = BatchRef {
        arrow_sha256: &delta.file_sha256,
        schema_sha256: &delta.schema_sha256,
        rows: 1,
        arrow_path: "/tmp/oo-mongodb-connection-whole/c1-prepare/delta.arrow",
    };
    let wire = producer_wire(
        "rs0",
        1,
        70,
        80,
        "S0",
        false,
        resume,
        &initial_ref,
        &delta_ref,
        &["gone".to_string()],
        catalog_sha,
    )
    .expect("wire");
    assert_eq!(wire["submitted"], false);
    assert_eq!(wire["ack"], false);
    assert_eq!(wire["import_invoked"], false);
    assert_eq!(wire["full_snapshot"], false);
    assert_eq!(wire["grade_t_claimed"], false);
    assert_eq!(wire["initial"]["capture_method"], "c1_cdc");
    assert_eq!(wire["initial"]["delete_detection"], "native");
    assert_eq!(wire["initial"]["consistency_grade"], "S0");
    assert_eq!(wire["initial"]["position_codec"], CLUSTER_CODEC);
    assert_eq!(wire["initial"]["full_snapshot"], false);
    assert_eq!(wire["delta"]["position_codec"], CLUSTER_CODEC);
    assert_ne!(wire["initial"]["position_codec"], "snapshot-zero/v1");
    assert_eq!(wire["on_position_expired"]["ack"], false);
    assert_eq!(wire["recovery"]["recovery"], "rebaseline_cdc_first");
    assert!(producer_wire("rs0", 1, 70, 80, "T", false, resume, &initial_ref, &delta_ref, &[], catalog_sha).is_err());
    assert!(producer_wire("rs0", 1, 0, 80, "S0", false, resume, &initial_ref, &delta_ref, &[], catalog_sha).is_err());
    let gold = source_gold(&current, "S0", false);
    let bank = question_bank(&gold).expect("bank");
    assert_eq!(bank.len(), 6);
    assert_eq!(gold["secret_note_public"], json!(false));
}