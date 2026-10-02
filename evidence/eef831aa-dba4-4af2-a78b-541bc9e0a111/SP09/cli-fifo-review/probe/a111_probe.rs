//! Independent spawn oracle for the FIFO open fix.
//!
//! This file is copied into an extracted git archive for one offline run.
//! It does not import the crate's test helpers. A signal death is never
//! scored as exit 1. Fixture bytes stay in the container temp directory.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const REQUEST_TIMEOUT_MS: &str = "120000";
const REJECT_DEADLINE_MS: u64 = 1_200;
const POSITIVE_DEADLINE_MS: u64 = 8_000;
const POLICY: &str = "retry_policy: max_extra_attempts=3 max_wait_ms=2000 nested_cli_retries=0 attempt_count=unknown\n";
const SANDBOX_MSG: &str = "sandbox proxy client is not implemented; from_sandbox_env is pending and this process did not read a token, OO_TOKEN, or a home credential";

struct Case {
    id: String,
    pass: bool,
    exit_code: Option<i32>,
    signal: Option<i32>,
    elapsed_ms: u128,
    stdout_len: usize,
    stderr_len: usize,
    stderr_exact: bool,
    accepts: usize,
    http_completed: usize,
    auth_match: bool,
    sql_match: bool,
    path_match: bool,
    secret_absent: bool,
    killed_as_exit1: bool,
    deadline_ms: u64,
    note: String,
}

#[test]
fn a111_fifo_oracle() {
    let root = PathBuf::from("/tmp/oo-a111-private-fixtures");
    let _ = fs::remove_dir_all(&root);
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700);
    builder.create(&root).expect("private fixture dir");

    let mut cases = Vec::new();
    fifo_cases(&root, &mut cases);
    preopen_cases(&root, &mut cases);
    bound_cases(&root, &mut cases);
    positive_cases(&root, &mut cases);
    tls_cases(&root, &mut cases);

    let failed: Vec<_> = cases.iter().filter(|case| !case.pass).map(|case| case.id.clone()).collect();
    write_report(&cases, &failed);
    let _ = fs::remove_dir_all(&root);
    assert!(
        failed.is_empty(),
        "a111 cases failed: {}",
        failed.len()
    );
}

fn fifo_cases(root: &Path, cases: &mut Vec<Case>) {
    let specs = [
        ("fifo_token_peerless", Role::Token, false),
        ("fifo_sql_peerless", Role::Sql, false),
        ("fifo_ca_peerless", Role::Ca, false),
        ("fifo_token_writer", Role::Token, true),
        ("fifo_sql_writer", Role::Sql, true),
        ("fifo_ca_writer", Role::Ca, true),
    ];
    for (id, role, writer) in specs {
        let token = fresh_token();
        let sql = fresh_sql();
        let token_path = root.join(format!("{id}-token"));
        write_mode(&token_path, token.as_bytes(), 0o600);
        let fifo = root.join(id);
        make_fifo(&fifo);
        let holder = if writer { Some(hold_writer(&fifo)) } else { None };
        let (flag_path, expect_exit, stderr) = match role {
            Role::Token => (&fifo, 4, static_text("auth", "token file rejected: not-file", "not_replayed")),
            Role::Sql => (&fifo, 1, static_text("local", "sql file rejected: not-file", "not_applicable")),
            Role::Ca => (&fifo, 1, static_text("local", "ca file rejected: not-file", "not_applicable")),
        };
        let case = drive(
            id,
            Drive {
                token_path: &token_path,
                token: &token,
                sql: &sql,
                sql_path: if matches!(role, Role::Sql) { Some(flag_path) } else { None },
                ca_path: if matches!(role, Role::Ca) { Some(flag_path) } else { None },
                token_override: if matches!(role, Role::Token) { Some(flag_path) } else { None },
                gateway_kind: GatewayKind::Http,
                transport: "local-development",
                auth: Some("external"),
                include_gateway: true,
                expect_exit,
                expect_stderr: Some(stderr),
                deadline_ms: REJECT_DEADLINE_MS,
                expect_http: 0,
                check_body: false,
                tls_material: None,
            },
        );
        drop(holder);
        cases.push(case);
    }
}

