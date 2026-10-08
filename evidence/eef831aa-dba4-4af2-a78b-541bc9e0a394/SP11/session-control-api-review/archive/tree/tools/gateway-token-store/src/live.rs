//! Live PostgreSQL proofs. An injected clock does not certify production `issue`.
#![deny(warnings)]

use std::fs;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use tokio::sync::{Barrier, Mutex};
use tokio_postgres::config::SslMode;
use tokio_postgres::Client;
use zeroize::Zeroize;

use crate::model::{
    CurrentTuple, DeclaredTransport, FixedWall, IssueGrant, RevokeScope, SystemWall, TokenKind, WaitGate, WallClock,
};
use crate::store::{InstallDisposition, LookupRequest, RevokeDisposition};
use crate::token::{draw_32, scrub};
use crate::{
    lookup, lookup_for_test, migration_sha256, Endpoint, LookupCache, StoreError, TrustedControlAdapter, CLIENT_MAX_MS,
    SESSION_ABSOLUTE_MS, SESSION_IDLE_MS,
};

const HOST: &str = "oo-a304-pg";
const DB: &str = "oo_a304";
const FIXTURE: &str = "/run/oo-a304/fixture";
const EVIDENCE: &str = "/evidence/linux/proof";
const AUDIENCE: &str = "https://gateway.example/v1";

fn gate() -> &'static Mutex<()> {
    static GATE: OnceLock<Mutex<()>> = OnceLock::new();
    GATE.get_or_init(|| Mutex::new(()))
}

fn require_live() {
    match std::env::var("OO_A304_LIVE") {
        Ok(value) if value == "1" => {}
        _ => panic!("live postgres fixture is required"),
    }
}

struct Fixture {
    endpoint: Endpoint,
    ca: Vec<u8>,
    wrong_ca: Vec<u8>,
    admin_pass: String,
    runtime_pass: String,
    super_pass: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.admin_pass.zeroize();
        self.runtime_pass.zeroize();
        self.super_pass.zeroize();
    }
}

impl Fixture {
    fn load() -> Self {
        Self {
            endpoint: Endpoint {
                host: HOST.to_owned(),
                port: 5432,
                database: DB.to_owned(),
            },
            ca: read_bytes(&format!("{FIXTURE}/ca.crt"), "ca"),
            wrong_ca: read_bytes(&format!("{FIXTURE}/wrong.crt"), "wrong ca"),
            admin_pass: read_secret(&format!("{FIXTURE}/token-admin.pass")),
            runtime_pass: read_secret(&format!("{FIXTURE}/token-runtime.pass")),
            super_pass: read_secret(&format!("{FIXTURE}/admin.pass")),
        }
    }

    fn admin(&self) -> TrustedControlAdapter {
        match TrustedControlAdapter::new(self.endpoint.clone(), "oo_token_admin", &self.admin_pass, self.ca.clone()) {
            Ok(adapter) => adapter,
            Err(_) => panic!("admin adapter was refused"),
        }
    }
}

struct Conn {
    client: Client,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Conn {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn read_bytes(path: &str, label: &str) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|_| panic!("{label} missing"))
}

fn read_secret(path: &str) -> String {
    let text = fs::read_to_string(path).unwrap_or_else(|_| panic!("fixture secret missing"));
    let text = text.trim().to_owned();
    if text.len() != 48 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        panic!("fixture secret is not 48 hex characters");
    }
    text
}

async fn hold() -> tokio::sync::MutexGuard<'static, ()> {
    require_live();
    gate().lock().await
}

async fn super_conn(fixture: &Fixture) -> Conn {
    let connector = match crate::rustls_connector(&fixture.ca) {
        Ok(connector) => connector,
        Err(_) => panic!("fixture tls setup failed"),
    };
    let mut config = tokio_postgres::Config::new();
    config.user("oo_fixture_admin");
    config.password(&fixture.super_pass);
    config.host(HOST);
    config.port(5432);
    config.dbname(DB);
    config.ssl_mode(SslMode::Require);
    config.application_name("oo-a304-observe");
    let (client, connection) = match config.connect(connector).await {
        Ok(pair) => pair,
        Err(_) => panic!("fixture connect failed"),
    };
    let task = tokio::spawn(async move {
        let _ = connection.await;
    });
    Conn { client, task }
}

async fn exec_ok(client: &Client, statement: &str) {
    if client.batch_execute(statement).await.is_err() {
        panic!("fixture statement failed");
    }
}

async fn reset(fixture: &Fixture, admin: &TrustedControlAdapter) {
    let conn = super_conn(fixture).await;
    exec_ok(&conn.client, "DROP SCHEMA IF EXISTS oo_token_control CASCADE").await;
    match admin.install().await {
        Ok(receipt) if receipt.disposition == InstallDisposition::Installed => {}
        Ok(_) => panic!("install disposition was not installed"),
        Err(err) => panic!("install failed: {err}"),
    }
}

fn session_grant(project: &str, session: &str) -> IssueGrant {
    IssueGrant {
        kind: TokenKind::Session,
        current: tuple(project, "alice", Some("box-1"), Some(session)),
        client_ttl_ms: None,
    }
}

fn client_grant(project: &str, ttl: Option<i64>) -> IssueGrant {
    IssueGrant {
        kind: TokenKind::Client,
        current: tuple(project, "alice", None, None),
        client_ttl_ms: ttl,
    }
}

fn tuple(project: &str, principal: &str, sandbox: Option<&str>, session: Option<&str>) -> CurrentTuple {
    CurrentTuple {
        project_id: project.to_owned(),
        principal_id: principal.to_owned(),
        role: "analyst".to_owned(),
        claims: vec![("desk".to_owned(), "a".to_owned())],
        policy_version: "pol-1".to_owned(),
        knowledge_version: "know-1".to_owned(),
        audience_url: AUDIENCE.to_owned(),
        sandbox_id: sandbox.map(str::to_owned),
        session_id: session.map(str::to_owned),
    }
}

fn egress(session: &str) -> DeclaredTransport {
    DeclaredTransport::EgressProxy {
        sandbox_id: "box-1".to_owned(),
        session_id: session.to_owned(),
    }
}

