//! Independent R2-W probe. Scripted finite TCP peer, current dependencies only.
//!
//! Measurements use the public allocation events plus process RSS. This file
//! does not use `unsafe` (the workspace forbids it, including in tests).

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use sqlx::postgres::{
    IncomingWireError, PgBackendFrame, PgConnectOptions, PgConnection, PgIncomingAllocEvent,
    PgIncomingAllocation, PgIncomingBounds, PgIncomingLimit, PgSslMode,
};
use sqlx::Connection;

const HANG: Duration = Duration::from_secs(10);
const CANCEL_WAIT: Duration = Duration::from_millis(800);
const LEAK: &[&str] = &["NOLEAK", "EXTRA", "SECRET"];

#[test]
fn parameter_validation_rejects_inconsistent_caps() {
    let low = PgIncomingBounds::try_new(3, 0, 0, 0).expect_err("message below 4");
    assert_invalid(low);
    let value = PgIncomingBounds::try_new(64, 5, 4, 1).expect_err("value above row");
    assert_invalid(value);
    let zero = PgIncomingBounds::try_new(4, 0, 0, 0).expect("zero value row columns");
    assert_eq!(
        (
            zero.max_message_len(),
            zero.max_value_len(),
            zero.max_row_payload(),
            zero.max_columns()
        ),
        (4, 0, 0, 0)
    );
    // A row cap above the message cap is accepted by try_new. The message cap
    // still fires first, so this is not a bypass. Recorded, not treated as a pass
    // of a tighter validator.
    let wide = PgIncomingBounds::try_new(8, 0, 100, 0).expect("row above message");
    assert!(wide.max_row_payload() > wide.max_message_len());
}

#[tokio::test(flavor = "current_thread")]
async fn fragmented_header_and_field_prefixes_resume_in_order() {
    let row = data_row(&[Some(b"ABCD"), Some(b"XY")]);
    // type, 4-byte length, natts, first field prefix, first payload, second prefix.
    let stops = [1usize, 4, 5, 6, 7, 8, 11, row.len() - 1];
    for stop in stops {
        cancel_resume(Some(bounds(4_000, 100, 4_000, 8)), &row, stop, 1).await;
    }
    // Two cancels on one frame, then the following frame.
    cancel_resume(Some(bounds(4_000, 100, 4_000, 8)), &row, 4, 2).await;
    cancel_resume(None, &row, 3, 1).await;
    cancel_resume(None, &row, 9, 1).await;

    let notice = frame(b'N', b"OK");
    cancel_resume(Some(bounds(64, 8, 64, 4)), &notice, 1, 1).await;
    cancel_resume(Some(bounds(64, 8, 64, 4)), &notice, 4, 1).await;
    cancel_resume(Some(bounds(64, 8, 64, 4)), &notice, notice.len() - 1, 1).await;
}

