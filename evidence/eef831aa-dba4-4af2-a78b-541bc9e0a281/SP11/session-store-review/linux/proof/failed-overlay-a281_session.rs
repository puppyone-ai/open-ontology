#![forbid(unsafe_code)]

//! Independent session-store oracle. Counts are for this fixture, not the author suite.
//! The operator clock and the identity tuple are the values supplied here.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use session_store::{
    bootstrap, get, install, migration_sha256, reconcile, transition, transition_with_fault, BootstrapDisposition,
    CommitFault, Disposition, Endpoint, Event, FailureCode, IdentityTuple, InstallDisposition, Outcome, Phase,
    Reason, ReconcileDisposition, ReleaseRecord, SessionState, StoreError, TransitionReceipt, TransitionRequest,
    CONTRACT,
};
use session_transition::{transition as reduce, TransitionInput, CONTRACT as REDUCER_CONTRACT};
use tokio_postgres::config::SslMode;
use tokio_postgres::Client;

const FIXTURE: &str = "/run/oo-a281/fixture";
const PROJECT: &str = "proj-a281";
const SESSION: &str = "sess-a281";
const BIND_NOW: u64 = 4_200;
const BIND_LEASE: u64 = 34_200;
const READY_NOW: u64 = 4_900;
const RACE_NOW: u64 = 5_400;
const SECRET_MARKER: &str = "a281-secret-marker";
static GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone)]
struct Fix {
    host: String,
    port: u16,
    database: String,
    ca: Vec<u8>,
    admin: String,
    runtime: String,
}