fn flags_clear(flags: crate::ControlFlags) {
    assert!(!flags.auth_provider_used, "auth provider flag was set");
    assert!(!flags.idp_used, "idp flag was set");
    assert!(!flags.mtls_authenticated, "mtls flag was set");
    assert!(!flags.pubsub_used, "pubsub flag was set");
}

async fn lookup_at(
    fixture: &Fixture,
    token: &str,
    current: &CurrentTuple,
    audience: &str,
    transport: &DeclaredTransport,
    now: i64,
) -> Result<crate::LookupReceipt, StoreError> {
    let cache = LookupCache::new();
    let request = LookupRequest {
        endpoint: &fixture.endpoint,
        user: "oo_token_runtime",
        password: &fixture.runtime_pass,
        ca_pem: &fixture.ca,
        cache: &cache,
        presented_token: token,
        current,
        gateway_audience: audience,
        transport,
    };
    lookup_for_test(request, &FixedWall(now)).await
}

fn must_reason(err: StoreError, reason: &str) {
    assert_eq!(err.reason(), reason, "unexpected failure");
}

#[tokio::test]
async fn install_replay_and_drift_fail_closed() {
    let _hold = hold().await;
    let fixture = Fixture::load();
    let admin = fixture.admin();
    await_reset(&fixture, &admin).await;
    match admin.install().await {
        Ok(receipt) => {
            assert_eq!(receipt.disposition, InstallDisposition::Replay);
            flags_clear(receipt.flags);
        }
        Err(err) => panic!("replay failed: {err}"),
    }
    let conn = super_conn(&fixture).await;
    exec_ok(&conn.client, "ALTER TABLE oo_token_control.token_record ADD COLUMN drift_extra text").await;
    expect_drift(admin.install().await, "columns_token_record");
    exec_ok(&conn.client, "ALTER TABLE oo_token_control.token_record DROP COLUMN drift_extra").await;
    exec_ok(
        &conn.client,
        "UPDATE oo_token_control.install_manifest SET sql_sha256 = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'",
    )
    .await;
    expect_drift(admin.install().await, "hash");
    let hash = match migration_sha256() {
        Ok(hash) => hash,
        Err(_) => panic!("migration hash failed"),
    };
    exec_ok(
        &conn.client,
        &format!("UPDATE oo_token_control.install_manifest SET sql_sha256 = '{hash}'"),
    )
    .await;
    exec_ok(&conn.client, "CREATE TABLE oo_token_control.drift_extra (id bigint)").await;
    expect_drift(admin.install().await, "relations");
    exec_ok(&conn.client, "DROP TABLE oo_token_control.drift_extra").await;
    exec_ok(&conn.client, "GRANT INSERT ON TABLE oo_token_control.token_record TO oo_token_runtime").await;
    expect_drift(admin.install().await, "privilege_token_insert");
    exec_ok(&conn.client, "REVOKE INSERT ON TABLE oo_token_control.token_record FROM oo_token_runtime").await;
    match admin.install().await {
        Ok(receipt) => assert_eq!(receipt.disposition, InstallDisposition::Replay),
        Err(err) => panic!("restore replay failed: {err}"),
    }
}

async fn await_reset(fixture: &Fixture, admin: &TrustedControlAdapter) {
    reset(fixture, admin).await;
}

fn expect_drift(result: Result<crate::InstallReceipt, StoreError>, check: &str) {
    match result {
        Err(StoreError::InstallDrift { check: actual }) => assert_eq!(actual, check, "drift check differed"),
        Err(err) => panic!("expected install_drift {check}, got {err}"),
        Ok(_) => panic!("expected install_drift {check}"),
    }
}

#[tokio::test]
async fn issue_stores_the_hash_only_and_samples_randomness() {
    let _hold = hold().await;
    let fixture = Fixture::load();
    let admin = fixture.admin();
    reset(&fixture, &admin).await;
    let clock = FixedWall(10_000);
    let first = match admin.issue_for_test(session_grant("proj-hash", "sess-hash"), &clock, WaitGate::default()).await {
        Ok(receipt) => receipt,
        Err(err) => panic!("issue failed: {err}"),
    };
    let second = match admin.issue_for_test(session_grant("proj-hash", "sess-hash-2"), &clock, WaitGate::default()).await {
        Ok(receipt) => receipt,
        Err(err) => panic!("second issue failed: {err}"),
    };
    assert_ne!(first.hash, second.hash, "two issues stored the same hash");
    assert_eq!(first.wall, crate::WallSource::Injected);
    flags_clear(first.flags);
    let debug = format!("{first:?}");
    assert!(!debug.contains("oost_"), "receipt debug leaked a token prefix");
    let hash = first.hash.clone();
    let mut revealed = first.token.reveal_once();
    assert!(revealed.starts_with("oost_"), "prefix missing");
    assert_eq!(revealed.len(), 48);
    let conn = super_conn(&fixture).await;
    scan_absent(&conn.client, &revealed).await;
    let row = match conn.client.query_one("SELECT token_sha256 FROM oo_token_control.token_record WHERE token_sha256 = $1", &[&hash]).await {
        Ok(row) => row,
        Err(_) => panic!("hash lookup failed"),
    };
    let stored: String = row.try_get(0).unwrap_or_default();
    assert_eq!(stored, hash, "stored identity was not the sha256");
    let prefixed = match conn
        .client
        .query_one(
            "SELECT count(*) FROM oo_token_control.token_record WHERE position('oost_' in token_sha256) > 0 OR position('ooct_' in token_sha256) > 0",
            &[],
        )
        .await
    {
        Ok(row) => row,
        Err(_) => panic!("prefix scan failed"),
    };
    let count: i64 = prefixed.try_get(0).unwrap_or(-1);
    assert_eq!(count, 0, "a stored column contained a token prefix");
    let names = match conn
        .client
        .query("SELECT column_name FROM information_schema.columns WHERE table_schema = 'oo_token_control'", &[])
        .await
    {
        Ok(rows) => rows,
        Err(_) => panic!("column name scan failed"),
    };
    for row in names {
        let name: String = row.try_get(0).unwrap_or_default();
        assert!(name != "token" && name != "raw_token" && name != "plaintext" && name != "jwt", "schema has a raw token column");
    }
    write_randomness_sample();
    scrub(&mut revealed);
}

