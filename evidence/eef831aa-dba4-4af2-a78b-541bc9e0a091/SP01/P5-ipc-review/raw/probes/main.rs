//! Independent a091 IPC probe. It links the archived spike and does not modify it.
//! Dangerous SQL is parsed in-process or returned as worker output. Nothing here
//! opens a database connection.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use oo_p05_pg17_parser::{
    admit_in_worker, admit_native, exercise_untrusted, exercise_worker, sha256_file, Decision, Policy, KIND_ADMIT,
};
use serde_json::{json, Value};

static SLEEP_PID: AtomicU32 = AtomicU32::new(0);
static FAILS: AtomicU32 = AtomicU32::new(0);

fn main() {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--prctl-deny") {
        let supervisor = args.next().expect("supervisor");
        let fault = args.next().expect("fault");
        prctl_deny(&supervisor, &fault);
        return;
    }
    let _ = fs::create_dir_all("/tmp/oo-a091-probe");
    let policy = Policy::parse(&fs::read_to_string("/src/fixtures/policy.json").expect("policy")).expect("policy parse");
    let hooks = PathBuf::from("/target/debug/oo-p05-parser-worker");
    let official = PathBuf::from("/probe/bin/worker-default");
    let flex = PathBuf::from("/probe/bin/evil-flex");
    let official_pin = sha256_file(&official).expect("official pin");
    let official_len = fs::metadata(&official).map(|m| m.len()).unwrap_or(0);
    println!("{}", json!({"case":"images","official_bytes":official_len,"hooks_bytes":fs::metadata(&hooks).map(|m| m.len()).unwrap_or(0)}));

    let mut sleep = Command::new("/bin/sleep")
        .arg("180")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("sleep");
    let sleep_pid = sleep.id();
    SLEEP_PID.store(sleep_pid, Ordering::SeqCst);
    let sleep_start = starttime(sleep_pid).expect("sleep start");
    println!("{}", json!({"case":"foreign-sleep-start","pid":sleep_pid,"starttime":sleep_start}));

    let native_ok = admit_native("SELECT 1", &policy);
    emit_decision("native-select-1", &native_ok, 0, json!({}), native_ok.syntax_allow && native_ok.code == "ALLOW_SYNTAX");

    let pinned_bin = official.clone();
    let pinned_policy = policy.clone();
    let pinned_hash = official_pin;
    let pinned = timed(12_000, move || admit_in_worker(&pinned_bin, &pinned_hash, "SELECT 1", &pinned_policy));
    match pinned {
        Ok((wall, d)) => {
            let clocks_ok = d.syntax_allow && d.code == "ALLOW_SYNTAX" && d.executed_sql.contains("SELECT") && !d.oid_acl_rls_proven && d.reaped && d.worker_identity == "pinned";
            emit_decision("pinned-select-1", &d, wall, json!({"native_code": native_ok.code}), clocks_ok && d.executed_sql == native_ok.executed_sql);
        }
        Err(err) => hang("pinned-select-1", &err),
    }

    let native_drop = admit_native("DROP TABLE analytics.orders", &policy);
    emit_decision(
        "native-drop-not-sent",
        &native_drop,
        0,
        json!({"sent_to_database": false}),
        !native_drop.syntax_allow && native_drop.executed_sql.is_empty(),
    );
    for (name, sql) in [
        ("open-set-config", "SELECT set_config('oo.claims', '{\"region\":[\"us\"]}', true)"),
        ("open-pg-read-file", "SELECT pg_read_file('/etc/passwd', 0, 10)"),
        ("open-where-true", "SELECT * FROM analytics.big_fact b WHERE true"),
        ("open-subquery-star", "SELECT * FROM (SELECT b.id, b.ts FROM analytics.big_fact b) s"),
        ("open-bare-star", "SELECT * FROM analytics.big_fact"),
    ] {
        let d = admit_native(sql, &policy);
        emit_decision(name, &d, 0, json!({"open_scope": true, "sent_to_database": false}), true);
    }

    let body = serde_json::to_vec(&json!({"sql": "SELECT 1", "policy": &policy})).unwrap();
    match timed(12_000, {
        let hooks = hooks.clone();
        let body = body.clone();
        move || exercise_worker(&hooks, KIND_ADMIT, &body)
    }) {
        Ok((wall, d)) => emit_decision(
            "self-hash-hook-not-allow",
            &d,
            wall,
            json!({}),
            !d.syntax_allow && d.executed_sql.is_empty() && d.code == "UNTRUSTED_WORKER" && d.worker_identity == "test-hooks-kind",
        ),
        Err(err) => hang("self-hash-hook-not-allow", &err),
    }

    admit_fixed("contradict", &policy, 8_000, |d| {
        d.code == "INVALID_OUTPUT" && !d.syntax_allow && d.executed_sql.is_empty() && !d.executed_sql.to_ascii_lowercase().contains("drop") && d.code != "PARSER_MISMATCH" && d.worker_identity == "pinned" && !d.oid_acl_rls_proven
    });
    admit_fixed("contradict-ver", &policy, 8_000, |d| {
        d.code == "INVALID_OUTPUT" && d.code != "PARSER_MISMATCH" && !d.syntax_allow && d.executed_sql.is_empty()
    });
    admit_fixed("garbage", &policy, 8_000, |d| d.code == "INVALID_OUTPUT" && !d.syntax_allow && d.code != native_ok.code);
    admit_fixed("oid-true", &policy, 8_000, |d| d.code == "INVALID_OUTPUT" && !d.syntax_allow && !d.oid_acl_rls_proven && d.executed_sql.is_empty());
    admit_fixed("unknown-field", &policy, 8_000, |d| d.code == "INVALID_OUTPUT" && !d.syntax_allow && d.executed_sql.is_empty());
    admit_fixed("unknown-code", &policy, 8_000, |d| d.code == "INVALID_OUTPUT" && !d.syntax_allow && d.executed_sql.is_empty());
    admit_fixed("deny-sql", &policy, 8_000, |d| d.code == "INVALID_OUTPUT" && !d.syntax_allow && d.executed_sql.is_empty());
    admit_fixed("mismatch-bool", &policy, 8_000, |d| d.code == "INVALID_OUTPUT" && !d.syntax_allow && d.executed_sql.is_empty());
    admit_fixed("false-allow-empty", &policy, 8_000, |d| d.code == "INVALID_OUTPUT" && !d.syntax_allow && d.executed_sql.is_empty());
    admit_fixed("version-server", &policy, 8_000, |d| d.code == "PARSER_MISMATCH" && !d.syntax_allow && d.executed_sql.is_empty());
    admit_fixed("oversize-sql", &policy, 8_000, |d| d.code == "INVALID_OUTPUT" && !d.syntax_allow && d.executed_sql.is_empty());
    admit_fixed("extra", &policy, 8_000, |d| d.code == "INVALID_OUTPUT" && !d.syntax_allow && d.executed_sql.is_empty());
    admit_fixed("allow-drop", &policy, 8_000, |d| d.code != "SPAWN_FAILED" && d.code != "UNTRUSTED_WORKER");
    admit_fixed("extra-delay", &policy, 8_000, |d| d.code != "SPAWN_FAILED" && d.code != "UNTRUSTED_WORKER");

    flex_case("bad-magic", &hooks, &flex, 12_000, |d, wall| {
        d.code == "INVALID_OUTPUT" && !d.syntax_allow && d.ready_wait_ms < 2_000 && d.cleanup_wait_ms < 700 && d.ready_wait_ms + d.parse_wait_ms + d.cleanup_wait_ms < 700 && wall < 12_000 && proof_alive(&d.cleanup_proof) == 0
    });
    flex_case("stdout-junk", &hooks, &flex, 12_000, |d, wall| d.code == "INVALID_OUTPUT" && !d.syntax_allow && d.cleanup_wait_ms < 700 && wall < 12_000 && proof_alive(&d.cleanup_proof) == 0);
    flex_case("oversize-frame", &hooks, &flex, 12_000, |d, wall| d.code == "RESPONSE_CAP" && !d.syntax_allow && d.cleanup_wait_ms < 700 && wall < 12_000 && proof_alive(&d.cleanup_proof) == 0);
    flex_case("malformed-json", &hooks, &flex, 12_000, |d, _| d.code == "INVALID_OUTPUT" && !d.syntax_allow && proof_alive(&d.cleanup_proof) == 0);
    flex_case("early", &hooks, &flex, 12_000, |d, _| d.code == "SPAWN_FAILED" && !d.syntax_allow && d.ready_wait_ms < 2_000 && proof_alive(&d.cleanup_proof) == 0);
    flex_case("partial", &hooks, &flex, 12_000, |d, wall| {
        d.code == "DEADLINE" && !d.syntax_allow && d.parse_wait_ms >= 450 && d.parse_wait_ms <= 800 && d.ready_wait_ms < 2_000 && d.cleanup_wait_ms < 700 && wall < 12_000 && proof_alive(&d.cleanup_proof) == 0
    });
    flex_case("stderr-flood", &hooks, &flex, 12_000, |d, wall| {
        !d.syntax_allow && d.executed_sql.is_empty() && d.code != "ALLOW_SYNTAX" && wall < 12_000 && proof_alive(&d.cleanup_proof) == 0
    });

    let stop = Arc::new(AtomicBool::new(false));
    let exes = Arc::new(Mutex::new(Vec::<String>::new()));
    let scan_stop = Arc::clone(&stop);
    let scan_exes = Arc::clone(&exes);
    let scanner = std::thread::spawn(move || {
        let parent = std::process::id();
        while !scan_stop.load(Ordering::SeqCst) {
            collect_exes(parent, &scan_exes);
            std::thread::sleep(Duration::from_millis(40));
        }
    });
    flex_case("silent", &hooks, &flex, 16_000, |d, wall| {
        d.code == "SPAWN_FAILED" && !d.syntax_allow && d.ready_wait_ms >= 4_900 && d.ready_wait_ms < 5_600 && d.parse_wait_ms < 50 && d.cleanup_wait_ms < 700 && wall < 16_000 && proof_alive(&d.cleanup_proof) == 0 && d.reaped
    });
    stop.store(true, Ordering::SeqCst);
    let _ = scanner.join();
    let seen = exes.lock().map(|g| g.clone()).unwrap_or_default();
    println!("{}", json!({"case":"memfd-exe","exes":seen,"has_memfd":seen.iter().any(|item| item.contains("memfd"))}));

    flex_case("hold-setsid", &hooks, &flex, 12_000, |d, wall| {
        !d.syntax_allow && d.cleanup_wait_ms < 700 && wall < 12_000 && proof_alive(&d.cleanup_proof) == 0 && recorded_alive("/tmp/oo-a091-probe/hold-pids") == 0 && d.reaped && proof_has_setsid(&d.cleanup_proof)
    });
    check_sleep("after-hold-setsid", sleep_pid, &sleep_start);
    clear_recorded("/tmp/oo-a091-probe/hold-pids");
    flex_case("hold-same", &hooks, &flex, 12_000, |d, wall| {
        !d.syntax_allow && d.cleanup_wait_ms < 700 && wall < 12_000 && proof_alive(&d.cleanup_proof) == 0 && recorded_alive("/tmp/oo-a091-probe/hold-pids") == 0 && d.reaped && proof_has_samegroup(&d.cleanup_proof)
    });
    check_sleep("after-hold-same", sleep_pid, &sleep_start);
    clear_recorded("/tmp/oo-a091-probe/hold-pids");

    admit_fixed("success-hold", &policy, 12_000, |d| {
        d.cleanup_wait_ms < 700 && proof_alive(&d.cleanup_proof) == 0 && recorded_alive("/tmp/oo-a091-probe/hold-pids") == 0 && d.reaped && proof_has_setsid(&d.cleanup_proof)
    });
    check_sleep("after-success-hold", sleep_pid, &sleep_start);
    clear_recorded("/tmp/oo-a091-probe/hold-pids");
    admit_fixed("success-same", &policy, 12_000, |d| {
        d.cleanup_wait_ms < 700 && proof_alive(&d.cleanup_proof) == 0 && recorded_alive("/tmp/oo-a091-probe/hold-pids") == 0 && d.reaped && proof_has_samegroup(&d.cleanup_proof)
    });
    clear_recorded("/tmp/oo-a091-probe/hold-pids");

    let _ = fs::remove_file("/tmp/oo-a091-probe/race-pids");
    flex_case("fork-race", &hooks, &flex, 12_000, |d, _| {
        !d.syntax_allow && d.executed_sql.is_empty() && !(d.reaped && (proof_alive(&d.cleanup_proof) > 0 || recorded_alive("/tmp/oo-a091-probe/race-pids") > 0))
    });
    println!("{}", json!({"case":"fork-race-alive","proof_alive_checked":true,"recorded_alive":recorded_alive("/tmp/oo-a091-probe/race-pids")}));
    clear_recorded("/tmp/oo-a091-probe/race-pids");
    check_sleep("after-fork-race", sleep_pid, &sleep_start);

    let _ = fs::remove_file("/tmp/oo-a091-probe/race-pids");
    flex_case("double-fork", &hooks, &flex, 12_000, |d, _| {
        !d.syntax_allow && d.executed_sql.is_empty() && !(d.reaped && (proof_alive(&d.cleanup_proof) > 0 || recorded_alive("/tmp/oo-a091-probe/race-pids") > 0))
    });
    println!("{}", json!({"case":"double-fork-alive","recorded_alive":recorded_alive("/tmp/oo-a091-probe/race-pids")}));
    clear_recorded("/tmp/oo-a091-probe/race-pids");

    flex_case("fsize", &hooks, &flex, 12_000, |d, _| {
        let wrote = wrote_of(&d.detail);
        !d.syntax_allow && d.executed_sql.is_empty() && wrote <= 256 * 1024
    });
    flex_case("dirbudget", &hooks, &flex, 12_000, |d, _| d.code == "RESOURCE_BUDGET" && !d.reaped && !d.syntax_allow && d.executed_sql.is_empty());
    flex_case("many-empty", &hooks, &flex, 12_000, |d, _| !d.syntax_allow && d.executed_sql.is_empty());
    let _ = fs::remove_file("/tmp/oo-a091-probe/outside.bin");
    flex_case("outside", &hooks, &flex, 12_000, |d, _| !d.syntax_allow && d.executed_sql.is_empty());
    let outside_alive = Path::new("/tmp/oo-a091-probe/outside.bin").exists();
    println!("{}", json!({"case":"outside-file","exists_after_return":outside_alive}));
    let _ = fs::remove_file("/tmp/oo-a091-probe/outside.bin");
    let parent_net = fs::read_link("/proc/self/ns/net").map(|p| p.display().to_string()).unwrap_or_default();
    flex_case("boundary", &hooks, &flex, 12_000, |d, _| !d.syntax_allow && d.executed_sql.is_empty() && d.detail.contains(&format!("netns={parent_net}")));

    identity_cases(&official, &official_pin, &policy);
    crash_supervisor(&hooks, &flex, sleep_pid, &sleep_start);
    run_prctl_child(&hooks, &flex);
    check_sleep("final", sleep_pid, &sleep_start);
    if same_start(sleep_pid, &sleep_start) {
        unsafe { libc::kill(sleep_pid as i32, libc::SIGKILL) };
    }
    let _ = sleep.wait();
    let fails = FAILS.load(Ordering::SeqCst);
    println!("{}", json!({"case":"summary","fails":fails,"sent_to_database":false}));
    std::process::exit(if fails == 0 { 0 } else { 1 });
}