fn load_fix() -> Fix {
    let root = std::path::Path::new(FIXTURE);
    let endpoint = std::fs::read_to_string(root.join("endpoint.txt")).expect("endpoint");
    let mut lines = endpoint.lines();
    Fix {
        host: lines.next().expect("host").to_owned(),
        port: lines.next().expect("port").parse().expect("port"),
        database: lines.next().expect("database").to_owned(),
        ca: std::fs::read(root.join("ca.crt")).expect("ca"),
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

fn operator(project_id: &str) -> IdentityTuple {
    IdentityTuple {
        project_id: project_id.to_owned(),
        principal_id: "operator-cedar".to_owned(),
        role: "analyst".to_owned(),
        claims_hash: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned(),
        policy_version: "pol-a281".to_owned(),
        knowledge_version: "know-a281".to_owned(),
        config_version: "cfg-a281".to_owned(),
    }
}

fn requested(session_id: &str) -> SessionState {
    SessionState {
        session_id: session_id.to_owned(),
        conversation_id: "conv-a281".to_owned(),
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

fn begin_event() -> Event {
    Event::BeginBind {
        owner_id: "entry-cedar".to_owned(),
        lease_expires_at_ms: BIND_LEASE,
    }
}

fn heartbeat_event(lease_expires_at_ms: u64) -> Event {
    Event::Heartbeat {
        owner_id: "entry-cedar".to_owned(),
        lease_expires_at_ms,
    }
}

fn turn_event() -> Event {
    Event::BeginTurn {
        turn_id: "turn-cedar".to_owned(),
    }
}

fn flags_clear(receipt: &TransitionReceipt) {
    assert!(receipt.database_used);
    assert!(!receipt.auth_provider_used);
    assert!(!receipt.runtime_used);
    assert!(!receipt.physical_release_certified);
    let reducer = receipt.reducer.as_ref().expect("reducer");
    assert!(!reducer.database_used);
    assert!(!reducer.auth_provider_used);
    assert!(!reducer.runtime_used);
    assert!(!reducer.physical_release_certified);
}

fn local_fingerprint(project: &str, session: &str, event_id: &str, now_ms: u64, event: &Event) -> String {
    let output = reduce(&TransitionInput {
        contract: REDUCER_CONTRACT.to_owned(),
        state: requested(session),
        identity: operator(project),
        expected_revision: 0,
        expected_owner_generation: 0,
        event_id: event_id.to_owned(),
        now_ms,
        event: event.clone(),
    });
    assert_eq!(output.outcome, Outcome::Applied);
    assert_eq!(output.revision, 1);
    assert_eq!(output.owner_generation, 1);
    assert!(!output.database_used);
    assert!(!output.auth_provider_used);
    assert!(!output.physical_release_certified);
    assert_eq!(output.state.journal.len(), 1);
    output.state.journal[0].canonical_sha256.clone()
}

async fn connect_as(fix: &Fix, user: &str, password: &str) -> Client {
    let tls = session_store::rustls_connector(&fix.ca).expect("connector");
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

async fn ensure(fix: &Fix) {
    let receipt = install(&endpoint_of(fix), "oo_session_admin", &fix.admin, &fix.ca)
        .await
        .expect("install");
    assert!(matches!(
        receipt.disposition,
        InstallDisposition::Installed | InstallDisposition::Replay
    ));
    assert!(receipt.database_used);
    assert!(!receipt.auth_provider_used);
    assert!(!receipt.physical_release_certified);
}

async fn boot(fix: &Fix, project: &str, session: &str) {
    let receipt = bootstrap(
        &endpoint_of(fix),
        "oo_session_admin",
        &fix.admin,
        &fix.ca,
        project,
        session,
        &requested(session),
    )
    .await
    .expect("bootstrap");
    assert_eq!(receipt.disposition, BootstrapDisposition::Inserted);
    assert!(receipt.database_used);
    assert!(!receipt.auth_provider_used);
}

#[allow(clippy::too_many_arguments)]
async fn apply(
    fix: &Fix,
    project: &str,
    session: &str,
    revision: u64,
    generation: u64,
    event_id: &str,
    now_ms: u64,
    event: &Event,
) -> TransitionReceipt {
    let identity = operator(project);
    transition(&TransitionRequest {
        endpoint: &endpoint_of(fix),
        user: "oo_session_runtime",
        password: &fix.runtime,
        ca_pem: &fix.ca,
        project_id: project,
        session_id: session,
        identity: &identity,
        expected_revision: revision,
        expected_owner_generation: generation,
        event_id,
        now_ms,
        event,
    })
    .await
    .expect("transition")
}

async fn fetch(fix: &Fix, project: &str, session: &str) -> session_store::GetReceipt {
    get(
        &endpoint_of(fix),
        "oo_session_runtime",
        &fix.runtime,
        &fix.ca,
        project,
        session,
    )
    .await
    .expect("get")
}

#[tokio::test]
#[allow(clippy::cognitive_complexity)]
async fn fresh_sequence_one_winner_and_exact_fingerprint() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    ensure(&fix).await;
    boot(&fix, PROJECT, SESSION).await;
    let bind_fingerprint = local_fingerprint(PROJECT, SESSION, "ev-a281-bind", BIND_NOW, &begin_event());

    let bound = apply(&fix, PROJECT, SESSION, 0, 0, "ev-a281-bind", BIND_NOW, &begin_event()).await;
    flags_clear(&bound);
    assert_eq!(bound.outcome, Some(Outcome::Applied));
    assert_eq!(bound.revision, Some(1));
    assert_eq!(bound.owner_generation, Some(1));
    assert_eq!(bound.phase, Some(Phase::Binding));

    let admin = connect_as(&fix, "oo_session_admin", &fix.admin).await;
    let server_ms: i64 = admin
        .query_one("SELECT (EXTRACT(EPOCH FROM clock_timestamp()) * 1000)::bigint", &[])
        .await
        .expect("clock")
        .get(0);
    let login: String = admin.query_one("SELECT current_user", &[]).await.expect("user").get(0);
    assert_eq!(login, "oo_session_admin");
    assert!(server_ms > 1_700_000_000_000, "server clock {server_ms}");

    let after_bind = fetch(&fix, PROJECT, SESSION).await;
    assert_eq!(after_bind.revision, 1);
    assert_eq!(after_bind.owner_generation, 1);
    assert_eq!(after_bind.journal_len, 1);
    assert_eq!(after_bind.state.last_now_ms, BIND_NOW);
    assert_eq!(after_bind.state.session_started_at_ms, BIND_NOW);
    assert_eq!(after_bind.state.journal[0].canonical_sha256, bind_fingerprint);
    assert_eq!(
        after_bind.state.bound.as_ref().map(|bound| bound.principal_id.as_str()),
        Some("operator-cedar")
    );
    println!("a281 bind revision=1 generation=1 journal=1 stored_now_ms={BIND_NOW} server_ms={server_ms}");

    let ready = apply(
        &fix,
        PROJECT,
        SESSION,
        1,
        1,
        "ev-a281-ready",
        READY_NOW,
        &Event::CompleteBind {
            workspace_id: "ws-cedar".to_owned(),
        },
    )
    .await;
    assert_eq!(ready.outcome, Some(Outcome::Applied));
    assert_eq!(ready.revision, Some(2));
    assert_eq!(ready.owner_generation, Some(1));
    assert_eq!(ready.phase, Some(Phase::Ready));

    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let left = race(&fix, &barrier, "ev-a281-beat", heartbeat_event(RACE_NOW + 30_000));
    let right = race(&fix, &barrier, "ev-a281-turn", turn_event());
    let (left, right) = tokio::join!(left, right);
    let mut applied = 0;
    let mut stale = 0;
    for result in [left.expect("left task"), right.expect("right task")] {
        let receipt = result.expect("race transition");
        flags_clear(&receipt);
        match (receipt.outcome, receipt.reason) {
            (Some(Outcome::Applied), None) => applied += 1,
            (Some(Outcome::Refused), Some(Reason::Stale)) => stale += 1,
            other => panic!("unexpected race result {other:?}"),
        }
    }
    assert_eq!((applied, stale), (1, 1));
    let raced = fetch(&fix, PROJECT, SESSION).await;
    assert_eq!(raced.revision, 3);
    assert_eq!(raced.owner_generation, 1);
    assert_eq!(raced.journal_len, 3);
    assert_eq!(raced.state.journal[0].event_id, "ev-a281-bind");
    assert_eq!(raced.state.journal[0].canonical_sha256, bind_fingerprint);
    assert_eq!(raced.state.journal[1].event_id, "ev-a281-ready");
    let winner = raced.state.journal[2].event_id.clone();
    assert!(winner == "ev-a281-beat" || winner == "ev-a281-turn");
    let loser = if winner == "ev-a281-beat" { "ev-a281-turn" } else { "ev-a281-beat" };
    assert!(!raced.state.journal.iter().any(|entry| entry.event_id == loser));
    println!("a281 cas applied=1 stale=1 revision=3 journal=3 winner={winner}");

    let winner_event = if winner == "ev-a281-beat" {
        heartbeat_event(RACE_NOW + 30_000)
    } else {
        turn_event()
    };
    let replay = apply(&fix, PROJECT, SESSION, 2, 1, &winner, RACE_NOW, &winner_event).await;
    assert_eq!(replay.disposition, Disposition::Acknowledged);
    assert_eq!(replay.outcome, Some(Outcome::Idempotent));
    assert_eq!(replay.revision, Some(3));
    assert_eq!(replay.owner_generation, Some(1));
    let after_replay = fetch(&fix, PROJECT, SESSION).await;
    assert_eq!(after_replay.journal_len, 3);
    assert_eq!(after_replay.revision, 3);
    println!("a281 retry outcome=idempotent revision=3 journal=3");

    let conflict_event = if winner == "ev-a281-beat" {
        turn_event()
    } else {
        heartbeat_event(RACE_NOW + 30_000)
    };
    let conflict = apply(&fix, PROJECT, SESSION, 3, 1, &winner, RACE_NOW, &conflict_event).await;
    assert_eq!(conflict.disposition, Disposition::Acknowledged);
    assert_eq!(conflict.outcome, Some(Outcome::Refused));
    assert_eq!(conflict.reason, Some(Reason::EventIdConflict));
    let after_conflict = fetch(&fix, PROJECT, SESSION).await;
    assert_eq!(after_conflict.revision, 3);
    assert_eq!(after_conflict.journal_len, 3);
    assert_eq!(after_conflict.state.journal[2].event_id, winner);

    let lease_expiry = after_conflict.state.lease_expires_at_ms;
    let takeover_now = lease_expiry;
    let taken = apply(
        &fix,
        PROJECT,
        SESSION,
        3,
        1,
        "ev-a281-take",
        takeover_now,
        &Event::Takeover {
            owner_id: "entry-cedar-2".to_owned(),
            lease_expires_at_ms: takeover_now + 30_000,
        },
    )
    .await;
    assert_eq!(taken.outcome, Some(Outcome::Applied));
    assert_eq!(taken.revision, Some(4));
    assert_eq!(taken.owner_generation, Some(2));
    assert_eq!(taken.phase, Some(Phase::Releasing));
    let released = fetch(&fix, PROJECT, SESSION).await;
    assert_eq!(released.revision, 4);
    assert_eq!(released.owner_generation, 2);
    assert_eq!(released.journal_len, 4);
    assert_eq!(released.state.phase, Phase::Releasing);
    assert_eq!(released.state.failure, Some(FailureCode::OwnerLost));
    assert_eq!(released.state.workspace_id.as_deref(), Some("ws-cedar"));
    assert_eq!(released.state.last_now_ms, takeover_now);
    assert_eq!(released.state.session_started_at_ms, BIND_NOW);
    assert_eq!(released.state.journal[0].canonical_sha256, bind_fingerprint);
    assert!(takeover_now < 100_000);
    assert!(server_ms > i64::from(takeover_now));

    let stale_generation = apply(&fix, PROJECT, SESSION, 0, 0, "ev-a281-bind", takeover_now, &begin_event()).await;
    assert_eq!(stale_generation.outcome, Some(Outcome::Idempotent));
    assert_eq!(stale_generation.revision, Some(4));
    assert_eq!(stale_generation.owner_generation, Some(2));

    let false_accept = apply(
        &fix,
        PROJECT,
        SESSION,
        4,
        2,
        "ev-a281-bind",
        takeover_now,
        &heartbeat_event(takeover_now + 30_000),
    )
    .await;
    assert_eq!(false_accept.disposition, Disposition::Acknowledged);
    assert_eq!(false_accept.outcome, Some(Outcome::Refused));
    assert_eq!(false_accept.reason, Some(Reason::EventIdConflict));
    let final_row = fetch(&fix, PROJECT, SESSION).await;
    assert_eq!(final_row.revision, 4);
    assert_eq!(final_row.owner_generation, 2);
    assert_eq!(final_row.journal_len, 4);
    assert_eq!(final_row.state.journal[0].event_id, "ev-a281-bind");
    assert_eq!(final_row.state.journal[0].canonical_sha256, bind_fingerprint);
    assert_eq!(
        final_row.state.bound.as_ref().map(|bound| bound.principal_id.as_str()),
        Some("operator-cedar")
    );

    let durable = reconcile(
        &endpoint_of(&fix),
        "oo_session_runtime",
        &fix.runtime,
        &fix.ca,
        PROJECT,
        SESSION,
        "ev-a281-bind",
    )
    .await
    .expect("reconcile bind");
    assert_eq!(durable.disposition, ReconcileDisposition::Durable);
    assert_eq!(durable.revision, Some(4));
    assert_eq!(durable.owner_generation, Some(2));
    assert!(durable.database_used);
    assert!(!durable.auth_provider_used);
    assert!(!durable.physical_release_certified);
    let absent = reconcile(
        &endpoint_of(&fix),
        "oo_session_runtime",
        &fix.runtime,
        &fix.ca,
        PROJECT,
        SESSION,
        "ev-a281-absent",
    )
    .await
    .expect("reconcile absent");
    assert_eq!(absent.disposition, ReconcileDisposition::Absent);
    assert_eq!(absent.revision, None);
    assert_eq!(absent.owner_generation, None);
    println!(
        "a281 conflict reason=event_id_conflict revision=4 generation=2 journal=4 fingerprint_match=1 reconcile=durable"
    );
}

fn race(
    fix: &Fix,
    barrier: &Arc<tokio::sync::Barrier>,
    event_id: &'static str,
    event: Event,
) -> tokio::task::JoinHandle<Result<TransitionReceipt, StoreError>> {
    let fix = fix.clone();
    let barrier = Arc::clone(barrier);
    tokio::spawn(async move {
        barrier.wait().await;
        let identity = operator(PROJECT);
        transition(&TransitionRequest {
            endpoint: &endpoint_of(&fix),
            user: "oo_session_runtime",
            password: &fix.runtime,
            ca_pem: &fix.ca,
            project_id: PROJECT,
            session_id: SESSION,
            identity: &identity,
            expected_revision: 2,
            expected_owner_generation: 1,
            event_id,
            now_ms: RACE_NOW,
            event: &event,
        })
        .await
    })
}

#[tokio::test]
#[allow(clippy::cognitive_complexity)]
async fn row_lock_is_one_backend_transaction() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    ensure(&fix).await;
    boot(&fix, "proj-a281-lock", "sess-a281-lock").await;
    let admin = connect_as(&fix, "oo_session_admin", &fix.admin).await;
    admin.batch_execute("BEGIN").await.expect("begin");
    let locked = admin
        .execute(
            "SELECT revision FROM oo_session_control.session_head WHERE project_id = $1 AND session_id = $2 FOR UPDATE",
            &[&"proj-a281-lock", &"sess-a281-lock"],
        )
        .await
        .expect("lock row");
    assert_eq!(locked, 1);

    let worker = {
        let fix = fix.clone();
        tokio::spawn(async move {
            apply(&fix, "proj-a281-lock", "sess-a281-lock", 0, 0, "ev-a281-lock", BIND_NOW, &begin_event()).await
        })
    };

    let mut saw = false;
    let mut backends = 0i64;
    let mut waiting = 0i64;
    for _ in 0..70 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let row = admin
            .query_one(
                "SELECT \
                   (SELECT count(*) FROM pg_catalog.pg_stat_activity WHERE application_name = 'session-store' AND datname = current_database()), \
                   (SELECT count(*) FROM pg_catalog.pg_stat_activity WHERE application_name = 'session-store' AND datname = current_database() AND wait_event_type = 'Lock')",
                &[],
            )
            .await
            .expect("activity");
        backends = row.get(0);
        waiting = row.get(1);
        if backends == 1 && waiting == 1 {
            saw = true;
            break;
        }
    }
    admin.batch_execute("ROLLBACK").await.expect("release");
    let receipt = worker.await.expect("worker");
    assert!(saw, "session_store_backends={backends} waiting={waiting}");
    assert_eq!(receipt.outcome, Some(Outcome::Applied));
    assert_eq!(receipt.revision, Some(1));
    assert_eq!(receipt.owner_generation, Some(1));
    let view = fetch(&fix, "proj-a281-lock", "sess-a281-lock").await;
    assert_eq!(view.revision, 1);
    assert_eq!(view.journal_len, 1);
    assert_eq!(view.state.last_now_ms, BIND_NOW);
    println!("a281 rowlock session_store_backends=1 waiting=1 revision=1 journal=1");
}

#[tokio::test]
#[allow(clippy::cognitive_complexity)]
async fn rollback_and_lost_commit_reconcile_exact_event() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    ensure(&fix).await;
    let project = "proj-a281-commit";
    let session = "sess-a281-commit";
    boot(&fix, project, session).await;
    let identity = operator(project);
    let event = begin_event();
    let rolled = transition_with_fault(
        &TransitionRequest {
            endpoint: &endpoint_of(&fix),
            user: "oo_session_runtime",
            password: &fix.runtime,
            ca_pem: &fix.ca,
            project_id: project,
            session_id: session,
            identity: &identity,
            expected_revision: 0,
            expected_owner_generation: 0,
            event_id: "ev-a281-rollback",
            now_ms: BIND_NOW,
            event: &event,
        },
        CommitFault::RollBackBeforeCommit,
    )
    .await
    .expect("rollback");
    assert_eq!(rolled.disposition, Disposition::RolledBack);
    assert!(rolled.reducer.is_none());
    assert!(rolled.revision.is_none());
    let untouched = fetch(&fix, project, session).await;
    assert_eq!(untouched.revision, 0);
    assert_eq!(untouched.journal_len, 0);
    assert_eq!(untouched.state.phase, Phase::Requested);

    let exact = local_fingerprint(project, session, "ev-a281-lost", BIND_NOW, &event);
    let unknown = transition_with_fault(
        &TransitionRequest {
            endpoint: &endpoint_of(&fix),
            user: "oo_session_runtime",
            password: &fix.runtime,
            ca_pem: &fix.ca,
            project_id: project,
            session_id: session,
            identity: &identity,
            expected_revision: 0,
            expected_owner_generation: 0,
            event_id: "ev-a281-lost",
            now_ms: BIND_NOW,
            event: &event,
        },
        CommitFault::DropCommitResponse,
    )
    .await;
    assert!(matches!(unknown, Err(StoreError::CommitUnknown)));
    let reconciled = reconcile(
        &endpoint_of(&fix),
        "oo_session_runtime",
        &fix.runtime,
        &fix.ca,
        project,
        session,
        "ev-a281-lost",
    )
    .await
    .expect("reconcile lost");
    assert_eq!(reconciled.disposition, ReconcileDisposition::Durable);
    assert_eq!(reconciled.revision, Some(1));
    assert_eq!(reconciled.owner_generation, Some(1));
    let stored = fetch(&fix, project, session).await;
    assert_eq!(stored.revision, 1);
    assert_eq!(stored.journal_len, 1);
    assert_eq!(stored.state.journal[0].event_id, "ev-a281-lost");
    assert_eq!(stored.state.journal[0].canonical_sha256, exact);
    assert_eq!(stored.state.last_now_ms, BIND_NOW);

    let conflict = apply(
        &fix,
        project,
        session,
        1,
        1,
        "ev-a281-lost",
        READY_NOW,
        &Event::CompleteBind {
            workspace_id: "ws-lost".to_owned(),
        },
    )
    .await;
    assert_eq!(conflict.reason, Some(Reason::EventIdConflict));
    let retry = apply(&fix, project, session, 0, 0, "ev-a281-lost", BIND_NOW, &event).await;
    assert_eq!(retry.outcome, Some(Outcome::Idempotent));
    let final_row = fetch(&fix, project, session).await;
    assert_eq!(final_row.revision, 1);
    assert_eq!(final_row.journal_len, 1);
    assert_eq!(final_row.state.journal[0].canonical_sha256, exact);
    assert_eq!(final_row.state.phase, Phase::Binding);
    println!("a281 commit_unknown reconcile=durable revision=1 journal=1 fingerprint_match=1 conflict_updates=0");
}

#[tokio::test]
#[allow(clippy::cognitive_complexity)]
async fn admin_runtime_split_and_declared_identity() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    ensure(&fix).await;
    let project = "proj-a281-auth";
    let session = "sess-a281-auth";
    let runtime_install = install(&endpoint_of(&fix), "oo_session_runtime", &fix.runtime, &fix.ca).await;
    assert!(matches!(runtime_install, Err(StoreError::LoginRole)));
    boot(&fix, project, session).await;
    boot(&fix, "proj-a281-other", SESSION).await;

    let runtime = connect_as(&fix, "oo_session_runtime", &fix.runtime).await;
    let current: String = runtime.query_one("SELECT current_user", &[]).await.expect("user").get(0);
    assert_eq!(current, "oo_session_runtime");
    assert_ne!(current, "operator-cedar");
    let insert = runtime
        .execute(
            "INSERT INTO oo_session_control.session_head (project_id, session_id, revision, owner_generation, state_json, journal_len) VALUES ('proj-a281-x', 'sess-a281-x', 0, 0, '{}', 0)",
            &[],
        )
        .await;
    let deleted = runtime
        .execute(
            "DELETE FROM oo_session_control.session_head WHERE project_id = $1",
            &[&project],
        )
        .await;
    let manifest = runtime
        .query_one("SELECT sql_sha256 FROM oo_session_control.install_manifest", &[])
        .await;
    assert_eq!(insert.as_ref().err().and_then(pg_code), Some("42501"));
    assert_eq!(deleted.as_ref().err().and_then(pg_code), Some("42501"));
    assert_eq!(manifest.as_ref().err().and_then(pg_code), Some("42501"));

    let identity = operator(project);
    let event = begin_event();
    let admin_transition = transition(&TransitionRequest {
        endpoint: &endpoint_of(&fix),
        user: "oo_session_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: project,
        session_id: session,
        identity: &identity,
        expected_revision: 0,
        expected_owner_generation: 0,
        event_id: "ev-a281-admin",
        now_ms: BIND_NOW,
        event: &event,
    })
    .await;
    assert!(matches!(admin_transition, Err(StoreError::LoginRole)));

    let mut role_principal = identity.clone();
    role_principal.principal_id = "oo_session_runtime".to_owned();
    let principal = transition(&TransitionRequest {
        endpoint: &endpoint_of(&fix),
        user: "oo_session_runtime",
        password: &fix.runtime,
        ca_pem: &fix.ca,
        project_id: project,
        session_id: session,
        identity: &role_principal,
        expected_revision: 0,
        expected_owner_generation: 0,
        event_id: "ev-a281-role",
        now_ms: BIND_NOW,
        event: &event,
    })
    .await;
    assert!(matches!(principal, Err(StoreError::PrincipalNotAuthentication)));

    let mut foreign = identity.clone();
    foreign.project_id = "proj-a281-other".to_owned();
    let mismatch = transition(&TransitionRequest {
        endpoint: &endpoint_of(&fix),
        user: "oo_session_runtime",
        password: &fix.runtime,
        ca_pem: &fix.ca,
        project_id: project,
        session_id: session,
        identity: &foreign,
        expected_revision: 0,
        expected_owner_generation: 0,
        event_id: "ev-a281-foreign",
        now_ms: BIND_NOW,
        event: &event,
    })
    .await;
    assert!(matches!(mismatch, Err(StoreError::ProjectKeyMismatch)));

    let applied = apply(&fix, project, session, 0, 0, "ev-a281-auth", BIND_NOW, &event).await;
    assert_eq!(applied.outcome, Some(Outcome::Applied));
    let mine = fetch(&fix, project, session).await;
    let other = fetch(&fix, "proj-a281-other", SESSION).await;
    assert_eq!(mine.revision, 1);
    assert_eq!(mine.journal_len, 1);
    assert_eq!(mine.state.bound.as_ref().map(|bound| bound.principal_id.as_str()), Some("operator-cedar"));
    assert_eq!(other.revision, 0);
    assert_eq!(other.journal_len, 0);
    assert!(other.state.bound.is_none());
    println!("a281 privileges insert=42501 delete=42501 manifest=42501 other_revision=0 auth_revision=1");
}

