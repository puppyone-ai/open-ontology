mod common;

use cdc_transaction::{
    canonical_preimage, encode_position, event_hash, parse_commit_lsn, Assembler, CanonicalData,
    Cell, Effect, FaultCode, Op, PendingStatus, MAX_LINE_BYTES, MAX_PAYLOAD_BYTES, MAX_TXN_EVENTS,
};
use common::{begin, bind, data, drive, end, rollback, upsert, Drive, Row, HASH, HASH_B, TS, TS_Z};
use serde_json::{json, Value};

fn code(drive: &Drive) -> FaultCode {
    drive.fault.as_ref().expect("refusal").code
}

fn orders(txn: &str, global: u64, key: &str) -> String {
    data(&upsert(txn, global, global, "public.orders", key))
}

#[test]
fn hand_two_table_update_delete_reinsert() {
    let line_key = "1";
    let delete = data(&Row {
        txn: "inv17",
        global: 2,
        collection: 1,
        table: "public.lines",
        key: line_key,
        op: "delete",
        payload: json!([]),
        unavailable: &[],
    });
    let reinsert = data(&Row {
        txn: "inv17",
        global: 3,
        collection: 2,
        table: "public.lines",
        key: line_key,
        op: "upsert",
        payload: json!([
            {"type": "text", "ordinal": 0, "value": "a"},
            {"type": "null", "ordinal": 1},
            {"type": "unavailable", "ordinal": 2}
        ]),
        unavailable: &[2],
    });
    let order = data(&Row {
        txn: "inv17",
        global: 1,
        collection: 1,
        table: "public.orders",
        key: "17",
        op: "upsert",
        payload: json!([{"type": "text", "ordinal": 0, "value": "open"}]),
        unavailable: &[],
    });
    let driven = drive(&[
        bind(&["public.lines", "public.orders"]),
        begin("inv17"),
        order,
        delete,
        reinsert,
        end(
            "inv17",
            3,
            &[("public.lines", 2), ("public.orders", 1)],
            "16/B6E40",
            TS,
        ),
    ]);
    assert!(driven.fault.is_none());
    assert_eq!(driven.completed.len(), 1);
    let txn = &driven.completed[0];
    assert_eq!(txn.kind, "complete_transaction");
    assert_eq!(txn.commit_order, 1);
    assert_eq!(txn.position, "18446744168199581248");
    assert_eq!(txn.event_count, 3);
    assert_eq!(txn.selected_row_count, 3);
    assert_eq!(txn.rows.len(), 3);
    assert_eq!(txn.rows[0].global_txn_sequence, 1);
    assert_eq!(txn.rows[1].op, Op::Delete);
    assert_eq!(txn.rows[2].global_txn_sequence, 3);
    assert_eq!(txn.rows[1].key, txn.rows[2].key);
    let payload = serde_json::to_value(&txn.rows[2].payload).expect("payload");
    assert_eq!(payload[1]["type"], "null");
    assert_eq!(payload[2]["type"], "unavailable");
    assert!(payload[1].get("value").is_none());
    assert!(payload[2].get("value").is_none());
    assert_eq!(txn.rows[2].unavailable_ordinals, vec![2]);
    let key = vec![Cell::Int {
        ordinal: 0,
        value: "1".to_owned(),
    }];
    let cells = vec![
        Cell::Text {
            ordinal: 0,
            value: "a".to_owned(),
        },
        Cell::Null { ordinal: 1 },
        Cell::Unavailable { ordinal: 2 },
    ];
    let preimage = canonical_preimage(&CanonicalData {
        txn_id: "inv17",
        global_txn_sequence: 3,
        data_collection_sequence: 2,
        table: "public.lines",
        key: &key,
        op: Op::Upsert,
        payload: &cells,
        unavailable_ordinals: &[2],
    })
    .expect("preimage");
    assert_eq!(txn.rows[2].event_hash, event_hash(&preimage).expect("hash"));
    let attestations = serde_json::to_value(&txn.attestations).expect("attestations");
    for (key, value) in attestations.as_object().expect("object") {
        assert_eq!(value, &Value::Bool(false), "{key}");
    }
    assert!(driven.progress.applied.is_none());
    assert!(!driven.progress.resume_required);
    assert!(!driven.progress.ack_allowed);
    assert_eq!(driven.progress.complete_decoded.transactions, 1);
    assert_eq!(driven.progress.seen.data, 3);
    assert_eq!(
        driven.progress.complete_decoded.last_position.as_deref(),
        Some("18446744168199581248")
    );
}

