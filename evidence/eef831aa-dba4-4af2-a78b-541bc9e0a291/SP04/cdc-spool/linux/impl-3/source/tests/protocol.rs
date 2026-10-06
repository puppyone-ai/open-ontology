#![forbid(unsafe_code)]

mod common;

use std::fs;
use std::io::Cursor;

use cdc_spool::{
    canonical_preimage, recover, verify_replay, CanonicalData, Cell, FaultCode, Limits, Op,
    OutcomeKind, Recovery, Spool,
};
use common::{
    begin, bind, data, end, feed, file_sha, first_event_disk, ingest, oracle, rollback, tempdir,
    Row, HASH, POSITION, TS, TS_Z,
};
use serde_json::{json, Value};

const EVENT1: &str = "{\"data_collection_sequence\":1,\"global_txn_sequence\":1,\"key\":[{\"ordinal\":0,\"type\":\"int\",\"value\":\"17\"}],\"op\":\"upsert\",\"payload\":[{\"ordinal\":0,\"type\":\"text\",\"value\":\"open\"}],\"table\":\"public.orders\",\"txn_id\":\"inv17\",\"unavailable_ordinals\":[]}";
const EVENT2: &str = "{\"data_collection_sequence\":1,\"global_txn_sequence\":2,\"key\":[{\"ordinal\":0,\"type\":\"int\",\"value\":\"1\"}],\"op\":\"delete\",\"payload\":[],\"table\":\"public.lines\",\"txn_id\":\"inv17\",\"unavailable_ordinals\":[]}";

fn sample_feed() -> Vec<u8> {
    feed(&[
        bind(&["public.lines", "public.orders"]),
        begin("inv17"),
        data(&Row {
            txn: "inv17",
            global: 1,
            collection: 1,
            table: "public.orders",
            key: "17",
            op: "upsert",
            payload: json!([{"type": "text", "ordinal": 0, "value": "open"}]),
            unavailable: &[],
        }),
        data(&Row {
            txn: "inv17",
            global: 2,
            collection: 1,
            table: "public.lines",
            key: "1",
            op: "delete",
            payload: json!([]),
            unavailable: &[],
        }),
        data(&Row {
            txn: "inv17",
            global: 3,
            collection: 2,
            table: "public.lines",
            key: "1",
            op: "upsert",
            payload: json!([
                {"type": "text", "ordinal": 0, "value": "a"},
                {"type": "null", "ordinal": 1},
                {"type": "unavailable", "ordinal": 2}
            ]),
            unavailable: &[2],
        }),
        end(
            "inv17",
            3,
            &[("public.lines", 2), ("public.orders", 1)],
            "16/B6E40",
            TS,
        ),
    ])
}

fn refused(result: Result<Spool, cdc_spool::Fault>) -> cdc_spool::Fault {
    match result {
        Err(err) => err,
        Ok(_) => panic!("expected refusal"),
    }
}

fn manifest(dir: &std::path::Path) -> Value {
    serde_json::from_slice(&fs::read(dir.join("manifest.json")).expect("manifest")).expect("json")
}

#[test]
fn oracle_literals_are_independent() {
    oracle::check();
}

