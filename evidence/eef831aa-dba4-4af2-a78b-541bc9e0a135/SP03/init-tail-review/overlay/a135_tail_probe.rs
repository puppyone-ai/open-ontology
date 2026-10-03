//! Independent probe of the init_mut tail truncation.
//!
//! The product file is the archived blob. This test does not edit it.
//! 512 short frames is the shape that exhausted an 8 GiB cgroup before the
//! truncation. The fuse here is 24 KiB, under that fourth-read peak. This
//! file does not allocate that cgroup.

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
use sqlx_core::bytes::{Buf, Bytes, BytesMut};
use sqlx_core::error::Error;
use sqlx_core::io::ReadBuf;
use sqlx_core::net::{BufferedSocket, Socket};

const FRAMES: usize = 512;
const CHUNK: usize = 64;
const PLATEAU: u64 = 24 * 1024;
const WARMUP: usize = 64;
const MESSAGE_CAP: usize = 32;
const PRODUCT_SHA256: &str = "b56fe942de40260bfaf39e83f32af8630ded0d739bc758480940099124cfe7d5";
const BYTES_VERSION: &str = "1.12.1";
const BYTES_SHA256: &str = "fc652a48c352aef3ea3aed32080501cf3ef6ed5da78602a020c991775b0aff04";
const PG_VIEW_FUSE: u64 = 1024 * 1024;
const PG_RSS_FUSE_KIB: u64 = 512 * 1024;

#[test]
fn product_blob_and_bytes_1_12_1() {
    let (version, checksum) = bytes_lock_identity();
    assert_eq!(version, BYTES_VERSION);
    assert_eq!(checksum, BYTES_SHA256);
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("vendor/sqlx-core/src/net/socket/buffered.rs");
    let bytes = std::fs::read(&path).expect("product blob");
    let digest = sha256_hex(&bytes);
    assert_eq!(digest, PRODUCT_SHA256);
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("fn committed_init_len"));
    assert!(text.contains("n.max(pre_len)"));
    eprintln!("a135_product_sha256={digest}");
    eprintln!("a135_bytes_version={version}");
    eprintln!("a135_bytes_sha256={checksum}");
}

#[test]
fn cap_set_512_dropped_and_retained_plateau() {
    let (_payloads, chunks) = wire(FRAMES);
    let dropped = run_frames(InitPeer::ready(chunks.clone()), Some(MESSAGE_CAP), false);
    let retained = run_frames(InitPeer::ready(chunks), Some(MESSAGE_CAP), true);
    assert_eq!(dropped.digest, oracle(FRAMES));
    assert_eq!(retained.digest, oracle(FRAMES));
    assert_eq!(dropped.digest, retained.digest);
    assert_eq!(retained.retained_payload, (FRAMES * 2) as u64);
    assert_ne!(dropped.peak, retained.retained_payload);
    assert_ne!(retained.peak, retained.retained_payload);
    assert_eq!(dropped.available_len_max, 0, "dropped tail leaked");
    assert_eq!(retained.available_len_max, 0, "retained tail leaked");
    assert!(dropped.reserved_nondecreasing);
    assert!(retained.reserved_nondecreasing);
    eprintln!("a135_view_peak_drop={}", dropped.peak);
    eprintln!("a135_view_peak_retain={}", retained.peak);
    eprintln!("a135_view_curve_drop={}", dropped.curve);
    eprintln!("a135_view_curve_retain={}", retained.curve);
    eprintln!("a135_retained_payload={}", retained.retained_payload);
    eprintln!("a135_reserved_drop={}", dropped.reserved);
    eprintln!("a135_reserved_retain={}", retained.reserved);
}

#[test]
fn append_matches_init_mut_bytes_and_stays_flat() {
    let (payloads, chunks) = wire(FRAMES);
    let init = run_frames(InitPeer::ready(chunks.clone()), Some(MESSAGE_CAP), true);
    let append = run_frames(AppendPeer::ready(chunks), Some(MESSAGE_CAP), true);
    assert_eq!(init.digest, append.digest);
    assert_eq!(init.digest, oracle(FRAMES));
    assert_eq!(append.available_len_max, 0);
    assert_eq!(&init.first_payload, payloads[0].as_slice());
    assert_eq!(&init.last_payload, payloads[FRAMES - 1].as_slice());
    assert_eq!(init.first_payload, append.first_payload);
    assert_eq!(init.last_payload, append.last_payload);
    eprintln!("a135_view_peak_append={}", append.peak);
    eprintln!("a135_view_curve_append={}", append.curve);
}

