//! Create a conversation and one turn through the fixed session store.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio_postgres::config::SslMode;

use gateway_token_store::{
    CurrentTuple, Endpoint as TokenEndpoint, IssueGrant, TokenKind, TrustedControlAdapter,
};
use session_store::{bootstrap, get, transition, BootstrapDisposition, Endpoint, Event, Outcome, SessionState, TransitionRequest};
use session_transition::{identity_ok, is_token, IdentityTuple, Phase, ReleaseRecord};

use crate::config::{self, Binding, Config};
use crate::error::Fault;
use crate::sha256::{hex, sha256};
use crate::sql;
use crate::store::{self, ConversationRow, Db, ResourceRow};

pub struct Tree {
    pub root: PathBuf,
    pub workspace: PathBuf,
    pub checkpoint: PathBuf,
    pub transcript: PathBuf,
    pub outputs: PathBuf,
    pub snapshot: PathBuf,
    pub token_file: PathBuf,
}

pub fn endpoint_of(cfg: &Config) -> Endpoint {
    Endpoint {
        host: cfg.pg.host.clone(),
        port: cfg.pg.port,
        database: cfg.pg.database.clone(),
    }
}

pub fn token_endpoint(endpoint: &Endpoint) -> TokenEndpoint {
    TokenEndpoint {
        host: endpoint.host.clone(),
        port: endpoint.port,
        database: endpoint.database.clone(),
    }
}

pub fn tree(state_dir: &str, slot: &str, session_id: &str) -> Tree {
    let root = PathBuf::from(state_dir).join(config::OWNER_UUID);
    let workspace = root.join("pool").join(slot);
    Tree {
        checkpoint: root.join("checkpoints").join(session_id),
        transcript: workspace.join("transcript.txt"),
        outputs: workspace.join("outputs.txt"),
        snapshot: root.join("snapshots").join(session_id).join("snapshot.bin"),
        token_file: root.join("secrets").join(format!("{session_id}.token")),
        workspace,
        root,
    }
}

pub fn identity_of(binding: &Binding) -> Result<IdentityTuple, Fault> {
    let identity = IdentityTuple {
        project_id: binding.project_id.clone(),
        principal_id: binding.principal_id.clone(),
        role: binding.role.clone(),
        claims_hash: config::claims_hash(&binding.claims)?,
        policy_version: binding.policy_version.clone(),
        knowledge_version: binding.knowledge_version.clone(),
        config_version: binding.config_version.clone(),
    };
    if !identity_ok(&identity) {
        return Err(Fault::Malformed("identity"));
    }
    Ok(identity)
}

pub struct Created {
    pub conversation_id: String,
    pub session_id: String,
    pub revision: u64,
    pub phase: String,
}

