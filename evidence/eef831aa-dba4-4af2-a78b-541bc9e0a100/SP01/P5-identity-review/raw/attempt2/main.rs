//! Independent identity probe for the fixed P5 IPC parent.
//!
//! `--supervise` dispatches to the archived production supervisor so the
//! pinned image can be the worker `admit_in_worker` execs. A 32-hex argument
//! is the worker session. `--probe` is the harness. The worker speaks frames
//! itself. It does not call the native parser and it does not open a database.
//! Built without `test-hooks`.

#![forbid(unsafe_code)]

use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Instant;

use oo_p05_pg17_parser::{admit_in_worker, admit_native, sha256_file, Decision, Policy, KIND_ADMIT, KIND_RESPONSE};
use serde_json::{json, Map, Value};

const ROOT: &str = "/tmp/oo-a100-5dd178d8";
const READY_FAST_MS: u64 = 2_000;
const PARSE_FAST_MS: u64 = 700;
const CLEANUP_FAST_MS: u64 = 700;
const WALL_CAP_MS: u64 = 20_000;

fn main() {
    let mut args = std::env::args();
    let _argv0 = args.next();
    let first = args.next();
    let code = match first.as_deref() {
        Some("--supervise") => oo_p05_pg17_parser::run_worker(),
        Some("--probe") => {
            if args.next().is_some() {
                2
            } else {
                probe_main()
            }
        }
        Some(nonce) if is_nonce(nonce) => {
            if args.next().is_some() {
                2
            } else {
                worker_main(nonce)
            }
        }
        _ => 2,
    };
    std::process::exit(code);
}

fn is_nonce(text: &str) -> bool {
    text.len() == 32 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

struct Stat {
    pid: u32,
    ppid: u32,
    starttime: String,
}

fn read_stat(pid: u32) -> Option<Stat> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = text.rsplit_once(')')?.1;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let ppid = fields.get(1)?.parse().ok()?;
    let starttime = fields.get(19)?.to_string();
    if starttime.is_empty() {
        return None;
    }
    Some(Stat { pid, ppid, starttime })
}

fn own_stat() -> Option<Stat> {
    let mut stat = read_stat(std::process::id())?;
    stat.pid = std::process::id();
    Some(stat)
}

fn same_start(pid: u32, starttime: &str) -> bool {
    matches!(read_stat(pid), Some(stat) if stat.starttime == starttime)
}

fn absent_pid(avoid: &[u32]) -> (u32, bool) {
    let max_text = fs::read_to_string("/proc/sys/kernel/pid_max").unwrap_or_default();
    let mut candidate = max_text.trim().parse::<u32>().unwrap_or(4_194_304);
    for _ in 0..16 {
        if candidate > 1 && !avoid.contains(&candidate) {
            let exists = fs::metadata(format!("/proc/{candidate}")).is_ok();
            if !exists {
                return (candidate, false);
            }
        }
        candidate = candidate.saturating_sub(1);
    }
    (u32::MAX, fs::metadata(format!("/proc/{}", u32::MAX)).is_ok())
}

struct Plan {
    mode: String,
    live_pid: u32,
    live_ppid: u32,
    live_start: String,
    live_nonce: String,
    sent_ready_nonce: String,
    sent_ready_pid: u32,
    ready_nonce_omitted: bool,
    ready_pid_omitted: bool,
    ready_start_omitted: bool,
    sent_ready_start: String,
    ready_raw: String,
    ready_dup: String,
    ready_unknown: bool,
    decision_raw: String,
    sent_decision_nonce: String,
    sent_decision_pid: u32,
    decision_nonce_omitted: bool,
    decision_dup: String,
    decision_unknown_start: bool,
    decision_matched: bool,
    decision_oid: bool,
    absent_pid: u32,
    absent_existed: bool,
    foreign_pid: u32,
    foreign_start: String,
    saw_admit: bool,
    admit_kind: u32,
    admit_len: usize,
    read_eof: bool,
    decision_sent: bool,
}

fn worker_main(nonce: &str) -> i32 {
    let mode = match fs::read_to_string(format!("{ROOT}/mode")) {
        Ok(text) => text.trim().to_string(),
        Err(_) => return 2,
    };
    let Some(stat) = own_stat() else {
        return 2;
    };
    let (foreign_pid, foreign_start) = read_foreign();
    let (absent_pid, absent_existed) = absent_pid(&[stat.pid, stat.ppid, foreign_pid]);
    let Some(mut plan) = build_plan(&mode, nonce, &stat, absent_pid, absent_existed, foreign_pid, &foreign_start) else {
        return 2;
    };
    let note_path = format!("{ROOT}/notes/{mode}.json");
    let _ = fs::create_dir_all(format!("{ROOT}/notes"));
    if write_note(&note_path, &plan).is_err() {
        return 2;
    }
    if write_frame(KIND_READY, plan.ready_raw.as_bytes()).is_err() {
        return 2;
    }
    match read_frame() {
        Err(FrameRead::Eof) => {
            plan.read_eof = true;
            let _ = write_note(&note_path, &plan);
            0
        }
        Err(FrameRead::Bad) => 2,
        Ok((kind, body)) => {
            plan.saw_admit = true;
            plan.admit_kind = u32::from(kind);
            plan.admit_len = body.len();
            let _ = write_note(&note_path, &plan);
            if kind != KIND_ADMIT || plan.decision_raw.is_empty() {
                return 0;
            }
            if write_frame(KIND_RESPONSE, plan.decision_raw.as_bytes()).is_err() {
                return 2;
            }
            plan.decision_sent = true;
            let _ = write_note(&note_path, &plan);
            0
        }
    }
}

