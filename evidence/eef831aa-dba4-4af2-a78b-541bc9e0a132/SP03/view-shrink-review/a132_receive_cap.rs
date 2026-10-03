//! a132 review overlay. Not a product test and not a workspace `cargo test`.
//!
//! The copy of vendored sqlx-core is pinned to bytes 1.12.1 outside the
//! workspace. Socket reads are hard-capped at 4 chunks of 64 bytes. The
//! oversize header is 100_000 bytes and must be rejected before that body
//! is requested. This is not the 512-frame run.

use std::future::Future;
use std::io;
use std::ops::ControlFlow;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

use sqlx_core::bytes::{Buf, BufMut, Bytes};
use sqlx_core::error::Error;
use sqlx_core::io::ReadBuf;
use sqlx_core::net::{BufferedSocket, Socket};

const FRAMES: usize = 36;
const CHUNK: usize = 64;
const MAX_CHUNKS: usize = 4;
const ACCEPT_CAP: u32 = 32;
const OVERSIZE_LEN: u32 = 100_000;
const VIEW_FUSE: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy)]
enum ReadMode {
    /// `init_mut` then write `n` at the front. Same shape as `RustlsSocket::try_read`.
    InitMut,
    /// Append `n` bytes. Same shape as Tokio `try_read_buf` (no `init_mut`).
    Append,
}

struct ChunkReader {
    chunks: Vec<Vec<u8>>,
    at: usize,
    mode: ReadMode,
}