pub async fn create_conversation(cfg: &Config, binding: &Binding) -> Result<Created, Fault> {
    let identity = identity_of(binding)?;
    let endpoint = endpoint_of(cfg);
    let conversation_id = random_token("c", 8)?;
    let session_id = random_token("s", 8)?;
    let slot = random_token("p", 8)?;
    let permit_id = random_token("m", 8)?;
    let mapping_id = random_token("n", 8)?;
    let workspace_id = random_token("w", 8)?;
    let event_prefix = random_token("e", 4)?;
    let handle = random_token("h", 8)?;
    let paths = tree(&cfg.state_dir, &slot, &session_id);
    fs::create_dir_all(&paths.workspace).map_err(|_| Fault::Unavailable("workspace"))?;
    fs::create_dir_all(&paths.checkpoint).map_err(|_| Fault::Unavailable("checkpoint"))?;
    fs::create_dir_all(paths.token_file.parent().unwrap_or(Path::new("/"))).map_err(|_| Fault::Unavailable("secrets"))?;
    let db = runtime_db(cfg, &endpoint);
    if store::held_permits(&db, &binding.project_id, &binding.principal_id).await? >= 2 {
        return Err(Fault::Conflict("permit_quota"));
    }
    let empty = requested(&session_id, &conversation_id);
    let boot = bootstrap(
        &endpoint,
        sql::SESSION_ADMIN,
        &cfg.pg.session_admin_password,
        cfg.pg.ca_pem.as_bytes(),
        &binding.project_id,
        &session_id,
        &empty,
    )
    .await
    .map_err(store_fault)?;
    if !matches!(boot.disposition, BootstrapDisposition::Inserted | BootstrapDisposition::Replay) {
        return Err(Fault::Incomplete("bootstrap"));
    }
    apply(cfg, &endpoint, &identity, &session_id, 0, 0, &format!("{event_prefix}-bind"), 10_000, &Event::BeginBind {
        owner_id: "entry".to_owned(),
        lease_expires_at_ms: 40_000,
    })
    .await?;
    let ready = apply(cfg, &endpoint, &identity, &session_id, 1, 1, &format!("{event_prefix}-ready"), 10_100, &Event::CompleteBind {
        workspace_id: workspace_id.clone(),
    })
    .await?;
    let token_hash = issue_token(cfg, &endpoint, binding, &workspace_id, &session_id, &paths.token_file).await?;
    let resource = ResourceRow {
        project_id: binding.project_id.clone(),
        session_id: session_id.clone(),
        conversation_id: conversation_id.clone(),
        principal_id: binding.principal_id.clone(),
        role_name: binding.role.clone(),
        claims_hash: identity.claims_hash.clone(),
        policy_version: binding.policy_version.clone(),
        knowledge_version: binding.knowledge_version.clone(),
        config_version: binding.config_version.clone(),
        workspace_id,
        event_prefix,
        pool_slot: slot,
        permit_id: permit_id.clone(),
        mapping_id,
        token_sha256: token_hash,
        application_name: format!("q{session_id}"),
        backend_pid: None,
        clock_ms: 10_100,
        stage: "ready".to_owned(),
        harness_pid: None,
        claims_json: serde_json::to_string(&binding.claims).map_err(|_| Fault::Malformed("claims"))?,
    };
    let conversation = ConversationRow {
        project_id: binding.project_id.clone(),
        conversation_id: conversation_id.clone(),
        principal_id: binding.principal_id.clone(),
        session_id: session_id.clone(),
    };
    store::insert_created(&db, &conversation, &resource, &permit_id, &handle, "10.88.0.8").await?;
    let seq = store::next_seq(&db, &binding.project_id, &conversation_id).await?;
    store::insert_event(&db, &binding.project_id, &conversation_id, seq, &format!("{}-created", resource.event_prefix), "created", "ready").await?;
    Ok(Created {
        conversation_id,
        session_id,
        revision: ready.revision,
        phase: phase_label(ready.phase),
    })
}

pub struct Turned {
    pub turn_id: String,
    pub revision: u64,
    pub phase: String,
    pub event_seq: i32,
}

pub async fn begin_turn(cfg: &Config, binding: &Binding, conversation_id: &str, turn_id: &str, text: &str) -> Result<Turned, Fault> {
    if !is_token(turn_id) || text.is_empty() || text.len() > 512 || text.contains('\0') {
        return Err(Fault::Malformed("turn"));
    }
    let endpoint = endpoint_of(cfg);
    let db = runtime_db(cfg, &endpoint);
    let conversation = own_conversation(&db, binding, conversation_id).await?;
    let resource = store::load_resource(&db, &conversation.project_id, &conversation.session_id).await?;
    if resource.stage != "ready" {
        return Err(Fault::Conflict("stage"));
    }
    let identity = identity_from_resource(&resource)?;
    let paths = tree(&cfg.state_dir, &resource.pool_slot, &resource.session_id);
    let transcript = format!("turn {turn_id}\n{text}\n");
    let outputs = format!("{{\"turn\":\"{turn_id}\"}}\n");
    write_file(&paths.transcript, transcript.as_bytes())?;
    write_file(&paths.outputs, outputs.as_bytes())?;
    let state = apply(
        cfg,
        &endpoint,
        &identity,
        &resource.session_id,
        2,
        1,
        &format!("{}-turn", resource.event_prefix),
        10_200,
        &Event::BeginTurn { turn_id: turn_id.to_owned() },
    )
    .await?;
    let pid = start_query(cfg, &endpoint, &resource.application_name).await?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    store::set_running(&db, &resource.project_id, &resource.session_id, pid, 10_200).await?;
    let transcript_sha = hash_bytes(transcript.as_bytes())?;
    let outputs_sha = hash_bytes(outputs.as_bytes())?;
    store::insert_output(&db, &resource, "transcript", &transcript_sha, transcript.as_bytes()).await?;
    store::insert_output(&db, &resource, "outputs", &outputs_sha, outputs.as_bytes()).await?;
    let seq = store::next_seq(&db, &resource.project_id, conversation_id).await?;
    store::insert_event(&db, &resource.project_id, conversation_id, seq, &format!("{}-turn", resource.event_prefix), "turn", text).await?;
    Ok(Turned { turn_id: turn_id.to_owned(), revision: state.revision, phase: phase_label(state.phase), event_seq: seq })
}