fn preopen_cases(root: &Path, cases: &mut Vec<Case>) {
    let fifo = root.join("preopen-token-fifo");
    make_fifo(&fifo);
    let token = fresh_token();
    let sql = fresh_sql();
    let token_path = root.join("preopen-unused-token");
    write_mode(&token_path, token.as_bytes(), 0o600);

    cases.push(drive(
        "preopen_sandbox_fifo",
        Drive {
            token_path: &token_path,
            token: &token,
            sql: &sql,
            sql_path: None,
            ca_path: None,
            token_override: Some(&fifo),
            gateway_kind: GatewayKind::Http,
            transport: "local-development",
            auth: Some("sandbox"),
            include_gateway: true,
            expect_exit: 1,
            expect_stderr: Some(static_text("sandbox", SANDBOX_MSG, "not_applicable")),
            deadline_ms: REJECT_DEADLINE_MS,
            expect_http: 0,
            check_body: false,
            tls_material: None,
        },
    ));
    cases.push(drive(
        "preopen_missing_auth_fifo",
        Drive {
            token_path: &token_path,
            token: &token,
            sql: &sql,
            sql_path: None,
            ca_path: None,
            token_override: Some(&fifo),
            gateway_kind: GatewayKind::Http,
            transport: "local-development",
            auth: None,
            include_gateway: true,
            expect_exit: 4,
            expect_stderr: Some(static_text(
                "auth",
                "authentication source is missing",
                "not_replayed",
            )),
            deadline_ms: REJECT_DEADLINE_MS,
            expect_http: 0,
            check_body: false,
            tls_material: None,
        },
    ));
    cases.push(drive(
        "preopen_usage_gateway_fifo",
        Drive {
            token_path: &token_path,
            token: &token,
            sql: &sql,
            sql_path: None,
            ca_path: None,
            token_override: Some(&fifo),
            gateway_kind: GatewayKind::Http,
            transport: "local-development",
            auth: Some("external"),
            include_gateway: false,
            expect_exit: 2,
            expect_stderr: Some(static_text("usage", "gateway is required", "not_applicable")),
            deadline_ms: REJECT_DEADLINE_MS,
            expect_http: 0,
            check_body: false,
            tls_material: None,
        },
    ));
}

fn bound_cases(root: &Path, cases: &mut Vec<Case>) {
    let token = fresh_token();
    let sql = fresh_sql();
    let token_path = root.join("bounds-token");
    write_mode(&token_path, token.as_bytes(), 0o600);

    let over = root.join("token-4098");
    write_mode(&over, &vec![b'A'; 4098], 0o600);
    cases.push(reject_token(
        "token_4098",
        &token_path,
        &token,
        &sql,
        &over,
        4,
        "token file rejected: budget",
    ));

    let sql_over = root.join("sql-65537");
    write_mode(&sql_over, &vec![b'b'; 65_537], 0o600);
    cases.push(reject_sql(
        "sql_65537",
        &token_path,
        &token,
        &sql_over,
        1,
        "sql file rejected: budget",
    ));

    let linked = root.join("token-link-target");
    write_mode(&linked, token.as_bytes(), 0o600);
    let link = root.join("token-symlink");
    std::os::unix::fs::symlink(&linked, &link).unwrap();
    cases.push(reject_token(
        "symlink_token",
        &token_path,
        &token,
        &sql,
        &link,
        4,
        "token file rejected: symlink",
    ));

    let dangling = root.join("token-dangling");
    std::os::unix::fs::symlink(root.join("missing-token-target"), &dangling).unwrap();
    cases.push(reject_token(
        "dangling_token",
        &token_path,
        &token,
        &sql,
        &dangling,
        4,
        "token file rejected: symlink",
    ));

    let dir = root.join("token-dir");
    fs::create_dir(&dir).unwrap();
    cases.push(reject_token(
        "directory_token",
        &token_path,
        &token,
        &sql,
        &dir,
        4,
        "token file rejected: not-file",
    ));

    let sql_dir = root.join("sql-dir");
    fs::create_dir(&sql_dir).unwrap();
    cases.push(reject_sql(
        "directory_sql",
        &token_path,
        &token,
        &sql_dir,
        1,
        "sql file rejected: not-file",
    ));
}