#[test]
fn three_byte_chunks_keep_split_frame_order() {
    let (payloads, _wide) = wire(FRAMES);
    let mut raw = Vec::new();
    for body in &payloads {
        raw.extend(frame(body));
    }
    let tiny: Vec<Vec<u8>> = raw.chunks(3).map(|chunk| chunk.to_vec()).collect();
    let init = run_frames(InitPeer::ready(tiny.clone()), Some(MESSAGE_CAP), true);
    let append = run_frames(AppendPeer::ready(tiny), Some(MESSAGE_CAP), true);
    assert_eq!(init.digest, append.digest);
    assert_eq!(init.digest, oracle(FRAMES));
    assert_eq!(init.available_len_max, 0);
    eprintln!("a135_view_peak_tiny_init={}", init.peak);
    eprintln!("a135_view_peak_tiny_append={}", append.peak);
}

#[test]
fn capped_oversize_header_does_not_read_a_body() {
    let mut header = Vec::new();
    header.push(b'D');
    header.extend_from_slice(&100_000u32.to_be_bytes());
    let mut socket = BufferedSocket::new(InitPeer::ready(vec![header]));
    let err = read_one(&mut socket, Some(MESSAGE_CAP)).expect_err("cap");
    let text = err.to_string();
    assert!(text.contains("message cap"), "{text}");
    let parts = socket.incoming_meter_parts();
    assert_eq!(parts[0], 5, "logical peak {}", parts[0]);
    assert!(parts[5] <= PLATEAU, "view {}", parts[5]);
    assert!(parts[9] <= 5, "bytes read {}", parts[9]);
    eprintln!("a135_view_peak_oversize={}", parts[5]);
}

#[test]
fn eof_keeps_the_reserve_and_later_frames_plateau() {
    let mut socket = BufferedSocket::new(EofThenPeer::ready(wire(FRAMES).1));
    let before = socket.incoming_meter_parts()[1];
    let err = drive(socket.try_read(|buf| take_frame(buf, None))).expect_err("eof");
    match err {
        Error::Io(err) => assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof),
        other => panic!("eof became {other}"),
    }
    let after_eof = socket.incoming_meter_parts();
    assert!(after_eof[1] >= before, "eof rolled the reserve back");
    let run = run_frames_on(&mut socket, None, true);
    assert_eq!(run.digest, oracle(FRAMES));
    assert!(run.peak <= PLATEAU, "view {}", run.peak);
    eprintln!("a135_view_peak_eof={}", run.peak);
    eprintln!("a135_reserved_eof={}", after_eof[1]);
}

#[test]
fn parked_cancel_charges_once_then_frames_plateau() {
    let mut socket = BufferedSocket::new(CancelThenPeer::ready(wire(FRAMES).1));
    let parked = poll_once(socket.try_read(|buf| take_frame(buf, None)));
    assert!(parked.is_pending(), "init_mut cancel completed");
    drop(parked);
    let after_cancel = socket.incoming_meter_parts();
    // A 5-byte ask fits in the initial spare, so the reserve charge is still
    // zero here. init_mut has already committed that spare. The next short
    // read is what reserves, once.
    assert!(after_cancel[5] <= PLATEAU, "cancel view {}", after_cancel[5]);
    let events = socket.incoming_meter_events();
    assert!(
        events.iter().any(|event| event[12] == 0),
        "cancel completed a row"
    );
    let run = run_frames_on(&mut socket, Some(MESSAGE_CAP), true);
    assert_eq!(run.digest, oracle(FRAMES));
    assert!(run.peak <= PLATEAU, "view {}", run.peak);
    assert!(
        run.peak <= run.warmup.unwrap_or(run.peak),
        "grew after warmup {} to {}",
        run.warmup.unwrap_or(0),
        run.peak
    );
    eprintln!("a135_view_peak_cancel_init={}", run.peak);
    eprintln!("a135_view_at_cancel={}", after_cancel[5]);
    eprintln!("a135_reserved_at_cancel={}", after_cancel[1]);
    eprintln!("a135_reserved_after_cancel_frames={}", run.reserved);
}

