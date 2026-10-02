//! Ignored live fixture for jobs-lease-operations/1.
//! `cargo test --locked --offline -p oo-jobs` does not start Docker.
//! `--ignored` fails unless the owned PostgreSQL 17.10 container is up.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const CONTAINER: &str = "oo-a102-pg1710-jobs";
const ADMIN: &str = "oo_fixture_admin";
const RUNTIME: &str = "oo_a102_runtime_login";
const OWNER: &str = "oo_a102_owner_login";
const PUBLIC_LOGIN: &str = "oo_a102_public_login";
const CANARY: &str = "canary-a102-secret-do-not-echo";
const LIVE: &str = "oo_a102_live";

struct Output {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

struct Ctx {
    dir: PathBuf,
    lines: Vec<String>,
    notes: Vec<String>,
    bootstrap: String,
    expand: String,
    event: String,
    lease: String,
}

impl Drop for Ctx {
    fn drop(&mut self) {
        let _ = std::fs::create_dir_all(&self.dir);
        let _ = std::fs::write(self.dir.join("cases.tsv"), self.lines.join("\n") + "\n");
        let _ = std::fs::write(self.dir.join("notes.txt"), self.notes.join("\n") + "\n");
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root")
        .to_path_buf()
}

fn sha256_file(path: &Path) -> String {
    let output = if Path::new("/usr/bin/sha256sum").exists() {
        Command::new("/usr/bin/sha256sum").arg(path).output()
    } else {
        Command::new("/usr/bin/shasum")
            .args(["-a", "256"])
            .arg(path)
            .output()
    }
    .unwrap_or_else(|error| panic!("hash {}: {error}", path.display()));
    assert!(output.status.success(), "hash {}", path.display());
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .expect("hash")
        .to_string()
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut command = if Path::new("/usr/bin/sha256sum").exists() {
        Command::new("/usr/bin/sha256sum")
    } else {
        let mut command = Command::new("/usr/bin/shasum");
        command.arg("-a").arg("256");
        command
    };
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("sha256");
    use std::io::Write;
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(bytes)
        .expect("write hash");
    let output = child.wait_with_output().expect("hash");
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .expect("hash")
        .to_string()
}

fn sqlstate(text: &str) -> String {
    for line in text.lines() {
        if let Some(index) = line.find("SQLSTATE:") {
            let rest = line[index + "SQLSTATE:".len()..].trim();
            if let Some(code) = rest.split_whitespace().next()
                && code.len() == 5
            {
                return code.to_string();
            }
        }
    }
    for line in text.lines() {
        let Some(index) = line.find("ERROR:") else {
            continue;
        };
        let rest = line[index + "ERROR:".len()..].trim();
        let Some((code, _)) = rest.split_once(':') else {
            continue;
        };
        let code = code.trim();
        if code.len() == 5 && code.chars().all(|ch| ch.is_ascii_alphanumeric()) {
            return code.to_string();
        }
    }
    String::new()
}

fn psql(db: &str, user: &str, file: &str, vars: &[(&str, &str)]) -> Output {
    let path = format!("/fixture/tests/fixtures/lease_operations/{file}");
    let mut command = Command::new("docker");
    command.args([
        "exec",
        "-i",
        "-u",
        "postgres",
        CONTAINER,
        "psql",
        "-U",
        user,
        "-d",
        db,
        "--no-psqlrc",
        "-q",
        "-A",
        "-t",
        "-F",
        "|",
        "-v",
        "ON_ERROR_STOP=1",
        "-f",
        &path,
    ]);
    for (key, value) in vars {
        command.arg("-v").arg(format!("{key}={value}"));
    }
    command.stdin(Stdio::null());
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("docker exec {file}: {error}"));
    Output {
        status: output.status,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn redact(text: &str) -> String {
    text.replace(CANARY, "[canary]")
}

fn tagged<'a>(stdout: &'a str, prefix: &str) -> &'a str {
    stdout
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with(prefix))
        .unwrap_or_else(|| panic!("missing {prefix} in {}", redact(stdout)))
}

fn field(line: &str, index: usize) -> &str {
    line.split('|').nth(index).unwrap_or_else(|| panic!("field {index} in {line}"))
}

fn gap_ok(value: &str) -> bool {
    value.parse::<i64>().ok().is_some_and(|gap| (28..=30).contains(&gap))
}

impl Ctx {
    fn checksums(&self) -> Vec<(String, String)> {
        vec![
            ("checksum_bootstrap".to_string(), self.bootstrap.clone()),
            ("checksum_expand".to_string(), self.expand.clone()),
            ("checksum_event_project".to_string(), self.event.clone()),
            ("checksum_lease".to_string(), self.lease.clone()),
        ]
    }

