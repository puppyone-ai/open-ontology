#![forbid(unsafe_code)]

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Arc;

use session_store::{
    bootstrap, get, install, migration_sha256, reconcile, transition, transition_with_fault, BootstrapDisposition,
    CommitFault, Disposition, Endpoint, Event, IdentityTuple, InstallDisposition, Outcome, Phase, Reason,
    ReconcileDisposition, ReleaseRecord, SessionState, StoreError, TransitionReceipt, TransitionRequest, CONTRACT,
    STDIN_MAX_BYTES,
};
use tokio_postgres::config::SslMode;
use tokio_postgres::{Client, Error};

const FIXTURE: &str = "/run/oo-a275/fixture";
static GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

struct Fix {
    host: String,
    port: u16,
    database: String,
    ca: Vec<u8>,
    wrong_ca: Vec<u8>,
    admin: String,
    runtime: String,
}

fn load_fix() -> Fix {
    let root = std::path::Path::new(FIXTURE);
    let endpoint = std::fs::read_to_string(root.join("endpoint.txt")).expect("endpoint");
    let mut lines = endpoint.lines();
    let host = lines.next().expect("host").to_owned();
    let port = lines.next().expect("port").parse().expect("port");
    let database = lines.next().expect("database").to_owned();
    Fix {
        host,
        port,
        database,
        ca: std::fs::read(root.join("ca.crt")).expect("ca"),
        wrong_ca: std::fs::read(root.join("wrong.crt")).expect("wrong ca"),
        admin: std::fs::read_to_string(root.join("admin.pass")).expect("admin").trim().to_owned(),
        runtime: std::fs::read_to_string(root.join("runtime.pass")).expect("runtime").trim().to_owned(),
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

fn requested(session_id: &str, conversation_id: &str) -> SessionState {
    SessionState {
        session_id: session_id.to_owned(),
        conversation_id: conversation_id.to_owned(),
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

async fn connect_as(fix: &Fix, user: &str, password: &str, ca: &[u8]) -> Client {
    let tls = session_store::rustls_connector(ca).expect("connector");
    let mut config = tokio_postgres::Config::new();
    config.user(user);
    config.password(password);
    config.host(&fix.host);
    config.port(fix.port);
    config.dbname(&fix.database);
    config.ssl_mode(SslMode::Require);
    config.application_name("session-store-harness");
    let (client, connection) = config.connect(tls).await.expect("harness connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

fn sqlstate(err: &Error) -> Option<&str> {
    err.code().map(|code| code.code())
}

async fn ensure_ready(fix: &Fix) {
    let endpoint = endpoint_of(fix);
    let receipt = install(&endpoint, "oo_session_admin", &fix.admin, &fix.ca)
        .await
        .expect("install");
    assert!(matches!(
        receipt.disposition,
        InstallDisposition::Installed | InstallDisposition::Replay
    ));
    assert!(receipt.database_used);
    assert!(!receipt.auth_provider_used);
}

async fn boot(fix: &Fix, project_id: &str, session_id: &str) {
    let endpoint = endpoint_of(fix);
    let receipt = bootstrap(
        &endpoint,
        "oo_session_admin",
        &fix.admin,
        &fix.ca,
        project_id,
        session_id,
        &requested(session_id, "conv-1"),
    )
    .await
    .expect("bootstrap");
    assert_eq!(receipt.disposition, BootstrapDisposition::Inserted);
}

fn begin_event() -> Event {
    Event::BeginBind {
        owner_id: "entry-1".to_owned(),
        lease_expires_at_ms: 31_000,
    }
}

async fn apply_begin(fix: &Fix, project_id: &str, session_id: &str, event_id: &str) -> TransitionReceipt {
    let endpoint = endpoint_of(fix);
    let identity = alice(project_id);
    let event = begin_event();
    transition(&TransitionRequest {
        endpoint: &endpoint,
        user: "oo_session_runtime",
        password: &fix.runtime,
        ca_pem: &fix.ca,
        project_id,
        session_id,
        identity: &identity,
        expected_revision: 0,
        expected_owner_generation: 0,
        event_id,
        now_ms: 1_000,
        event: &event,
    })
    .await
    .expect("begin")
}

#[tokio::test]
async fn install_replay_drift_partial_and_privileges() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    let endpoint = endpoint_of(&fix);
    let first = install(&endpoint, "oo_session_admin", &fix.admin, &fix.ca)
        .await
        .expect("first install");
    assert!(matches!(
        first.disposition,
        InstallDisposition::Installed | InstallDisposition::Replay
    ));
    assert_eq!(first.sql_sha256, migration_sha256().expect("hash"));
    let second = install(&endpoint, "oo_session_admin", &fix.admin, &fix.ca)
        .await
        .expect("replay");
    assert_eq!(second.disposition, InstallDisposition::Replay);
    assert_eq!(second.sql_sha256, first.sql_sha256);

    let admin = connect_as(&fix, "oo_session_admin", &fix.admin, &fix.ca).await;
    let damaged = "0".repeat(64);
    admin
        .execute(
            "UPDATE oo_session_control.install_manifest SET sql_sha256 = $1 WHERE singleton = TRUE",
            &[&damaged],
        )
        .await
        .expect("damage hash");
    let drifted = install(&endpoint, "oo_session_admin", &fix.admin, &fix.ca).await;
    let still: String = admin
        .query_one(
            "SELECT sql_sha256 FROM oo_session_control.install_manifest WHERE singleton = TRUE",
            &[],
        )
        .await
        .expect("read hash")
        .get(0);
    admin
        .execute(
            "UPDATE oo_session_control.install_manifest SET sql_sha256 = $1 WHERE singleton = TRUE",
            &[&first.sql_sha256],
        )
        .await
        .expect("restore hash");
    assert!(matches!(drifted, Err(StoreError::InstallDrift { check: "hash" })));
    assert_eq!(still, damaged);
    let restored = install(&endpoint, "oo_session_admin", &fix.admin, &fix.ca)
        .await
        .expect("restored install");
    assert_eq!(restored.disposition, InstallDisposition::Replay);

    admin
        .batch_execute("DROP SCHEMA oo_session_control CASCADE")
        .await
        .expect("drop");
    admin
        .batch_execute("CREATE SCHEMA oo_session_control")
        .await
        .expect("partial schema");
    let partial = install(&endpoint, "oo_session_admin", &fix.admin, &fix.ca).await;
    admin
        .batch_execute("DROP SCHEMA oo_session_control CASCADE")
        .await
        .expect("drop partial");
    let rebuilt = install(&endpoint, "oo_session_admin", &fix.admin, &fix.ca)
        .await
        .expect("rebuild");
    assert!(matches!(partial, Err(StoreError::InstallDrift { .. })));
    assert_eq!(rebuilt.disposition, InstallDisposition::Installed);

    let runtime = connect_as(&fix, "oo_session_runtime", &fix.runtime, &fix.ca).await;
    let insert = runtime
        .execute(
            "INSERT INTO oo_session_control.session_head (project_id, session_id, revision, owner_generation, state_json, journal_len) VALUES ('p', 's', 0, 0, '{}', 0)",
            &[],
        )
        .await;
    let manifest = runtime
        .query_one("SELECT sql_sha256 FROM oo_session_control.install_manifest", &[])
        .await;
    let delete = runtime
        .execute(
            "DELETE FROM oo_session_control.session_head WHERE project_id = $1",
            &[&"none"],
        )
        .await;
    assert_eq!(insert.as_ref().err().and_then(sqlstate), Some("42501"));
    assert_eq!(manifest.as_ref().err().and_then(sqlstate), Some("42501"));
    assert_eq!(delete.as_ref().err().and_then(sqlstate), Some("42501"));

    let row = runtime
        .query_one(
            "SELECT ssl, version FROM pg_catalog.pg_stat_ssl WHERE pid = pg_catalog.pg_backend_pid()",
            &[],
        )
        .await
        .expect("ssl fact");
    let ssl: bool = row.get(0);
    let version: String = row.get(1);
    assert!(ssl);
    assert!(version.starts_with("TLSv1."));
}

#[tokio::test]
async fn wrong_ca_does_not_connect() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    let endpoint = endpoint_of(&fix);
    let bad_pem = install(&endpoint, "oo_session_admin", &fix.admin, b"not-a-certificate").await;
    assert!(matches!(bad_pem, Err(StoreError::Tls)));
    let wrong = install(&endpoint, "oo_session_admin", &fix.admin, &fix.wrong_ca).await;
    assert!(matches!(wrong, Err(StoreError::Tls | StoreError::Connect)));
}

#[tokio::test]
async fn two_connections_race_one_winner() {
    let _gate = GATE.lock().await;
    let fix = Arc::new(load_fix());
    ensure_ready(&fix).await;
    let project = "project-race";
    let session = "sess-race";
    boot(&fix, project, session).await;
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let left = race_task(Arc::clone(&fix), Arc::clone(&barrier), project, session, "ev-race-a");
    let right = race_task(fix, barrier, project, session, "ev-race-b");
    let (left, right) = tokio::join!(left, right);
    let mut applied = 0;
    let mut stale = 0;
    for result in [left.expect("task"), right.expect("task")] {
        let receipt = result.expect("transition");
        match (receipt.outcome, receipt.reason) {
            (Some(Outcome::Applied), None) => applied += 1,
            (Some(Outcome::Refused), Some(Reason::Stale)) => stale += 1,
            other => panic!("unexpected race result {other:?}"),
        }
        assert!(receipt.reducer.as_ref().is_none_or(|reducer| !reducer.database_used));
    }
    assert_eq!((applied, stale), (1, 1));
    let fix = load_fix();
    let view = fetch(&fix, project, session).await;
    assert_eq!(view.revision, 1);
    assert_eq!(view.journal_len, 1);
}

fn race_task(
    fix: Arc<Fix>,
    barrier: Arc<tokio::sync::Barrier>,
    project: &'static str,
    session: &'static str,
    event_id: &'static str,
) -> tokio::task::JoinHandle<Result<TransitionReceipt, StoreError>> {
    tokio::spawn(async move {
        barrier.wait().await;
        let endpoint = endpoint_of(&fix);
        let identity = alice(project);
        let event = begin_event();
        transition(&TransitionRequest {
            endpoint: &endpoint,
            user: "oo_session_runtime",
            password: &fix.runtime,
            ca_pem: &fix.ca,
            project_id: project,
            session_id: session,
            identity: &identity,
            expected_revision: 0,
            expected_owner_generation: 0,
            event_id,
            now_ms: 1_000,
            event: &event,
        })
        .await
    })
}

#[tokio::test]
async fn rollback_before_commit_leaves_the_row() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    ensure_ready(&fix).await;
    let project = "project-rollback";
    let session = "sess-rollback";
    boot(&fix, project, session).await;
    let endpoint = endpoint_of(&fix);
    let identity = alice(project);
    let event = begin_event();
    let receipt = transition_with_fault(
        &TransitionRequest {
            endpoint: &endpoint,
            user: "oo_session_runtime",
            password: &fix.runtime,
            ca_pem: &fix.ca,
            project_id: project,
            session_id: session,
            identity: &identity,
            expected_revision: 0,
            expected_owner_generation: 0,
            event_id: "ev-rollback",
            now_ms: 1_000,
            event: &event,
        },
        CommitFault::RollBackBeforeCommit,
    )
    .await
    .expect("rollback ack");
    assert_eq!(receipt.disposition, Disposition::RolledBack);
    assert!(receipt.reducer.is_none());
    assert!(receipt.revision.is_none());
    let view = fetch(&fix, project, session).await;
    assert_eq!(view.revision, 0);
    assert_eq!(view.state.phase, Phase::Requested);
    assert!(view.state.journal.is_empty());
}

#[tokio::test]
async fn new_connection_replays_and_conflict_refuses() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    ensure_ready(&fix).await;
    let project = "project-replay";
    let session = "sess-replay";
    boot(&fix, project, session).await;
    let first = apply_begin(&fix, project, session, "ev-replay").await;
    assert_eq!(first.outcome, Some(Outcome::Applied));
    assert_eq!(first.revision, Some(1));
    assert!(first.reducer.as_ref().is_some_and(|reducer| !reducer.database_used));
    let endpoint = endpoint_of(&fix);
    let identity = alice(project);
    let event = begin_event();
    let replay = transition(&TransitionRequest {
        endpoint: &endpoint,
        user: "oo_session_runtime",
        password: &fix.runtime,
        ca_pem: &fix.ca,
        project_id: project,
        session_id: session,
        identity: &identity,
        expected_revision: 0,
        expected_owner_generation: 0,
        event_id: "ev-replay",
        now_ms: 1_000,
        event: &event,
    })
    .await
    .expect("replay");
    assert_eq!(replay.outcome, Some(Outcome::Idempotent));
    assert_eq!(replay.revision, Some(1));
    let conflict_event = Event::CompleteBind {
        workspace_id: "ws-1".to_owned(),
    };
    let conflict = transition(&TransitionRequest {
        endpoint: &endpoint,
        user: "oo_session_runtime",
        password: &fix.runtime,
        ca_pem: &fix.ca,
        project_id: project,
        session_id: session,
        identity: &identity,
        expected_revision: 0,
        expected_owner_generation: 0,
        event_id: "ev-replay",
        now_ms: 1_100,
        event: &conflict_event,
    })
    .await
    .expect("conflict");
    assert_eq!(conflict.reason, Some(Reason::EventIdConflict));
    let view = fetch(&fix, project, session).await;
    assert_eq!(view.revision, 1);
    assert_eq!(view.journal_len, 1);
}

#[tokio::test]
async fn commit_unknown_reconciles_without_a_second_transition() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    ensure_ready(&fix).await;
    let project = "project-unknown";
    let session = "sess-unknown";
    boot(&fix, project, session).await;
    let endpoint = endpoint_of(&fix);
    let identity = alice(project);
    let event = begin_event();
    let unknown = transition_with_fault(
        &TransitionRequest {
            endpoint: &endpoint,
            user: "oo_session_runtime",
            password: &fix.runtime,
            ca_pem: &fix.ca,
            project_id: project,
            session_id: session,
            identity: &identity,
            expected_revision: 0,
            expected_owner_generation: 0,
            event_id: "ev-unknown",
            now_ms: 1_000,
            event: &event,
        },
        CommitFault::DropCommitResponse,
    )
    .await;
    assert!(matches!(unknown, Err(StoreError::CommitUnknown)));
    let reconciled = reconcile(
        &endpoint,
        "oo_session_runtime",
        &fix.runtime,
        &fix.ca,
        project,
        session,
        "ev-unknown",
    )
    .await
    .expect("reconcile");
    assert_eq!(reconciled.disposition, ReconcileDisposition::Durable);
    assert_eq!(reconciled.revision, Some(1));
    let absent = reconcile(
        &endpoint,
        "oo_session_runtime",
        &fix.runtime,
        &fix.ca,
        project,
        session,
        "ev-not-stored",
    )
    .await
    .expect("absent");
    assert_eq!(absent.disposition, ReconcileDisposition::Absent);
    let replay = transition(&TransitionRequest {
        endpoint: &endpoint,
        user: "oo_session_runtime",
        password: &fix.runtime,
        ca_pem: &fix.ca,
        project_id: project,
        session_id: session,
        identity: &identity,
        expected_revision: 0,
        expected_owner_generation: 0,
        event_id: "ev-unknown",
        now_ms: 1_000,
        event: &event,
    })
    .await
    .expect("idempotent after unknown");
    assert_eq!(replay.outcome, Some(Outcome::Idempotent));
    let view = fetch(&fix, project, session).await;
    assert_eq!(view.revision, 1);
    assert_eq!(view.journal_len, 1);
    assert_eq!(view.state.journal[0].event_id, "ev-unknown");
}

#[tokio::test]
async fn owner_lease_takeover_keeps_the_workspace() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    ensure_ready(&fix).await;
    let project = "project-takeover";
    let session = "sess-takeover";
    boot(&fix, project, session).await;
    let endpoint = endpoint_of(&fix);
    let identity = alice(project);
    let begin = begin_event();
    let bound = transition(&TransitionRequest {
        endpoint: &endpoint,
        user: "oo_session_runtime",
        password: &fix.runtime,
        ca_pem: &fix.ca,
        project_id: project,
        session_id: session,
        identity: &identity,
        expected_revision: 0,
        expected_owner_generation: 0,
        event_id: "ev-bind",
        now_ms: 1_000,
        event: &begin,
    })
    .await
    .expect("bind");
    assert_eq!(bound.revision, Some(1));
    assert_eq!(bound.owner_generation, Some(1));
    assert_eq!(bound.phase, Some(Phase::Binding));
    let complete = Event::CompleteBind {
        workspace_id: "ws-1".to_owned(),
    };
    let ready = transition(&TransitionRequest {
        endpoint: &endpoint,
        user: "oo_session_runtime",
        password: &fix.runtime,
        ca_pem: &fix.ca,
        project_id: project,
        session_id: session,
        identity: &identity,
        expected_revision: 1,
        expected_owner_generation: 1,
        event_id: "ev-complete",
        now_ms: 1_100,
        event: &complete,
    })
    .await
    .expect("complete");
    assert_eq!(ready.revision, Some(2));
    assert_eq!(ready.owner_generation, Some(1));
    assert_eq!(ready.phase, Some(Phase::Ready));
    let takeover = Event::Takeover {
        owner_id: "entry-2".to_owned(),
        lease_expires_at_ms: 61_000,
    };
    let taken = transition(&TransitionRequest {
        endpoint: &endpoint,
        user: "oo_session_runtime",
        password: &fix.runtime,
        ca_pem: &fix.ca,
        project_id: project,
        session_id: session,
        identity: &identity,
        expected_revision: 2,
        expected_owner_generation: 1,
        event_id: "ev-takeover",
        now_ms: 31_000,
        event: &takeover,
    })
    .await
    .expect("takeover");
    assert_eq!(taken.revision, Some(3));
    assert_eq!(taken.owner_generation, Some(2));
    assert_eq!(taken.phase, Some(Phase::Releasing));
    let state = taken.reducer.expect("state").state;
    assert_eq!(state.workspace_id.as_deref(), Some("ws-1"));
    assert_eq!(state.owner_id, "entry-2");
    assert_eq!(state.bound, Some(identity));
    assert_eq!(state.failure, Some(session_store::FailureCode::OwnerLost));
}