fn positive_cases(root: &Path, cases: &mut Vec<Case>) {
    let token = fresh_token();
    let sql = fresh_sql();
    let token_path = root.join("positive-token");
    write_mode(&token_path, token.as_bytes(), 0o600);
    cases.push(positive(
        "positive_token_0600",
        &token_path,
        &token,
        &sql,
        None,
        None,
    ));

    let sql_path = root.join("positive-sql");
    write_mode(&sql_path, sql.as_bytes(), 0o600);
    cases.push(positive(
        "positive_sql_file",
        &token_path,
        &token,
        &sql,
        Some(&sql_path),
        None,
    ));

    let wide = "A".repeat(4096);
    let wide_path = root.join("token-4096");
    write_mode(&wide_path, wide.as_bytes(), 0o600);
    cases.push(positive(
        "token_4096",
        &wide_path,
        &wide,
        &sql,
        None,
        None,
    ));

    let mut wide_nl = wide.into_bytes();
    wide_nl.push(b'\n');
    let wide_nl_path = root.join("token-4096-nl");
    write_mode(&wide_nl_path, &wide_nl, 0o600);
    let wide = "A".repeat(4096);
    cases.push(positive(
        "token_4096_nl",
        &wide_nl_path,
        &wide,
        &sql,
        None,
        None,
    ));

    let big = "a".repeat(65_536);
    let big_path = root.join("sql-65536");
    write_mode(&big_path, big.as_bytes(), 0o600);
    cases.push(positive(
        "sql_65536",
        &token_path,
        &token,
        &big,
        Some(&big_path),
        None,
    ));

    let hard = root.join("token-hardlink");
    fs::hard_link(&token_path, &hard).unwrap();
    cases.push(positive(
        "hardlink_token",
        &hard,
        &token,
        &sql,
        None,
        None,
    ));
}

fn tls_cases(root: &Path, cases: &mut Vec<Case>) {
    let token = fresh_token();
    let sql = fresh_sql();
    let token_path = root.join("tls-token");
    write_mode(&token_path, token.as_bytes(), 0o600);
    let ca = PathBuf::from("/src/contracts/sdk/json-query/fixtures/tls/ca.pem");
    cases.push(drive(
        "tls_valid_ca",
        Drive {
            token_path: &token_path,
            token: &token,
            sql: &sql,
            sql_path: None,
            ca_path: Some(&ca),
            token_override: None,
            gateway_kind: GatewayKind::HttpsLocalhost,
            transport: "production",
            auth: Some("external"),
            include_gateway: true,
            expect_exit: 0,
            expect_stderr: None,
            deadline_ms: POSITIVE_DEADLINE_MS,
            expect_http: 1,
            check_body: true,
            tls_material: Some(TlsMaterial {
                cert: "localhost.pem",
                key: "localhost.key",
            }),
        },
    ));
    cases.push(drive(
        "tls_wrong_host",
        Drive {
            token_path: &token_path,
            token: &token,
            sql: &sql,
            sql_path: None,
            ca_path: Some(&ca),
            token_override: None,
            gateway_kind: GatewayKind::HttpsLoopback,
            transport: "production",
            auth: Some("external"),
            include_gateway: true,
            expect_exit: 4,
            expect_stderr: None,
            deadline_ms: POSITIVE_DEADLINE_MS,
            expect_http: 0,
            check_body: false,
            tls_material: Some(TlsMaterial {
                cert: "localhost.pem",
                key: "localhost.key",
            }),
        },
    ));
    cases.push(drive(
        "tls_wrong_ca",
        Drive {
            token_path: &token_path,
            token: &token,
            sql: &sql,
            sql_path: None,
            ca_path: Some(&ca),
            token_override: None,
            gateway_kind: GatewayKind::HttpsLocalhost,
            transport: "production",
            auth: Some("external"),
            include_gateway: true,
            expect_exit: 4,
            expect_stderr: None,
            deadline_ms: POSITIVE_DEADLINE_MS,
            expect_http: 0,
            check_body: false,
            tls_material: Some(TlsMaterial {
                cert: "other-localhost.pem",
                key: "other-localhost.key",
            }),
        },
    ));
    let _ = root;
}

#[derive(Clone, Copy)]
enum Role {
    Token,
    Sql,
    Ca,
}

#[derive(Clone, Copy)]
enum GatewayKind {
    Http,
    HttpsLocalhost,
    HttpsLoopback,
}

struct TlsMaterial {
    cert: &'static str,
    key: &'static str,
}

