//! Independent review of the one-shot parser worker.
//!
//! The oracle for legal SQL is `guard_select` in this process. Fault programs
//! are separate `rustc` binaries. This file is not part of the product tree.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use oo_gateway::parser_worker::{
    decode_frame, decode_refusal, decode_request, encode_frame, encode_refusal, encode_request,
    supervise_with, AdmitRequest, CancelFlag, CodecError, ParserResponse, ProcessReport,
    RestrictionCapability, SuperviseError, TrustedWorker, CATALOG_REASONS, CONSTRUCTS,
    KIND_REFUSAL, KIND_REQUEST, MAX_FRAME_BODY, PARSE_DEADLINE, RESTRICTION_CAPABILITY,
    SERIALIZE_REASONS,
};
use oo_gateway::query_executor::{UntrustedAdmission, UNTRUSTED_ADMISSION};
use oo_gateway::{
    guard_select, GuardError, LiteralTypes, OutputItem, PolicyCatalog, PolicyIdentity,
    ResolvedFunction, ResolvedOperator, VisibleColumn, VisibleRelation, PINNED_PARSER_VERSION,
};
use sha2::{Digest, Sha256};

const CANARY: &str = "canary_secret_sql_a195";

const FAULT_SRC: &str = r#"
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

fn main() {
    match option_env!("OO_FAULT_MODE").unwrap_or("") {
        "slow" => thread::sleep(Duration::from_secs(30)),
        "crash" => std::process::exit(99),
        "oversize" => {
            write_header(1, 2, u32::MAX);
            thread::sleep(Duration::from_secs(30));
        }
        "magic" => {
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(b"NOPE");
            let _ = out.flush();
        }
        "trailer" => {
            write_header(1, 3, 1);
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(&[1, 0xff]);
            let _ = out.flush();
        }
        "kind" => write_header(1, 99, 0),
        "partial" => {
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(b"OOSW");
            let _ = out.flush();
            thread::sleep(Duration::from_secs(30));
        }
        "hold-stdout" => {
            write_header(1, 3, 1);
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(&[1]);
            let _ = out.flush();
            thread::sleep(Duration::from_secs(30));
        }
        "late" => {
            thread::sleep(Duration::from_millis(700));
            write_header(1, 3, 1);
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(&[1]);
            let _ = out.flush();
        }
        "orphan" => orphan(false),
        "orphan-stdout" => orphan(true),
        "probe" => probe(),
        _ => std::process::exit(2),
    }
}

fn write_header(version: u16, kind: u16, len: u32) {
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(b"OOSW");
    let _ = out.write_all(&version.to_le_bytes());
    let _ = out.write_all(&kind.to_le_bytes());
    let _ = out.write_all(&len.to_le_bytes());
    let _ = out.flush();
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

fn probe() {
    let passwd = std::fs::File::open("/etc/passwd").is_ok();
    let listen = match std::net::TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => format!("ok:{}", listener.local_addr().map(|addr| addr.port()).unwrap_or(0)),
        Err(err) => format!("err:{err}"),
    };
    let _ = std::fs::write("probe.txt", format!("passwd={passwd} listen={listen}\n"));
    thread::sleep(Duration::from_secs(30));
}
"#;

const FAULT_MODES: &[&str] = &[
    "slow",
    "crash",
    "oversize",
    "magic",
    "trailer",
    "kind",
    "partial",
    "hold-stdout",
    "late",
    "orphan",
    "orphan-stdout",
    "probe",
];

#[derive(Clone, Copy, Default)]
struct Seen {
    pid: u32,
    start: u64,
}

fn note(line: &str) {
    static GATE: Mutex<()> = Mutex::new(());
    let _guard = GATE.lock().expect("oracle lock");
    let path = std::env::var("A195_ORACLE").unwrap_or_else(|_| "/tmp/a195-oracle.txt".to_string());
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .unwrap_or_else(|err| panic!("oracle {path}: {err}"));
    writeln!(file, "{line}").expect("oracle write");
    eprintln!("{line}");
}

fn sha256_hex(text: &str) -> String {
    let bytes = Sha256::digest(text.as_bytes());
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        hex.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
        hex.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
    }
    hex
}

fn fresh() -> PolicyCatalog {
    PolicyCatalog {
        identity: PolicyIdentity {
            project: "depot".to_string(),
            role: "oo_r_clerk".to_string(),
            policy_id: "policy-a195-fresh".to_string(),
        },
        relations: vec![VisibleRelation {
            schema: "depot".to_string(),
            name: "parcel".to_string(),
            physical_schema: "_oo".to_string(),
            physical_name: "pt_parcel".to_string(),
            columns: vec![
                VisibleColumn {
                    name: "parcel_id".to_string(),
                    physical_name: "pc_id".to_string(),
                    type_oid: 23,
                },
                VisibleColumn {
                    name: "zone".to_string(),
                    physical_name: "pc_zone".to_string(),
                    type_oid: 25,
                },
            ],
            pending_unclassified: 0,
            estimated_rows: 12,
        }],
        functions: vec![ResolvedFunction {
            oid: 8801,
            schema: "pg_catalog".to_string(),
            name: "count".to_string(),
            argument_type_oids: Vec::new(),
            aggregate_star: true,
        }],
        operators: vec![
            ResolvedOperator {
                oid: 8802,
                procedure_oid: 8803,
                schema: "pg_catalog".to_string(),
                name: "=".to_string(),
                left_type_oid: 23,
                right_type_oid: 23,
            },
            ResolvedOperator {
                oid: 8804,
                procedure_oid: 8805,
                schema: "pg_catalog".to_string(),
                name: "=".to_string(),
                left_type_oid: 25,
                right_type_oid: 25,
            },
        ],
        literal_types: LiteralTypes {
            integer: Some(23),
            text: Some(25),
            boolean: Some(16),
        },
        null_test_allowed: true,
        large_table_row_threshold: 1_000,
    }
}