#[tokio::test]
async fn changed_principal_new_session_and_project_separation() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    ensure_ready(&fix).await;
    let project = "project-sep";
    let session = "sess-sep";
    boot(&fix, project, session).await;
    let endpoint = endpoint_of(&fix);
    let identity = alice(project);
    let begin = begin_event();
    transition(&TransitionRequest {
        endpoint: &endpoint,
        user: "oo_session_runtime",
        password: &fix.runtime,
        ca_pem: &fix.ca,
        project_id: project,
        session_id: session,
        identity: &identity,
        expected_revision: 0,
        expected_owner_generation: 0,
        event_id: "ev-sep-bind",
        now_ms: 1_000,
        event: &begin,
    })
    .await
    .expect("bind");
    let complete = Event::CompleteBind {
        workspace_id: "ws-1".to_owned(),
    };
    transition(&TransitionRequest {
        endpoint: &endpoint,
        user: "oo_session_runtime",
        password: &fix.runtime,
        ca_pem: &fix.ca,
        project_id: project,
        session_id: session,
        identity: &identity,
        expected_revision: 1,
        expected_owner_generation: 1,
        event_id: "ev-sep-complete",
        now_ms: 1_100,
        event: &complete,
    })
    .await
    .expect("complete");
    let mut bob = alice(project);
    bob.principal_id = "bob".to_owned();
    let changed = transition(&TransitionRequest {
        endpoint: &endpoint,
        user: "oo_session_runtime",
        password: &fix.runtime,
        ca_pem: &fix.ca,
        project_id: project,
        session_id: session,
        identity: &bob,
        expected_revision: 2,
        expected_owner_generation: 1,
        event_id: "ev-sep-bob",
        now_ms: 1_200,
        event: &Event::Heartbeat {
            owner_id: "entry-1".to_owned(),
            lease_expires_at_ms: 32_000,
        },
    })
    .await
    .expect("changed principal");
    assert_eq!(changed.reason, Some(Reason::NewSessionRequired));
    let kept = fetch(&fix, project, session).await;
    assert_eq!(kept.revision, 2);
    assert_eq!(kept.state.workspace_id.as_deref(), Some("ws-1"));

    boot(&fix, project, "sess-new").await;
    let fresh = fetch(&fix, project, "sess-new").await;
    assert_eq!(fresh.revision, 0);
    assert_eq!(fetch(&fix, project, session).await.revision, 2);

    let other = "project-other";
    boot(&fix, other, session).await;
    let isolated = fetch(&fix, other, session).await;
    assert_eq!(isolated.revision, 0);
    assert_eq!(fetch(&fix, project, session).await.revision, 2);
}