fn write_randomness_sample() {
    let left = match draw_32() {
        Ok(bytes) => bytes,
        Err(_) => panic!("randomness draw failed"),
    };
    let right = match draw_32() {
        Ok(bytes) => bytes,
        Err(_) => panic!("randomness draw failed"),
    };
    let distinct = left != right && left.iter().any(|byte| *byte != 0) && right.iter().any(|byte| *byte != 0);
    let _ = fs::create_dir_all(EVIDENCE);
    let body = if distinct {
        "distinct_hashes=true\nquality_claim=false\n"
    } else {
        "distinct_hashes=false\nquality_claim=false\n"
    };
    if fs::write(format!("{EVIDENCE}/randomness.txt"), body).is_err() {
        panic!("randomness record failed");
    }
    assert!(distinct, "randomness sample was not two distinct non-zero draws");
}

async fn scan_absent(client: &Client, plaintext: &str) {
    let statement = "SELECT \
COALESCE(MAX(position($1 in token_sha256)),0), \
COALESCE(MAX(position($1 in token_kind)),0), \
COALESCE(MAX(position($1 in project_id)),0), \
COALESCE(MAX(position($1 in principal_id)),0), \
COALESCE(MAX(position($1 in role_name)),0), \
COALESCE(MAX(position($1 in claims_canonical)),0), \
COALESCE(MAX(position($1 in policy_version)),0), \
COALESCE(MAX(position($1 in knowledge_version)),0), \
COALESCE(MAX(position($1 in audience_url)),0), \
COALESCE(MAX(position($1 in sandbox_id)),0), \
COALESCE(MAX(position($1 in session_id)),0), \
COALESCE((SELECT MAX(position($1 in sql_sha256)) FROM oo_token_control.install_manifest),0), \
COALESCE((SELECT MAX(position($1 in scope)) FROM oo_token_control.revocation),0), \
COALESCE((SELECT MAX(position($1 in token_sha256)) FROM oo_token_control.revocation),0) \
FROM oo_token_control.token_record";
    let row = match client.query_one(statement, &[&plaintext]).await {
        Ok(row) => row,
        Err(_) => panic!("schema scan query failed"),
    };
    for index in 0..14 {
        let found: i32 = row.try_get(index).unwrap_or(-1);
        assert_eq!(found, 0, "plaintext found in a stored column");
    }
}

#[tokio::test]
async fn injected_expiry_does_not_certify_the_production_clock() {
    let _hold = hold().await;
    let fixture = Fixture::load();
    let admin = fixture.admin();
    reset(&fixture, &admin).await;
    let issued_at = 1_000_000_i64;
    let clock = FixedWall(issued_at);
    let session = match admin.issue_for_test(session_grant("proj-exp", "sess-idle"), &clock, WaitGate::default()).await {
        Ok(receipt) => receipt,
        Err(err) => panic!("session issue failed: {err}"),
    };
    assert_eq!(session.wall, crate::WallSource::Injected);
    assert_eq!(session.idle_deadline_ms, Some(issued_at + SESSION_IDLE_MS));
    assert_eq!(session.absolute_deadline_ms, issued_at + SESSION_ABSOLUTE_MS);
    let mut token = session.token.reveal_once();
    let current = tuple("proj-exp", "alice", Some("box-1"), Some("sess-idle"));
    let transport = egress("sess-idle");
    match lookup_at(&fixture, &token, &current, AUDIENCE, &transport, issued_at + SESSION_IDLE_MS).await {
        Err(err) => must_reason(err, "expired"),
        Ok(_) => panic!("idle deadline stayed live"),
    }
    scrub(&mut token);
    let live = match admin.issue_for_test(session_grant("proj-exp", "sess-idle-live"), &clock, WaitGate::default()).await {
        Ok(receipt) => receipt,
        Err(err) => panic!("idle boundary issue failed: {err}"),
    };
    let mut live_token = live.token.reveal_once();
    let live_current = tuple("proj-exp", "alice", Some("box-1"), Some("sess-idle-live"));
    match lookup_at(&fixture, &live_token, &live_current, AUDIENCE, &egress("sess-idle-live"), issued_at + SESSION_IDLE_MS - 1).await {
        Ok(receipt) => assert!(receipt.database_used, "boundary lookup skipped the database"),
        Err(err) => panic!("idle boundary was refused: {err}"),
    }
    scrub(&mut live_token);

    let absolute = match admin.issue_for_test(session_grant("proj-exp", "sess-abs"), &clock, WaitGate::default()).await {
        Ok(receipt) => receipt,
        Err(err) => panic!("absolute issue failed: {err}"),
    };
    let mut absolute_token = absolute.token.reveal_once();
    let absolute_current = tuple("proj-exp", "alice", Some("box-1"), Some("sess-abs"));
    let absolute_transport = egress("sess-abs");
    let mut cursor = issued_at;
    loop {
        match lookup_at(&fixture, &absolute_token, &absolute_current, AUDIENCE, &absolute_transport, cursor).await {
            Ok(_) => {}
            Err(err) => panic!("slide lookup failed: {err}"),
        }
        let slid = cursor + SESSION_IDLE_MS;
        if slid >= issued_at + SESSION_ABSOLUTE_MS {
            break;
        }
        cursor = slid - 1;
    }
    match lookup_at(&fixture, &absolute_token, &absolute_current, AUDIENCE, &absolute_transport, issued_at + SESSION_ABSOLUTE_MS - 1).await {
        Ok(_) => {}
        Err(err) => panic!("absolute boundary was refused: {err}"),
    }
    match lookup_at(&fixture, &absolute_token, &absolute_current, AUDIENCE, &absolute_transport, issued_at + SESSION_ABSOLUTE_MS).await {
        Err(err) => must_reason(err, "expired"),
        Ok(_) => panic!("absolute deadline stayed live"),
    }
    scrub(&mut absolute_token);
    client_deadlines(&admin, &fixture, issued_at, &clock).await;
}