#[tokio::test(flavor = "current_thread")]
async fn null_zero_negative_truncation_remaining_and_short_lengths() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(200, 8, 80, 8)))
        .expect("set");
    let row = data_row(&[None, Some(b""), Some(b"Z")]);
    peer.write(row.clone());
    let got = recv(&mut conn).await;
    assert_eq!(got.contents().as_ref(), &row[5..]);
    assert!(conn.incoming_synchronized());
    drop(peer);

    // Value cap 0 accepts an empty field and rejects a one-byte field.
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(80, 0, 40, 4)))
        .expect("set");
    let empty = data_row(&[Some(b"")]);
    peer.write(empty.clone());
    let got = recv(&mut conn).await;
    assert_eq!(got.contents().as_ref(), &empty[5..]);
    drop(peer);

    reject_one(
        "value-zero-plus-one",
        bounds(80, 0, 40, 4),
        prefix_row(4 + 2 + 4 + 1, 1, &[], 1),
        IncomingWireError::Limit(PgIncomingLimit::Value),
        11,
        0,
    )
    .await;

    reject_one(
        "negative-two",
        bounds(80, 8, 40, 4),
        prefix_row(16, 1, &[], -2),
        IncomingWireError::NegativeLength,
        11,
        0,
    )
    .await;
    reject_one(
        "negative-min",
        bounds(80, 8, 40, 4),
        prefix_row(16, 1, &[], i32::MIN),
        IncomingWireError::NegativeLength,
        11,
        0,
    )
    .await;

    // Declared payload runs past the declared message. The body is not sent.
    reject_one(
        "truncated-field",
        bounds(80, 30, 80, 4),
        prefix_row(12, 1, &[], 10),
        IncomingWireError::Truncated,
        11,
        0,
    )
    .await;

    // natts word itself does not fit in the declared frame.
    reject_one(
        "truncated-natts",
        bounds(80, 8, 80, 4),
        columns_header(1, 5),
        IncomingWireError::Truncated,
        5,
        0,
    )
    .await;

    // Field bytes end before the declared frame, and the extra tail is actually sent.
    let mut remaining = columns_header(0, 8);
    remaining.extend_from_slice(b"EXTRA");
    reject_one(
        "remaining-with-tail",
        bounds(80, 8, 80, 4),
        remaining,
        IncomingWireError::Remaining,
        7,
        0,
    )
    .await;

    // Empty row: message length 6 is natts only. Length 7 leaves one byte over.
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(80, 0, 40, 4)))
        .expect("set");
    let empty_row = columns_header(0, 6);
    assert_eq!(empty_row.len(), 7);
    peer.write(empty_row.clone());
    let got = recv(&mut conn).await;
    assert_eq!(got.format_byte(), b'D');
    assert_eq!(got.contents().as_ref(), &empty_row[5..]);
    drop(peer);

    for len in [0u32, 1, 2, 3] {
        reject_one(
            "short-length",
            bounds(80, 8, 40, 4),
            header(b'I', len),
            IncomingWireError::ShortLength,
            5,
            0,
        )
        .await;
    }

    reject_one(
        "length-overflow",
        bounds(u32::MAX, u32::MAX, u32::MAX, 8),
        header(b'D', u32::MAX),
        IncomingWireError::Overflow,
        5,
        0,
    )
    .await;

    // Row cap 0 rejects at the natts word. max_columns 0 still allows natts 0
    // through the column check; the row sum then rejects.
    reject_one(
        "row-cap-zero",
        bounds(80, 0, 0, 0),
        columns_header(0, 6),
        IncomingWireError::Limit(PgIncomingLimit::Row),
        7,
        0,
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn exact_and_plus_one_message_value_row_columns() {
    // Notice body of 6 bytes: message length 10. Cap 10 accepts, cap 9 rejects.
    let notice = frame(b'N', b"NOTICE");
    assert_eq!(notice.len(), 11);
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(10, 0, 0, 0)))
        .expect("set");
    peer.write(notice.clone());
    let got = recv(&mut conn).await;
    assert_eq!(got.contents().as_ref(), b"NOTICE");
    drop(peer);
    reject_one(
        "message-plus-one",
        bounds(9, 0, 0, 0),
        notice,
        IncomingWireError::Limit(PgIncomingLimit::Message),
        5,
        0,
    )
    .await;

    let exact = data_row(&[Some(b"abc")]);
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(40, 3, 30, 4)))
        .expect("set");
    peer.write(exact.clone());
    let got = recv(&mut conn).await;
    assert_eq!(got.contents().as_ref(), &exact[5..]);
    drop(peer);
    reject_one(
        "value-plus-one",
        bounds(40, 3, 30, 4),
        prefix_row(4 + 2 + 4 + 4, 1, &[], 4),
        IncomingWireError::Limit(PgIncomingLimit::Value),
        11,
        0,
    )
    .await;

    // Two 1-byte fields: row payload = 2 + 8 + 2 = 12. One more byte on the
    // second field is the row cap plus one. The second body is not sent.
    let row = data_row(&[Some(b"Q"), Some(b"R")]);
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(40, 1, 12, 4)))
        .expect("set");
    peer.write(row.clone());
    let got = recv(&mut conn).await;
    assert_eq!(got.contents().as_ref(), &row[5..]);
    drop(peer);
    reject_one(
        "row-plus-one",
        bounds(40, 2, 12, 4),
        prefix_row(4 + 2 + 4 + 1 + 4 + 2, 2, &[Some(b"Q")], 2),
        IncomingWireError::Limit(PgIncomingLimit::Row),
        16,
        0,
    )
    .await;

    let two = data_row(&[Some(b"Q"), Some(b"R")]);
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(40, 8, 40, 2)))
        .expect("set");
    peer.write(two.clone());
    let got = recv(&mut conn).await;
    assert_eq!(got.contents().as_ref(), &two[5..]);
    drop(peer);
    reject_one(
        "columns-plus-one",
        bounds(80, 8, 80, 2),
        columns_header(3, 4 + 2 + 12),
        IncomingWireError::Limit(PgIncomingLimit::Columns),
        7,
        0,
    )
    .await;

    let fields: Vec<Option<&[u8]>> = vec![None; 1600];
    let wide = data_row(&fields);
    assert_eq!(wide.len(), 6_407);
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(20_000, 0, 20_000, 1600)))
        .expect("set");
    let base = conn.incoming_allocation();
    peer.write(wide.clone());
    let got = recv(&mut conn).await;
    assert_eq!(got.contents().len(), 6_402);
    let meter = conn.incoming_allocation();
    review("columns-1600", &meter);
    assert_eq!(meter.reserved_additional, base.reserved_additional);
    assert_eq!(meter.view_capacity_peak, base.view_capacity_peak);
    assert!(meter
        .events
        .iter()
        .all(|event| event.logical_need != 6_410 && event.reserve_argument != 6_410));
    drop(peer);

    let declared: u32 = 4 + 2 + 1_601 * 4;
    reject_one(
        "columns-1601",
        bounds(20_000, 0, 20_000, 1600),
        columns_header(1_601, declared),
        IncomingWireError::Limit(PgIncomingLimit::Columns),
        7,
        declared as u64,
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn oversized_control_unknown_and_datarow_message_cap() {
    // Finite bodies, declared length just over the cap, and a huge declaration
    // with only a sentinel sent. Cap is checked before the type byte is classified.
    let formats = [b'N', b'E', b'S', b'd', b'A', b'T', b'C', b'Z', b'1', 0x7f];
    for format in formats {
        let mut over = header(format, 65);
        over.extend_from_slice(b"NOLEAK");
        reject_one(
            "control-cap-plus-one",
            bounds(64, 8, 64, 4),
            over,
            IncomingWireError::Limit(PgIncomingLimit::Message),
            5,
            65,
        )
        .await;

        let mut huge = header(format, 2_000_000_000);
        huge.extend_from_slice(b"NOLEAK");
        reject_one(
            "control-huge",
            bounds(64, 8, 64, 4),
            huge,
            IncomingWireError::Limit(PgIncomingLimit::Message),
            5,
            2_000_000_000,
        )
        .await;
    }

    let mut finite = header(0x7f, 10);
    finite.extend_from_slice(b"NOLEAK");
    reject_one(
        "unknown-under-cap",
        bounds(64, 0, 0, 0),
        finite,
        IncomingWireError::BadType,
        5,
        0,
    )
    .await;

    // Header only. The declared body is not sent and must not be read after poison.
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(64, 0, 0, 0)))
        .expect("set");
    let base = conn.incoming_allocation();
    peer.write(header(0x7f, 20));
    let err = recv_err(&mut conn).await;
    assert_static(&err, IncomingWireError::BadType);
    let mid = conn.incoming_allocation();
    assert_eq!(mid.bytes_read - base.bytes_read, 5);
    peer.write(b"NOLEAK".to_vec());
    let again = recv_err(&mut conn).await;
    assert_static(&again, IncomingWireError::BadType);
    assert_eq!(conn.incoming_allocation().bytes_read, mid.bytes_read);
    drop(peer);

    // DataRow over the message cap is rejected at byte 5, before natts.
    reject_one(
        "datarow-message-cap",
        bounds(20, 100, 100, 10),
        header(b'D', 40),
        IncomingWireError::Limit(PgIncomingLimit::Message),
        5,
        40,
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn buffered_frames_accept_first_then_reject_second() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(64, 8, 64, 4)))
        .expect("set");
    let base = conn.incoming_allocation();
    let mut bytes = frame(b'I', b"");
    bytes.extend_from_slice(&frame(b'C', b"SELECT"));
    let mut bad = header(b'N', 2_000_000_000);
    bad.extend_from_slice(b"NOLEAK");
    bytes.extend_from_slice(&bad);
    let sent = bytes.len() as u64;
    peer.write(bytes);
    let first = recv(&mut conn).await;
    assert_eq!(first.format_byte(), b'I');
    assert!(first.contents().is_empty());
    let second = recv(&mut conn).await;
    assert_eq!(second.format_byte(), b'C');
    assert_eq!(second.contents().as_ref(), b"SELECT");
    let after = conn.incoming_allocation();
    assert_eq!(after.bytes_read - base.bytes_read, sent);
    assert_eq!(after.reserved_additional, base.reserved_additional);
    let err = recv_err(&mut conn).await;
    assert_static(&err, IncomingWireError::Limit(PgIncomingLimit::Message));
    let rejected = conn.incoming_allocation();
    assert_eq!(rejected.bytes_read, after.bytes_read);
    assert_eq!(rejected.reads, after.reads);
    assert_no_declared(&rejected, 2_000_000_000);
    peer.write(b"NOLEAK".to_vec());
    let again = recv_err(&mut conn).await;
    assert_static(&again, IncomingWireError::Limit(PgIncomingLimit::Message));
    assert_eq!(conn.incoming_allocation().bytes_read, rejected.bytes_read);
    drop(peer);

    // Accepted row and a following value-cap reject share one read.
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(80, 2, 40, 4)))
        .expect("set");
    let base = conn.incoming_allocation();
    let good = data_row(&[Some(b"OK")]);
    let mut bytes = good.clone();
    bytes.extend_from_slice(&prefix_row(4 + 2 + 4 + 3, 1, &[], 3));
    peer.write(bytes);
    let got = recv(&mut conn).await;
    assert_eq!(got.contents().as_ref(), &good[5..]);
    let err = recv_err(&mut conn).await;
    assert_static(&err, IncomingWireError::Limit(PgIncomingLimit::Value));
    let meter = conn.incoming_allocation();
    review("accept-then-value-reject", &meter);
    assert_eq!(meter.reserved_additional, base.reserved_additional);
    assert_eq!(meter.last_logical_need, 11);
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn bounds_change_during_partial_receive() {
    // Retained header is re-checked under a tighter cap. The body is not sent.
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(50_000, 1_000, 50_000, 8)))
        .expect("set");
    let base = conn.incoming_allocation();
    peer.write(header(b'N', 10_000));
    let hung = tokio::time::timeout(CANCEL_WAIT, conn.recv_backend()).await;
    assert!(hung.is_err(), "header-only recv completed");
    let mid = conn.incoming_allocation();
    assert_eq!(mid.bytes_read - base.bytes_read, 5);
    assert!(conn.incoming_synchronized());
    conn.set_incoming_bounds(Some(bounds(64, 8, 64, 4)))
        .expect("tighten");
    let err = recv_err(&mut conn).await;
    assert_static(&err, IncomingWireError::Limit(PgIncomingLimit::Message));
    let after = conn.incoming_allocation();
    assert_eq!(after.bytes_read, mid.bytes_read);
    assert_eq!(after.reserved_additional, mid.reserved_additional);
    assert!(!conn.incoming_synchronized());
    drop(peer);

    // Enabling bounds on a header that arrived while unset also rejects.
    let (mut conn, peer) = connect_pair().await;
    assert!(conn.incoming_bounds().is_none());
    let base = conn.incoming_allocation();
    peer.write(header(b'C', 9_000));
    let hung = tokio::time::timeout(CANCEL_WAIT, conn.recv_backend()).await;
    assert!(hung.is_err(), "unset header recv completed");
    let mid = conn.incoming_allocation();
    assert_eq!(mid.bytes_read - base.bytes_read, 5);
    conn.set_incoming_bounds(Some(bounds(64, 8, 64, 4)))
        .expect("enable");
    let err = recv_err(&mut conn).await;
    assert_static(&err, IncomingWireError::Limit(PgIncomingLimit::Message));
    assert_eq!(conn.incoming_allocation().bytes_read, mid.bytes_read);
    assert_eq!(
        conn.incoming_allocation().reserved_additional,
        mid.reserved_additional
    );
    drop(peer);

    // A valid row whose header arrived under bounds still completes after disable.
    let row = data_row(&[Some(b"ABCD")]);
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(1_000, 100, 1_000, 4)))
        .expect("set");
    peer.write(row[..5].to_vec());
    let hung = tokio::time::timeout(CANCEL_WAIT, conn.recv_backend()).await;
    assert!(hung.is_err(), "partial valid row completed");
    conn.set_incoming_bounds(None).expect("disable");
    assert!(conn.incoming_bounds().is_none());
    peer.write(row[5..].to_vec());
    let got = recv(&mut conn).await;
    assert_eq!(got.contents().as_ref(), &row[5..]);
    assert!(conn.incoming_synchronized());
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn disable_during_partial_does_not_accept_corrupt_field() {
    // The header is legal under the active cap. The negative field length is
    // sent only after bounds are cleared. A successful frame here would be a
    // corrupt DataRow accepted because the setter changed the parser.
    let header = header(b'D', 20);
    assert_eq!(header.len(), 5);
    let mut rest = Vec::new();
    rest.extend_from_slice(&1u16.to_be_bytes());
    rest.extend_from_slice(&(-2i32).to_be_bytes());
    rest.extend_from_slice(b"EXTRA!!!!X"); // 10 bytes; declared body is 16
    assert_eq!(rest.len(), 16);

    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(1_000, 100, 1_000, 4)))
        .expect("set");
    peer.write(header);
    let hung = tokio::time::timeout(CANCEL_WAIT, conn.recv_backend()).await;
    assert!(hung.is_err(), "partial header completed");
    assert!(conn.incoming_synchronized());
    conn.set_incoming_bounds(None).expect("disable");
    peer.write(rest);
    let result = tokio::time::timeout(HANG, conn.recv_backend())
        .await
        .expect("recv timed out");
    match result {
        Ok(frame) => {
            println!(
                "REVIEW corrupt-after-disable accepted format={} len={} sync={}",
                frame.format_byte(),
                frame.contents().len(),
                conn.incoming_synchronized()
            );
            panic!("corrupt field accepted after disable");
        }
        Err(err) => {
            println!(
                "REVIEW corrupt-after-disable rejected sync={} err={err}",
                conn.incoming_synchronized()
            );
            assert!(
                err.as_incoming_wire().is_some() || !conn.incoming_synchronized(),
                "corrupt field accepted after disable: {err:?} sync={}",
                conn.incoming_synchronized()
            );
            let text = err.to_string();
            assert!(!text.contains("EXTRA"), "{text}");
        }
    }
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn poison_blocks_ping_setter_and_hard_close_flushes() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(16, 4, 16, 2)))
        .expect("set");
    let base = conn.incoming_allocation();
    peer.write(header(b'E', 2_000_000_000));
    let err = recv_err(&mut conn).await;
    assert_static(&err, IncomingWireError::Limit(PgIncomingLimit::Message));
    let stuck = conn.incoming_allocation().bytes_read;
    assert!(stuck >= base.bytes_read);

    let ping = tokio::time::timeout(Duration::from_secs(2), conn.ping())
        .await
        .expect("ping hung");
    assert_static(
        &ping.expect_err("ping"),
        IncomingWireError::Limit(PgIncomingLimit::Message),
    );
    assert_eq!(conn.incoming_allocation().bytes_read, stuck);
    conn.set_incoming_bounds(None).expect("clear");
    assert!(conn.incoming_bounds().is_none());
    assert!(!conn.incoming_synchronized());
    conn.set_incoming_bounds(Some(bounds(100_000, 50_000, 80_000, 1_600)))
        .expect("widen");
    assert!(!conn.incoming_synchronized());
    assert_eq!(conn.incoming_bounds().unwrap().max_message_len(), 100_000);
    let again = recv_err(&mut conn).await;
    assert_static(&again, IncomingWireError::Limit(PgIncomingLimit::Message));
    assert_eq!(conn.incoming_allocation().bytes_read, stuck);

    // ping queued Sync before wait_until_ready returned the fault.
    // close_hard must flush that queued write, then the peer sees EOF.
    let eof = peer.eof();
    conn.close_hard().await.expect("close_hard");
    let got = eof.recv_timeout(Duration::from_secs(5)).expect("eof wait");
    let nbytes = got.expect("peer read");
    // The assertion is the payload, checked below via a dedicated connection.
    // This call proves close_hard returns and the peer observes EOF.
    let _ = nbytes;
    drop(peer);

    let (conn, peer) = poison_sync_connection().await;
    let eof = peer.eof();
    conn.close_hard().await.expect("close_hard");
    let got = eof
        .recv_timeout(Duration::from_secs(5))
        .expect("eof wait")
        .expect("peer read");
    assert_eq!(got, b"S\x00\x00\x00\x04".to_vec(), "queued Sync was not flushed");
    drop(peer);

    let (conn, peer) = {
        let (conn, peer) = connect_pair().await;
        (conn, peer)
    };
    let eof = peer.eof();
    conn.close_hard().await.expect("close_hard");
    let got = eof
        .recv_timeout(Duration::from_secs(5))
        .expect("eof wait")
        .expect("peer read");
    assert_eq!(got, Vec::<u8>::new(), "empty hard close wrote bytes");
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn peer_drop_mid_frame_is_eof_not_panic() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(1_000, 100, 1_000, 4)))
        .expect("set");
    peer.write(header(b'D', 40));
    drop(peer);
    let err = recv_err(&mut conn).await;
    assert!(err.as_incoming_wire().is_none(), "{err:?}");
    let text = err.to_string();
    assert!(!text.contains("NOLEAK"), "{text}");
}