#[tokio::test]
async fn stored_fingerprint_conflict_is_not_idempotent() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    ensure(&fix).await;
    let project = "proj-a281-fp";
    let session = "sess-a281-fp";
    boot(&fix, project, session).await;
    let event = begin_event();
    let exact = local_fingerprint(project, session, "ev-a281-fp", BIND_NOW, &event);
    let applied = apply(&fix, project, session, 0, 0, "ev-a281-fp", BIND_NOW, &event).await;
    assert_eq!(applied.revision, Some(1));
    let admin = connect_as(&fix, "oo_session_admin", &fix.admin).await;
    let raw: String = admin
        .query_one(
            "SELECT state_json FROM oo_session_control.session_head WHERE project_id = $1 AND session_id = $2",
            &[&project, &session],
        )
        .await
        .expect("json")
        .get(0);
    let swapped = "0".repeat(64);
    assert_ne!(exact, swapped);
    assert_eq!(raw.matches(&exact).count(), 1);
    let damaged = raw.replacen(&exact, &swapped, 1);
    let updated = admin
        .execute(
            "UPDATE oo_session_control.session_head SET state_json = $3 WHERE project_id = $1 AND session_id = $2",
            &[&project, &session, &damaged],
        )
        .await
        .expect("swap hash");
    assert_eq!(updated, 1);

    let durable = reconcile(
        &endpoint_of(&fix),
        "oo_session_runtime",
        &fix.runtime,
        &fix.ca,
        project,
        session,
        "ev-a281-fp",
    )
    .await
    .expect("reconcile swapped");
    assert_eq!(durable.disposition, ReconcileDisposition::Durable);
    assert_eq!(durable.revision, Some(1));
    assert_eq!(durable.owner_generation, Some(1));

    let conflict = apply(&fix, project, session, 0, 0, "ev-a281-fp", BIND_NOW, &event).await;
    assert_eq!(conflict.outcome, Some(Outcome::Refused));
    assert_eq!(conflict.reason, Some(Reason::EventIdConflict));
    let view = fetch(&fix, project, session).await;
    assert_eq!(view.revision, 1);
    assert_eq!(view.owner_generation, 1);
    assert_eq!(view.journal_len, 1);
    assert_eq!(view.state.journal[0].canonical_sha256, swapped);
    assert_ne!(view.state.journal[0].canonical_sha256, exact);
    println!("a281 swapped_fingerprint reconcile=durable retry=event_id_conflict revision=1 updates=0");
}

