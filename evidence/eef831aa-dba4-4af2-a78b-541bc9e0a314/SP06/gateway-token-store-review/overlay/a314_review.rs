//! Independent review of gateway-token-store-v1. This module is copied onto a
//! private archive. It does not authenticate an IdP, terminate mTLS, or issue OAuth.
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
    WallSource,
};
use crate::store::{InstallDisposition, LookupRequest, RevokeDisposition};
use crate::token::{draw_32, scrub};
use crate::{
    lookup, lookup_for_test, Endpoint, LookupCache, StoreError, TrustedControlAdapter, CLIENT_DEFAULT_MS, CLIENT_MAX_MS,
    SESSION_ABSOLUTE_MS, SESSION_IDLE_MS,
};

const AUDIENCE: &str = "https://gateway.example/v1";

fn env_required(name: &str) -> String {
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => value,
        _ => panic!("missing {name}"),
    }
}

fn evidence_dir() -> String {
    env_required("OO_A314_EVIDENCE")
}

fn gate() -> &'static Mutex<()> {
    static GATE: OnceLock<Mutex<()>> = OnceLock::new();
    GATE.get_or_init(|| Mutex::new(()))
}

fn require_live() {
    match std::env::var("OO_A314_LIVE") {
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
        let fixture = env_required("OO_A314_FIXTURE");
        Self {
            endpoint: Endpoint {
                host: env_required("OO_A314_HOST"),
                port: 5432,
                database: env_required("OO_A314_DB"),
            },
            ca: read_bytes(&format!("{fixture}/ca.crt")),
            wrong_ca: read_bytes(&format!("{fixture}/wrong.crt")),
            admin_pass: read_secret(&format!("{fixture}/token-admin.pass")),
            runtime_pass: read_secret(&format!("{fixture}/token-runtime.pass")),
            super_pass: read_secret(&format!("{fixture}/admin.pass")),
        }
    }

    fn admin(&self) -> TrustedControlAdapter {
        match TrustedControlAdapter::new(self.endpoint.clone(), "oo_token_admin", &self.admin_pass, self.ca.clone()) {
            Ok(adapter) => adapter,
            Err(err) => panic!("admin adapter was refused: {err}"),
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

fn read_bytes(path: &str) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|_| panic!("fixture file missing"))
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
        Err(err) => panic!("fixture tls setup failed: {err}"),
    };
    let mut config = tokio_postgres::Config::new();
    config.user("oo_fixture_admin");
    config.password(&fixture.super_pass);
    config.host(&fixture.endpoint.host);
    config.port(fixture.endpoint.port);
    config.dbname(&fixture.endpoint.database);
    config.ssl_mode(SslMode::Require);
    config.application_name("oo-a314-observe");
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
        current: tuple(project, "alice", "analyst", Some("box-1"), Some(session)),
        client_ttl_ms: None,
    }
}

fn client_grant(project: &str, ttl: Option<i64>) -> IssueGrant {
    IssueGrant {
        kind: TokenKind::Client,
        current: tuple(project, "alice", "analyst", None, None),
        client_ttl_ms: ttl,
    }
}

