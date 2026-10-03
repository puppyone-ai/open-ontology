//! Measurements of incoming receive allocation.
//!
//! Each test is separate so one failure does not skip the next. Figures come from
//! the public meter after real socket reads. They are not copied from a formula
//! in the buffer implementation, and they are not `malloc_usable_size` or RSS.
//!
//! `reserve_argument` is what was passed to `BytesMut::reserve` (additional bytes
//! beyond `len`). `reserved_growth` is the change in `read.capacity() +
//! available.capacity()`. Those two sums are live windows: they can share an
//! allocation, and they do not include frames the caller still holds.
//! `assembly_bytes` counts copies of those windows. Zero there means the measured
//! copy was zero, not that the field was suppressed.

use std::io::{Read, Write};
use std::mem::size_of;
use std::net::{TcpListener, TcpStream};
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
async fn noop_request_is_zero_growth() {
    let (mut conn, peer) = connect_pair().await;
    let base = conn.incoming_allocation();
    let wire = frame(b'C', b"ok");
    peer.write(wire.clone());
    let got = recv(&mut conn).await;
    assert_eq!(got.format_byte(), b'C');
    assert_eq!(got.contents().as_ref(), b"ok");
    let meter = conn.incoming_allocation();
    trace("noop", &meter);
    assert_eq!(meter.reserved_additional, base.reserved_additional);
    assert_eq!(meter.reserve_argument_bytes, base.reserve_argument_bytes);
    assert_eq!(meter.noop_reserve_calls, base.noop_reserve_calls);
    assert_eq!(meter.noop_reserve_arguments, base.noop_reserve_arguments);
    assert_eq!(meter.reserve_calls, base.reserve_calls);
    let fresh = &meter.events[base.events.len()..];
    assert!(!fresh.is_empty(), "the small frame produced no read event");
    for event in fresh {
        assert_eq!(event.reserve_argument, 0, "{event:?}");
        assert_eq!(event.reserved_growth, 0, "{event:?}");
        assert_eq!(event.read_completed, 1, "{event:?}");
    }
    reconcile(&meter);
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn reserve_argument_is_distinct_from_growth() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(wide())).expect("set");
    let base = conn.incoming_allocation();
    let wire = sized_frame(12_000);
    peer.write(wire[..5].to_vec());
    let parked = tokio::time::timeout(PARK, conn.recv_backend()).await;
    assert!(parked.is_err(), "header-only read completed");
    peer.write(wire[5..].to_vec());
    let got = recv(&mut conn).await;
    assert_eq!(got.contents().as_ref(), &wire[5..]);
    let meter = conn.incoming_allocation();
    trace("argument-vs-growth", &meter);
    let fresh = &meter.events[base.events.len()..];
    let grown = fresh
        .iter()
        .find(|event| event.reserved_growth > 0)
        .expect("no view growth");
    let spare = grown
        .available_capacity_before
        .saturating_sub(grown.available_len_before);
    let deficit = grown.logical_need - grown.read_len_before;
    println!(
        "TRACE argument={} growth={} spare_before={} deficit={} view_before={} view_after={}",
        grown.reserve_argument,
        grown.reserved_growth,
        spare,
        deficit,
        grown.view_capacity_before,
        grown.view_capacity_after
    );
    assert!(spare > 0, "header-first growth had no spare to distinguish, {grown:?}");
    assert_eq!(grown.reserve_argument, deficit, "{grown:?}");
    assert!(grown.reserve_argument > spare, "{grown:?}");
    assert_ne!(
        grown.reserve_argument, grown.reserved_growth,
        "argument and growth collapsed to one number: {grown:?}"
    );
    assert_eq!(
        grown.reserved_growth,
        grown.view_capacity_after - grown.view_capacity_before,
        "{grown:?}"
    );
    assert_ne!(meter.reserve_argument_bytes, meter.reserved_additional);
    reconcile(&meter);
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn parked_read_drop_keeps_growth_and_reconciles() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(wide())).expect("set");
    let base = conn.incoming_allocation();
    let wire = sized_frame(12_000);
    peer.write(wire[..5].to_vec());
    let parked = tokio::time::timeout(PARK, conn.recv_backend()).await;
    assert!(parked.is_err(), "recv completed before the body existed");
    let mid = conn.incoming_allocation();
    trace("parked-drop", &mid);
    assert_eq!(mid.bytes_read - base.bytes_read, 5, "header was not absorbed");
    assert!(mid.reserved_additional > base.reserved_additional);
    let fresh = &mid.events[base.events.len()..];
    let hung = fresh
        .iter()
        .find(|event| event.read_completed == 0 && event.reserved_growth > 0)
        .expect("cancelled reserve left no event");
    assert_eq!(hung.bytes_returned, 0, "{hung:?}");
    assert!(hung.reserve_argument > 0, "{hung:?}");
    reconcile(&mid);

    peer.write(wire[5..].to_vec());
    let got = recv(&mut conn).await;
    assert_eq!(got.contents().as_ref(), &wire[5..]);
    let done = conn.incoming_allocation();
    trace("parked-retry", &done);
    assert_eq!(
        done.reserved_additional, mid.reserved_additional,
        "retry allocated again after the cancelled reserve"
    );
    let still = done
        .events
        .iter()
        .find(|event| {
            event.read_completed == 0
                && event.reserved_growth == hung.reserved_growth
                && event.reserve_argument == hung.reserve_argument
                && event.bytes_returned == 0
        })
        .expect("cancelled row was rewritten or removed");
    assert_eq!(still.view_capacity_before, hung.view_capacity_before);
    let retry = done.events[mid.events.len()..]
        .iter()
        .find(|event| event.read_completed == 1 && event.bytes_returned > 0)
        .expect("retry did not record the body read");
    assert_eq!(retry.reserved_growth, 0, "{retry:?}");
    assert_eq!(done.bytes_read - base.bytes_read, wire.len() as u64);
    reconcile(&done);
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn header_first_12000_trace() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(wide())).expect("set");
    let base = conn.incoming_allocation();
    let wire = sized_frame(12_000);
    assert_eq!(wire.len(), 12_000);
    peer.write(wire[..5].to_vec());
    let parked = tokio::time::timeout(PARK, conn.recv_backend()).await;
    assert!(parked.is_err(), "12000 header was not held for a second read");
    let mid = conn.incoming_allocation();
    assert_eq!(mid.bytes_read - base.bytes_read, 5);
    peer.write(wire[5..].to_vec());
    let got = recv(&mut conn).await;
    assert_eq!(got.format_byte(), b'C');
    assert_eq!(got.contents().as_ref(), &wire[5..]);
    assert_eq!(got.contents().len(), 11_995);
    let meter = conn.incoming_allocation();
    trace("header-first-12000", &meter);
    assert_eq!(meter.logical_requested_peak, 12_000);
    assert_eq!(meter.bytes_read - base.bytes_read, 12_000);
    assert_eq!(
        meter.accepted_logical_payload - base.accepted_logical_payload,
        11_995
    );
    let fresh = &meter.events[base.events.len()..];
    assert!(
        fresh.iter().any(|event| event.logical_need == 5 && event.bytes_returned == 5),
        "header read missing: {fresh:?}"
    );
    assert!(
        fresh.iter().any(|event| {
            event.logical_need == 12_000 && event.read_completed == 0 && event.reserved_growth > 0
        }),
        "growth was not logged before the body read returned: {fresh:?}"
    );
    let body = fresh
        .iter()
        .find(|event| event.logical_need == 12_000 && event.bytes_returned > 0)
        .expect("body read");
    assert_eq!(body.read_completed, 1, "{body:?}");
    assert!(body.assembly_bytes <= body.read_len_before, "{body:?}");
    assert!(body.assembly_bytes < body.bytes_returned, "{body:?}");
    assert_eq!(meter.reserved_additional, mid.reserved_additional);
    assert_eq!(meter.rounding_peak, measured_slack(&meter));
    reconcile(&meter);
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn one_shot_20011_trace() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(wide())).expect("set");
    let base = conn.incoming_allocation();
    let wire = sized_frame(20_011);
    assert_eq!(wire.len(), 20_011);
    peer.write(wire.clone());
    let got = recv(&mut conn).await;
    assert_eq!(got.contents().as_ref(), &wire[5..]);
    assert_eq!(got.contents().len(), 20_006);
    let meter = conn.incoming_allocation();
    trace("one-shot-20011", &meter);
    assert_eq!(meter.logical_requested_peak, 20_011);
    assert_eq!(meter.bytes_read - base.bytes_read, 20_011);
    assert!(meter.overread_bytes > base.overread_bytes);
    assert!(meter.reserved_additional > base.reserved_additional);
    let fresh = &meter.events[base.events.len()..];
    let grown = fresh
        .iter()
        .find(|event| event.reserved_growth > 0)
        .expect("one-shot growth");
    println!(
        "TRACE one-shot argument={} growth={} assembly={} read_len_before={} bytes_returned={} avail_before={} avail_after={} rounding={}",
        grown.reserve_argument,
        grown.reserved_growth,
        grown.assembly_bytes,
        grown.read_len_before,
        grown.bytes_returned,
        grown.available_capacity_before,
        grown.available_capacity_after,
        meter.rounding_peak
    );
    assert!(grown.reserve_argument > 0, "{grown:?}");
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
    assert_eq!(grown.read_completed, 1, "{grown:?}");
    assert_eq!(meter.rounding_peak, measured_slack(&meter));
    // Distinct fields. This delivery can make the numbers equal; the header-first
    // test is the one that requires them to differ. Both still have to match the
    // view delta and the deficit, not each other.
    assert_eq!(meter.events_dropped, 0);
    reconcile(&meter);
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn unsplit_copy_does_not_duplicate_the_body() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(wide())).expect("set");
    let base = conn.incoming_allocation();
    let wire = sized_frame(20_011);
    peer.write(wire.clone());
    let got = recv(&mut conn).await;
    assert_eq!(got.contents().len(), 20_006);
    let meter = conn.incoming_allocation();
    trace("unsplit-copy", &meter);
    let fresh = &meter.events[base.events.len()..];
    let grown = fresh
        .iter()
        .find(|event| event.reserved_growth > 0 && event.bytes_returned > 0)
        .expect("growth and body landed on different observations");
    assert!(
        grown.read_len_before > 0,
        "nothing was retained across the grow: {grown:?}"
    );
    // The new socket bytes are joined without a second copy. A relocated prefix
    // is the bytes already in `read`. In-place growth copies none of them.
    // A copied body would add `bytes_returned` on top of that prefix.
    assert!(grown.assembly_bytes <= grown.read_len_before, "{grown:?}");
    assert!(meter.assembly_bytes < wire.len() as u64, "{}", meter.assembly_bytes);
    assert_eq!(
        meter.assembly_bytes,
        meter.events.iter().map(|event| event.assembly_bytes).sum::<u64>()
    );
    let copied_prefix = grown.assembly_bytes == grown.read_len_before;
    let copied_nothing = grown.assembly_bytes == 0;
    assert!(
        copied_prefix || copied_nothing,
        "assembly {} is neither zero nor the retained prefix {} on {grown:?}",
        grown.assembly_bytes,
        grown.read_len_before
    );
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn retained_frames_are_not_summed_into_backing() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(wide())).expect("set");
    let mut held = Vec::new();
    let mut retained = 0u64;
    for index in 0..24 {
        let body = vec![b'A' + u8::try_from(index % 26).expect("letter"); 1_000];
        let wire = frame(b'C', &body);
        retained += wire.len() as u64;
        peer.write(wire);
        held.push(recv(&mut conn).await);
    }
    assert_eq!(held.len(), 24);
    assert_eq!(held[0].contents().len(), 1_000);
    assert_eq!(held[23].contents().as_ref()[0], b'X');
    let meter = conn.incoming_allocation();
    trace("retained-frames", &meter);
    let bodies: u64 = held.iter().map(|frame| frame.contents().len() as u64).sum();
    println!(
        "TRACE retained_logical={retained} bodies={bodies} accepted_backing={} view_peak={} accepted_logical={} assembly={}",
        meter.accepted_backing_peak,
        meter.view_capacity_peak,
        meter.accepted_logical_payload,
        meter.assembly_bytes
    );
    assert_eq!(bodies, 24_000);
    // Handshake AuthenticationOk contributes 4 content bytes and ReadyForQuery 1.
    assert_eq!(meter.accepted_logical_payload, bodies + 5);
    assert!(retained > meter.accepted_backing_peak);
    assert!(retained > meter.view_capacity_peak);
    assert!(meter.accepted_backing_peak <= meter.view_capacity_peak);
    assert!(meter.accepted_backing_peak < bodies);
    reconcile(&meter);
    drop(held);
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn event_log_is_bounded_and_counters_continue() {
    let (mut conn, peer) = connect_pair().await;
    let base = conn.incoming_allocation();
    assert!(!base.events.is_empty(), "connect recorded no read");
    let first = base.events[0];
    let mut sent = 0u64;
    while conn.incoming_allocation().events_dropped == 0 {
        assert!(sent < 90, "log did not stop storing events");
        peer.write(frame(b'C', b"x"));
        let got = recv(&mut conn).await;
        assert_eq!(got.contents().as_ref(), b"x");
        sent += 1;
    }
    let capped = conn.incoming_allocation();
    trace("event-cap", &capped);
    assert_eq!(capped.events.len(), 64);
    assert!(capped.events_dropped > 0);
    assert_eq!(capped.events[0].logical_need, first.logical_need);
    assert_eq!(capped.events[0].bytes_returned, first.bytes_returned);
    assert_eq!(capped.events[0].read_completed, first.read_completed);
    let stride = size_of::<PgIncomingAllocEvent>() as u64;
    assert_eq!(capped.trace_bytes % stride, 0);
    let slots = capped.trace_bytes / stride;
    assert!(
        (64..=128).contains(&slots),
        "trace slots {slots} bytes {}",
        capped.trace_bytes
    );
    assert!(capped.messages >= sent);
    assert!(capped.bytes_read >= base.bytes_read + sent * 6);

    for _ in 0..3 {
        peer.write(frame(b'C', b"x"));
        let got = recv(&mut conn).await;
        assert_eq!(got.contents().as_ref(), b"x");
    }
    let later = conn.incoming_allocation();
    trace("event-cap-later", &later);
    assert_eq!(later.events.len(), 64);
    assert!(later.events_dropped > capped.events_dropped);
    assert_eq!(later.trace_bytes, capped.trace_bytes);
    assert_eq!(later.messages, capped.messages + 3);
    assert_eq!(later.bytes_read, capped.bytes_read + 18);
    assert_eq!(later.events[0].bytes_returned, first.bytes_returned);
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn huge_prefix_reject_does_not_reserve_body() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(bounds(1_000, 0, 0, 0)))
        .expect("set");
    let base = conn.incoming_allocation();
    peer.write(header(b'C', 2_000_000_000));
    let err = recv_err(&mut conn).await;
    assert_wire(&err, IncomingWireError::Limit(PgIncomingLimit::Message));
    let meter = conn.incoming_allocation();
    trace("huge-prefix", &meter);
    assert_eq!(meter.bytes_read - base.bytes_read, 5);
    assert_eq!(meter.reserved_additional, base.reserved_additional);
    assert_eq!(meter.reserve_argument_bytes, base.reserve_argument_bytes);
    assert_eq!(meter.reserve_calls, base.reserve_calls);
    assert_eq!(meter.noop_reserve_arguments, base.noop_reserve_arguments);
    assert_eq!(meter.last_logical_need, 5);
    assert!(meter.logical_requested_peak < 2_000_000_000);
    assert!(meter.view_capacity_peak < 2_000_000_000);
    assert!(meter.events.iter().all(|event| {
        event.reserve_argument < 1_000_000 && event.logical_need < 1_000_000
    }));
    let again = recv_err(&mut conn).await;
    assert_wire(&again, IncomingWireError::Limit(PgIncomingLimit::Message));
    let poisoned = conn.incoming_allocation();
    assert_eq!(poisoned.bytes_read, meter.bytes_read);
    assert_eq!(poisoned.reserved_additional, meter.reserved_additional);
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn fragment_cancel_keeps_bytes_and_order() {
    let (mut conn, peer) = connect_pair().await;
    let before = conn.incoming_allocation();
    let wire = frame(b'C', b"ORDER-BODY");
    assert!(wire.len() > 10);
    peer.write(wire[..4].to_vec());
    let parked = tokio::time::timeout(PARK, conn.recv_backend()).await;
    assert!(parked.is_err(), "short header completed a frame");
    let mid = conn.incoming_allocation();
    assert_eq!(mid.bytes_read - before.bytes_read, 4);
    assert!(mid.reserved_additional >= before.reserved_additional);
    peer.write(wire[4..].to_vec());
    let got = recv(&mut conn).await;
    assert_eq!(got.format_byte(), b'C');
    assert_eq!(got.contents().as_ref(), b"ORDER-BODY");
    let after = conn.incoming_allocation();
    assert_eq!(after.bytes_read - before.bytes_read, wire.len() as u64);
    assert!(after.reserved_additional >= mid.reserved_additional);
    peer.write(frame(b'C', b"NEXT"));
    let next = recv(&mut conn).await;
    assert_eq!(next.contents().as_ref(), b"NEXT");
    trace("fragment-order", &conn.incoming_allocation());
    reconcile(&conn.incoming_allocation());
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn fragmented_growth_survives_cancel() {
    let (mut conn, peer) = connect_pair().await;
    conn.set_incoming_bounds(Some(wide())).expect("set");
    let base = conn.incoming_allocation();
    let wire = sized_frame(12_000);
    peer.write(wire[..5].to_vec());
    let first = tokio::time::timeout(PARK, conn.recv_backend()).await;
    assert!(first.is_err(), "frame completed on the header");
    let reserved = conn.incoming_allocation();
    assert_eq!(reserved.bytes_read - base.bytes_read, 5);
    assert!(reserved.reserved_additional > base.reserved_additional);
    let growth = reserved.reserved_additional;

    peer.write(wire[5..105].to_vec());
    let second = tokio::time::timeout(PARK, conn.recv_backend()).await;
    assert!(second.is_err(), "frame completed on a 100-byte fragment");
    let partial = conn.incoming_allocation();
    trace("fragment-growth-partial", &partial);
    assert_eq!(partial.bytes_read - base.bytes_read, 105);
    assert_eq!(partial.reserved_additional, growth);
    assert!(
        partial.events.iter().any(|event| {
            event.read_completed == 0 && event.reserved_growth > 0 && event.bytes_returned == 0
        }),
        "cancelled growth row disappeared"
    );

    peer.write(wire[105..].to_vec());
    let got = recv(&mut conn).await;
    assert_eq!(got.contents().as_ref(), &wire[5..]);
    let done = conn.incoming_allocation();
    trace("fragment-growth-done", &done);
    assert_eq!(done.reserved_additional, growth);
    assert_eq!(done.bytes_read - base.bytes_read, 12_000);
    assert!(
        done.events.iter().any(|event| {
            event.read_completed == 0 && event.reserved_growth > 0 && event.bytes_returned == 0
        }),
        "success rewrote the cancelled reserve"
    );
    reconcile(&done);
    drop(peer);
}

#[tokio::test(flavor = "current_thread")]
async fn poison_stops_reads_without_clearing_on_unset() {
    let (mut conn, peer) = connect_pair().await;
    let base = conn.incoming_allocation();
    peer.write(header(b'I', 3));
    let err = recv_err(&mut conn).await;
    assert_wire(&err, IncomingWireError::ShortLength);
    let mid = conn.incoming_allocation();
    trace("poison", &mid);
    assert_eq!(mid.bytes_read - base.bytes_read, 5);
    assert_eq!(mid.reserved_additional, base.reserved_additional);
    assert!(!conn.incoming_synchronized());
    peer.write(frame(b'C', b"AFTER"));
    let again = recv_err(&mut conn).await;
    assert_wire(&again, IncomingWireError::ShortLength);
    let held = conn.incoming_allocation();
    assert_eq!(held.bytes_read, mid.bytes_read);
    assert_eq!(held.reserved_additional, mid.reserved_additional);
    assert_eq!(held.reserve_calls, mid.reserve_calls);
    assert_eq!(held.reads, mid.reads);
    conn.set_incoming_bounds(None).expect("unset");
    assert!(!conn.incoming_synchronized());
    let third = recv_err(&mut conn).await;
    assert_wire(&third, IncomingWireError::ShortLength);
    let final_meter = conn.incoming_allocation();
    assert_eq!(final_meter.bytes_read, mid.bytes_read);
    assert_eq!(final_meter.reserved_additional, mid.reserved_additional);
    assert_eq!(final_meter.events.len(), mid.events.len());
    drop(peer);
}

fn reconcile(meter: &PgIncomingAllocation) {
    assert_eq!(meter.events_dropped, 0, "dropped diagnostic rows");
    let growth: u64 = meter.events.iter().map(|event| event.reserved_growth).sum();
    let assembly: u64 = meter.events.iter().map(|event| event.assembly_bytes).sum();
    let arguments: u64 = meter.events.iter().map(|event| event.reserve_argument).sum();
    let noop_arguments: u64 = meter
        .events
        .iter()
        .filter(|event| event.reserved_growth == 0)
        .map(|event| event.reserve_argument)
        .sum();
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
    assert_eq!(meter.reserved_additional, growth, "growth counter");
    assert_eq!(meter.assembly_bytes, assembly, "assembly counter");
    assert_eq!(meter.reserve_argument_bytes, arguments, "argument counter");
    assert_eq!(meter.noop_reserve_arguments, noop_arguments, "noop arguments");
    assert_eq!(meter.reserve_calls, calls, "reserve calls");
    assert_eq!(meter.noop_reserve_calls, noops, "noop calls");
    assert_eq!(meter.bytes_read, returned, "bytes read");
    assert_eq!(meter.rounding_peak, measured_slack(meter), "rounding");
    for event in &meter.events {
        assert_eq!(
            event.view_capacity_before,
            event.read_capacity_before + event.available_capacity_before,
            "{event:?}"
        );
        assert_eq!(
            event.reserved_growth,
            event.view_capacity_after - event.view_capacity_before,
            "{event:?}"
        );
        assert!(event.read_completed == 0 || event.read_completed == 1, "{event:?}");
        if event.assembly_bytes > 0 {
            assert!(
                event.assembly_bytes <= event.read_len_before + event.bytes_returned,
                "{event:?}"
            );
        }
    }
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
        "TRACE case={case} logical_peak={} reserved_growth={} reserve_arguments={} noop_arguments={} noop_calls={} reserve_calls={} view_capacity_peak={} reads={} bytes_read={} assembly={} accepted_logical={} accepted_backing={} overread={} rounding={} last_logical={} trace_bytes={} messages={} events={} dropped={}",
        meter.logical_requested_peak,
        meter.reserved_additional,
        meter.reserve_argument_bytes,
        meter.noop_reserve_arguments,
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
        meter.events_dropped,
    );
}

fn wide() -> PgIncomingBounds {
    bounds(100_000, 0, 0, 0)
}

fn bounds(message: u32, value: u32, row: u32, columns: u16) -> PgIncomingBounds {
    PgIncomingBounds::try_new(message, value, row, columns).expect("bounds")
}

fn sized_frame(total: usize) -> Vec<u8> {
    assert!(total >= 5, "frame shorter than a header");
    frame(b'C', &vec![b'B'; total - 5])
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
    assert!(!text.contains("SECRET"), "{text}");
    assert!(!debug.contains("SECRET"), "{debug}");
    assert!(!text.chars().any(|ch| ch.is_ascii_digit()), "{text}");
    assert!(!debug.chars().any(|ch| ch.is_ascii_digit()), "{debug}");
}

enum Cmd {
    Write(Vec<u8>, Sender<()>),
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