fn prctl_deny(supervisor: &str, fault: &str) {
    let installed = install_prctl_filter();
    if !installed {
        println!("{}", json!({"case":"prctl-unavailable","branch":"not-executed","install":false,"pass":true}));
        return;
    }
    let started = Instant::now();
    let d = exercise_untrusted(Path::new(supervisor), Path::new(fault), "bad-magic");
    let wall = started.elapsed().as_millis();
    let executed = d.code == "CONTAINMENT_UNAVAILABLE";
    let pass = executed && !d.reaped && !d.syntax_allow && d.executed_sql.is_empty() && d.containment == "unavailable";
    if !pass {
        FAILS.fetch_add(1, Ordering::SeqCst);
    }
    println!("{}", json!({"case":"prctl-unavailable","branch": if executed {"executed"} else {"not-hit"}, "install":true, "pass":pass, "wall_ms":wall, "decision": view(&d)}));
}

fn install_prctl_filter() -> bool {
    #[repr(C)]
    struct SockFilter {
        code: u16,
        jt: u8,
        jf: u8,
        k: u32,
    }
    #[repr(C)]
    struct SockFprog {
        len: u16,
        filter: *const SockFilter,
    }
    const BPF_LD: u16 = 0x00;
    const BPF_W: u16 = 0x00;
    const BPF_ABS: u16 = 0x20;
    const BPF_JMP: u16 = 0x05;
    const BPF_JEQ: u16 = 0x10;
    const BPF_K: u16 = 0x00;
    const BPF_RET: u16 = 0x06;
    const SECCOMP_SET_MODE_FILTER: i64 = 1;
    const SECCOMP_RET_ALLOW: u32 = 0x7fff0000;
    const SECCOMP_RET_ERRNO: u32 = 0x00050000;
    let prctl_nr = libc::SYS_prctl as u32;
    // Deny only PR_SET_CHILD_SUBREAPER (36). Other prctl calls stay allowed.
    let filter = [
        SockFilter { code: BPF_LD | BPF_W | BPF_ABS, jt: 0, jf: 0, k: 0 },
        SockFilter { code: BPF_JMP | BPF_JEQ | BPF_K, jt: 0, jf: 3, k: prctl_nr },
        SockFilter { code: BPF_LD | BPF_W | BPF_ABS, jt: 0, jf: 0, k: 16 },
        SockFilter { code: BPF_JMP | BPF_JEQ | BPF_K, jt: 0, jf: 1, k: 36 },
        SockFilter { code: BPF_RET | BPF_K, jt: 0, jf: 0, k: SECCOMP_RET_ERRNO | 1 },
        SockFilter { code: BPF_RET | BPF_K, jt: 0, jf: 0, k: SECCOMP_RET_ALLOW },
    ];
    let prog = SockFprog { len: filter.len() as u16, filter: filter.as_ptr() };
    unsafe {
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
            return false;
        }
        let prog_ptr = &prog as *const SockFprog;
        libc::syscall(277, SECCOMP_SET_MODE_FILTER, 0i64, prog_ptr) == 0
    }
}