#[test]
fn interleaved_commit_order_is_end_order() {
    let driven = drive(&[
        bind(&["public.orders"]),
        begin("long"),
        orders("long", 1, "1"),
        begin("short"),
        orders("short", 1, "2"),
        end("short", 1, &[("public.orders", 1)], "0/20", TS_Z),
        end("long", 1, &[("public.orders", 1)], "0/30", TS_Z),
    ]);
    assert!(driven.fault.is_none());
    assert_eq!(driven.completed[0].txn_id, "short");
    assert_eq!(driven.completed[0].commit_order, 1);
    assert_eq!(driven.completed[1].txn_id, "long");
    assert_eq!(driven.completed[1].commit_order, 2);
    assert_eq!(driven.completed[1].rows[0].global_txn_sequence, 1);
}

#[test]
fn row_lsn_is_not_a_commit_position() {
    let mut line = orders("inv17", 1, "1");
    line.insert_str(1, "\"row_lsn\":\"0/1\",");
    let driven = drive(&[
        bind(&["public.orders"]),
        begin("inv17"),
        line,
        end("inv17", 1, &[("public.orders", 1)], "0/1", TS_Z),
    ]);
    assert_eq!(code(&driven), FaultCode::Malformed);
    assert!(driven.completed.is_empty());
}

#[test]
fn unselected_rows_count_and_are_omitted() {
    let audit = data(&upsert("inv17", 1, 1, "public.audit", "9"));
    let driven = drive(&[
        bind(&["public.orders"]),
        begin("inv17"),
        audit,
        end("inv17", 1, &[("public.audit", 1)], "0/1", TS_Z),
    ]);
    assert!(driven.fault.is_none());
    assert_eq!(driven.completed[0].event_count, 1);
    assert_eq!(driven.completed[0].selected_row_count, 0);
    assert!(driven.completed[0].rows.is_empty());
    assert!(driven.progress.applied.is_none());
    assert!(!driven.progress.ack_allowed);
    assert_eq!(driven.progress.complete_decoded.transactions, 1);
}

#[test]
fn selected_siblings_keep_original_sequences() {
    let driven = drive(&[
        bind(&["public.orders"]),
        begin("inv17"),
        data(&upsert("inv17", 1, 1, "public.orders", "1")),
        data(&upsert("inv17", 2, 1, "public.audit", "9")),
        data(&upsert("inv17", 3, 2, "public.orders", "2")),
        end(
            "inv17",
            3,
            &[("public.audit", 1), ("public.orders", 2)],
            "0/4",
            TS_Z,
        ),
    ]);
    assert!(driven.fault.is_none());
    let rows = &driven.completed[0].rows;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].global_txn_sequence, 1);
    assert_eq!(rows[1].global_txn_sequence, 3);
}

#[test]
fn eof_keeps_open_txn_and_reports_completed_separately() {
    let open = drive(&[bind(&["public.orders"]), begin("inv17"), orders("inv17", 1, "1")]);
    assert!(open.fault.is_none());
    assert!(open.completed.is_empty());
    assert!(open.progress.resume_required);
    assert_eq!(open.progress.pending.len(), 1);
    assert_eq!(open.progress.pending[0].status, PendingStatus::Open);
    assert_eq!(open.progress.pending[0].data_events, 1);
    assert!(open.progress.applied.is_none());

    let mixed = drive(&[
        bind(&["public.orders"]),
        begin("done"),
        orders("done", 1, "1"),
        end("done", 1, &[("public.orders", 1)], "0/8", TS_Z),
        begin("open"),
        orders("open", 1, "2"),
    ]);
    assert!(mixed.fault.is_none());
    assert_eq!(mixed.completed.len(), 1);
    assert_eq!(mixed.completed[0].txn_id, "done");
    assert!(mixed.progress.resume_required);
    assert_eq!(mixed.progress.pending[0].txn_id, "open");
}

