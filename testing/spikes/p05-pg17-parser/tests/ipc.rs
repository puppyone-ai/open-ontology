use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::Instant;

use oo_p05_pg17_parser::{
    admit_in_worker, exercise_untrusted, exercise_worker, sha256_file, Decision, Policy, KIND_ADMIT, KIND_CRASH, KIND_FAKE_ALLOW,
    KIND_GARBAGE, KIND_SPIN,
};

static GATE: Mutex<()> = Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    GATE.lock().unwrap_or_else(|poison| poison.into_inner())
}

#[derive(serde::Deserialize)]
struct Corpus {
    ipc: Vec<IpcCase>,
}

#[derive(serde::Deserialize)]
struct IpcCase {
    id: String,
    code: String,
}

#[test]
fn ipc_failures_never_allow() {
    let _gate = lock();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let corpus: Corpus = serde_json::from_str(&std::fs::read_to_string(root.join("corpus/expected.json")).unwrap()).unwrap();
    let worker = PathBuf::from(env!("CARGO_BIN_EXE_oo-p05-parser-worker"));
    let expect = |id: &str| -> String { corpus.ipc.iter().find(|item| item.id == id).unwrap().code.clone() };

    let spin = exercise_worker(&worker, KIND_SPIN, b"{}");
    assert_eq!(spin.code, expect("spin"));
    assert!(!spin.syntax_allow);
    assert!(spin.executed_sql.is_empty());
    assert!(spin.reaped, "spin not reaped");
    assert!(spin.workdir_removed, "spin workdir");

    let crash = exercise_worker(&worker, KIND_CRASH, b"{}");
    assert_eq!(crash.code, expect("crash"));
    assert!(!crash.syntax_allow);
    assert!(crash.reaped);
    assert!(crash.workdir_removed);

    let garbage = exercise_worker(&worker, KIND_GARBAGE, b"{}");
    assert_eq!(garbage.code, expect("garbage"));
    assert!(!garbage.syntax_allow);
    assert!(garbage.reaped);
    assert!(garbage.workdir_removed);

    let version = exercise_worker(&worker, KIND_FAKE_ALLOW, br#"{"mode":"version"}"#);
    assert_eq!(version.code, expect("fake-version"));
    assert!(!version.syntax_allow);
    assert!(version.executed_sql.is_empty());
    assert!(version.reaped);

    let oid = exercise_worker(&worker, KIND_FAKE_ALLOW, br#"{"mode":"oid"}"#);
    assert_eq!(oid.code, expect("fake-oid"));
    assert!(!oid.syntax_allow);
    assert!(!oid.oid_acl_rls_proven);
    assert!(oid.reaped);
    assert!(oid.workdir_removed);
}

#[test]
fn test_hook_cannot_turn_a_real_allow_into_syntax_allow() {
    let _gate = lock();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let policy = Policy::parse(&std::fs::read_to_string(root.join("fixtures/policy.json")).unwrap()).unwrap();
    let worker = PathBuf::from(env!("CARGO_BIN_EXE_oo-p05-parser-worker"));
    let body = serde_json::to_vec(&serde_json::json!({"sql": "SELECT 1", "policy": policy})).unwrap();
    let decision = exercise_worker(&worker, KIND_ADMIT, &body);
    assert!(!decision.syntax_allow, "{}", decision.code);
    assert_eq!(decision.code, "UNTRUSTED_WORKER");
    assert!(decision.executed_sql.is_empty());
    assert_eq!(decision.worker_identity, "test-hooks-kind");
    assert_ne!(decision.code, "UTILITY");
}

#[test]
fn contradictory_and_malicious_worker_output_is_cleared() {
    let _gate = lock();
    let contradict = fault("contradict");
    assert_eq!(contradict.code, "INVALID_OUTPUT", "{}", contradict.detail);
    assert!(!contradict.syntax_allow);
    assert!(contradict.executed_sql.is_empty());
    assert!(!contradict.executed_sql.to_ascii_lowercase().contains("drop"));
    assert_ne!(contradict.code, "UTILITY");
    assert!(contradict.ready_wait_ms < 2_000, "ready {}", contradict.ready_wait_ms);
    assert!(contradict.cleanup_wait_ms < 700, "cleanup {}", contradict.cleanup_wait_ms);
    assert!(contradict.reaped, "proof {}", contradict.cleanup_proof);
    assert_eq!(contradict.containment, "proven");

    let oid = fault("oid-true");
    assert_eq!(oid.code, "INVALID_OUTPUT");
    assert!(!oid.syntax_allow);
    assert!(!oid.oid_acl_rls_proven);
    assert!(oid.executed_sql.is_empty());
    assert!(!oid.executed_sql.contains("secret"));

    let version = fault("version-server");
    assert_eq!(version.code, "PARSER_MISMATCH");
    assert!(!version.syntax_allow);
    assert!(version.executed_sql.is_empty());

    let deny_sql = fault("deny-with-sql");
    assert_eq!(deny_sql.code, "INVALID_OUTPUT");
    assert!(deny_sql.executed_sql.is_empty());

    let unknown = fault("unknown-code");
    assert_eq!(unknown.code, "INVALID_OUTPUT");
    assert!(unknown.executed_sql.is_empty());

    let field = fault("unknown-field");
    assert_eq!(field.code, "INVALID_OUTPUT");
    assert!(!field.syntax_allow);

    let oversize = fault("oversize-sql");
    assert_eq!(oversize.code, "INVALID_OUTPUT");
    assert!(oversize.executed_sql.is_empty());
    assert!(!oversize.syntax_allow);

    let consistent = fault("consistent-allow");
    assert_eq!(consistent.code, "UNTRUSTED_WORKER");
    assert!(!consistent.syntax_allow);
    assert!(consistent.executed_sql.is_empty());
    assert_eq!(consistent.worker_identity, "untrusted-test-hook");
    assert!(consistent.reaped, "proof {}", consistent.cleanup_proof);
}

#[test]
fn malformed_frames_finish_inside_the_cleanup_ceiling() {
    let _gate = lock();
    for mode in ["bad-magic", "stdout-junk", "oversize-frame", "malformed-json", "early-exit", "premature"] {
        let started = Instant::now();
        let decision = fault(mode);
        let wall = started.elapsed().as_millis();
        // Outer wall includes hashing and copying the worker into a memfd
        // (about 2.7s on this builder). The lifecycle clocks are separate:
        // a bad frame must not wait out the 5s readiness budget.
        let protocol = decision.ready_wait_ms + decision.parse_wait_ms + decision.cleanup_wait_ms;
        assert!(protocol < 700, "{mode} protocol {protocol} wall {wall}");
        assert!(wall < 6_000, "{mode} wall {wall} ready {} parse {} cleanup {}", decision.ready_wait_ms, decision.parse_wait_ms, decision.cleanup_wait_ms);
        assert!(decision.cleanup_wait_ms < 700, "{mode} cleanup {}", decision.cleanup_wait_ms);
        assert!(!decision.syntax_allow, "{mode}");
        assert!(decision.executed_sql.is_empty(), "{mode}");
        assert!(decision.ready_wait_ms < 2_000, "{mode} ready {}", decision.ready_wait_ms);
        let expect = match mode {
            "oversize-frame" => "RESPONSE_CAP",
            "early-exit" | "premature" => "SPAWN_FAILED",
            _ => "INVALID_OUTPUT",
        };
        assert_eq!(decision.code, expect, "{mode} wall {wall}");
        assert!(decision.reaped, "{mode} proof {}", decision.cleanup_proof);
        assert_eq!(decision.containment, "proven", "{mode}");
        assert!(pids_gone(&decision), "{mode} descendants still alive\n{}", decision.cleanup_proof);
    }
}

#[test]
fn deadlines_are_separate_and_measured() {
    let _gate = lock();
    let started = Instant::now();
    let silent = fault("silent-sleep");
    let wall = started.elapsed().as_millis();
    assert_eq!(silent.code, "SPAWN_FAILED");
    assert!(silent.ready_wait_ms >= 4_900 && silent.ready_wait_ms < 5_600, "ready {}", silent.ready_wait_ms);
    assert!(silent.parse_wait_ms < 50, "parse {}", silent.parse_wait_ms);
    // Measured wall was 7793ms: ~5s readiness plus the memfd image copy.
    assert!(wall < 11_000, "wall {wall} ready {}", silent.ready_wait_ms);
    assert!(silent.reaped, "{}", silent.cleanup_proof);
    assert!(pids_gone(&silent), "{}", silent.cleanup_proof);

    for mode in ["partial", "spin-via-worker"] {
        let started = Instant::now();
        let decision = if mode == "spin-via-worker" {
            let worker = PathBuf::from(env!("CARGO_BIN_EXE_oo-p05-parser-worker"));
            exercise_worker(&worker, KIND_SPIN, b"{}")
        } else {
            fault("partial")
        };
        let wall = started.elapsed().as_millis();
        assert_eq!(decision.code, "DEADLINE", "{mode}");
        assert!(decision.parse_wait_ms >= 450 && decision.parse_wait_ms <= 800, "{mode} parse {}", decision.parse_wait_ms);
        assert!(decision.ready_wait_ms < 2_000, "{mode} ready {}", decision.ready_wait_ms);
        assert!(wall < 7_000, "{mode} wall {wall} ready {} parse {}", decision.ready_wait_ms, decision.parse_wait_ms);
        assert!(decision.reaped, "{mode} {}", decision.cleanup_proof);
        assert!(pids_gone(&decision), "{mode} {}", decision.cleanup_proof);
    }
}

#[test]
fn held_stdout_samegroup_and_setsid_are_reaped() {
    let _gate = lock();
    for mode in ["hold-setsid-after", "hold-samegroup"] {
        let started = Instant::now();
        let decision = fault(mode);
        let wall = started.elapsed().as_millis();
        assert_eq!(decision.code, "INVALID_OUTPUT", "{mode} {}", decision.cleanup_proof);
        assert!(decision.parse_wait_ms < 400, "{mode} parse {}", decision.parse_wait_ms);
        assert!(wall < 6_000, "{mode} wall {wall} parse {}", decision.parse_wait_ms);
        assert!(decision.reaped, "{mode} {}", decision.cleanup_proof);
        assert_eq!(decision.containment, "proven");
        assert!(pids_gone(&decision), "{mode} {}", decision.cleanup_proof);
        if mode == "hold-setsid-after" {
            assert!(proof_has_setsid(&decision.cleanup_proof), "no setsid line\n{}", decision.cleanup_proof);
        } else {
            assert!(proof_has_samegroup(&decision.cleanup_proof), "no same-group line\n{}", decision.cleanup_proof);
        }
    }

    let started = Instant::now();
    let noreply = fault("hold-setsid-noreply");
    let wall = started.elapsed().as_millis();
    assert_eq!(noreply.code, "DEADLINE");
    assert!(noreply.parse_wait_ms >= 450 && noreply.parse_wait_ms <= 800, "parse {}", noreply.parse_wait_ms);
    assert!(wall < 7_000, "wall {wall} parse {}", noreply.parse_wait_ms);
    assert!(noreply.reaped, "{}", noreply.cleanup_proof);
    assert!(proof_has_setsid(&noreply.cleanup_proof), "{}", noreply.cleanup_proof);
    assert!(pids_gone(&noreply), "{}", noreply.cleanup_proof);
}

#[test]
fn foreign_sleep_survives_cleanup() {
    let _gate = lock();
    let mut sleep = Command::new("/bin/sleep").arg("30").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let pid = sleep.id();
    let start = starttime(pid).expect("sleep starttime");
    let decision = fault("contradict");
    assert_eq!(decision.code, "INVALID_OUTPUT");
    assert!(same_start(pid, &start), "foreign sleep was killed");
    assert!(pids_gone(&decision), "{}", decision.cleanup_proof);
    if same_start(pid, &start) {
        unsafe { libc::kill(pid as i32, libc::SIGKILL) };
    }
    let _ = sleep.wait();
    assert!(!same_start(pid, &start), "failed to stop the sleep this test started");
}

#[test]
fn spoofed_identity_is_rejected() {
    let _gate = lock();
    for mode in ["spoof-pid", "spoof-start", "spoof-nonce"] {
        let decision = fault(mode);
        assert_eq!(decision.code, "INVALID_OUTPUT", "{mode}");
        assert!(!decision.syntax_allow);
        assert!(decision.executed_sql.is_empty());
        assert!(!decision.starttime_matched, "{mode}");
        assert!(decision.ready_wait_ms < 2_000, "{mode} ready {}", decision.ready_wait_ms);
        assert!(decision.reaped, "{mode} {}", decision.cleanup_proof);
        assert!(pids_gone(&decision), "{mode}");
    }
}

#[test]
fn extra_frame_and_stderr_flood_do_not_block() {
    let _gate = lock();
    let started = Instant::now();
    let extra = fault("extra-frame");
    assert_eq!(extra.code, "INVALID_OUTPUT");
    let extra_wall = started.elapsed().as_millis();
    assert!(extra_wall < 6_000, "extra wall {extra_wall}");
    assert!(extra.reaped, "{}", extra.cleanup_proof);

    let started = Instant::now();
    let flood = fault("stderr-flood");
    assert_eq!(flood.code, "INVALID_OUTPUT");
    assert!(!flood.syntax_allow);
    let flood_wall = started.elapsed().as_millis();
    assert!(flood_wall < 6_000, "flood wall {flood_wall}");
    assert!(flood.reaped, "{}", flood.cleanup_proof);
}

#[test]
fn file_cap_network_and_inherited_fds_are_explicit() {
    let _gate = lock();
    let held = std::fs::File::open("/etc/hosts").ok();
    let decision = fault("fsize");
    assert_eq!(decision.code, "RESOURCE_BUDGET", "{}", decision.detail);
    assert!(!decision.syntax_allow);
    assert!(decision.executed_sql.is_empty());
    assert!(decision.reaped, "{}", decision.cleanup_proof);
    assert!(decision.detail.contains("wrote="), "{}", decision.detail);
    let wrote = decision.detail.split("wrote=").nth(1).and_then(|text| text.split_whitespace().next()).and_then(|text| text.parse::<u64>().ok());
    assert!(wrote.unwrap_or(u64::MAX) <= 256 * 1024, "fsize wrote past the cap: {}", decision.detail);

    let boundary = fault("boundary");
    assert_eq!(boundary.code, "INVALID_OUTPUT");
    assert!(!boundary.syntax_allow);
    let parent_net = std::fs::read_link("/proc/self/ns/net").unwrap().display().to_string();
    assert!(boundary.detail.contains(&format!("netns={parent_net}")), "netns {}", boundary.detail);
    assert!(boundary.detail.contains("home=unset"), "{}", boundary.detail);
    assert!(boundary.detail.contains("path=/usr/bin:/bin"), "{}", boundary.detail);
    assert!(boundary.detail.contains("passwd=yes"), "env clear is not a credential boundary: {}", boundary.detail);
    assert!(boundary.detail.contains("socket=yes"), "socket create failed: {}", boundary.detail);
    assert!(boundary.detail.contains("fds=0,1,2"), "inherited fds {}", boundary.detail);
    assert!(boundary.detail.contains("nnp=1"), "{}", boundary.detail);
    drop(held);
}

#[test]
fn substituted_paths_are_not_silently_trusted() {
    let _gate = lock();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let policy = Policy::parse(&std::fs::read_to_string(root.join("fixtures/policy.json")).unwrap()).unwrap();
    let worker = PathBuf::from(env!("CARGO_BIN_EXE_oo-p05-parser-worker"));
    let pin = sha256_file(&worker).unwrap();
    let dir = std::env::temp_dir().join(format!("oo-p05-id-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("real")).unwrap();
    let direct = dir.join("direct");
    std::os::unix::fs::symlink(&worker, &direct).unwrap();
    std::os::unix::fs::symlink(dir.join("real"), dir.join("jump")).unwrap();
    std::os::unix::fs::symlink(&worker, dir.join("jump").join("worker")).unwrap();

    let linked = admit_in_worker(&direct, &pin, "SELECT 1", &policy);
    assert_eq!(linked.code, "UNTRUSTED_WORKER");
    assert!(!linked.syntax_allow);
    assert!(linked.executed_sql.is_empty());
    assert!(!linked.reaped);

    let ancestor = admit_in_worker(&dir.join("jump").join("worker"), &pin, "SELECT 1", &policy);
    assert_eq!(ancestor.code, "UNTRUSTED_WORKER");
    assert!(!ancestor.syntax_allow);

    let relative = admit_in_worker(Path::new("oo-p05-parser-worker"), &pin, "SELECT 1", &policy);
    assert_eq!(relative.code, "UNTRUSTED_WORKER");

    let mut bad = pin;
    bad[0] ^= 0xff;
    let mismatch = admit_in_worker(&worker, &bad, "SELECT 1", &policy);
    assert_eq!(mismatch.code, "UNTRUSTED_WORKER");
    assert!(!mismatch.syntax_allow);
    assert!(mismatch.executed_sql.is_empty());

    let missing = admit_in_worker(Path::new("/tmp/oo-p05-no-such-worker"), &pin, "SELECT 1", &policy);
    assert_eq!(missing.code, "SPAWN_FAILED");
    assert!(!missing.reaped);

    let directory = admit_in_worker(&dir, &pin, "SELECT 1", &policy);
    assert_eq!(directory.code, "SPAWN_FAILED");
    let _ = std::fs::remove_dir_all(&dir);
}

fn fault(mode: &str) -> Decision {
    let supervisor = PathBuf::from(env!("CARGO_BIN_EXE_oo-p05-parser-worker"));
    let fault_bin = PathBuf::from(env!("CARGO_BIN_EXE_oo-p05-fault-worker"));
    exercise_untrusted(&supervisor, &fault_bin, mode)
}

fn pids_gone(decision: &Decision) -> bool {
    for line in decision.cleanup_proof.lines() {
        let Some(rest) = line.strip_prefix("PID ") else { continue };
        let mut parts = rest.split_whitespace();
        let Some(pid) = parts.next().and_then(|text| text.parse::<u32>().ok()) else { return false };
        let Some(start) = parts.next() else { return false };
        if same_start(pid, start) {
            return false;
        }
    }
    true
}

fn proof_has_setsid(proof: &str) -> bool {
    proof_lines(proof).into_iter().any(|(pid, pgid, sid)| pid == pgid && pid == sid)
}

fn proof_has_samegroup(proof: &str) -> bool {
    proof_lines(proof).into_iter().any(|(pid, pgid, sid)| pid != pgid && pid != sid)
}

fn proof_lines(proof: &str) -> Vec<(u32, u32, u32)> {
    let mut out = Vec::new();
    for line in proof.lines() {
        let Some(rest) = line.strip_prefix("PID ") else { continue };
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() < 5 {
            continue;
        }
        if let (Ok(pid), Ok(pgid), Ok(sid)) = (parts[0].parse(), parts[3].parse(), parts[4].parse()) {
            out.push((pid, pgid, sid));
        }
    }
    out
}

fn starttime(pid: u32) -> Option<String> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = text.rsplit_once(')')?.1;
    rest.split_whitespace().nth(19).map(str::to_string)
}

fn same_start(pid: u32, start: &str) -> bool {
    match starttime(pid) {
        Some(current) => current == start,
        None => false,
    }
}
