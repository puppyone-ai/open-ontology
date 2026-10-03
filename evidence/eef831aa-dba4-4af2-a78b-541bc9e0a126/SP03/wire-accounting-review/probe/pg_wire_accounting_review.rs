//! Independent finite TCP checks of the receive meter on ff783ac9.
//!
//! Figures are live-view deltas from the public meter. They are not RSS, not
//! `malloc_usable_size`, and not the sum of frames the caller still holds.
//! A positive no-op `BytesMut::reserve` is not claimed: these tests only
//! record that `noop_reserve_calls` stayed 0. The `available.len() != 0`
//! branch is not claimed either.

use std::io::{Read, Write};
use std::mem::size_of;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use sqlx::postgres::{
    IncomingWireError, PgBackendFrame, PgConnectOptions, PgConnection, PgIncomingAllocation,
    PgIncomingAllocEvent, PgIncomingBounds, PgIncomingLimit, PgSslMode,
};
use sqlx::Connection;

const HANG: Duration = Duration::from_secs(10);
const PARK: Duration = Duration::from_millis(750);

#[tokio::test(flavor = "current_thread")]
async fn header_first_12000_park_drop_and_retry() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(wide())).expect("set");
    let base = conn.incoming_allocation();
    let wire = sized_frame(12_000);
    peer.write(wire[..5].to_vec());
    let parked = tokio::time::timeout(PARK, conn.recv_backend()).await;
    assert!(parked.is_err(), "header-only read completed");
    let mid = conn.incoming_allocation();
    trace("review-12000-parked", &mid);
    assert_eq!(mid.bytes_read - base.bytes_read, 5, "header was not the only new read");
    assert!(mid.reserved_additional > base.reserved_additional);
    assert!(mid.reserve_argument_bytes > base.reserve_argument_bytes);
    assert_ne!(
        mid.reserve_argument_bytes - base.reserve_argument_bytes,
        mid.reserved_additional - base.reserved_additional,
        "parked argument delta and growth delta collapsed"
    );
    let hung = fresh(&base, &mid)
        .iter()
        .find(|event| event.read_completed == 0 && event.reserved_growth > 0)
        .expect("reserve was not logged before the parked read returned");
    assert_eq!(hung.bytes_returned, 0, "{hung:?}");
    assert_reserve_exceeds_spare(hung);
    assert!(
        hung.assembly_bytes <= hung.read_len_before,
        "{hung:?}"
    );

    peer.write(wire[5..].to_vec());
    let got = recv(&mut conn).await;
    assert_eq!(got.format_byte(), b'C');
    assert_eq!(got.contents().as_ref(), &wire[5..]);
    let done = conn.incoming_allocation();
    trace("review-12000-retry", &done);
    assert_eq!(done.reserved_additional, mid.reserved_additional);
    assert_eq!(done.reserve_calls, mid.reserve_calls);
    assert_eq!(done.bytes_read - base.bytes_read, wire.len() as u64);
    assert!(
        done.events.iter().any(|event| {
            event.read_completed == 0
                && event.reserved_growth == hung.reserved_growth
                && event.bytes_returned == 0
        }),
        "retry rewrote the parked reserve row"
    );
    let retry = fresh(&mid, &done)
        .iter()
        .find(|event| event.bytes_returned > 0)
        .expect("body read was not logged");
    assert_eq!(retry.reserved_growth, 0, "{retry:?}");
    assert_eq!(retry.read_completed, 1, "{retry:?}");
    assert_eq!(done.noop_reserve_calls, 0);
    reconcile_if_complete(&done);
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn partial_body_cancel_does_not_grow_again() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(wide())).expect("set");
    let base = conn.incoming_allocation();
    let wire = sized_frame(12_000);
    peer.write(wire[..5].to_vec());
    assert!(tokio::time::timeout(PARK, conn.recv_backend()).await.is_err());
    let reserved = conn.incoming_allocation();
    let growth = reserved.reserved_additional;
    assert!(growth > base.reserved_additional);
    peer.write(wire[5..105].to_vec());
    assert!(tokio::time::timeout(PARK, conn.recv_backend()).await.is_err());
    let partial = conn.incoming_allocation();
    trace("review-partial-body", &partial);
    assert_eq!(partial.bytes_read - base.bytes_read, 105);
    assert_eq!(partial.reserved_additional, growth);
    assert_eq!(partial.reserve_calls, reserved.reserve_calls);
    peer.write(wire[105..].to_vec());
    let got = recv(&mut conn).await;
    assert_eq!(got.contents().as_ref(), &wire[5..]);
    let done = conn.incoming_allocation();
    trace("review-partial-done", &done);
    assert_eq!(done.reserved_additional, growth);
    assert_eq!(done.bytes_read - base.bytes_read, 12_000);
    assert!(done.events.iter().any(|event| {
        event.read_completed == 0 && event.reserved_growth > 0 && event.bytes_returned == 0
    }));
    assert_eq!(done.noop_reserve_calls, 0);
    reconcile_if_complete(&done);
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn command_complete_20011_one_shot() {
    one_shot_20011(b'C', false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn data_row_20011_one_shot() {
    one_shot_20011(b'D', true).await;
}

async fn one_shot_20011(format: u8, data_row: bool) {
    let (mut conn, peer) = connect_pair().await;
    let bounds = if data_row {
        PgIncomingBounds::try_new(100_000, 50_000, 80_000, 4).expect("row bounds")
    } else {
        wide()
    };
    conn.set_incoming_bounds(Some(bounds)).expect("set");
    let base = conn.incoming_allocation();
    let wire = if data_row {
        data_row_frame(&vec![b'Q'; 20_000])
    } else {
        sized_frame(20_011)
    };
    assert_eq!(wire.len(), 20_011, "fixture size");
    peer.write(wire.clone());
    let got = recv(&mut conn).await;
    assert_eq!(got.format_byte(), format);
    assert_eq!(got.contents().len(), 20_006);
    let meter = conn.incoming_allocation();
    trace(
        if data_row {
            "review-datarow-20011"
        } else {
            "review-command-20011"
        },
        &meter,
    );
    assert_eq!(meter.logical_requested_peak, 20_011);
    assert_eq!(meter.bytes_read - base.bytes_read, 20_011);
    assert!(meter.reserved_additional > base.reserved_additional);
    assert_ne!(meter.view_capacity_peak, 100_000);
    let grown = fresh(&base, &meter)
        .iter()
        .find(|event| event.reserved_growth > 0 && event.bytes_returned > 0)
        .expect("growth and body were not on one completed read");
    println!(
        "REVIEW one-shot format={format} argument={} growth={} assembly={} read_len_before={} bytes_returned={} avail_before={} avail_len_before={} avail_after={} rounding={} view_after={}",
        grown.reserve_argument,
        grown.reserved_growth,
        grown.assembly_bytes,
        grown.read_len_before,
        grown.bytes_returned,
        grown.available_capacity_before,
        grown.available_len_before,
        grown.available_capacity_after,
        meter.rounding_peak,
        grown.view_capacity_after
    );
    assert_eq!(
        grown.reserve_argument,
        grown.logical_need - grown.read_len_before,
        "{grown:?}"
    );
    assert_eq!(
        grown.reserved_growth,
        grown.view_capacity_after - grown.view_capacity_before,
        "{grown:?}"
    );
    assert_eq!(grown.available_len_before, 0, "{grown:?}");
    assert!(grown.read_len_before > 0, "{grown:?}");
    assert!(
        grown.assembly_bytes <= grown.read_len_before,
        "assembly counted bytes past the live prefix: {grown:?}"
    );
    assert!(
        grown.assembly_bytes < grown.bytes_returned,
        "assembly included the new socket bytes: {grown:?}"
    );
    assert!(meter.assembly_bytes < wire.len() as u64);
    assert_eq!(meter.noop_reserve_calls, 0);
    assert_eq!(meter.rounding_peak, measured_slack(&meter));
    reconcile_if_complete(&meter);
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn shrink_makes_disjoint_copy_counted_once() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(wide())).expect("set");
    let base = conn.incoming_allocation();
    let wire = sized_frame(12_000);
    peer.write(wire[..5].to_vec());
    assert!(tokio::time::timeout(PARK, conn.recv_backend()).await.is_err());
    let mid = conn.incoming_allocation();
    trace("review-disjoint-parked", &mid);
    assert!(mid.reserved_additional > base.reserved_additional);
    let parked_growth = mid.reserved_additional;
    conn.shrink_buffers();
    let shrunk = conn.incoming_allocation();
    assert_eq!(shrunk.reserved_additional, parked_growth);
    assert_eq!(shrunk.bytes_read, mid.bytes_read);
    assert_eq!(shrunk.assembly_bytes, mid.assembly_bytes);
    peer.write(wire[5..].to_vec());
    let got = recv(&mut conn).await;
    assert_eq!(got.contents().as_ref(), &wire[5..]);
    let done = conn.incoming_allocation();
    trace("review-disjoint-done", &done);
    let body = (wire.len() - 5) as u64;
    let copied = done.assembly_bytes - mid.assembly_bytes;
    let grew = done.reserved_additional - parked_growth;
    println!("REVIEW disjoint copied={copied} body={body} grew={grew} calls={}", done.reserve_calls);
    assert!(
        grew > 0,
        "shrink left the old spare in place; disjoint reserve was not reached"
    );
    assert!(
        copied >= body,
        "failed try_unsplit copy was not counted, copied={copied} body={body}"
    );
    assert!(
        copied <= body + 8_192,
        "copy count exceeds one body plus one live window, copied={copied}"
    );
    assert!(copied < body * 2, "body counted twice, copied={copied}");
    assert_eq!(done.bytes_read - base.bytes_read, wire.len() as u64);
    assert_eq!(done.noop_reserve_calls, 0);
    reconcile_if_complete(&done);
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn retained_frames_exceed_live_view() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(wide())).expect("set");
    let mut held = Vec::new();
    let mut retained_wire = 0u64;
    for index in 0..24 {
        let body = vec![b'A' + u8::try_from(index % 26).expect("letter"); 1_000];
        let wire = frame(b'C', &body);
        retained_wire += wire.len() as u64;
        peer.write(wire);
        held.push(recv(&mut conn).await);
    }
    let bodies: u64 = held.iter().map(|frame| frame.contents().len() as u64).sum();
    let meter = conn.incoming_allocation();
    trace("review-retained", &meter);
    println!(
        "REVIEW retained_wire={retained_wire} bodies={bodies} view_peak={} accepted_backing={} reserved={} assembly={}",
        meter.view_capacity_peak,
        meter.accepted_backing_peak,
        meter.reserved_additional,
        meter.assembly_bytes
    );
    assert_eq!(bodies, 24_000);
    assert_eq!(meter.accepted_logical_payload, bodies + 5);
    assert!(meter.view_capacity_peak < bodies);
    assert!(meter.accepted_backing_peak < bodies);
    assert!(meter.accepted_backing_peak <= meter.view_capacity_peak);
    assert!(retained_wire > meter.view_capacity_peak);
    assert!(retained_wire > meter.accepted_backing_peak);
    assert_ne!(meter.reserved_additional, retained_wire);
    assert!(meter.assembly_bytes < bodies);
    assert_eq!(meter.noop_reserve_calls, 0);
    let before_drop = meter.view_capacity_peak;
    drop(held);
    assert_eq!(conn.incoming_allocation().view_capacity_peak, before_drop);
    reconcile_if_complete(&conn.incoming_allocation());
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn growth_after_event_cap_still_counts() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(wide())).expect("set");
    let first = conn.incoming_allocation().events[0];
    let mut sent = 0u64;
    while conn.incoming_allocation().events_dropped == 0 {
        assert!(sent < 90, "event log did not stop");
        peer.write(frame(b'C', b"x"));
        let got = recv(&mut conn).await;
        assert_eq!(got.contents().as_ref(), b"x");
        sent += 1;
    }
    let capped = conn.incoming_allocation();
    trace("review-event-cap", &capped);
    assert_eq!(capped.events.len(), 64);
    assert!(capped.events_dropped >= 1);
    assert_eq!(capped.events[0].logical_need, first.logical_need);
    assert_eq!(capped.events[0].bytes_returned, first.bytes_returned);
    let stride = size_of::<PgIncomingAllocEvent>() as u64;
    assert_eq!(capped.trace_bytes % stride, 0);
    let slots = capped.trace_bytes / stride;
    assert!((64..=128).contains(&slots), "trace slots {slots}");
    let stored_growth: u64 = capped.events.iter().map(|event| event.reserved_growth).sum();
    let stored_arguments: u64 = capped.events.iter().map(|event| event.reserve_argument).sum();

    let wire = sized_frame(12_000);
    peer.write(wire.clone());
    let got = recv(&mut conn).await;
    assert_eq!(got.contents().len(), 11_995);
    let later = conn.incoming_allocation();
    trace("review-growth-after-cap", &later);
    let stored_growth_later: u64 = later.events.iter().map(|event| event.reserved_growth).sum();
    let stored_arguments_later: u64 = later.events.iter().map(|event| event.reserve_argument).sum();
    println!(
        "REVIEW after-cap growth_counter={} stored_growth={} argument_counter={} stored_arguments={} dropped={} trace_bytes={}",
        later.reserved_additional,
        stored_growth_later,
        later.reserve_argument_bytes,
        stored_arguments_later,
        later.events_dropped,
        later.trace_bytes
    );
    assert_eq!(later.events.len(), 64);
    assert!(later.events_dropped > capped.events_dropped);
    assert_eq!(later.trace_bytes, capped.trace_bytes);
    assert_eq!(stored_growth_later, stored_growth);
    assert_eq!(stored_arguments_later, stored_arguments);
    assert!(later.reserved_additional > capped.reserved_additional);
    assert!(later.reserve_argument_bytes > capped.reserve_argument_bytes);
    assert!(later.reserved_additional > stored_growth_later);
    assert_eq!(later.bytes_read, capped.bytes_read + wire.len() as u64);
    assert!(later.messages > capped.messages);
    assert_eq!(later.events[0].bytes_returned, first.bytes_returned);
    assert_eq!(later.noop_reserve_calls, 0);
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn eof_after_reserve_completes_event() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(wide())).expect("set");
    let base = conn.incoming_allocation();
    let wire = sized_frame(12_000);
    peer.write_then_shutdown(wire[..5].to_vec());
    let err = recv_err(&mut conn).await;
    let meter = conn.incoming_allocation();
    trace("review-eof", &meter);
    let rendered = format!("{err} {err:?}");
    println!("REVIEW eof-error {rendered}");
    assert!(
        rendered.contains("EOF"),
        "expected the zero-byte read path, got {rendered}"
    );
    assert!(err.as_incoming_wire().is_none(), "{rendered}");
    assert!(meter.reserved_additional > base.reserved_additional);
    assert_eq!(meter.bytes_read - base.bytes_read, 5);
    let eof_event = fresh(&base, &meter)
        .iter()
        .find(|event| event.reserved_growth > 0 && event.bytes_returned == 0)
        .expect("eof lost the reserve row");
    assert_eq!(eof_event.read_completed, 1, "{eof_event:?}");
    assert_reserve_exceeds_spare(eof_event);
    assert_eq!(meter.noop_reserve_calls, 0);
    reconcile_if_complete(&meter);
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn huge_header_does_not_reserve_body() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(PgIncomingBounds::try_new(1_000, 0, 0, 0).expect("bounds")))
        .expect("set");
    let base = conn.incoming_allocation();
    peer.write(header(b'C', 2_000_000_000));
    let err = recv_err(&mut conn).await;
    assert_wire(&err, IncomingWireError::Limit(PgIncomingLimit::Message));
    let meter = conn.incoming_allocation();
    trace("review-huge", &meter);
    assert_eq!(meter.bytes_read - base.bytes_read, 5);
    assert_eq!(meter.reserved_additional, base.reserved_additional);
    assert_eq!(meter.reserve_argument_bytes, base.reserve_argument_bytes);
    assert_eq!(meter.reserve_calls, base.reserve_calls);
    assert!(meter.logical_requested_peak < 2_000_000_000);
    assert!(meter.events.iter().all(|event| event.logical_need < 1_000_000));
    peer.write(frame(b'C', b"NOLEAK"));
    let again = recv_err(&mut conn).await;
    assert_wire(&again, IncomingWireError::Limit(PgIncomingLimit::Message));
    let poisoned = conn.incoming_allocation();
    assert_eq!(poisoned.bytes_read, meter.bytes_read);
    assert_eq!(poisoned.reserved_additional, meter.reserved_additional);
    assert!(!conn.incoming_synchronized());
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn fragment_order_preserves_following_frame() {
    let (mut conn, peer) = connect_pair().await;
    let before = conn.incoming_allocation();
    let wire = frame(b'C', b"ORDER-BODY");
    peer.write(wire[..4].to_vec());
    assert!(tokio::time::timeout(PARK, conn.recv_backend()).await.is_err());
    let mid = conn.incoming_allocation();
    assert_eq!(mid.bytes_read - before.bytes_read, 4);
    assert_eq!(mid.reserved_additional, before.reserved_additional);
    peer.write(wire[4..].to_vec());
    let got = recv(&mut conn).await;
    assert_eq!(got.contents().as_ref(), b"ORDER-BODY");
    peer.write(frame(b'C', b"NEXT"));
    let next = recv(&mut conn).await;
    assert_eq!(next.contents().as_ref(), b"NEXT");
    let done = conn.incoming_allocation();
    trace("review-fragment-order", &done);
    assert_eq!(done.bytes_read - before.bytes_read, (wire.len() + 9) as u64);
    assert_eq!(done.noop_reserve_calls, 0);
    reconcile_if_complete(&done);
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn unset_large_frame_is_accepted() {
    let (mut conn, peer) = connect_pair().await;
    assert!(conn.incoming_bounds().is_none());
    let base = conn.incoming_allocation();
    let wire = sized_frame(12_000);
    peer.write(wire.clone());
    let got = recv(&mut conn).await;
    assert_eq!(got.format_byte(), b'C');
    assert_eq!(got.contents().len(), 11_995);
    let meter = conn.incoming_allocation();
    trace("review-unset-large", &meter);
    assert!(conn.incoming_synchronized());
    assert_eq!(meter.bytes_read - base.bytes_read, 12_000);
    assert!(meter.reserved_additional > base.reserved_additional);
    assert_eq!(meter.logical_requested_peak, 12_000);
    assert_eq!(meter.noop_reserve_calls, 0);
    let grown = fresh(&base, &meter)
        .iter()
        .find(|event| event.reserved_growth > 0)
        .expect("unset large frame did not grow a view");
    assert_eq!(grown.available_len_before, 0, "{grown:?}");
    assert_reserve_exceeds_spare(grown);
    reconcile_if_complete(&meter);
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn poison_blocks_ping_setter_and_flushes_on_hard_close() {
    let (mut conn, peer) = connect_pair().await;
    assert!(conn.incoming_bounds().is_none());
    let base = conn.incoming_allocation();
    peer.write(header(b'I', 3));
    let err = recv_err(&mut conn).await;
    assert_wire(&err, IncomingWireError::ShortLength);
    let mid = conn.incoming_allocation();
    trace("review-poison", &mid);
    assert_eq!(mid.bytes_read - base.bytes_read, 5);
    assert_eq!(mid.reserved_additional, base.reserved_additional);
    assert!(!conn.incoming_synchronized());
    peer.write(frame(b'C', b"NOLEAK"));
    let again = recv_err(&mut conn).await;
    assert_wire(&again, IncomingWireError::ShortLength);
    assert_eq!(conn.incoming_allocation().bytes_read, mid.bytes_read);

    let (mut other, other_peer) = connect_pair().await;
    other_peer.write(frame(b'C', b"ok"));
    let other_frame = recv(&mut other).await;
    assert_eq!(other_frame.contents().as_ref(), b"ok");
    assert!(other.incoming_synchronized());
    drop(other);
    drop(other_peer);

    conn.set_incoming_bounds(None).expect("unset");
    assert!(!conn.incoming_synchronized());
    conn.set_incoming_bounds(Some(wide())).expect("widen");
    assert!(!conn.incoming_synchronized());
    let ping = tokio::time::timeout(Duration::from_secs(2), conn.ping()).await;
    let ping_err = ping.expect("ping hung").expect_err("ping cleared the fault");
    assert_wire(&ping_err, IncomingWireError::ShortLength);
    assert_eq!(conn.incoming_allocation().bytes_read, mid.bytes_read);
    assert_eq!(conn.incoming_allocation().reserve_calls, mid.reserve_calls);

    let flushed = peer.begin_read();
    conn.close_hard().await.expect("close_hard");
    let bytes = flushed
        .recv_timeout(Duration::from_secs(5))
        .expect("peer did not finish")
        .expect("peer read");
    assert_eq!(bytes, b"S\x00\x00\x00\x04");
}

fn assert_reserve_exceeds_spare(event: &PgIncomingAllocEvent) {
    // Spare is capacity minus len on `available` before the call. It is 0 when
    // the bytes already read sit entirely in `read`. That is still a real
    // reserve: the argument is the deficit past `read.len()`, and it is not
    // the same number as the view-capacity growth.
    let spare = event
        .available_capacity_before
        .saturating_sub(event.available_len_before);
    assert!(event.read_len_before <= event.logical_need, "{event:?}");
    let deficit = event.logical_need - event.read_len_before;
    assert_eq!(event.reserve_argument, deficit, "{event:?}");
    assert!(event.reserve_argument > spare, "{event:?}");
    assert_ne!(event.reserve_argument, event.reserved_growth, "{event:?}");
    assert_eq!(
        event.reserved_growth,
        event.view_capacity_after - event.view_capacity_before,
        "{event:?}"
    );
    assert_eq!(
        event.view_capacity_before,
        event.read_capacity_before + event.available_capacity_before,
        "{event:?}"
    );
}

fn fresh<'a>(
    base: &'a PgIncomingAllocation,
    now: &'a PgIncomingAllocation,
) -> &'a [PgIncomingAllocEvent] {
    assert!(now.events.len() >= base.events.len());
    &now.events[base.events.len()..]
}

