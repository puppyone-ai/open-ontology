//! Real child process and real Arrow IPC. Not Oracle acceptance.
//!
//! Tests whose names start with `unit_` do not spawn a child. They are not
//! capability acceptance. Kernel network stays pending in every receipt.

use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use source_driver_isolate::duplicate_without_cloexec;
use source_driver_worker::{
    file_digest, fingerprint_hex, CallerRequest, Failure, FailureKind, KernelNetwork,
    ParentConfig, ParentError, ParentPull, DRIVER_MAX_BYTES, KERNEL_NETWORK, PROTOCOL,
};

const SOURCE_REF: &str = "secret://source/oo-dl01w1-04-src";
const TARGET_REF: &str = "secret://target/oo-dl01w1-04-tgt";

fn unique(prefix: &str) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!(
        "{prefix}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

fn exec_dir() -> PathBuf {
    let base = PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").expect("CARGO_TARGET_DIR"));
    assert!(
        base.is_absolute(),
        "CARGO_TARGET_DIR must be absolute so helper executables are not on noexec /tmp"
    );
    let dir = base.join("driver-fixtures");
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_exec(name: &str, body: &str) -> PathBuf {
    let path = exec_dir().join(name);
    fs::write(&path, body).unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).unwrap();
    path
}

fn volume_dir() -> PathBuf {
    let path = std::env::temp_dir().join(unique("oo-dl01w1-04-vol"));
    fs::create_dir(&path).unwrap();
    path
}

fn write_artifact(volume: &Path) -> String {
    let path = volume.join("driver-artifact.bin");
    fs::write(&path, b"oo-dl01w1-04-artifact").unwrap();
    file_digest(&path, DRIVER_MAX_BYTES).unwrap()
}

fn built_bin(name: &str) -> PathBuf {
    let key = format!("CARGO_BIN_EXE_{}", name.replace('-', "_"));
    if let Some(path) = std::env::var_os(&key) {
        let path = PathBuf::from(path);
        assert!(path.is_absolute() && path.is_file(), "cargo bin {name} missing");
        return path;
    }
    let base = PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").expect("CARGO_TARGET_DIR"));
    assert!(base.is_absolute(), "CARGO_TARGET_DIR must be absolute");
    let path = base.join("debug").join(name);
    assert!(
        path.is_file(),
        "missing {name} at {}; cargo did not export {key}",
        path.display()
    );
    path
}

fn child_bin() -> PathBuf {
    built_bin("oo-source-driver-child")
}

fn parent_bin() -> PathBuf {
    built_bin("oo-source-driver-parent")
}

struct Budget {
    batch_rows: u64,
    batch_bytes: u64,
    max_rows: u64,
    max_bytes: u64,
    child_deadline_ms: u64,
}

fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len().saturating_add(2));
    out.push('"');
    for byte in text.bytes() {
        match byte {
            b'\\' | b'"' => {
                out.push('\\');
                out.push(char::from(byte));
            }
            0x00..=0x1f => panic!("control byte in json string"),
            _ => out.push(char::from(byte)),
        }
    }
    out.push('"');
    out
}

fn envelope(volume: &Path, image: &str, driver_digest: &str, budget: Budget, statement: &str) -> Vec<u8> {
    let volume_text = volume.to_str().expect("utf8 volume");
    format!(
        concat!(
            r#"{{"protocol":{protocol},"source_credential_ref":{source},"target_credential_ref":{target},"#,
            r#""image_digest":{image},"driver":{{"name":"customer-provided","volume":{volume},"artifact":"driver-artifact.bin","digest":{digest}}},"#,
            r#""budget":{{"batch_rows":{batch_rows},"batch_bytes":{batch_bytes},"max_rows":{max_rows},"max_bytes":{max_bytes},"deadline_ms":{deadline}}},"#,
            r#""read":{{"kind":"batch_full","statement_ref":{statement}}}}}"#,
        ),
        protocol = json_string(PROTOCOL),
        source = json_string(SOURCE_REF),
        target = json_string(TARGET_REF),
        image = json_string(image),
        volume = json_string(volume_text),
        digest = json_string(driver_digest),
        batch_rows = budget.batch_rows,
        batch_bytes = budget.batch_bytes,
        max_rows = budget.max_rows,
        max_bytes = budget.max_bytes,
        deadline = budget.child_deadline_ms,
        statement = json_string(statement),
    )
    .into_bytes()
}