async fn client_deadlines(admin: &TrustedControlAdapter, fixture: &Fixture, issued_at: i64, clock: &FixedWall) {
    let client = match admin.issue_for_test(client_grant("proj-exp", None), clock, WaitGate::default()).await {
        Ok(receipt) => receipt,
        Err(err) => panic!("client issue failed: {err}"),
    };
    assert_eq!(client.absolute_deadline_ms, issued_at + 3_600_000);
    assert!(client.idle_deadline_ms.is_none(), "client token stored an idle deadline");
    let mut client_token = client.token.reveal_once();
    let client_current = tuple("proj-exp", "alice", None, None);
    match lookup_at(fixture, &client_token, &client_current, AUDIENCE, &DeclaredTransport::ExternalClient, issued_at + 3_600_000 - 1).await {
        Ok(_) => {}
        Err(err) => panic!("client boundary was refused: {err}"),
    }
    match lookup_at(fixture, &client_token, &client_current, AUDIENCE, &DeclaredTransport::ExternalClient, issued_at + 3_600_000).await {
        Err(err) => must_reason(err, "expired"),
        Ok(_) => panic!("client default deadline stayed live"),
    }
    scrub(&mut client_token);

    let long = match admin.issue_for_test(client_grant("proj-exp", Some(CLIENT_MAX_MS)), clock, WaitGate::default()).await {
        Ok(receipt) => receipt,
        Err(err) => panic!("90 day issue failed: {err}"),
    };
    assert_eq!(long.absolute_deadline_ms, issued_at + CLIENT_MAX_MS);
    let mut long_token = long.token.reveal_once();
    match lookup_at(fixture, &long_token, &client_current, AUDIENCE, &DeclaredTransport::ExternalClient, issued_at + CLIENT_MAX_MS - 1).await {
        Ok(_) => {}
        Err(err) => panic!("90 day boundary was refused: {err}"),
    }
    match lookup_at(fixture, &long_token, &client_current, AUDIENCE, &DeclaredTransport::ExternalClient, issued_at + CLIENT_MAX_MS).await {
        Err(err) => must_reason(err, "expired"),
        Ok(_) => panic!("90 day deadline stayed live"),
    }
    scrub(&mut long_token);
    match admin.issue_for_test(client_grant("proj-exp", Some(CLIENT_MAX_MS + 1)), clock, WaitGate::default()).await {
        Err(err) => must_reason(err, "input"),
        Ok(_) => panic!("90 day plus one millisecond was issued"),
    }
}

#[tokio::test]
async fn production_issue_reads_the_system_clock() {
    let _hold = hold().await;
    let fixture = Fixture::load();
    let admin = fixture.admin();
    reset(&fixture, &admin).await;
    let before = match SystemWall.now_ms() {
        Ok(now) => now,
        Err(_) => panic!("system clock failed"),
    };
    let receipt = match admin.issue(session_grant("proj-clock", "sess-clock")).await {
        Ok(receipt) => receipt,
        Err(err) => panic!("production issue failed: {err}"),
    };
    let after = match SystemWall.now_ms() {
        Ok(now) => now,
        Err(_) => panic!("system clock failed"),
    };
    assert_eq!(receipt.wall, crate::WallSource::System);
    assert!(receipt.issued_at_ms + 5_000 >= before, "issued time was far behind the test clock");
    assert!(receipt.issued_at_ms <= after + 5_000, "issued time was far ahead of the test clock");
    assert_eq!(receipt.absolute_deadline_ms, receipt.issued_at_ms + SESSION_ABSOLUTE_MS);
    assert_eq!(receipt.idle_deadline_ms, Some(receipt.issued_at_ms + SESSION_IDLE_MS));
    flags_clear(receipt.flags);
    let mut token = receipt.token.reveal_once();
    let cache = LookupCache::new();
    let current = tuple("proj-clock", "alice", Some("box-1"), Some("sess-clock"));
    let transport = egress("sess-clock");
    let request = LookupRequest {
        endpoint: &fixture.endpoint,
        user: "oo_token_runtime",
        password: &fixture.runtime_pass,
        ca_pem: &fixture.ca,
        cache: &cache,
        presented_token: &token,
        current: &current,
        gateway_audience: AUDIENCE,
        transport: &transport,
    };
    match lookup(request).await {
        Ok(found) => {
            assert_eq!(found.wall, crate::WallSource::System);
            assert!(found.database_used, "production lookup skipped the database");
            flags_clear(found.flags);
        }
        Err(err) => panic!("production lookup failed: {err}"),
    }
    scrub(&mut token);
}

#[tokio::test]
async fn stale_audience_role_claims_and_binding_are_refused() {
    let _hold = hold().await;
    let fixture = Fixture::load();
    let admin = fixture.admin();
    reset(&fixture, &admin).await;
    let clock = FixedWall(20_000);
    let receipt = match admin.issue_for_test(session_grant("proj-bind", "sess-bind"), &clock, WaitGate::default()).await {
        Ok(receipt) => receipt,
        Err(err) => panic!("issue failed: {err}"),
    };
    let mut token = receipt.token.reveal_once();
    let transport = egress("sess-bind");
    let base = tuple("proj-bind", "alice", Some("box-1"), Some("sess-bind"));
    expect_lookup(&fixture, &token, &base, AUDIENCE, &transport, 20_000, None).await;
    let mut stale_project = base.clone();
    stale_project.project_id = "proj-other".to_owned();
    expect_lookup(&fixture, &token, &stale_project, AUDIENCE, &transport, 20_000, Some("stale")).await;
    let mut stale_principal = base.clone();
    stale_principal.principal_id = "bob".to_owned();
    expect_lookup(&fixture, &token, &stale_principal, AUDIENCE, &transport, 20_000, Some("stale")).await;
    let mut stale_policy = base.clone();
    stale_policy.policy_version = "pol-2".to_owned();
    expect_lookup(&fixture, &token, &stale_policy, AUDIENCE, &transport, 20_000, Some("stale")).await;
    let mut stale_knowledge = base.clone();
    stale_knowledge.knowledge_version = "know-2".to_owned();
    expect_lookup(&fixture, &token, &stale_knowledge, AUDIENCE, &transport, 20_000, Some("stale")).await;
    expect_lookup(&fixture, &token, &base, "https://other.example/v1", &transport, 20_000, Some("audience")).await;
    let mut role = base.clone();
    role.role = "reader".to_owned();
    expect_lookup(&fixture, &token, &role, AUDIENCE, &transport, 20_000, Some("role")).await;
    let mut claims = base.clone();
    claims.claims = vec![("desk".to_owned(), "b".to_owned())];
    expect_lookup(&fixture, &token, &claims, AUDIENCE, &transport, 20_000, Some("claims")).await;
    let mut sandbox = base.clone();
    sandbox.sandbox_id = Some("box-2".to_owned());
    let wrong_box = DeclaredTransport::EgressProxy {
        sandbox_id: "box-2".to_owned(),
        session_id: "sess-bind".to_owned(),
    };
    expect_lookup(&fixture, &token, &sandbox, AUDIENCE, &wrong_box, 20_000, Some("binding")).await;
    expect_lookup(&fixture, &token, &base, AUDIENCE, &DeclaredTransport::ExternalClient, 20_000, Some("binding")).await;

    let client = match admin.issue_for_test(client_grant("proj-bind", None), &clock, WaitGate::default()).await {
        Ok(receipt) => receipt,
        Err(err) => panic!("client issue failed: {err}"),
    };
    let mut client_token = client.token.reveal_once();
    let client_current = tuple("proj-bind", "alice", None, None);
    expect_lookup(&fixture, &client_token, &client_current, AUDIENCE, &transport, 20_000, Some("binding")).await;
    scrub(&mut token);
    scrub(&mut client_token);
}