fn request(catalog: &PolicyCatalog, sql: &str) -> AdmitRequest {
    AdmitRequest {
        identity: catalog.identity.clone(),
        catalog: catalog.clone(),
        sql: sql.to_string(),
    }
}

fn prod() -> &'static TrustedWorker {
    static WORKER: OnceLock<TrustedWorker> = OnceLock::new();
    WORKER.get_or_init(|| {
        TrustedWorker::approve(Path::new(env!("CARGO_BIN_EXE_oo-sql-parser-worker")))
            .expect("approve production worker")
    })
}

fn fault_paths() -> &'static Vec<(String, PathBuf)> {
    static ALL: OnceLock<Vec<(String, PathBuf)>> = OnceLock::new();
    ALL.get_or_init(|| {
        let dir = std::env::current_exe()
            .expect("current_exe")
            .parent()
            .expect("exe dir")
            .join("a195-fault");
        fs::create_dir_all(&dir).expect("fault dir");
        let src = dir.join("fault.rs");
        fs::write(&src, FAULT_SRC).expect("fault src");
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
                "{mode} rustc {}\n{}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
            built.push(((*mode).to_string(), bin));
        }
        built
    })
}

fn fault(mode: &str) -> TrustedWorker {
    let path = fault_paths()
        .iter()
        .find(|(name, _)| name == mode)
        .unwrap_or_else(|| panic!("missing fault {mode}"))
        .1
        .clone();
    TrustedWorker::approve(&path).unwrap_or_else(|err| panic!("approve {mode}: {err}"))
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

fn gone(report: &ProcessReport, pid: u32, start: u64) {
    assert_eq!(report.pid, pid, "{report:?}");
    assert_eq!(report.start_ticks, Some(start), "{report:?}");
    assert!(report.reaped, "{report:?}");
    assert!(!report.same_pid_alive, "{report:?}");
    assert!(report.pid_absent || report.pid_reused, "{report:?}");
    if let Some(now) = proc_field(pid, 19) {
        assert_ne!(now, start, "pid {pid} still has starttime {start}");
    }
}

fn run_mode(mode: &str, req: &AdmitRequest, cancel: &CancelFlag) -> (Result<ParserResponse, SuperviseError>, Duration, Seen) {
    let worker = fault(mode);
    let slot = Arc::new(Mutex::new(Seen::default()));
    let observed = Arc::clone(&slot);
    let wall_at = Instant::now();
    let result = supervise_with(&worker, req, cancel, move |pid| {
        let start = start_ticks(pid);
        let mut seen = observed.lock().expect("seen");
        seen.pid = pid;
        seen.start = start;
    });
    let wall = wall_at.elapsed();
    let seen = *slot.lock().expect("seen");
    (result, wall, seen)
}

fn line_report(label: &str, wall: Duration, report: &ProcessReport) {
    let wall_ms = wall.as_millis();
    let elapsed_ms = report.elapsed.as_millis();
    let pid = report.pid;
    let start = report.start_ticks.unwrap_or(0);
    let reaped = u8::from(report.reaped);
    let same = u8::from(report.same_pid_alive);
    let reused = u8::from(report.pid_reused);
    let absent = u8::from(report.pid_absent);
    let exit = report.exit_code;
    let signal = report.term_signal;
    note(&format!(
        "case={label} wall_ms={wall_ms} elapsed_ms={elapsed_ms} pid={pid} start={start} reaped={reaped} same={same} reused={reused} absent={absent} exit={exit:?} signal={signal:?} "
    ));
}

fn expect_timeout(result: Result<ParserResponse, SuperviseError>, label: &str) -> ProcessReport {
    match result {
        Err(SuperviseError::TimedOut(report)) => report,
        other => panic!("{label}: {other:?}"),
    }
}

fn expect_cancel(result: Result<ParserResponse, SuperviseError>, label: &str) -> ProcessReport {
    match result {
        Err(SuperviseError::Cancelled(Some(report))) => report,
        other => panic!("{label}: {other:?}"),
    }
}

fn require_bounded(label: &str, wall: Duration, report: &ProcessReport) {
    assert!(
        wall < Duration::from_secs(3),
        "{label} wall {wall:?} exceeded 3s; a blocked pipe was not interrupted"
    );
    assert!(
        report.elapsed < Duration::from_secs(3),
        "{label} elapsed {:?} exceeded 3s",
        report.elapsed
    );
    assert!(
        wall + Duration::from_millis(200) >= report.elapsed,
        "{label} wall {wall:?} does not cover elapsed {:?}",
        report.elapsed
    );
}

#[test]
fn a_constants_are_unsupported_and_blocked() {
    assert_eq!(RESTRICTION_CAPABILITY, RestrictionCapability::Unsupported);
    assert_eq!(UNTRUSTED_ADMISSION, UntrustedAdmission::Blocked);
    assert_eq!(PARSE_DEADLINE, Duration::from_millis(500));
    let worker = prod();
    assert!(worker.path().is_absolute());
    assert!(worker.path().is_file());
    let src = fs::read_to_string("/src/crates/oo-gateway/src/parser_worker.rs").expect("source");
    assert!(!src.contains("unsafe {"), "production worker uses unsafe");
    assert!(!src.contains("unsafe fn"), "production worker uses unsafe");
    assert!(src.contains("RestrictionCapability::Unsupported"));
    assert!(src.contains(".process_group(0)"));
    assert!(src.contains("child.kill()"));
    assert!(!src.contains("PR_SET_CHILD_SUBREAPER"));
    let admission = fs::read_to_string("/src/crates/oo-gateway/src/query_executor.rs").expect("executor");
    assert!(admission.contains("pub const UNTRUSTED_ADMISSION: UntrustedAdmission = UntrustedAdmission::Blocked;"));
    assert!(admission.contains("No 500 ms process-isolated parser worker is implemented."));
    note("restriction=Unsupported admission=Blocked stale_executor_comment=present unsafe_block=absent group_kill=absent");
}

#[test]
fn b_codec_bounds_and_closed_refusals() {
    let started = Instant::now();
    let mut huge = Vec::new();
    huge.extend_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(decode_request(&huge).unwrap_err(), CodecError::Limit);
    assert!(started.elapsed() < Duration::from_millis(50), "u32::MAX length copied");

    let mut count = Vec::new();
    for text in ["depot", "oo_r_clerk", "policy-a195-fresh", "depot", "oo_r_clerk", "policy-a195-fresh"] {
        let len = u32::try_from(text.len()).expect("len");
        count.extend_from_slice(&len.to_le_bytes());
        count.extend_from_slice(text.as_bytes());
    }
    count.extend_from_slice(&u32::MAX.to_le_bytes());
    let counted = Instant::now();
    assert_eq!(decode_request(&count).unwrap_err(), CodecError::Limit);
    assert!(counted.elapsed() < Duration::from_millis(50), "u32::MAX count reserved");

    let mut over_relations = count;
    let rewrite_at = over_relations.len() - 4;
    over_relations[rewrite_at..].copy_from_slice(&129_u32.to_le_bytes());
    assert_eq!(decode_request(&over_relations).unwrap_err(), CodecError::Limit);

    let catalog = fresh();
    let req = request(&catalog, "SELECT 1");
    let encoded = encode_request(&req).expect("encode");
    assert_eq!(decode_request(&encoded).expect("decode"), req);

    let mut absent = catalog.clone();
    absent.literal_types.integer = None;
    absent.literal_types.text = None;
    absent.literal_types.boolean = None;
    absent.null_test_allowed = false;
    let absent_req = request(&absent, "SELECT 1");
    assert_eq!(
        decode_request(&encode_request(&absent_req).expect("encode absent")).expect("decode absent"),
        absent_req
    );

    let mut present = encode_request(&req).expect("present");
    let missing = encode_request(&request(&absent, "SELECT 1")).expect("missing");
    let tag_at = present
        .iter()
        .zip(missing.iter())
        .position(|(left, right)| left != right)
        .expect("optional tag differs");
    present[tag_at] = 2;
    assert_eq!(decode_request(&present).unwrap_err(), CodecError::Tag);

    let mut long_name = req.clone();
    long_name.identity.project = "p".repeat(128);
    assert!(encode_request(&long_name).is_ok());
    long_name.identity.project = "p".repeat(129);
    assert_eq!(encode_request(&long_name).unwrap_err(), CodecError::Limit);
    long_name.identity.project = "depot".to_string();
    long_name.catalog.relations[0].name = "n".repeat(63);
    assert!(encode_request(&long_name).is_ok());
    long_name.catalog.relations[0].name = "n".repeat(64);
    assert_eq!(encode_request(&long_name).unwrap_err(), CodecError::Limit);

    for reason in CATALOG_REASONS {
        let err = GuardError::CatalogInvalid { reason };
        assert_eq!(decode_refusal(&encode_refusal(&err).expect("cat")).expect("cat"), err);
    }
    for reason in SERIALIZE_REASONS {
        let err = GuardError::Serialize { reason };
        assert_eq!(decode_refusal(&encode_refusal(&err).expect("ser")).expect("ser"), err);
    }
    for construct in CONSTRUCTS {
        let err = GuardError::Unsupported { construct };
        assert_eq!(decode_refusal(&encode_refusal(&err).expect("con")).expect("con"), err);
    }
    assert_eq!(
        encode_refusal(&GuardError::Unsupported { construct: "not-a-construct" }).unwrap_err(),
        CodecError::Label
    );
    assert_eq!(
        decode_refusal(&encode_label(2, "not-a-reason")).unwrap_err(),
        CodecError::Label
    );

    let closed = [
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
            available: vec!["depot.parcel.zone".to_string()],
            pending: 2,
            ambiguous: false,
        },
        GuardError::Unqualified { suggestion: None },
        GuardError::Unqualified {
            suggestion: Some("depot.parcel".to_string()),
        },
        GuardError::CostTooHigh {
            relation: "depot.parcel".to_string(),
        },
    ];
    for err in closed {
        let back = decode_refusal(&encode_refusal(&err).expect("refusal")).expect("roundtrip");
        assert_eq!(back, err);
        let shown = format!("{err}");
        assert!(!shown.contains(CANARY), "{shown}");
    }
    for tag in [0_u8, 17, 255] {
        assert_eq!(decode_refusal(&[tag]).unwrap_err(), CodecError::Tag, "tag {tag}");
    }
    assert_eq!(decode_refusal(&[15, 2]).unwrap_err(), CodecError::Tag);
    assert_eq!(decode_refusal(&[3, 0xff]).unwrap_err(), CodecError::Trailing);
    let mut labels = vec![14_u8];
    labels.extend_from_slice(&513_u32.to_le_bytes());
    assert_eq!(decode_refusal(&labels).unwrap_err(), CodecError::Limit);
    let mut wide = vec![16_u8];
    wide.extend_from_slice(&257_u32.to_le_bytes());
    assert_eq!(decode_refusal(&wide).unwrap_err(), CodecError::Limit);
    let flag = vec![14_u8, 0, 0, 0, 0, 0, 0, 0, 0, 2];
    assert_eq!(flag.len(), 10);
    assert_eq!(decode_refusal(&flag).unwrap_err(), CodecError::Tag);

    let mut frame = encode_frame(KIND_REFUSAL, &[3]).expect("frame");
    frame.push(0xff);
    assert_eq!(decode_frame(&frame).unwrap_err(), CodecError::Trailing);
    let mut header = Vec::new();
    header.extend_from_slice(b"OOSW");
    header.extend_from_slice(&1_u16.to_le_bytes());
    header.extend_from_slice(&2_u16.to_le_bytes());
    header.extend_from_slice(&u32::try_from(MAX_FRAME_BODY + 1).expect("len").to_le_bytes());
    assert_eq!(header.len(), 12);
    assert_eq!(decode_frame(&header).unwrap_err(), CodecError::Limit);
    note("codec=optional,length,count,closed-refusal,trailing,oversize-header");
}

