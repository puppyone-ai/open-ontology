#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use session_release::{run, sha256_hex, ReleaseReceipt, ReleaseRequest};
use session_store::{
    bootstrap, get, install, transition, BootstrapDisposition, Endpoint, Event, IdentityTuple, InstallDisposition,
    Outcome, Phase, Reason, ReleaseRecord, SessionState, TransitionRequest,
};
use tokio_postgres::config::SslMode;

const UUID: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a295";
const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
static GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Fix {
    host: String,
    port: u16,
    database: String,
    ca: String,
    admin: String,
    runtime: String,
    cancel: String,
    query: String,
}

fn fixture_dir() -> PathBuf {
    std::env::var_os("OO_A295_FIXTURE").map_or_else(|| PathBuf::from("/run/oo-a295/fixture"), PathBuf::from)
}

fn load_fix() -> Fix {
    let root = fixture_dir();
    let endpoint = std::fs::read_to_string(root.join("endpoint.txt")).expect("endpoint");
    let mut lines = endpoint.lines();
    Fix {
        host: lines.next().expect("host").to_owned(),
        port: lines.next().expect("port").parse().expect("port"),
        database: lines.next().expect("database").to_owned(),
        ca: std::fs::read_to_string(root.join("ca.crt")).expect("ca"),
        admin: std::fs::read_to_string(root.join("admin.pass")).expect("admin").trim().to_owned(),
        runtime: std::fs::read_to_string(root.join("runtime.pass")).expect("runtime").trim().to_owned(),
        cancel: std::fs::read_to_string(root.join("cancel.pass")).expect("cancel").trim().to_owned(),
        query: std::fs::read_to_string(root.join("query.pass")).expect("query").trim().to_owned(),
    }
}

fn endpoint_of(fix: &Fix) -> Endpoint {
    Endpoint {
        host: fix.host.clone(),
        port: fix.port,
        database: fix.database.clone(),
    }
}

fn alice(project_id: &str) -> IdentityTuple {
    IdentityTuple {
        project_id: project_id.to_owned(),
        principal_id: "alice".to_owned(),
        role: "analyst".to_owned(),
        claims_hash: ABC.to_owned(),
        policy_version: "pol-1".to_owned(),
        knowledge_version: "know-1".to_owned(),
        config_version: "cfg-1".to_owned(),
    }
}

fn requested(session_id: &str) -> SessionState {
    SessionState {
        session_id: session_id.to_owned(),
        conversation_id: "conv-1".to_owned(),
        phase: Phase::Requested,
        revision: 0,
        owner_id: String::new(),
        owner_generation: 0,
        lease_expires_at_ms: 0,
        bound: None,
        workspace_id: None,
        session_started_at_ms: 0,
        last_activity_at_ms: 0,
        last_now_ms: 0,
        turn: None,
        tool_starts: 0,
        started_tool_ids: Vec::new(),
        open_tool_ids: Vec::new(),
        last_turn_outcome: None,
        failure: None,
        release: ReleaseRecord::default(),
        journal: Vec::new(),
    }
}

