//! Independent review of c27d6349. Not part of the product archive.
//!
//! The deadline under test is 500 ms from immediately before spawn.
//! The contract bounds the in-flight socket wait by one 20 ms slice.
//! The parent poll quantum in this tree is 10 ms. A call that is still
//! inside `supervise_with` after 2 s is a hung join.

#![forbid(unsafe_code)]

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use oo_gateway::parser_worker::{
    decode_frame, encode_frame, encode_request, supervise_with, AdmitRequest, CancelFlag,
    CodecError, ParserResponse, ProcessReport, RestrictionCapability, SuperviseError,
    TrustedWorker, KIND_REQUEST, PARSE_DEADLINE, RESTRICTION_CAPABILITY,
};
use oo_gateway::query_executor::{UntrustedAdmission, UNTRUSTED_ADMISSION};
use oo_gateway::{
    guard_select, GuardError, PolicyCatalog, PolicyIdentity, ResolvedFunction, ResolvedOperator,
    VisibleColumn, VisibleRelation, LiteralTypes, PINNED_PARSER_VERSION,
};
use sha2::{Digest, Sha256};

const DEADLINE_MS: u128 = 500;
const SLICE_MS: u128 = 20;
const POLL_MS: u128 = 10;
const CEILING_MS: u128 = DEADLINE_MS + SLICE_MS + POLL_MS;
const WATCHDOG_MS: u64 = 2_000;
const INT_OID: u32 = 23;
const TEXT_OID: u32 = 25;
const BOOL_OID: u32 = 16;
const COUNT_OID: u32 = 9101;
const EQ_INT_OID: u32 = 9102;
const EQ_INT_PROC: u32 = 9103;
const EQ_TEXT_OID: u32 = 9104;
const EQ_TEXT_PROC: u32 = 9105;
const CANARY: &str = "canary_secret_sql_a215";

const EXECUTOR_SRC: &str = include_str!("../src/query_executor.rs");
const CONTRACT_SRC: &str = include_str!("../../../contracts/gateway/parser-worker-v1.md");
const WORKER_SRC: &str = include_str!("../src/parser_worker.rs");

const FAULT_SRC: &str = r#"
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

fn main() {
    match option_env!("OO_FAULT_MODE").unwrap_or("") {
        "slow" => thread::sleep(Duration::from_secs(30)),
        "crash" => std::process::exit(99),
        "partial" => partial(),
        "hold-stdout" => hold_stdout(),
        "late" => late(),
        "orphan" => orphan(false),
        "orphan-stdout" => orphan(true),
        "setsid-stdout" => setsid_stdout(),
        _ => std::process::exit(2),
    }
}

fn partial() {
    let mut out = std::io::stdout().lock();
    out.write_all(b"OOSW").unwrap();
    out.flush().unwrap();
    thread::sleep(Duration::from_secs(30));
}

fn hold_stdout() {
    let mut out = std::io::stdout().lock();
    out.write_all(b"OOSW").unwrap();
    out.write_all(&1u16.to_le_bytes()).unwrap();
    out.write_all(&3u16.to_le_bytes()).unwrap();
    out.write_all(&1u32.to_le_bytes()).unwrap();
    out.write_all(&[1]).unwrap();
    out.flush().unwrap();
    thread::sleep(Duration::from_secs(30));
}

fn late() {
    thread::sleep(Duration::from_millis(700));
    hold_stdout();
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
    let child = Command::new(setsid)
        .arg("/bin/sleep")
        .arg("25")
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::null())
        .spawn()
        .expect("setsid");
    let id = child.id();
    std::mem::forget(child);
    let _ = std::fs::write("grandchild.pid", id.to_string());
    thread::sleep(Duration::from_secs(30));
}
"#;

struct Measured {
    wall: Duration,
    result: Result<ParserResponse, SuperviseError>,
    hung: bool,
}

struct Sleeper {
    child: std::process::Child,
    pid: u32,
    start: u64,
}

impl Drop for Sleeper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn note(line: &str) {
    eprintln!("{line}");
    static FILE: OnceLock<Mutex<File>> = OnceLock::new();
    let file = FILE.get_or_init(|| {
        let dir = std::env::var("OO_A215_ORACLE_DIR").expect("OO_A215_ORACLE_DIR");
        let mut path = PathBuf::from(dir);
        path.push("oracle.txt");
        let handle = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .expect("oracle");
        Mutex::new(handle)
    });
    let mut handle = file.lock().expect("oracle lock");
    writeln!(handle, "{line}").expect("oracle write");
}