async fn expect_lookup(
    fixture: &Fixture,
    token: &str,
    current: &CurrentTuple,
    audience: &str,
    transport: &DeclaredTransport,
    now: i64,
    reason: Option<&str>,
) {
    match lookup_at(fixture, token, current, audience, transport, now).await {
        Ok(_) => {
            if reason.is_some() {
                panic!("lookup accepted a refused tuple");
            }
        }
        Err(err) => match reason {
            Some(reason) => must_reason(err, reason),
            None => panic!("lookup failed: {err}"),
        },
    }
}

#[tokio::test]
async fn forged_claims_and_role_merge_never_issue() {
    let _hold = hold().await;
    let fixture = Fixture::load();
    let admin = fixture.admin();
    reset(&fixture, &admin).await;
    let clock = FixedWall(30_000);
    let mut forged = session_grant("proj-forge", "sess-forge");
    forged.current.claims = vec![("mtls".to_owned(), "true".to_owned())];
    match admin.issue_for_test(forged, &clock, WaitGate::default()).await {
        Err(err) => must_reason(err, "forged_auth_claim"),
        Ok(_) => panic!("forged claim was issued"),
    }
    let mut merged = session_grant("proj-forge", "sess-forge");
    merged.current.role = "analyst,admin".to_owned();
    match admin.issue_for_test(merged, &clock, WaitGate::default()).await {
        Err(err) => must_reason(err, "role_merge"),
        Ok(_) => panic!("merged role was issued"),
    }
    let conn = super_conn(&fixture).await;
    let row = match conn.client.query_one("SELECT count(*) FROM oo_token_control.token_record", &[]).await {
        Ok(row) => row,
        Err(_) => panic!("count failed"),
    };
    let count: i64 = row.try_get(0).unwrap_or(-1);
    assert_eq!(count, 0, "a refused issue left a row");
}

#[tokio::test]
async fn revoke_scopes_are_idempotent_and_do_not_resurrect() {
    let _hold = hold().await;
    let fixture = Fixture::load();
    let admin = fixture.admin();
    reset(&fixture, &admin).await;
    let clock = FixedWall(40_000);
    let first = issued(&admin, session_grant("proj-rev", "sess-a"), clock).await;
    let second = issued(&admin, session_grant("proj-rev", "sess-b"), clock).await;
    let again = admin.revoke(RevokeScope::Token { project_id: "proj-rev".to_owned(), token_sha256: first.hash.clone() }).await;
    let replay = admin.revoke(RevokeScope::Token { project_id: "proj-rev".to_owned(), token_sha256: first.hash.clone() }).await;
    let (first_seq, replay_seq) = match (again, replay) {
        (Ok(applied), Ok(replay)) => {
            assert_eq!(applied.disposition, RevokeDisposition::Applied);
            assert_eq!(replay.disposition, RevokeDisposition::Replay);
            flags_clear(applied.flags);
            (applied.seq, replay.seq)
        }
        _ => panic!("token revoke failed"),
    };
    assert_eq!(first_seq, replay_seq, "replay advanced the revocation sequence");
    let other = match admin.revoke(RevokeScope::Token { project_id: "proj-rev".to_owned(), token_sha256: second.hash.clone() }).await {
        Ok(receipt) => receipt,
        Err(err) => panic!("second revoke failed: {err}"),
    };
    assert_eq!(other.seq, first_seq + 1, "a later revoke did not take the next sequence");
    let mut first_token = first.token.reveal_once();
    let mut second_token = second.token.reveal_once();
    let current_a = tuple("proj-rev", "alice", Some("box-1"), Some("sess-a"));
    expect_lookup(&fixture, &first_token, &current_a, AUDIENCE, &egress("sess-a"), 40_000, Some("revoked")).await;

    let session = issued(&admin, session_grant("proj-rev", "sess-c"), clock).await;
    let mut session_token = session.token.reveal_once();
    match admin.revoke(RevokeScope::Session { project_id: "proj-rev".to_owned(), session_id: "sess-c".to_owned() }).await {
        Ok(receipt) => assert_eq!(receipt.disposition, RevokeDisposition::Applied),
        Err(err) => panic!("session revoke failed: {err}"),
    }
    match admin.revoke(RevokeScope::Session { project_id: "proj-rev".to_owned(), session_id: "sess-c".to_owned() }).await {
        Ok(receipt) => assert_eq!(receipt.disposition, RevokeDisposition::Replay),
        Err(err) => panic!("session replay failed: {err}"),
    }
    expect_lookup(&fixture, &session_token, &tuple("proj-rev", "alice", Some("box-1"), Some("sess-c")), AUDIENCE, &egress("sess-c"), 40_000, Some("revoked")).await;
    match admin.issue_for_test(session_grant("proj-rev", "sess-c"), &clock, WaitGate::default()).await {
        Err(err) => must_reason(err, "scope_closed"),
        Ok(_) => panic!("closed session was issued again"),
    }
    let fresh = issued(&admin, session_grant("proj-rev", "sess-d"), clock).await;
    let mut fresh_token = fresh.token.reveal_once();
    expect_lookup(&fixture, &fresh_token, &tuple("proj-rev", "alice", Some("box-1"), Some("sess-d")), AUDIENCE, &egress("sess-d"), 40_000, None).await;
    expect_lookup(&fixture, &session_token, &tuple("proj-rev", "alice", Some("box-1"), Some("sess-c")), AUDIENCE, &egress("sess-c"), 40_000, Some("revoked")).await;

    let principal = issued(&admin, session_grant("proj-rev", "sess-e"), clock).await;
    let mut principal_token = principal.token.reveal_once();
    match admin.revoke(RevokeScope::Principal { project_id: "proj-rev".to_owned(), principal_id: "alice".to_owned() }).await {
        Ok(_) => {}
        Err(err) => panic!("principal revoke failed: {err}"),
    }
    expect_lookup(&fixture, &principal_token, &tuple("proj-rev", "alice", Some("box-1"), Some("sess-e")), AUDIENCE, &egress("sess-e"), 40_000, Some("revoked")).await;
    let renewed = issued(&admin, session_grant("proj-rev", "sess-f"), clock).await;
    let mut renewed_token = renewed.token.reveal_once();
    expect_lookup(&fixture, &renewed_token, &tuple("proj-rev", "alice", Some("box-1"), Some("sess-f")), AUDIENCE, &egress("sess-f"), 40_000, None).await;
    expect_lookup(&fixture, &principal_token, &tuple("proj-rev", "alice", Some("box-1"), Some("sess-e")), AUDIENCE, &egress("sess-e"), 40_000, Some("revoked")).await;

    let project = issued(&admin, client_grant("proj-rev", None), clock).await;
    let mut project_token = project.token.reveal_once();
    match admin.revoke(RevokeScope::Project { project_id: "proj-rev".to_owned() }).await {
        Ok(receipt) => assert_eq!(receipt.disposition, RevokeDisposition::Applied),
        Err(err) => panic!("project revoke failed: {err}"),
    }
    match admin.revoke(RevokeScope::Project { project_id: "proj-rev".to_owned() }).await {
        Ok(receipt) => assert_eq!(receipt.disposition, RevokeDisposition::Replay),
        Err(err) => panic!("project replay failed: {err}"),
    }
    expect_lookup(&fixture, &project_token, &tuple("proj-rev", "alice", None, None), AUDIENCE, &DeclaredTransport::ExternalClient, 40_000, Some("revoked")).await;
    let replacement = issued(&admin, client_grant("proj-rev", None), clock).await;
    let mut replacement_token = replacement.token.reveal_once();
    expect_lookup(&fixture, &replacement_token, &tuple("proj-rev", "alice", None, None), AUDIENCE, &DeclaredTransport::ExternalClient, 40_000, None).await;
    expect_lookup(&fixture, &project_token, &tuple("proj-rev", "alice", None, None), AUDIENCE, &DeclaredTransport::ExternalClient, 40_000, Some("revoked")).await;
    scrub(&mut first_token);
    scrub(&mut second_token);
    scrub(&mut session_token);
    scrub(&mut fresh_token);
    scrub(&mut principal_token);
    scrub(&mut renewed_token);
    scrub(&mut project_token);
    scrub(&mut replacement_token);
}