async fn connect(fix: &Fix, user: &str, password: &str, application_name: &str) -> tokio_postgres::Client {
    let tls = session_store::rustls_connector(fix.ca.as_bytes()).expect("connector");
    let mut config = tokio_postgres::Config::new();
    config.user(user);
    config.password(password);
    config.host(&fix.host);
    config.port(fix.port);
    config.dbname(&fix.database);
    config.ssl_mode(SslMode::Require);
    config.application_name(application_name);
    let (client, connection) = config.connect(tls).await.expect("connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

async fn ensure_ready(fix: &Fix) {
    let receipt = install(&endpoint_of(fix), "oo_session_admin", &fix.admin, fix.ca.as_bytes()).await.expect("install");
    assert!(matches!(receipt.disposition, InstallDisposition::Installed | InstallDisposition::Replay));
}

async fn running(fix: &Fix, project: &str, session: &str) -> (u64, u64) {
    let endpoint = endpoint_of(fix);
    let boot = bootstrap(&endpoint, "oo_session_admin", &fix.admin, fix.ca.as_bytes(), project, session, &requested(session))
        .await
        .expect("bootstrap");
    assert_eq!(boot.disposition, BootstrapDisposition::Inserted);
    let identity = alice(project);
    let steps = [
        (0u64, 0u64, "ev-bind", 1_000u64, Event::BeginBind { owner_id: "entry-1".to_owned(), lease_expires_at_ms: 31_000 }),
        (1, 1, "ev-complete", 1_100, Event::CompleteBind { workspace_id: "ws-1".to_owned() }),
        (2, 1, "ev-turn", 1_200, Event::BeginTurn { turn_id: "turn-1".to_owned() }),
    ];
    let mut revision = 0;
    let mut generation = 0;
    for (expected_revision, expected_generation, event_id, now_ms, event) in steps {
        let receipt = transition(&TransitionRequest {
            endpoint: &endpoint,
            user: "oo_session_runtime",
            password: &fix.runtime,
            ca_pem: fix.ca.as_bytes(),
            project_id: project,
            session_id: session,
            identity: &identity,
            expected_revision,
            expected_owner_generation: expected_generation,
            event_id,
            now_ms,
            event: &event,
        })
        .await
        .expect(event_id);
        assert_eq!(receipt.outcome, Some(Outcome::Applied), "{event_id}");
        revision = receipt.revision.expect("revision");
        generation = receipt.owner_generation.expect("generation");
    }
    assert_eq!(revision, 3);
    assert_eq!(generation, 1);
    (revision, generation)
}

struct Tree {
    root: PathBuf,
    workspace: PathBuf,
    checkpoint: PathBuf,
    transcript: PathBuf,
    outputs: PathBuf,
}

fn fresh_ids(label: &str) -> (&'static str, &'static str) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let project = Box::leak(format!("p-{label}").into_boxed_str());
    let session = Box::leak(format!("s-{label}-{}-{n}", std::process::id()).into_boxed_str());
    (project, session)
}

fn tree(label: &str) -> Tree {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("clock").as_nanos();
    let base = std::env::temp_dir().join(format!("oo-a295-{label}-{nanos}"));
    let root = base.join(UUID);
    let workspace = root.join("workspace");
    let checkpoint = root.join("checkpoint");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir_all(&checkpoint).unwrap();
    Tree {
        root,
        workspace: workspace.clone(),
        checkpoint,
        transcript: workspace.join("transcript.txt"),
        outputs: workspace.join("outputs.txt"),
    }
}

#[allow(clippy::too_many_arguments)]
fn request(fix: &Fix, project: &str, session: &str, revision: u64, generation: u64, tree: &Tree, prefix: &str, app: &str, pid: i32, argv: Vec<String>) -> ReleaseRequest {
    ReleaseRequest {
        contract: session_release::CONTRACT.to_owned(),
        host: fix.host.clone(),
        port: fix.port,
        database: fix.database.clone(),
        runtime_user: "oo_session_runtime".to_owned(),
        runtime_password: fix.runtime.clone(),
        cancel_user: "oo_session_cancel".to_owned(),
        cancel_password: fix.cancel.clone(),
        ca_pem: fix.ca.clone(),
        project_id: project.to_owned(),
        session_id: session.to_owned(),
        identity: alice(project),
        expected_revision: revision,
        expected_owner_generation: generation,
        now_ms: 2_000,
        cancel_trigger: session_transition::CancelTrigger::User,
        event_prefix: prefix.to_owned(),
        owner_uuid: UUID.to_owned(),
        workspace_root: tree.root.display().to_string(),
        workspace_path: tree.workspace.display().to_string(),
        checkpoint_dir: tree.checkpoint.display().to_string(),
        transcript_path: tree.transcript.display().to_string(),
        outputs_path: tree.outputs.display().to_string(),
        backend_pid: pid,
        application_name: app.to_owned(),
        query_role: "oo_fixture_query".to_owned(),
        harness_argv: argv,
    }
}

fn bash(script: &str, transcript: &Path, outputs: &Path) -> Vec<String> {
    vec![
        "/bin/bash".to_owned(),
        "-c".to_owned(),
        script.to_owned(),
        "_".to_owned(),
        transcript.display().to_string(),
        outputs.display().to_string(),
    ]
}

fn step<'a>(receipt: &'a ReleaseReceipt, id: &str) -> &'a session_release::StepView {
    receipt.steps.iter().find(|step| step.id == id).expect(id)
}