#[test]
fn sample_seals_null_unavailable_and_distinct_cells() {
    oracle::check();
    let root = tempdir();
    let dir = root.join("txn");
    let outcome = ingest(&dir, Limits::product(), &sample_feed()).expect("seal");
    let OutcomeKind::Sealed(summary) = outcome.kind else {
        panic!("not sealed");
    };
    assert_eq!(summary.count, 3);
    assert_eq!(summary.commit_position, POSITION);
    assert_eq!(summary.commit_lsn, "16/B6E40");
    assert!(!summary.ack_allowed);
    assert!(!summary.target_applied);
    assert!(!summary.cross_table_consistent);
    assert_eq!(summary.durable_events, 3);
    assert_eq!(summary.decoded_events, 3);
    assert_eq!(outcome.progress.seen_data, 3);
    assert!(outcome.progress.applied.is_none());

    let body = fs::read(dir.join("manifest.json")).expect("manifest bytes");
    let seal = fs::read(dir.join("SEAL")).expect("seal");
    assert_eq!(
        seal,
        format!("{}\n", oracle::hex(&oracle::sha256(&body))).as_bytes()
    );
    let doc = manifest(&dir);
    assert_eq!(doc["contract"], "cdc-spool-v1");
    assert_eq!(doc["kind"], "committed_manifest");
    assert_eq!(doc["source"], "billing");
    assert_eq!(doc["lineage"], HASH);
    assert_eq!(doc["epoch"], json!(1));
    assert_eq!(doc["schema_hash"], HASH);
    assert_eq!(doc["config_hash"], HASH);
    assert_eq!(doc["type_hash"], HASH);
    assert_eq!(doc["hook_hashes"], json!([]));
    assert_eq!(doc["selected_tables"], json!(["public.lines", "public.orders"]));
    assert_eq!(doc["commit_position_provenance"], "caller_declared");
    assert_eq!(doc["txn_id"], "inv17");
    assert_eq!(doc["count"], json!(3));
    assert_eq!(doc["per_collection_counts"]["public.lines"], json!(2));
    assert_eq!(doc["per_collection_counts"]["public.orders"], json!(1));
    assert_eq!(doc["first_sequence"], json!(1));
    assert_eq!(doc["last_sequence"], json!(3));
    assert_eq!(doc["commit_lsn"], "16/B6E40");
    assert_eq!(doc["commit_position"], POSITION);
    assert_eq!(doc["source_commit_timestamp"], TS);
    assert_eq!(doc["selected_row_count"], json!(3));
    assert_eq!(doc["chunks"].as_array().expect("chunks").len(), 1);
    assert_eq!(doc["ack_allowed"], false);
    assert_eq!(doc["target_applied"], false);
    assert_eq!(doc["cross_table_consistent"], false);
    assert!(doc["applied"].is_null());
    assert_eq!(doc["upstream_commit_lsn_verified"], false);
    assert_eq!(doc["debezium_normalizer"], false);
    assert_eq!(doc["p2_verified"], false);
    assert_eq!(doc["p11_verified"], false);

    let chunk = fs::read(dir.join("chunk-000001")).expect("chunk");
    let chunk_hash = oracle::hex(&oracle::sha256(&chunk));
    assert_eq!(doc["chunks"][0]["sha256"], chunk_hash);
    assert_eq!(doc["chunks"][0]["events"], json!(3));
    assert_eq!(doc["chunks"][0]["bytes"], json!(chunk.len()));
    assert_eq!(doc["chunks"][0]["name"], "chunk-000001");
    let set = format!("{chunk_hash}\n");
    assert_eq!(doc["chunk_set_sha256"], oracle::hex(&oracle::sha256(set.as_bytes())));
    let lines: Vec<&str> = std::str::from_utf8(&chunk).expect("utf8").lines().collect();
    assert_eq!(lines, vec![EVENT1, EVENT2, oracle::SAMPLE_PREIMAGE]);
    assert_eq!(
        oracle::hex(&oracle::sha256(lines[2].as_bytes())),
        oracle::SAMPLE_HASH
    );
    assert!(!lines[2].contains("\"value\":null"));
    assert!(lines[1].contains("\"op\":\"delete\""));
    assert!(lines[0].contains("\"value\":\"open\""));
    assert!(lines[2].contains("\"value\":\"a\""));

    let progress = fs::read_to_string(dir.join("progress.json")).expect("progress");
    assert!(progress.contains("\"ack_allowed\":false"));
    assert!(progress.contains("\"applied\":null"));
    assert!(progress.contains("\"kind\":\"durable_progress\""));

    let Recovery::Committed(committed) = recover(&dir).expect("recover") else {
        panic!("not committed");
    };
    assert!(!committed.ack_allowed());
    let mut iter = committed.iter().expect("iter");
    let mut seen = Vec::new();
    while let Some(item) = iter.next() {
        seen.push(item.expect("event"));
    }
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[0].preimage, EVENT1);
    assert_eq!(seen[1].preimage, EVENT2);
    assert_eq!(seen[2].preimage, oracle::SAMPLE_PREIMAGE);
    assert_eq!(seen[2].event_hash, oracle::SAMPLE_HASH);
    assert_eq!(seen[2].table, "public.lines");
    assert_eq!(seen[2].global_txn_sequence, 3);
    assert_eq!(seen[2].data_collection_sequence, 2);
    assert_eq!(seen[2].op, Op::Upsert);
    assert_eq!(seen[1].op, Op::Delete);
    assert_eq!(
        seen[2].payload,
        vec![
            Cell::Text {
                ordinal: 0,
                value: "a".to_owned(),
            },
            Cell::Null { ordinal: 1 },
            Cell::Unavailable { ordinal: 2 },
        ]
    );
    assert_eq!(seen[2].unavailable_ordinals, vec![2]);
    assert_eq!(
        seen[0].key,
        vec![Cell::Int {
            ordinal: 0,
            value: "17".to_owned(),
        }]
    );
    assert!(!dir.join("ROLLED_BACK").exists());
}