pub async fn own_conversation(db: &Db<'_>, binding: &Binding, conversation_id: &str) -> Result<ConversationRow, Fault> {
    let Some(row) = store::load_conversation(db, conversation_id).await? else {
        return Err(Fault::NotFound);
    };
    if row.principal_id != binding.principal_id || row.project_id != binding.project_id {
        return Err(Fault::Forbidden("not_owner"));
    }
    Ok(row)
}

pub(crate) fn runtime_db<'a>(cfg: &'a Config, endpoint: &'a Endpoint) -> Db<'a> {
    Db {
        endpoint,
        user: sql::API_RUNTIME,
        password: &cfg.pg.api_runtime_password,
        ca_pem: cfg.pg.ca_pem.as_bytes(),
    }
}

pub(crate) fn identity_from_resource(resource: &ResourceRow) -> Result<IdentityTuple, Fault> {
    let identity = IdentityTuple {
        project_id: resource.project_id.clone(),
        principal_id: resource.principal_id.clone(),
        role: resource.role_name.clone(),
        claims_hash: resource.claims_hash.clone(),
        policy_version: resource.policy_version.clone(),
        knowledge_version: resource.knowledge_version.clone(),
        config_version: resource.config_version.clone(),
    };
    if !identity_ok(&identity) {
        return Err(Fault::Incomplete("identity"));
    }
    Ok(identity)
}

pub(crate) struct Applied {
    pub revision: u64,
    pub phase: Phase,
}

