//! Process supervisor for one SQL admission.
//!
//! The oracle is `guard_select` in this test process. The supervisor under
//! test does not parse SQL. Fault programs are separate `rustc` binaries.
//! The production worker has no fault flag.

use std::cell::Cell;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use oo_gateway::parser_worker::{
    AdmitRequest, CATALOG_REASONS, CONSTRUCTS, CancelFlag, CodecError, KIND_PLAN, KIND_REFUSAL,
    KIND_REQUEST, MAGIC, MAX_FRAME_BODY, PARSE_DEADLINE, PROTOCOL_VERSION, ParserResponse,
    ProcessReport, RESTRICTION_CAPABILITY, RestrictionCapability, SERIALIZE_REASONS,
    SuperviseError, TrustedWorker, WorkerPathError, decode_frame, decode_plan, decode_refusal,
    decode_request, encode_frame, encode_plan, encode_refusal, encode_request, supervise,
    supervise_with,
};
use oo_gateway::query_executor::{UNTRUSTED_ADMISSION, UntrustedAdmission};
use oo_gateway::{
    BoundFunction, BoundOperator, BoundOutputColumn, BoundRelation, GuardError, GuardedPlan,
    LiteralKind, LiteralTypes, OutputItem, PINNED_PARSER_VERSION, PolicyCatalog, PolicyIdentity,
    RecognizedBindings, ResolvedFunction, ResolvedOperator, SQL_BYTE_BUDGET, StarExpansion,
    VisibleColumn, VisibleRelation, guard_select,
};
use sha2::{Digest, Sha256};

const INT_OID: u32 = 11;
const TEXT_OID: u32 = 13;
const BOOL_OID: u32 = 17;
const CANARY: &str = "canary_secret_sql_9f3a";

const FAULT_SRC: &str = r#"
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

fn main() {
    match option_env!("OO_FAULT_MODE").unwrap_or("") {
        "slow" => thread::sleep(Duration::from_secs(30)),
        "crash" => std::process::exit(99),
        "oversize" => oversize(),
        "version" => write_header(99, 2, 0),
        "magic" => {
            let mut out = std::io::stdout().lock();
            out.write_all(b"NOPE").unwrap();
            out.flush().unwrap();
        }
        "echo" => echo(),
        "trailer" => {
            write_header(1, 3, 1);
            let mut out = std::io::stdout().lock();
            out.write_all(&[1, 0xff]).unwrap();
            out.flush().unwrap();
        }
        "delayed" => {
            thread::sleep(Duration::from_millis(250));
            write_header(1, 3, 1);
            let mut out = std::io::stdout().lock();
            out.write_all(&[1]).unwrap();
            out.flush().unwrap();
        }
        "kind" => write_header(1, 99, 0),
        "partial" => {
            let mut out = std::io::stdout().lock();
            out.write_all(b"OOSW").unwrap();
            out.flush().unwrap();
            thread::sleep(Duration::from_secs(30));
        }
        "hold-stdout" => {
            write_header(1, 3, 1);
            let mut out = std::io::stdout().lock();
            out.write_all(&[1]).unwrap();
            out.flush().unwrap();
            thread::sleep(Duration::from_secs(30));
        }
        "late" => {
            thread::sleep(Duration::from_millis(700));
            write_header(1, 3, 1);
            let mut out = std::io::stdout().lock();
            out.write_all(&[1]).unwrap();
            out.flush().unwrap();
        }
        "orphan" => orphan(false),
        "orphan-stdout" => orphan(true),
        "setsid-stdout" => setsid_stdout(),
        _ => std::process::exit(2),
    }
}

fn orphan(inherit_stdout: bool) {
    let mut cmd = Command::new("/bin/sleep");
    cmd.arg("25").stdin(Stdio::null()).stderr(Stdio::null());
    if inherit_stdout {
        cmd.stdout(Stdio::inherit());
    } else {
        cmd.stdout(Stdio::null());
    }
    let child = cmd.spawn().expect("sleep");
    let id = child.id();
    std::mem::forget(child);
    let _ = std::fs::write("grandchild.pid", id.to_string());
    thread::sleep(Duration::from_secs(30));
}

fn setsid_stdout() {
    let setsid = if std::path::Path::new("/usr/bin/setsid").is_file() {
        "/usr/bin/setsid"
    } else if std::path::Path::new("/bin/setsid").is_file() {
        "/bin/setsid"
    } else {
        let _ = std::fs::write("grandchild.pid", "absent");
        thread::sleep(Duration::from_secs(30));
        return;
    };
    let mut cmd = Command::new(setsid);
    cmd.arg("/bin/sleep")
        .arg("25")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::inherit());
    let child = cmd.spawn().expect("setsid");
    let id = child.id();
    std::mem::forget(child);
    let _ = std::fs::write("grandchild.pid", id.to_string());
    thread::sleep(Duration::from_secs(30));
}

fn oversize() {
    write_header(1, 2, u32::MAX);
    thread::sleep(Duration::from_secs(30));
}

fn write_header(version: u16, kind: u16, len: u32) {
    let mut out = std::io::stdout().lock();
    out.write_all(b"OOSW").unwrap();
    out.write_all(&version.to_le_bytes()).unwrap();
    out.write_all(&kind.to_le_bytes()).unwrap();
    out.write_all(&len.to_le_bytes()).unwrap();
    out.flush().unwrap();
}

fn echo() {
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    let mut buf = [0u8; 8192];
    let mut total = 0usize;
    let cap = 1024 * 1024;
    loop {
        let n = input.read(&mut buf).unwrap();
        if n == 0 || total >= cap {
            break;
        }
        let n = n.min(cap - total);
        output.write_all(&buf[..n]).unwrap();
        total += n;
    }
    output.flush().unwrap();
}
"#;

const FAULT_MODES: &[&str] = &[
    "slow",
    "crash",
    "oversize",
    "version",
    "magic",
    "echo",
    "trailer",
    "delayed",
    "kind",
    "partial",
    "hold-stdout",
    "late",
    "orphan",
    "orphan-stdout",
    "setsid-stdout",
];

fn bin_path() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_oo-sql-parser-worker"))
}

fn prod() -> &'static TrustedWorker {
    static WORKER: OnceLock<TrustedWorker> = OnceLock::new();
    WORKER.get_or_init(|| {
        let worker = TrustedWorker::approve(bin_path()).unwrap();
        let _ = supervise(&worker, &request(&small(), "SELECT 1"), &CancelFlag::new());
        worker
    })
}

fn fault_paths() -> &'static Vec<(String, PathBuf)> {
    static ALL: OnceLock<Vec<(String, PathBuf)>> = OnceLock::new();
    ALL.get_or_init(compile_faults)
}