fn run_prctl_child(hooks: &Path, flex: &Path) {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("/probe/harness/target/debug/p5-ipc-probe"));
    let output = Command::new(exe)
        .arg("--prctl-deny")
        .arg(hooks)
        .arg(flex)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    match output {
        Ok(output) => {
            let text = String::from_utf8_lossy(&output.stdout);
            println!("{}", text.trim());
            let parsed: Option<Value> = serde_json::from_str(text.trim()).ok();
            let pass = parsed.as_ref().and_then(|value| value.get("pass")).and_then(|value| value.as_bool()).unwrap_or(false);
            if !pass {
                FAILS.fetch_add(1, Ordering::SeqCst);
                let err = String::from_utf8_lossy(&output.stderr);
                let err = if err.len() > 400 { err[..400].to_string() } else { err.to_string() };
                println!("{}", json!({"case":"prctl-parent","pass":false,"status":format!("{:?}", output.status),"stderr":err}));
            }
        }
        Err(err) => {
            FAILS.fetch_add(1, Ordering::SeqCst);
            println!("{}", json!({"case":"prctl-unavailable","pass":false,"error":err.to_string()}));
        }
    }
}

fn crash_supervisor(hooks: &Path, flex: &Path, sleep_pid: u32, sleep_start: &str) {
    let _ = fs::remove_file("/tmp/oo-a091-probe/crash-child");
    let hooks = hooks.to_path_buf();
    let flex = flex.to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let d = exercise_untrusted(&hooks, &flex, "setsid-noready");
        let _ = tx.send(d);
    });
    let parent = std::process::id();
    let mut killed = 0u32;
    let started = Instant::now();
    let mut early: Option<Decision> = None;
    while started.elapsed() < Duration::from_secs(12) {
        if let Ok(decision) = rx.try_recv() {
            early = Some(decision);
            break;
        }
        if Path::new("/tmp/oo-a091-probe/crash-child").exists() {
            if let Some(sup) = direct_leader(parent, sleep_pid) {
                unsafe { libc::kill(sup as i32, libc::SIGKILL) };
                killed = sup;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let result = if let Some(decision) = early { Ok(decision) } else { rx.recv_timeout(Duration::from_secs(8)) };
    let survivor = fs::read_to_string("/tmp/oo-a091-probe/crash-child").unwrap_or_default();
    let mut survivor_alive = false;
    let mut survivor_ppid = 0u32;
    let mut survivor_pid = 0u32;
    if let Some(line) = survivor.lines().next() {
        let mut parts = line.split_whitespace();
        survivor_pid = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let start = parts.next().unwrap_or("");
        survivor_alive = same_start(survivor_pid, start);
        if let Some(desc) = read_desc(survivor_pid) {
            survivor_ppid = desc.0;
        }
    }
    match result {
        Ok(d) => {
            let wall = started.elapsed().as_millis();
            let pass = killed != 0 && !d.reaped && !d.syntax_allow && d.executed_sql.is_empty() && d.code != "ALLOW_SYNTAX";
            emit_decision(
                "supervisor-crash",
                &d,
                wall,
                json!({"killed_supervisor":killed,"survivor_pid":survivor_pid,"survivor_alive":survivor_alive,"survivor_ppid":survivor_ppid}),
                pass,
            );
        }
        Err(_) => hang("supervisor-crash", "timeout"),
    }
    clear_recorded("/tmp/oo-a091-probe/crash-child");
    check_sleep("after-supervisor-crash", sleep_pid, sleep_start);
}

fn identity_cases(official: &Path, pin: &[u8; 32], policy: &Policy) {
    let root = PathBuf::from("/tmp/oo-a091-probe/id");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("real")).unwrap();
    std::os::unix::fs::symlink(official, root.join("direct")).unwrap();
    std::os::unix::fs::symlink(root.join("real"), root.join("jump")).unwrap();
    std::os::unix::fs::symlink(official, root.join("jump").join("worker")).unwrap();
    fs::write(root.join("empty"), b"").unwrap();
    let cases = [
        ("symlink-file", root.join("direct"), *pin, "UNTRUSTED_WORKER"),
        ("symlink-ancestor", root.join("jump").join("worker"), *pin, "UNTRUSTED_WORKER"),
        ("empty-file", root.join("empty"), *pin, "UNTRUSTED_WORKER"),
        ("missing", PathBuf::from("/tmp/oo-a091-probe/no-such-worker"), *pin, "SPAWN_FAILED"),
        ("directory", root.clone(), *pin, "SPAWN_FAILED"),
    ];
    for (name, path, pin, expect) in cases {
        let started = Instant::now();
        let d = admit_in_worker(&path, &pin, "SELECT 1", policy);
        let wall = started.elapsed().as_millis();
        emit_decision(name, &d, wall, json!({}), d.code == expect && !d.syntax_allow && d.executed_sql.is_empty() && !d.reaped && wall < 2_000);
    }
    let started = Instant::now();
    let d = admit_in_worker(Path::new("oo-p05-parser-worker"), pin, "SELECT 1", policy);
    emit_decision("relative-path", &d, started.elapsed().as_millis(), json!({}), d.code == "UNTRUSTED_WORKER" && !d.syntax_allow && !d.reaped);
    let mut bad = *pin;
    bad[0] ^= 0xff;
    let started = Instant::now();
    let d = admit_in_worker(official, &bad, "SELECT 1", policy);
    emit_decision("pin-mismatch", &d, started.elapsed().as_millis(), json!({}), d.code == "UNTRUSTED_WORKER" && !d.syntax_allow && d.executed_sql.is_empty() && !d.reaped);
    let _ = fs::remove_dir_all(&root);
}

fn admit_fixed(mode: &str, policy: &Policy, budget: u64, pred: impl Fn(&Decision) -> bool) {
    let path = PathBuf::from(format!("/probe/bin/evil-{mode}"));
    let Ok(pin) = sha256_file(&path) else {
        hang(mode, "pin");
        return;
    };
    let path2 = path.clone();
    let policy = policy.clone();
    let mode_name = mode.to_string();
    match timed(budget, move || admit_in_worker(&path2, &pin, "SELECT 1", &policy)) {
        Ok((wall, d)) => {
            let pass = pred(&d);
            let observation = matches!(mode, "allow-drop" | "extra-delay");
            emit_decision(&mode_name, &d, wall, json!({"api":"admit_in_worker","observation":observation}), pass);
        }
        Err(err) => hang(&mode_name, &err),
    }
}

fn flex_case(mode: &str, hooks: &Path, flex: &Path, budget: u64, pred: impl Fn(&Decision, u128) -> bool) {
    let hooks = hooks.to_path_buf();
    let flex = flex.to_path_buf();
    let mode_name = mode.to_string();
    match timed(budget, move || exercise_untrusted(&hooks, &flex, &mode_name)) {
        Ok((wall, d)) => {
            let pass = pred(&d, wall);
            emit_decision(mode, &d, wall, json!({"api":"exercise_untrusted"}), pass);
        }
        Err(err) => hang(mode, &err),
    }
}

fn timed(budget_ms: u64, work: impl FnOnce() -> Decision + Send + 'static) -> Result<(u128, Decision), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let started = Instant::now();
        let decision = work();
        let _ = tx.send((started.elapsed().as_millis(), decision));
    });
    match rx.recv_timeout(Duration::from_millis(budget_ms)) {
        Ok(pair) => Ok(pair),
        Err(_) => {
            kill_children(SLEEP_PID.load(Ordering::SeqCst));
            Err("HANG".to_string())
        }
    }
}

