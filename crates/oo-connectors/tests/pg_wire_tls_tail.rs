//! `init_mut` receive tail. Plaintext TCP appends; rustls and async-io do not.
//!
//! `ReadBuf::init_mut` zero-fills the spare and `Read` writes `n` bytes at the
//! front of that slice. Leaving the spare committed makes the next short read
//! reserve on a full window, and bytes 1.12.1 doubles it. 512 frames is the
//! count that exhausted an 8 GiB cgroup. This file stops at 24 KiB, under the
//! old fourth-read peak of 32643 bytes. It does not shrink the frame count.

use std::future::Future;
use std::io;
use std::ops::ControlFlow;
use std::path::Path;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::Connection;
use sqlx::Row;
use sqlx_core::bytes::{Buf, BufMut, Bytes, BytesMut};
use sqlx_core::error::Error;
use sqlx_core::io::ReadBuf;
use sqlx_core::net::{BufferedSocket, Socket};

const FRAMES: usize = 512;
const CHUNK: usize = 64;
const PLATEAU: u64 = 24 * 1024;
const WARMUP: usize = 64;
const MESSAGE_CAP: usize = 32;
const PG_FUSE: u64 = 1024 * 1024;
const PG_ROWS: i64 = 60_000;
const BYTES_VERSION: &str = "1.12.1";
const BYTES_SHA256: &str = "fc652a48c352aef3ea3aed32080501cf3ef6ed5da78602a020c991775b0aff04";

#[test]
fn init_mut_frames_plateau_when_caller_drops_them() {
    let (payloads, chunks) = wire(FRAMES);
    let mut socket = BufferedSocket::new(InitPeer::new(chunks));
    let mut hasher = Sha256::new();
    let mut warmup = None;
    for (index, expected) in payloads.iter().enumerate() {
        let got = read_one(&mut socket, Some(MESSAGE_CAP)).expect("frame");
        assert_eq!(got.as_ref(), expected.as_slice(), "frame {index}");
        hasher.update(got.as_ref());
        note_peak(&socket, index, &mut warmup);
        drop(got);
    }
    let digest: [u8; 32] = hasher.finalize().into();
    assert_eq!(digest, oracle(FRAMES));
    assert_meter(&socket);
    let peak = socket.incoming_meter_parts()[5];
    assert!(peak <= PLATEAU, "view {peak}");
    eprintln!("a133_view_peak_drop={peak}");
}

#[test]
fn init_mut_frames_plateau_when_caller_retains_them() {
    let (payloads, chunks) = wire(FRAMES);
    let mut socket = BufferedSocket::new(InitPeer::new(chunks));
    let mut held = Vec::with_capacity(FRAMES);
    let mut warmup = None;
    for (index, expected) in payloads.iter().enumerate() {
        let got = read_one(&mut socket, None).expect("frame");
        assert_eq!(got.as_ref(), expected.as_slice(), "frame {index}");
        note_peak(&socket, index, &mut warmup);
        held.push(got);
    }
    let peak = socket.incoming_meter_parts()[5];
    assert_meter(&socket);
    assert!(peak <= PLATEAU, "view {peak}");
    eprintln!("a133_view_peak_retain={peak}");
    drop(socket);
    assert_eq!(held.len(), FRAMES);
    assert_eq!(hash_held(&held), oracle(FRAMES));
    assert_eq!(held[0].as_ref(), payloads[0].as_slice());
    assert_eq!(held[FRAMES - 1].as_ref(), payloads[FRAMES - 1].as_slice());
    let retained: u64 = held.iter().map(|bytes| bytes.len() as u64).sum();
    assert_eq!(retained, (FRAMES * 2) as u64);
    assert_ne!(retained, peak);
}