pub(crate) fn phase_label(phase: Phase) -> String {
    serde_json::to_value(phase)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

pub(crate) async fn load_head(cfg: &Config, project_id: &str, session_id: &str) -> Result<SessionState, Fault> {
    let endpoint = endpoint_of(cfg);
    let receipt = get(
        &endpoint,
        sql::SESSION_RUNTIME,
        &cfg.pg.session_runtime_password,
        cfg.pg.ca_pem.as_bytes(),
        project_id,
        session_id,
    )
    .await
    .map_err(store_fault)?;
    Ok(receipt.state)
}

pub(crate) async fn apply(
    cfg: &Config,
    endpoint: &Endpoint,
    identity: &IdentityTuple,
    session_id: &str,
    expected_revision: u64,
    expected_generation: u64,
    event_id: &str,
    now_ms: u64,
    event: &Event,
) -> Result<Applied, Fault> {
    let receipt = transition(&TransitionRequest {
        endpoint,
        user: sql::SESSION_RUNTIME,
        password: &cfg.pg.session_runtime_password,
        ca_pem: cfg.pg.ca_pem.as_bytes(),
        project_id: &identity.project_id,
        session_id,
        identity,
        expected_revision,
        expected_owner_generation: expected_generation,
        event_id,
        now_ms,
        event,
    })
    .await
    .map_err(store_fault)?;
    if !matches!(receipt.outcome, Some(Outcome::Applied | Outcome::Idempotent)) {
        return Err(Fault::Incomplete(reason_static(receipt.reason)));
    }
    let reducer = receipt.reducer.ok_or(Fault::Incomplete("reducer_missing"))?;
    if receipt.physical_release_certified || reducer.physical_release_certified {
        return Err(Fault::Incomplete("unexpected_certification"));
    }
    Ok(Applied {
        revision: reducer.state.revision,
        phase: reducer.state.phase,
    })
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

async fn issue_token(cfg: &Config, endpoint: &Endpoint, binding: &Binding, workspace_id: &str, session_id: &str, token_file: &Path) -> Result<String, Fault> {
    let adapter = TrustedControlAdapter::new(
        token_endpoint(endpoint),
        sql::TOKEN_ADMIN,
        cfg.pg.token_admin_password.clone(),
        cfg.pg.ca_pem.as_bytes().to_vec(),
    )
    .map_err(|_| Fault::Unavailable("token_login"))?;
    let grant = IssueGrant {
        kind: TokenKind::Session,
        current: CurrentTuple {
            project_id: binding.project_id.clone(),
            principal_id: binding.principal_id.clone(),
            role: binding.role.clone(),
            claims: binding.claims.clone(),
            policy_version: binding.policy_version.clone(),
            knowledge_version: binding.knowledge_version.clone(),
            audience_url: cfg.audience_url.clone(),
            sandbox_id: Some(workspace_id.to_owned()),
            session_id: Some(session_id.to_owned()),
        },
        client_ttl_ms: None,
    };
    let issued = adapter.issue(grant).await.map_err(|_| Fault::Unavailable("token_issue"))?;
    let plain = issued.token.reveal_once();
    let mut file = OpenOptions::new().create_new(true).write(true).mode(0o600).open(token_file).map_err(|_| Fault::Unavailable("token_file"))?;
    file.write_all(plain.as_bytes()).map_err(|_| Fault::Unavailable("token_file"))?;
    file.sync_all().map_err(|_| Fault::Unavailable("token_file"))?;
    Ok(issued.hash)
}

async fn start_query(cfg: &Config, endpoint: &Endpoint, application_name: &str) -> Result<i32, Fault> {
    if !is_token(application_name) {
        return Err(Fault::Malformed("application_name"));
    }
    let tls = session_store::rustls_connector(cfg.pg.ca_pem.as_bytes()).map_err(|_| Fault::Unavailable("tls"))?;
    let mut config = tokio_postgres::Config::new();
    config.user(sql::QUERY_ROLE);
    config.password(&cfg.pg.query_password);
    config.host(&endpoint.host);
    config.port(endpoint.port);
    config.dbname(&endpoint.database);
    config.ssl_mode(SslMode::Require);
    config.application_name(application_name);
    let (client, connection) = config.connect(tls).await.map_err(|_| Fault::Unavailable("query_connect"))?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let pid: i32 = client.query_one("SELECT pg_catalog.pg_backend_pid()", &[]).await.map_err(|_| Fault::Unavailable("query"))?.get(0);
    tokio::spawn(async move {
        let _ = client.batch_execute("SELECT pg_catalog.pg_sleep(30)").await;
    });
    Ok(pid)
}

pub(crate) fn random_hex(nbytes: usize) -> Result<String, Fault> {
    let mut buf = vec![0u8; nbytes];
    let mut file = File::open("/dev/urandom").map_err(|_| Fault::Unavailable("random"))?;
    std::io::Read::read_exact(&mut file, &mut buf).map_err(|_| Fault::Unavailable("random"))?;
    Ok(hex(&buf))
}

pub(crate) fn random_token(prefix: &str, nbytes: usize) -> Result<String, Fault> {
    let mut buf = vec![0u8; nbytes];
    let mut file = File::open("/dev/urandom").map_err(|_| Fault::Unavailable("random"))?;
    std::io::Read::read_exact(&mut file, &mut buf).map_err(|_| Fault::Unavailable("random"))?;
    let id = format!("{prefix}{}", hex(&buf));
    if !is_token(&id) {
        return Err(Fault::Unavailable("random"));
    }
    Ok(id)
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), Fault> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| Fault::Unavailable("workspace"))?;
    }
    let mut file = File::create(path).map_err(|_| Fault::Unavailable("workspace"))?;
    file.write_all(bytes).map_err(|_| Fault::Unavailable("workspace"))?;
    file.sync_all().map_err(|_| Fault::Unavailable("workspace"))?;
    Ok(())
}

pub(crate) fn hash_bytes(bytes: &[u8]) -> Result<String, Fault> {
    Ok(hex(&sha256(bytes).ok_or(Fault::Malformed("hash"))?))
}

pub(crate) fn store_fault(err: session_store::StoreError) -> Fault {
    match err {
        session_store::StoreError::CommitUnknown => Fault::Unavailable("commit_unknown"),
        session_store::StoreError::Connect | session_store::StoreError::Tls => Fault::Unavailable("store"),
        other => Fault::Incomplete(reason_leak(other.reason())),
    }
}

fn reason_static(reason: Option<session_transition::Reason>) -> &'static str {
    match reason {
        Some(session_transition::Reason::Stale) => "stale",
        Some(session_transition::Reason::Closed) => "closed",
        Some(session_transition::Reason::AlreadyReleasing) => "already_releasing",
        Some(_) => "refused",
        None => "refused",
    }
}

fn reason_leak(reason: &'static str) -> &'static str {
    match reason {
        "commit_unknown" => "commit_unknown",
        "connect" | "tls" => "store",
        _ => "store_refused",
    }
}