struct Drive<'a> {
    token_path: &'a Path,
    token: &'a str,
    sql: &'a str,
    sql_path: Option<&'a Path>,
    ca_path: Option<&'a Path>,
    token_override: Option<&'a Path>,
    gateway_kind: GatewayKind,
    transport: &'static str,
    auth: Option<&'static str>,
    include_gateway: bool,
    expect_exit: i32,
    expect_stderr: Option<Vec<u8>>,
    deadline_ms: u64,
    expect_http: usize,
    check_body: bool,
    tls_material: Option<TlsMaterial>,
}

fn reject_token(
    id: &str,
    token_path: &Path,
    token: &str,
    sql: &str,
    override_path: &Path,
    expect_exit: i32,
    message: &str,
) -> Case {
    drive(
        id,
        Drive {
            token_path,
            token,
            sql,
            sql_path: None,
            ca_path: None,
            token_override: Some(override_path),
            gateway_kind: GatewayKind::Http,
            transport: "local-development",
            auth: Some("external"),
            include_gateway: true,
            expect_exit,
            expect_stderr: Some(static_text("auth", message, "not_replayed")),
            deadline_ms: REJECT_DEADLINE_MS,
            expect_http: 0,
            check_body: false,
            tls_material: None,
        },
    )
}

fn reject_sql(
    id: &str,
    token_path: &Path,
    token: &str,
    sql_path: &Path,
    expect_exit: i32,
    message: &str,
) -> Case {
    let sql = "select unused";
    drive(
        id,
        Drive {
            token_path,
            token,
            sql,
            sql_path: Some(sql_path),
            ca_path: None,
            token_override: None,
            gateway_kind: GatewayKind::Http,
            transport: "local-development",
            auth: Some("external"),
            include_gateway: true,
            expect_exit,
            expect_stderr: Some(static_text("local", message, "not_applicable")),
            deadline_ms: REJECT_DEADLINE_MS,
            expect_http: 0,
            check_body: false,
            tls_material: None,
        },
    )
}

fn positive(
    id: &str,
    token_path: &Path,
    token: &str,
    sql: &str,
    sql_path: Option<&Path>,
    ca_path: Option<&Path>,
) -> Case {
    drive(
        id,
        Drive {
            token_path,
            token,
            sql,
            sql_path,
            ca_path,
            token_override: None,
            gateway_kind: GatewayKind::Http,
            transport: "local-development",
            auth: Some("external"),
            include_gateway: true,
            expect_exit: 0,
            expect_stderr: None,
            deadline_ms: POSITIVE_DEADLINE_MS,
            expect_http: 1,
            check_body: true,
            tls_material: None,
        },
    )
}

struct Counts {
    accepts: usize,
    http: usize,
    auth_match: bool,
    sql_match: bool,
    path_match: bool,
}

fn drive(id: &str, spec: Drive<'_>) -> Case {
    let body = fs::read("/src/contracts/sdk/json-query/fixtures/query-response-empty.json")
        .unwrap_or_default();
    let response = http_ok(&body);
    let stop = Arc::new(AtomicBool::new(false));
    let accepts = Arc::new(AtomicUsize::new(0));
    let http = Arc::new(AtomicUsize::new(0));
    let auth_match = Arc::new(AtomicBool::new(false));
    let sql_match = Arc::new(AtomicBool::new(false));
    let path_match = Arc::new(AtomicBool::new(false));
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.set_nonblocking(true).expect("nonblocking");
    let port = listener.local_addr().expect("port").port();
    let tls_config = spec.tls_material.as_ref().map(|material| tls_config(material));

    let outcome = thread::scope(|scope| {
        let stop_l = Arc::clone(&stop);
        let accepts_l = Arc::clone(&accepts);
        let http_l = Arc::clone(&http);
        let auth_l = Arc::clone(&auth_match);
        let sql_l = Arc::clone(&sql_match);
        let path_l = Arc::clone(&path_match);
        let response = response.clone();
        let token = spec.token.to_owned();
        let sql = spec.sql.to_owned();
        scope.spawn(move || {
            serve(
                listener,
                stop_l,
                accepts_l,
                http_l,
                auth_l,
                sql_l,
                path_l,
                token,
                sql,
                response,
                tls_config,
            );
        });

        let gateway = match spec.gateway_kind {
            GatewayKind::Http => format!("http://127.0.0.1:{port}/"),
            GatewayKind::HttpsLocalhost => format!("https://localhost:{port}/"),
            GatewayKind::HttpsLoopback => format!("https://127.0.0.1:{port}/"),
        };
        let argv = argv_for(&spec, &gateway);
        let started = Instant::now();
        let mut command = Command::new(env!("CARGO_BIN_EXE_oo"));
        command
            .args(&argv)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("spawn oo");
        let mut stdout_pipe = child.stdout.take().expect("stdout");
        let mut stderr_pipe = child.stderr.take().expect("stderr");
        let (stdout, stderr, status) = thread::scope(|inner| {
            let out_thread = inner.spawn(|| read_all(&mut stdout_pipe));
            let err_thread = inner.spawn(|| read_all(&mut stderr_pipe));
            let status = wait_child(&mut child, Duration::from_millis(spec.deadline_ms), started);
            (
                out_thread.join().unwrap_or_default(),
                err_thread.join().unwrap_or_default(),
                status,
            )
        });
        let elapsed = started.elapsed().as_millis();
        stop.store(true, Ordering::SeqCst);
        (stdout, stderr, status, elapsed)
    });

    let (stdout, stderr, status, elapsed) = outcome;
    let counts = Counts {
        accepts: accepts.load(Ordering::SeqCst),
        http: http.load(Ordering::SeqCst),
        auth_match: auth_match.load(Ordering::SeqCst),
        sql_match: sql_match.load(Ordering::SeqCst),
        path_match: path_match.load(Ordering::SeqCst),
    };
    score(id, &spec, stdout, stderr, status, elapsed, counts)
}

