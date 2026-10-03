//! Finite PostgreSQL peer for the named-portal driver API.
//!
//! The peer answers a scripted extended-query flow. It does not allocate from a
//! declared huge length, and it records each Execute before it writes a response.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use sqlx::postgres::{
    PgArguments, PgConnectOptions, PgConnection, PgNamedPortal, PgPortalAllowance, PgPortalOutcome,
    PgSslMode, EXECUTE_LIMIT,
};
use sqlx::{Arguments, Connection, Row};

const HANG: Duration = Duration::from_secs(5);

fn must_err<T, E: std::fmt::Display>(result: Result<T, E>, what: &str) -> E {
    match result {
        Err(err) => err,
        Ok(_) => panic!("{what}"),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Error,
    Hold,
    Fragment,
}

struct Plan {
    rows: Vec<Vec<i32>>,
    mode: Mode,
}

#[derive(Default)]
struct Log {
    executes: Vec<(String, u32)>,
    queries: Vec<String>,
    statements: Vec<String>,
    bind_ints: Vec<Option<i32>>,
    saw_commit: bool,
}

struct Peer {
    port: u16,
    log: Arc<Mutex<Log>>,
    early: Arc<AtomicUsize>,
    join: Option<JoinHandle<()>>,
}

impl Peer {
    fn start(plan: Plan) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let log = Arc::new(Mutex::new(Log::default()));
        let early = Arc::new(AtomicUsize::new(0));
        let log_thread = Arc::clone(&log);
        let early_thread = Arc::clone(&early);
        let join = thread::spawn(move || peer_main(listener, plan, log_thread, early_thread));
        Self {
            port,
            log,
            early,
            join: Some(join),
        }
    }

    fn executes(&self) -> Vec<(String, u32)> {
        self.log.lock().expect("log").executes.clone()
    }

    fn queries(&self) -> Vec<String> {
        self.log.lock().expect("log").queries.clone()
    }

    fn statements(&self) -> Vec<String> {
        self.log.lock().expect("log").statements.clone()
    }

    fn bind_ints(&self) -> Vec<Option<i32>> {
        self.log.lock().expect("log").bind_ints.clone()
    }

    fn saw_commit(&self) -> bool {
        self.log.lock().expect("log").saw_commit
    }

    fn early_bytes(&self) -> usize {
        self.early.load(Ordering::Relaxed)
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

struct Script {
    rows: Vec<i32>,
    sent: usize,
}

struct Session {
    scripts: Vec<Script>,
    next_script: usize,
    by_statement: HashMap<String, usize>,
    by_portal: HashMap<String, usize>,
    pending_statement: Option<(String, bool)>,
    pending_bind: Option<(String, String)>,
    pending_execute: Option<(String, u32)>,
    pending_closes: usize,
    status: u8,
    await_error_sync: bool,
    mode: Mode,
    fragment_done: bool,
    log: Arc<Mutex<Log>>,
    early: Arc<AtomicUsize>,
}

fn peer_main(listener: TcpListener, plan: Plan, log: Arc<Mutex<Log>>, early: Arc<AtomicUsize>) {
    let Ok((mut stream, _)) = listener.accept() else {
        return;
    };
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(HANG));
    let _ = stream.set_write_timeout(Some(HANG));
    if read_startup(&mut stream).is_err() {
        return;
    }
    if write_all(&mut stream, &auth_ready()).is_err() {
        return;
    }
    let mut session = Session {
        scripts: plan
            .rows
            .into_iter()
            .map(|rows| Script { rows, sent: 0 })
            .collect(),
        next_script: 0,
        by_statement: HashMap::new(),
        by_portal: HashMap::new(),
        pending_statement: None,
        pending_bind: None,
        pending_execute: None,
        pending_closes: 0,
        status: b'I',
        await_error_sync: false,
        mode: plan.mode,
        fragment_done: false,
        log,
        early,
    };
    loop {
        let Ok((kind, body)) = read_message(&mut stream) else {
            return;
        };
        let step = match kind {
            b'X' => return,
            b'Q' => session.simple(&mut stream, &cstring(&body, &mut 0)),
            b'P' => {
                let mut at = 0;
                let statement = cstring(&body, &mut at);
                let query = cstring(&body, &mut at);
                session.log.lock().expect("log").statements.push(statement.clone());
                session.pending_statement = Some((statement, query.contains("$1")));
                Ok(())
            }
            b'D' => Ok(()),
            b'B' => {
                let mut at = 0;
                let portal = cstring(&body, &mut at);
                let statement = cstring(&body, &mut at);
                let value = first_int_param(&body);
                session.log.lock().expect("log").bind_ints.push(value);
                session.pending_bind = Some((portal, statement));
                Ok(())
            }
            b'E' => {
                let mut at = 0;
                let portal = cstring(&body, &mut at);
                let limit = u32::from_be_bytes(body[at..at + 4].try_into().expect("limit"));
                session
                    .log
                    .lock()
                    .expect("log")
                    .executes
                    .push((portal.clone(), limit));
                session.pending_execute = Some((portal, limit));
                Ok(())
            }
            b'C' => {
                session.pending_closes += 1;
                Ok(())
            }
            b'H' => session.process(&mut stream, false),
            b'S' => session.process(&mut stream, true),
            _ => Err(std::io::Error::other("unexpected frontend message")),
        };
        if step.is_err() {
            return;
        }
    }
}

impl Session {
    fn simple(&mut self, stream: &mut TcpStream, sql: &str) -> std::io::Result<()> {
        let trimmed = sql.trim().trim_end_matches('\0');
        let upper = trimmed.to_ascii_uppercase();
        {
            let mut log = self.log.lock().expect("log");
            log.queries.push(trimmed.to_string());
            if upper.starts_with("COMMIT") {
                log.saw_commit = true;
            }
        }
        if upper.starts_with("BEGIN") {
            self.status = b'T';
            write_all(stream, &command_complete("BEGIN"))?;
            write_all(stream, &ready(b'T'))?;
        } else if upper.starts_with("ROLLBACK") {
            self.status = b'I';
            write_all(stream, &command_complete("ROLLBACK"))?;
            write_all(stream, &ready(b'I'))?;
        } else if upper.starts_with("COMMIT") {
            self.status = b'I';
            write_all(stream, &command_complete("COMMIT"))?;
            write_all(stream, &ready(b'I'))?;
        } else {
            return Err(std::io::Error::other("unexpected simple query"));
        }
        Ok(())
    }

    fn process(&mut self, stream: &mut TcpStream, sync: bool) -> std::io::Result<()> {
        if self.await_error_sync {
            if sync {
                self.status = b'E';
                self.await_error_sync = false;
                self.pending_statement = None;
                self.pending_bind = None;
                self.pending_execute = None;
                self.pending_closes = 0;
                write_all(stream, &ready(b'E'))?;
            }
            return Ok(());
        }
        if let Some((statement, has_param)) = self.pending_statement.take() {
            if self.next_script >= self.scripts.len() {
                return Err(std::io::Error::other("missing script"));
            }
            let idx = self.next_script;
            self.next_script += 1;
            self.by_statement.insert(statement, idx);
            let nparams = u16::from(has_param);
            write_all(stream, &empty(b'1'))?;
            write_all(stream, &param_desc(nparams))?;
            write_all(stream, &row_desc_id())?;
        }
        if let Some((portal, statement)) = self.pending_bind.take() {
            if let Some(idx) = self.by_statement.get(&statement).copied() {
                self.by_portal.insert(portal, idx);
            }
            write_all(stream, &empty(b'2'))?;
        }
        if let Some((portal, limit)) = self.pending_execute.take() {
            if self.mode == Mode::Hold {
                return Ok(());
            }
            if self.mode == Mode::Error {
                write_all(stream, &error_response("22012", "division by zero"))?;
                self.await_error_sync = true;
                return Ok(());
            }
            self.send_rows(stream, &portal, limit)?;
        }
        for _ in 0..self.pending_closes {
            write_all(stream, &empty(b'3'))?;
        }
        self.pending_closes = 0;
        if sync {
            write_all(stream, &ready(self.status))?;
        }
        Ok(())
    }

    fn send_rows(&mut self, stream: &mut TcpStream, portal: &str, limit: u32) -> std::io::Result<()> {
        let Some(idx) = self.by_portal.get(portal).copied() else {
            return Err(std::io::Error::other("unbound portal"));
        };
        let script = &mut self.scripts[idx];
        let remaining = script.rows.len().saturating_sub(script.sent);
        let take = if limit == 0 {
            remaining
        } else {
            remaining.min(limit as usize)
        };
        let rows = script.rows[script.sent..script.sent + take].to_vec();
        script.sent += take;
        for (i, id) in rows.iter().enumerate() {
            let frame = int_row(*id);
            if self.mode == Mode::Fragment && !self.fragment_done && i == 0 {
                self.fragment_done = true;
                write_fragmented(stream, &frame, &self.early)?;
            } else {
                write_all(stream, &frame)?;
            }
        }
        // PostgreSQL 17 sends PortalSuspended when a positive limit is filled,
        // including when that row was the last one. The next Execute, which
        // returns no row, is CommandComplete.
        if limit > 0 && take == limit as usize {
            write_all(stream, &empty(b's'))?;
        } else {
            write_all(stream, &command_complete(&format!("SELECT {take}")))?;
        }
        Ok(())
    }
}

fn write_fragmented(stream: &mut TcpStream, bytes: &[u8], early: &AtomicUsize) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_millis(15)))?;
    let mut buf = [0u8; 128];
    for &byte in bytes {
        stream.write_all(&[byte])?;
        stream.flush()?;
        loop {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    early.fetch_add(n, Ordering::Relaxed);
                    break;
                }
                Err(err)
                    if err.kind() == std::io::ErrorKind::WouldBlock
                        || err.kind() == std::io::ErrorKind::TimedOut =>
                {
                    break;
                }
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(err),
            }
        }
    }
    stream.set_read_timeout(Some(HANG))?;
    Ok(())
}