fn assert_open_release(receipt: &ReleaseReceipt) {
    assert!(!receipt.physical_release_certified);
    assert!(!receipt.production_sandbox);
    assert!(!receipt.complete_release_called);
    assert!(!receipt.cancel_return_was_sufficient);
    assert!(!receipt.auth_provider_used);
    assert!(!receipt.runtime_used);
    assert!(receipt.local_fixture);
    assert_eq!(receipt.containment, "local_fixture");
    assert_eq!(receipt.phase.as_deref(), Some("releasing"));
    for id in ["s5", "s6", "s7", "s9", "s10", "s11"] {
        let item = step(receipt, id);
        assert_eq!(item.orchestrator, "unsupported", "{id}");
        assert_eq!(item.reducer, "absent", "{id}");
    }
    assert!(receipt.unsupported.iter().any(|item| item.action == "runsc" && item.status == "unsupported"));
}

fn assert_preimage(receipt: &ReleaseReceipt, id: &str) {
    let item = step(receipt, id);
    let json = item.evidence_json.as_deref().expect("preimage");
    let hash = item.evidence_sha256.as_deref().expect("hash");
    assert_eq!(sha256_hex(json.as_bytes()).as_deref(), Some(hash));
}

async fn query_pid(fix: &Fix, app: &str) -> (tokio_postgres::Client, i32) {
    let client = connect(fix, "oo_fixture_query", &fix.query, app).await;
    let pid: i32 = client.query_one("SELECT pg_catalog.pg_backend_pid()", &[]).await.expect("pid").get(0);
    (client, pid)
}

fn sqlstate(err: &tokio_postgres::Error) -> String {
    err.code().map(|code| code.code().to_owned()).unwrap_or_default()
}