#[tokio::test]
async fn project_key_and_principal_are_not_authentication() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    ensure_ready(&fix).await;
    let project = "project-auth";
    let session = "sess-auth";
    boot(&fix, project, session).await;
    let endpoint = endpoint_of(&fix);
    let mut foreign = alice("project-other");
    let event = begin_event();
    let mismatch = transition(&TransitionRequest {
        endpoint: &endpoint,
        user: "oo_session_runtime",
        password: &fix.runtime,
        ca_pem: &fix.ca,
        project_id: project,
        session_id: session,
        identity: &foreign,
        expected_revision: 0,
        expected_owner_generation: 0,
        event_id: "ev-mismatch",
        now_ms: 1_000,
        event: &event,
    })
    .await;
    assert!(matches!(mismatch, Err(StoreError::ProjectKeyMismatch)));
    foreign.project_id = project.to_owned();
    foreign.principal_id = "oo_session_runtime".to_owned();
    let principal = transition(&TransitionRequest {
        endpoint: &endpoint,
        user: "oo_session_runtime",
        password: &fix.runtime,
        ca_pem: &fix.ca,
        project_id: project,
        session_id: session,
        identity: &foreign,
        expected_revision: 0,
        expected_owner_generation: 0,
        event_id: "ev-principal",
        now_ms: 1_000,
        event: &event,
    })
    .await;
    assert!(matches!(principal, Err(StoreError::PrincipalNotAuthentication)));
    let admin_call = transition(&TransitionRequest {
        endpoint: &endpoint,
        user: "oo_session_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: project,
        session_id: session,
        identity: &alice(project),
        expected_revision: 0,
        expected_owner_generation: 0,
        event_id: "ev-admin",
        now_ms: 1_000,
        event: &event,
    })
    .await;
    assert!(matches!(admin_call, Err(StoreError::LoginRole)));
    let view = fetch(&fix, project, session).await;
    assert_eq!(view.revision, 0);
    assert!(view.state.journal.is_empty());
}