fn small_envelope(volume: &Path, image: &str, driver_digest: &str, statement: &str, child_ms: u64) -> Vec<u8> {
    envelope(
        volume,
        image,
        driver_digest,
        Budget {
            batch_rows: 1,
            batch_bytes: 65_536,
            max_rows: 100,
            max_bytes: 1_048_576,
            child_deadline_ms: child_ms,
        },
        statement,
    )
}

fn start_pull(worker: &Path, volume: &Path, bytes: &[u8], parent_ms: u64) -> ParentPull {
    let request = CallerRequest::try_from_slice(bytes, volume).unwrap();
    ParentConfig::try_new(worker, parent_ms)
        .unwrap()
        .start(&request)
        .unwrap()
}

fn failure(err: ParentError) -> Failure {
    match err {
        ParentError::Failed(failure) => failure,
        ParentError::Config(token) => panic!("config {token}"),
    }
}

fn assert_reaped(err: ParentError, kind: FailureKind) -> Failure {
    let found = failure(err);
    assert_eq!(found.kind, kind);
    assert!(found.teardown.reaped);
    assert!(!found.teardown.pid_alive);
    found
}

#[test]
fn two_row_arrow_round_trip_receipt_only_after_finish() {
    let volume = volume_dir();
    let driver = write_artifact(&volume);
    let worker = child_bin();
    let image = file_digest(&worker, DRIVER_MAX_BYTES).unwrap();
    let bytes = small_envelope(&volume, &image, &driver, "fixture:two-rows", 10_000);
    let mut pull = start_pull(&worker, &volume, &bytes, 10_000);
    let first = pull.next_batch().unwrap().expect("first batch");
    assert_eq!(first.batch_index, 0);
    assert_eq!(first.rows.len(), 1);
    assert_eq!(first.rows[0].k, 1);
    assert_eq!(first.rows[0].v.as_deref(), Some("alpha"));
    let second = pull.next_batch().unwrap().expect("second batch");
    assert_eq!(second.batch_index, 1);
    assert_eq!(second.rows.len(), 1);
    assert_eq!(second.rows[0].k, 2);
    assert_eq!(second.rows[0].v, None);
    assert!(pull.next_batch().unwrap().is_none());
    let receipt = pull.finish().unwrap();
    assert_eq!(receipt.rows(), 2);
    assert_eq!(receipt.batches(), 2);
    assert!(receipt.arrow_ipc());
    assert_eq!(receipt.schema_fingerprint(), fingerprint_hex().unwrap());
    assert!(!receipt.admits_import());
    assert!(!receipt.published());
    assert!(!receipt.pg_run_complete());
    assert!(!receipt.fallback());
    assert!(!receipt.vendor_library_loaded());
    assert_eq!(receipt.kernel_network(), KernelNetwork::Pending);
}

#[test]
fn wide_backpressure_holds_progress_at_one() {
    let volume = volume_dir();
    let driver = write_artifact(&volume);
    let worker = child_bin();
    let image = file_digest(&worker, DRIVER_MAX_BYTES).unwrap();
    let bytes = envelope(
        &volume,
        &image,
        &driver,
        Budget {
            batch_rows: 1,
            batch_bytes: 16_777_216,
            max_rows: 10,
            max_bytes: 1_073_741_824,
            child_deadline_ms: 120_000,
        },
        "fixture:wide",
    );
    let mut pull = start_pull(&worker, &volume, &bytes, 120_000);
    let batch = pull.next_batch().unwrap().expect("wide batch");
    assert_eq!(batch.rows.len(), 1);
    let value = batch.rows[0].v.as_deref().expect("wide value");
    assert!(value.len() > 1_000_000);
    assert!(value.bytes().all(|byte| byte == b'W'));
    std::thread::sleep(Duration::from_millis(500));
    let progress = fs::read(pull.work_dir().unwrap().join("progress")).unwrap();
    assert_eq!(progress, b"1");
    let cancel_started = std::time::Instant::now();
    let _ = assert_reaped(pull.cancel(), FailureKind::Cancelled);
    assert!(
        cancel_started.elapsed() < Duration::from_secs(5),
        "cancel blocked for {:?}",
        cancel_started.elapsed()
    );
}