fn build_plan(
    mode: &str,
    nonce: &str,
    stat: &Stat,
    absent_pid: u32,
    absent_existed: bool,
    foreign_pid: u32,
    foreign_start: &str,
) -> Option<Plan> {
    let wrong_nonce = flip_nonce(nonce);
    let wrong_start = alter_start(&stat.starttime);
    let mut ready_nonce = nonce.to_string();
    let mut ready_pid = stat.pid;
    let mut ready_start = stat.starttime.clone();
    let mut ready_nonce_omitted = false;
    let mut ready_pid_omitted = false;
    let mut ready_start_omitted = false;
    let mut ready_dup = "none".to_string();
    let mut ready_unknown = false;
    let mut dup_ready_nonce: Option<String> = None;
    let mut decision_nonce = nonce.to_string();
    let mut decision_pid = stat.pid;
    let mut decision_nonce_omitted = false;
    let mut decision_dup = "none".to_string();
    let mut decision_unknown_start = false;
    let mut decision_matched = false;
    let mut decision_oid = false;
    let mut dup_decision_nonce: Option<String> = None;

    match mode {
        "control" | "decision-matched-false" => {}
        "ready-nonce" => ready_nonce = wrong_nonce.clone(),
        "ready-pid" => ready_pid = absent_pid,
        "ready-start" => ready_start = wrong_start,
        "ready-omit-nonce" => ready_nonce_omitted = true,
        "ready-omit-pid" => ready_pid_omitted = true,
        "ready-omit-start" => ready_start_omitted = true,
        "ready-unknown" => ready_unknown = true,
        "ready-dup-nonce-last-wrong" => {
            ready_dup = "last-wrong".to_string();
            dup_ready_nonce = Some(wrong_nonce.clone());
        }
        "ready-dup-nonce-last-correct" => {
            ready_nonce = wrong_nonce.clone();
            ready_dup = "last-correct".to_string();
            dup_ready_nonce = Some(nonce.to_string());
        }
        "decision-nonce" => decision_nonce = wrong_nonce.clone(),
        "decision-pid" => decision_pid = absent_pid,
        "decision-omit-nonce" => decision_nonce_omitted = true,
        "decision-dup-nonce-last-wrong" => {
            decision_dup = "last-wrong".to_string();
            dup_decision_nonce = Some(wrong_nonce.clone());
        }
        "decision-start-unknown" => decision_unknown_start = true,
        "decision-oid-true" => decision_oid = true,
        "decision-matched-true-bad-nonce" => {
            decision_nonce = wrong_nonce.clone();
            decision_matched = true;
        }
        _ => return None,
    }
    if mode == "decision-matched-false" {
        decision_matched = false;
    }

    let ready_raw = ready_json(
        if ready_nonce_omitted { None } else { Some(ready_nonce.as_str()) },
        if ready_pid_omitted { None } else { Some(ready_pid) },
        if ready_start_omitted { None } else { Some(ready_start.as_str()) },
        ready_unknown,
        dup_ready_nonce.as_deref(),
    );
    let decision_raw = decision_json(
        if decision_nonce_omitted { None } else { Some(decision_nonce.as_str()) },
        decision_pid,
        decision_matched,
        decision_oid,
        decision_unknown_start,
        dup_decision_nonce.as_deref(),
    );
    Some(Plan {
        mode: mode.to_string(),
        live_pid: stat.pid,
        live_ppid: stat.ppid,
        live_start: stat.starttime.clone(),
        live_nonce: nonce.to_string(),
        sent_ready_nonce: ready_nonce,
        sent_ready_pid: ready_pid,
        ready_nonce_omitted,
        ready_pid_omitted,
        ready_start_omitted,
        sent_ready_start: ready_start,
        ready_raw,
        ready_dup,
        ready_unknown,
        decision_raw,
        sent_decision_nonce: decision_nonce,
        sent_decision_pid: decision_pid,
        decision_nonce_omitted,
        decision_dup,
        decision_unknown_start,
        decision_matched,
        decision_oid,
        absent_pid,
        absent_existed,
        foreign_pid,
        foreign_start: foreign_start.to_string(),
        saw_admit: false,
        admit_kind: 0,
        admit_len: 0,
        read_eof: false,
        decision_sent: false,
    })
}

