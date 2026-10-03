//! Scripted PostgreSQL backend for the opt-in receive bounds.
//!
//! The peer sends length prefixes and finite bodies. It never allocates a declared huge payload.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use sqlx::postgres::{
    IncomingWireError, PgBackendFrame, PgConnectOptions, PgConnection, PgIncomingAllocation,
    PgIncomingBounds, PgIncomingLimit, PgSslMode,
};
use sqlx::Connection;

const HANG: Duration = Duration::from_secs(10);
const CANCEL_WAIT: Duration = Duration::from_millis(1000);
const SECRET: &[u8] = b"SECRET";

#[test]
fn bounds_reject_inconsistent() {
    let low = PgIncomingBounds::try_new(3, 0, 0, 0).expect_err("message cap");
    assert_invalid(low);
    let value = PgIncomingBounds::try_new(4, 2, 1, 0).expect_err("value above row");
    assert_invalid(value);
    let zero = PgIncomingBounds::try_new(4, 0, 0, 0).expect("zero row and columns");
    assert_eq!(zero.max_message_len(), 4);
    assert_eq!(zero.max_value_len(), 0);
    assert_eq!(zero.max_row_payload(), 0);
    assert_eq!(zero.max_columns(), 0);
    let wide = PgIncomingBounds::try_new(u32::MAX, u32::MAX, u32::MAX, 1).expect("equal caps");
    assert_eq!(wide.max_columns(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn default_off_accepts_row_over_tight_cap() {
    let row = data_row(&[Some(&b"ABCDE"[..])]);
    let (mut open, peer) = connect_pair().await;
    assert!(open.incoming_bounds().is_none());
    let base = open.incoming_allocation();
    peer.write(row.clone());
    let frame = recv(&mut open).await;
    assert_eq!(frame.format_byte(), b'D');
    assert_eq!(frame.contents().as_ref(), &row[5..]);
    let meter = open.incoming_allocation();
    trace("default-off-accept", &meter);
    assert!(open.incoming_synchronized());
    assert!(meter.messages > base.messages);
    assert_eq!(
        meter.accepted_logical_payload - base.accepted_logical_payload,
        (row.len() - 5) as u64
    );
    assert!(meter.logical_requested_peak >= row.len() as u64);
    assert!(meter.view_capacity_peak >= 8192);
    assert_eq!(meter.assembly_bytes, 0);
    assert!(meter.trace_bytes < 1_000_000);
    assert_ne!(meter.view_capacity_peak, meter.logical_requested_peak);
    drop(peer);
    drop(open);

    let (mut limited, peer) = connect_pair().await;
    limited
        .set_incoming_bounds(Some(bounds(1_000, 4, 100, 4)))
        .expect("set");
    let base = limited.incoming_allocation();
    peer.write(header_and(b'D', 4 + 2 + 4 + 5, &{
        let mut prefix = Vec::new();
        prefix.extend_from_slice(&1u16.to_be_bytes());
        prefix.extend_from_slice(&5i32.to_be_bytes());
        prefix
    }));
    let err = recv_err(&mut limited).await;
    assert_wire(
        &err,
        IncomingWireError::Limit(PgIncomingLimit::Value),
    );
    let meter = limited.incoming_allocation();
    trace("default-off-counterpart-value-reject", &meter);
    assert_small_reject(&base, &meter, 11, 11);
    assert!(!limited.incoming_synchronized());
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn default_off_short_length_does_not_panic() {
    let (mut conn, peer) = connect_pair().await;
    let base = conn.incoming_allocation();
    peer.write(header(b'I', 3));
    let err = recv_err(&mut conn).await;
    assert_wire(&err, IncomingWireError::ShortLength);
    let mid = conn.incoming_allocation();
    trace("default-off-short-length", &mid);
    assert_small_reject(&base, &mid, 5, 5);
    assert!(!conn.incoming_synchronized());
    let again = recv_err(&mut conn).await;
    assert_wire(&again, IncomingWireError::ShortLength);
    assert_eq!(conn.incoming_allocation().bytes_read, mid.bytes_read);
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn message_cap_exact_and_plus_one() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(4, 0, 0, 0)))
        .expect("set");
    peer.write(frame(b'I', b""));
    let got = recv(&mut conn).await;
    assert_eq!(got.format_byte(), b'I');
    assert!(got.contents().is_empty());
    assert!(conn.incoming_synchronized());
    trace("message-exact", &conn.incoming_allocation());
    drop(peer);

    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(4, 0, 0, 0)))
        .expect("set");
    let base = conn.incoming_allocation();
    let mut bytes = header(b'I', 5);
    assert_eq!(bytes.len(), 5);
    peer.write(bytes.split_off(0));
    let err = recv_err(&mut conn).await;
    assert_wire(&err, IncomingWireError::Limit(PgIncomingLimit::Message));
    let meter = conn.incoming_allocation();
    trace("message-plus-one", &meter);
    assert_small_reject(&base, &meter, 5, 5);
    assert_ne!(meter.logical_requested_peak, 6);
    assert_ne!(meter.view_capacity_peak, 4);
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn value_cap_exact_and_plus_one() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(100, 4, 100, 4)))
        .expect("set");
    let row = data_row(&[Some(&b"ABCD"[..])]);
    assert_eq!(row.len(), 15);
    peer.write(row.clone());
    let got = recv(&mut conn).await;
    assert_eq!(got.contents().as_ref(), &row[5..]);
    assert!(conn.incoming_synchronized());
    trace("value-exact", &conn.incoming_allocation());
    drop(peer);

    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(100, 4, 100, 4)))
        .expect("set");
    let base = conn.incoming_allocation();
    peer.write(prefix_row(4 + 2 + 4 + 5, 1, &[], 5));
    let err = recv_err(&mut conn).await;
    assert_wire(&err, IncomingWireError::Limit(PgIncomingLimit::Value));
    let meter = conn.incoming_allocation();
    trace("value-plus-one", &meter);
    assert_small_reject(&base, &meter, 11, 11);
    assert!(meter.events.iter().all(|event| event.reserve_argument < 5));
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn row_cap_exact_and_plus_one() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(100, 2, 14, 2)))
        .expect("set");
    let row = data_row(&[Some(&b"AB"[..]), Some(&b"CD"[..])]);
    peer.write(row.clone());
    let got = recv(&mut conn).await;
    assert_eq!(got.format_byte(), b'D');
    assert_eq!(got.contents().as_ref(), &row[5..]);
    trace("row-exact", &conn.incoming_allocation());
    drop(peer);

    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(100, 4, 14, 2)))
        .expect("set");
    let base = conn.incoming_allocation();
    // prefix 10 + first payload 2 + second payload 3 = 15, one past the row cap.
    // The second body is not sent.
    peer.write(prefix_row(
        4 + 2 + 4 + 2 + 4 + 3,
        2,
        &[Some(&b"AB"[..])],
        3,
    ));
    let err = recv_err(&mut conn).await;
    assert_wire(&err, IncomingWireError::Limit(PgIncomingLimit::Row));
    let meter = conn.incoming_allocation();
    trace("row-plus-one", &meter);
    assert_small_reject(&base, &meter, 17, 17);
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn columns_1600_and_1601() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(20_000, 0, 20_000, 1600)))
        .expect("set");
    let fields: Vec<Option<&[u8]>> = vec![None; 1600];
    let row = data_row(&fields);
    assert_eq!(row.len(), 6407);
    peer.write(row.clone());
    let got = recv(&mut conn).await;
    assert_eq!(got.format_byte(), b'D');
    assert_eq!(got.contents().len(), 6402);
    assert_eq!(got.contents().as_ref(), &row[5..]);
    trace("columns-1600", &conn.incoming_allocation());
    drop(peer);

    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(20_000, 0, 20_000, 1600)))
        .expect("set");
    let base = conn.incoming_allocation();
    let declared = 4 + 2 + 1601 * 4;
    peer.write(columns_header(1601, declared));
    let err = recv_err(&mut conn).await;
    assert_wire(
        &err,
        IncomingWireError::Limit(PgIncomingLimit::Columns),
    );
    let meter = conn.incoming_allocation();
    trace("columns-1601", &meter);
    assert_small_reject(&base, &meter, 7, 7);
    assert_no_declared(&meter, u64::from(declared));
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn null_zero_negative_truncated_remaining_overflow() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(100, 8, 80, 8)))
        .expect("set");
    let row = data_row(&[None, Some(b""), Some(&b"Z"[..])]);
    peer.write(row.clone());
    let got = recv(&mut conn).await;
    assert_eq!(got.contents().as_ref(), &row[5..]);
    assert!(conn.incoming_synchronized());
    trace("null-zero-short", &conn.incoming_allocation());
    drop(peer);

    reject_case(
        "negative-length",
        bounds(100, 8, 80, 4),
        prefix_row(10, 1, &[], -2),
        11,
        11,
        IncomingWireError::NegativeLength,
    )
    .await;
    reject_case(
        "truncated-row",
        bounds(100, 8, 80, 4),
        columns_header(1, 6),
        7,
        7,
        IncomingWireError::Truncated,
    )
    .await;
    reject_case(
        "remaining-body",
        bounds(100, 8, 80, 4),
        columns_header(0, 7),
        7,
        7,
        IncomingWireError::Remaining,
    )
    .await;
    reject_case(
        "length-overflow",
        bounds(u32::MAX, u32::MAX, u32::MAX, 1),
        header(b'D', u32::MAX),
        5,
        5,
        IncomingWireError::Overflow,
    )
    .await;

    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(u32::MAX, u32::MAX, u32::MAX, 1)))
        .expect("set");
    let base = conn.incoming_allocation();
    // 6 + i32::MAX fits in u32. The declared frame cannot hold that payload.
    peer.write(prefix_row(10, 1, &[], i32::MAX));
    let err = recv_err(&mut conn).await;
    assert_wire(&err, IncomingWireError::Truncated);
    let meter = conn.incoming_allocation();
    trace("huge-field-truncated", &meter);
    assert_small_reject(&base, &meter, 11, 11);
    assert!(meter.view_capacity_peak < 1_000_000);
    assert_ne!(
        meter.logical_requested_peak,
        u64::try_from(i32::MAX).expect("i32 max")
    );
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn control_messages_do_not_leak() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(64, 8, 80, 4)))
        .expect("set");
    peer.write(frame(b'N', b"hi"));
    let notice = recv(&mut conn).await;
    assert_eq!(notice.format_byte(), b'N');
    assert_eq!(notice.contents().as_ref(), b"hi");
    peer.write(frame(b'S', b"a\0b\0"));
    let status = recv(&mut conn).await;
    assert_eq!(status.format_byte(), b'S');
    assert_eq!(status.contents().as_ref(), b"a\0b\0");
    peer.write(frame(b'E', b"x"));
    let error = recv(&mut conn).await;
    assert_eq!(error.format_byte(), b'E');
    assert_eq!(error.contents().as_ref(), b"x");
    assert!(conn.incoming_synchronized());
    trace("control-under-cap", &conn.incoming_allocation());
    drop(peer);

    for format in [b'N', b'E', b'S', b'D', 0xff] {
        let (mut conn, peer) = connect_pair().await;
        conn.set_incoming_bounds(Some(bounds(64, 8, 80, 4)))
            .expect("set");
        let base = conn.incoming_allocation();
        let mut bytes = header(format, 2_000_000_000);
        bytes.extend_from_slice(SECRET);
        peer.write(bytes);
        let err = recv_err(&mut conn).await;
        assert_wire(&err, IncomingWireError::Limit(PgIncomingLimit::Message));
        let meter = conn.incoming_allocation();
        trace(&format!("control-huge-{format}"), &meter);
        assert_small_reject(&base, &meter, 11, 5);
        assert_no_declared(&meter, 2_000_000_000);
        let stuck = meter.bytes_read;
        peer.write(b"MORESECRET".to_vec());
        let again = recv_err(&mut conn).await;
        assert_wire(&again, IncomingWireError::Limit(PgIncomingLimit::Message));
        assert_eq!(conn.incoming_allocation().bytes_read, stuck);
        drop(peer);
    }

    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(64, 0, 0, 0)))
        .expect("set");
    let mut bytes = header(0xff, 4);
    bytes.extend_from_slice(SECRET);
    peer.write(bytes);
    let err = recv_err(&mut conn).await;
    assert_wire(&err, IncomingWireError::BadType);
    trace("bad-type-small", &conn.incoming_allocation());
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn overread_then_reject_does_not_allocate_or_drain() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(64, 8, 80, 4)))
        .expect("set");
    let base = conn.incoming_allocation();
    let mut bytes = frame(b'I', b"");
    bytes.extend_from_slice(&header(b'N', 2_000_000_000));
    bytes.extend_from_slice(SECRET);
    let sent = bytes.len() as u64;
    peer.write(bytes);
    let first = recv(&mut conn).await;
    assert_eq!(first.format_byte(), b'I');
    assert!(first.contents().is_empty());
    let after_accept = conn.incoming_allocation();
    assert!(after_accept.overread_bytes > base.overread_bytes);
    assert_eq!(after_accept.bytes_read - base.bytes_read, sent);
    assert_eq!(after_accept.reserved_additional, base.reserved_additional);
    let err = recv_err(&mut conn).await;
    assert_wire(&err, IncomingWireError::Limit(PgIncomingLimit::Message));
    let rejected = conn.incoming_allocation();
    trace("overread-then-reject", &rejected);
    assert_eq!(rejected.bytes_read, after_accept.bytes_read);
    assert_eq!(rejected.reads, after_accept.reads);
    assert_no_declared(&rejected, 2_000_000_000);
    assert_ne!(rejected.view_capacity_peak, 64);
    assert_ne!(rejected.view_capacity_peak, 2_000_000_000);

    peer.write(b"MORESECRET".to_vec());
    let third = recv_err(&mut conn).await;
    assert_wire(&third, IncomingWireError::Limit(PgIncomingLimit::Message));
    assert_eq!(conn.incoming_allocation().bytes_read, rejected.bytes_read);
    let ping = tokio::time::timeout(Duration::from_secs(2), conn.ping())
        .await
        .expect("ping hung");
    assert_wire(
        &ping.expect_err("ping"),
        IncomingWireError::Limit(PgIncomingLimit::Message),
    );
    assert_eq!(conn.incoming_allocation().bytes_read, rejected.bytes_read);
    conn.set_incoming_bounds(None).expect("clear");
    assert!(!conn.incoming_synchronized());
    conn.set_incoming_bounds(Some(bounds(100_000, 50_000, 80_000, 1600)))
        .expect("widen");
    assert!(!conn.incoming_synchronized());
    assert!(conn.incoming_bounds().is_some());
    assert_eq!(conn.incoming_allocation().bytes_read, rejected.bytes_read);
    let eof = peer.eof();
    conn.close_hard().await.expect("close_hard");
    eof.recv_timeout(Duration::from_secs(5))
        .expect("eof wait")
        .expect("peer saw disconnect");
}

