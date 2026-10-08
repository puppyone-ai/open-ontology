//! Real DedicatedSession, local WAL, collector, and PostgreSQL 17.10.
//! Skips unless `OO_A371_LIVE=1`. Auth stays false. SQL text is not an identity.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use gateway_audit_wal::{inspect, post_event_body, DiskWal, WalConfig};
use oo_gateway::query_executor::{
    Cell, CompletionStatus, DedicatedSession, ExecError, SelectRequest, UNTRUSTED_ADMISSION, UntrustedAdmission,
};
use oo_gateway::sql_guard::{
    GuardError, LiteralTypes, PolicyCatalog, PolicyIdentity, VisibleColumn, VisibleRelation,
};
use oo_gateway::{connect_require_tls, rustls_connector, TlsEndpoint};
use tokio::task::JoinHandle;

const AMOUNT: &str = "123456789012345678";
const WIDE: &str = "12345678901234567890123456789012345678";
const TOKEN: &str = "classified-literal-token";
const SQL: &str = "SELECT entry_id, amount, wide FROM shop.entry ORDER BY entry_id";

fn must_err<T>(result: Result<T, ExecError>, label: &'static str) -> ExecError {
    match result {
        Err(err) => err,
        Ok(_) => panic!("{label}"),
    }
}

#[allow(clippy::cognitive_complexity)]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn dedicated_session_audit_is_durable() {
    if std::env::var("OO_A371_LIVE").ok().as_deref() != Some("1") {
        return;
    }
    assert_eq!(UNTRUSTED_ADMISSION, UntrustedAdmission::Blocked);
    let env = Env::load();
    let mut notes = Vec::new();
    let admin = env.open(&env.admin_user, &env.admin_password, &env.query_db).await;
    privileges(&env).await;
    notes.push("privileges=ok".to_string());

    let spool = PathBuf::from("/tmp/oo-a371-spool");
    reset(&spool);
    let mut session = env.session().await;
    let owned = catalog();
    let wal = DiskWal::open(&spool, WalConfig { max_bytes: 8 * 1024 * 1024 }).unwrap();
    let mut stream = match session.execute_trusted_select(request(&owned, SQL), wal).await {
        Ok(stream) => stream,
        Err(err) => panic!("exec {err}"),
    };
    let early = inspect(&spool).expect("inspect");
    assert_eq!(early.requests.len(), 1);
    assert_eq!(early.requests[0].state, "admitted");
    assert!(early.frames.iter().any(|frame| frame.kind == "admission"));
    assert!(!early.frames.iter().any(|frame| frame.kind == "completion"));
    let (code, text, err) = run(env!("CARGO_BIN_EXE_wal-inspect"), &["replay", "--spool", spool.to_str().unwrap()]);
    assert_eq!(code, 0, "inspect");
    assert!(text.contains("state=admitted"));
    assert!(!text.contains("state=succeeded"));
    env.clean(&text, &err);

    let mut rows = Vec::new();
    loop {
        match stream.next_row().await {
            Ok(Some(row)) => rows.push(row),
            Ok(None) => break,
            Err(err) => panic!("row {err}"),
        }
    }
    assert_eq!(rows.len(), 6);
    expect_row(&rows[0], "1", false, AMOUNT, false, WIDE);
    expect_row(&rows[1], "2", true, AMOUNT, true, WIDE);
    expect_row(&rows[2], "3", false, "1", false, "1");
    expect_row(&rows[3], "4", true, "1", true, "1");
    expect_row(&rows[4], "5", false, "0", false, "0");
    assert!(matches!(rows[5][0], Cell::Int(ref text) if text == "6"));
    assert!(matches!(rows[5][1], Cell::Null));
    assert!(matches!(rows[5][2], Cell::Null));
    session.wait_idle().await.expect("idle");
    let summary = session.summary().expect("summary");
    assert_eq!(summary.status(), CompletionStatus::Succeeded);
    assert!(summary.end_audit_ok());
    let done = inspect(&spool).unwrap();
    assert_eq!(done.requests[0].state, "succeeded");
    notes.push("query=ok".to_string());
    drop(stream);
    drop(session);

    let mut wal = DiskWal::open(&spool, WalConfig::default()).unwrap();
    wal.seal_active_segment().unwrap();
    drop(wal);
    let send_cfg = env.send_config(&spool, None);
    let (code, out, err) = run(env!("CARGO_BIN_EXE_wal-send"), &["--config", send_cfg.to_str().unwrap()]);
    assert_eq!(code, 75, "offline");
    assert!(out.contains("retained"));
    env.clean(&out, &err);
    assert!(spool.join("seg-00000001.wal").exists());
    notes.push("offline=75".to_string());

    let collect_cfg = env.collector_config();
    let mut collector = Collector::start(&env.ingest_bin, &collect_cfg, &env);
    let (code, out, err) = run(env!("CARGO_BIN_EXE_wal-send"), &["--config", send_cfg.to_str().unwrap()]);
    let _ = fs::write(
        "/evidence/raw/wal-send-deliver.txt",
        format!("code={code}\nstdout={out}\nstderr={err}\n"),
    );
    assert_eq!(code, 0, "deliver stdout={out} stderr={err}");
    assert!(out.contains("delivered=2"));
    env.clean(&out, &err);
    let first = receipts(&env, &done.spool_id).await;
    assert_eq!(first.len(), 2);
    assert!(first.iter().all(|row| row.project == "shop"));
    fs::remove_file(spool.join("frontier.chk")).unwrap();
    let (code, out, err) = run(env!("CARGO_BIN_EXE_wal-send"), &["--config", send_cfg.to_str().unwrap()]);
    assert_eq!(code, 0, "dedup");
    assert!(out.contains("delivered=2"));
    env.clean(&out, &err);
    assert_eq!(receipts(&env, &done.spool_id).await, first);
    notes.push("dedup=ok".to_string());

    collector.stop(&env);
    collector = Collector::start(&env.ingest_bin, &collect_cfg, &env);
    fs::remove_file(spool.join("frontier.chk")).unwrap();
    let (code, out, err) = run(env!("CARGO_BIN_EXE_wal-send"), &["--config", send_cfg.to_str().unwrap()]);
    assert_eq!(code, 0, "restart");
    assert!(out.contains("delivered=2"));
    env.clean(&out, &err);
    assert_eq!(receipts(&env, &done.spool_id).await, first);
    notes.push("restart=ok".to_string());

    let (code, out, err) = run(env!("CARGO_BIN_EXE_wal-inspect"), &["gc", "--spool", spool.to_str().unwrap()]);
    assert_eq!(code, 0, "gc");
    assert!(out.contains("removed=1"));
    env.clean(&out, &err);
    assert!(!spool.join("seg-00000001.wal").exists());

    let mut wal = DiskWal::open(&spool, WalConfig::default()).unwrap();
    wal.record_not_admitted("shop", "oo_r_clerk").unwrap();
    wal.seal_active_segment().unwrap();
    drop(wal);
    fs::write(spool.join("ack.json"), b"{\"ack\":true}").unwrap();
    let (code, out, _) = run(env!("CARGO_BIN_EXE_wal-inspect"), &["gc", "--spool", spool.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(out.contains("removed=0"));
    assert!(spool.join("seg-00000002.wal").exists() || spool.join("seg-00000003.wal").exists());
    let kept = fs::read_dir(&spool).unwrap().filter_map(|entry| entry.ok()).any(|entry| {
        entry.file_name().to_string_lossy().starts_with("seg-") && entry.path() != spool.join("seg-00000001.wal")
    });
    assert!(kept);
    notes.push("gc=ok".to_string());

    let sample = &first[0];
    let payload = [0xaau8];
    let hash = sha256_hex(&payload);
    let conflict_body = serde_json::json!({
        "protocol": "oo.audit.ingest/v1",
        "spool_id": sample.spool,
        "request_id": sample.request,
        "event_seq": sample.seq,
        "payload_hash": hash,
        "payload_hex": "aa",
        "project": "shop",
        "kind": "admission"
    });
    let config = gateway_audit_wal::load_send_config(&send_cfg).unwrap();
    let status = post_event_body(&config, &serde_json::to_vec(&conflict_body).unwrap()).await.expect("post");
    assert_eq!(status, 409);
    assert_eq!(receipts(&env, &done.spool_id).await, first);
    notes.push("conflict=409".to_string());

    let stranger = env.send_config(&spool, Some("stranger"));
    let status = post_event_body(
        &gateway_audit_wal::load_send_config(&stranger).unwrap(),
        &serde_json::to_vec(&conflict_body).unwrap(),
    )
    .await
    .expect("stranger");
    assert_eq!(status, 403);
    assert_eq!(receipts(&env, &done.spool_id).await, first);
    notes.push("stranger=403".to_string());

    let forced_spool = PathBuf::from("/tmp/oo-a371-force");
    reset(&forced_spool);
    let mut wal = DiskWal::open(&forced_spool, WalConfig::default()).unwrap();
    wal.record_not_admitted("shop", "oo_r_clerk").unwrap();
    wal.seal_active_segment().unwrap();
    drop(wal);
    let forced = env.send_config(&forced_spool, Some("force"));
    let (code, out, err) = run(env!("CARGO_BIN_EXE_wal-send"), &["--config", forced.to_str().unwrap()]);
    assert_eq!(code, 75, "force");
    env.clean(&out, &err);
    assert_eq!(receipts(&env, &done.spool_id).await, first);
    assert!(forced_spool.join("seg-00000001.wal").exists());
    notes.push("force_project=retained".to_string());

    collector.scan(&env);
    collector.stop(&env);

    guard_refusal(&env).await;
    notes.push("guard=ok".to_string());
    fsync_blocks_query(&env, &admin).await;
    notes.push("fsync=ok".to_string());
    quota_blocks_query(&env, &admin).await;
    notes.push("quota=ok".to_string());
    enospc_blocks_query(&env, &admin).await;
    notes.push("enospc=ok".to_string());

    let loose = PathBuf::from("/tmp/oo-a371-loose.conf");
    write_mode(&loose, &fs::read_to_string(&send_cfg).unwrap(), 0o644);
    let (code, out, err) = run(env!("CARGO_BIN_EXE_wal-send"), &["--config", loose.to_str().unwrap()]);
    assert_ne!(code, 0);
    env.clean(&out, &err);
    notes.push("mode=ok".to_string());

    notes.insert(0, "live=ok".to_string());
    let result = std::env::var("OO_A371_RESULT").unwrap_or_else(|_| "/tmp/oo-a371-live.txt".to_string());
    fs::write(&result, notes.join("\n") + "\n").unwrap();
    drop(admin);
}

struct Env {
    host: String,
    port: u16,
    query_db: String,
    audit_db: String,
    login_user: String,
    login_password: String,
    admin_user: String,
    admin_password: String,
    ingest_user: String,
    ingest_password: String,
    ca: Vec<u8>,
    mtls: PathBuf,
    quota: PathBuf,
    ingest_bin: PathBuf,
    secrets: Vec<String>,
}

impl Env {
    fn load() -> Self {
        let login_password = secret("OO_A371_LOGIN_PASSWORD_FILE");
        let admin_password = secret("OO_A371_ADMIN_PASSWORD_FILE");
        let ingest_password = secret("OO_A371_INGEST_PASSWORD_FILE");
        let mtls = PathBuf::from(required("OO_A371_MTLS"));
        let mut secrets = vec![login_password.clone(), admin_password.clone(), ingest_password.clone()];
        for name in ["server.key", "client.key", "stranger.key"] {
            secrets.push(fs::read_to_string(mtls.join(name)).expect("key"));
        }
        Self {
            host: required("OO_A371_PG_HOST"),
            port: required("OO_A371_PG_PORT").parse().expect("port"),
            query_db: required("OO_A371_QUERY_DB"),
            audit_db: required("OO_A371_AUDIT_DB"),
            login_user: required("OO_A371_LOGIN_USER"),
            login_password,
            admin_user: required("OO_A371_ADMIN_USER"),
            admin_password,
            ingest_user: required("OO_A371_INGEST_USER"),
            ingest_password,
            ca: fs::read(required("OO_A371_PG_CA")).expect("ca"),
            mtls,
            quota: PathBuf::from(required("OO_A371_QUOTA")),
            ingest_bin: PathBuf::from(required("OO_A371_INGEST_BIN")),
            secrets,
        }
    }

    fn clean(&self, stdout: &str, stderr: &str) {
        for secret in &self.secrets {
            assert!(!secret.is_empty());
            assert!(!stdout.contains(secret), "secret leaked");
            assert!(!stderr.contains(secret), "secret leaked");
        }
    }

    async fn open(&self, user: &str, password: &str, db: &str) -> Db {
        let endpoint = TlsEndpoint {
            host: self.host.clone(),
            port: self.port,
            database: db.to_string(),
            user: user.to_string(),
        };
        let (client, connection) = connect_require_tls(&endpoint, password, &self.ca).await.expect("connect");
        let driver = tokio::spawn(async move {
            let _ = connection.await;
        });
        Db { client, driver }
    }

    async fn session(&self) -> DedicatedSession {
        let endpoint = TlsEndpoint {
            host: self.host.clone(),
            port: self.port,
            database: self.query_db.clone(),
            user: self.login_user.clone(),
        };
        let tls = rustls_connector(&self.ca).expect("tls");
        let (client, connection) = connect_require_tls(&endpoint, &self.login_password, &self.ca).await.expect("login");
        DedicatedSession::bind(client, connection, tls).await.expect("bind")
    }

    fn send_config(&self, spool: &Path, kind: Option<&str>) -> PathBuf {
        let path = match kind {
            Some("stranger") => PathBuf::from("/tmp/oo-a371-stranger.conf"),
            Some("force") => PathBuf::from("/tmp/oo-a371-force.conf"),
            _ => PathBuf::from("/tmp/oo-a371-send.conf"),
        };
        let cert = if kind == Some("stranger") { "stranger.crt" } else { "client.crt" };
        let key = if kind == Some("stranger") { "stranger.key" } else { "client.key" };
        let force = if kind == Some("force") { "\nforce_project=other" } else { "" };
        let text = format!(
            "spool={}\ncollector_host=127.0.0.1\ncollector_port=18443\nserver_name=audit.local\nca_file={}/ca.crt\ncert_file={}/{cert}\nkey_file={}/{key}{force}\n",
            spool.display(),
            self.mtls.display(),
            self.mtls.display(),
            self.mtls.display()
        );
        write_mode(&path, &text, 0o600);
        path
    }

    fn collector_config(&self) -> PathBuf {
        let path = PathBuf::from("/tmp/oo-a371-ingest.conf");
        let text = format!(
            "listen=127.0.0.1:18443\nca_file={mtls}/ca.crt\ncert_file={mtls}/server.crt\nkey_file={mtls}/server.key\nregistry_file={mtls}/registry.txt\npg_host={host}\npg_port={port}\npg_db={db}\npg_user={user}\npg_password_file={pass}\npg_ca_file={ca}\n",
            mtls = self.mtls.display(),
            host = self.host,
            port = self.port,
            db = self.audit_db,
            user = self.ingest_user,
            pass = required("OO_A371_INGEST_PASSWORD_FILE"),
            ca = required("OO_A371_PG_CA"),
        );
        write_mode(&path, &text, 0o600);
        path
    }
}

struct Db {
    client: tokio_postgres::Client,
    driver: JoinHandle<()>,
}

impl Drop for Db {
    fn drop(&mut self) {
        self.driver.abort();
    }
}

struct Collector {
    child: Option<Child>,
}

impl Collector {
    fn start(bin: &Path, config: &Path, env: &Env) -> Self {
        let server_err = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open("/evidence/raw/collector-stderr.txt")
            .expect("collector stderr");
        let mut child = Command::new(bin)
            .arg("--config")
            .arg(config)
            .env_clear()
            .env("HOME", "/tmp")
            .stdout(Stdio::piped())
            .stderr(Stdio::from(server_err))
            .spawn()
            .expect("collector");
        let mut stdout = child.stdout.take().expect("stdout");
        let mut buf = [0u8; 64];
        let count = stdout.read(&mut buf).expect("listening");
        let text = String::from_utf8_lossy(&buf[..count]);
        assert!(text.contains("listening"), "collector");
        env.clean(&text, "");
        scan_proc(child.id(), env);
        Self { child: Some(child) }
    }

    fn scan(&self, env: &Env) {
        if let Some(child) = &self.child {
            scan_proc(child.id(), env);
        }
    }

    fn stop(&mut self, env: &Env) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        let _ = child.kill();
        let output = child.wait_with_output().expect("wait");
        let server_err = fs::read_to_string("/evidence/raw/collector-stderr.txt").unwrap_or_default();
        env.clean(&String::from_utf8_lossy(&output.stdout), &server_err);
    }
}

