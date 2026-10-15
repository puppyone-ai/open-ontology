#![forbid(unsafe_code)]

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use serde_json::Value;

#[test]
#[ignore = "requires OO_JW_LIVE=1 and the private control database"]
fn closed_loop() {
    if std::env::var("OO_JW_LIVE").ok().as_deref() != Some("1") {
        panic!("OO_JW_LIVE must be 1");
    }
    let config = std::env::var("OO_JW_CONFIG").expect("OO_JW_CONFIG");
    let evidence = PathBuf::from(std::env::var("OO_JW_EVIDENCE").expect("OO_JW_EVIDENCE"));
    let live = evidence.join("live");
    fs::create_dir_all(&live).expect("live dir");
    prepare_cohort(&config, &live);
    let serve = first_wave(&config, &live);
    let again = second_wave(&config, &live, serve);
    assert_report(&config, &live, again);
}

fn prepare_cohort(config: &str, live: &Path) {
    step(config, live, "reset", &["reset"]);
    step(config, live, "apply", &["apply"]);
    step(config, live, "prepare", &["prepare"]);
}

fn first_wave(config: &str, live: &Path) -> Serve {
    let mut serve = spawn_serve(config, live, "serve1", SERVE_ARGS);
    step(
        config,
        live,
        "bulk",
        &[
            "wait",
            "--kind",
            "observation",
            "--name",
            "bulk_running",
            "--value",
            "100",
            "--seconds",
            "90",
        ],
    );
    alive(&mut serve, "after bulk");
    step(
        config,
        live,
        "checkpoint applied",
        &[
            "wait", "--kind", "applied", "--dedupe", "checkpoint", "--applied", "2", "--seconds",
            "180",
        ],
    );
    step(
        config,
        live,
        "beat",
        &["wait", "--kind", "beat", "--dedupe", "checkpoint", "--seconds", "60"],
    );
    wait_terminals(config, live);
    step(
        config,
        live,
        "hold running",
        &[
            "wait", "--kind", "state", "--dedupe", "hold-cancel", "--state", "running",
            "--seconds", "30",
        ],
    );
    alive(&mut serve, "before cancel");
    step(config, live, "show hold", &["show-lease", "--dedupe", "hold-cancel"]);
    step(config, live, "cancel hold", &["cancel", "--dedupe", "hold-cancel"]);
    step(
        config,
        live,
        "hold cancelled",
        &[
            "wait", "--kind", "state", "--dedupe", "hold-cancel", "--state", "cancelled",
            "--seconds", "30",
        ],
    );
    step(
        config,
        live,
        "show checkpoint",
        &["show-lease", "--dedupe", "checkpoint"],
    );
    alive(&mut serve, "before kill");
    serve
}

fn wait_terminals(config: &str, live: &Path) {
    for (dedupe, state) in [
        ("retry-once", "succeeded"),
        ("dead-letter", "failed"),
        ("terminal-fail", "failed"),
        ("bad-payload", "failed"),
        ("notify-loss", "succeeded"),
    ] {
        step(
            config,
            live,
            dedupe,
            &[
                "wait", "--kind", "state", "--dedupe", dedupe, "--state", state, "--seconds", "60",
            ],
        );
    }
    step(
        config,
        live,
        "schedule",
        &["wait", "--kind", "schedule", "--state", "succeeded", "--seconds", "60"],
    );
}

fn second_wave(config: &str, live: &Path, mut serve: Serve) -> Serve {
    serve.child.kill().expect("kill");
    finish_serve(serve, live, "serve1.exit", false);
    step(
        config,
        live,
        "expired",
        &["wait", "--kind", "expired", "--dedupe", "checkpoint", "--seconds", "50"],
    );
    step(
        config,
        live,
        "fence before",
        &["fence", "--phase", "before", "--dedupe", "checkpoint"],
    );
    let mut again = spawn_serve(config, live, "serve2", SERVE_ARGS);
    step(
        config,
        live,
        "checkpoint succeeded",
        &[
            "wait", "--kind", "state", "--dedupe", "checkpoint", "--state", "succeeded",
            "--seconds", "60",
        ],
    );
    alive(&mut again, "before fence after");
    step(
        config,
        live,
        "fence after",
        &["fence", "--phase", "after", "--dedupe", "checkpoint"],
    );
    again
}