    fn record(&mut self, name: &str, output: &Output) {
        let stdout_path = self.dir.join("out").join(format!("{name}.stdout"));
        let stderr_path = self.dir.join("out").join(format!("{name}.stderr"));
        let _ = std::fs::create_dir_all(self.dir.join("out"));
        let _ = std::fs::write(&stdout_path, &output.stdout);
        let _ = std::fs::write(&stderr_path, &output.stderr);
        let code = output.status.code().unwrap_or(-1);
        self.lines.push(format!(
            "{name}\t{code}\t{}\t{}\t{}",
            sqlstate(&format!("{}{}", output.stderr, output.stdout)),
            sha256_bytes(output.stdout.as_bytes()),
            sha256_bytes(output.stderr.as_bytes())
        ));
    }

    fn run(&mut self, name: &str, db: &str, user: &str, file: &str, vars: &[(&str, &str)]) -> Output {
        let output = psql(db, user, file, vars);
        self.record(name, &output);
        let combined = format!("{}{}", output.stderr, output.stdout);
        assert!(
            !combined.contains(CANARY),
            "{name} echoed the canary\n{}",
            redact(&combined)
        );
        assert!(
            !combined.contains("/run/secrets") && !combined.contains("PGPASSWORD"),
            "{name} captured fixture auth material"
        );
        output
    }

    fn vars(&self) -> Vec<(String, String)> {
        self.checksums()
    }

    fn with_lease(&self, lease: &str) -> Vec<(String, String)> {
        let mut vars = self.checksums();
        for pair in &mut vars {
            if pair.0 == "checksum_lease" {
                pair.1 = lease.to_string();
            }
        }
        vars
    }

    fn with_bootstrap(&self, bootstrap: &str) -> Vec<(String, String)> {
        let mut vars = self.checksums();
        for pair in &mut vars {
            if pair.0 == "checksum_bootstrap" {
                pair.1 = bootstrap.to_string();
            }
        }
        vars
    }

    fn ok(&mut self, name: &str, db: &str, user: &str, file: &str, vars: &[(&str, &str)]) -> String {
        let output = self.run(name, db, user, file, vars);
        assert!(
            output.status.success(),
            "{name} failed {}\n{}",
            sqlstate(&output.stderr),
            redact(&output.stderr)
        );
        output.stdout
    }

    fn fail(&mut self, name: &str, db: &str, user: &str, file: &str, vars: &[(&str, &str)], code: &str) {
        let output = self.run(name, db, user, file, vars);
        let found = sqlstate(&format!("{}{}", output.stderr, output.stdout));
        assert!(
            !output.status.success(),
            "{name} succeeded, expected {code}\n{}",
            redact(&output.stdout)
        );
        assert_eq!(found, code, "{name} stderr {}", redact(&output.stderr));
    }

}

fn as_refs(owned: &[(String, String)]) -> Vec<(&str, &str)> {
    owned.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect()
}

fn container_is_running() -> bool {
    let output = Command::new("docker")
        .args(["inspect", "-f", "{{.State.Running}}", CONTAINER])
        .output();
    matches!(output, Ok(output) if output.status.success() && output.stdout.starts_with(b"true"))
}

fn wait_stdout(db: &str, user: &str, file: &str, expected: &str, attempts: u32) -> String {
    let mut last = String::new();
    for _ in 0..attempts {
        let output = psql(db, user, file, &[]);
        last = output.stdout.trim().to_string();
        if output.status.success() && last.split_whitespace().any(|line| line == expected) {
            return last;
        }
        thread::sleep(Duration::from_millis(250));
    }
    last
}

#[test]
#[ignore = "requires owned PostgreSQL 17.10 container oo-a102-pg1710-jobs"]
fn lease_operations_live() {
    assert!(
        container_is_running(),
        "lease live fixture requires running container {CONTAINER}"
    );
    let root = repo_root();
    let migration = root.join("crates/oo-jobs/migrations");
    let mut ctx = Ctx {
        dir: root.join("evidence/eef831aa-dba4-4af2-a78b-541bc9e0a102/SP02/lease-operations/live"),
        lines: Vec::new(),
        notes: Vec::new(),
        bootstrap: sha256_file(&migration.join("0000_bootstrap_roles.sql")),
        expand: sha256_file(&migration.join("0001_jobs_control_expand.sql")),
        event: sha256_file(&migration.join("0002_event_project_binding_expand.sql")),
        lease: sha256_file(&migration.join("0003_job_lease_operations_expand.sql")),
    };
    let _ = std::fs::create_dir_all(ctx.dir.join("out"));
    ctx.notes.push(format!("lease_sha256={}", ctx.lease));
    assert_eq!(
        ctx.lease,
        "5cb2e9030a479903ecc57ce32fc3ee851f00d146e02ab3902fcbe7ab09501034"
    );

    protocols(&mut ctx);
    behavior(&mut ctx);
    concurrency(&mut ctx);

    let summary = ctx.dir.join("result.txt");
    std::fs::write(
        &summary,
        format!("live_fixture=pass\nlease_sha256={}\n", ctx.lease),
    )
    .expect("write result");
}

