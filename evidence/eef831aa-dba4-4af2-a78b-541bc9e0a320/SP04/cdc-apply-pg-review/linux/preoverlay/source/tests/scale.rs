mod common;

use std::fs::File;
use std::io::Write;

use cdc_apply_pg::{apply, Endpoint, Request, APPLY_LIMIT, CONTRACT};

#[tokio::test]
async fn over_limit_returns_needs_segments_before_connect() {
    let dir = common::work_dir("scale");
    let feed_path = dir.join("feed.jsonl");
    let spool = dir.join("spool");
    let count = APPLY_LIMIT + 1;
    {
        let mut feed = File::create(&feed_path).expect("feed");
        let bind = format!(
            r#"{{"kind":"bind","source":"billing","lineage":"{lineage}","epoch":1,"selected_tables":["public.orders"],"schema_hash":"{lineage}","config_hash":"{lineage}","type_hash":"{lineage}","hook_hashes":[],"commit_position_provenance":"caller_declared"}}"#,
            lineage = common::LINEAGE
        );
        feed.write_all(bind.as_bytes()).unwrap();
        feed.write_all(b"\n").unwrap();
        feed.write_all(br#"{"kind":"begin","txn_id":"wide1"}"#).unwrap();
        feed.write_all(b"\n").unwrap();
        for seq in 1..=count {
            let line = format!(
                r#"{{"kind":"data","txn_id":"wide1","global_txn_sequence":{seq},"data_collection_sequence":{seq},"table":"public.orders","key":[{{"type":"int","ordinal":0,"value":"{seq}"}}],"op":"upsert","payload":[{{"type":"int","ordinal":0,"value":"1"}}],"unavailable_ordinals":[]}}"#,
            );
            feed.write_all(line.as_bytes()).unwrap();
            feed.write_all(b"\n").unwrap();
        }
        let end = format!(
            r#"{{"kind":"end","txn_id":"wide1","count":{count},"per_collection_counts":{{"public.orders":{count}}},"commit_lsn":"{lsn}","source_commit_timestamp":"{ts}"}}"#,
            lsn = common::LSN,
            ts = common::TS
        );
        feed.write_all(end.as_bytes()).unwrap();
        feed.write_all(b"\n").unwrap();
        feed.sync_all().unwrap();
    }
    common::seal_feed(&spool, &feed_path);
    let request = Request {
        contract: CONTRACT.to_owned(),
        spool_dir: spool,
        endpoint: Endpoint {
            host: "invalid.invalid".to_owned(),
            port: 1,
            database: "oo_a301".to_owned(),
            user: "oo_a301_runtime".to_owned(),
            password_file: dir.join("missing-password"),
            ca_file: dir.join("missing-ca"),
            server_name: "invalid.invalid".to_owned(),
        },
        shard: "billing".to_owned(),
        owner: "11111111-1111-4111-8111-111111111111".to_owned(),
        generation: 1,
        table_generation: "g1".to_owned(),
        run_generation: 1,
        lineage: common::LINEAGE.to_owned(),
        source: "billing".to_owned(),
        epoch: 1,
        control_lease_valid: true,
        segment: None,
        keys: Vec::new(),
        columns: Vec::new(),
    };
    let output = apply(&request).await;
    assert_eq!(output.fault.as_deref(), Some("needs_segments"));
    assert!(!output.target_applied);
    assert!(output.generation.is_none());
}