#[tokio::test(flavor = "current_thread")]
async fn allocator_rounding_and_retained_storage() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(100_000, 50_000, 80_000, 4)))
        .expect("set");
    let base = conn.incoming_allocation();
    let payload = vec![b'Q'; 20_000];
    let row = data_row(&[Some(payload.as_slice())]);
    assert_eq!(row.len(), 20_011);
    peer.write(row.clone());
    let got = recv(&mut conn).await;
    assert_eq!(got.contents().len(), 20_006);
    let meter = conn.incoming_allocation();
    trace("rounding-20kb", &meter);
    assert_eq!(meter.logical_requested_peak, 20_011);
    assert!(meter.view_capacity_peak > 8192);
    assert_ne!(meter.view_capacity_peak, 100_000);
    assert_ne!(meter.view_capacity_peak, 50_000);
    assert_ne!(meter.view_capacity_peak, 80_000);
    assert!(meter.reserved_additional > base.reserved_additional);
    assert!(meter.overread_bytes > base.overread_bytes);
    // `assembly_bytes` is bytes copied while joining the windows, not a second
    // claim that the frame was not copied. The sum of the event rows is the
    // lifetime counter when the diagnostic log has not dropped a row.
    assert_eq!(meter.events_dropped, 0);
    assert_eq!(
        meter.assembly_bytes,
        meter
            .events
            .iter()
            .map(|event| event.assembly_bytes)
            .sum::<u64>()
    );
    assert!(meter.assembly_bytes <= meter.bytes_read);
    assert!(meter.trace_bytes < 1_000_000);
    assert_eq!(
        meter.accepted_logical_payload - base.accepted_logical_payload,
        20_006
    );
    assert!(meter.accepted_backing_peak >= 8_192);
    assert_ne!(meter.accepted_backing_peak, 100_000);
    let fresh = &meter.events[base.events.len()..];
    let grown = fresh
        .iter()
        .find(|event| event.reserved_growth > 0)
        .expect("body reserve");
    assert!(grown.available_capacity_after > grown.available_capacity_before);
    assert!(grown.reserve_argument > 0);
    // Spare above the reserve argument is rounding. Exact spare leaves it at 0.
    // A bytes original-capacity floor leaves it above 0. Both are measurements.
    let measured_slack = fresh
        .iter()
        .map(|event| {
            if event.reserve_argument == 0 || event.reserved_growth == 0 {
                0
            } else {
                event
                    .available_capacity_after
                    .saturating_sub(event.reserve_argument)
            }
        })
        .max()
        .unwrap_or(0);
    assert_eq!(meter.rounding_peak, measured_slack);
    assert_eq!(
        meter.reserved_additional - base.reserved_additional,
        fresh.iter().map(|event| event.reserved_growth).sum::<u64>()
    );
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn fragmented_cancel_resumes() {
    for stop in [4usize, 6, 10, 14] {
        cancel_and_resume(Some(bounds(1_000, 100, 100, 4)), stop).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn fragmented_cancel_default_off_resumes() {
    cancel_and_resume(None, 4).await;
    cancel_and_resume(None, 14).await;
}

async fn cancel_and_resume(limits: Option<PgIncomingBounds>, stop: usize) {
    let (mut conn, peer) = connect_pair().await;
    if let Some(limits) = limits {
        conn.set_incoming_bounds(Some(limits)).expect("set");
    }
    let base = conn.incoming_allocation();
    let row = data_row(&[Some(&b"ABCD"[..])]);
    assert_eq!(row.len(), 15);
    peer.write(row[..stop].to_vec());
    let hung = tokio::time::timeout(CANCEL_WAIT, conn.recv_backend()).await;
    assert!(hung.is_err(), "recv completed on a {stop}-byte fragment");
    let mid = conn.incoming_allocation();
    assert_eq!(
        mid.bytes_read - base.bytes_read,
        stop as u64,
        "cancel fired before the fragment was absorbed"
    );
    assert!(conn.incoming_synchronized());
    peer.write(row[stop..].to_vec());
    let got = recv(&mut conn).await;
    assert_eq!(got.format_byte(), b'D');
    assert_eq!(got.contents().as_ref(), &row[5..]);
    peer.write(frame(b'I', b""));
    let next = recv(&mut conn).await;
    assert_eq!(next.format_byte(), b'I');
    assert!(next.contents().is_empty());
    assert!(conn.incoming_synchronized());
    trace(
        &format!("cancel-{stop}-bounds-{}", limits.is_some()),
        &conn.incoming_allocation(),
    );
    drop(peer);
}

async fn reject_case(
    case: &str,
    limits: PgIncomingBounds,
    bytes: Vec<u8>,
    sent: u64,
    decision: u64,
    expected: IncomingWireError,
) {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(limits)).expect("set");
    let base = conn.incoming_allocation();
    let wire = bytes.len() as u64;
    assert_eq!(wire, sent);
    peer.write(bytes);
    let err = recv_err(&mut conn).await;
    assert_wire(&err, expected);
    let meter = conn.incoming_allocation();
    trace(case, &meter);
    assert_small_reject(&base, &meter, sent, decision);
    assert!(meter.view_capacity_peak < 1_000_000);
    assert!(!conn.incoming_synchronized());
    let again = recv_err(&mut conn).await;
    assert_wire(&again, expected);
    assert_eq!(conn.incoming_allocation().bytes_read, meter.bytes_read);
    drop(peer);
}

fn assert_small_reject(
    before: &PgIncomingAllocation,
    after: &PgIncomingAllocation,
    sent: u64,
    decision: u64,
) {
    assert_eq!(after.bytes_read - before.bytes_read, sent);
    assert_eq!(after.reserved_additional, before.reserved_additional);
    assert_eq!(after.view_capacity_peak, before.view_capacity_peak);
    assert_eq!(after.last_logical_need, decision);
    assert!(after.logical_requested_peak < 1_000_000);
    assert_eq!(after.assembly_bytes, 0);
    assert!(after.trace_bytes < 1_000_000);
    assert!(after.reads - before.reads <= 2);
}

fn assert_no_declared(meter: &PgIncomingAllocation, declared: u64) {
    assert!(meter.logical_requested_peak < declared);
    assert!(meter.reserved_additional < declared);
    assert_ne!(meter.view_capacity_peak, declared);
    // The socket starts with an 8192-byte spare. A declaration smaller than that
    // spare must not be treated as growth, and a huge declaration must not be reserved.
    assert!(
        meter.view_capacity_peak < declared || meter.view_capacity_peak <= 8_192,
        "view {} declared {declared}",
        meter.view_capacity_peak
    );
    assert!(meter.last_logical_need < declared);
    assert!(meter.events.iter().all(|event| {
        event.logical_need < declared && event.reserve_argument < declared
    }));
}

fn assert_wire(err: &sqlx::Error, expected: IncomingWireError) {
    assert_eq!(err.as_incoming_wire(), Some(expected));
    let text = err.to_string();
    let debug = format!("{err:?}");
    assert!(!text.contains("SECRET"), "{text}");
    assert!(!debug.contains("SECRET"), "{debug}");
    assert!(!text.chars().any(|ch| ch.is_ascii_digit()), "{text}");
    assert!(
        !debug.chars().any(|ch| ch.is_ascii_digit()),
        "{debug}"
    );
}

fn assert_invalid(err: sqlx::Error) {
    match err {
        sqlx::Error::InvalidArgument(text) => {
            assert_eq!(text, "incoming bounds are inconsistent");
        }
        other => panic!("unexpected {other:?}"),
    }
}

fn trace(case: &str, meter: &PgIncomingAllocation) {
    println!(
        "TRACE case={case} logical_peak={} reserved_growth={} view_capacity_peak={} reads={} bytes_read={} assembly={} accepted_logical={} overread={} rounding={} last_logical={} reserve_calls={} trace_bytes={} messages={} accepted_backing={}",
        meter.logical_requested_peak,
        meter.reserved_additional,
        meter.view_capacity_peak,
        meter.reads,
        meter.bytes_read,
        meter.assembly_bytes,
        meter.accepted_logical_payload,
        meter.overread_bytes,
        meter.rounding_peak,
        meter.last_logical_need,
        meter.reserve_calls,
        meter.trace_bytes,
        meter.messages,
        meter.accepted_backing_peak,
    );
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

fn header(format: u8, message_len: u32) -> Vec<u8> {
    let mut out = vec![format];
    out.extend_from_slice(&message_len.to_be_bytes());
    out
}

fn header_and(format: u8, message_len: u32, prefix: &[u8]) -> Vec<u8> {
    let mut out = header(format, message_len);
    out.extend_from_slice(prefix);
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
    Eof(Sender<std::io::Result<usize>>),
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

    fn eof(&self) -> Receiver<std::io::Result<usize>> {
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

fn read_to_eof(stream: &mut TcpStream) -> std::io::Result<usize> {
    let mut total = 0usize;
    let mut buf = [0u8; 128];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => return Ok(total),
            Ok(n) => total += n,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err)
                if err.kind() == std::io::ErrorKind::ConnectionReset
                    || err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                return Ok(total);
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
        .username("bounds")
        .database("bounds")
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