#[tokio::test(flavor = "current_thread")]
async fn cross_connection_poison_does_not_spread() {
    let (mut bad, bad_peer) = connect_pair().await;
    let (mut good, good_peer) = connect_pair().await;
    bad.set_incoming_bounds(Some(bounds(16, 4, 16, 2)))
        .expect("set");
    good.set_incoming_bounds(Some(bounds(16, 4, 16, 2)))
        .expect("set");
    bad_peer.write(header(b'N', 10_000));
    let err = recv_err(&mut bad).await;
    assert_static(&err, IncomingWireError::Limit(PgIncomingLimit::Message));
    assert!(!bad.incoming_synchronized());
    let row = data_row(&[Some(b"OK")]);
    good_peer.write(row.clone());
    let got = recv(&mut good).await;
    assert_eq!(got.contents().as_ref(), &row[5..]);
    assert!(good.incoming_synchronized());
    drop(bad_peer);
    drop(good_peer);
}

#[tokio::test(flavor = "current_thread")]
async fn default_unset_accepts_and_short_length_poisons() {
    let row = data_row(&[Some(b"ABCDEFGH")]);
    let (mut conn, peer) = connect_pair().await;
    assert!(conn.incoming_bounds().is_none());
    peer.write(row.clone());
    let got = recv(&mut conn).await;
    assert_eq!(got.contents().as_ref(), &row[5..]);
    peer.write(frame(b'I', b""));
    let next = recv(&mut conn).await;
    assert_eq!(next.format_byte(), b'I');
    assert!(conn.incoming_synchronized());
    let meter = conn.incoming_allocation();
    review("default-two-frames", &meter);
    assert_eq!(meter.assembly_bytes, 0);
    drop(peer);

    let (mut conn, peer) = connect_pair().await;
    peer.write(header(b'I', 2));
    let err = recv_err(&mut conn).await;
    assert_static(&err, IncomingWireError::ShortLength);
    assert!(!conn.incoming_synchronized());
    conn.set_incoming_bounds(Some(bounds(1_000, 100, 1_000, 4)))
        .expect("set");
    assert!(!conn.incoming_synchronized());
    let again = recv_err(&mut conn).await;
    assert_static(&again, IncomingWireError::ShortLength);
    drop(peer);

    let (mut conn, peer) = connect_pair().await;
    let mut bytes = header(0x7f, 8);
    bytes.extend_from_slice(b"NOLEAK");
    peer.write(bytes);
    let err = recv_err(&mut conn).await;
    assert!(err.as_incoming_wire().is_none(), "{err:?}");
    let text = format!("{err} {err:?}");
    println!(
        "REVIEW unset-unknown sync={} text={text}",
        conn.incoming_synchronized()
    );
    assert!(!text.contains("NOLEAK"), "{text}");
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn accounting_noop_reserve_offset_and_zero_assembly() {
    // Header is absorbed first, so the following read asks for a deficit that
    // is larger than the spare and at most twice the spare. BytesMut::reserve
    // treats that argument as already satisfied. The meter still adds it.
    let frame_bytes = sized_frame(b'C', 12_000);
    assert_eq!(frame_bytes.len(), 12_000);
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(100_000, 100_000, 100_000, 8)))
        .expect("set");
    let base = conn.incoming_allocation();
    peer.write(frame_bytes[..5].to_vec());
    let hung = tokio::time::timeout(CANCEL_WAIT, conn.recv_backend()).await;
    assert!(hung.is_err(), "5-byte header completed the 12000-byte frame");
    let mid = conn.incoming_allocation();
    assert_eq!(mid.bytes_read - base.bytes_read, 5);
    // The full-frame reserve runs before the read parks. A no-op reserve is
    // counted here and is not yet an event, because the event is pushed after
    // the socket read returns.
    println!(
        "REVIEW noop-mid reserved_delta={} reserve_calls_delta={} view_peak={} events={}",
        mid.reserved_additional - base.reserved_additional,
        mid.reserve_calls - base.reserve_calls,
        mid.view_capacity_peak,
        mid.events.len()
    );
    peer.write(frame_bytes[5..].to_vec());
    let got = recv(&mut conn).await;
    assert_eq!(got.format_byte(), b'C');
    assert_eq!(got.contents().len(), 11_995);
    let meter = conn.incoming_allocation();
    let fresh = &meter.events[mid.events.len()..];
    dump_events("noop-reserve", fresh);
    let growth = capacity_growth(fresh);
    let noop = noop_reserve(fresh);
    let reserved = meter.reserved_additional - mid.reserved_additional;
    println!("REVIEW noop reserved={reserved} growth={growth} noop={noop} rounding={} assembly={}",
        meter.rounding_peak, meter.assembly_bytes);
    assert!(
        noop > 0,
        "no reserve argument left capacity unchanged"
    );
    assert!(
        reserved > growth,
        "reserved_additional {reserved} was not greater than capacity growth {growth}"
    );
    assert_eq!(meter.assembly_bytes, 0);
    drop(peer);

    // One-shot row larger than the 8192 spare. The second read reserves from a
    // zero-capacity available view. That view is not contiguous with the bytes
    // already in `read`, so advance copies. assembly_bytes stays 0.
    let payload = vec![b'Q'; 20_000];
    let row = data_row(&[Some(payload.as_slice())]);
    assert_eq!(row.len(), 20_011);
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(100_000, 50_000, 80_000, 4)))
        .expect("set");
    let base = conn.incoming_allocation();
    let rss_before = vm_rss_kb();
    let peak = Arc::new(AtomicU64::new(rss_before.unwrap_or(0)));
    let stop = Arc::new(AtomicBool::new(false));
    let sampler = {
        let peak = Arc::clone(&peak);
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                if let Some(rss) = vm_rss_kb() {
                    peak.fetch_max(rss, Ordering::Relaxed);
                }
                thread::sleep(Duration::from_millis(1));
            }
        })
    };
    peer.write(row.clone());
    let got = recv(&mut conn).await;
    stop.store(true, Ordering::Relaxed);
    let _ = sampler.join();
    let rss_after = vm_rss_kb();
    assert_eq!(got.contents().len(), 20_006);
    // Hold the frame across the measurement so its window stays alive.
    let meter = conn.incoming_allocation();
    let fresh = &meter.events[base.events.len()..];
    dump_events("one-shot-20kb", fresh);
    review("one-shot-20kb", &meter);
    let from_zero = fresh.iter().find(|event| {
        event.reserve_argument > 0 && event.available_capacity_before == 0
    });
    let grown = from_zero.expect("reserve from an empty available view");
    println!(
        "REVIEW oneshot reserve={} cap_before={} cap_after={} view_after={} rounding={} assembly={} rss_before={rss_before:?} rss_peak={} rss_after={rss_after:?}",
        grown.reserve_argument,
        grown.available_capacity_before,
        grown.available_capacity_after,
        grown.view_capacity_after,
        meter.rounding_peak,
        meter.assembly_bytes,
        peak.load(Ordering::Relaxed),
    );
    assert_eq!(meter.assembly_bytes, 0);
    assert_eq!(meter.logical_requested_peak, row.len() as u64);
    assert_ne!(meter.view_capacity_peak, 100_000);
    assert_ne!(meter.view_capacity_peak, 50_000);
    assert_ne!(meter.view_capacity_peak, 80_000);
    assert!(grown.view_capacity_after >= row.len() as u64);
    if let Some(first) = fresh.first() {
        println!(
            "REVIEW offset view_peak_before={} available_before={} bytes_read_before={} gap={}",
            base.view_capacity_peak,
            first.available_capacity_before,
            base.bytes_read,
            base.view_capacity_peak
                .saturating_sub(first.available_capacity_before)
        );
        assert_eq!(first.reserve_argument, 0);
        assert_eq!(
            base.view_capacity_peak - first.available_capacity_before,
            base.bytes_read,
            "post-handshake window does not drop by the consumed handshake"
        );
    }
    drop(got);
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn event_log_is_not_the_frame_buffer() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(64, 8, 64, 4)))
        .expect("set");
    let base = conn.incoming_allocation();
    let mut held = Vec::new();
    for _ in 0..24 {
        let frame = frame(b'C', b"OK");
        peer.write(frame.clone());
        held.push(recv(&mut conn).await);
    }
    let meter = conn.incoming_allocation();
    review("event-log", &meter);
    let event_size = std::mem::size_of::<PgIncomingAllocEvent>();
    assert_eq!(meter.trace_bytes % event_size as u64, 0);
    assert!(meter.trace_bytes > base.trace_bytes);
    assert_eq!(meter.view_capacity_peak, base.view_capacity_peak);
    assert!(meter.accepted_logical_payload > base.accepted_logical_payload);
    assert!(meter.accepted_backing_peak <= meter.view_capacity_peak);
    assert_eq!(meter.accepted_backing_peak, base.accepted_backing_peak);
    assert_eq!(meter.assembly_bytes, 0);
    assert_eq!(held.len(), 24);
    drop(peer);
}