#[test]
fn no_data_cancel_leaves_an_open_reserve_row() {
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
    assert!(events.iter().any(|event| event[12] == 0));
    eprintln!("a135_view_peak_cancel_bare={}", parts[5]);
    eprintln!("a135_reserved_cancel_bare={}", parts[1]);
}

#[test]
fn reset_after_init_mut_does_not_resume_doubling() {
    let mut socket = BufferedSocket::new(ResetThenPeer::ready(wire(FRAMES).1));
    let err = drive(socket.try_read(|buf| take_frame(buf, None))).expect_err("reset");
    match err {
        Error::Io(err) => assert_eq!(err.kind(), io::ErrorKind::ConnectionReset),
        other => panic!("reset became {other}"),
    }
    let after = socket.incoming_meter_parts();
    assert!(after[5] <= PLATEAU, "view {}", after[5]);
    let run = run_frames_on(&mut socket, Some(MESSAGE_CAP), false);
    assert_eq!(run.digest, oracle(FRAMES));
    assert!(run.peak <= PLATEAU, "view {}", run.peak);
    eprintln!("a135_view_peak_reset={}", run.peak);
    eprintln!("a135_view_at_reset={}", after[5]);
}

#[tokio::test(flavor = "current_thread")]
async fn tls_verify_full_moderate_rows() {
    let Ok(path) = std::env::var("OO_A135_TLS_FIXTURE") else {
        if std::env::var_os("OO_A135_REQUIRE_TLS").is_some() {
            panic!("OO_A135_TLS_FIXTURE is required");
        }
        eprintln!("a135_tls=skipped");
        return;
    };
    let raw = std::fs::read_to_string(&path).expect("fixture pointer");
    let fixture: Fixture = serde_json::from_str(&raw).expect("fixture json");
    let oracle = read_oracle_file();
    let admin_password = std::fs::read_to_string(&fixture.admin_password_file)
        .expect("admin pass")
        .trim()
        .to_string();
    let reader_password = std::fs::read_to_string(&fixture.reader_password_file)
        .expect("reader pass")
        .trim()
        .to_string();
    assert!(!reader_password.is_empty() && !reader_password.contains('\''));
    assert!(fixture
        .reader_user
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_'));
    let secrets = [admin_password.as_str(), reader_password.as_str()];

    let mut admin = connect(&fixture, &fixture.admin_user, &admin_password, "oo-a135-admin", &secrets).await;
    exec(&mut admin, "SET log_statement = 'none'", &secrets).await;
    exec(&mut admin, "DROP SCHEMA IF EXISTS oo_a135 CASCADE", &secrets).await;
    exec(
        &mut admin,
        &format!("DROP ROLE IF EXISTS {}", fixture.reader_user),
        &secrets,
    )
    .await;
    exec(&mut admin, "CREATE SCHEMA oo_a135", &secrets).await;
    exec(
        &mut admin,
        "CREATE TABLE oo_a135.small (id int4 PRIMARY KEY)",
        &secrets,
    )
    .await;
    exec(
        &mut admin,
        &format!(
            "INSERT INTO oo_a135.small (id) SELECT generate_series(1, {})",
            oracle.rows
        ),
        &secrets,
    )
    .await;
    let admin_sql = format!(
        "SELECT count(*)::int8, coalesce(sum(id)::int8, 0), coalesce(bit_xor(id::int8), 0) \
         FROM oo_a135.small"
    );
    let admin_row = sqlx::raw_sql(sqlx::AssertSqlSafe(admin_sql))
        .fetch_one(&mut admin)
        .await
        .expect("admin oracle");
    let admin_count: i64 = admin_row.try_get(0).expect("count");
    let admin_sum: i64 = admin_row.try_get(1).expect("sum");
    let admin_bits: i64 = admin_row.try_get(2).expect("xor");
    assert_eq!(admin_count, oracle.rows);
    assert_eq!(admin_sum, oracle.sum);
    assert_eq!(admin_bits, oracle.bit_xor);
    exec(
        &mut admin,
        &format!(
            "CREATE ROLE {} LOGIN PASSWORD '{reader_password}'",
            fixture.reader_user
        ),
        &secrets,
    )
    .await;
    exec(
        &mut admin,
        &format!("GRANT USAGE ON SCHEMA oo_a135 TO {}", fixture.reader_user),
        &secrets,
    )
    .await;
    exec(
        &mut admin,
        &format!("GRANT SELECT ON TABLE oo_a135.small TO {}", fixture.reader_user),
        &secrets,
    )
    .await;
    exec(
        &mut admin,
        &format!("ALTER ROLE {} SET log_statement = 'all'", fixture.reader_user),
        &secrets,
    )
    .await;
    drop(admin);

    let mut reader = connect(
        &fixture,
        &fixture.reader_user,
        &reader_password,
        "oo-a135-reader",
        &secrets,
    )
    .await;
    assert!(reader.incoming_bounds().is_none());
    let rows = tokio::time::timeout(Duration::from_secs(60), async {
        sqlx::query("SELECT id FROM oo_a135.small ORDER BY id")
            .fetch_all(&mut reader)
            .await
    })
    .await
    .unwrap_or_else(|_| panic!("read exceeded 60s"));
    let rows = match rows {
        Ok(rows) => rows,
        Err(err) => panic!("{}", redact(&err.to_string(), &secrets)),
    };
    assert_eq!(rows.len() as i64, oracle.rows);
    let mut hasher = Sha256::new();
    for (index, row) in rows.iter().enumerate() {
        let id: i32 = row.try_get(0).expect("id");
        assert_eq!(id as i64, index as i64 + 1, "row {index}");
        hasher.update(id.to_be_bytes());
    }
    let digest: [u8; 32] = hasher.finalize().into();
    let row_hash = hex_encode(&digest);
    assert_eq!(row_hash, oracle.row_sha256);

    let caller_retained = (rows.len() * 4) as u64;
    let peak = reader.incoming_allocation().view_capacity_peak;
    let rss = rss_kib();
    assert!(peak < PG_VIEW_FUSE, "view {peak}");
    assert!(rss < PG_RSS_FUSE_KIB, "rss {rss} KiB");
    assert_ne!(peak, caller_retained, "view collided with caller payload");
    assert_ne!(peak, rss, "view collided with rss kib");
    assert_ne!(caller_retained, rss);

    let agg = sqlx::query(
        "SELECT count(*)::int8, coalesce(sum(id)::int8, 0), coalesce(bit_xor(id::int8), 0) \
         FROM oo_a135.small",
    )
    .fetch_one(&mut reader)
    .await;
    let agg = match agg {
        Ok(row) => row,
        Err(err) => panic!("{}", redact(&err.to_string(), &secrets)),
    };
    let count: i64 = agg.try_get(0).expect("count");
    let sum: i64 = agg.try_get(1).expect("sum");
    let bits: i64 = agg.try_get(2).expect("xor");
    assert_eq!(count, oracle.rows);
    assert_eq!(sum, oracle.sum);
    assert_eq!(bits, oracle.bit_xor);
    drop(reader);
    drop(rows);

    let (sql_lines, write_lines, set_lines) = reader_sql(&fixture.log_file, &secrets).await;
    assert!(sql_lines > 0, "reader SQL log is empty");
    assert_eq!(write_lines, 0, "reader write statements {write_lines}");
    assert_eq!(set_lines, 0, "reader issued SET");
    eprintln!("a135_pg_rows={}", oracle.rows);
    eprintln!("a135_pg_view_peak={peak}");
    eprintln!("a135_pg_rss_kib={rss}");
    eprintln!("a135_pg_caller_retained={caller_retained}");
    eprintln!("a135_pg_hash={row_hash}");
    eprintln!("a135_pg_reader_sql_lines={sql_lines}");
    eprintln!("a135_pg_reader_write_lines={write_lines}");
}

struct Run {
    peak: u64,
    warmup: Option<u64>,
    curve: String,
    digest: [u8; 32],
    reserved: u64,
    reserved_nondecreasing: bool,
    available_len_max: u64,
    retained_payload: u64,
    first_payload: Vec<u8>,
    last_payload: Vec<u8>,
}

fn run_frames<S: Socket>(peer: S, cap: Option<usize>, retain: bool) -> Run {
    let mut socket = BufferedSocket::new(peer);
    run_frames_on(&mut socket, cap, retain)
}

fn run_frames_on<S: Socket>(socket: &mut BufferedSocket<S>, cap: Option<usize>, retain: bool) -> Run {
    let mut held: Vec<Bytes> = Vec::new();
    let mut hasher = Sha256::new();
    let mut warmup = None;
    let mut curve = Vec::new();
    let mut previous_reserved = socket.incoming_meter_parts()[1];
    let mut reserved_nondecreasing = true;
    let mut first_payload = Vec::new();
    let mut last_payload = Vec::new();
    for index in 0..FRAMES {
        let got = read_one(socket, cap).expect("frame");
        let expected = (index as u16).to_be_bytes();
        assert_eq!(got.as_ref(), expected.as_slice(), "frame {index}");
        hasher.update(got.as_ref());
        if index == 0 {
            first_payload = got.as_ref().to_vec();
        }
        if index + 1 == FRAMES {
            last_payload = got.as_ref().to_vec();
        }
        let peak = socket.incoming_meter_parts()[5];
        assert!(peak <= PLATEAU, "frame {index} view {peak}");
        if index + 1 == WARMUP {
            warmup = Some(peak);
        } else if let Some(mark) = warmup {
            assert!(peak <= mark, "frame {index} grew past warmup {mark} to {peak}");
        }
        if matches!(index, 0 | 1 | 2 | 3 | 4 | 63 | 511) {
            curve.push(format!("{index}:{peak}"));
        }
        let reserved = socket.incoming_meter_parts()[1];
        if reserved < previous_reserved {
            reserved_nondecreasing = false;
        }
        previous_reserved = reserved;
        if retain {
            held.push(got);
        }
    }
    let parts = socket.incoming_meter_parts();
    let events = socket.incoming_meter_events();
    assert!(events.len() <= 64, "{}", events.len());
    assert_eq!(events.len() == 64, parts[19] > 0);
    let mut stored = 0u64;
    let mut available_len_max = 0u64;
    for event in &events {
        let growth = event[2];
        let before = event[7];
        let after = event[8];
        let completed = event[12];
        available_len_max = available_len_max.max(event[5]);
        assert!(completed == 0 || completed == 1);
        if after >= before {
            assert_eq!(growth, after - before, "{event:?}");
        } else {
            assert_eq!(growth, 0, "{event:?}");
        }
        stored += growth;
    }
    if parts[19] == 0 {
        assert_eq!(parts[1], stored);
    } else {
        assert!(parts[1] >= stored);
    }
    let retained_payload: u64 = held.iter().map(|bytes| bytes.len() as u64).sum();
    if retain {
        assert_eq!(held.len(), FRAMES);
    }
    Run {
        peak: parts[5],
        warmup,
        curve: curve.join(","),
        digest: hasher.finalize().into(),
        reserved: parts[1],
        reserved_nondecreasing,
        available_len_max,
        retained_payload,
        first_payload,
        last_payload,
    }
}

struct OracleFile {
    rows: i64,
    sum: i64,
    bit_xor: i64,
    row_sha256: String,
}

fn read_oracle_file() -> OracleFile {
    let path = std::env::var("OO_A135_ORACLE").expect("OO_A135_ORACLE");
    let text = std::fs::read_to_string(path).expect("oracle");
    let mut rows = None;
    let mut sum = None;
    let mut bit_xor = None;
    let mut row_sha256 = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("rows=") {
            rows = rest.parse().ok();
        } else if let Some(rest) = line.strip_prefix("sum=") {
            sum = rest.parse().ok();
        } else if let Some(rest) = line.strip_prefix("bit_xor=") {
            bit_xor = rest.parse().ok();
        } else if let Some(rest) = line.strip_prefix("row_sha256=") {
            row_sha256 = Some(rest.to_string());
        }
    }
    OracleFile {
        rows: rows.expect("rows"),
        sum: sum.expect("sum"),
        bit_xor: bit_xor.expect("xor"),
        row_sha256: row_sha256.expect("hash"),
    }
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
    if let Err(err) = sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(&mut *conn)
        .await
    {
        panic!("{}", redact(&err.to_string(), secrets));
    }
}