fn ready_json(nonce: Option<&str>, pid: Option<u32>, start: Option<&str>, unknown: bool, dup_nonce: Option<&str>) -> String {
    let mut map = Map::new();
    if let Some(nonce) = nonce {
        map.insert("nonce".to_string(), Value::String(nonce.to_string()));
    }
    if let Some(pid) = pid {
        map.insert("pid".to_string(), json!(pid));
    }
    if let Some(start) = start {
        map.insert("starttime".to_string(), Value::String(start.to_string()));
    }
    if unknown {
        map.insert("injected".to_string(), json!(1));
    }
    let mut text = Value::Object(map).to_string();
    if let Some(extra) = dup_nonce {
        text = append_key(text, "nonce", &Value::String(extra.to_string()).to_string());
    }
    text
}

fn decision_json(
    nonce: Option<&str>,
    pid: u32,
    matched: bool,
    oid: bool,
    unknown_start: bool,
    dup_nonce: Option<&str>,
) -> String {
    let mut map = Map::new();
    map.insert("syntax_allow".to_string(), json!(true));
    map.insert("code".to_string(), json!("ALLOW_SYNTAX"));
    map.insert("parser_version".to_string(), json!(170007));
    map.insert("node_count".to_string(), json!(7));
    map.insert("max_depth".to_string(), json!(1));
    map.insert("original_projection_star".to_string(), json!(false));
    map.insert("executed_sql".to_string(), json!("SELECT 1"));
    map.insert("detail".to_string(), json!("a100-control"));
    map.insert("oid_acl_rls_proven".to_string(), json!(oid));
    if let Some(nonce) = nonce {
        map.insert("nonce".to_string(), Value::String(nonce.to_string()));
    }
    map.insert("pid".to_string(), json!(pid));
    map.insert("reaped".to_string(), json!(false));
    map.insert("workdir_removed".to_string(), json!(false));
    map.insert("starttime_matched".to_string(), json!(matched));
    map.insert("worker_identity".to_string(), json!(""));
    map.insert("ready_wait_ms".to_string(), json!(0));
    map.insert("parse_wait_ms".to_string(), json!(0));
    map.insert("cleanup_wait_ms".to_string(), json!(0));
    map.insert("containment".to_string(), json!(""));
    map.insert("cleanup_proof".to_string(), json!(""));
    if unknown_start {
        map.insert("starttime".to_string(), json!("1"));
    }
    let mut text = Value::Object(map).to_string();
    if let Some(extra) = dup_nonce {
        text = append_key(text, "nonce", &Value::String(extra.to_string()).to_string());
    }
    text
}

fn append_key(mut body: String, key: &str, raw_value: &str) -> String {
    if body.ends_with('}') {
        body.pop();
        body.push(',');
        body.push('"');
        body.push_str(key);
        body.push_str("\":");
        body.push_str(raw_value);
        body.push('}');
    }
    body
}

fn flip_nonce(nonce: &str) -> String {
    let mut chars: Vec<char> = nonce.chars().collect();
    if let Some(last) = chars.last_mut() {
        *last = if *last == 'a' { 'b' } else { 'a' };
    } else {
        return "aa".to_string();
    }
    chars.into_iter().collect()
}

fn alter_start(start: &str) -> String {
    if start == "1" { "2".to_string() } else { "1".to_string() }
}

fn write_note(path: &str, plan: &Plan) -> io::Result<()> {
    let value = json!({
        "mode": plan.mode,
        "live_pid": plan.live_pid,
        "live_ppid": plan.live_ppid,
        "live_start": plan.live_start,
        "live_nonce": plan.live_nonce,
        "sent_ready_nonce": plan.sent_ready_nonce,
        "sent_ready_pid": plan.sent_ready_pid,
        "ready_nonce_omitted": plan.ready_nonce_omitted,
        "ready_pid_omitted": plan.ready_pid_omitted,
        "ready_start_omitted": plan.ready_start_omitted,
        "sent_ready_start": plan.sent_ready_start,
        "ready_raw": plan.ready_raw,
        "ready_dup": plan.ready_dup,
        "ready_unknown": plan.ready_unknown,
        "decision_raw": plan.decision_raw,
        "sent_decision_nonce": plan.sent_decision_nonce,
        "sent_decision_pid": plan.sent_decision_pid,
        "decision_nonce_omitted": plan.decision_nonce_omitted,
        "decision_dup": plan.decision_dup,
        "decision_unknown_start": plan.decision_unknown_start,
        "decision_matched": plan.decision_matched,
        "decision_oid": plan.decision_oid,
        "absent_pid": plan.absent_pid,
        "absent_existed": plan.absent_existed,
        "foreign_pid": plan.foreign_pid,
        "foreign_start": plan.foreign_start,
        "absent_is_foreign": plan.absent_pid == plan.foreign_pid,
        "saw_admit": plan.saw_admit,
        "admit_kind": plan.admit_kind,
        "admit_len": plan.admit_len,
        "read_eof": plan.read_eof,
        "decision_sent": plan.decision_sent,
    });
    let tmp = format!("{path}.tmp");
    fs::write(&tmp, serde_json::to_vec(&value).map_err(|err| io::Error::other(err.to_string()))?)?;
    fs::rename(tmp, path)
}