async fn poison_sync_connection() -> (PgConnection, Peer) {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(16, 4, 16, 2)))
        .expect("set");
    peer.write(header(b'E', 80));
    let err = recv_err(&mut conn).await;
    assert_static(&err, IncomingWireError::Limit(PgIncomingLimit::Message));
    let ping = tokio::time::timeout(Duration::from_secs(2), conn.ping())
        .await
        .expect("ping hung");
    assert!(ping.is_err());
    (conn, peer)
}

async fn cancel_resume(limits: Option<PgIncomingBounds>, frame_bytes: &[u8], stop: usize, rounds: usize) {
    assert!(stop < frame_bytes.len());
    let (mut conn, peer) = connect_pair().await;
    if let Some(limits) = limits {
        conn.set_incoming_bounds(Some(limits)).expect("set");
    }
    let base = conn.incoming_allocation();
    let mut sent = 0usize;
    for round in 0..rounds {
        let next = stop + round * ((frame_bytes.len() - stop) / rounds).max(1);
        let next = next.min(frame_bytes.len() - 1);
        if next <= sent {
            continue;
        }
        peer.write(frame_bytes[sent..next].to_vec());
        let hung = tokio::time::timeout(CANCEL_WAIT, conn.recv_backend()).await;
        assert!(hung.is_err(), "recv completed at {next} of {}", frame_bytes.len());
        sent = next;
        let mid = conn.incoming_allocation();
        assert_eq!(mid.bytes_read - base.bytes_read, sent as u64);
        assert!(conn.incoming_synchronized());
    }
    let mut rest = frame_bytes[sent..].to_vec();
    let follow = frame(b'I', b"");
    rest.extend_from_slice(&follow);
    peer.write(rest);
    let got = recv(&mut conn).await;
    assert_eq!(got.format_byte(), frame_bytes[0]);
    assert_eq!(got.contents().as_ref(), &frame_bytes[5..]);
    let next = recv(&mut conn).await;
    assert_eq!(next.format_byte(), b'I');
    assert!(next.contents().is_empty());
    assert!(conn.incoming_synchronized());
    drop(peer);
}