#[tokio::test]
async fn generation_column_mismatch_is_not_durable() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    ensure(&fix).await;
    let project = "proj-a281-col";
    let session = "sess-a281-col";
    boot(&fix, project, session).await;
    let applied = apply(&fix, project, session, 0, 0, "ev-a281-col", BIND_NOW, &begin_event()).await;
    assert_eq!(applied.revision, Some(1));
    assert_eq!(applied.owner_generation, Some(1));
    let admin = connect_as(&fix, "oo_session_admin", &fix.admin).await;
    let changed = admin
        .execute(
            "UPDATE oo_session_control.session_head SET owner_generation = 7 WHERE project_id = $1 AND session_id = $2",
            &[&project, &session],
        )
        .await
        .expect("damage generation");
    assert_eq!(changed, 1);
    let reconciled = reconcile(
        &endpoint_of(&fix),
        "oo_session_runtime",
        &fix.runtime,
        &fix.ca,
        project,
        session,
        "ev-a281-col",
    )
    .await;
    let read = get(
        &endpoint_of(&fix),
        "oo_session_runtime",
        &fix.runtime,
        &fix.ca,
        project,
        session,
    )
    .await;
    let identity = operator(project);
    let write = transition(&TransitionRequest {
        endpoint: &endpoint_of(&fix),
        user: "oo_session_runtime",
        password: &fix.runtime,
        ca_pem: &fix.ca,
        project_id: project,
        session_id: session,
        identity: &identity,
        expected_revision: 1,
        expected_owner_generation: 7,
        event_id: "ev-a281-col-2",
        now_ms: READY_NOW,
        event: &Event::CompleteBind {
            workspace_id: "ws-col".to_owned(),
        },
    })
    .await;
    let column: String = admin
        .query_one(
            "SELECT owner_generation::text FROM oo_session_control.session_head WHERE project_id = $1 AND session_id = $2",
            &[&project, &session],
        )
        .await
        .expect("column")
        .get(0);
    let json: String = admin
        .query_one(
            "SELECT state_json FROM oo_session_control.session_head WHERE project_id = $1 AND session_id = $2",
            &[&project, &session],
        )
        .await
        .expect("json")
        .get(0);
    assert!(matches!(reconciled, Err(StoreError::Consistency)));
    assert!(matches!(read, Err(StoreError::Consistency)));
    assert!(matches!(write, Err(StoreError::Consistency)));
    assert_eq!(column, "7");
    assert!(json.contains("\"owner_generation\":1,\"lease_expires_at_ms\""));
    println!("a281 generation_column reconcile=consistency column=7 json_generation=1 repaired=0");
}