fn emit_decision(case: &str, d: &Decision, wall: u128, extra: Value, pass: bool) {
    if !pass {
        FAILS.fetch_add(1, Ordering::SeqCst);
    }
    let remainder = wall.saturating_sub(d.ready_wait_ms as u128 + d.parse_wait_ms as u128 + d.cleanup_wait_ms as u128);
    println!(
        "{}",
        json!({
            "case": case,
            "pass": pass,
            "wall_ms": wall,
            "remainder_ms": remainder,
            "proof_alive": proof_alive(&d.cleanup_proof),
            "extra": extra,
            "decision": view(d),
        })
    );
    let _ = std::io::stdout().flush();
}

fn hang(case: &str, err: &str) {
    FAILS.fetch_add(1, Ordering::SeqCst);
    println!("{}", json!({"case":case,"pass":false,"error":err}));
    let _ = std::io::stdout().flush();
}

fn view(d: &Decision) -> Value {
    let sql = &d.executed_sql;
    json!({
        "syntax_allow": d.syntax_allow,
        "code": d.code,
        "parser_version": d.parser_version,
        "executed_sql": if sql.len() > 160 { format!("len={}", sql.len()) } else { sql.clone() },
        "sql_len": sql.len(),
        "sql_has_drop": sql.to_ascii_lowercase().contains("drop"),
        "oid_acl_rls_proven": d.oid_acl_rls_proven,
        "reaped": d.reaped,
        "workdir_removed": d.workdir_removed,
        "worker_identity": d.worker_identity,
        "containment": d.containment,
        "ready_wait_ms": d.ready_wait_ms,
        "parse_wait_ms": d.parse_wait_ms,
        "cleanup_wait_ms": d.cleanup_wait_ms,
        "detail": if d.detail.len() > 400 { format!("len={}", d.detail.len()) } else { d.detail.clone() },
        "cleanup_proof": d.cleanup_proof,
    })
}