#[test]
fn ordinal_32767_seals_and_32768_and_65535_are_refused() {
    let root = tempdir();
    let dir = root.join("ok");
    let line = data_with_ordinal("ord1", "32767");
    let bytes = feed(&[
        bind(&["public.orders"]),
        begin("ord1"),
        line,
        end("ord1", 1, &[("public.orders", 1)], "0/1", TS_Z),
    ]);
    ingest(&dir, Limits::product(), &bytes).expect("32767");
    let Recovery::Committed(committed) = recover(&dir).expect("recover") else {
        panic!("not committed");
    };
    let event = committed.iter().expect("iter").next().expect("row").expect("event");
    assert_eq!(event.payload[0].ordinal(), 32_767);

    for bad in ["32768", "65535", "-1"] {
        let root = tempdir();
        let dir = root.join("bad");
        let bytes = feed(&[
            bind(&["public.orders"]),
            begin("ord1"),
            data_with_ordinal("ord1", bad),
            end("ord1", 1, &[("public.orders", 1)], "0/1", TS_Z),
        ]);
        let err = ingest(&dir, Limits::product(), &bytes).expect_err(bad);
        assert_eq!(err.code, FaultCode::OrdinalRange, "{bad}");
        assert!(!dir.join("chunk-000001").exists(), "{bad}");
        assert!(!dir.join("SEAL").exists(), "{bad}");
        assert!(!dir.join("manifest.json").exists(), "{bad}");
    }
}

fn data_with_ordinal(txn: &str, ordinal: &str) -> String {
    format!(
        "{{\"kind\":\"data\",\"txn_id\":\"{txn}\",\"global_txn_sequence\":1,\"data_collection_sequence\":1,\"table\":\"public.orders\",\"key\":[{{\"type\":\"int\",\"ordinal\":0,\"value\":\"1\"}}],\"op\":\"upsert\",\"payload\":[{{\"type\":\"int\",\"ordinal\":{ordinal},\"value\":\"9\"}}],\"unavailable_ordinals\":[]}}"
    )
}

#[test]
fn rollback_has_no_sealed_manifest() {
    let root = tempdir();
    let dir = root.join("txn");
    let bytes = feed(&[
        bind(&["public.orders"]),
        begin("rb1"),
        data(&Row {
            txn: "rb1",
            global: 1,
            collection: 1,
            table: "public.orders",
            key: "1",
            op: "upsert",
            payload: json!([]),
            unavailable: &[],
        }),
        rollback("rb1"),
        rollback("rb1"),
    ]);
    let outcome = ingest(&dir, Limits::product(), &bytes).expect("rollback");
    let OutcomeKind::RolledBack { txn_id } = outcome.kind else {
        panic!("not rolled back");
    };
    assert_eq!(txn_id, "rb1");
    assert!(!outcome.progress.ack_allowed);
    assert!(outcome.progress.duplicate_rollback >= 1);
    assert_eq!(fs::read(dir.join("ROLLED_BACK")).expect("marker"), b"rolled_back\n");
    assert!(!dir.join("SEAL").exists());
    assert!(!dir.join("manifest.json").exists());
    let Recovery::RolledBack(rolled) = recover(&dir).expect("recover") else {
        panic!("not rolled back");
    };
    assert_eq!(rolled.txn_id.as_deref(), Some("rb1"));
    assert!(!rolled.ack_allowed);
    assert!(!rolled.target_applied);
    assert!(!rolled.cross_table_consistent);
}

#[test]
fn existing_path_and_foreign_pending_are_not_overwritten() {
    let root = tempdir();
    let file = root.join("file");
    fs::write(&file, b"keep-file").unwrap();
    let err = refused(Spool::create_new(&file, Limits::product()));
    assert_eq!(err.code, FaultCode::PathExists);
    assert_eq!(fs::read(&file).unwrap(), b"keep-file");

    let pending = root.join("pending");
    fs::create_dir(&pending).unwrap();
    fs::write(pending.join("note"), b"foreign").unwrap();
    let err = refused(Spool::create_new(&pending, Limits::product()));
    assert_eq!(err.code, FaultCode::PathExists);
    assert_eq!(fs::read(pending.join("note")).unwrap(), b"foreign");

    let sealed = root.join("sealed");
    ingest(&sealed, Limits::product(), &sample_feed()).unwrap();
    let before = fs::read(sealed.join("SEAL")).unwrap();
    let err = refused(Spool::create_new(&sealed, Limits::product()));
    assert_eq!(err.code, FaultCode::PathExists);
    assert_eq!(fs::read(sealed.join("SEAL")).unwrap(), before);

    let real = root.join("real");
    fs::create_dir(&real).unwrap();
    let link = root.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let err = refused(Spool::create_new(&link.join("txn"), Limits::product()));
    assert_eq!(err.code, FaultCode::SymlinkRefused);
    assert!(!real.join("txn").exists());
}