fn protocols(ctx: &mut Ctx) {
    let real = ctx.vars();
    let real_ref = as_refs(&real);
    ctx.ok("reset", "postgres", ADMIN, "reset.sql", &[]);

    ctx.ok("bad_control", "oo_a102_bad", ADMIN, "protocol_control.psql", &real_ref);
    ctx.ok("bad_logins", "oo_a102_bad", ADMIN, "create_logins.sql", &[]);
    ctx.ok("bad_seed", "oo_a102_bad", RUNTIME, "seed_bad.sql", &[]);
    ctx.fail("bad_event", "oo_a102_bad", ADMIN, "protocol_bad_event.psql", &real_ref, "OO004");
    let fk = ctx.ok("bad_fk", "oo_a102_bad", ADMIN, "probe_fk.sql", &[]);
    assert_eq!(fk.trim(), "0");
    let versions = ctx.ok("bad_versions", "oo_a102_bad", ADMIN, "probe_versions.sql", &[]);
    assert!(versions.contains("jobs-bootstrap/1"));
    assert!(versions.contains("jobs-control/1"));
    assert!(!versions.contains("jobs-event-project/1"));
    assert!(!versions.contains("jobs-lease-operations/1"));
    ctx.ok("reap_bad", "postgres", ADMIN, "reap_early.sql", &[]);

    ctx.ok("upgrade_v1", "oo_a102_upgrade", ADMIN, "protocol_v1.psql", &real_ref);
    ctx.ok("upgrade_logins", "oo_a102_upgrade", ADMIN, "create_logins.sql", &[]);
    ctx.ok("upgrade_seed", "oo_a102_upgrade", RUNTIME, "seed_legal.sql", &[]);
    let before = ctx.ok("upgrade_before", "oo_a102_upgrade", ADMIN, "snapshot_rows.sql", &[]);
    ctx.ok("upgrade_v3", "oo_a102_upgrade", ADMIN, "protocol_upgrade_v3.psql", &real_ref);
    let after = ctx.ok("upgrade_after", "oo_a102_upgrade", ADMIN, "snapshot_rows.sql", &[]);
    assert_eq!(before, after, "legal history bytes changed");
    ctx.notes.push(format!("legal_history_sha256={}", sha256_bytes(before.as_bytes())));
    let ready = ctx.ok("upgrade_ready", "oo_a102_upgrade", RUNTIME, "probe_ready.sql", &[]);
    assert!(ready.trim().starts_with("true|"));
    assert!(ready.contains(&ctx.lease));
    let oid = ctx.ok("upgrade_oid", "oo_a102_upgrade", ADMIN, "probe_oid.sql", &[]).trim().to_string();
    ctx.fail("runtime_replay", "oo_a102_upgrade", RUNTIME, "runtime_replay.sql", &[], "OO013");
    let oid_after = ctx.ok("upgrade_oid_after", "oo_a102_upgrade", ADMIN, "probe_oid.sql", &[]);
    assert_eq!(oid_after.trim(), oid);
    ctx.ok("reap_upgrade", "postgres", ADMIN, "reap_early.sql", &[]);

    ctx.ok("adopt_v1", "oo_a102_adopt", ADMIN, "protocol_v1.psql", &real_ref);
    ctx.ok("adopt_stub", "oo_a102_adopt", ADMIN, "adopt_stub.sql", &[]);
    ctx.fail("adopt_0003", "oo_a102_adopt", ADMIN, "protocol_0003_noready.psql", &[], "OO014");
    let adopted = ctx.ok("adopt_after", "oo_a102_adopt", ADMIN, "adopt_after.sql", &[]);
    let mut adopted_lines = adopted.lines().map(str::trim).filter(|line| !line.is_empty());
    assert_eq!(adopted_lines.next(), Some("0|"));
    assert_eq!(adopted_lines.next(), Some("7"));
    ctx.ok("reap_adopt", "postgres", ADMIN, "reap_early.sql", &[]);

    ctx.ok("forced_v1", "oo_a102_forced", ADMIN, "protocol_v1.psql", &real_ref);
    ctx.fail("forced_0003", "oo_a102_forced", ADMIN, "protocol_0003_forced.psql", &[], "OO003");
    let shape = ctx.ok("forced_shape", "oo_a102_forced", ADMIN, "probe_claim_shape.sql", &[]);
    assert!(shape.trim().is_empty());
    let versions = ctx.ok("forced_versions", "oo_a102_forced", ADMIN, "probe_versions.sql", &[]);
    assert!(!versions.contains("jobs-lease-operations/1"));
    ctx.ok("reap_forced", "postgres", ADMIN, "reap_early.sql", &[]);

    ctx.ok("v1_install", "oo_a102_v1", ADMIN, "protocol_v1.psql", &real_ref);
    ctx.ok("v1_logins", "oo_a102_v1", ADMIN, "create_logins.sql", &[]);
    let service = ctx.ok("v1_service", "oo_a102_v1", RUNTIME, "probe_service.sql", &[]);
    assert_eq!(service.trim(), "true");
    ctx.fail("v1_claim", "oo_a102_v1", RUNTIME, "v1_claim.sql", &[], "42883");
    ctx.ok("reap_v1", "postgres", ADMIN, "reap_early.sql", &[]);

    ctx.ok("partial_v1", "oo_a102_partial", ADMIN, "protocol_v1.psql", &real_ref);
    ctx.ok("partial_0003", "oo_a102_partial", ADMIN, "protocol_0003_noready.psql", &[]);
    ctx.ok("partial_logins", "oo_a102_partial", ADMIN, "create_logins.sql", &[]);
    let ready = ctx.ok("partial_ready", "oo_a102_partial", ADMIN, "probe_ready.sql", &[]);
    assert!(ready.trim().is_empty());
    ctx.fail("partial_claim", "oo_a102_partial", RUNTIME, "v1_claim.sql", &[], "OO015");
    ctx.ok("reap_partial", "postgres", ADMIN, "reap_early.sql", &[]);

    let zeros = "0".repeat(64);
    let lie = ctx.with_lease(&zeros);
    let lie_ref = as_refs(&lie);
    ctx.ok("lie_install", "oo_a102_lie", ADMIN, "protocol_v3.psql", &lie_ref);
    ctx.ok("lie_logins", "oo_a102_lie", ADMIN, "create_logins.sql", &[]);
    let ready = ctx.ok("lie_ready", "oo_a102_lie", RUNTIME, "probe_ready.sql", &[]).trim().to_string();
    let view_present = ready.starts_with("true|");
    let conjunction = view_present && ready.split('|').nth(1) == Some(ctx.lease.as_str());
    assert!(view_present);
    assert!(!conjunction, "a stored lie must not count as file-hash ready");
    ctx.notes.push(format!("f3_view={ready} conjunction={conjunction}"));
    let touched = ctx.ok("lie_touch", "oo_a102_lie", RUNTIME, "lie_touch.sql", &[]);
    assert!(touched.contains("enqueued|queued"));
    ctx.ok("reap_lie", "postgres", ADMIN, "reap_early.sql", &[]);

    let real = ctx.vars();
    let real_ref = as_refs(&real);
    ctx.ok("live_install", LIVE, ADMIN, "protocol_v3.psql", &real_ref);
    ctx.ok("live_logins", LIVE, ADMIN, "create_logins.sql", &[]);
    let ready = ctx.ok("live_ready", LIVE, RUNTIME, "probe_ready.sql", &[]).trim().to_string();
    assert_eq!(ready, format!("true|{}", ctx.lease));
    let oid = ctx.ok("live_oid", LIVE, ADMIN, "probe_oid.sql", &[]).trim().to_string();
    let versions = ctx.ok("live_versions", LIVE, ADMIN, "probe_versions.sql", &[]);
    ctx.ok("live_repeat", LIVE, ADMIN, "protocol_repeat.psql", &real_ref);
    let oid_repeat = ctx.ok("live_oid_repeat", LIVE, ADMIN, "probe_oid.sql", &[]);
    assert_eq!(oid_repeat.trim(), oid);
    let ones = "1".repeat(64);
    let mismatch = ctx.with_bootstrap(&ones);
    let mismatch_ref = as_refs(&mismatch);
    ctx.fail("live_mismatch", LIVE, ADMIN, "protocol_repeat.psql", &mismatch_ref, "OO001");
    let versions_after = ctx.ok("live_versions_after", LIVE, ADMIN, "probe_versions.sql", &[]);
    assert_eq!(versions, versions_after);
    assert!(versions_after.contains(&ctx.bootstrap));
    assert!(!versions_after.contains(&ones));
    let oid_mismatch = ctx.ok("live_oid_mismatch", LIVE, ADMIN, "probe_oid.sql", &[]);
    assert_eq!(oid_mismatch.trim(), oid);
}