fn read_foreign() -> (u32, String) {
    let Ok(text) = fs::read_to_string(format!("{ROOT}/foreign.json")) else {
        return (0, String::new());
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return (0, String::new());
    };
    let pid = value.get("pid").and_then(|item| item.as_u64()).unwrap_or(0) as u32;
    let start = value.get("starttime").and_then(|item| item.as_str()).unwrap_or("").to_string();
    (pid, start)
}

enum FrameRead {
    Eof,
    Bad,
}

fn write_frame(kind: u16, body: &[u8]) -> io::Result<()> {
    let mut out = io::stdout().lock();
    out.write_all(b"P05P")?;
    out.write_all(&1u16.to_le_bytes())?;
    out.write_all(&kind.to_le_bytes())?;
    out.write_all(&(body.len() as u32).to_le_bytes())?;
    out.write_all(body)?;
    out.flush()
}

fn read_exact_n(n: usize) -> Result<Vec<u8>, FrameRead> {
    let mut buf = vec![0u8; n];
    let mut off = 0;
    let mut stdin = io::stdin();
    while off < n {
        match stdin.read(&mut buf[off..]) {
            Ok(0) => return Err(if off == 0 { FrameRead::Eof } else { FrameRead::Bad }),
            Ok(got) => off += got,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(FrameRead::Bad),
        }
    }
    Ok(buf)
}

fn read_frame() -> Result<(u16, Vec<u8>), FrameRead> {
    let head = read_exact_n(12)?;
    if &head[0..4] != b"P05P" {
        return Err(FrameRead::Bad);
    }
    let version = u16::from_le_bytes([head[4], head[5]]);
    if version != 1 {
        return Err(FrameRead::Bad);
    }
    let kind = u16::from_le_bytes([head[6], head[7]]);
    let len = u32::from_le_bytes([head[8], head[9], head[10], head[11]]) as usize;
    if len > 256 * 1024 {
        return Err(FrameRead::Bad);
    }
    let body = if len == 0 { Vec::new() } else { read_exact_n(len)? };
    Ok((kind, body))
}

fn probe_main() -> i32 {
    let _ = fs::create_dir_all(format!("{ROOT}/notes"));
    let policy = match fs::read_to_string("/src/fixtures/policy.json").ok().and_then(|text| Policy::parse(&text).ok()) {
        Some(policy) => policy,
        None => {
            emit(&json!({"case":"setup","error":"policy"}));
            return 2;
        }
    };
    let exe = match std::env::current_exe() {
        Ok(path) if path.is_absolute() => path,
        Ok(path) => {
            emit(&json!({"case":"setup","error":"exe-not-absolute","path":path.display().to_string()}));
            return 2;
        }
        Err(err) => {
            emit(&json!({"case":"setup","error":"exe","detail":err.to_string()}));
            return 2;
        }
    };
    let pin = match sha256_file(&exe) {
        Ok(pin) => pin,
        Err(err) => {
            emit(&json!({"case":"setup","error":"pin","detail":err.to_string()}));
            return 2;
        }
    };
    let bytes = fs::metadata(&exe).map(|meta| meta.len()).unwrap_or(0);
    emit(&json!({
        "case": "image",
        "path": exe.display().to_string(),
        "bytes": bytes,
        "pin": hex(&pin),
        "caller_hash_is_signature": false,
        "features": "default",
        "test_hooks": false,
    }));
    let mut bad_pin = pin;
    bad_pin[0] ^= 0xff;

    let mut sleep = match Command::new("/bin/sleep")
        .arg("240")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            emit(&json!({"case":"setup","error":"sleep","detail":err.to_string()}));
            return 2;
        }
    };
    let sleep_pid = sleep.id();
    let Some(sleep_stat) = read_stat(sleep_pid) else {
        emit(&json!({"case":"setup","error":"sleep-stat","pid":sleep_pid}));
        return 2;
    };
    fs::write(
        format!("{ROOT}/foreign.json"),
        serde_json::to_vec(&json!({"pid": sleep_pid, "starttime": sleep_stat.starttime})).unwrap_or_default(),
    )
    .ok();
    emit(&json!({"case":"foreign-sleep-start","pid":sleep_pid,"ppid":sleep_stat.ppid,"starttime":sleep_stat.starttime}));

    let native = admit_native("SELECT 1", &policy);
    emit(&json!({
        "case": "native-select-1-in-process",
        "code": native.code,
        "syntax_allow": native.syntax_allow,
        "executed_sql": native.executed_sql,
        "detail": native.detail,
        "node_count": native.node_count,
        "parser_version": native.parser_version,
        "sent_to_database": false,
        "note": "comparison only; the parent under test is admit_in_worker",
    }));

    let mut fails = 0u32;
    fails += run_pin("pin-mismatch", &exe, &bad_pin, &policy, sleep_pid, &sleep_stat.starttime);
    for mode in [
        "control",
        "ready-nonce",
        "ready-pid",
        "ready-start",
        "ready-omit-nonce",
        "ready-omit-pid",
        "ready-omit-start",
        "ready-unknown",
        "ready-dup-nonce-last-wrong",
        "ready-dup-nonce-last-correct",
        "decision-nonce",
        "decision-pid",
        "decision-omit-nonce",
        "decision-dup-nonce-last-wrong",
        "decision-start-unknown",
        "decision-matched-false",
        "decision-oid-true",
        "decision-matched-true-bad-nonce",
    ] {
        fails += run_mode(mode, &exe, &pin, &policy, sleep_pid, &sleep_stat.starttime, &native);
    }

    let survived = same_start(sleep_pid, &sleep_stat.starttime);
    let mut sleep_kill = "not-attempted";
    if survived {
        match sleep.kill() {
            Ok(()) => {
                let _ = sleep.wait();
                sleep_kill = "sigkill-own-sleep-after-proof";
            }
            Err(_) => sleep_kill = "kill-failed",
        }
    }
    let gone_after = !same_start(sleep_pid, &sleep_stat.starttime);
    emit(&json!({
        "summary": true,
        "fails": fails,
        "foreign_survived_all_cases": survived,
        "own_sleep_action": sleep_kill,
        "own_sleep_starttime_gone_after_own_kill": gone_after,
        "sent_to_database": false,
    }));
    if fails == 0 { 0 } else { 1 }
}

