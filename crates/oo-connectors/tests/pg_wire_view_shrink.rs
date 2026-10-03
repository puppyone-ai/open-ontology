//! Real receive path: many small frames, retained by the caller.
//!
//! Plaintext `TcpStream` reads append into spare capacity. The two live windows
//! stay contiguous, so this peer does not by itself trip the view-capacity
//! panic. It still checks order, the 64-event trace, and that cancel and EOF
//! do not roll the growth counter backwards.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use sqlx::postgres::{PgBackendFrame, PgConnectOptions, PgConnection, PgSslMode};
use sqlx::Connection;

const HANG: Duration = Duration::from_secs(10);
const PARK: Duration = Duration::from_millis(750);
const FRAMES: u16 = 1024;

#[tokio::test(flavor = "current_thread")]
async fn retained_small_frames_keep_order_and_meter() {
    let (mut conn, peer) = connect_pair().await;
    let base = conn.incoming_allocation();

    let mut burst = Vec::new();
    for id in 0..FRAMES {
        burst.extend(frame(b'D', &id.to_be_bytes()));
    }
    let mut offset = 0;
    while offset < burst.len() {
        let end = (offset + 32).min(burst.len());
        peer.write(burst[offset..end].to_vec());
        offset = end;
    }

    let mut held = Vec::with_capacity(FRAMES as usize);
    for id in 0..FRAMES {
        let got = recv(&mut conn).await;
        assert_eq!(got.format_byte(), b'D');
        assert_eq!(got.contents().as_ref(), &id.to_be_bytes());
        held.push(got);
    }
    assert_eq!(held.len(), FRAMES as usize);

    let meter = conn.incoming_allocation();
    assert_eq!(meter.messages - base.messages, u64::from(FRAMES));
    assert_eq!(
        meter.accepted_logical_payload - base.accepted_logical_payload,
        u64::from(FRAMES) * 2
    );
    assert!(meter.events.len() <= 64);
    assert!(meter.bytes_read >= base.bytes_read + burst.len() as u64);
    let stored: u64 = meter.events.iter().map(|event| event.reserved_growth).sum();
    if meter.events_dropped == 0 {
        assert_eq!(meter.reserved_additional, stored);
    } else {
        assert!(meter.reserved_additional >= stored);
    }
    for event in &meter.events {
        if event.view_capacity_after >= event.view_capacity_before {
            assert_eq!(
                event.reserved_growth,
                event.view_capacity_after - event.view_capacity_before,
                "{event:?}"
            );
        } else {
            assert_eq!(event.reserved_growth, 0, "{event:?}");
        }
        assert!(event.read_completed == 0 || event.read_completed == 1, "{event:?}");
    }

    let before_park = conn.incoming_allocation();
    let huge = frame(b'D', &vec![0x11; 80_000]);
    peer.write(huge[..5].to_vec());
    let parked = tokio::time::timeout(PARK, conn.recv_backend()).await;
    assert!(parked.is_err(), "header-only read completed");
    let mid = conn.incoming_allocation();
    assert!(mid.reserved_additional >= before_park.reserved_additional);
    assert_eq!(mid.bytes_read - before_park.bytes_read, 5);

    let before_eof = mid.reserved_additional;
    drop(peer);
    let err = recv_err(&mut conn).await;
    let text = err.to_string();
    assert!(
        text.contains("EOF") || text.contains("eof") || text.contains("bytes"),
        "{text}"
    );
    let after = conn.incoming_allocation();
    assert!(after.reserved_additional >= before_eof);
    assert_eq!(held.len(), FRAMES as usize);
}

fn frame(format: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + body.len());
    out.push(format);
    let message_len = u32::try_from(body.len() + 4).expect("message len");
    out.extend_from_slice(&message_len.to_be_bytes());
    out.extend_from_slice(body);
    out
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