#[test]
fn disk_quota_is_a_byte_counter_and_keeps_written_files() {
    let root = tempdir();
    let dir = root.join("txn");
    let preimage = canonical_preimage(&CanonicalData {
        txn_id: "q1",
        global_txn_sequence: 1,
        data_collection_sequence: 1,
        table: "public.orders",
        key: &[Cell::Int {
            ordinal: 0,
            value: "1".to_owned(),
        }],
        op: Op::Upsert,
        payload: &[],
        unavailable_ordinals: &[],
    })
    .unwrap();
    let mut limits = Limits::product();
    limits.max_chunk_events = 1;
    limits.max_disk_bytes = first_event_disk(u64::try_from(preimage.len()).unwrap() + 1);
    assert!(limits.max_disk_bytes < 256 * 1024 * 1024);
    let bytes = feed(&[
        bind(&["public.orders"]),
        begin("q1"),
        data(&Row {
            txn: "q1",
            global: 1,
            collection: 1,
            table: "public.orders",
            key: "1",
            op: "upsert",
            payload: json!([]),
            unavailable: &[],
        }),
        data(&Row {
            txn: "q1",
            global: 2,
            collection: 2,
            table: "public.orders",
            key: "2",
            op: "upsert",
            payload: json!([]),
            unavailable: &[],
        }),
        end("q1", 2, &[("public.orders", 2)], "0/2", TS_Z),
    ]);
    let err = ingest(&dir, limits, &bytes).expect_err("quota");
    assert_eq!(err.code, FaultCode::DiskQuota);
    assert!(dir.join("chunk-000001").is_file());
    assert!(!dir.join("chunk-000002").exists());
    assert!(!dir.join("SEAL").exists());
    assert!(!dir.join("ROLLED_BACK").exists());
    fs::write(dir.join("still-writable"), b"ok").unwrap();
    assert_eq!(fs::read(dir.join("still-writable")).unwrap(), b"ok");
}

#[test]
fn file_cap_stops_before_the_manifest() {
    let root = tempdir();
    let dir = root.join("txn");
    let mut limits = Limits::product();
    limits.max_chunk_events = 1;
    limits.max_files = 3;
    let bytes = feed(&[
        bind(&["public.orders"]),
        begin("f1"),
        data(&Row {
            txn: "f1",
            global: 1,
            collection: 1,
            table: "public.orders",
            key: "1",
            op: "upsert",
            payload: json!([]),
            unavailable: &[],
        }),
        end("f1", 1, &[("public.orders", 1)], "0/1", TS_Z),
    ]);
    let err = ingest(&dir, limits, &bytes).expect_err("files");
    assert_eq!(err.code, FaultCode::FileCap);
    assert!(dir.join("chunk-000001").is_file());
    assert!(!dir.join("SEAL").exists());
    assert!(!dir.join("manifest.json").exists());
}