impl Drop for Collector {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
        }
    }
}

struct OwnedCatalog {
    identity: PolicyIdentity,
    catalog: PolicyCatalog,
}

fn catalog() -> OwnedCatalog {
    let identity = PolicyIdentity {
        project: "shop".to_string(),
        role: "oo_r_clerk".to_string(),
        policy_id: "a".repeat(64),
    };
    let relation = VisibleRelation {
        schema: "shop".to_string(),
        name: "entry".to_string(),
        physical_schema: "_oo".to_string(),
        physical_name: "pt_entry".to_string(),
        columns: vec![
            VisibleColumn { name: "entry_id".to_string(), physical_name: "pc_entry_id".to_string(), type_oid: 20 },
            VisibleColumn { name: "amount".to_string(), physical_name: "pc_amount".to_string(), type_oid: 1700 },
            VisibleColumn { name: "wide".to_string(), physical_name: "pc_wide".to_string(), type_oid: 1700 },
        ],
        pending_unclassified: 1,
        estimated_rows: 6,
    };
    let catalog = PolicyCatalog {
        identity: identity.clone(),
        relations: vec![relation],
        functions: Vec::new(),
        operators: Vec::new(),
        literal_types: LiteralTypes { integer: Some(23), text: Some(25), boolean: Some(16) },
        null_test_allowed: false,
        large_table_row_threshold: 1_000_000,
    };
    OwnedCatalog { identity, catalog }
}