fn wrote_of(detail: &str) -> u64 {
    detail.split("wrote=").nth(1).and_then(|text| text.split(|c: char| !c.is_ascii_digit()).next()).and_then(|text| text.parse().ok()).unwrap_or(u64::MAX)
}

fn proof_alive(proof: &str) -> usize {
    let mut n = 0;
    for line in proof.lines() {
        let Some(rest) = line.strip_prefix("PID ") else { continue };
        let mut parts = rest.split_whitespace();
        let Some(pid) = parts.next().and_then(|text| text.parse::<u32>().ok()) else { continue };
        let Some(start) = parts.next() else { continue };
        if same_start(pid, start) {
            n += 1;
        }
    }
    n
}

fn proof_has_setsid(proof: &str) -> bool {
    proof_rows(proof).into_iter().any(|(pid, pgid, sid)| pid == pgid && pid == sid && pid != 0)
}

fn proof_has_samegroup(proof: &str) -> bool {
    proof_rows(proof).into_iter().any(|(pid, pgid, sid)| pid != pgid && pid != sid)
}

fn proof_rows(proof: &str) -> Vec<(u32, u32, u32)> {
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

fn recorded_alive(path: &str) -> usize {
    let Ok(text) = fs::read_to_string(path) else { return 0 };
    text.lines()
        .filter(|line| {
            let mut parts = line.split_whitespace();
            let pid = parts.next().and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
            let start = parts.next().unwrap_or("");
            pid > 1 && same_start(pid, start)
        })
        .count()
}

fn clear_recorded(path: &str) {
    if let Ok(text) = fs::read_to_string(path) {
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            let pid = parts.next().and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
            let start = parts.next().unwrap_or("");
            if pid > 1 && same_start(pid, start) {
                unsafe { libc::kill(pid as i32, libc::SIGKILL) };
            }
        }
    }
    let _ = fs::remove_file(path);
}