#[test]
fn end_too_early_keeps_the_pending_event() {
    let driven = drive(&[
        bind(&["public.orders"]),
        begin("inv17"),
        orders("inv17", 1, "1"),
        end("inv17", 2, &[("public.orders", 2)], "0/1", TS_Z),
    ]);
    assert_eq!(code(&driven), FaultCode::EndTooEarly);
    assert!(driven.completed.is_empty());
    assert_eq!(driven.progress.pending[0].status, PendingStatus::Refused);
    assert_eq!(driven.progress.pending[0].data_events, 1);
    assert_eq!(driven.progress.complete_decoded.transactions, 0);
}

#[test]
fn rollback_drops_pending_and_replays() {
    let driven = drive(&[
        bind(&["public.orders"]),
        begin("inv17"),
        orders("inv17", 1, "1"),
        rollback("inv17"),
        rollback("inv17"),
    ]);
    assert!(driven.fault.is_none());
    assert!(driven.completed.is_empty());
    assert!(driven.progress.pending.is_empty());
    assert!(!driven.progress.resume_required);
    assert_eq!(driven.progress.seen.rollback, 1);
    assert_eq!(driven.progress.seen.duplicate_rollback, 1);
    assert_eq!(driven.progress.complete_decoded.transactions, 0);
}

#[test]
fn identical_replay_is_idempotent_and_conflict_refuses() {
    let mut assembler = Assembler::new();
    let row = orders("inv17", 1, "1");
    for line in [bind(&["public.orders"]), begin("inv17"), row.clone()] {
        assembler.apply_line(&line).expect("accept");
    }
    let again = assembler.apply_line(&row).expect("replay");
    assert_eq!(again.effect, Effect::Idempotent);
    assert!(again.completed.is_none());
    let end_line = end("inv17", 1, &[("public.orders", 1)], "0/1", TS_Z);
    let done = assembler.apply_line(&end_line).expect("end");
    assert_eq!(done.completed.expect("txn").event_count, 1);
    assert_eq!(assembler.progress().seen.duplicate_data, 1);

    let changed = data(&Row {
        txn: "other",
        global: 1,
        collection: 1,
        table: "public.orders",
        key: "1",
        op: "upsert",
        payload: json!([{"type": "text", "ordinal": 0, "value": "x"}]),
        unavailable: &[],
    });
    let conflict = drive(&[
        bind(&["public.orders"]),
        begin("other"),
        orders("other", 1, "1"),
        changed,
    ]);
    assert_eq!(code(&conflict), FaultCode::DuplicateConflict);
    assert_eq!(conflict.progress.pending[0].data_events, 1);
}

#[test]
fn commit_position_is_unique_and_resets_only_on_a_barrier() {
    let regression = drive(&[
        bind(&["public.orders"]),
        begin("a"),
        orders("a", 1, "1"),
        end("a", 1, &[("public.orders", 1)], "0/30", TS_Z),
        begin("b"),
        orders("b", 1, "2"),
        end("b", 1, &[("public.orders", 1)], "0/10", TS_Z),
    ]);
    assert_eq!(code(&regression), FaultCode::CommitPositionRegression);
    assert_eq!(regression.completed.len(), 1);

    let reuse = drive(&[
        bind(&["public.orders"]),
        begin("a"),
        orders("a", 1, "1"),
        end("a", 1, &[("public.orders", 1)], "0/30", TS_Z),
        json!({"kind":"barrier","reason":"reset","lineage":HASH_B,"epoch":1}).to_string(),
        begin("b"),
        orders("b", 1, "2"),
        end("b", 1, &[("public.orders", 1)], "0/10", TS_Z),
        begin("c"),
        orders("c", 1, "3"),
        end("c", 1, &[("public.orders", 1)], "0/30", TS_Z),
    ]);
    assert_eq!(code(&reuse), FaultCode::CommitPositionConflict);
    assert_eq!(reuse.completed.len(), 2);
    assert_eq!(reuse.completed[1].commit_lsn, "0/10");
    assert_eq!(reuse.completed[1].lineage, HASH_B);
}