fn compile_faults() -> Vec<(String, PathBuf)> {
    let dir = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("a187-fault");
    fs::create_dir_all(&dir).unwrap();
    let src = dir.join("fault.rs");
    fs::write(&src, FAULT_SRC).unwrap();
    let mut built = Vec::new();
    for mode in FAULT_MODES {
        let bin = dir.join(format!("fault-{mode}"));
        let output = Command::new("rustc")
            .current_dir(&dir)
            .arg("--edition")
            .arg("2024")
            .arg("-D")
            .arg("warnings")
            .arg("-O")
            .arg("-o")
            .arg(&bin)
            .arg(&src)
            .env("OO_FAULT_MODE", mode)
            .output()
            .unwrap_or_else(|err| panic!("rustc {mode}: {err}"));
        assert!(
            output.status.success(),
            "{mode} rustc status {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        built.push(((*mode).to_string(), bin));
    }
    built
}

fn fault(mode: &str) -> TrustedWorker {
    let path = fault_paths()
        .iter()
        .find(|(name, _)| name == mode)
        .unwrap_or_else(|| panic!("missing fault mode {mode}"))
        .1
        .clone();
    TrustedWorker::approve(&path).unwrap()
}

fn identity() -> PolicyIdentity {
    PolicyIdentity {
        project: "orders".to_string(),
        role: "oo_r_dispatcher".to_string(),
        policy_id: "policy-a175".to_string(),
    }
}

fn column(name: &str, physical: &str, type_oid: u32) -> VisibleColumn {
    VisibleColumn {
        name: name.to_string(),
        physical_name: physical.to_string(),
        type_oid,
    }
}

fn shipment(rows: u64) -> VisibleRelation {
    VisibleRelation {
        schema: "logistics".to_string(),
        name: "shipment".to_string(),
        physical_schema: "_oo".to_string(),
        physical_name: "pt_ship".to_string(),
        columns: vec![
            column("order_id", "pc_order", INT_OID),
            column("region", "pc_region", TEXT_OID),
        ],
        pending_unclassified: 1,
        estimated_rows: rows,
    }
}

fn catalog(relations: Vec<VisibleRelation>, threshold: u64) -> PolicyCatalog {
    PolicyCatalog {
        identity: identity(),
        relations,
        functions: vec![ResolvedFunction {
            oid: 201,
            schema: "pg_catalog".to_string(),
            name: "count".to_string(),
            argument_type_oids: Vec::new(),
            aggregate_star: true,
        }],
        operators: vec![
            ResolvedOperator {
                oid: 301,
                procedure_oid: 302,
                schema: "pg_catalog".to_string(),
                name: "=".to_string(),
                left_type_oid: INT_OID,
                right_type_oid: INT_OID,
            },
            ResolvedOperator {
                oid: 303,
                procedure_oid: 304,
                schema: "pg_catalog".to_string(),
                name: "=".to_string(),
                left_type_oid: TEXT_OID,
                right_type_oid: TEXT_OID,
            },
        ],
        literal_types: LiteralTypes {
            integer: Some(INT_OID),
            text: Some(TEXT_OID),
            boolean: Some(BOOL_OID),
        },
        null_test_allowed: true,
        large_table_row_threshold: threshold,
    }
}

fn small() -> PolicyCatalog {
    catalog(vec![shipment(10)], 100)
}

fn large() -> PolicyCatalog {
    catalog(vec![shipment(424_242)], 100)
}

fn request(catalog: &PolicyCatalog, sql: &str) -> AdmitRequest {
    AdmitRequest {
        identity: catalog.identity.clone(),
        catalog: catalog.clone(),
        sql: sql.to_string(),
    }
}

fn digest(text: &str) -> String {
    let bytes = Sha256::digest(text.as_bytes());
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        hex.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
        hex.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
    }
    hex
}

fn proc_field(pid: u32, index: usize) -> Option<u64> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let end = text.rfind(')')?;
    let from = end.checked_add(2)?;
    let rest = text.get(from..)?;
    rest.split_whitespace().nth(index)?.parse().ok()
}