#[test]
fn damaged_chunks_are_never_committed_and_files_stay() {
    let root = tempdir();
    let dir = root.join("txn");
    ingest(&dir, Limits::product(), &sample_feed()).unwrap();
    let seal = fs::read(dir.join("SEAL")).unwrap();
    fs::remove_file(dir.join("chunk-000001")).unwrap();
    let err = recover(&dir).expect_err("missing");
    assert_eq!(err.code, FaultCode::MissingChunk);
    assert_eq!(fs::read(dir.join("SEAL")).unwrap(), seal);

    let dir = root.join("trunc");
    ingest(&dir, Limits::product(), &sample_feed()).unwrap();
    let mut chunk = fs::read(dir.join("chunk-000001")).unwrap();
    chunk.pop();
    fs::write(dir.join("chunk-000001"), &chunk).unwrap();
    let err = recover(&dir).expect_err("trunc");
    assert_eq!(err.code, FaultCode::Truncated);
    assert!(dir.join("manifest.json").is_file());

    let dir = root.join("hash");
    ingest(&dir, Limits::product(), &sample_feed()).unwrap();
    let mut chunk = fs::read(dir.join("chunk-000001")).unwrap();
    chunk[0] = b'}';
    fs::write(dir.join("chunk-000001"), &chunk).unwrap();
    let kept = fs::read(dir.join("SEAL")).unwrap();
    let err = recover(&dir).expect_err("hash");
    assert_eq!(err.code, FaultCode::ChecksumFault);
    assert_eq!(fs::read(dir.join("SEAL")).unwrap(), kept);

    let dir = root.join("link");
    ingest(&dir, Limits::product(), &sample_feed()).unwrap();
    let chunk = dir.join("chunk-000001");
    let kept = dir.join("chunk-000001.kept");
    fs::rename(&chunk, &kept).unwrap();
    std::os::unix::fs::symlink(&kept, &chunk).unwrap();
    let err = recover(&dir).expect_err("symlink");
    assert_eq!(err.code, FaultCode::SymlinkRefused);
    assert!(kept.is_file());
    assert!(dir.join("SEAL").is_file());
}

#[test]
fn torn_seal_is_unknown_and_not_rolled_back() {
    let root = tempdir();
    let dir = root.join("txn");
    ingest(&dir, Limits::product(), &sample_feed()).unwrap();
    fs::write(dir.join("SEAL"), b"torn\n").unwrap();
    let Recovery::Unknown { reason } = recover(&dir).expect("unknown") else {
        panic!("not unknown");
    };
    assert_eq!(reason, "seal_torn");
    assert!(dir.join("manifest.json").is_file());
    assert!(!dir.join("ROLLED_BACK").exists());

    fs::write(dir.join("SEAL"), format!("{}\n", "ab".repeat(32))).unwrap();
    fs::write(dir.join("ROLLED_BACK"), b"rolled_back\n").unwrap();
    let Recovery::Unknown { reason } = recover(&dir).expect("both") else {
        panic!("not unknown");
    };
    assert_eq!(reason, "seal_and_rollback");
    assert!(dir.join("manifest.json").is_file());
    assert!(dir.join("ROLLED_BACK").is_file());
}

#[test]
fn attestation_and_raised_limit_are_refused() {
    let root = tempdir();
    let dir = root.join("ack");
    ingest(&dir, Limits::product(), &sample_feed()).unwrap();
    rewrite_manifest(&dir, |doc| {
        doc["ack_allowed"] = json!(true);
    });
    let err = recover(&dir).expect_err("ack");
    assert_eq!(err.code, FaultCode::AttestationRefused);

    let dir = root.join("limit");
    ingest(&dir, Limits::product(), &sample_feed()).unwrap();
    rewrite_manifest(&dir, |doc| {
        doc["limits"]["max_events"] = json!(500_001);
    });
    let err = recover(&dir).expect_err("limit");
    assert_eq!(err.code, FaultCode::LimitRaised);
}

fn rewrite_manifest(dir: &std::path::Path, edit: impl FnOnce(&mut Value)) {
    let mut doc = manifest(dir);
    edit(&mut doc);
    let body = serde_json::to_vec(&doc).unwrap();
    fs::write(dir.join("manifest.json"), &body).unwrap();
    fs::write(
        dir.join("SEAL"),
        format!("{}\n", oracle::hex(&oracle::sha256(&body))),
    )
    .unwrap();
}

#[test]
fn replay_matches_or_refuses_without_writing() {
    let root = tempdir();
    let dir = root.join("txn");
    ingest(&dir, Limits::product(), &sample_feed()).unwrap();
    let before = file_sha(&dir.join("SEAL"));
    verify_replay(&dir, Cursor::new(sample_feed())).expect("replay");
    assert_eq!(file_sha(&dir.join("SEAL")), before);
    let mut bad = sample_feed();
    let text = String::from_utf8(bad.clone()).unwrap().replace("\"open\"", "\"shut\"");
    bad = text.into_bytes();
    let err = verify_replay(&dir, Cursor::new(bad)).expect_err("conflict");
    assert_eq!(err.code, FaultCode::PayloadConflict);
    assert_eq!(file_sha(&dir.join("SEAL")), before);

    let open = root.join("open");
    let bytes = feed(&[bind(&["public.orders"]), begin("inv17")]);
    let outcome = ingest(&open, Limits::product(), &bytes).unwrap();
    assert!(matches!(outcome.kind, OutcomeKind::Incomplete { .. }));
    let err = verify_replay(&open, Cursor::new(bytes)).expect_err("open");
    assert_eq!(err.code, FaultCode::NotCommitted);
}