async fn reject_one(
    case: &str,
    limits: PgIncomingBounds,
    bytes: Vec<u8>,
    expected: IncomingWireError,
    decision: u64,
    declared: u64,
) {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(limits)).expect("set");
    let base = conn.incoming_allocation();
    let sent = bytes.len() as u64;
    peer.write(bytes);
    let err = recv_err(&mut conn).await;
    assert_static(&err, expected);
    let meter = conn.incoming_allocation();
    review(case, &meter);
    assert_eq!(meter.bytes_read - base.bytes_read, sent);
    assert_eq!(meter.reserved_additional, base.reserved_additional);
    assert_eq!(meter.view_capacity_peak, base.view_capacity_peak);
    assert_eq!(meter.last_logical_need, decision);
    assert_eq!(meter.assembly_bytes, 0);
    if declared > 0 {
        assert_no_declared(&meter, declared);
    }
    assert!(!conn.incoming_synchronized());
    let again = recv_err(&mut conn).await;
    assert_static(&again, expected);
    assert_eq!(conn.incoming_allocation().bytes_read, meter.bytes_read);
    drop(peer);
}

fn assert_no_declared(meter: &PgIncomingAllocation, declared: u64) {
    assert!(meter.logical_requested_peak < declared);
    assert!(meter.reserved_additional < declared);
    assert_ne!(meter.view_capacity_peak, declared);
    assert!(meter.last_logical_need < declared);
    assert!(meter.events.iter().all(|event| {
        event.logical_need < declared && event.reserve_argument < declared
    }));
}