fn pg_code(err: &tokio_postgres::Error) -> Option<&str> {
    err.code().map(|code| code.code())
}

#[tokio::test]
#[allow(clippy::cognitive_complexity)]
async fn migration_hash_and_partial_schema_stay_drift() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    let endpoint = endpoint_of(&fix);
    let first = install(&endpoint, "oo_session_admin", &fix.admin, &fix.ca).await.expect("install");
    let hash = migration_sha256().expect("hash");
    assert_eq!(first.sql_sha256, hash);
    let admin = connect_as(&fix, "oo_session_admin", &fix.admin).await;
    let zeros = "0".repeat(64);
    admin
        .execute(
            "UPDATE oo_session_control.install_manifest SET sql_sha256 = $1 WHERE singleton = TRUE",
            &[&zeros],
        )
        .await
        .expect("damage hash");
    let drifted = install(&endpoint, "oo_session_admin", &fix.admin, &fix.ca).await;
    let stored: String = admin
        .query_one("SELECT sql_sha256 FROM oo_session_control.install_manifest WHERE singleton = TRUE", &[])
        .await
        .expect("read hash")
        .get(0);
    assert!(matches!(drifted, Err(StoreError::InstallDrift { check: "hash" })));
    assert_eq!(stored, zeros);
    admin
        .execute(
            "UPDATE oo_session_control.install_manifest SET sql_sha256 = $1 WHERE singleton = TRUE",
            &[&hash],
        )
        .await
        .expect("restore hash");
    let replay = install(&endpoint, "oo_session_admin", &fix.admin, &fix.ca).await.expect("replay");
    assert_eq!(replay.disposition, InstallDisposition::Replay);

    admin.batch_execute("DROP SCHEMA oo_session_control CASCADE").await.expect("drop");
    admin.batch_execute("CREATE SCHEMA oo_session_control").await.expect("partial");
    let partial = install(&endpoint, "oo_session_admin", &fix.admin, &fix.ca).await;
    let tables: i64 = admin
        .query_one(
            "SELECT count(*) FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'oo_session_control' AND c.relname = 'session_head'",
            &[],
        )
        .await
        .expect("count")
        .get(0);
    assert!(matches!(partial, Err(StoreError::InstallDrift { .. })));
    assert_eq!(tables, 0);
    admin.batch_execute("DROP SCHEMA oo_session_control CASCADE").await.expect("drop partial");
    let rebuilt = install(&endpoint, "oo_session_admin", &fix.admin, &fix.ca).await.expect("rebuild");
    assert_eq!(rebuilt.disposition, InstallDisposition::Installed);
    assert_eq!(rebuilt.sql_sha256, hash);
    println!("a281 migration hash_drift=1 partial_tables=0 rebuilt=installed");
}