#[test]
fn iterator_refuses_a_changed_chunk_and_does_not_write() {
    let root = tempdir();
    let dir = root.join("txn");
    ingest(&dir, Limits::product(), &sample_feed()).unwrap();
    let Recovery::Committed(committed) = recover(&dir).unwrap() else {
        panic!("committed");
    };
    let mut names: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|item| item.unwrap().file_name())
        .collect();
    names.sort();
    let mut chunk = fs::read(dir.join("chunk-000001")).unwrap();
    chunk[10] ^= 0x01;
    fs::write(dir.join("chunk-000001"), &chunk).unwrap();
    let err = committed.iter().unwrap().next().unwrap().expect_err("mismatch");
    assert_eq!(err.code, FaultCode::OutputMismatch);
    let mut after: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|item| item.unwrap().file_name())
        .collect();
    after.sort();
    assert_eq!(names, after);
}

#[test]
fn end_too_early_stays_open_and_a_later_end_can_seal() {
    let root = tempdir();
    let dir = root.join("txn");
    let mut spool = Spool::create_new(&dir, Limits::product()).unwrap();
    let early = feed(&[
        bind(&["public.orders"]),
        begin("e1"),
        end("e1", 1, &[("public.orders", 1)], "0/1", TS_Z),
    ]);
    let err = spool.ingest_reader(Cursor::new(early)).expect_err("early");
    assert_eq!(err.code, FaultCode::EndTooEarly);
    assert!(!dir.join("SEAL").exists());
    let rest = feed(&[
        data(&Row {
            txn: "e1",
            global: 1,
            collection: 1,
            table: "public.orders",
            key: "4",
            op: "upsert",
            payload: json!([]),
            unavailable: &[],
        }),
        end("e1", 1, &[("public.orders", 1)], "0/1", TS_Z),
    ]);
    let outcome = spool.ingest_reader(Cursor::new(rest)).expect("later");
    assert!(matches!(outcome.kind, OutcomeKind::Sealed(_)));
    assert!(dir.join("SEAL").is_file());
}

#[test]
fn invalid_commit_lsn_does_not_seal() {
    let root = tempdir();
    let dir = root.join("txn");
    let bytes = feed(&[
        bind(&["public.orders"]),
        begin("lsn"),
        data(&Row {
            txn: "lsn",
            global: 1,
            collection: 1,
            table: "public.orders",
            key: "1",
            op: "upsert",
            payload: json!([]),
            unavailable: &[],
        }),
        end("lsn", 1, &[("public.orders", 1)], "00/1", TS_Z),
    ]);
    let err = ingest(&dir, Limits::product(), &bytes).expect_err("lsn");
    assert_eq!(err.code, FaultCode::CommitLsnInvalid);
    assert!(!dir.join("SEAL").exists());
}

#[test]
fn lowered_event_cap_and_active_txn_and_barrier() {
    let root = tempdir();
    let dir = root.join("cap");
    let mut limits = Limits::product();
    limits.max_events = 1;
    let bytes = feed(&[
        bind(&["public.orders"]),
        begin("cap"),
        data(&Row {
            txn: "cap",
            global: 1,
            collection: 1,
            table: "public.orders",
            key: "1",
            op: "upsert",
            payload: json!([]),
            unavailable: &[],
        }),
        data(&Row {
            txn: "cap",
            global: 2,
            collection: 2,
            table: "public.orders",
            key: "2",
            op: "upsert",
            payload: json!([]),
            unavailable: &[],
        }),
    ]);
    let err = ingest(&dir, limits, &bytes).expect_err("cap");
    assert_eq!(err.code, FaultCode::EventCap);
    assert!(!dir.join("SEAL").exists());

    let dir = root.join("active");
    let bytes = feed(&[
        bind(&["public.orders"]),
        begin("one"),
        begin("two"),
    ]);
    let err = ingest(&dir, Limits::product(), &bytes).expect_err("active");
    assert_eq!(err.code, FaultCode::ActiveTxnExceeded);

    let dir = root.join("barrier");
    let bytes = feed(&[
        bind(&["public.orders"]),
        json!({"kind": "barrier", "reason": "stop"}).to_string(),
    ]);
    let err = ingest(&dir, Limits::product(), &bytes).expect_err("barrier");
    assert_eq!(err.code, FaultCode::SingleStream);

    let mut raised = Limits::product();
    raised.max_events = 500_001;
    let err = refused(Spool::create_new(&root.join("raised"), raised));
    assert_eq!(err.code, FaultCode::LimitRaised);
    assert!(!root.join("raised").exists());
}