#[test]
fn missing_driver_does_not_exec_and_returns_driver_missing() {
    let marker = exec_dir().join(unique("missing-marker"));
    let sentinel = write_exec(
        &unique("missing-sentinel"),
        &format!("#!/bin/sh\n/bin/touch '{}'\nexit 0\n", marker.display()),
    );
    let image = file_digest(&sentinel, DRIVER_MAX_BYTES).unwrap();
    let volume = volume_dir();
    let absent = small_envelope(&volume, &image, &("sha256:".to_owned() + &"a".repeat(64)), "fixture:two-rows", 5_000);
    let request = CallerRequest::try_from_slice(&absent, &volume).unwrap();
    let err = ParentConfig::try_new(&sentinel, 5_000)
        .unwrap()
        .start(&request)
        .unwrap_err();
    assert!(matches!(err, ParentError::Config("DRIVER_MISSING")));
    let real = volume.join("real-bytes");
    fs::write(&real, b"not-the-artifact").unwrap();
    std::os::unix::fs::symlink(&real, volume.join("driver-artifact.bin")).unwrap();
    let linked = small_envelope(&volume, &image, &("sha256:".to_owned() + &"b".repeat(64)), "fixture:two-rows", 5_000);
    let request = CallerRequest::try_from_slice(&linked, &volume).unwrap();
    let err = ParentConfig::try_new(&sentinel, 5_000)
        .unwrap()
        .start(&request)
        .unwrap_err();
    assert!(matches!(err, ParentError::Config("DRIVER_MISSING")));
    assert!(!marker.exists(), "missing-driver path executed the worker");
}

#[test]
fn image_and_driver_digest_mismatch_do_not_spawn() {
    let marker = exec_dir().join(unique("digest-marker"));
    let sentinel = write_exec(
        &unique("digest-sentinel"),
        &format!("#!/bin/sh\n/bin/touch '{}'\nexit 0\n", marker.display()),
    );
    let image = file_digest(&sentinel, DRIVER_MAX_BYTES).unwrap();
    let volume = volume_dir();
    let driver = write_artifact(&volume);
    let wrong_image = "sha256:".to_owned() + &"a".repeat(64);
    assert_ne!(wrong_image, image);
    let bytes = small_envelope(&volume, &wrong_image, &driver, "fixture:two-rows", 5_000);
    let request = CallerRequest::try_from_slice(&bytes, &volume).unwrap();
    let err = ParentConfig::try_new(&sentinel, 5_000)
        .unwrap()
        .start(&request)
        .unwrap_err();
    assert!(matches!(err, ParentError::Config("image_digest")));
    let wrong_driver = "sha256:".to_owned() + &"b".repeat(64);
    assert_ne!(wrong_driver, driver);
    let bytes = small_envelope(&volume, &image, &wrong_driver, "fixture:two-rows", 5_000);
    let request = CallerRequest::try_from_slice(&bytes, &volume).unwrap();
    let err = ParentConfig::try_new(&sentinel, 5_000)
        .unwrap()
        .start(&request)
        .unwrap_err();
    assert!(matches!(err, ParentError::Config("driver_digest")));
    assert!(!marker.exists(), "digest mismatch executed the worker");
}

#[test]
fn parent_timeout_reaps_slow_child() {
    let volume = volume_dir();
    let driver = write_artifact(&volume);
    let worker = child_bin();
    let image = file_digest(&worker, DRIVER_MAX_BYTES).unwrap();
    let bytes = small_envelope(&volume, &image, &driver, "fixture:slow", 30_000);
    let mut pull = start_pull(&worker, &volume, &bytes, 2_000);
    let found = assert_reaped(pull.next_batch().unwrap_err(), FailureKind::TimedOut);
    assert!(found.teardown.elapsed < Duration::from_secs(15));
    assert_eq!(found.teardown.term_signal, Some(9));
}

#[test]
fn child_deadline_token_is_timed_out() {
    let volume = volume_dir();
    let driver = write_artifact(&volume);
    let worker = child_bin();
    let image = file_digest(&worker, DRIVER_MAX_BYTES).unwrap();
    let bytes = small_envelope(&volume, &image, &driver, "fixture:slow", 400);
    let mut pull = start_pull(&worker, &volume, &bytes, 30_000);
    let found = assert_reaped(pull.next_batch().unwrap_err(), FailureKind::TimedOut);
    assert_eq!(found.token, Some("deadline"));
    assert_eq!(found.teardown.exit_code, Some(1));
    assert!(found.teardown.term_signal.is_none());
    assert!(found.teardown.elapsed >= Duration::from_millis(300));
    assert!(found.teardown.elapsed < Duration::from_secs(20));
}