fn assert_report(config: &str, live: &Path, again: Serve) {
    let pid = again.child.id().to_string();
    let term = Command::new("kill").args(["-TERM", &pid]).status().expect("term");
    assert!(term.success(), "kill -TERM");
    finish_serve(again, live, "serve2.exit", true);
    let report = run(config, &["report"]);
    must(&report, "report", live);
    fs::write(live.join("report.json"), &report.stdout).expect("report file");
    let value: Value = serde_json::from_slice(&report.stdout).expect("json");
    expect_counts(&value);
    expect_texts(&value);
    expect_clock(&value);
}

fn expect_counts(value: &Value) {
    for (key, expected) in [
        ("bulk_running_peak", 100),
        ("bulk_succeeded", 100),
        ("bulk_effects", 100),
        ("bulk_owners", 100),
        ("replayed", 1),
        ("rollback_tasks", 0),
        ("rollback_intents", 0),
        ("notify_left_queued", 1),
        ("notify_loss_effects", 1),
        ("schedule_occurrences", 1),
        ("schedule_tasks", 1),
        ("schedule_effects", 1),
        ("retry_effects", 1),
        ("retry_attempt", 2),
        ("dead_effects", 0),
        ("dead_letters", 1),
        ("terminal_effects", 0),
        ("bad_effects", 0),
        ("queued_cancel_effects", 0),
        ("running_cancel_effects", 0),
        ("cancel_follow_terminal", 1),
        ("cancel_follow_succeeded", 0),
        ("checkpoint_effects", 4),
        ("checkpoint_generations", 2),
        ("fence_before_inserted", 0),
        ("fence_after_inserted", 0),
        ("effect_rows", 107),
        ("target_fence_true", 0),
        ("duplicate_succeeded_events", 0),
    ] {
        assert_num(value, key, expected);
    }
}

fn expect_texts(value: &Value) {
    for (key, expected) in [
        ("notify_loss_state", "succeeded"),
        ("retry_state", "succeeded"),
        ("dead_state", "failed"),
        ("dead_error", "attempt_exhausted"),
        ("terminal_state", "failed"),
        ("terminal_error", "worker_failed"),
        ("bad_state", "failed"),
        ("bad_error", "payload_rejected"),
        ("queued_cancel_state", "cancelled"),
        ("running_cancel_state", "cancelled"),
        ("checkpoint_state", "succeeded"),
        ("fence_before_complete", "lost_lease"),
        ("fence_before_heartbeat", "lost_lease"),
        ("fence_after_complete", "terminal"),
        ("fence_after_heartbeat", "terminal"),
    ] {
        assert_text(value, key, expected);
    }
}

fn expect_clock(value: &Value) {
    let span = num(value, "lease_span_seconds");
    assert!((25..=30).contains(&span), "lease span {span}");
    let gap = num(value, "beat_gap_seconds");
    assert!(gap >= 10, "beat gap {gap}");
    let remain = num(value, "beat_lease_seconds");
    assert!((25..=30).contains(&remain), "beat lease {remain}");
    assert_eq!(num(value, "effect_rows"), num(value, "effect_txids"));
    assert!(num(value, "xact_commit_delta") >= 107);
    assert_eq!(value["checksums_match"], Value::Bool(true));
}

const SERVE_ARGS: &[&str] = &[
    "serve",
    "--workers",
    "100",
    "--barrier-prefix",
    "bulk-",
    "--barrier-target",
    "100",
];

struct Serve {
    child: std::process::Child,
    out_thread: thread::JoinHandle<()>,
    err_thread: thread::JoinHandle<()>,
}

fn spawn_serve(config: &str, live: &Path, name: &str, args: &[&str]) -> Serve {
    let mut child = Command::new(env!("CARGO_BIN_EXE_job-worker"))
        .arg("--config")
        .arg(config)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn serve");
    let mut stdout = child.stdout.take().expect("stdout");
    let mut stderr = child.stderr.take().expect("stderr");
    let out_path = live.join(format!("{name}.stdout"));
    let err_path = live.join(format!("{name}.stderr"));
    let out_thread = thread::spawn(move || {
        let mut file = fs::File::create(out_path).expect("stdout file");
        let mut buf = [0_u8; 8192];
        while let Ok(n) = stdout.read(&mut buf) {
            if n == 0 {
                break;
            }
            let _ = file.write_all(&buf[..n]);
        }
    });
    let err_thread = thread::spawn(move || {
        let mut file = fs::File::create(err_path).expect("stderr file");
        let mut buf = [0_u8; 8192];
        while let Ok(n) = stderr.read(&mut buf) {
            if n == 0 {
                break;
            }
            let _ = file.write_all(&buf[..n]);
        }
    });
    thread::sleep(Duration::from_millis(200));
    Serve {
        child,
        out_thread,
        err_thread,
    }
}

