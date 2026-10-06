#![forbid(unsafe_code)]

mod common;

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};

use cdc_spool::{recover, Cell, Limits, Op, OutcomeKind, Recovery, Spool};
use common::{begin, bind, end, oracle, tempdir, TS};

fn expect_line(seq: u64) -> String {
    let (table, collection) = if seq.is_multiple_of(2) {
        ("public.lines", seq / 2)
    } else {
        ("public.orders", seq.div_ceil(2))
    };
    format!(
        "{{\"data_collection_sequence\":{collection},\"global_txn_sequence\":{seq},\"key\":[{{\"ordinal\":0,\"type\":\"int\",\"value\":\"{seq}\"}}],\"op\":\"upsert\",\"payload\":[{{\"ordinal\":0,\"type\":\"int\",\"value\":\"{seq}\"}}],\"table\":\"{table}\",\"txn_id\":\"bulk1\",\"unavailable_ordinals\":[]}}"
    )
}

fn input_line(seq: u64) -> String {
    let (table, collection) = if seq.is_multiple_of(2) {
        ("public.lines", seq / 2)
    } else {
        ("public.orders", seq.div_ceil(2))
    };
    format!(
        "{{\"kind\":\"data\",\"txn_id\":\"bulk1\",\"global_txn_sequence\":{seq},\"data_collection_sequence\":{collection},\"table\":\"{table}\",\"key\":[{{\"type\":\"int\",\"ordinal\":0,\"value\":\"{seq}\"}}],\"op\":\"upsert\",\"payload\":[{{\"type\":\"int\",\"ordinal\":0,\"value\":\"{seq}\"}}],\"unavailable_ordinals\":[]}}"
    )
}

#[test]
#[allow(clippy::too_many_lines)]
fn five_hundred_thousand_events_stream_without_a_full_payload_journal() {
    oracle::check();
    let root = tempdir();
    let feed = root.join("feed.jsonl");
    {
        let mut out = BufWriter::new(File::create(&feed).unwrap());
        writeln!(out, "{}", bind(&["public.lines", "public.orders"])).unwrap();
        writeln!(out, "{}", begin("bulk1")).unwrap();
        for seq in 1..=500_000 {
            writeln!(out, "{}", input_line(seq)).unwrap();
        }
        writeln!(
            out,
            "{}",
            end(
                "bulk1",
                500_000,
                &[("public.lines", 250_000), ("public.orders", 250_000)],
                "16/B6E40",
                TS
            )
        )
        .unwrap();
        out.flush().unwrap();
    }

    let mut lines = 0u64;
    let mut first_data = String::new();
    let mut last_data = String::new();
    for line in BufReader::new(File::open(&feed).unwrap()).lines() {
        let line = line.unwrap();
        lines += 1;
        if line.contains("\"kind\":\"data\"") {
            if first_data.is_empty() {
                first_data = line.clone();
            }
            last_data = line;
        }
    }
    assert_eq!(lines, 500_003);
    assert!(first_data.contains("\"global_txn_sequence\":1"));
    assert!(last_data.contains("\"global_txn_sequence\":500000"));
    assert_eq!(oracle::hex(&oracle::sha256(first_data.as_bytes())).len(), 64);

    let dir = root.join("txn");
    let mut spool = Spool::create_new(&dir, Limits::product()).unwrap();
    let outcome = spool
        .ingest_reader(BufReader::new(File::open(&feed).unwrap()))
        .expect("seal 500000");
    let OutcomeKind::Sealed(summary) = outcome.kind else {
        panic!("not sealed");
    };
    assert_eq!(summary.count, 500_000);
    assert_eq!(summary.commit_position, "18446744168199581248");
    assert!(!summary.ack_allowed);
    assert!(!summary.target_applied);
    assert!(!summary.cross_table_consistent);
    let hwm = summary.resident_hwm_bytes.expect("VmHWM");
    assert!(hwm > 0, "{hwm}");
    assert!(hwm < 256 * 1024 * 1024, "{hwm}");
    if let Ok(path) = std::env::var("CDC_SPOOL_HWM_OUT") {
        std::fs::write(&path, format!("{hwm}\n")).unwrap();
    }
    assert_eq!(outcome.progress.seen_data, 500_000);
    assert_eq!(outcome.progress.durable_events, 500_000);
    assert!(outcome.progress.applied.is_none());

    let Recovery::Committed(committed) = recover(&dir).expect("recover") else {
        panic!("not committed");
    };
    assert_eq!(committed.manifest.count, 500_000);
    assert_eq!(committed.manifest.first_sequence, Some(1));
    assert_eq!(committed.manifest.last_sequence, Some(500_000));
    assert_eq!(
        committed
            .manifest
            .per_collection_counts
            .get("public.lines")
            .copied(),
        Some(250_000)
    );
    assert_eq!(
        committed
            .manifest
            .per_collection_counts
            .get("public.orders")
            .copied(),
        Some(250_000)
    );
    assert!(!committed.manifest.ack_allowed);
    assert!(committed.manifest.applied.is_null());
    let mut chunk_events = 0u64;
    let mut set = String::new();
    for chunk in &committed.manifest.chunks {
        let bytes = std::fs::read(dir.join(&chunk.name)).unwrap();
        assert_eq!(bytes.len() as u64, chunk.bytes);
        assert_eq!(oracle::hex(&oracle::sha256(&bytes)), chunk.sha256);
        chunk_events += chunk.events;
        set.push_str(&chunk.sha256);
        set.push('\n');
        assert!(chunk.bytes <= 4 * 1024 * 1024);
        assert!(chunk.events <= 8_192);
    }
    assert_eq!(chunk_events, 500_000);
    assert_eq!(
        committed.manifest.chunk_set_sha256,
        oracle::hex(&oracle::sha256(set.as_bytes()))
    );
    assert!(committed.manifest.chunks.len() < 96);

    let mut iter = committed.iter().unwrap();
    let mut orders = 0u64;
    let mut line_rows = 0u64;
    for seq in 1..=500_000 {
        let event = iter.next().expect("row").expect("event");
        let expected = expect_line(seq);
        assert_eq!(event.preimage, expected, "{seq}");
        assert_eq!(
            event.event_hash,
            oracle::hex(&oracle::sha256(expected.as_bytes())),
            "{seq}"
        );
        assert_eq!(event.global_txn_sequence, seq);
        let (table, collection) = if seq.is_multiple_of(2) {
            line_rows += 1;
            ("public.lines", seq / 2)
        } else {
            orders += 1;
            ("public.orders", seq.div_ceil(2))
        };
        assert_eq!(event.table, table);
        assert_eq!(event.data_collection_sequence, collection);
        assert_eq!(event.op, Op::Upsert);
        assert_eq!(
            event.key,
            vec![Cell::Int {
                ordinal: 0,
                value: seq.to_string(),
            }]
        );
        assert_eq!(
            event.payload,
            vec![Cell::Int {
                ordinal: 0,
                value: seq.to_string(),
            }]
        );
        assert!(event.unavailable_ordinals.is_empty());
    }
    assert!(iter.next().is_none());
    assert_eq!(orders, 250_000);
    assert_eq!(line_rows, 250_000);
}
