//! Event notifications as change hints over real HTTP (10 §4.3.6, B-S11-02): `POST /drives/{id}:events` takes the
//! messages of a bucket's event queue, drops repeats and older events, honours the Drive policy and answers with what
//! a targeted sync found. The local event source is the request body itself (the neutral format and the S3, R2 and OSS
//! shapes are in `oo_connectors::objectstore_events::parse`).
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test events_live -- --ignored --test-threads=1`

mod common;

use common::engine::{Opts, TestEngine};
use serde_json::{Value, json};

fn put(key: &str, seq: &str, at: i64) -> Value {
    json!({"hints": [{"key": key, "op": "put", "sequencer": seq, "at_ms": at}]})
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn events_find_a_new_file_once_and_never_decide_deletions() {
    let e = TestEngine::start(
        "oo_docs_events_live",
        Opts {
            workers: true,
            ..Opts::default()
        },
    )
    .await;
    let created = e
        .post(
            "/v1/docs/drives",
            &json!({"name": "ev", "backend": "fs", "config": {"root": e.root.to_str().unwrap(),
                    "policy": {"deny": ["Secret"]}}}),
        )
        .await;
    assert_eq!(
        created.status,
        201,
        "{}",
        String::from_utf8_lossy(&created.body)
    );
    let id = created.json()["id"].as_str().unwrap().to_owned();
    e.write("Notes/new.md", "# new\n");
    e.write("Secret/s.md", "# s\n");
    let path = format!("/v1/docs/drives/{id}:events");

    // S3 shaped message, a repeat inside one batch, and a path the policy denies.
    let s3 = json!({"Records": [{"eventName": "ObjectCreated:Put", "eventTime": "2026-11-17T08:00:00Z",
                                 "s3": {"object": {"key": "Notes/new.md", "sequencer": "0A"}}}]});
    let secret = put("Secret/s.md", "01", 1);
    let r = e
        .post(
            &path,
            &json!({"messages": [s3.clone(), s3.clone(), secret]}),
        )
        .await;
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    let b = r.json();
    assert_eq!(
        (
            b["fresh"].as_i64(),
            b["duplicates"].as_i64(),
            b["added"].as_i64(),
            b["denied_by_policy"].as_i64()
        ),
        (Some(2), Some(1), Some(1), Some(1)),
        "{b}"
    );
    assert_eq!(b["looked_at"], 1, "the denied path is never looked at");
    let files = e
        .get(&format!("/v1/docs/drives/{id}/files?limit=50"))
        .await
        .json();
    let paths: Vec<&str> = files["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        ["Notes/new.md"],
        "the file was found by the event, without a poll"
    );

    // An older event of the same key, delivered late, is dropped; a repeat of a batch is all duplicates.
    let r = e
        .post(
            &path,
            &json!({"messages": [put("Notes/new.md", "0C", 9), put("Notes/new.md", "0B", 8)]}),
        )
        .await
        .json();
    assert_eq!(
        (r["fresh"].as_i64(), r["stale"].as_i64()),
        (Some(1), Some(1)),
        "{r}"
    );
    let r = e.post(&path, &json!({"messages": [s3]})).await.json();
    assert_eq!(
        (r["fresh"].as_i64(), r["duplicates"].as_i64()),
        (Some(0), Some(1)),
        "{r}"
    );

    // A delete event is only reported: the file stays until a full reconcile confirms it (D10).
    e.rm("Notes/new.md");
    let del =
        json!({"hints": [{"key": "Notes/new.md", "op": "delete", "sequencer": "0F", "at_ms": 20}]});
    let r = e.post(&path, &json!({"messages": [del]})).await.json();
    assert_eq!(r["maybe_deleted"], json!(["Notes/new.md"]), "{r}");
    let files = e
        .get(&format!("/v1/docs/drives/{id}/files?limit=50"))
        .await
        .json();
    assert_eq!(
        files["items"].as_array().unwrap().len(),
        1,
        "an event does not delete"
    );

    // Garbage and an empty batch are refused or counted, never applied.
    let r = e
        .post(&path, &json!({"messages": ["not json", {"unknown": 1}]}))
        .await
        .json();
    assert_eq!(r["rejected"], 2, "{r}");
    assert_eq!(e.post(&path, &json!({"messages": []})).await.status, 400);
}