#[test]
fn capped_oversize_header_does_not_reserve_the_body() {
    let mut header = Vec::new();
    header.push(b'D');
    header.extend_from_slice(&100_000u32.to_be_bytes());
    let mut socket = BufferedSocket::new(InitPeer::new(vec![header]));
    let err = read_one(&mut socket, Some(MESSAGE_CAP)).expect_err("cap");
    let text = err.to_string();
    assert!(text.contains("message cap"), "{text}");
    let parts = socket.incoming_meter_parts();
    assert_eq!(parts[0], 5, "logical peak {}", parts[0]);
    assert!(parts[5] <= PLATEAU, "view {}", parts[5]);
    eprintln!("a133_view_peak_oversize={}", parts[5]);
}

#[test]
fn append_frames_keep_their_bytes() {
    let (payloads, chunks) = wire(FRAMES);
    let mut socket = BufferedSocket::new(AppendPeer::new(chunks));
    let mut held = Vec::with_capacity(FRAMES);
    for (index, expected) in payloads.iter().enumerate() {
        let got = read_one(&mut socket, None).expect("frame");
        assert_eq!(got.as_ref(), expected.as_slice(), "frame {index}");
        let peak = socket.incoming_meter_parts()[5];
        assert!(peak <= PLATEAU, "frame {index} view {peak}");
        held.push(got);
    }
    assert_meter(&socket);
    let peak = socket.incoming_meter_parts()[5];
    eprintln!("a133_view_peak_append={peak}");
    drop(socket);
    assert_eq!(hash_held(&held), oracle(FRAMES));
}

#[test]
fn init_mut_eof_then_frames_stay_inside_the_plateau() {
    let (payloads, chunks) = wire(FRAMES);
    let mut socket = BufferedSocket::new(EofThenPeer::new(chunks));
    let before = socket.incoming_meter_parts()[1];
    let err = drive(socket.try_read(|buf| take_frame(buf, None))).expect_err("eof");
    match err {
        Error::Io(err) => assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof),
        other => panic!("eof became {other}"),
    }
    let after = socket.incoming_meter_parts()[1];
    assert!(after >= before, "eof rolled reserved_additional {before} -> {after}");
    let mut warmup = None;
    let mut held = Vec::with_capacity(FRAMES);
    for (index, expected) in payloads.iter().enumerate() {
        let got = read_one(&mut socket, None).expect("frame");
        assert_eq!(got.as_ref(), expected.as_slice(), "frame {index}");
        note_peak(&socket, index, &mut warmup);
        held.push(got);
    }
    let peak = socket.incoming_meter_parts()[5];
    assert!(peak <= PLATEAU, "view {peak}");
    eprintln!("a133_view_peak_eof={peak}");
    drop(socket);
    assert_eq!(hash_held(&held), oracle(FRAMES));
}

#[test]
fn no_data_cancel_keeps_the_reserve_charge() {
    let mut socket = BufferedSocket::new(BareCancel);
    let before = socket.incoming_meter_parts()[1];
    let parked = poll_once(socket.try_read(|buf| {
        if buf.len() < 9000 {
            Ok(ControlFlow::Continue(9000))
        } else {
            Ok(ControlFlow::Break(()))
        }
    }));
    assert!(parked.is_pending(), "no-data read completed");
    drop(parked);
    let parts = socket.incoming_meter_parts();
    assert!(parts[1] >= before);
    assert!(parts[1] > 0, "9000-byte ask did not reserve");
    assert!(parts[5] <= PLATEAU, "view {}", parts[5]);
    let events = socket.incoming_meter_events();
    assert!(events.iter().any(|event| event[12] == 0), "cancel completed a row");
    eprintln!("a133_view_peak_cancel_bare={}", parts[5]);
}