#[test]
fn epoch_barrier_changes_the_position_high_half() {
    let open = drive(&[
        bind(&["public.orders"]),
        begin("a"),
        json!({"kind":"barrier","reason":"epoch","epoch":2}).to_string(),
    ]);
    assert_eq!(code(&open), FaultCode::OpenTxnAtBarrier);

    let same = drive(&[
        bind(&["public.orders"]),
        json!({"kind":"barrier","reason":"epoch","epoch":1}).to_string(),
    ]);
    assert_eq!(code(&same), FaultCode::EpochUnchanged);

    let with_lineage = drive(&[
        bind(&["public.orders"]),
        json!({"kind":"barrier","reason":"epoch","epoch":2,"lineage":HASH_B}).to_string(),
    ]);
    assert_eq!(code(&with_lineage), FaultCode::LineageNotAllowed);

    let missing = drive(&[
        bind(&["public.orders"]),
        json!({"kind":"barrier","reason":"reset","epoch":1}).to_string(),
    ]);
    assert_eq!(code(&missing), FaultCode::LineageRequired);

    let unchanged = drive(&[
        bind(&["public.orders"]),
        json!({"kind":"barrier","reason":"reset","lineage":HASH,"epoch":2}).to_string(),
    ]);
    assert_eq!(code(&unchanged), FaultCode::LineageUnchanged);

    let lsn = parse_commit_lsn("0/1").expect("lsn");
    let driven = drive(&[
        bind(&["public.orders"]),
        begin("a"),
        orders("a", 1, "1"),
        end("a", 1, &[("public.orders", 1)], "0/1", TS_Z),
        json!({"kind":"barrier","reason":"epoch","epoch":2}).to_string(),
        begin("b"),
        orders("b", 1, "2"),
        end("b", 1, &[("public.orders", 1)], "0/1", TS_Z),
    ]);
    assert!(driven.fault.is_none());
    assert_eq!(
        driven.completed[0].position,
        encode_position(1, lsn).expect("epoch1")
    );
    assert_eq!(
        driven.completed[1].position,
        encode_position(2, lsn).expect("epoch2")
    );
    assert_eq!(driven.completed[1].epoch, 2);
}

#[test]
fn count_overflow_and_bad_lsn_do_not_wrap() {
    let overflow = drive(&[
        bind(&["public.orders", "public.lines"]),
        begin("inv17"),
        end(
            "inv17",
            0,
            &[("public.lines", u64::MAX), ("public.orders", 1)],
            "0/1",
            TS_Z,
        ),
    ]);
    assert_eq!(code(&overflow), FaultCode::CountOverflow);

    let wide = drive(&[
        bind(&["public.orders"]),
        begin("inv17"),
        orders("inv17", 1, "1"),
        end("inv17", 1, &[("public.orders", 1)], "FFFFFFFFF/0", TS_Z),
    ]);
    assert_eq!(code(&wide), FaultCode::CommitLsnInvalid);
    assert!(wide.completed.is_empty());
}