fn run_pin(name: &str, exe: &PathBuf, pin: &[u8; 32], policy: &Policy, sleep_pid: u32, sleep_start: &str) -> u32 {
    let started = Instant::now();
    let decision = admit_in_worker(exe, pin, "SELECT 1", policy);
    let wall = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    finish_case(name, &decision, wall, None, sleep_pid, sleep_start, "pin", check_pin)
}

fn run_mode(
    mode: &str,
    exe: &PathBuf,
    pin: &[u8; 32],
    policy: &Policy,
    sleep_pid: u32,
    sleep_start: &str,
    native: &Decision,
) -> u32 {
    let note_path = format!("{ROOT}/notes/{mode}.json");
    let _ = fs::remove_file(&note_path);
    let tmp = format!("{ROOT}/mode.tmp");
    if fs::write(&tmp, mode).is_err() || fs::rename(&tmp, format!("{ROOT}/mode")).is_err() {
        emit(&json!({"case": mode, "error": "mode-write"}));
        return 1;
    }
    let started = Instant::now();
    let decision = admit_in_worker(exe, pin, "SELECT 1", policy);
    let wall = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let note = fs::read_to_string(&note_path).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let want = match mode {
        "control" | "decision-matched-false" => "accept",
        "ready-dup-nonce-last-correct" | "ready-dup-nonce-last-wrong" => "dup-ready",
        "decision-dup-nonce-last-wrong" => "dup-decision",
        mode if mode.starts_with("ready-") => "ready",
        _ => "decision",
    };
    finish_case(mode, &decision, wall, note.as_ref(), sleep_pid, sleep_start, want, |view| check_mode(mode, view, native))
}

struct View<'a> {
    d: &'a Decision,
    wall: u64,
    proof_alive: u32,
    foreign_alive: bool,
    note: Option<&'a Value>,
    stage: &'static str,
}

fn finish_case<F>(
    name: &str,
    decision: &Decision,
    wall: u64,
    note: Option<&Value>,
    sleep_pid: u32,
    sleep_start: &str,
    want: &str,
    check: F,
) -> u32
where
    F: FnOnce(&View) -> Result<(), String>,
{
    let proof_alive = proof_still_alive(&decision.cleanup_proof);
    let foreign_alive = same_start(sleep_pid, sleep_start);
    let stage = stage_of(decision);
    let view = View { d: decision, wall, proof_alive, foreign_alive, note, stage };
    let judged = check(&view);
    let ok = judged.is_ok();
    emit(&json!({
        "case": name,
        "ok": ok,
        "want": want,
        "stage": stage,
        "error": judged.err().unwrap_or_default(),
        "wall_ms": wall,
        "syntax_allow": decision.syntax_allow,
        "code": decision.code,
        "executed_sql": decision.executed_sql,
        "detail": decision.detail,
        "nonce": decision.nonce,
        "pid": decision.pid,
        "oid_acl_rls_proven": decision.oid_acl_rls_proven,
        "starttime_matched": decision.starttime_matched,
        "reaped": decision.reaped,
        "workdir_removed": decision.workdir_removed,
        "worker_identity": decision.worker_identity,
        "containment": decision.containment,
        "ready_wait_ms": decision.ready_wait_ms,
        "parse_wait_ms": decision.parse_wait_ms,
        "cleanup_wait_ms": decision.cleanup_wait_ms,
        "node_count": decision.node_count,
        "parser_version": decision.parser_version,
        "cleanup_proof": decision.cleanup_proof,
        "proof_alive": proof_alive,
        "foreign_pid": sleep_pid,
        "foreign_alive": foreign_alive,
        "note": note.cloned().unwrap_or(Value::Null),
        "sent_to_database": false,
    }));
    if ok { 0 } else { 1 }
}