#[test]
fn cancel_after_init_mut_then_frames_plateau() {
    let (payloads, chunks) = wire(FRAMES);
    let mut socket = BufferedSocket::new(CancelThenPeer::new(chunks));
    let parked = poll_once(socket.try_read(|buf| take_frame(buf, None)));
    assert!(parked.is_pending(), "init_mut cancel completed");
    drop(parked);
    let mut warmup = None;
    let mut held = Vec::with_capacity(FRAMES);
    for (index, expected) in payloads.iter().enumerate() {
        let got = read_one(&mut socket, None).expect("frame");
        assert_eq!(got.as_ref(), expected.as_slice(), "frame {index}");
        note_peak(&socket, index, &mut warmup);
        held.push(got);
    }
    let peak = socket.incoming_meter_parts()[5];
    assert!(peak <= PLATEAU, "view {peak}");
    assert_meter(&socket);
    eprintln!("a133_view_peak_cancel_init={peak}");
    drop(socket);
    assert_eq!(hash_held(&held), oracle(FRAMES));
}

#[test]
fn bytes_workspace_contract_and_product_blob() {
    let (version, checksum) = bytes_lock_identity();
    assert_eq!(version, BYTES_VERSION);
    assert_eq!(checksum, BYTES_SHA256);
    let mut buf = BytesMut::with_capacity(8);
    let cap = buf.capacity();
    buf.resize(cap, 0);
    assert_eq!(buf.len(), buf.capacity());
    assert_eq!(buf.remaining_mut(), isize::MAX as usize - buf.len());
    assert!(buf.remaining_mut() > 0);

    match std::env::var("OO_A133_PRODUCT") {
        Ok(path) => {
            let bytes = std::fs::read(&path).expect("product blob");
            let text = String::from_utf8_lossy(&bytes);
            assert!(
                text.contains("fn committed_init_len"),
                "product blob has no tail correction"
            );
            let digest = sha256_hex(&bytes);
            eprintln!("a133_product_sha256={digest}");
            eprintln!("a133_bytes_version={version}");
            eprintln!("a133_bytes_sha256={checksum}");
        }
        Err(_) if std::env::var_os("OO_A133_REQUIRE_PRODUCT").is_some() => {
            panic!("OO_A133_PRODUCT is required");
        }
        Err(_) => {}
    }
}