fn behavior(ctx: &mut Ctx) {
    let who = ctx.ok("whoami", LIVE, RUNTIME, "whoami.sql", &[]);
    assert_eq!(who.trim(), format!("{RUNTIME}|{RUNTIME}"));
    let shape = ctx.ok("catalog_shape", LIVE, ADMIN, "catalog_shape.sql", &[]);
    for name in ["enqueue", "claim", "heartbeat", "complete_success", "complete_failure", "retry_failure", "cancel"] {
        let line = tagged(&shape, name);
        assert!(line.contains("|true|{\"search_path=pg_catalog, jobs\"}|"), "{line}");
    }
    let heartbeat = tagged(&shape, "heartbeat");
    assert!(heartbeat.contains(
        "p_project_id uuid, p_task_id uuid, p_worker_id text, p_generation bigint"
    ));
    assert!(!heartbeat.contains("interval"));
    let privileges = ctx.ok("privileges", LIVE, ADMIN, "privileges.sql", &[]);
    assert_eq!(
        privileges.trim(),
        "false|false|false|true|false|false|true|true|true|false|false|false"
    );

    let identity = ctx.ok("identity", LIVE, RUNTIME, "identity.sql", &[]);
    let first = tagged(&identity, "first|");
    let replay = tagged(&identity, "replay|");
    let cross = tagged(&identity, "cross|");
    assert_eq!(field(first, 1), "enqueued");
    assert_eq!(field(replay, 1), "replayed");
    assert_eq!(field(first, 2), field(replay, 2));
    assert_eq!(field(first, 4), field(replay, 4));
    assert_eq!(field(cross, 1), "enqueued");
    assert_ne!(field(cross, 2), field(first, 2));
    let before_conflict = ctx.ok("conflict_before", LIVE, RUNTIME, "conflict_count.sql", &[]);
    ctx.fail("conflict", LIVE, RUNTIME, "conflict.sql", &[], "OO011");
    let after_conflict = ctx.ok("conflict_after", LIVE, RUNTIME, "conflict_count.sql", &[]);
    assert_eq!(before_conflict.trim(), after_conflict.trim());

    ctx.fail("dml_task", LIVE, RUNTIME, "dml_task.sql", &[], "42501");
    ctx.fail("dml_event", LIVE, RUNTIME, "dml_event.sql", &[], "42501");
    ctx.fail("public_exec", LIVE, PUBLIC_LOGIN, "public_exec.sql", &[], "42501");
    ctx.fail("internal_exec", LIVE, RUNTIME, "internal_exec.sql", &[], "42501");
    let scheduled = ctx.ok("schedule_insert", LIVE, RUNTIME, "schedule_insert.sql", &[]);
    assert!(scheduled.trim().starts_with("scheduled|"));
    let fence = ctx.ok("fence_insert", LIVE, RUNTIME, "fence_insert.sql", &[]);
    assert_eq!(fence.trim(), "fence|false");
    ctx.fail("fence_runtime", LIVE, RUNTIME, "fence_update_runtime.sql", &[], "42501");
    ctx.fail("fence_owner", LIVE, OWNER, "fence_update_owner.sql", &[], "23514");
    let fence_value = ctx.ok("fence_read", LIVE, RUNTIME, "fence_read.sql", &[]);
    assert_eq!(fence_value.trim(), "false");
    ctx.notes.push("target_fence_certified=false".to_string());

    for name in [
        "neg_null_claim.sql",
        "neg_null_enqueue.sql",
        "neg_empty_kinds.sql",
        "neg_dup_kinds.sql",
        "neg_worker_space.sql",
        "neg_worker_unicode.sql",
        "neg_worker_long.sql",
        "neg_max_zero.sql",
        "neg_max_33.sql",
        "neg_bad_digest.sql",
        "neg_payload_array.sql",
        "neg_payload_long.sql",
        "neg_bad_json.sql",
        "neg_password_key.sql",
        "neg_kind_canary.sql",
        "neg_65_kinds.sql",
    ] {
        let stem = name.trim_end_matches(".sql");
        ctx.fail(stem, LIVE, RUNTIME, name, &[], "OO010");
    }

    let cycle = ctx.ok("lease_cycle", LIVE, RUNTIME, "lease_cycle.sql", &[]);
    assert_eq!(field(tagged(&cycle, "enq|"), 1), "enqueued");
    let claim = tagged(&cycle, "claim|");
    assert_eq!(field(claim, 1), "claimed");
    assert_eq!(field(claim, 2), "running");
    assert_eq!(field(claim, 3), "1");
    assert_eq!(field(claim, 4), "1");
    assert_eq!(field(claim, 5), "worker-a");
    assert!(gap_ok(field(claim, 6)), "claim gap {}", field(claim, 6));
    assert_eq!(field(claim, 7), "jlt1.");
    assert_eq!(field(tagged(&cycle, "stale|"), 1), "lost_lease");
    let beat = tagged(&cycle, "beat|");
    assert_eq!(field(beat, 1), "extended");
    assert!(gap_ok(field(beat, 2)), "heartbeat gap {}", field(beat, 2));
    assert_eq!(field(tagged(&cycle, "events|"), 1), "2");
    assert_eq!(field(tagged(&cycle, "oldgen|"), 1), "lost_lease");
    assert_eq!(field(tagged(&cycle, "done|"), 1), "succeeded");
    assert_eq!(field(tagged(&cycle, "done|"), 3), "");
    assert_eq!(field(tagged(&cycle, "again|"), 1), "terminal");
    assert_eq!(field(tagged(&cycle, "anygen|"), 1), "terminal");
    assert!(!cycle.contains("already_applied"));

    let started = Instant::now();
    let hb = ctx.ok("heartbeat_10s", LIVE, RUNTIME, "heartbeat_10s.sql", &[]);
    assert!(started.elapsed() >= Duration::from_secs(9), "10s heartbeat returned too fast");
    let hb_line = hb
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .expect("heartbeat line");
    let parts: Vec<_> = hb_line.split('|').collect();
    assert_eq!(parts.first().copied(), Some("enqueued"));
    assert_eq!(parts.get(1).copied(), Some("claimed"));
    assert_eq!(parts.get(2).copied(), Some("extended"));
    assert_eq!(parts.get(3).copied(), Some("true"));
    assert!(gap_ok(parts.get(4).copied().unwrap_or("")), "10s gap {hb_line}");

    let reclaim = ctx.ok("reclaim_claim", LIVE, RUNTIME, "reclaim_claim.sql", &[]);
    let first = tagged(&reclaim, "claim|");
    assert_eq!(field(first, 3), "1");
    assert!(field(first, 5).starts_with("jlt1."));
    ctx.ok("reclaim_expire", LIVE, OWNER, "reclaim_expire.sql", &[]);
    let finish = ctx.ok("reclaim_finish", LIVE, RUNTIME, "reclaim_finish.sql", &[]);
    let second = tagged(&finish, "reclaim|");
    assert_eq!(field(second, 1), "claimed");
    assert_eq!(field(second, 2), "2");
    assert_eq!(field(second, 3), "2");
    assert_eq!(field(second, 4), "worker-same");
    assert!(field(second, 5).starts_with("jlt1."));
    assert_ne!(field(first, 5), field(second, 5));
    assert_eq!(field(tagged(&finish, "old|"), 1), "lost_lease");
    assert_eq!(field(tagged(&finish, "oldbeat|"), 1), "lost_lease");
    assert_eq!(field(tagged(&finish, "new|"), 1), "succeeded");

    let cancel = ctx.ok("cancel_setup", LIVE, RUNTIME, "cancel_setup.sql", &[]);
    assert_eq!(field(tagged(&cancel, "qc|"), 1), "cancelled");
    assert_eq!(field(tagged(&cancel, "qc|"), 3), "");
    assert_eq!(field(tagged(&cancel, "qr|"), 1), "terminal");
    assert_eq!(field(tagged(&cancel, "run|"), 1), "claimed");
    assert_eq!(field(tagged(&cancel, "run|"), 2), "1");
    ctx.ok("cancel_expire", LIVE, OWNER, "cancel_expire.sql", &[]);
    let cancelled = ctx.ok("cancel_finish", LIVE, RUNTIME, "cancel_finish.sql", &[]);
    let cx = tagged(&cancelled, "cx|");
    assert_eq!(field(cx, 1), "cancelled");
    assert_eq!(field(cx, 3), "worker-c");
    assert_eq!(field(cx, 4), "1");
    assert!(!field(cx, 5).is_empty());
    assert_eq!(field(tagged(&cancelled, "hb|"), 1), "terminal");
    assert_eq!(field(tagged(&cancelled, "again|"), 1), "terminal");

    let future = ctx.ok("future_empty", LIVE, RUNTIME, "future_and_empty.sql", &[]);
    assert_eq!(field(tagged(&future, "future|"), 1), "none");
    assert_eq!(field(tagged(&future, "stored|"), 1), "queued");
    assert_eq!(field(tagged(&future, "empty|"), 1), "none");

    ctx.fail("rollback_enqueue", LIVE, RUNTIME, "rollback_enqueue.sql", &[], "22012");
    let rolled = ctx.ok("rollback_enqueue_count", LIVE, RUNTIME, "rollback_enqueue_count.sql", &[]);
    assert_eq!(rolled.trim(), "0");
    let before_claim = ctx.ok("rollback_claim_setup", LIVE, RUNTIME, "rollback_claim_setup.sql", &[]);
    assert_eq!(tagged(&before_claim, "before|"), "before|queued|0|0|1");
    ctx.ok("rollback_claim", LIVE, RUNTIME, "rollback_claim.sql", &[]);
    let after_claim = ctx.ok("rollback_claim_count", LIVE, RUNTIME, "rollback_claim_count.sql", &[]);
    assert_eq!(after_claim.trim(), "queued|0|0|1");

    ctx.ok("failure_setup", LIVE, RUNTIME, "failure_setup.sql", &[]);
    ctx.fail("failure_bad_kind", LIVE, RUNTIME, "failure_bad_kind.sql", &[], "OO010");
    let failed = ctx.ok("failure_ok", LIVE, RUNTIME, "failure_ok.sql", &[]);
    assert_eq!(field(tagged(&failed, "fail|"), 1), "failed");
    assert_eq!(field(tagged(&failed, "stored|"), 1), "payload_rejected");
    assert_eq!(field(tagged(&failed, "again|"), 1), "terminal");

    ctx.ok("cap_seed", LIVE, OWNER, "cap_seed.sql", &[]);
    let cap = ctx.ok("cap_claim", LIVE, RUNTIME, "cap_claim.sql", &[]);
    assert_eq!(tagged(&cap, "first|"), "first|none|");
    assert_eq!(tagged(&cap, "counts|"), "counts|8|1");
    assert_eq!(field(tagged(&cap, "second|"), 1), "claimed");
    assert_eq!(field(tagged(&cap, "second|"), 3), "1");
    assert_eq!(field(tagged(&cap, "resurrect|"), 1), "terminal");

    ctx.ok("overflow_enqueue", LIVE, RUNTIME, "overflow_enqueue.sql", &[]);
    ctx.ok("overflow_bump", LIVE, OWNER, "overflow_bump.sql", &[]);
    ctx.fail("overflow_claim", LIVE, RUNTIME, "overflow_claim.sql", &[], "OO012");
    let over = ctx.ok("overflow_check", LIVE, RUNTIME, "overflow_check.sql", &[]);
    assert_eq!(over.trim(), "queued|9223372036854775807");

    ctx.ok("retry_seed", LIVE, OWNER, "retry_seed.sql", &[]);
    let retry = ctx.ok("retry_run", LIVE, RUNTIME, "retry_run.sql", &[]);
    assert_eq!(field(tagged(&retry, "miss|"), 1), "lost_lease");
    let d1 = tagged(&retry, "d1|");
    assert_eq!(field(d1, 1), "queued");
    assert!((0..=2).contains(&field(d1, 2).parse::<i64>().unwrap_or(-1)), "{d1}");
    assert_eq!(field(d1, 3), "worker_failed");
    assert_eq!(field(d1, 4), "1");
    let d6 = field(tagged(&retry, "d6|"), 2).parse::<i64>().unwrap_or(-1);
    assert!((31..=33).contains(&d6), "d6 {d6}");
    let d7 = field(tagged(&retry, "d7|"), 2).parse::<i64>().unwrap_or(-1);
    assert!((59..=60).contains(&d7), "d7 {d7}");
    assert_eq!(tagged(&retry, "ex|"), "ex|failed|attempt_exhausted");

    ctx.ok("poison_create", LIVE, ADMIN, "poison_create.sql", &[]);
    let poisoned = ctx.run("poison_claim", LIVE, RUNTIME, "poison_claim.sql", &[]);
    let dropped = ctx.run("poison_drop", LIVE, ADMIN, "poison_drop.sql", &[]);
    assert!(dropped.status.success(), "{}", redact(&dropped.stderr));
    assert!(poisoned.status.success(), "{}", redact(&poisoned.stderr));
    let poison_gap = field(tagged(&poisoned.stdout, "gap|"), 2);
    assert!(gap_ok(poison_gap), "poison gap {poison_gap}");
    ctx.notes.push(format!("poison_gap={poison_gap}"));
}