fn stage_of(decision: &Decision) -> &'static str {
    if decision.worker_identity == "none" && decision.pid == 0 && decision.ready_wait_ms == 0 && decision.parse_wait_ms == 0 {
        return "pin";
    }
    if decision.starttime_matched {
        return if decision.syntax_allow { "accept" } else { "decision" };
    }
    if decision.parse_wait_ms == 0 {
        "ready"
    } else {
        "other"
    }
}

fn check_pin(view: &View) -> Result<(), String> {
    let d = view.d;
    if view.stage != "pin" {
        return Err(format!("stage {}", view.stage));
    }
    if d.code != "UNTRUSTED_WORKER" {
        return Err(format!("code {}", d.code));
    }
    cleared(d)?;
    if d.reaped {
        return Err("reaped true before spawn".to_string());
    }
    if d.containment != "not-applicable" {
        return Err(format!("containment {}", d.containment));
    }
    if !d.cleanup_proof.is_empty() {
        return Err("proof without spawn".to_string());
    }
    if view.proof_alive != 0 {
        return Err(format!("falsely reaped alive={}", view.proof_alive));
    }
    foreign(view)?;
    if view.wall >= WALL_CAP_MS {
        return Err(format!("wall {}", view.wall));
    }
    Ok(())
}

fn check_mode(mode: &str, view: &View, native: &Decision) -> Result<(), String> {
    foreign(view)?;
    falsely_reaped(view)?;
    match mode {
        "control" => check_accept(view, native, false),
        "decision-matched-false" => check_accept(view, native, true),
        "ready-dup-nonce-last-correct" | "ready-dup-nonce-last-wrong" => check_dup_ready(view, native, mode),
        "decision-dup-nonce-last-wrong" => check_dup_decision(view),
        "ready-nonce" | "ready-pid" | "ready-start" | "ready-omit-nonce" | "ready-omit-pid" | "ready-omit-start" | "ready-unknown" => {
            check_ready_reject(view, mode)
        }
        _ => check_decision_reject(view, mode),
    }
}

fn check_accept_frame(view: &View, native: &Decision) -> Result<(), String> {
    let d = view.d;
    bounded(view)?;
    proved_reaped(view)?;
    if view.stage != "accept" || d.code != "ALLOW_SYNTAX" || !d.syntax_allow {
        return Err(format!("not accept stage {} code {}", view.stage, d.code));
    }
    if d.executed_sql != "SELECT 1" {
        return Err(format!("sql {}", d.executed_sql));
    }
    if d.detail != "a100-control" || d.node_count != 7 {
        return Err(format!("frame replaced detail {} nodes {}", d.detail, d.node_count));
    }
    if d.parser_version != 170007 || d.oid_acl_rls_proven {
        return Err(format!("version {} oid {}", d.parser_version, d.oid_acl_rls_proven));
    }
    if d.worker_identity != "pinned" || !d.starttime_matched {
        return Err(format!("identity {} matched {}", d.worker_identity, d.starttime_matched));
    }
    if native.detail == "a100-control" && native.node_count == 7 {
        return Err("native comparison is not distinct".to_string());
    }
    let note = view.note.ok_or("missing worker note")?;
    if !note_bool(note, "saw_admit") || note_u64(note, "admit_kind") != u64::from(KIND_ADMIT) || !note_bool(note, "decision_sent") {
        return Err("admit frame not consumed".to_string());
    }
    Ok(())
}

fn check_accept(view: &View, native: &Decision, matched_was_false: bool) -> Result<(), String> {
    check_accept_frame(view, native)?;
    let note = view.note.ok_or("missing worker note")?;
    if note_bool(note, "saw_admit") != true || note_u64(note, "admit_kind") != u64::from(KIND_ADMIT) {
        return Err("admit frame not consumed".to_string());
    }
    if note_bool(note, "decision_sent") != true {
        return Err("decision not sent".to_string());
    }
    if note_str(note, "sent_ready_nonce") != note_str(note, "live_nonce")
        || note_u64(note, "sent_ready_pid") != note_u64(note, "live_pid")
        || note_str(note, "sent_ready_start") != note_str(note, "live_start")
    {
        return Err("control ready was not live identity".to_string());
    }
    if matched_was_false && note_bool(note, "decision_matched") {
        return Err("worker matched flag was not false".to_string());
    }
    if note_u64(note, "sent_decision_pid") == note_u64(note, "foreign_pid") && note_u64(note, "foreign_pid") != 0 {
        return Err("decision pid is foreign sleep".to_string());
    }
    Ok(())
}

fn check_ready_reject(view: &View, mode: &str) -> Result<(), String> {
    let d = view.d;
    bounded(view)?;
    proved_reaped(view)?;
    cleared(d)?;
    if view.stage != "ready" || d.code != "INVALID_OUTPUT" || d.parse_wait_ms != 0 || d.starttime_matched || d.pid != 0 {
        return Err(format!(
            "ready gate stage {} code {} parse {} matched {} pid {}",
            view.stage, d.code, d.parse_wait_ms, d.starttime_matched, d.pid
        ));
    }
    if d.worker_identity != "pinned" {
        return Err(format!("identity {}", d.worker_identity));
    }
    let note = view.note.ok_or("missing worker note")?;
    if note_bool(note, "saw_admit") || note_bool(note, "decision_sent") {
        return Err("parent admitted a bad ready frame".to_string());
    }
    one_ready_change(note, mode)?;
    if note_bool(note, "absent_is_foreign") || note_bool(note, "absent_existed") {
        return Err("absent pid was a live or foreign target".to_string());
    }
    Ok(())
}