fn tuple(project: &str, principal: &str, role: &str, sandbox: Option<&str>, session: Option<&str>) -> CurrentTuple {
    CurrentTuple {
        project_id: project.to_owned(),
        principal_id: principal.to_owned(),
        role: role.to_owned(),
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

fn must_reason(err: &StoreError, reason: &str) {
    assert_eq!(err.reason(), reason, "unexpected failure");
}

async fn issue_at(admin: &TrustedControlAdapter, grant: IssueGrant, now: i64) -> crate::IssueReceipt {
    match admin.issue_for_test(grant, &FixedWall(now), WaitGate::default()).await {
        Ok(receipt) => receipt,
        Err(err) => panic!("issue failed: {err}"),
    }
}

async fn lookup_at(
    fixture: &Fixture,
    cache: &LookupCache,
    token: &str,
    current: &CurrentTuple,
    audience: &str,
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
        gateway_audience: audience,
        transport,
    };
    lookup_for_test(request, &FixedWall(now)).await
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
    let cache = LookupCache::new();
    match lookup_at(fixture, &cache, token, current, audience, transport, now).await {
        Ok(receipt) => {
            assert!(reason.is_none(), "lookup was accepted, expected {reason:?}");
            flags_clear(receipt.flags);
            assert_eq!(receipt.wall, WallSource::Injected);
        }
        Err(err) => must_reason(&err, reason.unwrap_or("accepted")),
    }
}

async fn row_count(fixture: &Fixture, project: &str) -> i64 {
    let conn = super_conn(fixture).await;
    let row = match conn
        .client
        .query_one("SELECT count(*) FROM oo_token_control.token_record WHERE project_id = $1", &[&project])
        .await
    {
        Ok(row) => row,
        Err(_) => panic!("row count failed"),
    };
    row.try_get(0).unwrap_or(-1)
}

#[tokio::test]
async fn a314_hash_only_random_debug_and_system_clock() {
    let _hold = hold().await;
    assert_eq!(SESSION_IDLE_MS, 30_i64 * 60 * 1000);
    assert_eq!(SESSION_ABSOLUTE_MS, 8_i64 * 60 * 60 * 1000);
    assert_eq!(CLIENT_DEFAULT_MS, 60_i64 * 60 * 1000);
    assert_eq!(CLIENT_MAX_MS, 90_i64 * 24 * 60 * 60 * 1000);
    let left = draw_32().expect("draw");
    let right = draw_32().expect("draw");
    assert_eq!(left.len(), 32);
    assert!(left.iter().any(|byte| *byte != 0), "sample was all zeroes");
    assert!(right != left, "two draws matched");
    let fixture = Fixture::load();
    let admin = fixture.admin();
    reset(&fixture, &admin).await;
    let issued = issue_at(&admin, session_grant("proj-a314-hash", "sess-hash"), 10_000).await;
    assert_eq!(issued.wall, WallSource::Injected);
    flags_clear(issued.flags);
    assert_eq!(issued.hash.len(), 64);
    assert!(issued.hash.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
    let debug = format!("{issued:?}");
    let mut revealed = issued.token.reveal_once();
    assert!(revealed.starts_with("oost_"), "session prefix missing");
    assert_eq!(revealed.len(), 48);
    assert!(!debug.contains("oost_"), "debug leaked a token prefix");
    assert!(!debug.contains(&revealed), "debug leaked the plaintext");
    let cache = LookupCache::new();
    assert!(cache.expiry_nanos(&revealed).expect("cache").is_none(), "plaintext was a cache key");
    let (kind, hashed) = crate::token_hash(&revealed).expect("hash");
    assert_eq!(kind, TokenKind::Session);
    assert_eq!(hashed, issued.hash);
    let conn = super_conn(&fixture).await;
    let stored: String = conn
        .client
        .query_one("SELECT token_sha256 FROM oo_token_control.token_record WHERE token_sha256 = $1", &[&issued.hash])
        .await
        .expect("hash row")
        .get(0);
    assert_eq!(stored, issued.hash);
    let prefixed: i64 = conn
        .client
        .query_one(
            "SELECT count(*) FROM oo_token_control.token_record WHERE position('oost_' in token_sha256) > 0 OR position('ooct_' in token_sha256) > 0",
            &[],
        )
        .await
        .expect("prefix scan")
        .get(0);
    assert_eq!(prefixed, 0);
    let found: i32 = conn
        .client
        .query_one(
            "SELECT COALESCE(MAX(position($1 in token_sha256)),0) FROM oo_token_control.token_record",
            &[&revealed],
        )
        .await
        .expect("plaintext scan")
        .get(0);
    assert_eq!(found, 0, "plaintext was stored");
    scrub(&mut revealed);
    let before = SystemWall.now_ms().expect("clock");
    let production = match admin.issue(session_grant("proj-a314-hash", "sess-system")).await {
        Ok(receipt) => receipt,
        Err(err) => panic!("production issue failed: {err}"),
    };
    let after = SystemWall.now_ms().expect("clock");
    assert_eq!(production.wall, WallSource::System);
    assert!(production.issued_at_ms >= before && production.issued_at_ms <= after, "production issue did not read SystemTime");
    let mut production_token = production.token.reveal_once();
    let current = tuple("proj-a314-hash", "alice", "analyst", Some("box-1"), Some("sess-system"));
    let transport = egress("sess-system");
    let request = LookupRequest {
        endpoint: &fixture.endpoint,
        user: "oo_token_runtime",
        password: &fixture.runtime_pass,
        ca_pem: &fixture.ca,
        cache: &cache,
        presented_token: &production_token,
        current: &current,
        gateway_audience: AUDIENCE,
        transport: &transport,
    };
    let looked = lookup(request).await.expect("production lookup");
    assert_eq!(looked.wall, WallSource::System);
    flags_clear(looked.flags);
    scrub(&mut production_token);
    let body = "distinct_draws=true\nquality_claim=false\nproduction_wall=system\ninjected_wall=injected\n";
    fs::write(format!("{}/randomness.txt", evidence_dir()), body).expect("randomness record");
}

#[tokio::test]
async fn a314_tuple_audience_role_claims_and_declared_transport() {
    let _hold = hold().await;
    let fixture = Fixture::load();
    let admin = fixture.admin();
    reset(&fixture, &admin).await;
    let now = 20_000_i64;
    let issued = issue_at(&admin, session_grant("proj-a314-tuple", "sess-tuple"), now).await;
    flags_clear(issued.flags);
    assert!(!issued.flags.mtls_authenticated, "issue claimed an mTLS decision");
    let mut token = issued.token.reveal_once();
    let current = tuple("proj-a314-tuple", "alice", "analyst", Some("box-1"), Some("sess-tuple"));
    expect_lookup(&fixture, &token, &current, AUDIENCE, &egress("sess-tuple"), now, None).await;
    let mut stale = current.clone();
    stale.policy_version = "pol-2".to_owned();
    expect_lookup(&fixture, &token, &stale, AUDIENCE, &egress("sess-tuple"), now, Some("stale")).await;
    expect_lookup(&fixture, &token, &current, "https://other.example/v1", &egress("sess-tuple"), now, Some("audience")).await;
    let mut role = current.clone();
    role.role = "other".to_owned();
    expect_lookup(&fixture, &token, &role, AUDIENCE, &egress("sess-tuple"), now, Some("role")).await;
    let mut claims = current.clone();
    claims.claims = vec![("desk".to_owned(), "b".to_owned())];
    expect_lookup(&fixture, &token, &claims, AUDIENCE, &egress("sess-tuple"), now, Some("claims")).await;
    expect_lookup(&fixture, &token, &current, AUDIENCE, &DeclaredTransport::ExternalClient, now, Some("binding")).await;
    let merged = IssueGrant {
        kind: TokenKind::Session,
        current: tuple("proj-a314-tuple", "alice", "analyst,admin", Some("box-1"), Some("sess-merge")),
        client_ttl_ms: None,
    };
    match admin.issue_for_test(merged, &FixedWall(now), WaitGate::default()).await {
        Err(err) => must_reason(&err, "role_merge"),
        Ok(_) => panic!("a merged role was issued"),
    }
    let mut forged = tuple("proj-a314-tuple", "alice", "analyst", Some("box-1"), Some("sess-forged"));
    forged.claims = vec![("authenticated".to_owned(), "true".to_owned())];
    let forged_grant = IssueGrant { kind: TokenKind::Session, current: forged, client_ttl_ms: None };
    match admin.issue_for_test(forged_grant, &FixedWall(now), WaitGate::default()).await {
        Err(err) => must_reason(&err, "forged_auth_claim"),
        Ok(_) => panic!("a forged auth claim was issued"),
    }
    assert_eq!(row_count(&fixture, "proj-a314-tuple").await, 1, "a refused issue left a row");
    let client = issue_at(&admin, client_grant("proj-a314-client", None), now).await;
    let mut client_token = client.token.reveal_once();
    assert!(client_token.starts_with("ooct_"), "client prefix missing");
    let client_current = tuple("proj-a314-client", "alice", "analyst", None, None);
    expect_lookup(&fixture, &client_token, &client_current, AUDIENCE, &egress("sess-tuple"), now, Some("binding")).await;
    let wrong_cache = LookupCache::new();
    let wrong = LookupRequest {
        endpoint: &fixture.endpoint,
        user: "oo_token_runtime",
        password: &fixture.runtime_pass,
        ca_pem: &fixture.wrong_ca,
        cache: &wrong_cache,
        presented_token: &client_token,
        current: &client_current,
        gateway_audience: AUDIENCE,
        transport: &DeclaredTransport::ExternalClient,
    };
    match lookup_for_test(wrong, &FixedWall(now)).await {
        Err(err) => assert!(matches!(err.reason(), "unavailable" | "tls" | "connect"), "wrong ca was accepted"),
        Ok(_) => panic!("wrong ca was accepted"),
    }
    scrub(&mut token);
    scrub(&mut client_token);
}

#[tokio::test]
async fn a314_session_and_client_deadlines() {
    let _hold = hold().await;
    let fixture = Fixture::load();
    let admin = fixture.admin();
    reset(&fixture, &admin).await;
    let issued_at = 1_000_000_i64;
    let session = issue_at(&admin, session_grant("proj-a314-ttl", "sess-idle"), issued_at).await;
    assert_eq!(session.wall, WallSource::Injected);
    assert_eq!(session.idle_deadline_ms, Some(issued_at + SESSION_IDLE_MS));
    assert_eq!(session.absolute_deadline_ms, issued_at + SESSION_ABSOLUTE_MS);
    let mut token = session.token.reveal_once();
    let current = tuple("proj-a314-ttl", "alice", "analyst", Some("box-1"), Some("sess-idle"));
    expect_lookup(&fixture, &token, &current, AUDIENCE, &egress("sess-idle"), issued_at + SESSION_IDLE_MS, Some("expired")).await;
    scrub(&mut token);
    let live = issue_at(&admin, session_grant("proj-a314-ttl", "sess-idle-live"), issued_at).await;
    let mut live_token = live.token.reveal_once();
    let live_current = tuple("proj-a314-ttl", "alice", "analyst", Some("box-1"), Some("sess-idle-live"));
    expect_lookup(&fixture, &live_token, &live_current, AUDIENCE, &egress("sess-idle-live"), issued_at + SESSION_IDLE_MS - 1, None).await;
    scrub(&mut live_token);
    let absolute = issue_at(&admin, session_grant("proj-a314-ttl", "sess-abs"), issued_at).await;
    let mut absolute_token = absolute.token.reveal_once();
    let absolute_current = tuple("proj-a314-ttl", "alice", "analyst", Some("box-1"), Some("sess-abs"));
    let mut cursor = issued_at;
    loop {
        expect_lookup(&fixture, &absolute_token, &absolute_current, AUDIENCE, &egress("sess-abs"), cursor, None).await;
        let slid = cursor + SESSION_IDLE_MS;
        if slid >= issued_at + SESSION_ABSOLUTE_MS {
            break;
        }
        cursor = slid - 1;
    }
    expect_lookup(
        &fixture,
        &absolute_token,
        &absolute_current,
        AUDIENCE,
        &egress("sess-abs"),
        issued_at + SESSION_ABSOLUTE_MS - 1,
        None,
    )
    .await;
    expect_lookup(
        &fixture,
        &absolute_token,
        &absolute_current,
        AUDIENCE,
        &egress("sess-abs"),
        issued_at + SESSION_ABSOLUTE_MS,
        Some("expired"),
    )
    .await;
    scrub(&mut absolute_token);
    let client = issue_at(&admin, client_grant("proj-a314-ttl", None), issued_at).await;
    assert!(client.idle_deadline_ms.is_none(), "client token stored an idle deadline");
    assert_eq!(client.absolute_deadline_ms, issued_at + CLIENT_DEFAULT_MS);
    let mut client_token = client.token.reveal_once();
    let client_current = tuple("proj-a314-ttl", "alice", "analyst", None, None);
    expect_lookup(
        &fixture,
        &client_token,
        &client_current,
        AUDIENCE,
        &DeclaredTransport::ExternalClient,
        issued_at + CLIENT_DEFAULT_MS - 1,
        None,
    )
    .await;
    expect_lookup(
        &fixture,
        &client_token,
        &client_current,
        AUDIENCE,
        &DeclaredTransport::ExternalClient,
        issued_at + CLIENT_DEFAULT_MS,
        Some("expired"),
    )
    .await;
    scrub(&mut client_token);
    let long = issue_at(&admin, client_grant("proj-a314-ttl", Some(CLIENT_MAX_MS)), issued_at).await;
    assert_eq!(long.absolute_deadline_ms, issued_at + CLIENT_MAX_MS);
    match admin.issue_for_test(client_grant("proj-a314-ttl", Some(CLIENT_MAX_MS + 1)), &FixedWall(issued_at), WaitGate::default()).await {
        Err(err) => must_reason(&err, "input"),
        Ok(_) => panic!("a client ttl past 90 days was issued"),
    }
    match admin.issue_for_test(
        IssueGrant {
            kind: TokenKind::Session,
            current: tuple("proj-a314-ttl", "alice", "analyst", Some("box-1"), Some("sess-ttl")),
            client_ttl_ms: Some(1),
        },
        &FixedWall(issued_at),
        WaitGate::default(),
    )
    .await
    {
        Err(err) => must_reason(&err, "input"),
        Ok(_) => panic!("a session accepted a caller ttl"),
    }
}

#[tokio::test]
async fn a314_cache_revoke_stop_and_restart() {
    let _hold = hold().await;
    let fixture = Fixture::load();
    let admin = fixture.admin();
    reset(&fixture, &admin).await;
    let now = 60_000_i64;
    let issued = issue_at(&admin, session_grant("proj-a314-cache", "sess-cache"), now).await;
    let hash = issued.hash.clone();
    let mut token = issued.token.reveal_once();
    let current = tuple("proj-a314-cache", "alice", "analyst", Some("box-1"), Some("sess-cache"));
    let transport = egress("sess-cache");
    let cache = LookupCache::new();
    let first = lookup_at(&fixture, &cache, &token, &current, AUDIENCE, &transport, now).await.expect("first lookup");
    assert!(first.database_used, "first lookup skipped the database");
    let expiry = cache.expiry_nanos(&hash).expect("expiry").expect("present");
    let second = lookup_at(&fixture, &cache, &token, &current, AUDIENCE, &transport, now).await.expect("cached lookup");
    assert!(!second.database_used, "cache hit used the database");
    assert_eq!(cache.expiry_nanos(&hash).expect("expiry").expect("present"), expiry, "cache hit moved the expiry");
    match admin.revoke(RevokeScope::Token { project_id: "proj-a314-cache".to_owned(), token_sha256: hash.clone() }).await {
        Ok(receipt) => assert_eq!(receipt.disposition, RevokeDisposition::Applied),
        Err(err) => panic!("revoke failed: {err}"),
    }
    let fresh = LookupCache::new();
    match lookup_at(&fixture, &fresh, &token, &current, AUDIENCE, &transport, now).await {
        Err(err) => must_reason(&err, "revoked"),
        Ok(_) => panic!("a fresh lookup accepted a revoked token"),
    }
    let warm = lookup_at(&fixture, &cache, &token, &current, AUDIENCE, &transport, now).await.expect("warm lookup");
    assert!(!warm.database_used, "warm lookup used the database after revoke");
    let phase = format!("{}/phase", evidence_dir());
    fs::create_dir_all(&phase).expect("phase dir");
    fs::write(format!("{phase}/stop.request"), b"stop").expect("stop request");
    let started = Instant::now();
    loop {
        if fs::read(format!("{phase}/stop.done")).is_ok() {
            let remaining = cache.remaining_ms(&hash).expect("remaining").expect("ttl");
            fs::write(format!("{phase}/stop-window.txt"), format!("remaining_ms={remaining}\n")).expect("window");
            assert!(remaining > 0, "stop exceeded the cache window");
            let during = lookup_at(&fixture, &cache, &token, &current, AUDIENCE, &transport, now).await.expect("outage lookup");
            assert!(!during.database_used, "outage lookup used the database");
            assert_eq!(cache.expiry_nanos(&hash).expect("expiry").expect("present"), expiry, "outage moved the expiry");
            let remaining_ms = u64::try_from(remaining).unwrap_or(u64::MAX);
            assert!(remaining_ms <= 5_000, "cache ttl exceeded five seconds");
            tokio::time::sleep(Duration::from_millis(remaining_ms.saturating_add(250))).await;
            match lookup_at(&fixture, &cache, &token, &current, AUDIENCE, &transport, now).await {
                Err(err) => must_reason(&err, "unavailable"),
                Ok(_) => panic!("an expired cache returned a row while postgres was down"),
            }
            assert_eq!(cache.expiry_nanos(&hash).expect("expiry").expect("present"), expiry, "refusal moved the expiry");
            break;
        }
        if started.elapsed() > Duration::from_secs(8) {
            panic!("stop was not observed inside the cache window");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    fs::write(format!("{phase}/start.request"), b"start").expect("start request");
    let restart = Instant::now();
    loop {
        if fs::read(format!("{phase}/start.done")).is_ok() {
            break;
        }
        if restart.elapsed() > Duration::from_secs(30) {
            panic!("restart was not observed");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let mut saw_revoked = false;
    for _ in 0..25 {
        match lookup_at(&fixture, &cache, &token, &current, AUDIENCE, &transport, now).await {
            Err(err) if err.reason() == "revoked" => {
                saw_revoked = true;
                break;
            }
            Err(err) if err.reason() == "unavailable" => tokio::time::sleep(Duration::from_millis(200)).await,
            Err(err) => panic!("restart lookup failed: {err}"),
            Ok(_) => panic!("restart lookup accepted a revoked token"),
        }
    }
    assert!(saw_revoked, "restart lookup did not observe revocation");
    scrub(&mut token);
}

#[tokio::test]
async fn a314_runtime_grants_and_install_drift() {
    let _hold = hold().await;
    let fixture = Fixture::load();
    let admin = fixture.admin();
    reset(&fixture, &admin).await;
    let runtime = runtime_conn(&fixture).await;
    expect_sqlstate(&runtime.client, "INSERT INTO oo_token_control.token_record (token_sha256) VALUES ('abcd')", "42501").await;
    expect_sqlstate(&runtime.client, "UPDATE oo_token_control.token_record SET role_name = 'analyst'", "42501").await;
    expect_sqlstate(&runtime.client, "DELETE FROM oo_token_control.revocation", "42501").await;
    expect_sqlstate(&runtime.client, "SELECT nextval('oo_token_control.revocation_seq')", "42501").await;
    runtime.client.batch_execute("UPDATE oo_token_control.token_record SET last_used_ms = last_used_ms").await.expect("narrow update");
    runtime.client.batch_execute("UPDATE oo_token_control.token_record SET idle_deadline_ms = idle_deadline_ms").await.expect("idle update");
    match admin.install().await {
        Ok(receipt) => assert_eq!(receipt.disposition, InstallDisposition::Replay),
        Err(err) => panic!("replay failed: {err}"),
    }
    let conn = super_conn(&fixture).await;
    exec_ok(&conn.client, "ALTER TABLE oo_token_control.token_record ADD COLUMN drift_extra text").await;
    match admin.install().await {
        Err(StoreError::InstallDrift { check }) => assert_eq!(check, "columns_token_record"),
        other => panic!("column drift was not refused: {other:?}"),
    }
    exec_ok(&conn.client, "ALTER TABLE oo_token_control.token_record DROP COLUMN drift_extra").await;
    exec_ok(&conn.client, "GRANT INSERT ON TABLE oo_token_control.token_record TO oo_token_runtime").await;
    match admin.install().await {
        Err(StoreError::InstallDrift { check }) => assert_eq!(check, "privilege_token_insert"),
        other => panic!("privilege drift was not refused: {other:?}"),
    }
    exec_ok(&conn.client, "REVOKE INSERT ON TABLE oo_token_control.token_record FROM oo_token_runtime").await;
    match admin.install().await {
        Ok(receipt) => assert_eq!(receipt.disposition, InstallDisposition::Replay),
        Err(err) => panic!("drift restore failed: {err}"),
    }
}

async fn runtime_conn(fixture: &Fixture) -> Conn {
    let connector = crate::rustls_connector(&fixture.ca).expect("runtime tls");
    let mut config = tokio_postgres::Config::new();
    config.user("oo_token_runtime");
    config.password(&fixture.runtime_pass);
    config.host(&fixture.endpoint.host);
    config.port(fixture.endpoint.port);
    config.dbname(&fixture.endpoint.database);
    config.ssl_mode(SslMode::Require);
    config.application_name("oo-a314-runtime-probe");
    let (client, connection) = config.connect(connector).await.expect("runtime connect");
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

#[tokio::test]
async fn a314_revoke_scopes_do_not_resurrect() {
    let _hold = hold().await;
    let fixture = Fixture::load();
    let admin = fixture.admin();
    reset(&fixture, &admin).await;
    let now = 40_000_i64;
    let first = issue_at(&admin, session_grant("proj-a314-rev", "sess-a"), now).await;
    let applied = admin
        .revoke(RevokeScope::Token { project_id: "proj-a314-rev".to_owned(), token_sha256: first.hash.clone() })
        .await
        .expect("token revoke");
    let replay = admin
        .revoke(RevokeScope::Token { project_id: "proj-a314-rev".to_owned(), token_sha256: first.hash })
        .await
        .expect("token replay");
    assert_eq!(applied.disposition, RevokeDisposition::Applied);
    assert_eq!(replay.disposition, RevokeDisposition::Replay);
    assert_eq!(applied.seq, replay.seq, "replay advanced the sequence");
    let mut first_token = first.token.reveal_once();
    expect_lookup(&fixture, &first_token, &tuple("proj-a314-rev", "alice", "analyst", Some("box-1"), Some("sess-a")), AUDIENCE, &egress("sess-a"), now, Some("revoked")).await;
    let session = issue_at(&admin, session_grant("proj-a314-rev", "sess-c"), now).await;
    let mut session_token = session.token.reveal_once();
    admin.revoke(RevokeScope::Session { project_id: "proj-a314-rev".to_owned(), session_id: "sess-c".to_owned() }).await.expect("session revoke");
    expect_lookup(&fixture, &session_token, &tuple("proj-a314-rev", "alice", "analyst", Some("box-1"), Some("sess-c")), AUDIENCE, &egress("sess-c"), now, Some("revoked")).await;
    match admin.issue_for_test(session_grant("proj-a314-rev", "sess-c"), &FixedWall(now), WaitGate::default()).await {
        Err(err) => must_reason(&err, "scope_closed"),
        Ok(_) => panic!("a closed session was issued again"),
    }
    let principal = issue_at(&admin, session_grant("proj-a314-rev", "sess-e"), now).await;
    let mut principal_token = principal.token.reveal_once();
    admin.revoke(RevokeScope::Principal { project_id: "proj-a314-rev".to_owned(), principal_id: "alice".to_owned() }).await.expect("principal revoke");
    expect_lookup(&fixture, &principal_token, &tuple("proj-a314-rev", "alice", "analyst", Some("box-1"), Some("sess-e")), AUDIENCE, &egress("sess-e"), now, Some("revoked")).await;
    let renewed = issue_at(&admin, session_grant("proj-a314-rev", "sess-f"), now).await;
    let mut renewed_token = renewed.token.reveal_once();
    expect_lookup(&fixture, &renewed_token, &tuple("proj-a314-rev", "alice", "analyst", Some("box-1"), Some("sess-f")), AUDIENCE, &egress("sess-f"), now, None).await;
    expect_lookup(&fixture, &principal_token, &tuple("proj-a314-rev", "alice", "analyst", Some("box-1"), Some("sess-e")), AUDIENCE, &egress("sess-e"), now, Some("revoked")).await;
    let project = issue_at(&admin, client_grant("proj-a314-rev", None), now).await;
    let mut project_token = project.token.reveal_once();
    let project_revoke = admin.revoke(RevokeScope::Project { project_id: "proj-a314-rev".to_owned() }).await.expect("project revoke");
    let project_replay = admin.revoke(RevokeScope::Project { project_id: "proj-a314-rev".to_owned() }).await.expect("project replay");
    assert_eq!(project_revoke.disposition, RevokeDisposition::Applied);
    assert_eq!(project_replay.disposition, RevokeDisposition::Replay);
    expect_lookup(&fixture, &project_token, &tuple("proj-a314-rev", "alice", "analyst", None, None), AUDIENCE, &DeclaredTransport::ExternalClient, now, Some("revoked")).await;
    let replacement = issue_at(&admin, client_grant("proj-a314-rev", None), now).await;
    let mut replacement_token = replacement.token.reveal_once();
    expect_lookup(&fixture, &replacement_token, &tuple("proj-a314-rev", "alice", "analyst", None, None), AUDIENCE, &DeclaredTransport::ExternalClient, now, None).await;
    expect_lookup(&fixture, &project_token, &tuple("proj-a314-rev", "alice", "analyst", None, None), AUDIENCE, &DeclaredTransport::ExternalClient, now, Some("revoked")).await;
    scrub(&mut first_token);
    scrub(&mut session_token);
    scrub(&mut principal_token);
    scrub(&mut renewed_token);
    scrub(&mut project_token);
    scrub(&mut replacement_token);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a314_race_does_not_resurrect() {
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
    let gate = WaitGate { paused: Some(Arc::clone(&paused)), release: Some(Arc::clone(&release)) };
    let issuer = fixture.admin();
    let revoker = fixture.admin();
    let issue_task = tokio::spawn(async move {
        issuer.issue_for_test(session_grant("proj-a314-race", "sess-race"), &FixedWall(50_000), gate).await
    });
    wait_barrier(&paused).await;
    let revoke_task = tokio::spawn(async move {
        revoker
            .revoke_for_test(
                RevokeScope::Session { project_id: "proj-a314-race".to_owned(), session_id: "sess-race".to_owned() },
                &FixedWall(50_000),
                WaitGate::default(),
            )
            .await
    });
    wait_for_lock(fixture).await;
    wait_barrier(&release).await;
    let issued = issue_task.await.expect("issue task").expect("racing issue");
    let revoked = revoke_task.await.expect("revoke task").expect("racing revoke");
    assert_eq!(revoked.disposition, RevokeDisposition::Applied);
    let mut token = issued.token.reveal_once();
    expect_lookup(fixture, &token, &tuple("proj-a314-race", "alice", "analyst", Some("box-1"), Some("sess-race")), AUDIENCE, &egress("sess-race"), 50_000, Some("revoked")).await;
    scrub(&mut token);
}

async fn race_revoke_then_issue(fixture: &Fixture) {
    let paused = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let gate = WaitGate { paused: Some(Arc::clone(&paused)), release: Some(Arc::clone(&release)) };
    let revoker = fixture.admin();
    let issuer = fixture.admin();
    let revoke_task = tokio::spawn(async move {
        revoker
            .revoke_for_test(
                RevokeScope::Session { project_id: "proj-a314-race".to_owned(), session_id: "sess-closed".to_owned() },
                &FixedWall(50_000),
                gate,
            )
            .await
    });
    wait_barrier(&paused).await;
    let issue_task = tokio::spawn(async move {
        issuer.issue_for_test(session_grant("proj-a314-race", "sess-closed"), &FixedWall(50_000), WaitGate::default()).await
    });
    wait_for_lock(fixture).await;
    wait_barrier(&release).await;
    let revoked = revoke_task.await.expect("revoke task").expect("leading revoke");
    assert_eq!(revoked.disposition, RevokeDisposition::Applied);
    match issue_task.await.expect("issue task") {
        Err(err) => must_reason(&err, "scope_closed"),
        Ok(_) => panic!("a closed session issued a token"),
    }
    assert_eq!(row_count(fixture, "proj-a314-race").await, 1, "scope closure left a second token row");
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
        let count: i64 = conn
            .client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_stat_activity WHERE application_name = 'gateway-token-store' AND wait_event_type = 'Lock'",
                &[],
            )
            .await
            .expect("lock observation")
            .get(0);
        if count >= 1 {
            return;
        }
        if started.elapsed() > Duration::from_secs(8) {
            panic!("advisory lock wait was not observed");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