fn assert_static(err: &sqlx::Error, expected: IncomingWireError) {
    assert_eq!(err.as_incoming_wire(), Some(expected));
    let text = err.to_string();
    let debug = format!("{err:?}");
    for leak in LEAK {
        assert!(!text.contains(leak), "{text}");
        assert!(!debug.contains(leak), "{debug}");
    }
    assert!(!text.chars().any(|ch| ch.is_ascii_digit()), "{text}");
    assert!(!debug.chars().any(|ch| ch.is_ascii_digit()), "{debug}");
}

fn assert_invalid(err: sqlx::Error) {
    match err {
        sqlx::Error::InvalidArgument(text) => {
            assert_eq!(text, "incoming bounds are inconsistent");
        }
        other => panic!("unexpected {other:?}"),
    }
}

fn review(case: &str, meter: &PgIncomingAllocation) {
    println!(
        "REVIEW case={case} logical_peak={} reserved={} view_peak={} reads={} bytes_read={} assembly={} accepted_logical={} accepted_backing={} overread={} rounding={} last={} reserve_calls={} trace_bytes={} messages={}",
        meter.logical_requested_peak,
        meter.reserved_additional,
        meter.view_capacity_peak,
        meter.reads,
        meter.bytes_read,
        meter.assembly_bytes,
        meter.accepted_logical_payload,
        meter.accepted_backing_peak,
        meter.overread_bytes,
        meter.rounding_peak,
        meter.last_logical_need,
        meter.reserve_calls,
        meter.trace_bytes,
        meter.messages,
    );
}