#[tokio::test(flavor = "current_thread")]
async fn tls_pg_sixty_thousand_small_rows() {
    let path = std::env::var("OO_A133_TLS_FIXTURE").expect("OO_A133_TLS_FIXTURE");
    let raw = std::fs::read_to_string(&path).expect("fixture pointer");
    let fixture: Fixture = serde_json::from_str(&raw).expect("fixture json");
    let admin_password = std::fs::read_to_string(&fixture.admin_password_file).expect("admin pass");
    let reader_password =
        std::fs::read_to_string(&fixture.reader_password_file).expect("reader pass");
    let admin_password = admin_password.trim().to_string();
    let reader_password = reader_password.trim().to_string();
    assert!(
        !reader_password.is_empty() && !reader_password.contains('\''),
        "reader password is not a hex token"
    );
    assert!(
        fixture
            .reader_user
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_'),
        "reader user is not a plain ident"
    );
    let secrets = [admin_password.as_str(), reader_password.as_str()];

    let mut admin = connect(
        &fixture,
        &fixture.admin_user,
        &admin_password,
        "oo-a133-admin",
        &secrets,
    )
    .await;
    exec(
        &mut admin,
        "SET log_statement = 'none'",
        &secrets,
    )
    .await;
    exec(&mut admin, "DROP SCHEMA IF EXISTS oo_a133 CASCADE", &secrets).await;
    exec(
        &mut admin,
        &format!("DROP ROLE IF EXISTS {}", fixture.reader_user),
        &secrets,
    )
    .await;
    exec(&mut admin, "CREATE SCHEMA oo_a133", &secrets).await;
    exec(
        &mut admin,
        "CREATE TABLE oo_a133.small (id int4 PRIMARY KEY)",
        &secrets,
    )
    .await;
    exec(
        &mut admin,
        "INSERT INTO oo_a133.small (id) SELECT generate_series(1, 60000)",
        &secrets,
    )
    .await;
    let role = format!(
        "CREATE ROLE {} LOGIN PASSWORD '{reader_password}'",
        fixture.reader_user
    );
    exec(&mut admin, &role, &secrets).await;
    exec(
        &mut admin,
        &format!("GRANT USAGE ON SCHEMA oo_a133 TO {}", fixture.reader_user),
        &secrets,
    )
    .await;
    exec(
        &mut admin,
        &format!(
            "GRANT SELECT ON TABLE oo_a133.small TO {}",
            fixture.reader_user
        ),
        &secrets,
    )
    .await;
    // `log_statement` is superuser-only. The reader cannot SET it. The admin
    // records it on the role after the password statement, so the reader
    // session logs its own SQL and does not issue a write.
    exec(
        &mut admin,
        &format!(
            "ALTER ROLE {} SET log_statement = 'all'",
            fixture.reader_user
        ),
        &secrets,
    )
    .await;
    drop(admin);

    let mut reader = connect(
        &fixture,
        &fixture.reader_user,
        &reader_password,
        "oo-a133-reader",
        &secrets,
    )
    .await;
    assert!(reader.incoming_bounds().is_none());
    let rows = tokio::time::timeout(Duration::from_secs(60), async {
        sqlx::query("SELECT id FROM oo_a133.small ORDER BY id")
            .fetch_all(&mut reader)
            .await
    })
    .await
    .unwrap_or_else(|_| panic!("read exceeded 60s"));
    let rows = match rows {
        Ok(rows) => rows,
        Err(err) => panic!("{}", redact(&err.to_string(), &secrets)),
    };
    assert_eq!(rows.len() as i64, PG_ROWS);
    let mut hasher = Sha256::new();
    for (index, row) in rows.iter().enumerate() {
        let id: i32 = row.try_get(0).expect("id");
        assert_eq!(id as i64, index as i64 + 1, "row {index}");
        hasher.update(id.to_be_bytes());
    }
    let digest: [u8; 32] = hasher.finalize().into();
    assert_eq!(digest, pg_oracle_hash(PG_ROWS));

    let agg = sqlx::query(
        "SELECT count(*)::int8, coalesce(sum(id)::int8, 0), coalesce(bit_xor(id::int8), 0) \
         FROM oo_a133.small",
    )
    .fetch_one(&mut reader)
    .await;
    let agg = match agg {
        Ok(row) => row,
        Err(err) => panic!("{}", redact(&err.to_string(), &secrets)),
    };
    let count: i64 = agg.try_get(0).expect("count");
    let sum: i64 = agg.try_get(1).expect("sum");
    let bits: i64 = agg.try_get(2).expect("bit_xor");
    assert_eq!(count, PG_ROWS);
    assert_eq!(sum, PG_ROWS * (PG_ROWS + 1) / 2);
    assert_eq!(sum, 1_800_030_000);
    assert_eq!(bits, pg_bit_xor(PG_ROWS));

    let peak = reader.incoming_allocation().view_capacity_peak;
    assert!(peak < PG_FUSE, "view {peak}");
    let rss = rss_kib();
    assert!(rss < 512 * 1024, "rss {rss} KiB");
    drop(reader);
    let (sql_lines, write_lines) = reader_sql(&fixture.log_file, &secrets).await;
    assert!(sql_lines > 0, "reader SQL log is empty");
    assert_eq!(write_lines, 0, "reader write statements {write_lines}");
    eprintln!("a133_pg_rows={PG_ROWS}");
    eprintln!("a133_pg_view_peak={peak}");
    eprintln!("a133_pg_rss_kib={rss}");
    eprintln!("a133_pg_hash={}", sha256_hex(&digest));
    eprintln!("a133_pg_reader_sql_lines={sql_lines}");
    eprintln!("a133_pg_reader_write_lines={write_lines}");
}