#[test]
fn cancel_reaps_slow_child() {
    let volume = volume_dir();
    let driver = write_artifact(&volume);
    let worker = child_bin();
    let image = file_digest(&worker, DRIVER_MAX_BYTES).unwrap();
    let bytes = small_envelope(&volume, &image, &driver, "fixture:slow", 30_000);
    let mut pull = start_pull(&worker, &volume, &bytes, 10_000);
    let found = assert_reaped(pull.cancel(), FailureKind::Cancelled);
    assert_eq!(found.teardown.term_signal, Some(9));
}

#[test]
fn deadline_bounds_do_not_spawn() {
    let marker = exec_dir().join(unique("deadline-marker"));
    let sentinel = write_exec(
        &unique("deadline-sentinel"),
        &format!("#!/bin/sh\n/bin/touch '{}'\nexit 0\n", marker.display()),
    );
    assert!(matches!(
        ParentConfig::try_new(&sentinel, 0),
        Err(ParentError::Config("deadline"))
    ));
    assert!(matches!(
        ParentConfig::try_new(&sentinel, 120_001),
        Err(ParentError::Config("deadline"))
    ));
    assert!(!marker.exists(), "deadline refusal executed the worker");
}

#[test]
fn crash_is_worker_and_reaped_without_receipt() {
    let volume = volume_dir();
    let driver = write_artifact(&volume);
    let worker = child_bin();
    let image = file_digest(&worker, DRIVER_MAX_BYTES).unwrap();
    let bytes = small_envelope(&volume, &image, &driver, "fixture:crash", 5_000);
    let mut pull = start_pull(&worker, &volume, &bytes, 5_000);
    let found = assert_reaped(pull.next_batch().unwrap_err(), FailureKind::Worker);
    assert_eq!(found.token, Some("crash"));
    assert_eq!(found.teardown.exit_code, Some(2));
}

#[test]
fn malformed_frame_is_not_a_receipt() {
    let volume = volume_dir();
    let driver = write_artifact(&volume);
    let worker = child_bin();
    let image = file_digest(&worker, DRIVER_MAX_BYTES).unwrap();
    let bytes = small_envelope(&volume, &image, &driver, "fixture:malformed", 5_000);
    let mut pull = start_pull(&worker, &volume, &bytes, 5_000);
    let found = failure(pull.next_batch().unwrap_err());
    assert_eq!(found.kind, FailureKind::Malformed);
    assert!(found.token.is_none());
    assert!(found.teardown.reaped);
}

#[test]
fn oracle_statement_is_read_unsupported_without_fallback() {
    let marker = exec_dir().join(unique("fallback-ran"));
    let _fallback = write_exec(
        &unique("oo-source-driver-fallback"),
        &format!("#!/bin/sh\n/bin/touch '{}'\nexit 0\n", marker.display()),
    );
    let volume = volume_dir();
    let driver = write_artifact(&volume);
    let worker = child_bin();
    let image = file_digest(&worker, DRIVER_MAX_BYTES).unwrap();
    let bytes = small_envelope(&volume, &image, &driver, "oracle:select-1", 5_000);
    let mut pull = start_pull(&worker, &volume, &bytes, 5_000);
    let found = assert_reaped(pull.next_batch().unwrap_err(), FailureKind::Worker);
    assert_eq!(found.token, Some("read_unsupported"));
    assert!(!marker.exists(), "read_unsupported executed a fallback binary");
}