fn check_sleep(label: &str, pid: u32, start: &str) {
    let alive = same_start(pid, start);
    if !alive {
        FAILS.fetch_add(1, Ordering::SeqCst);
    }
    println!("{}", json!({"case":format!("foreign-sleep-{label}"),"pass":alive,"pid":pid}));
}

fn starttime(pid: u32) -> Option<String> {
    read_stat(pid).map(|item| item.1)
}

fn same_start(pid: u32, start: &str) -> bool {
    matches!(starttime(pid), Some(current) if current == start)
}

fn read_desc(pid: u32) -> Option<(u32, String, u32, u32)> {
    read_stat(pid).map(|(ppid, start, pgid, sid)| (ppid, start, pgid, sid))
}

fn read_stat(pid: u32) -> Option<(u32, String, u32, u32)> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = text.rsplit_once(')')?.1;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let ppid = fields.get(1)?.parse().ok()?;
    let pgid = fields.get(2)?.parse().ok()?;
    let sid = fields.get(3)?.parse().ok()?;
    let start = fields.get(19)?.to_string();
    Some((ppid, start, pgid, sid))
}

fn tgid(pid: u32) -> Option<u32> {
    let text = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Tgid:") {
            return rest.trim().parse().ok();
        }
    }
    None
}

fn direct_leader(parent: u32, skip: u32) -> Option<u32> {
    let mut found = None;
    let rd = fs::read_dir("/proc").ok()?;
    for entry in rd.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else { continue };
        if pid == skip || pid == parent {
            continue;
        }
        let Some((ppid, _, _, _)) = read_stat(pid) else { continue };
        if ppid == parent && tgid(pid) == Some(pid) {
            found = Some(pid);
            break;
        }
    }
    found
}