#[tokio::test]
async fn blocked_query_and_slow_child_release_without_closing() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    ensure_ready(&fix).await;
    let (project, session) = fresh_ids("ok");
    let (revision, generation) = running(&fix, project, session).await;
    let app = "fixture-ok";
    let (query, pid) = query_pid(&fix, app).await;
    tokio::spawn(async move {
        let _ = query.batch_execute("SELECT pg_catalog.pg_sleep(30)").await;
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let tree = tree("ok");
    let argv = bash(
        "trap \"sleep 6; exit 0\" TERM; printf transcript > \"$1\"; printf outputs > \"$2\"; while true; do sleep 1; done",
        &tree.transcript,
        &tree.outputs,
    );
    let started = Instant::now();
    let result = run(request(&fix, project, session, revision, generation, &tree, "ok", app, pid, argv)).await;
    let elapsed = started.elapsed();
    assert_eq!(result.exit_code, 0, "{}", result.receipt.reason);
    assert_eq!(result.receipt.disposition, "released_unclosed");
    assert_eq!(result.receipt.reason, "local_fixture_not_certified");
    assert_open_release(&result.receipt);
    assert!(!result.receipt.slo_miss);
    assert!(!result.receipt.resumed);
    assert_eq!(result.receipt.deadline_basis, "monotonic_and_wall");
    for id in ["s1", "s2", "s3", "s4", "s8"] {
        let item = step(&result.receipt, id);
        assert_eq!(item.orchestrator, "confirmed", "{id}");
        assert_eq!(item.reducer, "confirmed", "{id}");
        assert_preimage(&result.receipt, id);
    }
    let workspace = step(&result.receipt, "s8").evidence_json.as_deref().unwrap();
    assert!(workspace.contains("\"adapter\":\"local_fixture_directory\""));
    assert!(workspace.contains("\"actual_absence\":true"));
    assert!(!workspace.contains("runsc"));
    assert!(matches!(result.receipt.backend_basis.as_deref(), Some("observed_idle" | "observed_absent")));
    let transcript_hash = sha256_hex(b"transcript").expect("transcript hash");
    let outputs_hash = sha256_hex(b"outputs").expect("outputs hash");
    assert!(step(&result.receipt, "s4").evidence_json.as_deref().unwrap().contains(&transcript_hash));
    assert!(step(&result.receipt, "s3").evidence_json.as_deref().unwrap().contains(&outputs_hash));
    let backend_ms = result.receipt.backend_elapsed_ms.expect("backend elapsed");
    let harness_ms = result.receipt.harness_elapsed_ms.expect("harness elapsed");
    assert!(backend_ms <= 5_000, "backend {backend_ms}");
    assert!(harness_ms >= 5_000, "harness {harness_ms}");
    assert!(backend_ms < harness_ms, "backend {backend_ms} harness {harness_ms}");
    assert!(elapsed < Duration::from_secs(20), "wall {:?}", elapsed);
    assert!(std::fs::symlink_metadata(&tree.workspace).is_err());
    let body = serde_json::to_string(&result.receipt).unwrap();
    assert!(!body.contains(&fix.runtime));
    assert!(!body.contains(&fix.cancel));
    assert!(!body.contains(&fix.query));
    let endpoint = endpoint_of(&fix);
    let identity = alice(project);
    let follow = transition(&TransitionRequest {
        endpoint: &endpoint,
        user: "oo_session_runtime",
        password: &fix.runtime,
        ca_pem: fix.ca.as_bytes(),
        project_id: project,
        session_id: session,
        identity: &identity,
        expected_revision: result.receipt.revision.expect("revision"),
        expected_owner_generation: generation,
        event_id: "ev-after",
        now_ms: 10_000,
        event: &Event::BeginTurn { turn_id: "turn-2".to_owned() },
    })
    .await
    .expect("admission");
    assert_eq!(follow.reason, Some(Reason::AdmissionStopped));
    assert_eq!(follow.phase, Some(Phase::Releasing));
    assert!(!follow.physical_release_certified);
    let view = get(&endpoint, "oo_session_runtime", &fix.runtime, fix.ca.as_bytes(), project, session).await.expect("get");
    assert_eq!(view.state.phase, Phase::Releasing);
    assert_eq!(view.state.failure, Some(session_store::FailureCode::UserCancel));
    assert!(view.state.release.s11.is_none());
}

#[tokio::test]
async fn cancel_cannot_treat_an_idle_transaction_as_stopped() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    ensure_ready(&fix).await;
    let (project, session) = fresh_ids("slo");
    let (revision, generation) = running(&fix, project, session).await;
    let app = "fixture-slo";
    let (query, pid) = query_pid(&fix, app).await;
    query.batch_execute("BEGIN; SELECT 1").await.expect("begin");
    let tree = tree("slo");
    let argv = bash("printf ok > \"$1\"; printf ok > \"$2\"; exit 0", &tree.transcript, &tree.outputs);
    let started = Instant::now();
    let result = run(request(&fix, project, session, revision, generation, &tree, "slo", app, pid, argv)).await;
    assert_eq!(result.exit_code, 2, "{}", result.receipt.reason);
    assert!(result.receipt.slo_miss);
    assert_eq!(result.receipt.backend_basis.as_deref(), Some("still_active"));
    assert_ne!(result.receipt.disposition, "released_unclosed");
    assert_open_release(&result.receipt);
    assert_eq!(step(&result.receipt, "s2").orchestrator, "still_active");
    assert_eq!(step(&result.receipt, "s2").reducer, "absent");
    let backend_ms = result.receipt.backend_elapsed_ms.expect("backend");
    let harness_ms = result.receipt.harness_elapsed_ms.expect("harness");
    assert!(backend_ms >= 4_900, "backend {backend_ms}");
    assert!(backend_ms < 8_000, "backend {backend_ms}");
    assert!(harness_ms < 2_000, "harness {harness_ms}");
    assert!(harness_ms < backend_ms, "harness {harness_ms} backend {backend_ms}");
    assert!(started.elapsed() < Duration::from_secs(12));
    let evidence = step(&result.receipt, "s2").evidence_json.as_deref().unwrap();
    assert!(evidence.contains("\"cancel_return_was_sufficient\":false"));
    let watcher = connect(&fix, "oo_session_cancel", &fix.cancel, "oo-session-watch").await;
    let state: String = watcher
        .query_one("SELECT COALESCE(state, '') FROM pg_catalog.pg_stat_activity WHERE pid = $1", &[&pid])
        .await
        .expect("state")
        .get(0);
    assert_eq!(state, "idle in transaction");
    let _ = query.batch_execute("ROLLBACK").await;
}