#[test]
fn forbidden_agent_db_password_does_not_spawn() {
    let marker = exec_dir().join(unique("forbidden-marker"));
    let sentinel = write_exec(
        &unique("forbidden-sentinel"),
        &format!("#!/bin/sh\n/bin/touch '{}'\nexit 0\n", marker.display()),
    );
    let volume = volume_dir();
    let driver = write_artifact(&volume);
    let image = file_digest(&sentinel, DRIVER_MAX_BYTES).unwrap();
    let clean = small_envelope(&volume, &image, &driver, "fixture:two-rows", 5_000);
    let text = String::from_utf8(clean).unwrap();
    assert!(text.starts_with('{'));
    let bytes = format!(r#"{{"agent_db_password":"withheld",{}"#, &text[1..]).into_bytes();
    let err = CallerRequest::try_from_slice(&bytes, &volume).unwrap_err();
    assert!(matches!(err, ParentError::Config("forbidden_credential")));
    assert!(!marker.exists(), "forbidden credential key executed the worker");
}

#[test]
fn child_stdin_has_source_ref_and_not_target() {
    let capture = exec_dir().join(unique("stdin-capture"));
    let worker = write_exec(
        &unique("stdin-wrapper"),
        &format!("#!/bin/sh\n/bin/cat > '{}'\nexit 0\n", capture.display()),
    );
    let image = file_digest(&worker, DRIVER_MAX_BYTES).unwrap();
    let volume = volume_dir();
    let driver = write_artifact(&volume);
    let bytes = small_envelope(&volume, &image, &driver, "fixture:two-rows", 5_000);
    let mut pull = start_pull(&worker, &volume, &bytes, 5_000);
    let found = failure(pull.next_batch().unwrap_err());
    assert!(found.teardown.reaped);
    let text = fs::read_to_string(&capture).unwrap();
    assert!(text.contains(SOURCE_REF), "child stdin missing source credential ref");
    assert!(!text.contains("oo-dl01w1-04-tgt"), "child stdin contains the target canary");
    assert!(!text.contains("target_credential_ref"), "child stdin contains the target field");
}

#[test]
fn finish_before_end_is_protocol_and_not_a_receipt() {
    let volume = volume_dir();
    let driver = write_artifact(&volume);
    let worker = child_bin();
    let image = file_digest(&worker, DRIVER_MAX_BYTES).unwrap();
    let bytes = small_envelope(&volume, &image, &driver, "fixture:two-rows", 5_000);
    let pull = start_pull(&worker, &volume, &bytes, 5_000);
    let found = assert_reaped(pull.finish().unwrap_err(), FailureKind::Protocol);
    assert!(found.token.is_none());
}

#[test]
fn kernel_network_stays_pending_and_is_not_a_network_proof() {
    // Unit check of the published constant. It does not create a network
    // namespace, an iptables rule, or a seccomp filter, and it is not a pass.
    assert_eq!(KERNEL_NETWORK, KernelNetwork::Pending);
}

#[test]
fn child_protocol_flag_and_rejects_extra_args() {
    let bin = child_bin();
    let ok = Command::new(&bin).arg("--protocol").output().unwrap();
    assert!(ok.status.success());
    assert_eq!(ok.stdout, format!("{PROTOCOL}\n").into_bytes());
    let extra = Command::new(&bin).arg("--protocol").arg("extra").output().unwrap();
    assert_eq!(extra.status.code(), Some(2));
}

#[test]
fn cli_help_states_protocol_missing_driver_and_pending_network() {
    let out = Command::new(parent_bin()).arg("--help").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains(PROTOCOL));
    assert!(text.contains("DRIVER_MISSING"));
    assert!(text.contains("kernel_network=pending"));
    assert!(text.contains("not Oracle acceptance"));
}

#[test]
fn cli_supervise_reports_two_rows_without_secret() {
    let volume = volume_dir();
    let driver = write_artifact(&volume);
    let worker = child_bin();
    let image = file_digest(&worker, DRIVER_MAX_BYTES).unwrap();
    let bytes = small_envelope(&volume, &image, &driver, "fixture:two-rows", 10_000);
    let request = std::env::temp_dir().join(unique("oo-dl01w1-04-req"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&request)
        .unwrap();
    file.write_all(&bytes).unwrap();
    drop(file);
    assert_eq!(fs::metadata(&request).unwrap().permissions().mode() & 0o777, 0o600);
    let out = Command::new(parent_bin())
        .arg("supervise")
        .arg("--worker")
        .arg(&worker)
        .arg("--driver-volume")
        .arg(&volume)
        .arg("--image-digest")
        .arg(&image)
        .arg("--driver-digest")
        .arg(&driver)
        .arg("--deadline-ms")
        .arg("10000")
        .arg("--request")
        .arg(&request)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("secret://"), "parent stderr contains a credential ref");
    assert!(!stderr.contains("oo-dl01w1-04-tgt"), "parent stderr contains the target canary");
    assert!(out.status.success(), "supervise failed: {stderr}");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(!text.contains("secret://"), "supervise stdout contains a credential ref");
    assert!(!text.contains("oo-dl01w1-04-tgt"), "supervise stdout contains the target canary");
    assert!(text.contains("rows=2"), "stdout missing rows=2");
    assert!(text.contains("kernel_network=pending"));
    assert!(text.contains("fallback=false"));
    assert!(text.contains("vendor_library_loaded=false"));
}