#[test]
fn sequence_and_collection_faults() {
    let out_of_range = drive(&[
        bind(&["public.orders"]),
        begin("inv17"),
        orders("inv17", 1, "1"),
        orders("inv17", 3, "2"),
        end("inv17", 2, &[("public.orders", 2)], "0/1", TS_Z),
    ]);
    assert_eq!(code(&out_of_range), FaultCode::SequenceOutOfRange);

    let early = drive(&[
        bind(&["public.orders"]),
        begin("inv17"),
        orders("inv17", 1, "1"),
        orders("inv17", 3, "2"),
        end("inv17", 3, &[("public.orders", 3)], "0/1", TS_Z),
    ]);
    assert_eq!(code(&early), FaultCode::EndTooEarly);

    let unknown = drive(&[
        bind(&["public.orders"]),
        begin("inv17"),
        orders("inv17", 1, "1"),
        end("inv17", 1, &[("public.lines", 1)], "0/1", TS_Z),
    ]);
    assert_eq!(code(&unknown), FaultCode::CollectionUnknown);

    let gap = drive(&[
        bind(&["public.orders"]),
        begin("inv17"),
        orders("inv17", 1, "1"),
        orders("inv17", 2, "2"),
        end("inv17", 1, &[("public.orders", 1)], "0/1", TS_Z),
    ]);
    assert_eq!(code(&gap), FaultCode::CollectionCountGap);

    let swapped = drive(&[
        bind(&["public.orders"]),
        begin("inv17"),
        orders("inv17", 2, "2"),
        orders("inv17", 1, "1"),
        end("inv17", 2, &[("public.orders", 2)], "0/1", TS_Z),
    ]);
    assert!(swapped.fault.is_none());
    assert_eq!(swapped.completed[0].rows[0].global_txn_sequence, 1);
    assert_eq!(swapped.completed[0].rows[1].global_txn_sequence, 2);
}

#[test]
fn ordinal_null_and_duplicate_keys() {
    let mismatch = data(&Row {
        txn: "inv17",
        global: 1,
        collection: 1,
        table: "public.orders",
        key: "1",
        op: "upsert",
        payload: json!([{"type": "null", "ordinal": 1}]),
        unavailable: &[1],
    });
    let driven = drive(&[bind(&["public.orders"]), begin("inv17"), mismatch]);
    assert_eq!(code(&driven), FaultCode::OrdinalMismatch);

    let decimal = data(&Row {
        txn: "inv17",
        global: 1,
        collection: 1,
        table: "public.orders",
        key: "1",
        op: "upsert",
        payload: json!([{"type": "decimal", "ordinal": 0, "value": "1.10"}]),
        unavailable: &[],
    });
    let kept = drive(&[
        bind(&["public.orders"]),
        begin("inv17"),
        decimal,
        end("inv17", 1, &[("public.orders", 1)], "0/1", TS_Z),
    ]);
    let payload = serde_json::to_value(&kept.completed[0].rows[0].payload).expect("payload");
    assert_eq!(payload[0]["value"], "1.10");

    let leading = data(&Row {
        txn: "inv17",
        global: 1,
        collection: 1,
        table: "public.orders",
        key: "1",
        op: "upsert",
        payload: json!([{"type": "int", "ordinal": 0, "value": "01"}]),
        unavailable: &[],
    });
    assert_eq!(
        code(&drive(&[bind(&["public.orders"]), begin("inv17"), leading])),
        FaultCode::PayloadInvalid
    );

    let duplicate = "{\"kind\":\"begin\",\"txn_id\":\"a\",\"\\u0074xn_id\":\"b\"}";
    let parsed = drive(&[bind(&["public.orders"]), duplicate.to_owned()]);
    assert_eq!(code(&parsed), FaultCode::Malformed);

    let mut assembler = Assembler::new();
    assembler
        .apply_line(&bind(&["public.orders"]))
        .expect("bind");
    let err = assembler
        .apply_line("{\"kind\":\"begin\",\"txn_id\":\"a\"}\r")
        .expect_err("cr");
    assert_eq!(err.code, FaultCode::Malformed);
}

#[test]
fn open_set_stops_at_sixteen() {
    let mut lines = vec![bind(&["public.orders"])];
    for index in 1..=16 {
        lines.push(begin(&format!("t{index}")));
    }
    lines.push(begin("t17"));
    let driven = drive(&lines);
    assert_eq!(code(&driven), FaultCode::OpenSetExceeded);
    assert!(!driven.progress.needs_spool);
    assert_eq!(driven.progress.pending.len(), 16);
    assert!(driven
        .progress
        .pending
        .iter()
        .all(|txn| txn.txn_id != "t17"));
}