fn check_decision_reject(view: &View, mode: &str) -> Result<(), String> {
    let d = view.d;
    bounded(view)?;
    proved_reaped(view)?;
    cleared(d)?;
    if view.stage != "decision" || d.code != "INVALID_OUTPUT" || !d.starttime_matched {
        return Err(format!("decision gate stage {} code {} matched {}", view.stage, d.code, d.starttime_matched));
    }
    if d.worker_identity != "pinned" {
        return Err(format!("identity {}", d.worker_identity));
    }
    if d.detail == "" && d.node_count != 0 && d.syntax_allow {
        return Err("native-shaped repair".to_string());
    }
    let note = view.note.ok_or("missing worker note")?;
    if !note_bool(note, "saw_admit") || note_u64(note, "admit_kind") != u64::from(KIND_ADMIT) || !note_bool(note, "decision_sent") {
        return Err("decision case did not consume admit and reply".to_string());
    }
    if note_str(note, "sent_ready_nonce") != note_str(note, "live_nonce")
        || note_u64(note, "sent_ready_pid") != note_u64(note, "live_pid")
        || note_str(note, "sent_ready_start") != note_str(note, "live_start")
    {
        return Err("decision case changed the ready identity".to_string());
    }
    one_decision_change(note, mode)?;
    if d.syntax_allow || !d.executed_sql.is_empty() {
        return Err("sql or allow survived decision reject".to_string());
    }
    Ok(())
}

fn check_dup_ready(view: &View, native: &Decision, mode: &str) -> Result<(), String> {
    if view.stage == "ready" {
        return check_ready_reject(view, mode);
    }
    if view.stage == "accept" {
        check_accept_frame(view, native)?;
        let note = view.note.ok_or("missing worker note")?;
        if note_str(note, "ready_dup") != if mode.ends_with("last-correct") { "last-correct" } else { "last-wrong" } {
            return Err("dup tag".to_string());
        }
        let raw = note_str(note, "ready_raw");
        let live = note_str(note, "live_nonce");
        if live.is_empty() || !raw.contains(live) || note_str(note, "sent_decision_nonce") != live {
            return Err("accepted duplicate did not keep the session nonce".to_string());
        }
        if note_u64(note, "sent_ready_pid") != note_u64(note, "live_pid")
            || note_str(note, "sent_ready_start") != note_str(note, "live_start")
        {
            return Err("dup ready changed pid or starttime".to_string());
        }
        return Ok(());
    }
    Err(format!("dup ready unexpected stage {}", view.stage))
}

fn check_dup_decision(view: &View) -> Result<(), String> {
    let d = view.d;
    bounded(view)?;
    proved_reaped(view)?;
    falsely_reaped(view)?;
    foreign(view)?;
    if d.syntax_allow || !d.executed_sql.is_empty() {
        return Err(format!("duplicate decision nonce allowed sql={}", d.executed_sql));
    }
    if view.stage != "decision" || !d.starttime_matched || d.code != "INVALID_OUTPUT" {
        return Err(format!("dup decision stage {} code {}", view.stage, d.code));
    }
    let note = view.note.ok_or("missing worker note")?;
    if note_str(note, "decision_dup") != "last-wrong" || !note_bool(note, "decision_sent") {
        return Err("dup decision note".to_string());
    }
    Ok(())
}

fn one_ready_change(note: &Value, mode: &str) -> Result<(), String> {
    let nonce_same = note_str(note, "sent_ready_nonce") == note_str(note, "live_nonce");
    let pid_same = note_u64(note, "sent_ready_pid") == note_u64(note, "live_pid");
    let start_same = note_str(note, "sent_ready_start") == note_str(note, "live_start");
    let ok = match mode {
        "ready-nonce" => !nonce_same && pid_same && start_same && !note_bool(note, "ready_nonce_omitted"),
        "ready-pid" => nonce_same && !pid_same && start_same && note_u64(note, "sent_ready_pid") == note_u64(note, "absent_pid") && note_u64(note, "sent_ready_pid") != note_u64(note, "foreign_pid"),
        "ready-start" => nonce_same && pid_same && !start_same,
        "ready-omit-nonce" => note_bool(note, "ready_nonce_omitted") && !note_bool(note, "ready_pid_omitted") && !note_bool(note, "ready_start_omitted"),
        "ready-omit-pid" => note_bool(note, "ready_pid_omitted") && !note_bool(note, "ready_nonce_omitted") && !note_bool(note, "ready_start_omitted"),
        "ready-omit-start" => note_bool(note, "ready_start_omitted") && !note_bool(note, "ready_nonce_omitted") && !note_bool(note, "ready_pid_omitted"),
        "ready-unknown" => nonce_same && pid_same && start_same && note_bool(note, "ready_unknown"),
        "ready-dup-nonce-last-wrong" => note_str(note, "ready_dup") == "last-wrong" && nonce_same && pid_same && start_same,
        "ready-dup-nonce-last-correct" => note_str(note, "ready_dup") == "last-correct",
        _ => false,
    };
    if ok { Ok(()) } else { Err(format!("ready mutation {mode}")) }
}