fn concurrency(ctx: &mut Ctx) {
    ctx.ok("deadlock_setup", LIVE, RUNTIME, "deadlock_setup.sql", &[]);
    let owner = thread::spawn(|| psql(LIVE, ADMIN, "deadlock_owner.sql", &[]));
    let saw = wait_stdout(LIVE, ADMIN, "activity_deadlock.sql", "1", 40);
    assert_eq!(saw, "1", "deadlock owner did not reach pg_sleep");
    let claim = thread::spawn(|| psql(LIVE, RUNTIME, "deadlock_claim.sql", &[]));
    let claim = claim.join().expect("deadlock claim");
    let owner = owner.join().expect("deadlock owner");
    ctx.record("deadlock_owner", &owner);
    ctx.record("deadlock_claim", &claim);
    let owner_state = sqlstate(&format!("{}{}", owner.stderr, owner.stdout));
    let claim_state = sqlstate(&format!("{}{}", claim.stderr, claim.stdout));
    ctx.notes.push(format!("deadlock owner={owner_state} claim={claim_state}"));
    assert!(
        owner_state == "40P01" || claim_state == "40P01",
        "expected 40P01\nowner {}\nclaim {}",
        redact(&owner.stderr),
        redact(&claim.stderr)
    );
    let _ = psql(LIVE, ADMIN, "terminate_deadlock.sql", &[]);
    let after = ctx.ok("deadlock_after", LIVE, ADMIN, "deadlock_after.sql", &[]);
    assert!(after.trim().starts_with("1|"));

    let versions = ctx.ok("lock_versions_before", LIVE, ADMIN, "probe_versions.sql", &[]);
    let holder = thread::spawn(|| psql(LIVE, ADMIN, "lock_holder.sql", &[]));
    let saw = wait_stdout(LIVE, ADMIN, "activity_holder.sql", "1", 40);
    assert_eq!(saw, "1", "advisory holder did not reach pg_sleep");
    let real = ctx.vars();
    let real_ref = as_refs(&real);
    ctx.fail("lock_waiter", LIVE, ADMIN, "protocol_repeat.psql", &real_ref, "55P03");
    let _ = psql(LIVE, ADMIN, "terminate_holder.sql", &[]);
    let holder = holder.join().expect("lock holder");
    ctx.record("lock_holder", &holder);
    let versions_after = ctx.ok("lock_versions_after", LIVE, ADMIN, "probe_versions.sql", &[]);
    assert_eq!(versions, versions_after);

    let queued = ctx.ok("batch_enqueue", LIVE, RUNTIME, "batch_enqueue.sql", &[]);
    assert_eq!(queued.trim(), "100");
    let mut handles = Vec::new();
    for index in 0..100 {
        let worker = format!("w{index:03}");
        handles.push(thread::spawn(move || {
            let vars = [("worker_id", worker.as_str())];
            let output = psql(LIVE, RUNTIME, "claim_one.sql", &vars);
            (worker, output)
        }));
    }
    let mut claimed = Vec::new();
    for handle in handles {
        let (worker, output) = handle.join().expect("claim worker");
        let name = format!("claim_{worker}");
        ctx.record(&name, &output);
        assert!(output.status.success(), "{worker} {}", redact(&output.stderr));
        let line = output.stdout.trim().to_string();
        assert_eq!(field(&line, 0), "claimed", "{line}");
        assert_eq!(field(&line, 2), "1", "{line}");
        assert_eq!(field(&line, 3), worker, "{line}");
        claimed.push(field(&line, 1).to_string());
    }
    claimed.sort();
    claimed.dedup();
    assert_eq!(claimed.len(), 100);
    let counts = ctx.ok("batch_counts", LIVE, ADMIN, "batch_counts.sql", &[]);
    assert_eq!(counts.trim(), "100|100|100|100|100|100");
    let locks = ctx.ok("locks", LIVE, ADMIN, "locks.sql", &[]);
    assert_eq!(locks.trim(), "0|0");

    ctx.ok("kill_enqueue", LIVE, RUNTIME, "kill_enqueue.sql", &[]);
    let holder = thread::spawn(|| psql(LIVE, RUNTIME, "claim_hold.sql", &[]));
    let saw = wait_stdout(LIVE, ADMIN, "activity_hold.sql", "1", 40);
    let _ = psql(LIVE, ADMIN, "terminate_hold.sql", &[]);
    let holder = holder.join().expect("claim hold");
    ctx.record("claim_hold", &holder);
    assert_eq!(saw, "1", "claim hold did not reach pg_sleep");
    assert!(!holder.status.success(), "killed claim committed");
    let checked = ctx.ok("kill_check", LIVE, ADMIN, "kill_check.sql", &[]);
    assert_eq!(checked.trim(), "queued|0|0|1");
    let recovered = ctx.ok("kill_recover", LIVE, RUNTIME, "kill_recover.sql", &[]);
    assert_eq!(recovered.trim(), "claimed|1|1|worker-hold");

    let durable = ctx.ok("durability_before", LIVE, ADMIN, "durability.sql", &[]);
    let restart = Command::new("docker")
        .args(["restart", CONTAINER])
        .output()
        .expect("restart");
    assert!(restart.status.success(), "{}", String::from_utf8_lossy(&restart.stderr));
    let mut ready = false;
    for _ in 0..90 {
        let output = psql("postgres", ADMIN, "ready_ping.sql", &[]);
        if output.status.success() && output.stdout.contains('1') {
            ready = true;
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }
    assert!(ready, "postgres did not become ready after restart");
    let durable_after = ctx.ok("durability_after", LIVE, ADMIN, "durability.sql", &[]);
    assert_eq!(durable, durable_after);
    ctx.notes.push(format!("durability_sha256={}", sha256_bytes(durable.as_bytes())));
}