fn encode_label(tag: u8, text: &str) -> Vec<u8> {
    let mut bytes = vec![tag];
    let len = u32::try_from(text.len()).expect("len");
    bytes.extend_from_slice(&len.to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

#[test]
fn c_fresh_catalog_matches_the_guard_hashes() {
    let catalog = fresh();
    let select = "SELECT/*a195*/ parcel_id, zone FROM depot.parcel WHERE parcel_id = 4";
    let star = "SELECT * FROM depot.parcel WHERE zone = 'east'";
    let counted = "SELECT count(*) FROM depot.parcel WHERE parcel_id = 4";
    for sql in [select, star, counted] {
        let expected = guard_select(&catalog.identity, &catalog, sql).expect("guard");
        let wall = Instant::now();
        let response = supervise_with(prod(), &request(&catalog, sql), &CancelFlag::new(), |_| {})
            .unwrap_or_else(|err| panic!("worker {sql}: {err:?}"));
        let wall = wall.elapsed();
        let ParserResponse::Plan(plan) = response else {
            panic!("worker refused {sql}");
        };
        assert_eq!(*plan, expected, "{sql}");
        assert_eq!(plan.original_sha256, sha256_hex(sql));
        assert_eq!(plan.executed_sha256, sha256_hex(&plan.executed_sql));
        assert_eq!(plan.parser_version, PINNED_PARSER_VERSION);
        let executed_sql = plan.executed_sql.as_str();
        assert!(executed_sql.contains("pt_parcel"), "{executed_sql}");
        assert!(executed_sql.contains("pc_id"), "{executed_sql}");
        assert!(wall < PARSE_DEADLINE, "legal call took {wall:?}");
        let sql_len = sql.len();
        let wall_ms = wall.as_millis();
        let original = &plan.original_sha256;
        let executed = &plan.executed_sha256;
        let stars = plan.star_expansions.len();
        let fn_oid = plan.bindings.functions.first().map(|item| item.oid).unwrap_or(0);
        let mut op_oids = String::new();
        for item in &plan.bindings.operators {
            if !op_oids.is_empty() {
                op_oids.push(',');
            }
            op_oids.push_str(&item.oid.to_string());
        }
        note(&format!(
            "case=legal sql_len={sql_len} wall_ms={wall_ms} original={original} executed={executed} stars={stars} fn_oid={fn_oid} op_oids={op_oids}"
        ));
    }
    let star_plan = guard_select(&catalog.identity, &catalog, star).expect("star");
    assert_eq!(star_plan.star_expansions.len(), 1);
    assert_eq!(star_plan.star_expansions[0].columns, vec!["parcel_id".to_string(), "zone".to_string()]);
    let star_sql = star_plan.executed_sql.as_str();
    assert!(star_sql.contains("pc_zone"), "{star_sql}");
    let count_plan = guard_select(&catalog.identity, &catalog, counted).expect("count");
    assert_eq!(count_plan.bindings.functions[0].oid, 8801);
    assert!(count_plan.bindings.functions[0].aggregate_star);
    assert_eq!(count_plan.bindings.operators[0].oid, 8802);
    assert_eq!(count_plan.bindings.operators[0].procedure_oid, 8803);
    assert!(matches!(count_plan.bindings.outputs[0], OutputItem::Function(_)));

    let mut mismatched = request(&catalog, CANARY);
    mismatched.identity.project = "other".to_string();
    let expected = guard_select(&mismatched.identity, &catalog, CANARY).expect_err("guard mismatch");
    assert_eq!(expected, GuardError::IdentityMismatch);
    let response = supervise_with(prod(), &mismatched, &CancelFlag::new(), |_| {})
        .unwrap_or_else(|err| panic!("mismatch spawn: {err:?}"));
    let ParserResponse::Refusal(err) = response else {
        panic!("mismatch was admitted");
    };
    assert_eq!(err, GuardError::IdentityMismatch);
    let shown = format!("{err} {err:?}");
    assert!(!shown.contains(CANARY), "{shown}");
    note("case=identity-mismatch canary_in_display=0");
}

#[test]
fn d_faults_reap_the_child_they_kill() {
    let req = request(&fresh(), "SELECT 1");
    let (result, wall, seen) = run_mode("slow", &req, &CancelFlag::new());
    let SuperviseError::TimedOut(report) = result.unwrap_err() else {
        panic!("slow was not a timeout");
    };
    assert_eq!(report.term_signal, Some(9), "{report:?}");
    assert!(report.elapsed >= PARSE_DEADLINE, "{report:?}");
    assert!(wall >= PARSE_DEADLINE, "slow wall {wall:?}");
    gone(&report, seen.pid, seen.start);
    require_bounded("slow", wall, &report);
    line_report("slow", wall, &report);

    let (result, wall, seen) = run_mode("crash", &req, &CancelFlag::new());
    let SuperviseError::Crashed(report) = result.unwrap_err() else {
        panic!("crash was not crashed");
    };
    assert_eq!(report.exit_code, Some(99), "{report:?}");
    assert_eq!(report.term_signal, None, "{report:?}");
    gone(&report, seen.pid, seen.start);
    require_bounded("crash", wall, &report);
    line_report("crash", wall, &report);

    let (result, wall, seen) = run_mode("oversize", &req, &CancelFlag::new());
    let SuperviseError::Oversize(report) = result.unwrap_err() else {
        panic!("oversize was not oversize");
    };
    assert_eq!(report.term_signal, Some(9), "{report:?}");
    gone(&report, seen.pid, seen.start);
    require_bounded("oversize", wall, &report);
    assert!(wall < Duration::from_secs(2), "oversize waited {wall:?}");
    line_report("oversize", wall, &report);

    for mode in ["magic", "kind", "trailer"] {
        let (result, wall, seen) = run_mode(mode, &req, &CancelFlag::new());
        let SuperviseError::Malformed(report) = result.unwrap_err() else {
            panic!("{mode} was not malformed");
        };
        gone(&report, seen.pid, seen.start);
        require_bounded(mode, wall, &report);
        line_report(mode, wall, &report);
    }

    let flag = CancelFlag::new();
    flag.cancel();
    let (result, wall, seen) = run_mode("slow", &req, &flag);
    assert!(matches!(result, Err(SuperviseError::Cancelled(None))), "{result:?}");
    assert_eq!(seen.pid, 0, "cancel before spawn started a child");
    assert!(wall < PARSE_DEADLINE, "cancel-before wall {wall:?}");
    note(&format!("case=cancel-before wall_ms={} pid=0", wall.as_millis()));
}

#[test]
fn d2_cancel_after_spawn_reaps() {
    let req = request(&fresh(), "SELECT 1");
    let worker = fault("slow");
    let slot = Arc::new(Mutex::new(Seen::default()));
    let observed = Arc::clone(&slot);
    let cancel = CancelFlag::new();
    let wall_at = Instant::now();
    let result = supervise_with(&worker, &req, &cancel, {
        let cancel = &cancel;
        move |pid| {
            let start = start_ticks(pid);
            let mut seen = observed.lock().expect("seen");
            seen.pid = pid;
            seen.start = start;
            cancel.cancel();
        }
    });
    let wall = wall_at.elapsed();
    let seen = *slot.lock().expect("seen");
    let SuperviseError::Cancelled(Some(report)) = result.unwrap_err() else {
        panic!("cancel-after did not reap");
    };
    assert_eq!(report.term_signal, Some(9), "{report:?}");
    assert!(report.elapsed < PARSE_DEADLINE, "{report:?}");
    assert!(wall < PARSE_DEADLINE, "cancel-after wall {wall:?}");
    gone(&report, seen.pid, seen.start);
    line_report("cancel-after", wall, &report);
}

#[test]
fn e_blocked_stdin_and_stdout_do_not_join_forever() {
    let mut bulky = request(&fresh(), "SELECT 1");
    bulky.catalog.relations.clear();
    for rel in 0..128 {
        let mut columns = Vec::new();
        for col in 0..256 {
            columns.push(VisibleColumn {
                name: format!("n{rel:03}{col:03}{}", "x".repeat(20)),
                physical_name: format!("p{rel:03}{col:03}"),
                type_oid: 23,
            });
        }
        bulky.catalog.relations.push(VisibleRelation {
            schema: "depot".to_string(),
            name: format!("r{rel:03}"),
            physical_schema: "_oo".to_string(),
            physical_name: format!("t{rel:03}"),
            columns,
            pending_unclassified: 0,
            estimated_rows: 1,
        });
    }
    let encoded = encode_request(&bulky).expect("bulky encodes");
    assert!(encoded.len() > 1_000_000, "request {} did not fill a pipe", encoded.len());
    assert!(encoded.len() <= MAX_FRAME_BODY, "request {}", encoded.len());
    note(&format!("case=bulky-bytes len={}", encoded.len()));
    let (result, wall, seen) = run_mode("slow", &bulky, &CancelFlag::new());
    let report = expect_timeout(result, "blocked stdin was not a timeout");
    assert_eq!(report.term_signal, Some(9), "{report:?}");
    assert!(report.elapsed >= PARSE_DEADLINE, "{report:?}");
    gone(&report, seen.pid, seen.start);
    require_bounded("blocked-stdin", wall, &report);
    line_report("blocked-stdin", wall, &report);

    let req = request(&fresh(), "SELECT 1");
    let (result, wall, seen) = run_mode("hold-stdout", &req, &CancelFlag::new());
    let report = expect_timeout(result, "held stdout was accepted or misclassified");
    assert_eq!(report.term_signal, Some(9), "{report:?}");
    assert!(report.elapsed >= PARSE_DEADLINE, "{report:?}");
    gone(&report, seen.pid, seen.start);
    require_bounded("blocked-stdout", wall, &report);
    line_report("blocked-stdout", wall, &report);

    let (result, wall, seen) = run_mode("partial", &req, &CancelFlag::new());
    let report = expect_timeout(result, "partial header was not a timeout");
    assert_eq!(report.term_signal, Some(9), "{report:?}");
    assert!(report.elapsed >= PARSE_DEADLINE, "{report:?}");
    gone(&report, seen.pid, seen.start);
    require_bounded("partial-stdout", wall, &report);
    line_report("partial-stdout", wall, &report);

    let (result, wall, seen) = run_mode("late", &req, &CancelFlag::new());
    let report = expect_timeout(result, "late frame was accepted");
    assert_eq!(report.term_signal, Some(9), "{report:?}");
    assert!(report.elapsed >= PARSE_DEADLINE, "{report:?}");
    assert!(
        report.elapsed < Duration::from_millis(690),
        "late frame was not killed before its 700ms write: {report:?}"
    );
    gone(&report, seen.pid, seen.start);
    require_bounded("late", wall, &report);
    line_report("late", wall, &report);
}

#[test]
fn f_timeout_does_not_kill_the_next_child_or_a_neighbor() {
    let mut neighbor = Command::new("/bin/sleep")
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("neighbor");
    let neighbor_pid = neighbor.id();
    let neighbor_start = start_ticks(neighbor_pid);
    let req = request(&fresh(), "SELECT/*a195*/ parcel_id FROM depot.parcel");
    let (result, wall, seen) = run_mode("slow", &req, &CancelFlag::new());
    let SuperviseError::TimedOut(report) = result.unwrap_err() else {
        panic!("first child was not timed out");
    };
    assert_eq!(report.term_signal, Some(9), "{report:?}");
    gone(&report, seen.pid, seen.start);
    require_bounded("first", wall, &report);
    assert_eq!(proc_field(neighbor_pid, 19), Some(neighbor_start), "neighbor was killed");

    let second_at = Instant::now();
    let mut second_pid = 0_u32;
    let response = supervise_with(prod(), &req, &CancelFlag::new(), |pid| second_pid = pid)
        .unwrap_or_else(|err| panic!("next child: {err:?}"));
    let second_wall = second_at.elapsed();
    let ParserResponse::Plan(plan) = response else {
        panic!("next child was refused");
    };
    assert_eq!(plan.original_sha256, sha256_hex(&req.sql));
    assert_ne!(second_pid, seen.pid);
    assert_ne!(second_pid, neighbor_pid);
    if let Some(now) = proc_field(seen.pid, 19) {
        assert_ne!(now, seen.start, "first child starttime returned");
    }
    assert_eq!(proc_field(neighbor_pid, 19), Some(neighbor_start));
    assert!(second_wall < PARSE_DEADLINE, "next child took {second_wall:?}");
    let _ = neighbor.kill();
    let _ = neighbor.wait();
    let first_pid = seen.pid;
    let second_wall_ms = second_wall.as_millis();
    note(&format!(
        "case=next-child first_pid={first_pid} second_pid={second_pid} neighbor_pid={neighbor_pid} neighbor_alive=1 second_wall_ms={second_wall_ms}"
    ));
}

#[test]
fn g_direct_kill_leaves_a_grandchild() {
    let req = request(&fresh(), "SELECT 1");
    let worker = fault("orphan");
    let slot = Arc::new(Mutex::new(0_u32));
    let observed = Arc::clone(&slot);
    let result = supervise_with(&worker, &req, &CancelFlag::new(), move |pid| {
        let cwd = fs::read_link(format!("/proc/{pid}/cwd")).unwrap_or_default();
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(200) {
            if let Ok(text) = fs::read_to_string(cwd.join("grandchild.pid"))
                && let Ok(grandchild) = text.trim().parse::<u32>()
            {
                *observed.lock().expect("slot") = grandchild;
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
    });
    let report = expect_timeout(result, "orphan parent was not timed out");
    assert!(report.reaped, "{report:?}");
    assert_eq!(report.term_signal, Some(9), "{report:?}");
    let grandchild = *slot.lock().expect("slot");
    assert_ne!(grandchild, 0, "grandchild pid was not recorded before reap");
    let start = proc_field(grandchild, 19).expect("grandchild starttime");
    let comm = fs::read_to_string(format!("/proc/{grandchild}/comm")).unwrap_or_default();
    let comm_text = comm.trim();
    note(&format!(
        "case=grandchild-survives pid={grandchild} start={start} comm={comm_text} parent_signal=9"
    ));
    assert!(comm.starts_with("sleep"), "grandchild comm {comm}");
    let _ = Command::new("/bin/kill").arg("-9").arg(grandchild.to_string()).status();
    thread::sleep(Duration::from_millis(50));
    if let Some(now) = proc_field(grandchild, 19) {
        assert_ne!(now, start, "cleanup did not reap the grandchild");
    }
}

#[test]
fn h_grandchild_stdout_is_not_a_sandbox() {
    let req = request(&fresh(), "SELECT 1");
    let worker = fault("orphan-stdout");
    let slot = Arc::new(Mutex::new((PathBuf::new(), 0_u32)));
    let observed = Arc::clone(&slot);
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = supervise_with(&worker, &req, &CancelFlag::new(), move |pid| {
            let cwd = fs::read_link(format!("/proc/{pid}/cwd")).unwrap_or_default();
            let start = Instant::now();
            let mut grandchild = 0_u32;
            while start.elapsed() < Duration::from_millis(200) {
                if let Ok(text) = fs::read_to_string(cwd.join("grandchild.pid"))
                    && let Ok(parsed) = text.trim().parse::<u32>()
                {
                    grandchild = parsed;
                    break;
                }
                thread::sleep(Duration::from_millis(5));
            }
            *observed.lock().expect("cwd") = (cwd, grandchild);
        });
        let _ = tx.send(result);
    });
    match rx.recv_timeout(Duration::from_secs(3)) {
        Ok(result) => {
            let report = expect_timeout(result, "grandchild-stdout was not a timeout");
            assert_eq!(report.term_signal, Some(9), "{report:?}");
            assert!(report.reaped, "{report:?}");
            let (_cwd, gpid) = slot.lock().expect("cwd").clone();
            let alive = proc_field(gpid, 19);
            note(&format!(
                "case=grandchild-stdout hung=0 signal=9 reaped=1 grandchild={gpid} alive={}",
                u8::from(alive.is_some())
            ));
            assert_ne!(gpid, 0, "grandchild pid was not recorded");
            assert!(alive.is_some(), "direct SIGKILL also removed the grandchild");
            let _ = Command::new("/bin/kill").arg("-9").arg(gpid.to_string()).status();
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let (cwd, gpid) = slot.lock().expect("cwd").clone();
            note(&format!(
                "case=grandchild-stdout hung=1 cwd={} grandchild={gpid}",
                cwd.display()
            ));
            if gpid != 0 {
                let _ = Command::new("/bin/kill").arg("-9").arg(gpid.to_string()).status();
            }
            match rx.recv_timeout(Duration::from_secs(2)) {
                Ok(result) => note(&format!("case=grandchild-stdout unblocked_by_test_kill={result:?}")),
                Err(_) => {
                    note("case=grandchild-stdout still_hung_after_grandchild_kill=1");
                    panic!("killing the grandchild did not unblock the supervisor");
                }
            }
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => panic!("supervisor thread ended without a result"),
    }
}

#[test]
fn i_namespace_and_file_descriptors_are_not_isolated() {
    let marker = File::open("/etc/hosts").expect("hosts");
    let marker_fd = marker.as_raw_fd();
    let req = request(&fresh(), "SELECT 1");
    let worker = fault("probe");
    let slot = Arc::new(Mutex::new(String::new()));
    let observed = Arc::clone(&slot);
    let cancel = CancelFlag::new();
    let result = supervise_with(&worker, &req, &cancel, {
        let cancel = &cancel;
        move |pid| {
            let cwd = fs::read_link(format!("/proc/{pid}/cwd")).unwrap_or_default();
            let start = Instant::now();
            let mut probe = String::new();
            while start.elapsed() < Duration::from_millis(250) {
                if let Ok(text) = fs::read_to_string(cwd.join("probe.txt")) {
                    probe = text;
                    break;
                }
                thread::sleep(Duration::from_millis(5));
            }
            let fds = fd_list(pid);
            let same_net = read_ns(pid, "net") == read_ns(0, "net");
            let same_mnt = read_ns(pid, "mnt") == read_ns(0, "mnt");
            let same_pid = read_ns(pid, "pid") == read_ns(0, "pid");
            let same_user = read_ns(pid, "user") == read_ns(0, "user");
            let pgrp = proc_field(pid, 2).unwrap_or(0);
            let env = fs::read(format!("/proc/{pid}/environ")).unwrap_or_default();
            let cmdline = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
            let exe = fs::read_link(format!("/proc/{pid}/exe")).ok();
            let hosts = fds.iter().any(|item| item.contains("/etc/hosts"));
            let fds_text = fds.join(";");
            let exe_len = exe
                .as_ref()
                .map(|path| path.as_os_str().as_bytes().len())
                .unwrap_or(0);
            let line = format!(
                "case=probe pid={pid} pgrp={pgrp} own_group={} env_bytes={} cmdline_bytes={} fds_n={} hosts_fd={hosts} marker_fd={marker_fd} net_shared={same_net} mnt_shared={same_mnt} pid_shared={same_pid} user_shared={same_user} exe_len={exe_len} probe={} fds={fds_text}",
                u8::from(pgrp == u64::from(pid)),
                env.len(),
                cmdline.len(),
                fds.len(),
                probe.trim()
            );
            *observed.lock().expect("probe") = line;
            cancel.cancel();
        }
    });
    let report = expect_cancel(result, "probe was not cancelled");
    assert_eq!(report.term_signal, Some(9), "{report:?}");
    assert!(report.reaped, "{report:?}");
    let line = slot.lock().expect("probe").clone();
    assert!(line.contains("net_shared=true"), "{line}");
    assert!(line.contains("mnt_shared=true"), "{line}");
    assert!(line.contains("passwd=true"), "{line}");
    assert!(
        line.contains("listen=ok:") || line.contains("listen=err:"),
        "{line}"
    );
    if line.contains("listen=err:") {
        note("case=probe listen_error_recorded=1 not_relabeled_as_sandbox=1");
    }
    assert!(line.contains("env_bytes=0"), "{line}");
    assert!(line.contains("own_group=1"), "{line}");
    assert!(line.contains("hosts_fd=false"), "{line}");
    note(&line);
    drop(marker);
}

fn fd_list(pid: u32) -> Vec<String> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(format!("/proc/{pid}/fd")) else {
        return found;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let target = fs::read_link(entry.path())
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| "?".to_string());
        found.push(format!("{name}->{target}"));
    }
    found.sort();
    found
}

fn read_ns(pid: u32, kind: &str) -> String {
    let path = if pid == 0 {
        format!("/proc/self/ns/{kind}")
    } else {
        format!("/proc/{pid}/ns/{kind}")
    };
    fs::read_link(path)
        .map(|link| link.display().to_string())
        .unwrap_or_default()
}

#[test]
fn j_production_binary_request_trailer_is_recorded() {
    let arg = Command::new(prod().path())
        .arg("--slow")
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("slow arg");
    let env_slow = Command::new(prod().path())
        .env_clear()
        .env("OO_PARSER_WORKER_SLOW", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("slow env");
    let arg_exit = arg.code();
    let env_exit = env_slow.code();
    note(&format!(
        "case=production-rejects-fault-switches arg_exit={arg_exit:?} env_exit={env_exit:?}"
    ));
    assert_eq!(arg.code(), Some(2));
    assert_eq!(env_slow.code(), Some(2));

    let req = request(&fresh(), "SELECT 1");
    let mut frame = encode_frame(KIND_REQUEST, &encode_request(&req).expect("body")).expect("frame");
    frame.push(0x7f);
    assert_eq!(decode_frame(&frame).unwrap_err(), CodecError::Trailing);
    // Direct spawn, outside supervise_with. The supervisor rejects a trailing
    // byte. This records whether the live child does.
    let mut child = Command::new(prod().path())
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn worker");
    let mut stdin = child.stdin.take().expect("stdin");
    stdin.write_all(&frame).expect("write");
    drop(stdin);
    let mut stdout = child.stdout.take().expect("stdout");
    let reader = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let status = child.wait().expect("wait");
    let out = reader.join().expect("read");
    let exit = status.code();
    let stdout_len = out.len();
    note(&format!(
        "case=child-request-trailer exit={exit:?} stdout_len={stdout_len} decode_frame=trailing"
    ));
}