fn reconcile_if_complete(meter: &PgIncomingAllocation) {
    assert_eq!(meter.events_dropped, 0, "dropped rows");
    let growth: u64 = meter.events.iter().map(|event| event.reserved_growth).sum();
    let assembly: u64 = meter.events.iter().map(|event| event.assembly_bytes).sum();
    let arguments: u64 = meter.events.iter().map(|event| event.reserve_argument).sum();
    let calls = meter
        .events
        .iter()
        .filter(|event| event.reserve_argument > 0)
        .count() as u64;
    let noops = meter
        .events
        .iter()
        .filter(|event| event.reserve_argument > 0 && event.reserved_growth == 0)
        .count() as u64;
    let returned: u64 = meter.events.iter().map(|event| event.bytes_returned).sum();
    assert_eq!(meter.reserved_additional, growth);
    assert_eq!(meter.assembly_bytes, assembly);
    assert_eq!(meter.reserve_argument_bytes, arguments);
    assert_eq!(meter.reserve_calls, calls);
    assert_eq!(meter.noop_reserve_calls, noops);
    assert_eq!(meter.noop_reserve_calls, 0);
    assert_eq!(meter.bytes_read, returned);
    assert_eq!(meter.rounding_peak, measured_slack(meter));
}

fn measured_slack(meter: &PgIncomingAllocation) -> u64 {
    meter
        .events
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
        .unwrap_or(0)
}