/// Unit test. Manifest text only. It does not prove the linked binary.
#[test]
fn unit_manifest_has_no_vendor_driver_crates() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for name in ["Cargo.toml", "isolate/Cargo.toml"] {
        let text = fs::read_to_string(root.join(name)).unwrap();
        let lower = text.to_ascii_lowercase();
        for needle in ["odbc", "oracle", "jni", "jvm", "arrow-odbc", "libclntsh", "libocci"] {
            assert!(!lower.contains(needle), "{name} mentions {needle}");
        }
    }
}

/// Unit test. Parses one caller envelope and does not spawn.
#[test]
fn unit_forwarded_json_keeps_source_ref_and_omits_target() {
    let volume = Path::new("/tmp/oo-dl01w1-04-unit-vol");
    let digest = "sha256:".to_owned() + &"ab".repeat(32);
    let bytes = small_envelope(volume, &digest, &digest, "fixture:two-rows", 1_000);
    let request = CallerRequest::try_from_slice(&bytes, volume).unwrap();
    let text = std::str::from_utf8(request.forwarded_json()).unwrap();
    assert!(text.contains(SOURCE_REF), "source ref missing from child json");
    assert!(!text.contains("oo-dl01w1-04-tgt"), "target canary reached child json");
    assert!(!text.contains("target_credential_ref"), "target field reached child json");
    assert_eq!(format!("{request:?}"), "CallerRequest { redacted }");
}

/// Unit test. Duplicate keys are refused before a typed read.
#[test]
fn unit_duplicate_key_is_refused() {
    let raw = br#"{"protocol":"oo.source.driver-worker/v1","protocol":"oo.source.driver-worker/v1"}"#;
    let err = CallerRequest::try_from_slice(raw, Path::new("/tmp/oo-dl01w1-04-unit-vol")).unwrap_err();
    assert!(matches!(err, ParentError::Config("duplicate_key")));
}

#[cfg(target_os = "linux")]
fn fdinfo_flags(fd: i32) -> String {
    let text = fs::read_to_string(format!("/proc/self/fdinfo/{fd}")).unwrap();
    text.lines()
        .find(|line| line.starts_with("flags:"))
        .unwrap_or("flags: missing")
        .to_owned()
}

#[cfg(target_os = "linux")]
fn proc_link(pid: u32, fd: &str) -> String {
    fs::read_link(format!("/proc/{pid}/fd/{fd}"))
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "unreadable".to_owned())
}

#[cfg(target_os = "linux")]
fn child_fd_links(pid: u32) -> Vec<(String, String)> {
    let mut links = Vec::new();
    let entries = fs::read_dir(format!("/proc/{pid}/fd")).unwrap();
    for entry in entries {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        links.push((name.clone(), proc_link(pid, &name)));
    }
    links.sort();
    links
}

#[cfg(target_os = "linux")]
fn wait_file(path: &Path) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        if path.is_file() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

#[cfg(target_os = "linux")]
fn children_of(parent: u32) -> Vec<u32> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return found;
    };
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(text) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
            continue;
        };
        let Some(end) = text.rfind(')') else {
            continue;
        };
        let Some(rest) = text.get(end + 2..) else {
            continue;
        };
        let mut fields = rest.split_whitespace();
        let _state = fields.next();
        let Some(ppid) = fields.next().and_then(|item| item.parse::<u32>().ok()) else {
            continue;
        };
        if ppid == parent {
            found.push(pid);
        }
    }
    found
}

#[cfg(target_os = "linux")]
fn pgrp_of(pid: u32) -> Option<u32> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let end = text.rfind(')')?;
    let rest = text.get(end + 2..)?;
    rest.split_whitespace().nth(2)?.parse().ok()
}