#[test]
fn empty_txn_seals_and_slash_txn_id_is_not_a_path() {
    let root = tempdir();
    let dir = root.join("empty");
    let bytes = feed(&[
        bind(&["public.orders"]),
        begin("empty1"),
        end("empty1", 0, &[], "0/1", TS_Z),
    ]);
    ingest(&dir, Limits::product(), &bytes).unwrap();
    let doc = manifest(&dir);
    assert_eq!(doc["count"], json!(0));
    assert!(doc["first_sequence"].is_null());
    assert!(doc["last_sequence"].is_null());
    assert_eq!(doc["chunks"], json!([]));
    assert_eq!(doc["chunk_set_sha256"], oracle::EMPTY_SHA256);
    assert!(dir.join("chunks.idx").is_file());
    assert_eq!(fs::read(dir.join("chunks.idx")).unwrap(), b"");
    let Recovery::Committed(committed) = recover(&dir).unwrap() else {
        panic!("committed");
    };
    assert!(committed.iter().unwrap().next().is_none());
    assert_eq!(doc["commit_position"], "18446744073709551617");

    let dir = root.join("slash");
    let bytes = feed(&[
        bind(&["public.orders"]),
        begin("a/b"),
        data(&Row {
            txn: "a/b",
            global: 1,
            collection: 1,
            table: "public.orders",
            key: "1",
            op: "upsert",
            payload: json!([]),
            unavailable: &[],
        }),
        end("a/b", 1, &[("public.orders", 1)], "0/1", TS_Z),
    ]);
    ingest(&dir, Limits::product(), &bytes).unwrap();
    assert!(!dir.join("a").exists());
    assert_eq!(manifest(&dir)["txn_id"], "a/b");
}

#[test]
fn duplicate_data_idempotent_and_conflict_and_count_faults() {
    let root = tempdir();
    let row = data(&Row {
        txn: "d1",
        global: 1,
        collection: 1,
        table: "public.orders",
        key: "1",
        op: "upsert",
        payload: json!([]),
        unavailable: &[],
    });
    let bytes = feed(&[
        bind(&["public.orders"]),
        begin("d1"),
        begin("d1"),
        row.clone(),
        row,
        end("d1", 1, &[("public.orders", 1)], "0/1", TS_Z),
        end("d1", 1, &[("public.orders", 1)], "0/1", TS_Z),
    ]);
    let outcome = ingest(&root.join("dup"), Limits::product(), &bytes).unwrap();
    let OutcomeKind::Sealed(summary) = outcome.kind else {
        panic!("sealed");
    };
    assert_eq!(summary.count, 1);
    assert_eq!(outcome.progress.duplicate_data, 1);
    assert_eq!(outcome.progress.duplicate_begin, 1);
    assert_eq!(outcome.progress.duplicate_end, 1);

    let bad = data(&Row {
        txn: "d1",
        global: 1,
        collection: 1,
        table: "public.orders",
        key: "9",
        op: "upsert",
        payload: json!([]),
        unavailable: &[],
    });
    let bytes = feed(&[
        bind(&["public.orders"]),
        begin("d1"),
        data(&Row {
            txn: "d1",
            global: 1,
            collection: 1,
            table: "public.orders",
            key: "1",
            op: "upsert",
            payload: json!([]),
            unavailable: &[],
        }),
        bad,
    ]);
    let err = ingest(&root.join("conflict"), Limits::product(), &bytes).expect_err("conflict");
    assert_eq!(err.code, FaultCode::DuplicateConflict);
    assert!(!root.join("conflict").join("SEAL").exists());

    let bytes = feed(&[
        bind(&["public.orders"]),
        begin("d1"),
        data(&Row {
            txn: "d1",
            global: 1,
            collection: 1,
            table: "public.orders",
            key: "1",
            op: "upsert",
            payload: json!([]),
            unavailable: &[],
        }),
        data(&Row {
            txn: "d1",
            global: 2,
            collection: 2,
            table: "public.orders",
            key: "2",
            op: "upsert",
            payload: json!([]),
            unavailable: &[],
        }),
        end("d1", 1, &[("public.orders", 1)], "0/1", TS_Z),
    ]);
    let err = ingest(&root.join("count"), Limits::product(), &bytes).expect_err("count");
    assert_eq!(err.code, FaultCode::CountMismatch);

    let bytes = feed(&[
        bind(&["public.orders"]),
        begin("d1"),
        data(&Row {
            txn: "d1",
            global: 1,
            collection: 1,
            table: "public.orders",
            key: "1",
            op: "upsert",
            payload: json!([]),
            unavailable: &[],
        }),
        end("d1", 2, &[("public.orders", 1)], "0/1", TS_Z),
    ]);
    let err = ingest(&root.join("gap"), Limits::product(), &bytes).expect_err("gap");
    assert_eq!(err.code, FaultCode::CollectionCountGap);
}