#[tokio::test]
async fn column_json_mismatch_is_not_repaired() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    ensure_ready(&fix).await;
    let project = "project-consistency";
    let session = "sess-consistency";
    boot(&fix, project, session).await;
    let applied = apply_begin(&fix, project, session, "ev-consistency").await;
    assert_eq!(applied.revision, Some(1));
    let admin = connect_as(&fix, "oo_session_admin", &fix.admin, &fix.ca).await;
    admin
        .execute(
            "UPDATE oo_session_control.session_head SET revision = revision + 1 WHERE project_id = $1 AND session_id = $2",
            &[&project, &session],
        )
        .await
        .expect("damage revision");
    let endpoint = endpoint_of(&fix);
    let read = get(&endpoint, "oo_session_runtime", &fix.runtime, &fix.ca, project, session).await;
    let identity = alice(project);
    let event = Event::CompleteBind {
        workspace_id: "ws-1".to_owned(),
    };
    let write = transition(&TransitionRequest {
        endpoint: &endpoint,
        user: "oo_session_runtime",
        password: &fix.runtime,
        ca_pem: &fix.ca,
        project_id: project,
        session_id: session,
        identity: &identity,
        expected_revision: 1,
        expected_owner_generation: 1,
        event_id: "ev-consistency-2",
        now_ms: 1_100,
        event: &event,
    })
    .await;
    let damaged: String = admin
        .query_one(
            "SELECT revision::text FROM oo_session_control.session_head WHERE project_id = $1 AND session_id = $2",
            &[&project, &session],
        )
        .await
        .expect("read damaged")
        .get(0);
    admin
        .execute(
            "UPDATE oo_session_control.session_head SET revision = revision - 1 WHERE project_id = $1 AND session_id = $2",
            &[&project, &session],
        )
        .await
        .expect("restore revision");
    assert!(matches!(read, Err(StoreError::Consistency)));
    assert!(matches!(write, Err(StoreError::Consistency)));
    assert_eq!(damaged, "2");
    let view = fetch(&fix, project, session).await;
    assert_eq!(view.revision, 1);
    assert_eq!(view.journal_len, 1);
}