#[test]
fn empty_transaction_and_rollback_after_complete() {
    let empty = drive(&[
        bind(&["public.orders"]),
        begin("empty"),
        end("empty", 0, &[], "0/1", TS_Z),
    ]);
    assert!(empty.fault.is_none());
    assert_eq!(empty.completed[0].event_count, 0);
    assert!(empty.completed[0].rows.is_empty());

    let late = drive(&[
        bind(&["public.orders"]),
        begin("inv17"),
        orders("inv17", 1, "1"),
        end("inv17", 1, &[("public.orders", 1)], "0/2", TS_Z),
        rollback("inv17"),
    ]);
    assert_eq!(code(&late), FaultCode::RollbackAfterComplete);
    assert_eq!(late.completed.len(), 1);
}

#[test]
fn line_of_exactly_one_mebibyte_is_accepted() {
    let mut line = bind(&["public.orders"]);
    let pad = MAX_LINE_BYTES - line.len();
    line.extend(std::iter::repeat_n(' ', pad));
    assert_eq!(line.len(), MAX_LINE_BYTES);
    let mut assembler = Assembler::new();
    assert!(assembler.apply_line(&line).is_ok());
    line.push(' ');
    let err = Assembler::new().apply_line(&line).expect_err("over");
    assert_eq!(err.code, FaultCode::LineTooLong);
}

#[test]
fn txn_of_200000_completes_and_the_next_needs_spool() {
    let mut assembler = Assembler::new();
    assembler
        .apply_line(&bind(&["public.orders"]))
        .expect("bind");
    assembler.apply_line(&begin("t")).expect("begin");
    for seq in 1..=MAX_TXN_EVENTS {
        assembler.apply_line(&tiny(seq)).expect("data");
    }
    let done = assembler
        .apply_line(&end("t", MAX_TXN_EVENTS, &[("public.orders", MAX_TXN_EVENTS)], "0/1", TS_Z))
        .expect("end");
    let txn = done.completed.expect("completed");
    assert_eq!(txn.event_count, MAX_TXN_EVENTS);
    assert_eq!(txn.selected_row_count, MAX_TXN_EVENTS);
    assert_eq!(txn.rows.first().expect("first").global_txn_sequence, 1);
    assert_eq!(
        txn.rows.last().expect("last").global_txn_sequence,
        MAX_TXN_EVENTS
    );
    drop(txn);

    let mut spool = Assembler::new();
    spool.apply_line(&bind(&["public.orders"])).expect("bind");
    spool.apply_line(&begin("t")).expect("begin");
    for seq in 1..=MAX_TXN_EVENTS {
        spool.apply_line(&tiny(seq)).expect("data");
    }
    let err = spool.apply_line(&tiny(MAX_TXN_EVENTS + 1)).expect_err("spool");
    assert_eq!(err.code, FaultCode::NeedsSpool);
    let progress = spool.finish();
    assert!(progress.needs_spool);
    assert!(progress.resume_required);
    assert!(progress.applied.is_none());
    assert!(!progress.ack_allowed);
    assert_eq!(progress.pending[0].data_events, MAX_TXN_EVENTS);
    assert_eq!(progress.pending[0].status, PendingStatus::Refused);
    assert_eq!(progress.complete_decoded.transactions, 0);
}