fn start_ticks(pid: u32) -> u64 {
    let started = Instant::now();
    loop {
        if let Some(value) = proc_field(pid, 19) {
            return value;
        }
        if started.elapsed() > Duration::from_millis(200) {
            panic!("no /proc/{pid}/stat starttime");
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn live_child(pid: u32, worker: &TrustedWorker) -> u64 {
    let start = start_ticks(pid);
    let pgrp = proc_field(pid, 2).unwrap_or_else(|| panic!("no pgrp for {pid}"));
    assert_eq!(
        pgrp,
        u64::from(pid),
        "worker did not get its own process group"
    );
    let env = fs::read(format!("/proc/{pid}/environ")).unwrap();
    assert!(
        env.is_empty(),
        "child inherited environment bytes {}",
        env.len()
    );
    let cmd = fs::read(format!("/proc/{pid}/cmdline")).unwrap();
    let parts: Vec<_> = cmd
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .collect();
    assert_eq!(parts, vec![worker.path().as_os_str().as_bytes()]);
    let exe = fs::read_link(format!("/proc/{pid}/exe")).unwrap();
    assert_eq!(exe, worker.path());
    let cwd = fs::read_link(format!("/proc/{pid}/cwd")).unwrap();
    let name = cwd.file_name().unwrap().to_string_lossy();
    assert!(name.starts_with("oo-parser-worker-"), "{cwd:?}");
    start
}

fn assert_gone(report: &ProcessReport, pid: u32, start: u64) {
    assert_eq!(report.pid, pid);
    assert_eq!(report.start_ticks, Some(start));
    assert!(report.reaped, "{report:?}");
    assert!(!report.same_pid_alive, "{report:?}");
    assert!(report.pid_absent || report.pid_reused, "{report:?}");
    if let Some(now) = proc_field(pid, 19) {
        assert_ne!(now, start, "pid {pid} still has the child starttime");
    }
}

fn assert_killed(report: &ProcessReport) {
    assert_eq!(report.term_signal, Some(9), "{report:?}");
    assert!(report.reaped, "{report:?}");
    assert!(!report.same_pid_alive, "{report:?}");
    assert!(report.elapsed >= PARSE_DEADLINE, "{report:?}");
    assert!(report.elapsed < Duration::from_secs(3), "{report:?}");
}

fn supervise_ok(req: &AdmitRequest, mut on_start: impl FnMut(u32)) -> ParserResponse {
    let mut last = String::new();
    for _ in 0..3 {
        match supervise_with(prod(), req, &CancelFlag::new(), &mut on_start) {
            Ok(response) => return response,
            Err(SuperviseError::TimedOut(report)) => {
                assert_killed(&report);
                last = format!("{report:?}");
            }
            Err(err) => panic!("{err:?}"),
        }
    }
    panic!("worker stayed past the deadline: {last}");
}

fn expect_plan(catalog: &PolicyCatalog, sql: &str) -> GuardedPlan {
    let req = request(catalog, sql);
    let expected = guard_select(&req.identity, &req.catalog, sql).unwrap();
    match supervise_ok(&req, |_| {}) {
        ParserResponse::Plan(plan) => {
            assert_eq!(*plan, expected);
            *plan
        }
        ParserResponse::Refusal(err) => panic!("{sql} refused {err:?}"),
    }
}

fn expect_refusal(catalog: &PolicyCatalog, sql: &str) -> GuardError {
    let req = request(catalog, sql);
    let expected = guard_select(&req.identity, &req.catalog, sql).expect_err("oracle");
    let mut last = String::new();
    for _ in 0..3 {
        match supervise(prod(), &req, &CancelFlag::new()) {
            Ok(ParserResponse::Refusal(err)) => {
                assert_eq!(err, expected);
                return err;
            }
            Ok(ParserResponse::Plan(plan)) => panic!("{sql} admitted {}", plan.executed_sql),
            Err(SuperviseError::TimedOut(report)) => {
                assert_killed(&report);
                last = format!("{report:?}");
            }
            Err(err) => panic!("{sql} -> {err:?}"),
        }
    }
    panic!("refusal timed out: {last}");
}

fn push_str(buf: &mut Vec<u8>, text: &str) {
    let len = u32::try_from(text.len()).unwrap();
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(text.as_bytes());
}

fn push_identities(buf: &mut Vec<u8>) {
    for _ in 0..2 {
        push_str(buf, "p");
        push_str(buf, "r");
        push_str(buf, "i");
    }
}

fn sample_plan() -> GuardedPlan {
    GuardedPlan {
        identity: identity(),
        original_sql: "SELECT 1".to_string(),
        executed_sql: "SELECT 1".to_string(),
        original_sha256: "ab".repeat(32),
        executed_sha256: "cd".repeat(32),
        fingerprint_hex: "ef".to_string(),
        parser_version: PINNED_PARSER_VERSION,
        star_expansions: vec![StarExpansion {
            qualifier: "logistics.shipment".to_string(),
            schema: "logistics".to_string(),
            name: "shipment".to_string(),
            columns: vec!["order_id".to_string(), "region".to_string()],
        }],
        bindings: RecognizedBindings {
            relations: vec![
                BoundRelation {
                    schema: "logistics".to_string(),
                    name: "shipment".to_string(),
                    alias: Some("s".to_string()),
                    physical_schema: "_oo".to_string(),
                    physical_name: "pt_ship".to_string(),
                },
                BoundRelation {
                    schema: "logistics".to_string(),
                    name: "customer".to_string(),
                    alias: None,
                    physical_schema: "_oo".to_string(),
                    physical_name: "pt_cust".to_string(),
                },
            ],
            outputs: vec![
                OutputItem::Column(BoundOutputColumn {
                    relation_schema: "logistics".to_string(),
                    relation_name: "shipment".to_string(),
                    name: "order_id".to_string(),
                    physical_name: "pc_order".to_string(),
                    output_name: "order_id".to_string(),
                    type_oid: INT_OID,
                }),
                OutputItem::Function(BoundFunction {
                    oid: 201,
                    schema: "pg_catalog".to_string(),
                    name: "count".to_string(),
                    aggregate_star: true,
                }),
                OutputItem::Literal(LiteralKind::Integer),
                OutputItem::Literal(LiteralKind::Text),
                OutputItem::Literal(LiteralKind::Boolean),
                OutputItem::Literal(LiteralKind::Null),
                OutputItem::Literal(LiteralKind::Date),
                OutputItem::Compound,
            ],
            functions: vec![BoundFunction {
                oid: 201,
                schema: "pg_catalog".to_string(),
                name: "count".to_string(),
                aggregate_star: true,
            }],
            operators: vec![BoundOperator {
                oid: 301,
                procedure_oid: 302,
                schema: "pg_catalog".to_string(),
                name: "=".to_string(),
            }],
            null_tests: 2,
        },
    }
}

fn refusal_matrix() -> Vec<GuardError> {
    let mut errors = vec![
        GuardError::IdentityMismatch,
        GuardError::Parse,
        GuardError::Budget,
        GuardError::Statement,
        GuardError::Write,
        GuardError::Into,
        GuardError::Locking,
        GuardError::SystemSchema,
        GuardError::FunctionNotAllowed,
        GuardError::RelationNotFound,
        GuardError::ColumnNotAllowed {
            available: vec!["s.order_id".to_string(), "s.region".to_string()],
            pending: 1,
            ambiguous: true,
        },
        GuardError::ColumnNotAllowed {
            available: Vec::new(),
            pending: 0,
            ambiguous: false,
        },
        GuardError::Unqualified {
            suggestion: Some("logistics.shipment".to_string()),
        },
        GuardError::Unqualified { suggestion: None },
        GuardError::CostTooHigh {
            relation: "logistics.shipment".to_string(),
        },
    ];
    for &reason in CATALOG_REASONS {
        errors.push(GuardError::CatalogInvalid { reason });
    }
    for &reason in SERIALIZE_REASONS {
        errors.push(GuardError::Serialize { reason });
    }
    for &construct in CONSTRUCTS {
        errors.push(GuardError::Unsupported { construct });
    }
    errors
}

fn drain(mut pipe: impl Read + Send + 'static, cap: usize) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut got = Vec::new();
        loop {
            let read = match pipe.read(&mut buf) {
                Ok(0) => break,
                Ok(count) => count,
                Err(_) => break,
            };
            if got.len() >= cap {
                break;
            }
            let count = read.min(cap - got.len());
            got.extend_from_slice(&buf[..count]);
        }
        got
    })
}

fn wait_limited(child: &mut Child, limit: Duration) -> ExitStatus {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status,
            Ok(None) if started.elapsed() > limit => {
                let _ = child.kill();
                return child.wait().unwrap();
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(err) => panic!("{err}"),
        }
    }
}

fn collect_worker(input: &[u8], configure: impl FnOnce(&mut Command)) -> (i32, Vec<u8>, Vec<u8>) {
    let mut cmd = Command::new(bin_path());
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure(&mut cmd);
    let mut child = cmd.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = drain(child.stdout.take().unwrap(), 256 * 1024);
    let stderr = drain(child.stderr.take().unwrap(), 16 * 1024);
    if !input.is_empty() {
        stdin.write_all(input).unwrap();
        stdin.flush().unwrap();
    }
    drop(stdin);
    let status = wait_limited(&mut child, Duration::from_secs(3));
    let out = stdout.join().unwrap();
    let err = stderr.join().unwrap();
    (status.code().unwrap_or(-1), out, err)
}