async fn reader_sql(path: &str, secrets: &[&str]) -> (usize, usize, usize) {
    let mut sql_lines = 0usize;
    let mut write_lines = 0usize;
    let mut set_lines = 0usize;
    for _ in 0..40 {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let redacted = redact(&text, secrets);
        assert!(
            !redacted.contains("[redacted]"),
            "reader log contains a secret"
        );
        sql_lines = 0;
        write_lines = 0;
        set_lines = 0;
        let mut saw_table = false;
        for line in redacted.lines() {
            if !line.contains("app=oo-a135-reader") {
                continue;
            }
            if !line.contains("statement:") && !line.contains("execute ") && !line.contains("parse ")
            {
                continue;
            }
            sql_lines += 1;
            if line.contains("SELECT") && line.contains("oo_a135.small") {
                saw_table = true;
            }
            if has_word(line, "SET") {
                set_lines += 1;
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
            return (sql_lines, write_lines, set_lines);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    (sql_lines, write_lines, set_lines)
}

fn has_word(line: &str, word: &str) -> bool {
    line.split(|ch: char| !ch.is_ascii_alphanumeric())
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
            return rest.split_whitespace().next().expect("rss").parse().expect("rss number");
        }
    }
    panic!("VmRSS missing");
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
    fn ready(chunks: Vec<Vec<u8>>) -> Self {
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
    fn ready(chunks: Vec<Vec<u8>>) -> Self {
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
    fn ready(chunks: Vec<Vec<u8>>) -> Self {
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
    fn ready(chunks: Vec<Vec<u8>>) -> Self {
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

    fn poll_read_ready(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.phase == 1 {
            self.phase = 2;
            Poll::Pending
        } else {
            ready_or_eof(self.chunks.ready())
        }
    }

    fn try_write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    fn poll_write_ready(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

struct ResetThenPeer {
    chunks: Chunks,
    phase: u8,
}

impl ResetThenPeer {
    fn ready(chunks: Vec<Vec<u8>>) -> Self {
        Self {
            chunks: Chunks::new(chunks),
            phase: 0,
        }
    }
}

impl Socket for ResetThenPeer {
    fn try_read(&mut self, buf: &mut dyn ReadBuf) -> io::Result<usize> {
        if self.phase == 0 {
            let _ = buf.init_mut();
            self.phase = 1;
            return Err(io::Error::new(io::ErrorKind::ConnectionReset, "reset"));
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
        ready_or_eof(self.chunks.ready())
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

fn take_frame(buf: &mut BytesMut, cap: Option<usize>) -> Result<ControlFlow<Bytes, usize>, Error> {
    if buf.len() < 5 {
        return Ok(ControlFlow::Continue(5));
    }
    let message_len = u32::from_be_bytes(buf[1..5].try_into().expect("len")) as usize;
    if cap.is_some_and(|limit| message_len > limit) {
        return Err(Error::Protocol("message cap".into()));
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

fn read_one<S: Socket>(socket: &mut BufferedSocket<S>, cap: Option<usize>) -> Result<Bytes, Error> {
    drive(socket.try_read(move |buf| take_frame(buf, cap)))
}

fn oracle(frames: usize) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for id in 0..frames {
        hasher.update((id as u16).to_be_bytes());
    }
    hasher.finalize().into()
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex_encode(&Sha256::digest(bytes))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
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

fn drive<T>(future: impl Future<Output = T>) -> T {
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    for _ in 0..128 {
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