/// Pipe, file, and socket opened without close-on-exec stay usable here and
/// are absent from the executed child's descriptor table.
#[cfg(target_os = "linux")]
#[test]
fn inherited_pipe_file_and_socket_stay_in_parent() {
    let file_canary = b"oo-dl01w1-04-fd-file-canary";
    let pipe_canary = b"oo-dl01w1-04-fd-pipe-canary";
    let socket_canary = b"oo-dl01w1-04-fd-socket-canary";
    let canary_path = exec_dir().join(unique("fd-file"));
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&canary_path)
        .unwrap();
    file.write_all(file_canary).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    let (mut pipe_read, mut pipe_write) = std::io::pipe().unwrap();
    pipe_write.write_all(pipe_canary).unwrap();
    let (mut sock_a, mut sock_b) = UnixStream::pair().unwrap();
    sock_a
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    sock_a
        .set_write_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    sock_b
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    sock_b
        .set_write_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let leaked_file = duplicate_without_cloexec(&file).unwrap();
    let leaked_pipe = duplicate_without_cloexec(&pipe_read).unwrap();
    let leaked_sock = duplicate_without_cloexec(&sock_a).unwrap();
    let file_fd = file.as_raw_fd();
    let pipe_fd = pipe_read.as_raw_fd();
    let sock_fd = sock_a.as_raw_fd();
    let leaked_file_fd = leaked_file.as_raw_fd();
    let leaked_pipe_fd = leaked_pipe.as_raw_fd();
    let leaked_sock_fd = leaked_sock.as_raw_fd();
    let flags_before = [
        fdinfo_flags(file_fd),
        fdinfo_flags(pipe_fd),
        fdinfo_flags(sock_fd),
        fdinfo_flags(leaked_file_fd),
        fdinfo_flags(leaked_pipe_fd),
        fdinfo_flags(leaked_sock_fd),
    ];
    let leaked_links = [
        fs::read_link(format!("/proc/self/fd/{leaked_file_fd}"))
            .unwrap()
            .display()
            .to_string(),
        fs::read_link(format!("/proc/self/fd/{leaked_pipe_fd}"))
            .unwrap()
            .display()
            .to_string(),
        fs::read_link(format!("/proc/self/fd/{leaked_sock_fd}"))
            .unwrap()
            .display()
            .to_string(),
    ];
    for link in &leaked_links {
        assert!(!link.is_empty());
    }

    let worker = write_exec(
        &unique("fd-scan"),
        "#!/bin/sh\n/bin/cat > stdin.txt\n{\nfor entry in /proc/self/fd/*; do\nname=${entry##*/}\ncase \"$name\" in\n0|1|2) continue ;;\nesac\n/bin/dd if=\"$entry\" bs=128 count=1 status=none iflag=nonblock 2>/dev/null || true\ndone\n} > stolen.txt\nprintf done > scan.done\n/bin/sleep 5\n",
    );
    let image = file_digest(&worker, DRIVER_MAX_BYTES).unwrap();
    let volume = volume_dir();
    let driver = write_artifact(&volume);
    let bytes = small_envelope(&volume, &image, &driver, "fixture:two-rows", 5_000);
    let mut pull = start_pull(&worker, &volume, &bytes, 8_000);
    let pid = pull.pid();
    assert_eq!(
        [
            fdinfo_flags(file_fd),
            fdinfo_flags(pipe_fd),
            fdinfo_flags(sock_fd),
            fdinfo_flags(leaked_file_fd),
            fdinfo_flags(leaked_pipe_fd),
            fdinfo_flags(leaked_sock_fd),
        ],
        flags_before,
        "parent descriptor flags changed across spawn"
    );
    assert_eq!(file.as_raw_fd(), file_fd);
    assert_eq!(pipe_read.as_raw_fd(), pipe_fd);
    assert_eq!(sock_a.as_raw_fd(), sock_fd);
    let work = pull.work_dir().unwrap().to_path_buf();
    assert!(wait_file(&work.join("scan.done")), "child scan did not finish");
    assert!(Path::new(&format!("/proc/{pid}")).exists(), "child exited before fd listing");
    let links = child_fd_links(pid);
    let rendered = links
        .iter()
        .map(|(name, target)| format!("{name} {target}"))
        .collect::<Vec<_>>()
        .join("\n");
    for link in &leaked_links {
        assert!(
            !links.iter().any(|(_, target)| target == link),
            "child inherited {link}:\n{rendered}"
        );
    }
    assert!(
        links.iter().any(|(name, _)| name == "0"),
        "child stdin missing:\n{rendered}"
    );
    assert!(
        links.iter().any(|(name, _)| name == "1"),
        "child stdout missing:\n{rendered}"
    );
    assert!(
        links.iter().any(|(name, _)| name == "2"),
        "child stderr missing:\n{rendered}"
    );
    let stolen = fs::read(work.join("stolen.txt")).unwrap();
    for canary in [file_canary.as_slice(), pipe_canary.as_slice(), socket_canary.as_slice()] {
        assert!(
            !stolen.windows(canary.len()).any(|window| window == canary),
            "child read a caller descriptor: {}",
            String::from_utf8_lossy(&stolen)
        );
    }
    let environ = fs::read(format!("/proc/{pid}/environ")).unwrap();
    assert!(environ.is_empty(), "child environment was not empty: {environ:?}");
    let stdin = fs::read_to_string(work.join("stdin.txt")).unwrap();
    assert!(stdin.contains(SOURCE_REF), "request socket missing source ref");
    assert!(!stdin.contains("oo-dl01w1-04-tgt"), "request socket has the target canary");
    assert!(!stdin.contains("target_credential_ref"));

    let mut file_buf = Vec::new();
    file.seek(SeekFrom::Start(0)).unwrap();
    file.read_to_end(&mut file_buf).unwrap();
    assert_eq!(file_buf, file_canary);
    let mut pipe_buf = [0u8; 64];
    let pipe_n = pipe_read.read(&mut pipe_buf).unwrap();
    assert_eq!(&pipe_buf[..pipe_n], pipe_canary);
    sock_a.write_all(socket_canary).unwrap();
    let mut sock_buf = [0u8; 64];
    let sock_n = sock_b.read(&mut sock_buf).unwrap();
    assert_eq!(&sock_buf[..sock_n], socket_canary);

    let found = assert_reaped(pull.cancel(), FailureKind::Cancelled);
    assert_eq!(found.teardown.term_signal, Some(9));
    assert!(!Path::new(&format!("/proc/{pid}")).exists());
    let _keep = (leaked_file, leaked_pipe, leaked_sock, pipe_write);
}