fn argv_for(spec: &Drive<'_>, gateway: &str) -> Vec<String> {
    let mut argv = vec!["query".to_owned()];
    if let Some(auth) = spec.auth {
        argv.push("--auth".to_owned());
        argv.push(auth.to_owned());
    }
    let token_file = spec.token_override.unwrap_or(spec.token_path);
    argv.push("--token-file".to_owned());
    argv.push(token_file.display().to_string());
    if spec.include_gateway {
        argv.push("--gateway".to_owned());
        argv.push(gateway.to_owned());
    }
    argv.push("--transport".to_owned());
    argv.push(spec.transport.to_owned());
    argv.push("--timeout-ms".to_owned());
    argv.push(REQUEST_TIMEOUT_MS.to_owned());
    if let Some(ca) = spec.ca_path {
        argv.push("--ca-file".to_owned());
        argv.push(ca.display().to_string());
    }
    if let Some(sql_path) = spec.sql_path {
        argv.push("-f".to_owned());
        argv.push(sql_path.display().to_string());
    } else {
        argv.push(spec.sql.to_owned());
    }
    argv
}

fn score(
    id: &str,
    spec: &Drive<'_>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    status: ExitStatus,
    elapsed: u128,
    counts: Counts,
) -> Case {
    let signal = status.signal();
    let exit_code = status.code();
    let killed_as_exit1 = signal.is_some() && spec.expect_exit == 1;
    let secret_absent = !leaks(&stdout, spec.token, spec.sql) && !leaks(&stderr, spec.token, spec.sql);
    let stderr_exact = match &spec.expect_stderr {
        Some(expected) => stderr == *expected,
        None => true,
    };
    let stdout_ok = if spec.expect_stderr.is_some() {
        stdout.is_empty()
    } else {
        true
    };
    let exit_ok = signal.is_none() && exit_code == Some(spec.expect_exit);
    let time_ok = elapsed <= u128::from(spec.deadline_ms);
    let http_ok = counts.http == spec.expect_http;
    let accept_ok = if spec.expect_http == 0 && spec.tls_material.is_none() {
        counts.accepts == 0
    } else {
        true
    };
    let body_ok = if spec.check_body {
        counts.auth_match && counts.sql_match && counts.path_match && counts.accepts >= 1
    } else {
        true
    };
    let pass = exit_ok
        && time_ok
        && stderr_exact
        && stdout_ok
        && http_ok
        && accept_ok
        && body_ok
        && secret_absent
        && !killed_as_exit1;
    store_streams(id, &stdout, &stderr, secret_absent);
    let mut note = String::new();
    if !exit_ok {
        note.push_str("exit ");
    }
    if !time_ok {
        note.push_str("deadline ");
    }
    if !stderr_exact {
        note.push_str("stderr ");
    }
    if !stdout_ok {
        note.push_str("stdout ");
    }
    if !http_ok {
        note.push_str("http ");
    }
    if !accept_ok {
        note.push_str("accept ");
    }
    if !body_ok {
        note.push_str("body ");
    }
    if !secret_absent {
        note.push_str("secret ");
    }
    if killed_as_exit1 {
        note.push_str("killed ");
    }
    println!(
        "case={id} pass={pass} exit={} signal={} elapsed_ms={elapsed} accepts={} http={} stdout_len={} stderr_len={} stderr_exact={stderr_exact} secret_absent={secret_absent}",
        fmt_opt(exit_code),
        fmt_opt(signal),
        counts.accepts,
        counts.http,
        stdout.len(),
        stderr.len()
    );
    let case = Case {
        id: id.to_owned(),
        pass,
        exit_code,
        signal,
        elapsed_ms: elapsed,
        stdout_len: stdout.len(),
        stderr_len: stderr.len(),
        stderr_exact,
        accepts: counts.accepts,
        http_completed: counts.http,
        auth_match: counts.auth_match,
        sql_match: counts.sql_match,
        path_match: counts.path_match,
        secret_absent,
        killed_as_exit1,
        deadline_ms: spec.deadline_ms,
        note,
    };
    write_meta(&case);
    case
}