#[tokio::test]
async fn cli_acks_refusals_and_hides_the_password() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    ensure_ready(&fix).await;
    let project = "project-cli";
    let session = "sess-cli";
    boot(&fix, project, session).await;
    let usage = run_cli(&["extra"], &[]);
    assert_eq!(usage.status, 1);
    assert!(!usage.stdout.contains(&fix.runtime));

    let garbage = run_cli(&[], b"{");
    assert_eq!(garbage.status, 1);
    assert!(garbage.stdout.contains("\"reason\":\"json\""));
    assert!(!garbage.stdout.contains("expected"));

    let mut too_big = vec![b' '; STDIN_MAX_BYTES + 1];
    too_big[0] = b'{';
    let large = run_cli(&[], &too_big);
    assert_eq!(large.status, 1);
    assert!(large.stdout.contains("stdin_too_large"));

    let install_body = cli_body(&fix, "oo_session_admin", &fix.admin, "install", serde_json::json!({}));
    let installed = run_cli(&[], install_body.as_bytes());
    assert_eq!(installed.status, 0, "{}", installed.stdout);
    assert!(installed.stdout.contains("\"disposition\":\"replay\""));
    assert!(!installed.stdout.contains(&fix.admin));
    assert!(!installed.stderr.contains(&fix.admin));

    let transition_body = cli_transition(&fix, project, session, "ev-cli", 0, 0, &begin_event());
    let applied = run_cli(&[], transition_body.as_bytes());
    assert_eq!(applied.status, 0, "{}", applied.stdout);
    assert!(applied.stdout.contains("\"outcome\":\"applied\""));
    assert!(!applied.stdout.contains(&fix.runtime));

    let stale_body = cli_transition(&fix, project, session, "ev-cli-stale", 0, 0, &begin_event());
    let stale = run_cli(&[], stale_body.as_bytes());
    assert_eq!(stale.status, 2, "{}", stale.stdout);
    assert!(stale.stdout.contains("\"reason\":\"stale\""));

    let mut fault = serde_json::from_str::<serde_json::Value>(&transition_body).expect("json");
    fault["fault"] = serde_json::json!("drop_commit_response");
    let rejected = run_cli(&[], fault.to_string().as_bytes());
    assert_eq!(rejected.status, 1);
    assert!(!rejected.stdout.contains(&fix.runtime));
}