#[derive(serde::Deserialize)]
struct Fixture {
    host: String,
    port: u16,
    database: String,
    admin_user: String,
    reader_user: String,
    admin_password_file: String,
    reader_password_file: String,
    ca_file: String,
    log_file: String,
}

async fn connect(
    fixture: &Fixture,
    user: &str,
    password: &str,
    app: &str,
    secrets: &[&str],
) -> PgConnection {
    let options = PgConnectOptions::from_explicit()
        .host(&fixture.host)
        .port(fixture.port)
        .username(user)
        .password(password)
        .database(&fixture.database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert(Path::new(&fixture.ca_file))
        .application_name(app);
    match tokio::time::timeout(Duration::from_secs(20), PgConnection::connect_with(&options)).await
    {
        Ok(Ok(conn)) => conn,
        Ok(Err(err)) => panic!("{}", redact(&err.to_string(), secrets)),
        Err(_) => panic!("{app} connect exceeded 20s"),
    }
}

async fn exec(conn: &mut PgConnection, sql: &str, secrets: &[&str]) {
    // `query` only accepts a `'static` string. The role name is a checked ident.
    if let Err(err) = sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(&mut *conn)
        .await
    {
        panic!("{}", redact(&err.to_string(), secrets));
    }
}

async fn reader_sql(path: &str, secrets: &[&str]) -> (usize, usize) {
    let mut sql_lines = 0usize;
    let mut write_lines = 0usize;
    for _ in 0..20 {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let redacted = redact(&text, secrets);
        assert!(!redacted.contains("[redacted]"), "reader log contains a secret");
        sql_lines = 0;
        write_lines = 0;
        let mut saw_table = false;
        for line in redacted.lines() {
            if !line.contains("app=oo-a133-reader") {
                continue;
            }
            if !line.contains("statement:") && !line.contains("execute ") && !line.contains("parse ")
            {
                continue;
            }
            sql_lines += 1;
            if line.contains("SELECT") && line.contains("oo_a133.small") {
                saw_table = true;
            }
            for word in [
                "INSERT", "UPDATE", "DELETE", "TRUNCATE", "MERGE", "COPY", "CREATE", "ALTER",
                "DROP", "GRANT", "REVOKE",
            ] {
                if has_word(line, word) {
                    write_lines += 1;
                    break;
                }
            }
        }
        if sql_lines > 0 && saw_table {
            return (sql_lines, write_lines);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    (sql_lines, write_lines)
}

fn has_word(line: &str, word: &str) -> bool {
    line.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|part| part.eq_ignore_ascii_case(word))
}

fn redact(text: &str, secrets: &[&str]) -> String {
    let mut out = text.to_string();
    for secret in secrets {
        if secret.len() >= 8 {
            out = out.replace(secret, "[redacted]");
        }
    }
    out
}

fn rss_kib() -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").expect("status");
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            return rest
                .split_whitespace()
                .next()
                .expect("rss")
                .parse()
                .expect("rss number");
        }
    }
    panic!("VmRSS missing");
}

fn pg_oracle_hash(rows: i64) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for id in 1..=rows {
        hasher.update((id as i32).to_be_bytes());
    }
    hasher.finalize().into()
}