async fn connect(plan: Plan) -> (PgConnection, Peer) {
    let peer = Peer::start(plan);
    let options = PgConnectOptions::from_explicit()
        .host("127.0.0.1")
        .port(peer.port)
        .username("portal")
        .database("portal")
        .ssl_mode(PgSslMode::Disable);
    let conn = tokio::time::timeout(HANG, PgConnection::connect_with(&options))
        .await
        .expect("connect timed out")
        .expect("connect");
    (conn, peer)
}

fn allowance() -> PgPortalAllowance {
    PgPortalAllowance::positive(4).expect("allowance")
}

fn row_id(row: &sqlx::postgres::PgRow) -> i32 {
    row.try_get(0).expect("id")
}

#[test]
fn allowance_zero_rejected() {
    let err = PgPortalAllowance::positive(0).expect_err("zero allowance");
    assert!(err.to_string().contains("positive"), "{err}");
    assert_eq!(EXECUTE_LIMIT, 1);
    assert_eq!(PgPortalAllowance::positive(1).expect("one").rows(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn named_portal_pages_suspend_and_complete() {
    let (mut conn, peer) = connect(Plan {
        rows: vec![vec![1, 2, 3], vec![], vec![7], vec![9]],
        mode: Mode::Normal,
    })
    .await;
    let refused = must_err(
        conn.open_named_portal("select id", PgArguments::default())
            .await,
        "portal before begin",
    );
    assert!(
        refused.to_string().contains("open transaction"),
        "{refused}"
    );

    let mut tx = conn.begin().await.expect("begin");
    let mut portal = tx
        .open_named_portal("select id from pages", PgArguments::default())
        .await
        .expect("open");
    let first = portal.pull(allowance()).await.expect("pull 1");
    let PgPortalOutcome::Suspended(row) = first.outcome else {
        panic!("first page should suspend");
    };
    assert_eq!(row_id(&row), 1);
    assert_eq!(first.accounting.execute_limit, 1);
    assert_eq!(first.accounting.reserved_rows, 1);
    assert_eq!(first.accounting.consumed_rows, 1);
    assert_eq!(peer.executes().len(), 1, "no second execute without demand");
    assert_eq!(peer.executes()[0].1, 1);

    let second = portal.pull(allowance()).await.expect("pull 2");
    let PgPortalOutcome::Suspended(row) = second.outcome else {
        panic!("second page should suspend");
    };
    assert_eq!(row_id(&row), 2);
    assert_eq!(peer.executes().len(), 2);

    let third = portal.pull(allowance()).await.expect("pull 3");
    let PgPortalOutcome::Suspended(row) = third.outcome else {
        panic!("the last row still suspends when the execute limit is filled");
    };
    assert_eq!(row_id(&row), 3);
    assert_eq!(third.accounting.consumed_rows, 1);
    assert_eq!(peer.executes().len(), 3);
    let finished = portal.pull(allowance()).await.expect("pull 4");
    let PgPortalOutcome::Complete {
        row: None,
        command_tag,
    } = finished.outcome
    else {
        panic!("the demanded follow-up completes with no row");
    };
    assert_eq!(finished.accounting.consumed_rows, 0);
    assert_eq!(command_tag.rows_affected(), 0);
    assert!(portal.pull(allowance()).await.is_err(), "closed portal");
    assert_eq!(peer.executes().len(), 4, "failed pull must not write Execute");
    drop(portal);

    let mut none = tx
        .open_named_portal("select id from none", PgArguments::default())
        .await
        .expect("empty portal");
    let step = none.pull(allowance()).await.expect("empty pull");
    let PgPortalOutcome::Complete {
        row: None,
        command_tag,
    } = step.outcome
    else {
        panic!("zero rows should complete without suspension");
    };
    assert_eq!(step.accounting.reserved_rows, 1);
    assert_eq!(step.accounting.consumed_rows, 0);
    assert_eq!(command_tag.rows_affected(), 0);
    assert!(!command_tag.truncated());
    drop(none);

    let mut args = PgArguments::default();
    args.add(7i32).expect("bind");
    let mut one = tx
        .open_named_portal("select id from one where id = $1", args)
        .await
        .expect("one portal");
    let step = one.pull(allowance()).await.expect("one pull");
    let PgPortalOutcome::Suspended(row) = step.outcome else {
        panic!("one row fills the execute limit and suspends");
    };
    assert_eq!(row_id(&row), 7);
    let finished = one.pull(allowance()).await.expect("one finish");
    let PgPortalOutcome::Complete { row: None, .. } = finished.outcome else {
        panic!("the follow-up execute completes with no row");
    };
    assert_eq!(finished.accounting.consumed_rows, 0);
    drop(one);

    let statements = peer.statements();
    let portals: Vec<String> = peer
        .executes()
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| !name.is_empty())
        .collect();
    assert!(statements.iter().all(|name| name.starts_with("sqlx_s_")));
    assert!(portals.iter().all(|name| name.starts_with("sqlx_p_")));
    assert_eq!(statements.len(), 3);
    assert_eq!(portals.len(), 7);
    let parked = tx
        .open_named_portal("select id from parked", PgArguments::default())
        .await
        .expect("parked");
    let closed = parked.close().await.expect("close");
    assert_eq!(closed.close_completes, 2);
    assert!(closed.ready && closed.transaction_open);
    assert_eq!(peer.executes().len(), 7, "close must not write Execute");
    assert_eq!(peer.statements().len(), 4);
    let mut unique_statements = statements.clone();
    unique_statements.sort();
    unique_statements.dedup();
    assert_eq!(unique_statements.len(), statements.len());
    let mut unique_portals = portals.clone();
    unique_portals.sort();
    unique_portals.dedup();
    assert_eq!(unique_portals.len(), 3);
    assert!(peer.bind_ints().contains(&Some(7)));

    tx.rollback().await.expect("rollback");
    assert!(!peer.saw_commit());
    assert!(peer
        .queries()
        .iter()
        .any(|query| query.to_ascii_uppercase().starts_with("ROLLBACK")));
    assert!(!conn.named_portal_poisoned());
    conn.ping().await.expect("ping after rollback");
}

#[tokio::test(flavor = "current_thread")]
async fn fragmented_row_does_not_send_next_execute() {
    let (mut conn, peer) = connect(Plan {
        rows: vec![vec![1, 2]],
        mode: Mode::Fragment,
    })
    .await;
    let mut tx = conn.begin().await.expect("begin");
    let mut portal = tx
        .open_named_portal("select id from fragment", PgArguments::default())
        .await
        .expect("open");
    let step = portal.pull(allowance()).await.expect("fragment pull");
    let PgPortalOutcome::Suspended(row) = step.outcome else {
        panic!("fragmented first row should suspend");
    };
    assert_eq!(row_id(&row), 1);
    assert_eq!(peer.early_bytes(), 0, "client wrote during a partial DataRow");
    assert_eq!(peer.executes().len(), 1);
    let step = portal.pull(allowance()).await.expect("second pull");
    let PgPortalOutcome::Suspended(row) = step.outcome else {
        panic!("second row fills the execute limit and suspends");
    };
    assert_eq!(row_id(&row), 2);
    assert_eq!(peer.executes().len(), 2);
    let finished = portal.pull(allowance()).await.expect("fragment finish");
    let PgPortalOutcome::Complete { row: None, .. } = finished.outcome else {
        panic!("the follow-up execute completes with no row");
    };
    assert_eq!(finished.accounting.consumed_rows, 0);
    assert_eq!(peer.executes().len(), 3);
    drop(portal);
    tx.rollback().await.expect("rollback");
}

#[tokio::test(flavor = "current_thread")]
async fn execute_error_resyncs_aborted_transaction() {
    let (mut conn, peer) = connect(Plan {
        rows: vec![vec![1]],
        mode: Mode::Error,
    })
    .await;
    let mut tx = conn.begin().await.expect("begin");
    let mut portal = tx
        .open_named_portal("select 1/0", PgArguments::default())
        .await
        .expect("open");
    let err = must_err(portal.pull(allowance()).await, "server error");
    let db = err.as_database_error().expect("database error");
    assert_eq!(db.code().as_deref(), Some("22012"));
    let last = portal.last_pull().expect("reserved");
    assert_eq!(last.reserved_rows, 1);
    assert_eq!(last.consumed_rows, 0);
    assert!(!portal.connection_poisoned());
    drop(portal);
    tx.rollback().await.expect("rollback after error");
    assert!(!conn.named_portal_poisoned());
    assert!(!peer.saw_commit());
    conn.ping().await.expect("ping after resync");
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_pull_poisons_connection() {
    let (mut conn, _peer) = connect(Plan {
        rows: vec![vec![1, 2]],
        mode: Mode::Hold,
    })
    .await;
    let mut tx = conn.begin().await.expect("begin");
    let mut portal: PgNamedPortal<'_> = tx
        .open_named_portal("select id from hold", PgArguments::default())
        .await
        .expect("open");
    let timed = tokio::time::timeout(Duration::from_millis(400), portal.pull(allowance())).await;
    assert!(timed.is_err(), "peer held the execute response");
    assert!(portal.connection_poisoned());
    let again = must_err(portal.pull(allowance()).await, "poisoned pull");
    assert!(again.as_database_error().is_none());
    drop(portal);
    drop(tx);
    assert!(conn.named_portal_poisoned());
    assert!(conn.ping().await.is_err(), "poisoned connection is not reused");
}

#[tokio::test(flavor = "current_thread")]
async fn default_executor_keeps_unnamed_limit_zero() {
    let (mut conn, peer) = connect(Plan {
        rows: vec![vec![1]],
        mode: Mode::Normal,
    })
    .await;
    let row = sqlx::query("select 1")
        .fetch_one(&mut conn)
        .await
        .expect("default query");
    assert_eq!(row.try_get::<i32, _>(0).expect("value"), 1);
    let executes = peer.executes();
    assert_eq!(executes.len(), 1);
    assert_eq!(executes[0].0, "");
    assert_eq!(executes[0].1, 0);
    assert!(!conn.named_portal_poisoned());
}

fn auth_ready() -> Vec<u8> {
    let mut hello = Vec::with_capacity(15);
    hello.push(b'R');
    hello.extend_from_slice(&8u32.to_be_bytes());
    hello.extend_from_slice(&0u32.to_be_bytes());
    hello.push(b'Z');
    hello.extend_from_slice(&5u32.to_be_bytes());
    hello.push(b'I');
    hello
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

fn read_message(stream: &mut TcpStream) -> std::io::Result<(u8, Vec<u8>)> {
    let mut head = [0u8; 5];
    stream.read_exact(&mut head)?;
    let len = u32::from_be_bytes([head[1], head[2], head[3], head[4]]);
    if !(4..=1_000_000).contains(&len) {
        return Err(std::io::Error::other("frontend length"));
    }
    let mut body = vec![0u8; (len as usize) - 4];
    if !body.is_empty() {
        stream.read_exact(&mut body)?;
    }
    Ok((head[0], body))
}

fn cstring(body: &[u8], at: &mut usize) -> String {
    let start = *at;
    let rel = body[start..]
        .iter()
        .position(|byte| *byte == 0)
        .expect("cstring");
    let end = start + rel;
    let text = String::from_utf8_lossy(&body[start..end]).into_owned();
    *at = end + 1;
    text
}

fn first_int_param(body: &[u8]) -> Option<i32> {
    let mut at = 0;
    let _ = cstring(body, &mut at);
    let _ = cstring(body, &mut at);
    if body.len() < at + 2 {
        return None;
    }
    let nformats = u16::from_be_bytes([body[at], body[at + 1]]) as usize;
    at += 2 + nformats * 2;
    if body.len() < at + 2 {
        return None;
    }
    let nparams = u16::from_be_bytes([body[at], body[at + 1]]);
    at += 2;
    if nparams == 0 || body.len() < at + 4 {
        return None;
    }
    let plen = i32::from_be_bytes(body[at..at + 4].try_into().ok()?);
    at += 4;
    if plen != 4 || body.len() < at + 4 {
        return None;
    }
    Some(i32::from_be_bytes(body[at..at + 4].try_into().ok()?))
}

fn write_all(stream: &mut TcpStream, bytes: &[u8]) -> std::io::Result<()> {
    stream.write_all(bytes)?;
    stream.flush()
}

fn frame(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + body.len());
    out.push(kind);
    let len = u32::try_from(body.len() + 4).expect("len");
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(body);
    out
}

fn empty(kind: u8) -> Vec<u8> {
    frame(kind, b"")
}

fn ready(status: u8) -> Vec<u8> {
    frame(b'Z', &[status])
}

fn command_complete(tag: &str) -> Vec<u8> {
    let mut body = tag.as_bytes().to_vec();
    body.push(0);
    frame(b'C', &body)
}

fn int_row(id: i32) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&1u16.to_be_bytes());
    body.extend_from_slice(&4i32.to_be_bytes());
    body.extend_from_slice(&id.to_be_bytes());
    frame(b'D', &body)
}

fn param_desc(nparams: u16) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&nparams.to_be_bytes());
    for _ in 0..nparams {
        body.extend_from_slice(&23u32.to_be_bytes());
    }
    frame(b't', &body)
}

fn row_desc_id() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&1u16.to_be_bytes());
    body.extend_from_slice(b"id\0");
    body.extend_from_slice(&0u32.to_be_bytes());
    body.extend_from_slice(&0i16.to_be_bytes());
    body.extend_from_slice(&23u32.to_be_bytes());
    body.extend_from_slice(&4i16.to_be_bytes());
    body.extend_from_slice(&(-1i32).to_be_bytes());
    body.extend_from_slice(&0i16.to_be_bytes());
    frame(b'T', &body)
}

fn error_response(code: &str, message: &str) -> Vec<u8> {
    let mut body = Vec::new();
    for (tag, value) in [(b'S', "ERROR"), (b'V', "ERROR"), (b'C', code), (b'M', message)] {
        body.push(tag);
        body.extend_from_slice(value.as_bytes());
        body.push(0);
    }
    body.push(0);
    frame(b'E', &body)
}