fn cli_body(fix: &Fix, user: &str, password: &str, command: &str, extra: serde_json::Value) -> String {
    let mut body = serde_json::json!({
        "contract": CONTRACT,
        "host": fix.host,
        "port": fix.port,
        "database": fix.database,
        "user": user,
        "password": password,
        "ca_pem": String::from_utf8(fix.ca.clone()).expect("pem"),
        "command": command,
    });
    if let (Some(map), Some(extra)) = (body.as_object_mut(), extra.as_object()) {
        for (key, value) in extra {
            map.insert(key.clone(), value.clone());
        }
    }
    body.to_string()
}

fn cli_transition(
    fix: &Fix,
    project_id: &str,
    session_id: &str,
    event_id: &str,
    expected_revision: u64,
    expected_owner_generation: u64,
    event: &Event,
) -> String {
    cli_body(
        fix,
        "oo_session_runtime",
        &fix.runtime,
        "transition",
        serde_json::json!({
            "project_id": project_id,
            "session_id": session_id,
            "identity": alice(project_id),
            "expected_revision": expected_revision,
            "expected_owner_generation": expected_owner_generation,
            "event_id": event_id,
            "now_ms": 1000,
            "event": event,
        }),
    )
}

struct CliOut {
    status: i32,
    stdout: String,
    stderr: String,
}

fn run_cli(args: &[&str], stdin_bytes: &[u8]) -> CliOut {
    let mut child = Command::new(env!("CARGO_BIN_EXE_session-store"))
        .args(args)
        .env_clear()
        .env("HOME", "/tmp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cli");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin.write_all(stdin_bytes).expect("write stdin");
    }
    let output = child.wait_with_output().expect("wait cli");
    CliOut {
        status: output.status.code().unwrap_or(1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    }
}

async fn fetch(fix: &Fix, project_id: &str, session_id: &str) -> session_store::GetReceipt {
    let endpoint = endpoint_of(fix);
    get(
        &endpoint,
        "oo_session_runtime",
        &fix.runtime,
        &fix.ca,
        project_id,
        session_id,
    )
    .await
    .expect("get")
}

#[test]
fn product_does_not_read_the_fixture_directory() {
    let marker = ["/run/oo-a275/", "fixture"].concat();
    assert!(!include_str!("../src/store.rs").contains(&marker));
    assert!(!include_str!("../src/main.rs").contains(&marker));
    assert!(!include_str!("../src/lib.rs").contains(&marker));
    assert!(!include_str!("../src/sql.rs").contains(&marker));
    assert!(!include_str!("../src/tls.rs").contains(&marker));
}