#[tokio::test]
#[allow(clippy::cognitive_complexity)]
async fn cli_hides_static_secret_marker() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    ensure(&fix).await;
    boot(&fix, "proj-a281-cli", "sess-a281-cli").await;

    let usage = run_cli(&["--help"], &[]);
    assert_eq!(usage.status, 1);
    assert!(usage.stdout.contains("\"reason\":\"usage\""));
    assert!(!usage.stdout.contains(SECRET_MARKER));
    assert!(!usage.stderr.contains(SECRET_MARKER));

    let malformed = serde_json::json!({
        "contract": CONTRACT,
        "host": fix.host,
        "port": fix.port,
        "database": fix.database,
        "user": "oo_session_runtime",
        "password": SECRET_MARKER,
        "ca_pem": "not-a-secret",
        "command": "get",
        "project_id": "proj-a281-cli",
        "session_id": "sess-a281-cli",
        "secret_file": SECRET_MARKER,
    });
    let bad = run_cli(&[], malformed.to_string().as_bytes());
    assert_eq!(bad.status, 1);
    assert!(bad.stdout.contains("\"reason\":\"json\""));
    assert!(!bad.stdout.contains(SECRET_MARKER));
    assert!(!bad.stderr.contains(SECRET_MARKER));
    assert!(!bad.cmdline.contains(SECRET_MARKER));
    assert!(!bad.environ.contains(SECRET_MARKER));

    let wrong = cli_document(&fix, "oo_session_runtime", SECRET_MARKER, "get", serde_json::json!({
        "project_id": "proj-a281-cli",
        "session_id": "sess-a281-cli",
    }));
    let denied = run_cli(&[], wrong.as_bytes());
    assert_eq!(denied.status, 1);
    assert!(!denied.stdout.contains(SECRET_MARKER));
    assert!(!denied.stderr.contains(SECRET_MARKER));
    assert!(!denied.cmdline.contains(SECRET_MARKER));
    assert!(!denied.environ.contains(SECRET_MARKER));
    assert!(!denied.stdout.contains("PRIVATE KEY"));

    let install_body = cli_document(&fix, "oo_session_admin", &fix.admin, "install", serde_json::json!({}));
    let installed = run_cli(&[], install_body.as_bytes());
    assert_eq!(installed.status, 0);
    assert!(installed.stdout.contains("\"disposition\":\"replay\"") || installed.stdout.contains("\"disposition\":\"installed\""));
    assert!(!installed.stdout.contains(&fix.admin));
    assert!(!installed.stderr.contains(&fix.admin));
    assert!(!installed.cmdline.contains(&fix.admin));
    assert!(!installed.environ.contains(&fix.admin));

    let transition_body = cli_document(
        &fix,
        "oo_session_runtime",
        &fix.runtime,
        "transition",
        serde_json::json!({
            "project_id": "proj-a281-cli",
            "session_id": "sess-a281-cli",
            "identity": operator("proj-a281-cli"),
            "expected_revision": 0,
            "expected_owner_generation": 0,
            "event_id": "ev-a281-cli",
            "now_ms": BIND_NOW,
            "event": begin_event(),
        }),
    );
    let applied = run_cli(&[], transition_body.as_bytes());
    assert_eq!(applied.status, 0);
    assert!(applied.stdout.contains("\"outcome\":\"applied\""));
    assert!(applied.stdout.contains("\"revision\":1"));
    assert!(!applied.stdout.contains(&fix.runtime));
    assert!(!applied.stderr.contains(&fix.runtime));
    assert!(!applied.cmdline.contains(&fix.runtime));
    assert!(!applied.environ.contains(&fix.runtime));

    let stale_body = cli_document(
        &fix,
        "oo_session_runtime",
        &fix.runtime,
        "transition",
        serde_json::json!({
            "project_id": "proj-a281-cli",
            "session_id": "sess-a281-cli",
            "identity": operator("proj-a281-cli"),
            "expected_revision": 0,
            "expected_owner_generation": 0,
            "event_id": "ev-a281-cli-stale",
            "now_ms": READY_NOW,
            "event": turn_event(),
        }),
    );
    let stale = run_cli(&[], stale_body.as_bytes());
    assert_eq!(stale.status, 2);
    assert!(stale.stdout.contains("\"reason\":\"stale\""));
    assert!(!stale.stdout.contains(&fix.runtime));
    assert!(!stale.stderr.contains(&fix.runtime));
    println!("a281 cli usage=1 json_marker_leaked=0 connect_marker_leaked=0 applied_revision=1 stale=2");
}

fn cli_document(fix: &Fix, user: &str, password: &str, command: &str, extra: serde_json::Value) -> String {
    let mut body = serde_json::json!({
        "contract": CONTRACT,
        "host": fix.host,
        "port": fix.port,
        "database": fix.database,
        "user": user,
        "password": password,
        "ca_pem": String::from_utf8(fix.ca.clone()).expect("pem text"),
        "command": command,
    });
    if let (Some(map), Some(extra)) = (body.as_object_mut(), extra.as_object()) {
        for (key, value) in extra {
            map.insert(key.clone(), value.clone());
        }
    }
    body.to_string()
}

struct CliOut {
    status: i32,
    stdout: String,
    stderr: String,
    cmdline: String,
    environ: String,
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
    let pid = child.id();
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    let environ = std::fs::read(format!("/proc/{pid}/environ")).unwrap_or_default();
    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin.write_all(stdin_bytes).expect("write stdin");
    }
    let output = child.wait_with_output().expect("wait cli");
    CliOut {
        status: output.status.code().unwrap_or(1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        cmdline: String::from_utf8_lossy(&cmdline).into_owned(),
        environ: String::from_utf8_lossy(&environ).into_owned(),
    }
}