#[test]
fn drop_does_not_seal_or_delete() {
    let root = tempdir();
    let dir = root.join("txn");
    let mut limits = Limits::product();
    limits.max_chunk_events = 1;
    {
        let mut spool = Spool::create_new(&dir, limits).unwrap();
        let bytes = feed(&[
            bind(&["public.orders"]),
            begin("drop1"),
            data(&Row {
                txn: "drop1",
                global: 1,
                collection: 1,
                table: "public.orders",
                key: "1",
                op: "upsert",
                payload: json!([]),
                unavailable: &[],
            }),
        ]);
        let outcome = spool.ingest_reader(Cursor::new(bytes)).unwrap();
        assert!(matches!(outcome.kind, OutcomeKind::Incomplete { .. }));
        assert_eq!(outcome.progress.decoded_events, 1);
        assert_eq!(outcome.progress.durable_events, 1);
    }
    assert!(dir.join("chunk-000001").is_file());
    assert!(!dir.join("SEAL").exists());
    assert!(!dir.join("ROLLED_BACK").exists());
    let Recovery::Incomplete(pending) = recover(&dir).unwrap() else {
        panic!("incomplete");
    };
    assert!(!pending.ack_allowed);
    assert_eq!(pending.durable_events, 1);
}

#[test]
fn open_chunk_decoded_can_lead_durable_progress() {
    let root = tempdir();
    let dir = root.join("txn");
    let bytes = feed(&[
        bind(&["public.orders"]),
        begin("open1"),
        data(&Row {
            txn: "open1",
            global: 1,
            collection: 1,
            table: "public.orders",
            key: "1",
            op: "upsert",
            payload: json!([]),
            unavailable: &[],
        }),
    ]);
    let outcome = ingest(&dir, Limits::product(), &bytes).unwrap();
    assert!(matches!(outcome.kind, OutcomeKind::Incomplete { .. }));
    assert_eq!(outcome.progress.decoded_events, 1);
    assert_eq!(outcome.progress.durable_events, 0);
    assert!(!outcome.progress.ack_allowed);
    assert!(!dir.join("SEAL").exists());
    assert!(!dir.join("chunk-000001").exists());
}

#[test]
fn extra_data_after_seal_is_stream_closed_and_the_seal_remains() {
    let root = tempdir();
    let dir = root.join("txn");
    let mut lines = sample_feed();
    lines.extend_from_slice(
        data(&Row {
            txn: "inv17",
            global: 4,
            collection: 3,
            table: "public.orders",
            key: "99",
            op: "upsert",
            payload: json!([]),
            unavailable: &[],
        })
        .as_bytes(),
    );
    lines.push(b'\n');
    let err = ingest(&dir, Limits::product(), &lines).expect_err("extra");
    assert_eq!(err.code, FaultCode::StreamClosed);
    let Recovery::Committed(committed) = recover(&dir).unwrap() else {
        panic!("committed");
    };
    assert_eq!(committed.manifest.count, 3);
    assert!(!committed.ack_allowed());
}

#[test]
fn line_too_long_and_cr_do_not_write_a_chunk() {
    let root = tempdir();
    let mut limits = Limits::product();
    limits.max_line_bytes = 32;
    let err = ingest(
        &root.join("long"),
        limits,
        &feed(&[bind(&["public.orders"])]),
    )
    .expect_err("long");
    assert_eq!(err.code, FaultCode::LineTooLong);
    assert!(!root.join("long").join("chunk-000001").exists());

    let err = ingest(
        &root.join("cr"),
        Limits::product(),
        b"{\"kind\":\"begin\",\"txn_id\":\"x\"}\r\n",
    )
    .expect_err("cr");
    assert_eq!(err.code, FaultCode::Malformed);
    assert!(!root.join("cr").join("SEAL").exists());
}