fn request<'a>(owned: &'a OwnedCatalog, sql: &'a str) -> SelectRequest<'a> {
    SelectRequest {
        identity: &owned.identity,
        catalog: &owned.catalog,
        original_sql: sql,
        role: "oo_r_clerk",
        max_rows: 20,
        max_result_bytes: 4096,
        wall_clock: Duration::from_secs(5),
    }
}

fn expect_row(row: &[Cell], id: &str, amount_neg: bool, amount: &str, wide_neg: bool, wide: &str) {
    assert!(matches!(&row[0], Cell::Int(text) if text == id));
    decimal(&row[1], amount_neg, amount);
    decimal(&row[2], wide_neg, wide);
}

fn decimal(cell: &Cell, negative: bool, coefficient: &str) {
    match cell {
        Cell::Decimal(value) => {
            assert_eq!(value.is_negative(), negative);
            assert_eq!(value.coefficient(), coefficient);
            assert_eq!(value.scale(), 4);
        }
        _ => panic!("decimal"),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Receipt {
    spool: String,
    request: String,
    seq: u64,
    project: String,
    hash: String,
    receipt: String,
}

async fn receipts(env: &Env, spool_id: &str) -> Vec<Receipt> {
    let db = env.open(&env.admin_user, &env.admin_password, &env.audit_db).await;
    let rows = db
        .client
        .query(
            "SELECT e.spool_id::text, e.request_id::text, e.event_seq, e.project, encode(e.payload_hash, 'hex'), r.receipt_id::text \
             FROM oo_audit_ingest.event e \
             JOIN oo_audit_ingest.receipt r USING (spool_id, request_id, event_seq) \
             WHERE e.spool_id = ($1::text)::uuid \
             ORDER BY e.request_id::text, e.event_seq",
            &[&spool_id],
        )
        .await
        .expect("receipts");
    rows.into_iter()
        .map(|row| Receipt {
            spool: row.get(0),
            request: row.get(1),
            seq: row.get::<_, i64>(2) as u64,
            project: row.get(3),
            hash: row.get(4),
            receipt: row.get(5),
        })
        .collect()
}

async fn privileges(env: &Env) {
    let admin = env.open(&env.admin_user, &env.admin_password, &env.audit_db).await;
    let grants = admin
        .client
        .query(
            "SELECT privilege_type FROM information_schema.role_table_grants \
             WHERE grantee = 'audit_ingest' AND table_schema = 'oo_audit_ingest'",
            &[],
        )
        .await
        .expect("grants");
    let mut kinds = Vec::new();
    for row in grants {
        kinds.push(row.get::<_, String>(0));
    }
    assert!(kinds.iter().all(|kind| kind == "INSERT" || kind == "SELECT"));
    assert!(kinds.iter().any(|kind| kind == "INSERT"));
    let feedback: i64 = admin
        .client
        .query_one(
            "SELECT count(*) FROM information_schema.tables WHERE table_schema = 'oo_audit_ingest' AND table_name = 'feedback'",
            &[],
        )
        .await
        .expect("feedback")
        .get(0);
    assert_eq!(feedback, 0);
    let ingest = env.open(&env.ingest_user, &env.ingest_password, &env.audit_db).await;
    assert!(ingest.client.batch_execute("UPDATE oo_audit_ingest.event SET project = project").await.is_err());
    assert!(ingest.client.batch_execute("SELECT rolpassword FROM pg_authid").await.is_err());
    assert!(env.try_open(&env.ingest_user, &env.ingest_password, &env.query_db).await.is_none());
    assert!(env.try_open(&env.login_user, &env.login_password, &env.audit_db).await.is_none());
}

impl Env {
    async fn try_open(&self, user: &str, password: &str, db: &str) -> Option<Db> {
        let endpoint = TlsEndpoint {
            host: self.host.clone(),
            port: self.port,
            database: db.to_string(),
            user: user.to_string(),
        };
        match connect_require_tls(&endpoint, password, &self.ca).await {
            Ok((client, connection)) => {
                let driver = tokio::spawn(async move {
                    let _ = connection.await;
                });
                Some(Db { client, driver })
            }
            Err(_) => None,
        }
    }
}

async fn guard_refusal(env: &Env) {
    let dir = PathBuf::from("/tmp/oo-a371-guard");
    reset(&dir);
    let mut session = env.session().await;
    let owned = catalog();
    let wal = DiskWal::open(&dir, WalConfig::default()).unwrap();
    let sql = format!("INSERT INTO shop.entry (entry_id) VALUES (1) -- {TOKEN}");
    let err = must_err(session.execute_trusted_select(request(&owned, &sql), wal).await, "guard");
    assert!(matches!(err, ExecError::Guard(ref inner) if matches!(inner.as_ref(), GuardError::Write)));
    assert_absent(&dir, TOKEN);
    let mut wal = DiskWal::open(&dir, WalConfig::default()).unwrap();
    wal.record_not_admitted("shop", "oo_r_clerk").unwrap();
    drop(wal);
    assert_absent(&dir, TOKEN);
    let view = inspect(&dir).unwrap();
    assert_eq!(view.requests[0].state, "not_admitted");
    let hidden = "SELECT hidden FROM shop.entry";
    let wal = DiskWal::open(&dir, WalConfig::default()).unwrap();
    let err = must_err(session.execute_trusted_select(request(&owned, hidden), wal).await, "hidden");
    assert!(matches!(err, ExecError::Guard(_)));
    assert_absent(&dir, TOKEN);
}

async fn fsync_blocks_query(env: &Env, admin: &Db) {
    let dir = PathBuf::from("/tmp/oo-a371-fsync");
    reset(&dir);
    let mut session = env.session().await;
    let pid = session.backend_pid();
    let owned = catalog();
    let mut wal = DiskWal::open(&dir, WalConfig::default()).unwrap();
    wal.fail_next_syncs(1);
    let err = must_err(session.execute_trusted_select(request(&owned, SQL), wal).await, "fsync");
    assert!(matches!(err, ExecError::AuditUnavailable));
    assert!(inspect(&dir).unwrap().frames.is_empty());
    assert_eq!(fs::read_to_string(dir.join("last-fault.txt")).unwrap(), "sync");
    assert_idle(admin, pid).await;
}

async fn quota_blocks_query(env: &Env, admin: &Db) {
    let dir = PathBuf::from("/tmp/oo-a371-quota");
    reset(&dir);
    let mut session = env.session().await;
    let pid = session.backend_pid();
    let owned = catalog();
    let wal = DiskWal::open(&dir, WalConfig { max_bytes: 100 }).unwrap();
    let err = must_err(session.execute_trusted_select(request(&owned, SQL), wal).await, "quota");
    assert!(matches!(err, ExecError::AuditUnavailable));
    assert!(inspect(&dir).unwrap().frames.is_empty());
    assert_eq!(fs::read_to_string(dir.join("last-fault.txt")).unwrap(), "quota");
    assert_idle(admin, pid).await;
}

async fn enospc_blocks_query(env: &Env, admin: &Db) {
    let dir = env.quota.join("spool");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let mut wal = DiskWal::open(&dir, WalConfig { max_bytes: 32 * 1024 * 1024 }).unwrap();
    let mut failed = false;
    for _ in 0..4000 {
        if wal.record_not_admitted("shop", "oo_r_clerk").is_err() {
            failed = true;
            break;
        }
    }
    assert!(failed, "disk accepted every frame");
    drop(wal);
    let mut hold = OpenOptions::new().create(true).append(true).open(dir.join("hold.bin")).unwrap();
    let block = [0u8; 512];
    while hold.write_all(&block).is_ok() {}
    let one = [0u8; 1];
    while hold.write_all(&one).is_ok() {}
    let _ = hold.sync_all();
    drop(hold);
    let mut session = env.session().await;
    let pid = session.backend_pid();
    let owned = catalog();
    let wal = DiskWal::open(&dir, WalConfig { max_bytes: 32 * 1024 * 1024 }).expect("reopen");
    let err = must_err(session.execute_trusted_select(request(&owned, SQL), wal).await, "enospc");
    assert!(matches!(err, ExecError::AuditUnavailable));
    assert_eq!(fs::read_to_string(dir.join("last-fault.txt")).unwrap(), "no_space");
    assert_idle(admin, pid).await;
}

async fn assert_idle(admin: &Db, pid: i32) {
    let row = admin
        .client
        .query_one(
            "SELECT (xact_start IS NULL), COALESCE(state, '') FROM pg_catalog.pg_stat_activity WHERE pid = $1",
            &[&pid],
        )
        .await
        .expect("activity");
    let clear: bool = row.get(0);
    let state: String = row.get(1);
    assert!(clear, "transaction");
    assert_eq!(state, "idle");
}

fn assert_absent(dir: &Path, token: &str) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            let bytes = fs::read(&path).unwrap();
            assert!(!bytes.windows(token.len()).any(|window| window == token.as_bytes()), "token stored");
        }
    }
}