impl Socket for ChunkReader {
    fn try_read(&mut self, buf: &mut dyn ReadBuf) -> io::Result<usize> {
        let Some(src) = self.chunks.get(self.at) else {
            return Err(io::ErrorKind::WouldBlock.into());
        };
        if self.at >= MAX_CHUNKS {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let n = match self.mode {
            ReadMode::InitMut => {
                let dst = buf.init_mut();
                if dst.is_empty() || src.is_empty() {
                    return Err(io::ErrorKind::WouldBlock.into());
                }
                let n = src.len().min(dst.len());
                dst[..n].copy_from_slice(&src[..n]);
                n
            }
            ReadMode::Append => {
                if src.is_empty() {
                    return Err(io::ErrorKind::WouldBlock.into());
                }
                let n = src.len();
                buf.put_slice(&src[..n]);
                n
            }
        };
        self.at += 1;
        Ok(n)
    }

    fn try_write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    fn poll_read_ready(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.at < self.chunks.len() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Ready(Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "chunks exhausted",
            )))
        }
    }

    fn poll_write_ready(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

struct FixedRead {
    mode: FixedMode,
}

enum FixedMode {
    Eof,
    Pending,
}

impl Socket for FixedRead {
    fn try_read(&mut self, buf: &mut dyn ReadBuf) -> io::Result<usize> {
        match self.mode {
            FixedMode::Eof => {
                let _ = buf.init_mut();
                Ok(0)
            }
            FixedMode::Pending => Err(io::ErrorKind::WouldBlock.into()),
        }
    }

    fn try_write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    fn poll_read_ready(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.mode {
            FixedMode::Eof => Poll::Ready(Ok(())),
            FixedMode::Pending => Poll::Pending,
        }
    }

    fn poll_write_ready(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

fn drive<T>(future: impl Future<Output = T>) -> T {
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    for _ in 0..64 {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
    }
    panic!("receive stalled");
}

fn frame(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + body.len());
    out.push(b'D');
    let message_len = u32::try_from(body.len() + 4).expect("len");
    out.extend_from_slice(&message_len.to_be_bytes());
    out.extend_from_slice(body);
    out
}

fn small_wire() -> (Vec<[u8; 2]>, Vec<Vec<u8>>) {
    let mut payloads = Vec::with_capacity(FRAMES);
    let mut wire = Vec::new();
    for id in 0..FRAMES {
        let body = (id as u16).to_be_bytes();
        payloads.push(body);
        wire.extend(frame(&body));
    }
    let chunks: Vec<Vec<u8>> = wire.chunks(CHUNK).map(|chunk| chunk.to_vec()).collect();
    assert_eq!(chunks.len(), MAX_CHUNKS, "chunk fuse {}", chunks.len());
    assert!(wire.len() <= MAX_CHUNKS * CHUNK);
    (payloads, chunks)
}

/// Message-length gate from `PgStream::inspect`: a length above the cap returns
/// before `Continue(expected)`. This does not construct `PgStream`.
fn take_frame(buf: &mut sqlx_core::bytes::BytesMut, cap: Option<u32>) -> Result<ControlFlow<Bytes, usize>, Error> {
    if buf.len() < 5 {
        return Ok(ControlFlow::Continue(5));
    }
    let message_len = u32::from_be_bytes(buf[1..5].try_into().expect("len"));
    if message_len < 4 {
        return Err(Error::Protocol("short length".to_owned()));
    }
    if let Some(cap) = cap {
        if message_len > cap {
            return Err(Error::Protocol(format!("cap reject {message_len}")));
        }
    }
    let expected = (message_len as usize)
        .checked_add(1)
        .ok_or_else(|| Error::Protocol("overflow".to_owned()))?;
    if expected > 64 {
        return Err(Error::Protocol(format!("fuse refuse {expected}")));
    }
    if buf.len() < expected {
        return Ok(ControlFlow::Continue(expected));
    }
    buf.advance(1);
    let mut contents = buf.split_to(message_len as usize).freeze();
    contents.advance(4);
    Ok(ControlFlow::Break(contents))
}

struct Measured {
    name: &'static str,
    parts: [u64; 20],
    events: Vec<[u64; 13]>,
    samples: Vec<u64>,
    held: usize,
}

fn print_measured(measured: &Measured) {
    let parts = measured.parts;
    println!(
        "CASE {} reserved_additional={} reserve_calls={} view_capacity_peak={} read_capacity_peak={} available_capacity_peak={} rounding_peak={} reads={} bytes_read={} assembly_bytes={} logical_peak={} events_dropped={} events={} held={} samples={:?}",
        measured.name,
        parts[1],
        parts[2],
        parts[5],
        parts[3],
        parts[4],
        parts[6],
        parts[8],
        parts[9],
        parts[13],
        parts[0],
        parts[19],
        measured.events.len(),
        measured.held,
        measured.samples,
    );
    for (index, event) in measured.events.iter().enumerate() {
        println!(
            "STEP {} i={} logical={} reserve_arg={} growth={} view_before={} view_after={} avail_len_before={} avail_cap_after={} returned={} assembly={} completed={}",
            measured.name,
            index,
            event[0],
            event[1],
            event[2],
            event[7],
            event[8],
            event[5],
            event[6],
            event[10],
            event[11],
            event[12],
        );
    }
}

fn assert_meter(measured: &Measured) {
    assert!(measured.parts[5] < VIEW_FUSE, "{} view peak over fuse", measured.name);
    assert_eq!(measured.parts[19], 0, "{} dropped a row", measured.name);
    assert!(measured.parts[0] <= 9000, "{} logical {}", measured.name, measured.parts[0]);
    let mut stored = 0u64;
    for event in &measured.events {
        let growth = event[2];
        let before = event[7];
        let after = event[8];
        let completed = event[12];
        assert!(completed == 0 || completed == 1);
        if after >= before {
            assert_eq!(growth, after - before, "{event:?}");
        } else {
            assert_eq!(growth, 0, "{event:?}");
        }
        stored += growth;
    }
    assert_eq!(measured.parts[1], stored, "{} counter vs rows", measured.name);
    let mut previous = 0u64;
    for sample in &measured.samples {
        assert!(
            *sample >= previous,
            "{} counter fell {previous} -> {sample}",
            measured.name
        );
        previous = *sample;
    }
}

fn run_frames(name: &'static str, mode: ReadMode, cap: Option<u32>, hold: bool) -> Measured {
    let (payloads, chunks) = small_wire();
    let mut socket = BufferedSocket::new(ChunkReader {
        chunks,
        at: 0,
        mode,
    });
    let mut held: Vec<Bytes> = Vec::new();
    let mut samples = Vec::new();
    for expected in &payloads {
        let got = drive(socket.try_read(|buf| take_frame(buf, cap))).expect(name);
        assert_eq!(got.as_ref(), expected.as_slice(), "{name} frame {}", held.len());
        socket.note_message_accepted(got.len());
        samples.push(socket.incoming_meter_parts()[1]);
        if hold {
            held.push(got);
        }
    }
    let measured = Measured {
        name,
        parts: socket.incoming_meter_parts(),
        events: socket.incoming_meter_events(),
        samples,
        held: held.len(),
    };
    print_measured(&measured);
    assert_meter(&measured);
    assert!(
        measured.parts[8] <= MAX_CHUNKS as u64,
        "{name} reads {}",
        measured.parts[8]
    );
    if hold {
        assert_eq!(measured.held, FRAMES);
    }
    measured
}

fn run_oversize() -> Measured {
    let mut header = vec![b'D'];
    header.extend_from_slice(&OVERSIZE_LEN.to_be_bytes());
    assert_eq!(header.len(), 5);
    let mut socket = BufferedSocket::new(ChunkReader {
        chunks: vec![header],
        at: 0,
        mode: ReadMode::InitMut,
    });
    let error = drive(socket.try_read(|buf| take_frame(buf, Some(ACCEPT_CAP)))).expect_err("oversize");
    let message = match &error {
        Error::Protocol(message) => message.clone(),
        other => panic!("oversize error variant {other:?}"),
    };
    assert!(message.contains("cap reject"), "{message}");
    let measured = Measured {
        name: "oversize_cap32",
        parts: socket.incoming_meter_parts(),
        events: socket.incoming_meter_events(),
        samples: vec![socket.incoming_meter_parts()[1]],
        held: 0,
    };
    print_measured(&measured);
    assert_meter(&measured);
    assert_eq!(measured.parts[0], 5, "logical peak asked for the body");
    assert!(
        measured.parts[5] < 32 * 1024,
        "oversize view {} reserved the body",
        measured.parts[5]
    );
    assert!(measured.parts[8] <= 1, "oversize reads {}", measured.parts[8]);
    measured
}

fn run_eof() -> Measured {
    let mut socket = BufferedSocket::new(FixedRead { mode: FixedMode::Eof });
    let error = drive(socket.try_read(|buf| {
        if buf.len() >= 9000 {
            return Ok(ControlFlow::Break(()));
        }
        Ok(ControlFlow::Continue(9000))
    }))
    .expect_err("eof");
    let Error::Io(io_error) = &error else {
        panic!("eof variant {error:?}");
    };
    assert_eq!(io_error.kind(), io::ErrorKind::UnexpectedEof);
    let measured = Measured {
        name: "eof_reserve",
        parts: socket.incoming_meter_parts(),
        events: socket.incoming_meter_events(),
        samples: vec![socket.incoming_meter_parts()[1]],
        held: 0,
    };
    print_measured(&measured);
    assert_eq!(measured.events.len(), 1);
    let event = measured.events[0];
    assert_eq!(event[12], 1, "eof did not finish the row");
    assert_eq!(event[10], 0, "eof reported payload bytes");
    assert!(event[2] > 0, "eof reserve did not grow");
    assert_eq!(measured.parts[1], event[2], "eof dropped the reserve charge");
    assert_meter(&measured);
    measured
}

fn run_cancel() -> Measured {
    let mut socket = BufferedSocket::new(FixedRead {
        mode: FixedMode::Pending,
    });
    let future = socket.try_read(|buf| {
        if buf.len() >= 9000 {
            return Ok(ControlFlow::Break(()));
        }
        Ok(ControlFlow::Continue(9000))
    });
    let mut future = Pin::from(Box::pin(future));
    let mut context = Context::from_waker(Waker::noop());
    let polled = future.as_mut().poll(&mut context);
    assert!(polled.is_pending(), "cancel read completed: {polled:?}");
    drop(future);
    let measured = Measured {
        name: "cancel_reserve",
        parts: socket.incoming_meter_parts(),
        events: socket.incoming_meter_events(),
        samples: vec![socket.incoming_meter_parts()[1]],
        held: 0,
    };
    print_measured(&measured);
    assert_eq!(measured.events.len(), 1);
    let event = measured.events[0];
    assert_eq!(event[12], 0, "cancel finished the row");
    assert_eq!(event[10], 0);
    assert!(event[2] > 0, "cancel reserve did not grow");
    assert_eq!(measured.parts[1], event[2], "cancel dropped the reserve charge");
    assert_meter(&measured);
    measured
}

#[test]
fn receive_cap_does_not_stop_init_mut_growth() {
    println!("PROBE_BEGIN frames={FRAMES} chunk={CHUNK} max_chunks={MAX_CHUNKS} accept_cap={ACCEPT_CAP}");
    let init_drop = run_frames("init_mut_drop", ReadMode::InitMut, None, false);
    let init_hold = run_frames("init_mut_retain", ReadMode::InitMut, None, true);
    let capped = run_frames("init_mut_cap32_drop", ReadMode::InitMut, Some(ACCEPT_CAP), false);
    let append = run_frames("append_drop", ReadMode::Append, None, false);
    let oversize = run_oversize();
    let eof = run_eof();
    let cancel = run_cancel();

    let cap_relation = match capped.parts[5].cmp(&init_drop.parts[5]) {
        std::cmp::Ordering::Equal => "SAME",
        std::cmp::Ordering::Less => "LOWER",
        std::cmp::Ordering::Greater => "HIGHER",
    };
    let contract_relation = match init_drop.parts[5].cmp(&append.parts[5]) {
        std::cmp::Ordering::Greater => "LARGER",
        std::cmp::Ordering::Equal => "SAME",
        std::cmp::Ordering::Less => "SMALLER",
    };
    println!("CLASS cap_vs_uncapped={cap_relation}");
    println!("CLASS init_mut_vs_append={contract_relation}");
    println!(
        "CLASS init_mut_drop_peak={} init_mut_retain_peak={} capped_peak={} append_peak={} oversize_peak={} eof_charge={} cancel_charge={}",
        init_drop.parts[5],
        init_hold.parts[5],
        capped.parts[5],
        append.parts[5],
        oversize.parts[5],
        eof.parts[1],
        cancel.parts[1],
    );
    let grew = init_drop
        .events
        .windows(2)
        .any(|pair| pair[1][8] > pair[0][8]);
    println!(
        "CLASS init_mut_view_rose_across_reads={}",
        if grew { "YES" } else { "NO" }
    );
    println!("PROBE_END");
}