fn kill_children(skip: u32) {
    let parent = std::process::id();
    let Ok(rd) = fs::read_dir("/proc") else { return };
    for entry in rd.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else { continue };
        if pid == skip || pid == parent {
            continue;
        }
        let Some((ppid, _, _, _)) = read_stat(pid) else { continue };
        if ppid == parent && tgid(pid) == Some(pid) {
            unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        }
    }
}

fn collect_exes(parent: u32, into: &Mutex<Vec<String>>) {
    let Ok(rd) = fs::read_dir("/proc") else { return };
    for entry in rd.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else { continue };
        let Some((ppid, _, _, _)) = read_stat(pid) else { continue };
        if ppid != parent && !ancestor_is(pid, parent) {
            continue;
        }
        if let Ok(link) = fs::read_link(format!("/proc/{pid}/exe")) {
            let text = link.display().to_string();
            if let Ok(mut guard) = into.lock() {
                if !guard.contains(&text) {
                    guard.push(text);
                }
            }
        }
    }
}

fn ancestor_is(pid: u32, ancestor: u32) -> bool {
    let mut current = pid;
    for _ in 0..8 {
        let Some((ppid, _, _, _)) = read_stat(current) else { return false };
        if ppid == ancestor {
            return true;
        }
        if ppid <= 1 || ppid == current {
            return false;
        }
        current = ppid;
    }
    false
}