fn serve(
    listener: TcpListener,
    stop: Arc<AtomicBool>,
    accepts: Arc<AtomicUsize>,
    http: Arc<AtomicUsize>,
    auth_match: Arc<AtomicBool>,
    sql_match: Arc<AtomicBool>,
    path_match: Arc<AtomicBool>,
    token: String,
    sql: String,
    response: Vec<u8>,
    tls_config: Option<Arc<rustls::ServerConfig>>,
) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((mut sock, _)) => {
                accepts.fetch_add(1, Ordering::SeqCst);
                let _ = sock.set_nonblocking(false);
                let _ = sock.set_read_timeout(Some(Duration::from_millis(1_500)));
                let _ = sock.set_write_timeout(Some(Duration::from_millis(1_500)));
                if let Some(config) = &tls_config {
                    handle_tls(
                        sock,
                        config,
                        &response,
                        &token,
                        &sql,
                        &http,
                        &auth_match,
                        &sql_match,
                        &path_match,
                    );
                } else {
                    handle_plain(
                        &mut sock,
                        &response,
                        &token,
                        &sql,
                        &http,
                        &auth_match,
                        &sql_match,
                        &path_match,
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(_) => break,
        }
    }
}

struct Req {
    first: String,
    head: String,
    body: Vec<u8>,
}

fn handle_plain(
    sock: &mut TcpStream,
    response: &[u8],
    token: &str,
    sql: &str,
    http: &AtomicUsize,
    auth_match: &AtomicBool,
    sql_match: &AtomicBool,
    path_match: &AtomicBool,
) {
    if let Some(req) = read_http(sock) {
        observe(&req, token, sql, auth_match, sql_match, path_match);
        if sock.write_all(response).is_ok() {
            let _ = sock.flush();
            http.fetch_add(1, Ordering::SeqCst);
        }
    }
}

fn handle_tls(
    sock: TcpStream,
    config: &Arc<rustls::ServerConfig>,
    response: &[u8],
    token: &str,
    sql: &str,
    http: &AtomicUsize,
    auth_match: &AtomicBool,
    sql_match: &AtomicBool,
    path_match: &AtomicBool,
) {
    let Ok(conn) = rustls::ServerConnection::new(Arc::clone(config)) else {
        return;
    };
    let mut tls = rustls::StreamOwned::new(conn, sock);
    if let Some(req) = read_http(&mut tls) {
        observe(&req, token, sql, auth_match, sql_match, path_match);
        if tls.write_all(response).is_ok() {
            let _ = tls.flush();
            http.fetch_add(1, Ordering::SeqCst);
        }
    }
}

fn observe(
    req: &Req,
    token: &str,
    sql: &str,
    auth_match: &AtomicBool,
    sql_match: &AtomicBool,
    path_match: &AtomicBool,
) {
    if req.first.starts_with("POST /v1/query ") {
        path_match.store(true, Ordering::SeqCst);
    }
    let expected = format!("Bearer {token}");
    if header_value(&req.head, "authorization").as_deref() == Some(expected.as_str()) {
        auth_match.store(true, Ordering::SeqCst);
    }
    if !sql.is_empty() && req.body.windows(sql.len()).any(|window| window == sql.as_bytes()) {
        sql_match.store(true, Ordering::SeqCst);
    }
}