#[tokio::test]
async fn restart_after_the_orchestrator_is_killed_does_not_signal_or_certify() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    ensure_ready(&fix).await;
    let (project, session) = fresh_ids("restart");
    let (revision, generation) = running(&fix, project, session).await;
    let app = "fixture-restart";
    let (query, pid) = query_pid(&fix, app).await;
    tokio::spawn(async move {
        let _ = query.batch_execute("SELECT pg_catalog.pg_sleep(40)").await;
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let tree = tree("restart");
    let argv = bash(
        "trap \"sleep 30; exit 0\" TERM; printf x > \"$1\"; printf y > \"$2\"; while true; do sleep 1; done",
        &tree.transcript,
        &tree.outputs,
    );
    let mut payload = serde_json::to_vec(&request(&fix, project, session, revision, generation, &tree, "restart", app, pid, argv)).unwrap();
    let bin = env!("CARGO_BIN_EXE_session-release");
    let mut child = Command::new(bin).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().expect("cli");
    child.stdin.take().unwrap().write_all(&payload).unwrap();
    let marker = tree.checkpoint.join("harness_armed");
    let deadline = Instant::now() + Duration::from_secs(20);
    while !marker.exists() {
        assert!(Instant::now() < deadline, "harness was not armed");
        if let Some(status) = child.try_wait().expect("wait") {
            let mut out = String::new();
            if let Some(mut stdout) = child.stdout.take() {
                let _ = stdout.read_to_string(&mut out);
            }
            panic!("orchestrator exited before harness_armed: {status} {out}");
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    let checkpoint: serde_json::Value = serde_json::from_slice(&std::fs::read(tree.checkpoint.join("checkpoint.json")).unwrap()).unwrap();
    let harness_pid = checkpoint["harness"]["pid"].as_u64().unwrap();
    let starttime = checkpoint["harness"]["starttime"].as_u64().unwrap();
    let kill = Command::new("/bin/bash")
        .args(["-c", "kill -s KILL -- \"$1\"", "kill", &child.id().to_string()])
        .status()
        .expect("kill");
    assert!(kill.success());
    let status = child.wait().unwrap();
    assert!(!status.success());
    let again = Command::new(bin).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().expect("resume");
    let mut again = again;
    again.stdin.take().unwrap().write_all(&payload).unwrap();
    let output = again.wait_with_output().expect("resume wait");
    payload.fill(0);
    std::hint::black_box(&payload);
    assert_eq!(output.status.code(), Some(2), "{}", String::from_utf8_lossy(&output.stdout));
    let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).expect("receipt");
    assert_eq!(receipt["resumed"], true);
    assert_eq!(receipt["deadline_basis"], "wall_epoch");
    let steps = receipt["steps"].as_array().expect("steps");
    let s1 = steps.iter().find(|item| item["id"] == "s1").expect("s1");
    assert_eq!(s1["orchestrator"], "harness_not_owned");
    assert_ne!(s1["reducer"], "confirmed");
    assert_ne!(receipt["disposition"], "released_unclosed");
    assert_eq!(receipt["physical_release_certified"], false);
    assert_eq!(receipt["phase"], "releasing");
    assert!(tree.workspace.exists());
    let stat = std::fs::read_to_string(format!("/proc/{harness_pid}/stat")).expect("child still alive");
    assert!(stat.contains(&format!(" {starttime} ")) || stat.ends_with(&format!(" {starttime}\n")) || stat.split_whitespace().any(|field| field == starttime.to_string()));
    let _ = Command::new("/bin/bash").args(["-c", "kill -s KILL -- \"$1\"", "kill", &harness_pid.to_string()]).status();
}

#[tokio::test]
async fn inaccessible_postgres_does_not_spawn_or_close() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    ensure_ready(&fix).await;
    let (project, session) = fresh_ids("down");
    let (revision, generation) = running(&fix, project, session).await;
    let tree = tree("down");
    let mut request = request(&fix, project, session, revision, generation, &tree, "down", "fixture-down", 1, bash("exit 0", &tree.transcript, &tree.outputs));
    request.port = 1;
    let result = run(request).await;
    assert_eq!(result.exit_code, 3, "{}", result.receipt.reason);
    assert_eq!(result.receipt.disposition, "store_unavailable");
    assert_eq!(result.receipt.reason, "pg_unavailable");
    assert!(!result.receipt.database_used);
    assert!(!tree.checkpoint.join("harness_armed").exists());
    assert!(!tree.checkpoint.join("cancel_persisted").exists());
    let view = get(&endpoint_of(&fix), "oo_session_runtime", &fix.runtime, fix.ca.as_bytes(), project, session).await.expect("get");
    assert_eq!(view.state.phase, Phase::Running);
    assert_eq!(view.revision, revision);
}

#[tokio::test]
async fn noisy_transcript_is_not_confirmed_and_workspace_remains() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    ensure_ready(&fix).await;
    let (project, session) = fresh_ids("noise");
    let (revision, generation) = running(&fix, project, session).await;
    let app = "fixture-noise";
    let (query, pid) = query_pid(&fix, app).await;
    drop(query);
    let tree = tree("noise");
    let argv = bash(
        "dd if=/dev/zero of=\"$1\" bs=65537 count=1 status=none; printf ok > \"$2\"; dd if=/dev/zero bs=100000 count=1 status=none",
        &tree.transcript,
        &tree.outputs,
    );
    let result = run(request(&fix, project, session, revision, generation, &tree, "noise", app, pid, argv)).await;
    assert_eq!(result.exit_code, 2, "{}", result.receipt.reason);
    assert_eq!(step(&result.receipt, "s4").orchestrator, "too_big");
    assert_ne!(step(&result.receipt, "s4").reducer, "confirmed");
    assert_ne!(step(&result.receipt, "s8").orchestrator, "confirmed");
    assert!(tree.workspace.exists());
    assert!(std::fs::metadata(&tree.transcript).expect("transcript").len() > 65_536);
    let body = serde_json::to_string(&result.receipt).unwrap();
    assert!(body.len() < 20_000, "receipt embedded output {}", body.len());
    assert!(!result.receipt.physical_release_certified);
}