fn dump_events(case: &str, events: &[PgIncomingAllocEvent]) {
    for (index, event) in events.iter().enumerate() {
        println!(
            "REVIEW event case={case} i={index} logical={} reserve={} read_cap_before={} avail_cap_before={} avail_len_before={} avail_cap_after={} view_after={} read_len_before={} returned={}",
            event.logical_need,
            event.reserve_argument,
            event.read_capacity_before,
            event.available_capacity_before,
            event.available_len_before,
            event.available_capacity_after,
            event.view_capacity_after,
            event.read_len_before,
            event.bytes_returned,
        );
    }
}

fn capacity_growth(events: &[PgIncomingAllocEvent]) -> u64 {
    events
        .iter()
        .map(|event| {
            event
                .available_capacity_after
                .saturating_sub(event.available_capacity_before)
        })
        .sum()
}

fn noop_reserve(events: &[PgIncomingAllocEvent]) -> u64 {
    events
        .iter()
        .filter(|event| {
            event.reserve_argument > 0
                && event.available_capacity_after == event.available_capacity_before
        })
        .map(|event| event.reserve_argument)
        .sum()
}

fn vm_rss_kb() -> Option<u64> {
    let text = fs::read_to_string("/proc/self/status").ok()?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            return rest.split_whitespace().next()?.parse().ok();
        }
    }
    None
}

fn bounds(message: u32, value: u32, row: u32, columns: u16) -> PgIncomingBounds {
    PgIncomingBounds::try_new(message, value, row, columns).expect("bounds")
}

fn frame(format: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + body.len());
    out.push(format);
    let message_len = u32::try_from(body.len() + 4).expect("message len");
    out.extend_from_slice(&message_len.to_be_bytes());
    out.extend_from_slice(body);
    out
}

fn sized_frame(format: u8, frame_len: usize) -> Vec<u8> {
    assert!(frame_len >= 5);
    let message_len = u32::try_from(frame_len - 1).expect("message len");
    let mut out = header(format, message_len);
    out.resize(frame_len, b'A');
    out
}

fn header(format: u8, message_len: u32) -> Vec<u8> {
    let mut out = vec![format];
    out.extend_from_slice(&message_len.to_be_bytes());
    out
}

fn columns_header(natts: u16, message_len: u32) -> Vec<u8> {
    let mut out = header(b'D', message_len);
    out.extend_from_slice(&natts.to_be_bytes());
    out
}