#[test]
fn payload_budget_is_exact_and_the_next_event_is_not_stored() {
    let plan = payload_plan();
    let mut total = 0u64;
    for (seq, text) in plan.iter().enumerate() {
        total += u64::try_from(text_charge(u64::try_from(seq).expect("seq") + 1, text))
            .expect("charge");
    }
    assert_eq!(total, MAX_PAYLOAD_BYTES);

    let mut assembler = Assembler::new();
    apply_plan(&mut assembler, &plan);
    let done = assembler
        .apply_line(&end(
            "p",
            u64::try_from(plan.len()).expect("len"),
            &[("public.orders", u64::try_from(plan.len()).expect("len"))],
            "0/1",
            TS_Z,
        ))
        .expect("end");
    assert_eq!(
        done.completed.expect("txn").event_count,
        u64::try_from(plan.len()).expect("len")
    );

    let mut spool = Assembler::new();
    apply_plan(&mut spool, &plan);
    let extra = text_line(u64::try_from(plan.len()).expect("len") + 1, "a");
    let err = spool.apply_line(&extra).expect_err("over");
    assert_eq!(err.code, FaultCode::NeedsSpool);
    assert_eq!(
        spool.finish().pending[0].data_events,
        u64::try_from(plan.len()).expect("len")
    );
}

fn tiny(seq: u64) -> String {
    format!(
        "{{\"kind\":\"data\",\"txn_id\":\"t\",\"global_txn_sequence\":{seq},\"data_collection_sequence\":{seq},\"table\":\"public.orders\",\"key\":[{{\"type\":\"int\",\"ordinal\":0,\"value\":\"{seq}\"}}],\"op\":\"upsert\",\"payload\":[],\"unavailable_ordinals\":[]}}"
    )
}

fn apply_plan(assembler: &mut Assembler, plan: &[String]) {
    assembler
        .apply_line(&bind(&["public.orders"]))
        .expect("bind");
    assembler.apply_line(&begin("p")).expect("begin");
    for (seq, text) in plan.iter().enumerate() {
        let line = text_line(u64::try_from(seq).expect("seq") + 1, text);
        assert!(line.len() <= MAX_LINE_BYTES);
        assembler.apply_line(&line).expect("data");
    }
}

fn payload_plan() -> Vec<String> {
    let chunk = "a".repeat(800_000);
    let mut plan = Vec::new();
    let mut total = 0u64;
    let mut seq = 0u64;
    loop {
        let charge = u64::try_from(text_charge(seq + 1, &chunk)).expect("charge");
        if total.saturating_add(charge) > MAX_PAYLOAD_BYTES {
            break;
        }
        plan.push(chunk.clone());
        total += charge;
        seq += 1;
    }
    let rem = MAX_PAYLOAD_BYTES - total;
    if rem == 0 {
        return plan;
    }
    let next = seq + 1;
    let empty = u64::try_from(text_charge(next, "")).expect("empty");
    assert_eq!(
        text_charge(next, "a"),
        text_charge(next, "").saturating_add(1)
    );
    if rem >= empty {
        let pad = usize::try_from(rem - empty).expect("pad");
        plan.push("a".repeat(pad));
        return plan;
    }
    let last = plan.last_mut().expect("chunk");
    last.push_str(&"a".repeat(usize::try_from(rem).expect("rem")));
    plan
}

fn text_charge(seq: u64, text: &str) -> usize {
    let key = vec![Cell::Int {
        ordinal: 0,
        value: seq.to_string(),
    }];
    let payload = vec![Cell::Text {
        ordinal: 0,
        value: text.to_owned(),
    }];
    canonical_preimage(&CanonicalData {
        txn_id: "p",
        global_txn_sequence: seq,
        data_collection_sequence: seq,
        table: "public.orders",
        key: &key,
        op: Op::Upsert,
        payload: &payload,
        unavailable_ordinals: &[],
    })
    .expect("preimage")
    .len()
}

fn text_line(seq: u64, text: &str) -> String {
    json!({
        "kind": "data",
        "txn_id": "p",
        "global_txn_sequence": seq,
        "data_collection_sequence": seq,
        "table": "public.orders",
        "key": [{"type": "int", "ordinal": 0, "value": seq.to_string()}],
        "op": "upsert",
        "payload": [{"type": "text", "ordinal": 0, "value": text}],
        "unavailable_ordinals": []
    })
    .to_string()
}