#[tokio::test]
async fn missing_workspace_and_foreign_path_have_no_side_effects() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    ensure_ready(&fix).await;
    let (project, session) = fresh_ids("path");
    let (revision, generation) = running(&fix, project, session).await;
    let tree = tree("path");
    let mut missing = request(&fix, project, session, revision, generation, &tree, "miss", "fixture-miss", 1, bash("exit 0", &tree.transcript, &tree.outputs));
    let absent = tree.root.join("absent-workspace");
    missing.workspace_path = absent.display().to_string();
    missing.transcript_path = absent.join("transcript.txt").display().to_string();
    missing.outputs_path = absent.join("outputs.txt").display().to_string();
    let missing_result = run(missing).await;
    assert_eq!(missing_result.exit_code, 2);
    assert_eq!(missing_result.receipt.reason, "workspace_missing");
    assert!(!missing_result.receipt.database_used);
    let foreign_base = std::env::temp_dir().join(format!("oo-a295-foreign-{}", std::process::id()));
    std::fs::create_dir_all(&foreign_base).unwrap();
    std::fs::write(foreign_base.join("keep.txt"), b"keep").unwrap();
    let mut foreign = request(&fix, project, session, revision, generation, &tree, "foreign", "fixture-foreign", 1, bash("exit 0", &tree.transcript, &tree.outputs));
    foreign.workspace_root = std::env::temp_dir().display().to_string();
    foreign.workspace_path = foreign_base.display().to_string();
    foreign.transcript_path = foreign_base.join("transcript.txt").display().to_string();
    foreign.outputs_path = foreign_base.join("outputs.txt").display().to_string();
    let foreign_result = run(foreign).await;
    assert_eq!(foreign_result.exit_code, 2);
    assert_eq!(foreign_result.receipt.reason, "workspace_foreign");
    assert_eq!(std::fs::read(foreign_base.join("keep.txt")).unwrap(), b"keep");
    let view = get(&endpoint_of(&fix), "oo_session_runtime", &fix.runtime, fix.ca.as_bytes(), project, session).await.expect("get");
    assert_eq!(view.state.phase, Phase::Running);
    assert_eq!(view.revision, revision);
    assert_ne!(view.state.phase, Phase::Closed);
    let _ = std::fs::remove_dir_all(&foreign_base);
}