fn reset(path: &Path) {
    let _ = fs::remove_dir_all(path);
    fs::create_dir_all(path).unwrap();
}

fn write_mode(path: &Path, text: &str, mode: u32) {
    let mut file = OpenOptions::new().create(true).write(true).truncate(true).mode(mode).open(path).unwrap();
    file.write_all(text.as_bytes()).unwrap();
}

fn run(bin: &str, args: &[&str]) -> (i32, String, String) {
    let output = Command::new(bin).args(args).env_clear().env("HOME", "/tmp").output().expect("spawn");
    (
        output.status.code().unwrap_or(1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn scan_proc(pid: u32, env: &Env) {
    let command = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    let process_env = fs::read(format!("/proc/{pid}/environ")).unwrap_or_default();
    let text = String::from_utf8_lossy(&command);
    let vars = String::from_utf8_lossy(&process_env);
    env.clean(&text, &vars);
    assert!(!text.contains("PRIVATE KEY"));
    assert!(!vars.contains("PRIVATE KEY"));
}

fn secret(name: &str) -> String {
    let mut raw = fs::read(required(name)).unwrap_or_else(|_| panic!("missing {name}"));
    while raw.last().is_some_and(|byte| *byte == b'\n' || *byte == b'\r') {
        raw.pop();
    }
    String::from_utf8(raw).expect("utf8")
}

fn required(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("missing {name}"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex_of(&Sha256::digest(bytes))
}

fn hex_of(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::new();
    for byte in bytes {
        out.push(char::from(HEX[(byte >> 4) as usize]));
        out.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
    out
}