fn data_row(fields: &[Option<&[u8]>]) -> Vec<u8> {
    let mut body = Vec::new();
    let natts = u16::try_from(fields.len()).expect("natts");
    body.extend_from_slice(&natts.to_be_bytes());
    for field in fields {
        match field {
            None => body.extend_from_slice(&(-1i32).to_be_bytes()),
            Some(payload) => {
                let len = i32::try_from(payload.len()).expect("field len");
                body.extend_from_slice(&len.to_be_bytes());
                body.extend_from_slice(payload);
            }
        }
    }
    frame(b'D', &body)
}

fn prefix_row(message_len: u32, natts: u16, done: &[Option<&[u8]>], stop_flen: i32) -> Vec<u8> {
    let mut out = columns_header(natts, message_len);
    for field in done {
        match field {
            None => out.extend_from_slice(&(-1i32).to_be_bytes()),
            Some(payload) => {
                let len = i32::try_from(payload.len()).expect("field len");
                out.extend_from_slice(&len.to_be_bytes());
                out.extend_from_slice(payload);
            }
        }
    }
    out.extend_from_slice(&stop_flen.to_be_bytes());
    out
}

enum Cmd {
    Write(Vec<u8>, Sender<()>),
    Eof(Sender<std::io::Result<Vec<u8>>>),
}

struct Peer {
    port: u16,
    tx: Option<Sender<Cmd>>,
    join: Option<JoinHandle<()>>,
}

impl Peer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("local addr").port();
        let (tx, rx) = mpsc::channel();
        let join = thread::spawn(move || peer_loop(listener, rx));
        Self {
            port,
            tx: Some(tx),
            join: Some(join),
        }
    }

    fn write(&self, bytes: Vec<u8>) {
        let (ack, wait) = mpsc::channel();
        self.tx
            .as_ref()
            .expect("peer")
            .send(Cmd::Write(bytes, ack))
            .expect("peer command");
        wait.recv_timeout(Duration::from_secs(5))
            .expect("peer write");
    }

    fn eof(&self) -> Receiver<std::io::Result<Vec<u8>>> {
        let (ack, wait) = mpsc::channel();
        self.tx
            .as_ref()
            .expect("peer")
            .send(Cmd::Eof(ack))
            .expect("peer eof");
        wait
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn peer_loop(listener: TcpListener, rx: Receiver<Cmd>) {
    let Ok((mut stream, _)) = listener.accept() else {
        return;
    };
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(8)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(8)));
    if read_startup(&mut stream).is_err() {
        return;
    }
    let mut hello = Vec::with_capacity(15);
    hello.push(b'R');
    hello.extend_from_slice(&8u32.to_be_bytes());
    hello.extend_from_slice(&0u32.to_be_bytes());
    hello.push(b'Z');
    hello.extend_from_slice(&5u32.to_be_bytes());
    hello.push(b'I');
    if stream.write_all(&hello).is_err() || stream.flush().is_err() {
        return;
    }
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Write(bytes, ack) => {
                if stream.write_all(&bytes).is_err() || stream.flush().is_err() {
                    return;
                }
                let _ = ack.send(());
            }
            Cmd::Eof(ack) => {
                let _ = ack.send(read_to_eof(&mut stream));
                return;
            }
        }
    }
}

fn read_startup(stream: &mut TcpStream) -> std::io::Result<()> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let mut len = u32::from_be_bytes(len_buf);
    if len == 8 {
        let mut rest = [0u8; 4];
        stream.read_exact(&mut rest)?;
        let mut hdr = [0u8; 8];
        hdr[..4].copy_from_slice(&len_buf);
        hdr[4..].copy_from_slice(&rest);
        if &hdr == b"\x00\x00\x00\x08\x04\xd2\x16\x2f" {
            stream.write_all(b"N")?;
            stream.flush()?;
            stream.read_exact(&mut len_buf)?;
            len = u32::from_be_bytes(len_buf);
        } else {
            return Err(std::io::Error::other("short startup"));
        }
    }
    if !(4..=65_536).contains(&len) {
        return Err(std::io::Error::other("startup length rejected"));
    }
    let mut body = vec![0u8; (len as usize) - 4];
    stream.read_exact(&mut body)?;
    Ok(())
}

fn read_to_eof(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf = [0u8; 256];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => return Ok(out),
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err)
                if err.kind() == std::io::ErrorKind::ConnectionReset
                    || err.kind() == std::io::ErrorKind::UnexpectedEof
                    || err.kind() == std::io::ErrorKind::TimedOut =>
            {
                return Ok(out);
            }
            Err(err) => return Err(err),
        }
    }
}

async fn connect_pair() -> (PgConnection, Peer) {
    let peer = Peer::start();
    let options = PgConnectOptions::from_explicit()
        .host("127.0.0.1")
        .port(peer.port)
        .username("review")
        .database("review")
        .ssl_mode(PgSslMode::Disable);
    let conn = tokio::time::timeout(HANG, PgConnection::connect_with(&options))
        .await
        .expect("connect timed out")
        .expect("connect");
    (conn, peer)
}

async fn recv(conn: &mut PgConnection) -> PgBackendFrame {
    tokio::time::timeout(HANG, conn.recv_backend())
        .await
        .expect("recv timed out")
        .expect("recv")
}

async fn recv_err(conn: &mut PgConnection) -> sqlx::Error {
    tokio::time::timeout(HANG, conn.recv_backend())
        .await
        .expect("recv timed out")
        .expect_err("frame was accepted")
}