fn ms(duration: Duration) -> u128 {
    duration.as_millis()
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

fn wait_absent(pid: u32) -> bool {
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(200) {
        if proc_field(pid, 19).is_none() {
            return true;
        }
        thread::sleep(Duration::from_millis(5));
    }
    proc_field(pid, 19).is_none()
}

fn start_ticks(pid: u32) -> u64 {
    let started = Instant::now();
    loop {
        if let Some(value) = proc_field(pid, 19) {
            return value;
        }
        if started.elapsed() > Duration::from_millis(200) {
            panic!("no starttime for {pid}");
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn signal_pid(pid: u32) {
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

fn own_pgrp() -> u64 {
    proc_field(std::process::id(), 2).expect("own pgrp")
}

fn signal_foreign_group(leader: u32) {
    if leader <= 1 || u64::from(leader) == own_pgrp() {
        return;
    }
    if proc_field(leader, 2) != Some(u64::from(leader)) {
        return;
    }
    let _ = Command::new("/bin/kill")
        .arg("-KILL")
        .arg("--")
        .arg(format!("-{leader}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn group_members(leader: u32) -> Vec<u32> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return found;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        if proc_field(pid, 2) == Some(u64::from(leader)) {
            found.push(pid);
        }
    }
    found
}

fn spawn_neighbor() -> Sleeper {
    let child = Command::new("/bin/sleep")
        .arg("40")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("neighbor");
    let pid = child.id();
    let start = start_ticks(pid);
    assert_eq!(proc_field(pid, 2), Some(own_pgrp()), "neighbor left our group");
    Sleeper { child, pid, start }
}

fn neighbor_same(neighbor: &Sleeper) -> bool {
    proc_field(neighbor.pid, 19) == Some(neighbor.start)
}

fn prod() -> TrustedWorker {
    TrustedWorker::approve(Path::new(env!("CARGO_BIN_EXE_oo-sql-parser-worker"))).expect("worker")
}

fn faults() -> &'static Vec<(String, PathBuf)> {
    static ALL: OnceLock<Vec<(String, PathBuf)>> = OnceLock::new();
    ALL.get_or_init(|| {
        let dir = std::env::current_exe()
            .expect("exe")
            .parent()
            .expect("exe dir")
            .join("a215-fault");
        fs::create_dir_all(&dir).expect("fault dir");
        let src = dir.join("fault.rs");
        fs::write(&src, FAULT_SRC).expect("fault src");
        let modes = [
            "slow",
            "crash",
            "partial",
            "hold-stdout",
            "late",
            "orphan",
            "orphan-stdout",
            "setsid-stdout",
        ];
        let mut built = Vec::new();
        for mode in modes {
            let bin = dir.join(format!("fault-{mode}"));
            let output = Command::new("rustc")
                .current_dir(&dir)
                .arg("--edition")
                .arg("2024")
                .arg("-D")
                .arg("warnings")
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
            built.push((mode.to_string(), bin));
        }
        built
    })
}

fn fault(mode: &str) -> TrustedWorker {
    let path = faults()
        .iter()
        .find(|(name, _)| name == mode)
        .unwrap_or_else(|| panic!("missing {mode}"))
        .1
        .clone();
    TrustedWorker::approve(&path).expect("fault path")
}

fn column(name: &str, physical: &str, type_oid: u32) -> VisibleColumn {
    VisibleColumn {
        name: name.to_string(),
        physical_name: physical.to_string(),
        type_oid,
    }
}

fn yard() -> PolicyCatalog {
    PolicyCatalog {
        identity: PolicyIdentity {
            project: "yard".to_string(),
            role: "oo_r_clerk".to_string(),
            policy_id: "policy-a215-yard".to_string(),
        },
        relations: vec![VisibleRelation {
            schema: "yard".to_string(),
            name: "bin".to_string(),
            physical_schema: "_oo".to_string(),
            physical_name: "pt_bin".to_string(),
            columns: vec![
                column("bin_id", "pc_bin", INT_OID),
                column("zone", "pc_zone", TEXT_OID),
            ],
            pending_unclassified: 0,
            estimated_rows: 12,
        }],
        functions: vec![ResolvedFunction {
            oid: COUNT_OID,
            schema: "pg_catalog".to_string(),
            name: "count".to_string(),
            argument_type_oids: Vec::new(),
            aggregate_star: true,
        }],
        operators: vec![
            ResolvedOperator {
                oid: EQ_INT_OID,
                procedure_oid: EQ_INT_PROC,
                schema: "pg_catalog".to_string(),
                name: "=".to_string(),
                left_type_oid: INT_OID,
                right_type_oid: INT_OID,
            },
            ResolvedOperator {
                oid: EQ_TEXT_OID,
                procedure_oid: EQ_TEXT_PROC,
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
        large_table_row_threshold: 100,
    }
}

fn request(catalog: &PolicyCatalog, sql: &str) -> AdmitRequest {
    AdmitRequest {
        identity: catalog.identity.clone(),
        catalog: catalog.clone(),
        sql: sql.to_string(),
    }
}

fn bulky() -> AdmitRequest {
    let mut catalog = yard();
    catalog.relations.clear();
    for rel in 0..70 {
        let mut columns = Vec::new();
        for col in 0..70 {
            columns.push(column(
                &format!("n{rel:02}{col:02}{}", "x".repeat(40)),
                &format!("p{rel:02}{col:02}"),
                INT_OID,
            ));
        }
        catalog.relations.push(VisibleRelation {
            schema: "yard".to_string(),
            name: format!("r{rel:02}"),
            physical_schema: "_oo".to_string(),
            physical_name: format!("t{rel:02}"),
            columns,
            pending_unclassified: 0,
            estimated_rows: 1,
        });
    }
    let req = request(&catalog, "SELECT 1");
    let encoded = encode_request(&req).expect("bulky");
    assert!(
        encoded.len() > 300_000,
        "request {} can sit in a socket buffer",
        encoded.len()
    );
    req
}

fn assert_live(pid: u32, worker: &TrustedWorker) -> u64 {
    let start = start_ticks(pid);
    let pgrp = proc_field(pid, 2).unwrap_or_else(|| panic!("no pgrp {pid}"));
    assert_eq!(pgrp, u64::from(pid), "child {pid} pgrp {pgrp}");
    let env = fs::read(format!("/proc/{pid}/environ")).unwrap_or_default();
    assert!(env.is_empty(), "child env {}", env.len());
    let exe = fs::read_link(format!("/proc/{pid}/exe")).unwrap_or_default();
    assert_eq!(exe, worker.path());
    let cwd = fs::read_link(format!("/proc/{pid}/cwd")).unwrap_or_default();
    let name = cwd.file_name().and_then(|item| item.to_str()).unwrap_or("");
    assert!(name.starts_with("oo-parser-worker-"), "{cwd:?}");
    start
}

fn wait_grand(pid: u32, slot: &AtomicU32) {
    let cwd = fs::read_link(format!("/proc/{pid}/cwd")).unwrap_or_default();
    let look = Instant::now();
    while look.elapsed() < Duration::from_millis(150) {
        if let Ok(text) = fs::read_to_string(cwd.join("grandchild.pid"))
            && let Ok(parsed) = text.trim().parse::<u32>()
        {
            slot.store(parsed, Ordering::SeqCst);
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn run_case(
    worker: &TrustedWorker,
    request: &AdmitRequest,
    check_identity: bool,
    grand_slot: bool,
) -> (Measured, u32, u64, u32) {
    let child_pid = Arc::new(AtomicU32::new(0));
    let child_start = Arc::new(AtomicU64::new(0));
    let grand = Arc::new(AtomicU32::new(0));
    let (tx, rx) = mpsc::channel();
    let started = Instant::now();
    let worker_call = worker.clone();
    let worker_id = worker.clone();
    let request_call = request.clone();
    let pid_slot = Arc::clone(&child_pid);
    let start_slot = Arc::clone(&child_start);
    let grand_copy = Arc::clone(&grand);
    thread::spawn(move || {
        let result = supervise_with(&worker_call, &request_call, &CancelFlag::new(), move |pid| {
            pid_slot.store(pid, Ordering::SeqCst);
            if check_identity {
                let start = assert_live(pid, &worker_id);
                start_slot.store(start, Ordering::SeqCst);
            }
            if grand_slot {
                wait_grand(pid, &grand_copy);
            }
        });
        let _ = tx.send(result);
    });
    let measured = match rx.recv_timeout(Duration::from_millis(WATCHDOG_MS)) {
        Ok(result) => Measured {
            wall: started.elapsed(),
            result,
            hung: false,
        },
        Err(mpsc::RecvTimeoutError::Disconnected) => panic!("supervisor thread disconnected"),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let pid = child_pid.load(Ordering::SeqCst);
            signal_pid(grand.load(Ordering::SeqCst));
            signal_pid(pid);
            signal_foreign_group(pid);
            match rx.recv_timeout(Duration::from_millis(WATCHDOG_MS)) {
                Ok(result) => Measured {
                    wall: started.elapsed(),
                    result,
                    hung: true,
                },
                Err(mpsc::RecvTimeoutError::Disconnected) => panic!("supervisor thread disconnected"),
                Err(mpsc::RecvTimeoutError::Timeout) => panic!("supervisor still blocked after cleanup"),
            }
        }
    };
    let pid = child_pid.load(Ordering::SeqCst);
    let start = child_start.load(Ordering::SeqCst);
    let grand_pid = grand.load(Ordering::SeqCst);
    (measured, pid, start, grand_pid)
}

fn timeout_report(label: &str, measured: &Measured) -> ProcessReport {
    assert!(!measured.hung, "{label} hung wall_ms={}", ms(measured.wall));
    match &measured.result {
        Err(SuperviseError::TimedOut(report)) => report.clone(),
        other => panic!("{label} {other:?}"),
    }
}

fn assert_clock(label: &str, wall: Duration, report: &ProcessReport) {
    let elapsed_ms = ms(report.elapsed);
    let wall_ms = ms(wall);
    assert!(
        elapsed_ms >= DEADLINE_MS,
        "{label} elapsed {elapsed_ms} ms ended before the 500 ms deadline"
    );
    assert!(
        elapsed_ms <= CEILING_MS,
        "{label} elapsed {elapsed_ms} ms exceeds 500+20 slice+10 poll"
    );
    assert!(
        wall_ms <= CEILING_MS.saturating_add(250),
        "{label} wall {wall_ms} ms is not a bounded return"
    );
    assert!(wall_ms < u128::from(WATCHDOG_MS), "{label} wall {wall_ms}");
    assert_eq!(report.term_signal, Some(9), "{label} {report:?}");
    assert!(report.reaped, "{label} {report:?}");
    assert!(!report.same_pid_alive, "{label} {report:?}");
    assert!(report.pid_absent, "{label} {report:?}");
    assert!(!report.pid_reused, "{label} {report:?}");
}

fn note_clock(label: &str, wall: Duration, report: &ProcessReport, extra: &str) {
    note(&format!(
        "case={label} hung=0 wall_ms={} elapsed_ms={} signal={} reaped={} pid_absent={} pid_reused={} pid={} {extra}",
        ms(wall),
        ms(report.elapsed),
        report.term_signal.map(|item| item.to_string()).unwrap_or_default(),
        u8::from(report.reaped),
        u8::from(report.pid_absent),
        u8::from(report.pid_reused),
        report.pid
    ));
}

fn direct_worker(bytes: &[u8], configure: impl FnOnce(&mut Command)) -> (i32, Vec<u8>, Duration) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_oo-sql-parser-worker"));
    cmd.env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    configure(&mut cmd);
    let mut child = cmd.spawn().expect("worker");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = child.stdout.take().expect("stdout");
    stdin.write_all(bytes).expect("write");
    drop(stdin);
    let started = Instant::now();
    let status = loop {
        match child.try_wait().expect("wait") {
            Some(status) => break status,
            None if started.elapsed() > Duration::from_secs(2) => {
                let _ = child.kill();
                panic!("direct worker did not exit");
            }
            None => thread::sleep(Duration::from_millis(5)),
        }
    };
    let wall = started.elapsed();
    let mut out = Vec::new();
    stdout.read_to_end(&mut out).expect("read");
    (status.code().unwrap_or(-1), out, wall)
}

#[test]
fn capability_admission_and_the_kill_binary() {
    assert_eq!(RESTRICTION_CAPABILITY, RestrictionCapability::Unsupported);
    assert_eq!(UNTRUSTED_ADMISSION, UntrustedAdmission::Blocked);
    assert_eq!(PARSE_DEADLINE, Duration::from_millis(DEADLINE_MS as u64));
    assert!(WORKER_SRC.contains("const IO_SLICE: Duration = Duration::from_millis(20);"));
    assert!(WORKER_SRC.contains("const GROUP_KILL_BIN: &str = \"/bin/kill\";"));
    assert!(WORKER_SRC.contains(".arg(\"-KILL\")"));
    assert!(WORKER_SRC.contains(".arg(\"--\")"));
    assert!(CONTRACT_SRC.contains("Reuse of that id is not prevented."));
    assert!(CONTRACT_SRC.contains("bounded by one slice"));
    let claim = CONTRACT_SRC.contains("still says a 500 ms worker is not implemented");
    let stale = EXECUTOR_SRC.contains("No 500 ms process-isolated parser worker is implemented.");
    let blocked_reason = EXECUTOR_SRC.contains("kernel sandbox is unsupported");
    let mut magic = [0u8; 4];
    File::open("/bin/kill")
        .expect("kill")
        .read_exact(&mut magic)
        .expect("kill bytes");
    let followed = fs::metadata("/bin/kill").expect("kill meta");
    let link = fs::symlink_metadata("/bin/kill").expect("kill link");
    assert!(followed.is_file(), "/bin/kill is not a file");
    assert_eq!(&magic, b"\x7fELF");
    note(&format!(
        "case=identity restriction=Unsupported admission=Blocked deadline_ms={DEADLINE_MS} slice_ms={SLICE_MS} poll_ms={POLL_MS} ceiling_ms={CEILING_MS} kill_elf=1 kill_len={} kill_symlink={} contract_claims_stale={} executor_has_stale_sentence={} executor_names_unsupported={} pid_reuse_residual=recorded_group_signaled_when_proc_leader_absent",
        followed.len(),
        u8::from(link.file_type().is_symlink()),
        u8::from(claim),
        u8::from(stale),
        u8::from(blocked_reason)
    ));
}

#[test]
fn compiled_worker_matches_the_guard_and_rejects_a_trailer() {
    let catalog = yard();
    let statements = [
        "SELECT/*a215*/ bin_id, zone FROM yard.bin WHERE bin_id = 7",
        "SELECT * FROM yard.bin WHERE zone = 'north'",
        "SELECT count(*) FROM yard.bin WHERE bin_id = 7",
    ];
    for sql in statements {
        let req = request(&catalog, sql);
        let expected = guard_select(&req.identity, &req.catalog, sql).expect("guard");
        let (measured, pid, _, _) = run_case(&prod(), &req, false, false);
        assert!(!measured.hung, "legal hung {}", ms(measured.wall));
        let ParserResponse::Plan(plan) = measured.result.expect("legal plan") else {
            panic!("{sql} refused");
        };
        assert_eq!(*plan, expected);
        assert_eq!(plan.original_sha256, digest(sql));
        assert_eq!(plan.executed_sha256, digest(&plan.executed_sql));
        assert_eq!(plan.parser_version, PINNED_PARSER_VERSION);
        assert!(plan.executed_sql.contains("pt_bin"), "{}", plan.executed_sql);
        assert!(ms(measured.wall) < DEADLINE_MS, "legal wall {}", ms(measured.wall));
        note(&format!(
            "case=legal wall_ms={} pid={} sha_match=1 parser_version={} sql_len={}",
            ms(measured.wall),
            pid,
            plan.parser_version,
            sql.len()
        ));
    }
    let star = guard_select(&catalog.identity, &catalog, statements[1]).expect("star");
    assert_eq!(
        star.star_expansions[0].columns,
        vec!["bin_id".to_string(), "zone".to_string()]
    );
    let counted = guard_select(&catalog.identity, &catalog, statements[2]).expect("count");
    assert_eq!(counted.bindings.functions[0].oid, COUNT_OID);
    assert!(counted.bindings.functions[0].aggregate_star);
    assert_eq!(counted.bindings.operators[0].oid, EQ_INT_OID);

    let mut mismatch = request(
        &catalog,
        "SELECT bin_id FROM yard.bin WHERE zone = 'canary_secret_sql_a215'",
    );
    mismatch.identity.project = "other-yard".to_string();
    let expected = guard_select(&mismatch.identity, &mismatch.catalog, &mismatch.sql).expect_err("oracle");
    assert_eq!(expected, GuardError::IdentityMismatch);
    let (measured, _, _, _) = run_case(&prod(), &mismatch, false, false);
    assert!(!measured.hung, "mismatch hung {}", ms(measured.wall));
    match measured.result {
        Ok(ParserResponse::Refusal(err)) => {
            assert_eq!(err, expected);
            let shown = format!("{err} {err:?}");
            assert!(!shown.contains(CANARY), "{shown}");
            assert!(ms(measured.wall) < DEADLINE_MS, "mismatch wall {}", ms(measured.wall));
            note(&format!(
                "case=identity-mismatch wall_ms={} canary_in_display=0",
                ms(measured.wall)
            ));
        }
        other => panic!("identity mismatch {other:?}"),
    }
    trailer_and_flags();
}

fn trailer_and_flags() {
    let req = request(&yard(), "SELECT bin_id FROM yard.bin WHERE bin_id = 7");
    let frame = encode_frame(KIND_REQUEST, &encode_request(&req).expect("body")).expect("frame");
    let (code, out, wall) = direct_worker(&frame, |_| {});
    assert_eq!(code, 0, "clean frame exit {code} out {}", out.len());
    assert!(decode_frame(&out).is_ok(), "clean reply {}", out.len());
    assert!(ms(wall) < DEADLINE_MS, "clean direct wall {}", ms(wall));
    let mut trailed = frame.clone();
    trailed.push(0x7f);
    assert_eq!(decode_frame(&trailed), Err(CodecError::Trailing));
    let (code, out, wall) = direct_worker(&trailed, |_| {});
    assert_eq!(code, 1, "trailer exit {code}");
    assert!(out.is_empty(), "trailer wrote {} bytes", out.len());
    assert!(ms(wall) < DEADLINE_MS, "trailer wall {}", ms(wall));
    note(&format!(
        "case=trailer exit={code} stdout_len={} decode=Trailing wall_ms={}",
        out.len(),
        ms(wall)
    ));
    let (slow_code, slow_out, _) = direct_worker(&[], |cmd| {
        cmd.arg("--slow");
    });
    assert_eq!(slow_code, 2);
    assert!(slow_out.is_empty());
    let (env_code, env_out, _) = direct_worker(&[], |cmd| {
        cmd.env("OO_PARSER_WORKER_SLOW", "1");
    });
    assert_eq!(env_code, 2);
    assert!(env_out.is_empty());
    note("case=production-flags slow_arg_exit=2 slow_env_exit=2 stdout_empty=1");
}

#[test]
fn bounded_request_reply_timeout_and_direct_wait() {
    let neighbor = spawn_neighbor();
    let init = start_ticks(1);
    let self_start = start_ticks(std::process::id());
    let req = request(&yard(), "SELECT 1");

    let bulky_req = bulky();
    let (measured, pid, start, _) = run_case(&fault("slow"), &bulky_req, true, false);
    let report = timeout_report("blocked-stdin", &measured);
    assert_clock("blocked-stdin", measured.wall, &report);
    assert_eq!(report.pid, pid);
    assert_eq!(report.start_ticks, Some(start));
    note_clock("blocked-stdin", measured.wall, &report, "join_returned=1");

    for mode in ["hold-stdout", "partial"] {
        let (measured, pid, start, _) = run_case(&fault(mode), &req, true, false);
        let report = timeout_report(mode, &measured);
        assert_clock(mode, measured.wall, &report);
        assert_eq!(report.pid, pid);
        assert_eq!(report.start_ticks, Some(start));
        note_clock(mode, measured.wall, &report, "join_returned=1");
    }

    let (measured, pid, start, _) = run_case(&fault("late"), &req, true, false);
    let report = timeout_report("late", &measured);
    assert_clock("late", measured.wall, &report);
    assert!(
        ms(report.elapsed) < 690,
        "late elapsed {} reached the 700 ms write",
        ms(report.elapsed)
    );
    assert_eq!(report.pid, pid);
    assert_eq!(report.start_ticks, Some(start));
    note_clock("late", measured.wall, &report, "below_late_write=1");

    let (measured, pid, _, _) = run_case(&fault("crash"), &req, false, false);
    assert!(!measured.hung, "crash hung");
    let Err(SuperviseError::Crashed(report)) = &measured.result else {
        panic!("crash {:?}", measured.result);
    };
    assert_eq!(report.exit_code, Some(99), "{report:?}");
    assert_eq!(report.term_signal, None, "{report:?}");
    assert!(report.reaped, "{report:?}");
    assert!(report.pid_absent, "{report:?}");
    assert!(!report.pid_reused, "{report:?}");
    assert!(ms(report.elapsed) < DEADLINE_MS, "crash waited {}", ms(report.elapsed));
    assert!(ms(measured.wall) < DEADLINE_MS, "crash wall {}", ms(measured.wall));
    assert_eq!(report.pid, pid);
    assert!(report.start_ticks.is_some(), "{report:?}");
    note(&format!(
        "case=crash hung=0 wall_ms={} elapsed_ms={} exit=99 signal= reaped=1 pid_absent=1 direct_wait=1",
        ms(measured.wall),
        ms(report.elapsed)
    ));

    let (again, next_pid, _, _) = run_case(
        &prod(),
        &request(&yard(), "SELECT bin_id FROM yard.bin"),
        false,
        false,
    );
    assert!(!again.hung);
    assert!(again.result.is_ok(), "{:?}", again.result);
    assert_ne!(next_pid, pid);
    assert!(neighbor_same(&neighbor), "neighbor start changed");
    assert_eq!(proc_field(1, 19), Some(init), "pid 1 changed");
    assert_eq!(proc_field(std::process::id(), 19), Some(self_start));
    note(&format!(
        "case=after-timeout next_pid={next_pid} previous_pid={pid} neighbor_same=1 pid1_same=1 own_same=1"
    ));
}

#[test]
fn same_group_grandchild_stdout_join_is_bounded() {
    let neighbor = spawn_neighbor();
    let init = start_ticks(1);
    let req = request(&yard(), "SELECT 1");
    for mode in ["orphan-stdout", "orphan"] {
        let (measured, pid, start, grand) = run_case(&fault(mode), &req, true, true);
        let report = timeout_report(mode, &measured);
        assert_clock(mode, measured.wall, &report);
        assert_eq!(report.pid, pid);
        assert_eq!(report.start_ticks, Some(start));
        assert_ne!(grand, 0, "{mode} grandchild was not recorded");
        assert!(wait_absent(grand), "{mode} grandchild {grand} still alive");
        let members = group_members(pid);
        assert!(members.is_empty(), "{mode} group {pid} still has {members:?}");
        assert!(neighbor_same(&neighbor), "{mode} neighbor changed");
        note_clock(
            mode,
            measured.wall,
            &report,
            &format!("grandchild={grand} grandchild_alive=0 group_empty=1 neighbor_same=1 join_returned=1"),
        );
    }
    assert_eq!(proc_field(1, 19), Some(init), "pid 1 changed");
}

#[test]
fn escaped_setsid_returns_and_is_not_reaped() {
    assert!(
        Path::new("/usr/bin/setsid").is_file() || Path::new("/bin/setsid").is_file(),
        "setsid missing"
    );
    let neighbor = spawn_neighbor();
    let init = start_ticks(1);
    let self_start = start_ticks(std::process::id());
    let (measured, pid, start, escaped) = run_case(
        &fault("setsid-stdout"),
        &request(&yard(), "SELECT 1"),
        true,
        true,
    );
    let report = timeout_report("setsid-stdout", &measured);
    assert_clock("setsid-stdout", measured.wall, &report);
    assert_eq!(report.pid, pid);
    assert_eq!(report.start_ticks, Some(start));
    assert_ne!(escaped, 0, "setsid pid was not recorded");
    let comm = fs::read_to_string(format!("/proc/{escaped}/comm")).unwrap_or_default();
    assert!(
        comm.starts_with("sleep") || comm.starts_with("setsid"),
        "escaped comm {comm}"
    );
    assert_ne!(proc_field(escaped, 2), Some(u64::from(pid)), "escaped stayed in the child group");
    assert!(neighbor_same(&neighbor), "neighbor changed before cleanup");
    signal_pid(escaped);
    assert!(wait_absent(escaped), "escaped pid {escaped} survived a direct signal");
    assert!(neighbor_same(&neighbor), "neighbor changed when the escaped pid was signaled");
    assert_eq!(proc_field(1, 19), Some(init), "pid 1 changed");
    assert_eq!(proc_field(std::process::id(), 19), Some(self_start));
    note_clock(
        "setsid-stdout",
        measured.wall,
        &report,
        &format!(
            "escaped={escaped} escaped_alive_before_test_signal=1 escaped_group_diff=1 neighbor_same=1 pid1_same=1 own_same=1 join_returned=1"
        ),
    );
}