fn one_decision_change(note: &Value, mode: &str) -> Result<(), String> {
    let ok = match mode {
        "decision-nonce" => note_str(note, "sent_decision_nonce") != note_str(note, "live_nonce") && note_u64(note, "sent_decision_pid") == note_u64(note, "live_pid") && !note_bool(note, "decision_oid") && !note_bool(note, "decision_unknown_start") && !note_bool(note, "decision_matched"),
        "decision-pid" => note_u64(note, "sent_decision_pid") == note_u64(note, "absent_pid") && note_u64(note, "sent_decision_pid") != note_u64(note, "live_pid") && note_u64(note, "sent_decision_pid") != note_u64(note, "foreign_pid") && note_str(note, "sent_decision_nonce") == note_str(note, "live_nonce"),
        "decision-omit-nonce" => note_bool(note, "decision_nonce_omitted"),
        "decision-start-unknown" => note_bool(note, "decision_unknown_start") && note_str(note, "sent_decision_nonce") == note_str(note, "live_nonce") && note_u64(note, "sent_decision_pid") == note_u64(note, "live_pid"),
        "decision-oid-true" => note_bool(note, "decision_oid") && note_str(note, "sent_decision_nonce") == note_str(note, "live_nonce"),
        "decision-matched-true-bad-nonce" => note_bool(note, "decision_matched") && note_str(note, "sent_decision_nonce") != note_str(note, "live_nonce"),
        _ => false,
    };
    if ok { Ok(()) } else { Err(format!("decision mutation {mode}")) }
}

fn cleared(decision: &Decision) -> Result<(), String> {
    if decision.syntax_allow || !decision.executed_sql.is_empty() || decision.oid_acl_rls_proven {
        return Err(format!(
            "not cleared allow={} sql={} oid={}",
            decision.syntax_allow, decision.executed_sql, decision.oid_acl_rls_proven
        ));
    }
    Ok(())
}

fn bounded(view: &View) -> Result<(), String> {
    let d = view.d;
    if d.ready_wait_ms >= READY_FAST_MS || d.parse_wait_ms >= PARSE_FAST_MS || d.cleanup_wait_ms >= CLEANUP_FAST_MS || view.wall >= WALL_CAP_MS {
        return Err(format!(
            "clocks ready {} parse {} cleanup {} wall {}",
            d.ready_wait_ms, d.parse_wait_ms, d.cleanup_wait_ms, view.wall
        ));
    }
    Ok(())
}

fn proved_reaped(view: &View) -> Result<(), String> {
    falsely_reaped(view)?;
    let d = view.d;
    if !d.reaped || d.containment != "proven" || !d.workdir_removed {
        return Err(format!("reap unproved reaped={} containment={} workdir={}", d.reaped, d.containment, d.workdir_removed));
    }
    if !d.cleanup_proof.contains("FINAL 0") || d.cleanup_proof.contains("SCAN_ERROR") || d.cleanup_proof.contains("UNAVAILABLE") {
        return Err("proof text".to_string());
    }
    Ok(())
}

fn falsely_reaped(view: &View) -> Result<(), String> {
    if view.d.reaped && view.proof_alive > 0 {
        return Err(format!("falsely reaped alive={}", view.proof_alive));
    }
    Ok(())
}

fn foreign(view: &View) -> Result<(), String> {
    if view.foreign_alive { Ok(()) } else { Err("foreign sleep starttime gone".to_string()) }
}

fn proof_still_alive(proof: &str) -> u32 {
    let mut alive = 0u32;
    for line in proof.lines() {
        let Some(rest) = line.strip_prefix("PID ") else { continue };
        let mut parts = rest.split_whitespace();
        let Some(pid) = parts.next().and_then(|item| item.parse::<u32>().ok()) else { continue };
        let Some(start) = parts.next() else { continue };
        if same_start(pid, start) {
            alive += 1;
        }
    }
    alive
}

fn note_bool(note: &Value, key: &str) -> bool {
    note.get(key).and_then(|item| item.as_bool()).unwrap_or(false)
}

fn note_u64(note: &Value, key: &str) -> u64 {
    note.get(key).and_then(|item| item.as_u64()).unwrap_or(0)
}

fn note_str<'a>(note: &'a Value, key: &str) -> &'a str {
    note.get(key).and_then(|item| item.as_str()).unwrap_or("")
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn emit(value: &Value) {
    let mut out = io::stdout().lock();
    let _ = serde_json::to_writer(&mut out, value);
    let _ = out.write_all(b"\n");
    let _ = out.flush();
}