fn step(config: &str, live: &Path, name: &str, args: &[&str]) {
    must(&run(config, args), name, live);
}

fn finish_serve(mut serve: Serve, live: &Path, exit_name: &str, expect_success: bool) {
    let status = serve.child.wait().expect("wait serve");
    serve.out_thread.join().expect("stdout");
    serve.err_thread.join().expect("stderr");
    fs::write(live.join(exit_name), format!("{status:?}\n")).expect("exit");
    if expect_success {
        assert!(status.success(), "{exit_name} {status:?}");
    } else {
        assert!(!status.success(), "{exit_name} {status:?}");
    }
}

fn alive(serve: &mut Serve, step: &str) {
    if let Some(status) = serve.child.try_wait().expect("try_wait") {
        panic!("{step}: serve exited early {status:?}");
    }
}

fn run(config: &str, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_job-worker"))
        .arg("--config")
        .arg(config)
        .args(args)
        .output()
        .expect("command")
}

fn must(output: &std::process::Output, step: &str, live: &Path) {
    let _ = fs::write(
        live.join(format!("{step}.stdout").replace(' ', "_")),
        &output.stdout,
    );
    let _ = fs::write(
        live.join(format!("{step}.stderr").replace(' ', "_")),
        &output.stderr,
    );
    let _ = fs::write(
        live.join(format!("{step}.exit").replace(' ', "_")),
        format!("{:?}\n", output.status),
    );
    if !output.status.success() {
        panic!(
            "{step} failed {:?}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn num(value: &Value, key: &str) -> i64 {
    value[key]
        .as_i64()
        .unwrap_or_else(|| panic!("{key} is {}", value[key]))
}

fn assert_num(value: &Value, key: &str, expected: i64) {
    let actual = num(value, key);
    assert_eq!(actual, expected, "{key}");
}

fn assert_text(value: &Value, key: &str, expected: &str) {
    let actual = value[key].as_str().unwrap_or_else(|| panic!("{key}"));
    assert_eq!(actual, expected, "{key}");
}

#[test]
fn c2_cancel_reuses_c4_fence_and_leaves_c1_c3_alone() {
    let src = include_str!("../src/native_schedule.rs");
    let release = src
        .split_once("async fn release_terminal_native")
        .and_then(|(_, rest)| rest.split_once("pub async fn cancel_native"))
        .map(|(body, _)| body)
        .expect("release");
    let cancel = src
        .split_once("pub async fn cancel_native")
        .and_then(|(_, rest)| rest.split_once("pub async fn install"))
        .map(|(body, _)| body)
        .expect("cancel");
    assert!(release.contains("b.method IN ('C1','C2','C4','C5')"));
    assert!(release.contains("t.state IN ('failed','cancelled')"));
    assert!(release.contains("NOT EXISTS(SELECT 1 FROM jw_native.pending_acks a WHERE a.task_id=t.id)"));
    assert!(release.contains("SET active_task_id=NULL"));
    assert!(release.contains("session::admin_db"));
    assert!(release.contains("EXISTS(SELECT 1 FROM _oo.full_import_run WHERE source_id=$1 AND source_generation>$2)"));
    assert!(!release.contains("'C3'"));
    assert!(cancel.contains("(b.method IN ('C2','C4') AND t.state IN ('queued','running') OR b.method='C5' AND t.state='queued')"));
    assert!(cancel.contains("t.generation=$3"));
    assert!(cancel.contains("NOT EXISTS(SELECT 1 FROM jw_native.pending_acks a WHERE a.task_id=t.id)"));
    assert!(cancel.contains("session::admin_db"));
    assert!(cancel.contains("EXISTS(SELECT 1 FROM _oo.full_import_run WHERE source_id=$1 AND source_generation>$2)"));
    assert!(cancel.contains("if applied != base || sealed"));
    assert!(cancel.contains("lease::cancel"));
    assert!(!cancel.contains("'C1'") && !cancel.contains("'C3'"));
}