#[cfg(target_os = "linux")]
#[test]
fn cancel_kills_group_grandchild() {
    let worker = write_exec(
        &unique("group-sleep"),
        "#!/bin/sh\n/bin/sleep 30 &\nwait\n",
    );
    let image = file_digest(&worker, DRIVER_MAX_BYTES).unwrap();
    let volume = volume_dir();
    let driver = write_artifact(&volume);
    let bytes = small_envelope(&volume, &image, &driver, "fixture:slow", 30_000);
    let mut pull = start_pull(&worker, &volume, &bytes, 10_000);
    let leader = pull.pid();
    let start = Instant::now();
    let grandchild = loop {
        let kids = children_of(leader);
        if let Some(pid) = kids.into_iter().find(|pid| pgrp_of(*pid) == Some(leader)) {
            break pid;
        }
        assert!(start.elapsed() < Duration::from_secs(3), "grandchild did not join the child group");
        thread::sleep(Duration::from_millis(20));
    };
    let found = assert_reaped(pull.cancel(), FailureKind::Cancelled);
    assert_eq!(found.teardown.group_signal, source_driver_worker::GroupSignal::Sent);
    assert_eq!(found.teardown.term_signal, Some(9));
    assert!(found.teardown.elapsed < Duration::from_secs(5));
    let settle = Instant::now();
    while Path::new(&format!("/proc/{grandchild}")).exists() && settle.elapsed() < Duration::from_secs(2) {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !Path::new(&format!("/proc/{grandchild}")).exists(),
        "grandchild {grandchild} survived group kill"
    );
    assert!(!Path::new(&format!("/proc/{leader}")).exists());

    let child = child_bin();
    let child_image = file_digest(&child, DRIVER_MAX_BYTES).unwrap();
    let again = small_envelope(&volume, &child_image, &driver, "fixture:two-rows", 5_000);
    let mut next = start_pull(&child, &volume, &again, 8_000);
    assert_ne!(next.pid(), leader);
    let batch = next.next_batch().unwrap().expect("row after group cancel");
    assert_eq!(batch.rows[0].k, 1);
    assert_eq!(batch.rows[0].v.as_deref(), Some("alpha"));
    let receipt = {
        assert!(next.next_batch().unwrap().is_some());
        assert!(next.next_batch().unwrap().is_none());
        next.finish().unwrap()
    };
    assert_eq!(receipt.rows(), 2);
    assert_eq!(receipt.schema_fingerprint(), fingerprint_hex().unwrap());
    assert_eq!(receipt.kernel_network(), KernelNetwork::Pending);
}

#[cfg(target_os = "linux")]
#[test]
fn drop_reaps_a_slow_child() {
    let volume = volume_dir();
    let driver = write_artifact(&volume);
    let worker = child_bin();
    let image = file_digest(&worker, DRIVER_MAX_BYTES).unwrap();
    let bytes = small_envelope(&volume, &image, &driver, "fixture:slow", 30_000);
    let pull = start_pull(&worker, &volume, &bytes, 10_000);
    let pid = pull.pid();
    assert!(Path::new(&format!("/proc/{pid}")).exists());
    drop(pull);
    assert!(
        !Path::new(&format!("/proc/{pid}")).exists(),
        "dropped supervisor left the child pid"
    );
}