#[tokio::test]
async fn revision_conflict_and_event_id_conflict_do_not_close() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    ensure_ready(&fix).await;
    let (project, session) = fresh_ids("rev");
    let (revision, generation) = running(&fix, project, session).await;
    let tree = tree("rev");
    let mut conflict = request(&fix, project, session, revision, generation, &tree, "rev", "fixture-rev", 1, bash("exit 0", &tree.transcript, &tree.outputs));
    conflict.expected_revision = revision + 9;
    let result = run(conflict).await;
    assert_eq!(result.exit_code, 2);
    assert_eq!(result.receipt.reason, "revision_conflict");
    assert_eq!(result.receipt.phase.as_deref(), Some("running"));
    assert_eq!(result.receipt.revision, Some(revision));
    assert!(!tree.checkpoint.join("harness_armed").exists());
    assert!(!result.receipt.physical_release_certified);
    let endpoint = endpoint_of(&fix);
    let identity = alice(project);
    transition(&TransitionRequest {
        endpoint: &endpoint,
        user: "oo_session_runtime",
        password: &fix.runtime,
        ca_pem: fix.ca.as_bytes(),
        project_id: project,
        session_id: session,
        identity: &identity,
        expected_revision: revision,
        expected_owner_generation: generation,
        event_id: "idc-cancel",
        now_ms: 1_300,
        event: &Event::Heartbeat { owner_id: "entry-1".to_owned(), lease_expires_at_ms: 31_300 },
    })
    .await
    .expect("heartbeat");
    let mut reused = request(&fix, project, session, revision + 1, generation, &tree, "idc", "fixture-rev", 1, bash("exit 0", &tree.transcript, &tree.outputs));
    reused.expected_revision = revision + 1;
    let again = run(reused).await;
    assert_eq!(again.exit_code, 2, "{}", again.receipt.reason);
    assert_eq!(again.receipt.reason, "event_id_conflict");
    assert_ne!(again.receipt.phase.as_deref(), Some("closed"));
    let view = get(&endpoint, "oo_session_runtime", &fix.runtime, fix.ca.as_bytes(), project, session).await.expect("get");
    assert_eq!(view.state.phase, Phase::Running);
    assert!(view.state.release.s1.is_none());
}

#[tokio::test]
async fn escaped_grandchild_and_canary_are_not_confirmed() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    ensure_ready(&fix).await;
    let (project, session) = fresh_ids("escape");
    let (revision, generation) = running(&fix, project, session).await;
    let app = "fixture-escape";
    let (query, pid) = query_pid(&fix, app).await;
    drop(query);
    let tree = tree("escape");
    let mut canary = Command::new("/bin/sleep").arg("120").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("canary");
    let canary_pid = canary.id();
    let argv = vec![env!("CARGO_BIN_EXE_session-release").to_owned(), "fixture-child".to_owned(), "escape".to_owned()];
    let result = run(request(&fix, project, session, revision, generation, &tree, "escape", app, pid, argv)).await;
    assert_eq!(result.exit_code, 2, "{}", result.receipt.reason);
    assert_eq!(step(&result.receipt, "s1").orchestrator, "escaped");
    assert_ne!(step(&result.receipt, "s1").reducer, "confirmed");
    assert_ne!(step(&result.receipt, "s8").orchestrator, "confirmed");
    assert!(tree.workspace.exists());
    assert!(!result.receipt.physical_release_certified);
    assert!(Path::new(&format!("/proc/{canary_pid}")).exists(), "canary was killed");
    let stdout = std::fs::read_to_string(tree.checkpoint.join("child.stdout")).unwrap_or_default();
    let grand = stdout.trim().parse::<u32>().expect("grandchild pid");
    assert!(Path::new(&format!("/proc/{grand}")).exists(), "grandchild was killed");
    let _ = Command::new("/bin/bash").args(["-c", "kill -s KILL -- \"$1\"", "kill", &canary_pid.to_string()]).status();
    let _ = canary.wait();
    let _ = Command::new("/bin/bash").args(["-c", "kill -s KILL -- \"$1\"", "kill", &grand.to_string()]).status();
}

#[tokio::test]
async fn cancel_role_is_limited_and_runtime_cannot_create() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    ensure_ready(&fix).await;
    let cancel = connect(&fix, "oo_session_cancel", &fix.cancel, "oo-session-cancel").await;
    let superuser: bool = cancel
        .query_one("SELECT rolsuper FROM pg_catalog.pg_roles WHERE rolname = current_user", &[])
        .await
        .expect("rolsuper")
        .get(0);
    assert!(!superuser);
    let update = cancel.execute("UPDATE oo_session_control.session_head SET revision = revision", &[]).await;
    assert_eq!(sqlstate(&update.expect_err("update")), "42501");
    let runtime = connect(&fix, "oo_session_runtime", &fix.runtime, "session-store").await;
    let created = runtime.batch_execute("CREATE TABLE oo_session_control.nope(id int)").await;
    assert_eq!(sqlstate(&created.expect_err("create")), "42501");
}