fn assert_exit2(configure: impl FnOnce(&mut Command)) {
    let (code, out, err) = collect_worker(&[], configure);
    assert_eq!(code, 2, "stderr {}", String::from_utf8_lossy(&err));
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn admission_stays_blocked_and_the_sandbox_is_unsupported() {
    let restriction = RESTRICTION_CAPABILITY;
    let admission = UNTRUSTED_ADMISSION;
    let version = PROTOCOL_VERSION;
    let request_kind = KIND_REQUEST;
    let plan_kind = KIND_PLAN;
    let refusal_kind = KIND_REFUSAL;
    let magic = MAGIC;
    let deadline = PARSE_DEADLINE;
    assert_eq!(restriction, RestrictionCapability::Unsupported);
    assert_eq!(admission, UntrustedAdmission::Blocked);
    assert_eq!(version, 1);
    assert_eq!(request_kind, 1);
    assert_eq!(plan_kind, 2);
    assert_eq!(refusal_kind, 3);
    assert_eq!(magic, *b"OOSW");
    assert_eq!(deadline, Duration::from_millis(500));
    assert_eq!(
        SuperviseError::WorkerPath.to_string(),
        "worker path was not approved"
    );
    assert_eq!(
        SuperviseError::RequestBudget.to_string(),
        "parser request exceeds its budget"
    );
    assert_eq!(
        CodecError::Limit.to_string(),
        "frame field exceeds its budget"
    );
    assert_eq!(
        WorkerPathError::NotAbsolute.to_string(),
        "worker path is not absolute"
    );
}

#[test]
fn worker_path_is_a_separate_approval() {
    let approved = TrustedWorker::approve(bin_path()).unwrap();
    assert!(approved.path().is_absolute());
    assert_eq!(approved.path(), &bin_path().canonicalize().unwrap());
    assert_eq!(
        TrustedWorker::approve(Path::new("oo-sql-parser-worker")).unwrap_err(),
        WorkerPathError::NotAbsolute
    );
    assert_eq!(
        TrustedWorker::approve(Path::new("/")).unwrap_err(),
        WorkerPathError::Unavailable
    );
    assert_eq!(
        TrustedWorker::approve(Path::new("/no/such/oo-a187-worker")).unwrap_err(),
        WorkerPathError::Unavailable
    );
}

#[test]
fn frame_v1_rejects_a_bad_header_before_a_large_body() {
    let body = encode_request(&request(&small(), "SELECT 1")).unwrap();
    let frame = encode_frame(KIND_REQUEST, &body).unwrap();
    assert_eq!(&frame[..4], b"OOSW");
    assert_eq!(u16::from_le_bytes([frame[4], frame[5]]), PROTOCOL_VERSION);
    let decoded = decode_frame(&frame).unwrap();
    assert_eq!(decoded.version, 1);
    assert_eq!(decoded.kind, KIND_REQUEST);
    assert_eq!(decoded.body, body);
    assert_eq!(decode_frame(b"OOS"), Err(CodecError::Truncated));
    assert_eq!(decode_frame(b"XXXX"), Err(CodecError::Magic));
    assert_eq!(decode_frame(b"OOSW"), Err(CodecError::Truncated));
    let mut version = frame.clone();
    version[4] = 99;
    assert_eq!(decode_frame(&version), Err(CodecError::Version));
    let mut trailed = frame;
    trailed.push(0);
    assert_eq!(decode_frame(&trailed), Err(CodecError::Trailing));
    let mut header = [0u8; 12];
    header[..4].copy_from_slice(b"OOSW");
    header[4..6].copy_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    header[6..8].copy_from_slice(&KIND_PLAN.to_le_bytes());
    let over = u32::try_from(MAX_FRAME_BODY).unwrap().saturating_add(1);
    header[8..12].copy_from_slice(&over.to_le_bytes());
    assert_eq!(decode_frame(&header), Err(CodecError::Limit));
    header[8..12].copy_from_slice(&5u32.to_le_bytes());
    assert_eq!(decode_frame(&header), Err(CodecError::Truncated));
}

#[test]
fn encode_rejects_a_body_past_the_frame_cap() {
    let body = vec![0u8; MAX_FRAME_BODY + 1];
    assert_eq!(encode_frame(KIND_PLAN, &body), Err(CodecError::Limit));
}

#[test]
fn catalog_oids_and_options_roundtrip() {
    let req = request(&small(), "SELECT 1");
    let back = decode_request(&encode_request(&req).unwrap()).unwrap();
    assert_eq!(back, req);
    assert_eq!(back.catalog.relations[0].columns[0].type_oid, INT_OID);
    assert_eq!(back.catalog.relations[0].columns[1].type_oid, TEXT_OID);
    assert_eq!(back.catalog.relations[0].pending_unclassified, 1);
    assert_eq!(back.catalog.functions[0].oid, 201);
    assert!(back.catalog.functions[0].aggregate_star);
    assert_eq!(back.catalog.operators[0].oid, 301);
    assert_eq!(back.catalog.operators[0].procedure_oid, 302);
    assert_eq!(back.catalog.operators[1].oid, 303);
    assert_eq!(back.catalog.operators[1].procedure_oid, 304);
    assert_eq!(back.catalog.literal_types.integer, Some(INT_OID));
    assert_eq!(back.catalog.literal_types.text, Some(TEXT_OID));
    assert_eq!(back.catalog.literal_types.boolean, Some(BOOL_OID));
    assert_eq!(back.catalog.large_table_row_threshold, 100);

    let mut optional = small();
    optional.literal_types = LiteralTypes {
        integer: None,
        text: None,
        boolean: Some(BOOL_OID),
    };
    optional.null_test_allowed = false;
    optional.functions[0].argument_type_oids = vec![INT_OID, TEXT_OID];
    optional.functions[0].aggregate_star = false;
    optional.relations.clear();
    let req = request(&optional, "SELECT 1");
    assert_eq!(decode_request(&encode_request(&req).unwrap()).unwrap(), req);
}

#[test]
fn string_and_count_caps_fail_before_the_payload_is_taken() {
    let mut huge = Vec::new();
    huge.extend_from_slice(&u32::MAX.to_le_bytes());
    let started = Instant::now();
    assert_eq!(decode_request(&huge), Err(CodecError::Limit));
    assert!(started.elapsed() < Duration::from_millis(200));

    let mut over_ident = Vec::new();
    over_ident.extend_from_slice(&129u32.to_le_bytes());
    assert_eq!(decode_request(&over_ident), Err(CodecError::Limit));

    let mut bad_utf8 = Vec::new();
    bad_utf8.extend_from_slice(&2u32.to_le_bytes());
    bad_utf8.extend_from_slice(&[0xff, 0xff]);
    assert_eq!(decode_request(&bad_utf8), Err(CodecError::Utf8));

    let mut count = Vec::new();
    push_identities(&mut count);
    count.extend_from_slice(&u32::MAX.to_le_bytes());
    let started = Instant::now();
    assert_eq!(decode_request(&count), Err(CodecError::Limit));
    assert!(started.elapsed() < Duration::from_millis(200));
}

#[test]
fn tags_and_trailing_bytes_are_rejected() {
    let mut body = Vec::new();
    push_identities(&mut body);
    body.extend_from_slice(&0u32.to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes());
    body.push(0);
    body.push(0);
    body.push(0);
    body.push(1);
    body.extend_from_slice(&0u64.to_le_bytes());
    push_str(&mut body, "SELECT 1");
    assert!(decode_request(&body).is_ok());
    let mut flagged = body.clone();
    let mut prefix = Vec::new();
    push_identities(&mut prefix);
    let flag_at = prefix.len() + 12 + 3;
    flagged[flag_at] = 2;
    assert_eq!(decode_request(&flagged), Err(CodecError::Tag));
    let mut optional = body.clone();
    optional[prefix.len() + 12] = 2;
    assert_eq!(decode_request(&optional), Err(CodecError::Tag));
    body.push(0);
    assert_eq!(decode_request(&body), Err(CodecError::Trailing));

    assert_eq!(decode_refusal(&[0]), Err(CodecError::Tag));
    assert_eq!(decode_refusal(&[3, 0]), Err(CodecError::Trailing));
    let mut unknown = Vec::new();
    unknown.push(10);
    push_str(&mut unknown, "nope");
    assert_eq!(decode_refusal(&unknown), Err(CodecError::Label));
    assert_eq!(
        encode_refusal(&GuardError::Unsupported { construct: "nope" }),
        Err(CodecError::Label)
    );
}

#[test]
fn plans_and_every_refusal_class_roundtrip() {
    let plan = sample_plan();
    assert_eq!(decode_plan(&encode_plan(&plan).unwrap()).unwrap(), plan);
    let admitted = guard_select(
        &small().identity,
        &small(),
        "SELECT * FROM logistics.shipment",
    )
    .unwrap();
    assert_eq!(
        decode_plan(&encode_plan(&admitted).unwrap()).unwrap(),
        admitted
    );
    for err in refusal_matrix() {
        let bytes = encode_refusal(&err).unwrap();
        assert_eq!(decode_refusal(&bytes).unwrap(), err);
        assert!(
            !bytes
                .windows(CANARY.len())
                .any(|item| item == CANARY.as_bytes())
        );
    }
}

#[test]
fn identifier_length_is_enforced_at_the_codec_boundary() {
    let mut req = request(&small(), "SELECT 1");
    req.catalog.relations[0].name = "a".repeat(63);
    let back = decode_request(&encode_request(&req).unwrap()).unwrap();
    assert_eq!(back.catalog.relations[0].name.len(), 63);
    req.catalog.relations[0].name = "a".repeat(64);
    let started = Cell::new(false);
    let err = supervise_with(&fault("slow"), &req, &CancelFlag::new(), |_| {
        started.set(true)
    })
    .unwrap_err();
    assert!(matches!(err, SuperviseError::RequestBudget));
    assert!(!started.get());
}

#[test]
fn legal_select_matches_the_guard_including_hash_and_physical_names() {
    let sql = "SELECT/*x*/ order_id, region FROM logistics.shipment";
    let plan = expect_plan(&small(), sql);
    assert_eq!(plan.original_sql, sql);
    assert_ne!(plan.executed_sql, sql);
    assert!(
        plan.executed_sql.contains("pt_ship"),
        "{}",
        plan.executed_sql
    );
    assert!(
        plan.executed_sql.contains("pc_order"),
        "{}",
        plan.executed_sql
    );
    assert_eq!(plan.original_sha256, digest(sql));
    assert_eq!(plan.executed_sha256, digest(&plan.executed_sql));
    assert_eq!(plan.parser_version, PINNED_PARSER_VERSION);
    assert!(plan.bindings.relations[0].alias.is_none());
    assert_eq!(plan.bindings.relations[0].physical_schema, "_oo");
}

#[test]
fn star_expansion_and_count_bindings_match_the_guard() {
    let plan = expect_plan(&small(), "SELECT * FROM logistics.shipment");
    assert_eq!(plan.star_expansions.len(), 1);
    assert_eq!(plan.star_expansions[0].schema, "logistics");
    assert_eq!(plan.star_expansions[0].name, "shipment");
    assert_eq!(
        plan.star_expansions[0].columns,
        vec!["order_id".to_string(), "region".to_string()]
    );
    let counted = expect_plan(
        &small(),
        "SELECT count(*) FROM logistics.shipment WHERE order_id = 1",
    );
    assert_eq!(counted.bindings.functions[0].oid, 201);
    assert!(counted.bindings.functions[0].aggregate_star);
    assert_eq!(counted.bindings.operators[0].oid, 301);
    assert_eq!(counted.bindings.operators[0].procedure_oid, 302);
    assert_eq!(counted.original_sha256, digest(&counted.original_sql));
    assert_eq!(counted.executed_sha256, digest(&counted.executed_sql));
}

#[test]
fn alias_and_literals_match_the_guard() {
    let plan = expect_plan(&small(), "SELECT s.order_id FROM logistics.shipment AS s");
    assert_eq!(plan.bindings.relations[0].alias.as_deref(), Some("s"));
    let literal = expect_plan(&small(), "SELECT 1");
    assert_eq!(
        literal.bindings.outputs,
        vec![OutputItem::Literal(LiteralKind::Integer)]
    );
    let boolean = expect_plan(&small(), "SELECT 'true'::bool");
    assert_eq!(
        boolean.bindings.outputs,
        vec![OutputItem::Literal(LiteralKind::Boolean)]
    );
}

#[test]
fn static_refusals_match_the_guard_and_hide_the_sql() {
    let distinct = expect_refusal(&small(), "SELECT DISTINCT order_id FROM logistics.shipment");
    assert_eq!(
        distinct,
        GuardError::Unsupported {
            construct: "DISTINCT"
        }
    );
    let write = expect_refusal(
        &small(),
        "INSERT INTO logistics.shipment (order_id) VALUES (1)",
    );
    assert_eq!(write, GuardError::Write);
    let like = expect_refusal(
        &small(),
        "SELECT region FROM logistics.shipment WHERE region LIKE 'w%'",
    );
    assert_eq!(
        like,
        GuardError::Unsupported {
            construct: "LIKE predicate"
        }
    );
    let into = expect_refusal(
        &small(),
        "SELECT order_id INTO result FROM logistics.shipment",
    );
    assert_eq!(into, GuardError::Into);
    let locking = expect_refusal(
        &small(),
        "SELECT order_id FROM logistics.shipment FOR UPDATE",
    );
    assert_eq!(locking, GuardError::Locking);
    let hidden = expect_refusal(
        &small(),
        &format!("SELECT {CANARY} FROM logistics.shipment"),
    );
    assert!(matches!(hidden, GuardError::ColumnNotAllowed { .. }));
    assert!(!hidden.to_string().contains(CANARY), "{}", hidden);
    assert!(!format!("{hidden:?}").contains(CANARY));
    let parsed = expect_refusal(&small(), &format!("SELEKT {CANARY}"));
    assert_eq!(parsed, GuardError::Parse);
    assert_eq!(parsed.to_string(), "SQL parse failed");
    assert!(!format!("{parsed:?}").contains(CANARY));
    let bytes = encode_refusal(&parsed).unwrap();
    assert!(
        !bytes
            .windows(CANARY.len())
            .any(|item| item == CANARY.as_bytes())
    );
    let cost = expect_refusal(&large(), "SELECT * FROM logistics.shipment");
    assert_eq!(
        cost,
        GuardError::CostTooHigh {
            relation: "logistics.shipment".to_string()
        }
    );
    assert!(!cost.to_string().contains("424"));
}

#[test]
fn identity_mismatch_is_the_guard_error() {
    let mut req = request(&small(), "SELECT order_id FROM logistics.shipment");
    req.identity.policy_id = "other-policy".to_string();
    let err = match supervise_ok(&req, |_| {}) {
        ParserResponse::Refusal(err) => err,
        ParserResponse::Plan(plan) => panic!("{}", plan.executed_sql),
    };
    assert_eq!(err, GuardError::IdentityMismatch);
    assert_eq!(err.to_string(), "policy identity does not match");
    assert!(!err.to_string().contains("other-policy"));
}

#[test]
fn two_legal_calls_are_two_children() {
    let req = request(&small(), "SELECT order_id FROM logistics.shipment");
    let expected = guard_select(&req.identity, &req.catalog, &req.sql).unwrap();
    let mut seen = Vec::new();
    for _ in 0..2 {
        let pid = Cell::new(0u32);
        let start = Cell::new(0u64);
        match supervise_ok(&req, |child| {
            pid.set(child);
            start.set(start_ticks(child));
        }) {
            ParserResponse::Plan(plan) => assert_eq!(*plan, expected),
            ParserResponse::Refusal(err) => panic!("{err:?}"),
        }
        seen.push((pid.get(), start.get()));
    }
    if seen[0].0 == seen[1].0 {
        assert_ne!(seen[0].1, seen[1].1, "same pid and starttime on two calls");
    }
}

#[test]
fn production_worker_clears_the_environment() {
    assert!(std::env::var_os("HOME").is_some() || std::env::var_os("CARGO_HOME").is_some());
    let req = request(&small(), "SELECT order_id, region FROM logistics.shipment");
    let expected = guard_select(&req.identity, &req.catalog, &req.sql).unwrap();
    let start = Cell::new(0u64);
    let pid = Cell::new(0u32);
    match supervise_ok(&req, |child| {
        pid.set(child);
        start.set(live_child(child, prod()));
    }) {
        ParserResponse::Plan(plan) => assert_eq!(*plan, expected),
        ParserResponse::Refusal(err) => panic!("{err:?}"),
    }
    assert_ne!(pid.get(), 0);
    assert_ne!(start.get(), 0);
}

#[test]
fn direct_binary_matches_the_guard_and_rejects_a_non_request() {
    let sql = "SELECT/*x*/ order_id, region FROM logistics.shipment";
    let req = request(&small(), sql);
    let bytes = encode_frame(KIND_REQUEST, &encode_request(&req).unwrap()).unwrap();
    let (code, out, err) = collect_worker(&bytes, |cmd| {
        cmd.env_clear();
    });
    assert_eq!(code, 0, "stderr {}", String::from_utf8_lossy(&err));
    let frame = decode_frame(&out).unwrap();
    assert_eq!(frame.version, 1);
    assert_eq!(frame.kind, KIND_PLAN);
    let plan = decode_plan(&frame.body).unwrap();
    let expected = guard_select(&req.identity, &req.catalog, sql).unwrap();
    assert_eq!(plan, expected);

    let wrong = encode_frame(KIND_PLAN, b"x").unwrap();
    let (code, out, _) = collect_worker(&wrong, |cmd| {
        cmd.env_clear();
    });
    assert_eq!(code, 1);
    assert!(out.is_empty(), "{out:?}");
    let (code, out, _) = collect_worker(&[], |cmd| {
        cmd.env_clear();
    });
    assert_eq!(code, 1);
    assert!(out.is_empty(), "{out:?}");

    let mut trailed = bytes.clone();
    trailed.push(0x7f);
    assert!(matches!(decode_frame(&trailed), Err(CodecError::Trailing)));
    let (code, out, _) = collect_worker(&trailed, |cmd| {
        cmd.env_clear();
    });
    assert_eq!(code, 1, "request trailer was accepted");
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn production_binary_rejects_fault_switches() {
    assert_exit2(|cmd| {
        cmd.arg("--slow");
    });
    assert_exit2(|cmd| {
        cmd.arg("slow");
    });
    assert_exit2(|cmd| {
        cmd.env("OO_PARSER_WORKER_SLOW", "1");
    });
    assert_exit2(|cmd| {
        cmd.env("OO_PARSER_WORKER_SLOW", "");
    });
}

#[test]
fn over_budget_sql_does_not_spawn() {
    let sql = " ".repeat(SQL_BYTE_BUDGET + 1);
    let started_child = Cell::new(false);
    let started = Instant::now();
    let err = supervise_with(
        &fault("slow"),
        &request(&small(), &sql),
        &CancelFlag::new(),
        |_| started_child.set(true),
    )
    .unwrap_err();
    assert!(matches!(err, SuperviseError::RequestBudget));
    assert!(!started_child.get());
    assert!(started.elapsed() < PARSE_DEADLINE);
    assert_eq!(err.to_string(), "parser request exceeds its budget");
    assert!(!err.to_string().contains(CANARY));
}

#[test]
fn cancel_before_spawn_has_no_pid() {
    let flag = CancelFlag::new();
    flag.cancel();
    let started = Cell::new(false);
    let err = supervise_with(
        &fault("slow"),
        &request(&small(), "SELECT 1"),
        &flag,
        |_| {
            started.set(true);
        },
    )
    .unwrap_err();
    assert!(matches!(err, SuperviseError::Cancelled(None)));
    assert!(!started.get());
    assert_eq!(err.to_string(), "parser worker was cancelled");
}

#[test]
fn cancel_after_spawn_kills_and_reaps() {
    let flag = CancelFlag::new();
    let worker = fault("slow");
    let pid = Cell::new(0u32);
    let start = Cell::new(0u64);
    let err = supervise_with(&worker, &request(&small(), "SELECT 1"), &flag, |child| {
        pid.set(child);
        start.set(live_child(child, &worker));
        flag.cancel();
    })
    .unwrap_err();
    let SuperviseError::Cancelled(Some(report)) = err else {
        panic!("cancel did not reap");
    };
    assert_eq!(report.term_signal, Some(9), "{report:?}");
    assert!(report.elapsed < PARSE_DEADLINE, "{report:?}");
    assert_gone(&report, pid.get(), start.get());
    assert_eq!(
        SuperviseError::Cancelled(Some(report)).to_string(),
        "parser worker was cancelled"
    );
}

#[test]
fn slow_worker_is_killed_inside_the_deadline_and_the_pid_is_gone() {
    let worker = fault("slow");
    let pid = Cell::new(0u32);
    let start = Cell::new(0u64);
    let err = supervise_with(
        &worker,
        &request(&small(), "SELECT 1"),
        &CancelFlag::new(),
        |child| {
            pid.set(child);
            start.set(live_child(child, &worker));
        },
    )
    .unwrap_err();
    let SuperviseError::TimedOut(report) = err else {
        panic!("slow worker was not timed out");
    };
    assert_killed(&report);
    assert_gone(&report, pid.get(), start.get());
    assert_eq!(
        SuperviseError::TimedOut(report).to_string(),
        "parser worker exceeded the deadline"
    );
}

#[test]
fn crash_worker_is_reaped_with_its_exit_code() {
    let worker = fault("crash");
    let pid = Cell::new(0u32);
    let start = Cell::new(0u64);
    let err = supervise_with(
        &worker,
        &request(&small(), "SELECT 1"),
        &CancelFlag::new(),
        |child| {
            pid.set(child);
            start.set(start_ticks(child));
        },
    )
    .unwrap_err();
    let SuperviseError::Crashed(report) = err else {
        panic!("crash was not classified");
    };
    assert_eq!(report.exit_code, Some(99), "{report:?}");
    assert_eq!(report.term_signal, None, "{report:?}");
    assert_gone(&report, pid.get(), start.get());
    assert_eq!(
        SuperviseError::Crashed(report).to_string(),
        "parser worker crashed"
    );
}

#[test]
fn oversize_header_kills_without_waiting_out_the_sleep() {
    let worker = fault("oversize");
    let pid = Cell::new(0u32);
    let start = Cell::new(0u64);
    let err = supervise_with(
        &worker,
        &request(&small(), "SELECT 1"),
        &CancelFlag::new(),
        |child| {
            pid.set(child);
            start.set(live_child(child, &worker));
        },
    )
    .unwrap_err();
    let SuperviseError::Oversize(report) = err else {
        panic!("oversize was not classified");
    };
    assert_eq!(report.term_signal, Some(9), "{report:?}");
    assert!(report.elapsed < Duration::from_secs(2), "{report:?}");
    assert_gone(&report, pid.get(), start.get());
    assert_eq!(
        SuperviseError::Oversize(report).to_string(),
        "parser worker frame exceeded its budget"
    );
}

#[test]
fn bad_version_magic_kind_and_trailer_are_malformed() {
    for mode in ["version", "magic", "kind", "trailer"] {
        let worker = fault(mode);
        let pid = Cell::new(0u32);
        let start = Cell::new(0u64);
        let err = supervise_with(
            &worker,
            &request(&small(), "SELECT 1"),
            &CancelFlag::new(),
            |child| {
                pid.set(child);
                start.set(start_ticks(child));
            },
        )
        .unwrap_err();
        let SuperviseError::Malformed(report) = err else {
            panic!("{mode} was not malformed");
        };
        assert_gone(&report, pid.get(), start.get());
        assert_eq!(
            SuperviseError::Malformed(report).to_string(),
            "parser worker frame was malformed"
        );
    }
}

#[test]
fn echo_is_not_accepted_and_the_sql_stays_out_of_the_error() {
    let worker = fault("echo");
    let pid = Cell::new(0u32);
    let start = Cell::new(0u64);
    let err = supervise_with(
        &worker,
        &request(&small(), CANARY),
        &CancelFlag::new(),
        |child| {
            pid.set(child);
            start.set(live_child(child, &worker));
        },
    )
    .unwrap_err();
    let SuperviseError::Malformed(report) = err else {
        panic!("echo was accepted");
    };
    assert_gone(&report, pid.get(), start.get());
    let shown = SuperviseError::Malformed(report);
    let text = format!("{shown} {shown:?} {report:?}");
    assert!(!text.contains(CANARY), "{text}");
}

#[test]
fn a_timeout_does_not_kill_the_next_child() {
    let slow = fault("slow");
    let req = request(&small(), "SELECT 1");
    let first_pid = Cell::new(0u32);
    let first_start = Cell::new(0u64);
    let err = supervise_with(&slow, &req, &CancelFlag::new(), |child| {
        first_pid.set(child);
        first_start.set(live_child(child, &slow));
    })
    .unwrap_err();
    let SuperviseError::TimedOut(report) = err else {
        panic!("first child was not timed out");
    };
    assert_killed(&report);
    assert_gone(&report, first_pid.get(), first_start.get());

    let delayed = fault("delayed");
    let second_pid = Cell::new(0u32);
    let started = Instant::now();
    let response = supervise_with(&delayed, &req, &CancelFlag::new(), |child| {
        second_pid.set(child);
        let _ = live_child(child, &delayed);
    })
    .unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the next child did not finish on its own"
    );
    assert_eq!(
        response,
        ParserResponse::Refusal(GuardError::IdentityMismatch)
    );
    assert_ne!(second_pid.get(), 0);
}

fn expect_timeout(result: Result<ParserResponse, SuperviseError>, label: &str) -> ProcessReport {
    let Err(SuperviseError::TimedOut(report)) = result else {
        panic!("{label}");
    };
    report
}

fn wait_for_grandchild(pid: u32, slot: &Cell<u32>) {
    let cwd = fs::read_link(format!("/proc/{pid}/cwd")).unwrap_or_default();
    let look = Instant::now();
    while look.elapsed() < Duration::from_millis(200) {
        if let Ok(text) = fs::read_to_string(cwd.join("grandchild.pid"))
            && let Ok(parsed) = text.trim().parse::<u32>()
        {
            slot.set(parsed);
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn assert_group_gone(leader: u32) {
    let entries = fs::read_dir("/proc").unwrap();
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_str().unwrap_or("").parse::<u32>() else {
            continue;
        };
        if proc_field(pid, 2) == Some(u64::from(leader)) {
            let comm = fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
            panic!("pid {pid} comm {} still in group {leader}", comm.trim());
        }
    }
}

fn signal_one(pid: u32) {
    if pid <= 1 {
        return;
    }
    let _ = Command::new("/bin/kill")
        .arg("-KILL")
        .arg("--")
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[test]
fn deadline_covers_blocked_stdin_stdout_partial_and_late() {
    let mut bulky = request(&small(), "SELECT 1");
    bulky.catalog.relations.clear();
    for rel in 0..80 {
        let mut columns = Vec::new();
        for col in 0..80 {
            columns.push(column(
                &format!("n{rel:02}{col:02}{}", "x".repeat(50)),
                &format!("p{rel:02}{col:02}"),
                INT_OID,
            ));
        }
        bulky.catalog.relations.push(VisibleRelation {
            schema: "logistics".to_string(),
            name: format!("r{rel:02}"),
            physical_schema: "_oo".to_string(),
            physical_name: format!("t{rel:02}"),
            columns,
            pending_unclassified: 0,
            estimated_rows: 1,
        });
    }
    let encoded = encode_request(&bulky).expect("bulky encodes");
    assert!(
        encoded.len() > 300_000,
        "request {} is small enough to sit in a socket buffer",
        encoded.len()
    );

    let wall = Instant::now();
    let pid = Cell::new(0u32);
    let start = Cell::new(0u64);
    let err = supervise_with(&fault("slow"), &bulky, &CancelFlag::new(), |child| {
        pid.set(child);
        start.set(live_child(child, &fault("slow")));
    });
    let report = expect_timeout(err, "blocked stdin was not a timeout");
    assert!(
        wall.elapsed() < Duration::from_secs(2),
        "blocked stdin wall {:?}",
        wall.elapsed()
    );
    assert!(report.elapsed < Duration::from_secs(1), "{report:?}");
    assert_killed(&report);
    assert_gone(&report, pid.get(), start.get());

    for mode in ["hold-stdout", "partial"] {
        let wall = Instant::now();
        let pid = Cell::new(0u32);
        let start = Cell::new(0u64);
        let err = supervise_with(
            &fault(mode),
            &request(&small(), "SELECT 1"),
            &CancelFlag::new(),
            |child| {
                pid.set(child);
                start.set(live_child(child, &fault(mode)));
            },
        );
        let report = expect_timeout(err, mode);
        assert!(
            wall.elapsed() < Duration::from_secs(1),
            "{mode} wall {:?}",
            wall.elapsed()
        );
        assert_killed(&report);
        assert_gone(&report, pid.get(), start.get());
    }

    let wall = Instant::now();
    let pid = Cell::new(0u32);
    let start = Cell::new(0u64);
    let err = supervise_with(
        &fault("late"),
        &request(&small(), "SELECT 1"),
        &CancelFlag::new(),
        |child| {
            pid.set(child);
            start.set(live_child(child, &fault("late")));
        },
    );
    let report = expect_timeout(err, "late frame was accepted");
    assert!(
        wall.elapsed() < Duration::from_secs(1),
        "late wall {:?}",
        wall.elapsed()
    );
    assert_killed(&report);
    assert!(
        report.elapsed < Duration::from_millis(690),
        "late frame was not killed before its 700 ms write: {report:?}"
    );
    assert_gone(&report, pid.get(), start.get());
}

#[test]
fn same_group_grandchild_is_reaped_inside_one_second() {
    let mut neighbor = Command::new("/bin/sleep")
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("neighbor");
    let neighbor_pid = neighbor.id();
    let neighbor_start = start_ticks(neighbor_pid);
    let req = request(&small(), "SELECT 1");

    for mode in ["orphan", "orphan-stdout"] {
        let wall = Instant::now();
        let child_pid = Cell::new(0u32);
        let child_start = Cell::new(0u64);
        let grand = Cell::new(0u32);
        let err = supervise_with(&fault(mode), &req, &CancelFlag::new(), |pid| {
            child_pid.set(pid);
            child_start.set(live_child(pid, &fault(mode)));
            wait_for_grandchild(pid, &grand);
        });
        let report = expect_timeout(err, mode);
        assert!(
            wall.elapsed() < Duration::from_secs(1),
            "{mode} wall {:?}",
            wall.elapsed()
        );
        assert_killed(&report);
        assert_gone(&report, child_pid.get(), child_start.get());
        let grandchild = grand.get();
        assert_ne!(grandchild, 0, "{mode} grandchild pid was not recorded");
        if let Some(now) = proc_field(grandchild, 19) {
            panic!("{mode} grandchild {grandchild} still alive start {now}");
        }
        assert_group_gone(child_pid.get());
        assert_eq!(
            proc_field(neighbor_pid, 19),
            Some(neighbor_start),
            "neighbor was killed"
        );
    }

    let _ = neighbor.kill();
    let _ = neighbor.wait();
}

#[test]
fn escaped_session_is_not_reaped_and_the_parent_still_returns() {
    let setsid_present =
        Path::new("/usr/bin/setsid").is_file() || Path::new("/bin/setsid").is_file();
    let mut neighbor = Command::new("/bin/sleep")
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("neighbor");
    let neighbor_pid = neighbor.id();
    let neighbor_start = start_ticks(neighbor_pid);
    let wall = Instant::now();
    let child_pid = Cell::new(0u32);
    let child_start = Cell::new(0u64);
    let recorded = Cell::new(0u32);
    let err = supervise_with(
        &fault("setsid-stdout"),
        &request(&small(), "SELECT 1"),
        &CancelFlag::new(),
        |pid| {
            child_pid.set(pid);
            child_start.set(live_child(pid, &fault("setsid-stdout")));
            wait_for_grandchild(pid, &recorded);
        },
    );
    let report = expect_timeout(err, "setsid stdout blocked the parent");
    assert!(
        wall.elapsed() < Duration::from_secs(1),
        "setsid wall {:?}",
        wall.elapsed()
    );
    assert_killed(&report);
    assert_gone(&report, child_pid.get(), child_start.get());
    assert_eq!(proc_field(neighbor_pid, 19), Some(neighbor_start));
    if setsid_present {
        let escaped = recorded.get();
        assert_ne!(escaped, 0, "setsid pid was not recorded");
        let comm = fs::read_to_string(format!("/proc/{escaped}/comm")).unwrap_or_default();
        assert!(
            comm.starts_with("sleep") || comm.starts_with("setsid"),
            "escaped comm {comm}"
        );
        assert_ne!(proc_field(escaped, 2), Some(u64::from(child_pid.get())));
        signal_one(escaped);
        thread::sleep(Duration::from_millis(50));
        assert_eq!(proc_field(neighbor_pid, 19), Some(neighbor_start));
    }
    let _ = neighbor.kill();
    let _ = neighbor.wait();
}