fn read_http<R: Read>(reader: &mut R) -> Option<Req> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let sep = loop {
        let read = reader.read(&mut tmp).ok()?;
        if read == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..read]);
        if let Some(pos) = buf.windows(4).position(|window| window == b"\r\n\r\n") {
            break pos;
        }
        if buf.len() > 1_048_576 {
            return None;
        }
    };
    let head = String::from_utf8(buf[..sep].to_vec()).ok()?;
    let first = head.lines().next().unwrap_or("").to_owned();
    let length = header_value(&head, "content-length")?.parse::<usize>().ok()?;
    let mut body = buf[sep + 4..].to_vec();
    while body.len() < length {
        let read = reader.read(&mut tmp).ok()?;
        if read == 0 {
            return None;
        }
        body.extend_from_slice(&tmp[..read]);
        if body.len() > length + 65_536 {
            return None;
        }
    }
    body.truncate(length);
    Some(Req { first, head, body })
}

fn header_value(head: &str, name: &str) -> Option<String> {
    for line in head.split("\r\n").skip(1) {
        if let Some((key, value)) = line.split_once(':') {
            if key.eq_ignore_ascii_case(name) {
                return Some(value.trim().to_owned());
            }
        }
    }
    None
}

fn tls_config(material: &TlsMaterial) -> Arc<rustls::ServerConfig> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let dir = Path::new("/src/contracts/sdk/json-query/fixtures/tls");
    let cert_pem = fs::read_to_string(dir.join(material.cert)).expect("cert");
    let key_pem = fs::read_to_string(dir.join(material.key)).expect("key");
    let cert = rustls::pki_types::CertificateDer::from(pem_block(&cert_pem, "CERTIFICATE"));
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(
        rustls::pki_types::PrivatePkcs8KeyDer::from(pem_block(&key_pem, "PRIVATE KEY")),
    );
    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .expect("server config");
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Arc::new(config)
}