async fn issued(admin: &TrustedControlAdapter, grant: IssueGrant, clock: FixedWall) -> crate::IssueReceipt {
    match admin.issue_for_test(grant, &clock, WaitGate::default()).await {
        Ok(receipt) => receipt,
        Err(err) => panic!("issue failed: {err}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_issue_and_revoke_cannot_resurrect() {
    let _hold = hold().await;
    let fixture = Fixture::load();
    let admin = fixture.admin();
    reset(&fixture, &admin).await;
    race_issue_then_revoke(&fixture).await;
    race_revoke_then_issue(&fixture).await;
}

async fn race_issue_then_revoke(fixture: &Fixture) {
    let paused = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let gate = WaitGate {
        paused: Some(Arc::clone(&paused)),
        release: Some(Arc::clone(&release)),
    };
    let issuer = fixture.admin();
    let revoker = fixture.admin();
    let issue_task = tokio::spawn(async move {
        issuer.issue_for_test(session_grant("proj-race", "sess-race"), &FixedWall(50_000), gate).await
    });
    wait_barrier(&paused).await;
    let revoke_task = tokio::spawn(async move {
        revoker
            .revoke_for_test(
                RevokeScope::Session {
                    project_id: "proj-race".to_owned(),
                    session_id: "sess-race".to_owned(),
                },
                &FixedWall(50_000),
                WaitGate::default(),
            )
            .await
    });
    wait_for_lock(fixture).await;
    wait_barrier(&release).await;
    let issued = match issue_task.await {
        Ok(Ok(receipt)) => receipt,
        Ok(Err(err)) => panic!("racing issue failed: {err}"),
        Err(_) => panic!("racing issue task panicked"),
    };
    match revoke_task.await {
        Ok(Ok(receipt)) => assert_eq!(receipt.disposition, RevokeDisposition::Applied),
        Ok(Err(err)) => panic!("racing revoke failed: {err}"),
        Err(_) => panic!("racing revoke task panicked"),
    }
    let mut token = issued.token.reveal_once();
    expect_lookup(fixture, &token, &tuple("proj-race", "alice", Some("box-1"), Some("sess-race")), AUDIENCE, &egress("sess-race"), 50_000, Some("revoked")).await;
    scrub(&mut token);
}

async fn race_revoke_then_issue(fixture: &Fixture) {
    let paused = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let gate = WaitGate {
        paused: Some(Arc::clone(&paused)),
        release: Some(Arc::clone(&release)),
    };
    let revoker = fixture.admin();
    let issuer = fixture.admin();
    let revoke_task = tokio::spawn(async move {
        revoker
            .revoke_for_test(
                RevokeScope::Session {
                    project_id: "proj-race".to_owned(),
                    session_id: "sess-closed".to_owned(),
                },
                &FixedWall(50_000),
                gate,
            )
            .await
    });
    wait_barrier(&paused).await;
    let issue_task = tokio::spawn(async move {
        issuer.issue_for_test(session_grant("proj-race", "sess-closed"), &FixedWall(50_000), WaitGate::default()).await
    });
    wait_for_lock(fixture).await;
    wait_barrier(&release).await;
    match revoke_task.await {
        Ok(Ok(receipt)) => assert_eq!(receipt.disposition, RevokeDisposition::Applied),
        Ok(Err(err)) => panic!("leading revoke failed: {err}"),
        Err(_) => panic!("leading revoke task panicked"),
    }
    match issue_task.await {
        Ok(Err(err)) => must_reason(err, "scope_closed"),
        Ok(Ok(_)) => panic!("closed session issued a token"),
        Err(_) => panic!("leading issue task panicked"),
    }
    let conn = super_conn(fixture).await;
    let row = match conn.client.query_one("SELECT count(*) FROM oo_token_control.token_record WHERE session_id = 'sess-closed'", &[]).await {
        Ok(row) => row,
        Err(_) => panic!("closed session count failed"),
    };
    let count: i64 = row.try_get(0).unwrap_or(-1);
    assert_eq!(count, 0, "scope closure left a token row");
}

async fn wait_barrier(barrier: &Barrier) {
    if tokio::time::timeout(Duration::from_secs(8), barrier.wait()).await.is_err() {
        panic!("barrier was not reached");
    }
}

async fn wait_for_lock(fixture: &Fixture) {
    let conn = super_conn(fixture).await;
    let started = Instant::now();
    loop {
        let row = match conn
            .client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_stat_activity WHERE application_name = 'gateway-token-store' AND wait_event_type = 'Lock'",
                &[],
            )
            .await
        {
            Ok(row) => row,
            Err(_) => panic!("lock observation failed"),
        };
        let count: i64 = row.try_get(0).unwrap_or(0);
        if count >= 1 {
            return;
        }
        if started.elapsed() > Duration::from_secs(8) {
            panic!("advisory lock wait was not observed");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn cache_bounds_revocation_and_fails_closed_when_the_database_is_down() {
    let _hold = hold().await;
    let fixture = Fixture::load();
    let admin = fixture.admin();
    reset(&fixture, &admin).await;
    let clock = FixedWall(60_000);
    let issued = issued(&admin, session_grant("proj-cache", "sess-cache"), clock).await;
    let hash = issued.hash.clone();
    let mut token = issued.token.reveal_once();
    let current = tuple("proj-cache", "alice", Some("box-1"), Some("sess-cache"));
    let transport = egress("sess-cache");
    let cache = LookupCache::new();
    let first = lookup_cached(&fixture, &cache, &token, &current, &transport, 60_000).await;
    assert!(first.database_used, "first lookup skipped the database");
    let expiry = match cache.expiry_nanos(&hash) {
        Ok(Some(expiry)) => expiry,
        _ => panic!("cache expiry missing"),
    };
    let used_before = last_used(&fixture, &hash).await;
    let second = lookup_cached(&fixture, &cache, &token, &current, &transport, 60_000).await;
    assert!(!second.database_used, "cache hit used the database");
    flags_clear(second.flags);
    let expiry_after_hit = match cache.expiry_nanos(&hash) {
        Ok(Some(expiry)) => expiry,
        _ => panic!("cache expiry missing after hit"),
    };
    assert_eq!(expiry, expiry_after_hit, "cache hit moved the expiry");
    assert_eq!(used_before, last_used(&fixture, &hash).await, "cache hit slid the database row");
    match admin.revoke(RevokeScope::Token { project_id: "proj-cache".to_owned(), token_sha256: hash.clone() }).await {
        Ok(_) => {}
        Err(err) => panic!("revoke failed: {err}"),
    }
    let fresh = LookupCache::new();
    match lookup_cached_result(&fixture, &fresh, &token, &current, &transport, 60_000).await {
        Err(err) => must_reason(err, "revoked"),
        Ok(_) => panic!("fresh lookup accepted a revoked token"),
    }
    let warm = lookup_cached(&fixture, &cache, &token, &current, &transport, 60_000).await;
    assert!(!warm.database_used, "warm lookup used the database after revoke");
    let _ = fs::create_dir_all(format!("{EVIDENCE}/phase"));
    if fs::write(format!("{EVIDENCE}/phase/stop.request"), b"stop").is_err() {
        panic!("stop request failed");
    }
    wait_phase(&format!("{EVIDENCE}/phase/stop.done"), &cache, &hash).await;
    let during = lookup_cached(&fixture, &cache, &token, &current, &transport, 60_000).await;
    assert!(!during.database_used, "outage lookup used the database");
    let expiry_during = match cache.expiry_nanos(&hash) {
        Ok(Some(expiry)) => expiry,
        _ => panic!("cache expiry missing during outage"),
    };
    assert_eq!(expiry, expiry_during, "outage moved the expiry");
    let remaining = match cache.remaining_ms(&hash) {
        Ok(Some(remaining)) => remaining,
        _ => panic!("remaining ttl missing"),
    };
    let remaining_ms = u64::try_from(remaining).unwrap_or(u64::MAX);
    assert!(remaining_ms <= 5_000, "cache ttl exceeded five seconds");
    tokio::time::sleep(Duration::from_millis(remaining_ms.saturating_add(250))).await;
    match lookup_cached_result(&fixture, &cache, &token, &current, &transport, 60_000).await {
        Err(err) => must_reason(err, "unavailable"),
        Ok(_) => panic!("expired cache returned a row while the database was down"),
    }
    let expiry_after = match cache.expiry_nanos(&hash) {
        Ok(Some(expiry)) => expiry,
        _ => panic!("cache expiry missing after refusal"),
    };
    assert_eq!(expiry, expiry_after, "refusal moved the expiry");
    if fs::write(format!("{EVIDENCE}/phase/start.request"), b"start").is_err() {
        panic!("start request failed");
    }
    wait_file(&format!("{EVIDENCE}/phase/start.done")).await;
    let mut saw_revoked = false;
    for _ in 0..25 {
        match lookup_cached_result(&fixture, &cache, &token, &current, &transport, 60_000).await {
            Err(err) if err.reason() == "revoked" => {
                saw_revoked = true;
                break;
            }
            Err(err) if err.reason() == "unavailable" => {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(err) => panic!("restart lookup failed: {err}"),
            Ok(_) => panic!("restart lookup accepted a revoked token"),
        }
    }
    assert!(saw_revoked, "restart lookup did not observe revocation");
    scrub(&mut token);
}

async fn lookup_cached(
    fixture: &Fixture,
    cache: &LookupCache,
    token: &str,
    current: &CurrentTuple,
    transport: &DeclaredTransport,
    now: i64,
) -> crate::LookupReceipt {
    match lookup_cached_result(fixture, cache, token, current, transport, now).await {
        Ok(receipt) => receipt,
        Err(err) => panic!("lookup failed: {err}"),
    }
}

async fn lookup_cached_result(
    fixture: &Fixture,
    cache: &LookupCache,
    token: &str,
    current: &CurrentTuple,
    transport: &DeclaredTransport,
    now: i64,
) -> Result<crate::LookupReceipt, StoreError> {
    let request = LookupRequest {
        endpoint: &fixture.endpoint,
        user: "oo_token_runtime",
        password: &fixture.runtime_pass,
        ca_pem: &fixture.ca,
        cache,
        presented_token: token,
        current,
        gateway_audience: AUDIENCE,
        transport,
    };
    lookup_for_test(request, &FixedWall(now)).await
}

async fn last_used(fixture: &Fixture, hash: &str) -> i64 {
    let conn = super_conn(fixture).await;
    let row = match conn.client.query_one("SELECT last_used_ms FROM oo_token_control.token_record WHERE token_sha256 = $1", &[&hash]).await {
        Ok(row) => row,
        Err(_) => panic!("last_used read failed"),
    };
    row.try_get(0).unwrap_or(-1)
}

async fn wait_phase(path: &str, cache: &LookupCache, hash: &str) {
    let started = Instant::now();
    loop {
        if fs::read(path).is_ok() {
            let remaining = match cache.remaining_ms(hash) {
                Ok(Some(remaining)) => remaining,
                _ => 0,
            };
            let _ = fs::write(format!("{EVIDENCE}/phase/stop-window.txt"), format!("remaining_ms={remaining}\n"));
            if remaining == 0 {
                panic!("stop_exceeded_cache_window");
            }
            return;
        }
        if started.elapsed() > Duration::from_secs(8) {
            panic!("stop_exceeded_cache_window");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_file(path: &str) {
    let started = Instant::now();
    loop {
        if fs::read(path).is_ok() {
            return;
        }
        if started.elapsed() > Duration::from_secs(30) {
            panic!("restart signal was not observed");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn runtime_grants_and_wrong_callers_are_refused() {
    let _hold = hold().await;
    let fixture = Fixture::load();
    let admin = fixture.admin();
    reset(&fixture, &admin).await;
    let runtime = runtime_conn(&fixture).await;
    expect_sqlstate(&runtime.client, "INSERT INTO oo_token_control.token_record (token_sha256) VALUES ('abcd')", "42501").await;
    expect_sqlstate(&runtime.client, "UPDATE oo_token_control.token_record SET role_name = 'analyst'", "42501").await;
    expect_sqlstate(&runtime.client, "DELETE FROM oo_token_control.revocation", "42501").await;
    expect_sqlstate(&runtime.client, "TRUNCATE oo_token_control.token_record", "42501").await;
    expect_sqlstate(&runtime.client, "SELECT nextval('oo_token_control.revocation_seq')", "42501").await;
    match runtime.client.batch_execute("UPDATE oo_token_control.token_record SET last_used_ms = last_used_ms").await {
        Ok(()) => {}
        Err(err) => {
            let state = err.code().map(|code| code.code().to_owned()).unwrap_or_default();
            let _ = fs::create_dir_all(EVIDENCE);
            let _ = fs::write(format!("{EVIDENCE}/column-update-sqlstate.txt"), format!("sqlstate={state}\n"));
            panic!("column update was refused");
        }
    }
    match TrustedControlAdapter::new(fixture.endpoint.clone(), "oo_token_runtime", &fixture.runtime_pass, fixture.ca.clone()) {
        Err(err) => must_reason(err, "login_role"),
        Ok(_) => panic!("runtime login constructed an issuer"),
    }
    let cache = LookupCache::new();
    let current = tuple("proj-login", "alice", Some("box-1"), Some("sess-login"));
    let transport = egress("sess-login");
    let request = LookupRequest {
        endpoint: &fixture.endpoint,
        user: "oo_token_admin",
        password: &fixture.admin_pass,
        ca_pem: &fixture.ca,
        cache: &cache,
        presented_token: "oost_abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ012",
        current: &current,
        gateway_audience: AUDIENCE,
        transport: &transport,
    };
    match lookup_for_test(request, &FixedWall(1)).await {
        Err(err) => must_reason(err, "login_role"),
        Ok(_) => panic!("admin login was accepted for lookup"),
    }
    let sample = issued(&admin, client_grant("proj-login", None), FixedWall(1)).await;
    let mut sample_token = sample.token.reveal_once();
    let wrong_current = tuple("proj-login", "alice", None, None);
    let wrong = LookupRequest {
        endpoint: &fixture.endpoint,
        user: "oo_token_runtime",
        password: &fixture.runtime_pass,
        ca_pem: &fixture.wrong_ca,
        cache: &cache,
        presented_token: &sample_token,
        current: &wrong_current,
        gateway_audience: AUDIENCE,
        transport: &DeclaredTransport::ExternalClient,
    };
    match lookup_for_test(wrong, &FixedWall(1)).await {
        Err(err) => assert!(matches!(err.reason(), "unavailable" | "tls" | "connect"), "wrong ca was accepted"),
        Ok(_) => panic!("wrong ca was accepted"),
    }
    scrub(&mut sample_token);
}

async fn runtime_conn(fixture: &Fixture) -> Conn {
    let connector = match crate::rustls_connector(&fixture.ca) {
        Ok(connector) => connector,
        Err(_) => panic!("runtime tls setup failed"),
    };
    let mut config = tokio_postgres::Config::new();
    config.user("oo_token_runtime");
    config.password(&fixture.runtime_pass);
    config.host(HOST);
    config.port(5432);
    config.dbname(DB);
    config.ssl_mode(SslMode::Require);
    config.application_name("oo-a304-runtime-probe");
    let (client, connection) = match config.connect(connector).await {
        Ok(pair) => pair,
        Err(_) => panic!("runtime connect failed"),
    };
    let task = tokio::spawn(async move {
        let _ = connection.await;
    });
    Conn { client, task }
}

async fn expect_sqlstate(client: &Client, statement: &str, expected: &str) {
    match client.batch_execute(statement).await {
        Ok(()) => panic!("runtime statement was accepted"),
        Err(err) => {
            let state = err.code().map(|code| code.code().to_owned()).unwrap_or_default();
            assert_eq!(state, expected, "runtime grant was not refused");
        }
    }
}