fn trace(case: &str, meter: &PgIncomingAllocation) {
    println!(
        "REVIEW case={case} logical_peak={} reserved_growth={} reserve_arguments={} noop_calls={} reserve_calls={} view_capacity_peak={} reads={} bytes_read={} assembly={} accepted_logical={} accepted_backing={} overread={} rounding={} last_logical={} trace_bytes={} messages={} events={} dropped={}",
        meter.logical_requested_peak,
        meter.reserved_additional,
        meter.reserve_argument_bytes,
        meter.noop_reserve_calls,
        meter.reserve_calls,
        meter.view_capacity_peak,
        meter.reads,
        meter.bytes_read,
        meter.assembly_bytes,
        meter.accepted_logical_payload,
        meter.accepted_backing_peak,
        meter.overread_bytes,
        meter.rounding_peak,
        meter.last_logical_need,
        meter.trace_bytes,
        meter.messages,
        meter.events.len(),
        meter.events_dropped
    );
}

fn wide() -> PgIncomingBounds {
    PgIncomingBounds::try_new(100_000, 0, 0, 0).expect("bounds")
}

fn sized_frame(total: usize) -> Vec<u8> {
    assert!(total >= 5);
    frame(b'C', &vec![b'B'; total - 5])
}

fn data_row_frame(payload: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(6 + payload.len());
    body.extend_from_slice(&1u16.to_be_bytes());
    body.extend_from_slice(&u32::try_from(payload.len()).expect("field len").to_be_bytes());
    body.extend_from_slice(payload);
    frame(b'D', &body)
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

fn assert_wire(err: &sqlx::Error, expected: IncomingWireError) {
    assert_eq!(err.as_incoming_wire(), Some(expected));
    let text = err.to_string();
    let debug = format!("{err:?}");
    assert!(!text.contains("NOLEAK"), "{text}");
    assert!(!debug.contains("NOLEAK"), "{debug}");
    assert!(!text.chars().any(|ch| ch.is_ascii_digit()), "{text}");
    assert!(!debug.chars().any(|ch| ch.is_ascii_digit()), "{debug}");
}

enum Cmd {
    Write(Vec<u8>, Sender<()>),
    WriteShutdown(Vec<u8>, Sender<()>),
    ReadToEof(Sender<()>, Sender<std::io::Result<Vec<u8>>>),
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
        wait.recv_timeout(Duration::from_secs(5)).expect("peer write");
    }

    fn write_then_shutdown(&self, bytes: Vec<u8>) {
        let (ack, wait) = mpsc::channel();
        self.tx
            .as_ref()
            .expect("peer")
            .send(Cmd::WriteShutdown(bytes, ack))
            .expect("peer command");
        wait.recv_timeout(Duration::from_secs(5))
            .expect("peer shutdown");
    }

    fn begin_read(&self) -> Receiver<std::io::Result<Vec<u8>>> {
        let (started, started_wait) = mpsc::channel();
        let (done, done_wait) = mpsc::channel();
        self.tx
            .as_ref()
            .expect("peer")
            .send(Cmd::ReadToEof(started, done))
            .expect("peer command");
        started_wait
            .recv_timeout(Duration::from_secs(5))
            .expect("peer started read");
        done_wait
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
            Cmd::WriteShutdown(bytes, ack) => {
                if stream.write_all(&bytes).is_err() || stream.flush().is_err() {
                    return;
                }
                let _ = stream.shutdown(Shutdown::Write);
                let _ = ack.send(());
            }
            Cmd::ReadToEof(started, done) => {
                let _ = started.send(());
                let _ = done.send(read_to_eof(&mut stream));
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
    let mut buf = [0u8; 128];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => return Ok(out),
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err)
                if err.kind() == std::io::ErrorKind::ConnectionReset
                    || err.kind() == std::io::ErrorKind::UnexpectedEof =>
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