fn pg_bit_xor(rows: i64) -> i64 {
    let mut bits = 0i64;
    for id in 1..=rows {
        bits ^= id;
    }
    bits
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn bytes_lock_identity() -> (String, String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    let text = std::fs::read_to_string(&path).expect("workspace lock");
    let mut version = None;
    let mut checksum = None;
    let mut in_bytes = false;
    for line in text.lines() {
        if line.starts_with("name = ") {
            in_bytes = line == "name = \"bytes\"";
            continue;
        }
        if !in_bytes {
            continue;
        }
        if let Some(rest) = line.trim().strip_prefix("version = \"") {
            version = Some(rest.trim_end_matches('"').to_string());
        }
        if let Some(rest) = line.trim().strip_prefix("checksum = \"") {
            checksum = Some(rest.trim_end_matches('"').to_string());
            break;
        }
    }
    (
        version.expect("bytes version"),
        checksum.expect("bytes checksum"),
    )
}

struct Chunks {
    chunks: Vec<Vec<u8>>,
    at: usize,
    rest: Vec<u8>,
}

impl Chunks {
    fn new(chunks: Vec<Vec<u8>>) -> Self {
        Self {
            chunks,
            at: 0,
            rest: Vec::new(),
        }
    }

    fn pull(&mut self, room: usize) -> io::Result<Vec<u8>> {
        if self.rest.is_empty() {
            let Some(src) = self.chunks.get(self.at) else {
                return Err(io::ErrorKind::WouldBlock.into());
            };
            self.rest.extend_from_slice(src);
            self.at += 1;
        }
        if room == 0 || self.rest.is_empty() {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let n = room.min(self.rest.len());
        let out = self.rest[..n].to_vec();
        self.rest.drain(..n);
        Ok(out)
    }

    fn ready(&self) -> bool {
        !self.rest.is_empty() || self.at < self.chunks.len()
    }
}

struct InitPeer {
    chunks: Chunks,
}

impl InitPeer {
    fn new(chunks: Vec<Vec<u8>>) -> Self {
        Self {
            chunks: Chunks::new(chunks),
        }
    }
}

impl Socket for InitPeer {
    fn try_read(&mut self, buf: &mut dyn ReadBuf) -> io::Result<usize> {
        let dst = buf.init_mut();
        let got = self.chunks.pull(dst.len())?;
        dst[..got.len()].copy_from_slice(&got);
        Ok(got.len())
    }

    fn try_write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    fn poll_read_ready(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready_or_eof(self.chunks.ready())
    }

    fn poll_write_ready(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

struct AppendPeer {
    chunks: Chunks,
}

impl AppendPeer {
    fn new(chunks: Vec<Vec<u8>>) -> Self {
        Self {
            chunks: Chunks::new(chunks),
        }
    }
}

impl Socket for AppendPeer {
    fn try_read(&mut self, buf: &mut dyn ReadBuf) -> io::Result<usize> {
        let room = buf.chunk_mut().len();
        let got = self.chunks.pull(room)?;
        buf.put_slice(&got);
        Ok(got.len())
    }

    fn try_write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    fn poll_read_ready(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready_or_eof(self.chunks.ready())
    }

    fn poll_write_ready(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

struct EofThenPeer {
    chunks: Chunks,
    eof_done: bool,
}

impl EofThenPeer {
    fn new(chunks: Vec<Vec<u8>>) -> Self {
        Self {
            chunks: Chunks::new(chunks),
            eof_done: false,
        }
    }
}

impl Socket for EofThenPeer {
    fn try_read(&mut self, buf: &mut dyn ReadBuf) -> io::Result<usize> {
        if !self.eof_done {
            let _ = buf.init_mut();
            self.eof_done = true;
            return Ok(0);
        }
        let dst = buf.init_mut();
        let got = self.chunks.pull(dst.len())?;
        dst[..got.len()].copy_from_slice(&got);
        Ok(got.len())
    }

    fn try_write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    fn poll_read_ready(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_write_ready(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

struct CancelThenPeer {
    chunks: Chunks,
    phase: u8,
}

impl CancelThenPeer {
    fn new(chunks: Vec<Vec<u8>>) -> Self {
        Self {
            chunks: Chunks::new(chunks),
            phase: 0,
        }
    }
}

impl Socket for CancelThenPeer {
    fn try_read(&mut self, buf: &mut dyn ReadBuf) -> io::Result<usize> {
        if self.phase == 0 {
            let _ = buf.init_mut();
            self.phase = 1;
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let dst = buf.init_mut();
        let got = self.chunks.pull(dst.len())?;
        dst[..got.len()].copy_from_slice(&got);
        Ok(got.len())
    }

    fn try_write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    fn poll_read_ready(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.phase == 1 {
            self.phase = 2;
            Poll::Pending
        } else {
            ready_or_eof(self.chunks.ready())
        }
    }

    fn poll_write_ready(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

struct BareCancel;

impl Socket for BareCancel {
    fn try_read(&mut self, _buf: &mut dyn ReadBuf) -> io::Result<usize> {
        Err(io::ErrorKind::WouldBlock.into())
    }

    fn try_write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    fn poll_read_ready(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Pending
    }

    fn poll_write_ready(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

fn ready_or_eof(ready: bool) -> Poll<io::Result<()>> {
    if ready {
        Poll::Ready(Ok(()))
    } else {
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "chunks exhausted",
        )))
    }
}

fn wire(frames: usize) -> (Vec<[u8; 2]>, Vec<Vec<u8>>) {
    let mut payloads = Vec::with_capacity(frames);
    let mut bytes = Vec::new();
    for id in 0..frames {
        let body = (id as u16).to_be_bytes();
        payloads.push(body);
        bytes.extend(frame(&body));
    }
    let chunks = bytes.chunks(CHUNK).map(|chunk| chunk.to_vec()).collect();
    (payloads, chunks)
}

fn frame(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + body.len());
    out.push(b'D');
    let message_len = u32::try_from(body.len() + 4).expect("len");
    out.extend_from_slice(&message_len.to_be_bytes());
    out.extend_from_slice(body);
    out
}

fn take_frame(
    buf: &mut BytesMut,
    cap: Option<usize>,
) -> Result<ControlFlow<Bytes, usize>, Error> {
    if buf.len() < 5 {
        return Ok(ControlFlow::Continue(5));
    }
    let message_len = u32::from_be_bytes(buf[1..5].try_into().expect("len")) as usize;
    if cap.is_some_and(|limit| message_len > limit) {
        return Err(Error::Protocol("message cap".to_string()));
    }
    let expected_len = message_len + 1;
    if buf.len() < expected_len {
        return Ok(ControlFlow::Continue(expected_len));
    }
    buf.advance(1);
    let mut contents = buf.split_to(message_len).freeze();
    contents.advance(4);
    Ok(ControlFlow::Break(contents))
}

fn read_one<S: Socket>(
    socket: &mut BufferedSocket<S>,
    cap: Option<usize>,
) -> Result<Bytes, Error> {
    drive(socket.try_read(move |buf| take_frame(buf, cap)))
}

fn note_peak<S: Socket>(
    socket: &BufferedSocket<S>,
    index: usize,
    warmup: &mut Option<u64>,
) -> u64 {
    let peak = socket.incoming_meter_parts()[5];
    assert!(peak <= PLATEAU, "frame {index} view {peak}");
    if index + 1 == WARMUP {
        *warmup = Some(peak);
    } else if let Some(mark) = *warmup {
        assert!(
            peak <= mark,
            "frame {index} grew past warmup {mark} to {peak}"
        );
    }
    peak
}

fn assert_meter<S: Socket>(socket: &BufferedSocket<S>) {
    let parts = socket.incoming_meter_parts();
    let events = socket.incoming_meter_events();
    let events_dropped = parts[19];
    assert!(events.len() <= 64, "{}", events.len());
    assert_eq!(events.len() == 64, events_dropped > 0);
    let mut stored = 0u64;
    for event in &events {
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
    if events_dropped == 0 {
        assert_eq!(parts[1], stored);
    } else {
        assert!(parts[1] >= stored);
    }
}

fn hash_held(held: &[Bytes]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for bytes in held {
        hasher.update(bytes.as_ref());
    }
    hasher.finalize().into()
}

fn oracle(frames: usize) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for id in 0..frames {
        hasher.update((id as u16).to_be_bytes());
    }
    hasher.finalize().into()
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

fn poll_once<T>(future: impl Future<Output = T>) -> Poll<T> {
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    future.as_mut().poll(&mut context)
}
