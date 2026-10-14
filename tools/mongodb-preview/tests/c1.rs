use std::collections::BTreeMap;

use mongodb_preview::c1::{
    Event, Store, apply_interface_gap, cluster_decimal, cmp_cluster, may_pull, position_expired,
    token_cluster, CLUSTER_CODEC,
};
use serde_json::json;

fn event(op: &str, key: Option<&str>, doc: Option<serde_json::Value>, ordinal: u32) -> Event {
    Event {
        op: op.to_string(),
        key: key.map(str::to_string),
        doc,
        pre: None,
        cluster: 50,
        lsid: Some("sid".to_string()),
        txn: Some(7),
        ordinal,
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
            ordinal: 1,
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
            ordinal: 3,
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
            ordinal: 3,
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
            ordinal: 2,
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