fn http_ok(body: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

fn wait_child(child: &mut Child, deadline: Duration, started: Instant) -> ExitStatus {
    loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            return status;
        }
        if started.elapsed() >= deadline {
            let _ = child.kill();
            return child.wait().expect("wait after kill");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn read_all(pipe: &mut impl Read) -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = pipe.read_to_end(&mut buf);
    buf
}

fn static_text(class: &str, message: &str, outcome: &str) -> Vec<u8> {
    format!(
        "error: {class} {message}\nretryable: false\nretry_outcome: {outcome}\n{POLICY}"
    )
    .into_bytes()
}

fn leaks(bytes: &[u8], token: &str, sql: &str) -> bool {
    let text = String::from_utf8_lossy(bytes);
    (!token.is_empty() && text.contains(token))
        || (!sql.is_empty() && sql.len() >= 8 && text.contains(sql))
        || text.contains("Bearer ")
}

fn fresh_token() -> String {
    let mut raw = [0u8; 32];
    let mut file = File::open("/dev/urandom").expect("urandom");
    file.read_exact(&mut raw).expect("urandom read");
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    raw.iter()
        .map(|byte| ALPHABET[usize::from(*byte) % ALPHABET.len()] as char)
        .collect()
}

fn fresh_sql() -> String {
    format!("select {}", fresh_token())
}

fn write_mode(path: &Path, bytes: &[u8], mode: u32) {
    fs::write(path, bytes).expect("write fixture");
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("chmod");
    let got = fs::metadata(path).expect("meta").permissions().mode() & 0o777;
    assert_eq!(got, mode, "fixture mode");
}

fn make_fifo(path: &Path) {
    let status = Command::new("mkfifo").arg(path).status().expect("mkfifo");
    assert!(status.success(), "mkfifo");
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("fifo mode");
}

fn hold_writer(path: &Path) -> File {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .expect("writer fd")
}

fn pem_block(pem: &str, label: &str) -> Vec<u8> {
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let start = pem.find(&begin).expect("pem begin") + begin.len();
    let stop = pem[start..].find(&end).expect("pem end") + start;
    let encoded: String = pem[start..stop]
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect();
    b64_decode(&encoded).expect("pem base64")
}

fn b64_decode(input: &str) -> Option<Vec<u8>> {
    fn value(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes = input.as_bytes();
    if bytes.len() & 3 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let pad = chunk.iter().filter(|byte| **byte == b'=').count();
        if pad > 2 {
            return None;
        }
        let first = value(chunk[0])?;
        let second = value(chunk[1])?;
        let third = if chunk[2] == b'=' { 0 } else { value(chunk[2])? };
        let fourth = if chunk[3] == b'=' { 0 } else { value(chunk[3])? };
        out.push((first << 2) | (second >> 4));
        if pad < 2 {
            out.push((second << 4) | (third >> 2));
        }
        if pad < 1 {
            out.push((third << 6) | fourth);
        }
    }
    Some(out)
}

fn store_streams(id: &str, stdout: &[u8], stderr: &[u8], secret_absent: bool) {
    let dir = case_dir(id);
    let _ = fs::create_dir_all(&dir);
    if secret_absent {
        let _ = fs::write(dir.join("stdout"), stdout);
        let _ = fs::write(dir.join("stderr"), stderr);
    } else {
        let _ = fs::write(dir.join("stdout"), b"redacted\n");
        let _ = fs::write(dir.join("stderr"), b"redacted\n");
    }
}

fn write_meta(case: &Case) {
    let dir = case_dir(&case.id);
    let _ = fs::create_dir_all(&dir);
    let text = format!(
        "id={}\npass={}\nexit_code={}\nsignal={}\nelapsed_ms={}\nstdout_len={}\nstderr_len={}\nstderr_exact={}\naccepts={}\nhttp_completed={}\nauth_match={}\nsql_match={}\npath_match={}\nsecret_absent={}\nkilled_as_exit1={}\ndeadline_ms={}\nrequest_timeout_ms={}\nnote={}\n",
        case.id,
        case.pass,
        fmt_opt(case.exit_code),
        fmt_opt(case.signal),
        case.elapsed_ms,
        case.stdout_len,
        case.stderr_len,
        case.stderr_exact,
        case.accepts,
        case.http_completed,
        case.auth_match,
        case.sql_match,
        case.path_match,
        case.secret_absent,
        case.killed_as_exit1,
        case.deadline_ms,
        REQUEST_TIMEOUT_MS,
        case.note
    );
    let _ = fs::write(dir.join("meta.txt"), text);
}

fn write_report(cases: &[Case], failed: &[String]) {
    let path = std::env::var("A111_REPORT").unwrap_or_else(|_| "/out/probe-report.json".to_owned());
    let mut body = String::from("{\n  \"cases\": [\n");
    for (index, case) in cases.iter().enumerate() {
        body.push_str(&format!(
            "    {{\"id\":\"{}\",\"pass\":{},\"exit_code\":{},\"signal\":{},\"elapsed_ms\":{},\"stdout_len\":{},\"stderr_len\":{},\"stderr_exact\":{},\"accepts\":{},\"http_completed\":{},\"auth_match\":{},\"sql_match\":{},\"path_match\":{},\"secret_absent\":{},\"killed_as_exit1\":{},\"deadline_ms\":{},\"request_timeout_ms\":{}}}",
            case.id,
            case.pass,
            json_opt(case.exit_code),
            json_opt(case.signal),
            case.elapsed_ms,
            case.stdout_len,
            case.stderr_len,
            case.stderr_exact,
            case.accepts,
            case.http_completed,
            case.auth_match,
            case.sql_match,
            case.path_match,
            case.secret_absent,
            case.killed_as_exit1,
            case.deadline_ms,
            REQUEST_TIMEOUT_MS
        ));
        if index + 1 != cases.len() {
            body.push(',');
        }
        body.push('\n');
    }
    body.push_str(&format!(
        "  ],\n  \"case_count\": {},\n  \"failed\": {},\n  \"failed_ids\": [{}],\n  \"killed_child_is_pass\": false\n}}\n",
        cases.len(),
        failed.len(),
        failed
            .iter()
            .map(|id| format!("\"{id}\""))
            .collect::<Vec<_>>()
            .join(",")
    ));
    let _ = fs::write(path, body);
}

fn case_dir(id: &str) -> PathBuf {
    let root = std::env::var("A111_CASES").unwrap_or_else(|_| "/out/cases".to_owned());
    PathBuf::from(root).join(id)
}

fn fmt_opt(value: Option<i32>) -> String {
    match value {
        Some(number) => number.to_string(),
        None => "none".to_owned(),
    }
}

fn json_opt(value: Option<i32>) -> String {
    match value {
        Some(number) => number.to_string(),
        None => "null".to_owned(),
    }
}

