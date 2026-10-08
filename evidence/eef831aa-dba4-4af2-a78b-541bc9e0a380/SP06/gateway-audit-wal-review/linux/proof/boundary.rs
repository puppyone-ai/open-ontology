//! Independent E55 boundary. Not the author live test.
//! Real DedicatedSession, fsync admission, mTLS ingest, one private PostgreSQL.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use gateway_audit_wal::{inspect, load_send_config, post_event_body, DiskWal, WalConfig};
use oo_gateway::query_executor::{
    Cell, CompletionStatus, DedicatedSession, ExecError, SelectRequest, UntrustedAdmission, UNTRUSTED_ADMISSION,
};
use oo_gateway::sql_guard::{
    GuardError, LiteralTypes, PolicyCatalog, PolicyIdentity, VisibleColumn, VisibleRelation,
};
use oo_gateway::{connect_require_tls, rustls_connector, TlsEndpoint};
use sha2::{Digest, Sha256};
use tokio::task::JoinHandle;

fn must_err<T>(result: Result<T, ExecError>, label: &'static str) -> ExecError {
    match result {
        Err(err) => err,
        Ok(_) => panic!("{label}"),
    }
}

const SQL: &str = "SELECT entry_id, amount, wide FROM shop.entry ORDER BY entry_id";
const TOKEN: &str = "classified-literal-token";
const AMOUNT: &str = "123456789012345678";
const WIDE: &str = "12345678901234567890123456789012345678";
const PRODUCT_SHA: &str = "\
118e4b3011866148754866f5be20e5ef744ba012dc338d7097223d2de8576e87  tools/audit-ingest/CONTRACT.md
189ea17abf357eb2259b7a8395428a857efb68a19c921c18f7254a20c02f5356  tools/audit-ingest/Cargo.lock
fdffd9ced2f6b7d6c8c9986f46af29327238db1505f0955d7273eb2ad2f6b9ad  tools/audit-ingest/Cargo.toml
e80d8c0a6f0c891e348f172d3f783f18b5e8d6186f6a33543a753ccd18ce68f1  tools/audit-ingest/migrations/oo_audit_ingest_v1.sql
b02298cca2dac5a1924aa98b759ef7bf8e16c84c015bf187667cb6d5fa2b192c  tools/audit-ingest/schema/ack.v1.json
92bb935b46467ed5c3392b4d1e951be3de7c8ad0f78b43c3f3f7823243b26134  tools/audit-ingest/schema/event.v1.json
02c40f309b0f48969d1df365981c9902bb1d24cfe97f2d9c78bc2570c3ea2546  tools/audit-ingest/schema/registry.md
5a6b652f5e3d37870b061f0d0317b366824e5f1f41e2324d2d664b150966c355  tools/audit-ingest/src/bin/audit_ingest.rs
752144016f91b1d44532ee2f0e98b57387b13d0adb96d8b03e3aec28a33410cf  tools/audit-ingest/src/lib.rs
4e2a37d786931102851d554277ab5e02942bcd84b58b083e2a84b20550d51c46  tools/audit-ingest/src/pg.rs
3dd2e48f1d297d1a82e50f0471725635f29c337b73ec00b8ac071e68b92e5790  tools/gateway-audit-wal/CONTRACT.md
dd0a08d01a108f13443a8a24e2eb00ba95ca37b3f81ddc5dd2436ce9fca4290f  tools/gateway-audit-wal/Cargo.lock
dd573805f340548cda03b86498c3b44c65ab8a8405607cc3562d6d8dbdd96cde  tools/gateway-audit-wal/Cargo.toml
a0a293ff0ab2dd097d5bf0759bda6da78236b68ad077b54df80412a3e412effd  tools/gateway-audit-wal/src/bin/wal_inspect.rs
96908ebedc6097cd7a5689d9b2ac9f7c7ca4d7951129ae9fe3793ff182170307  tools/gateway-audit-wal/src/bin/wal_send.rs
cfdb495a21c598f69b3393ba74058f246aa705370ca310926f676a131b966e73  tools/gateway-audit-wal/src/deliver.rs
eabb5f526eeee60dbcdc1b7b96962b866174c55d98b77566c640b37ea236b378  tools/gateway-audit-wal/src/error.rs
3435fa0c5bbfb65626c17f1d167f638026f3d49cbd24289e04e5b99bf221675f  tools/gateway-audit-wal/src/frame.rs
ea3eaa77d5881472dc5445f1357b852bdfa5d69620530214d4a33da8882b468b  tools/gateway-audit-wal/src/lib.rs
85abf4b90f95e718596b9eab2c99e3c150aa2ac5ad88f4065165ac119e169abd  tools/gateway-audit-wal/src/store.rs
dcccff5e3f45d4b6031d9bdd08e8886eef2e8236ef54195fe0d8baa214bba7b4  tools/gateway-audit-wal/tests/contract.rs
8094b4a909d201f09c92c11e9159e67316609576d3a9cfa5508be729cf8a07bd  tools/gateway-audit-wal/tests/live.rs
a7ce7c151c6e733c47eb5c106c93e511908095ed24420226c8057d3a0b602b60  tools/gateway-audit-wal/tests/lock_subset.rs
";

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn independent_e55_boundary() {
    if std::env::var("OO_A380_BOUNDARY").ok().as_deref() != Some("1") {
        return;
    }
    let env = Env::load();
    let result = PathBuf::from(required("OO_A380_RESULT"));
    let _ = fs::remove_file(&result);
    let mut notes = Notes { path: result };
    assert_eq!(UNTRUSTED_ADMISSION, UntrustedAdmission::Blocked);
    notes.push("untrusted=blocked");
    assert_product_tree(&env.repo);
    notes.push("product_sha=ok");
    assert_source_order(&env.repo);
    notes.push("source_order=ok");

    let admin = env.open(&env.admin_user, &env.admin_password, &env.query_db).await;
    privileges(&env).await;
    notes.push("privileges=ok");

    let spool = PathBuf::from("/tmp/oo-a380-spool");
    reset(&spool);
    let mut session = env.session().await;
    let owned = catalog();
    let wal = DiskWal::open(&spool, WalConfig { max_bytes: 8 * 1024 * 1024 }).unwrap();
    let mut stream = session
        .execute_trusted_select(request(&owned, SQL), wal)
        .await
        .expect("exec");
    let early = inspect(&spool).expect("inspect");
    assert_eq!(early.requests.len(), 1);
    assert_eq!(early.requests[0].state, "admitted");
    assert!(early.frames.iter().any(|frame| frame.kind == "admission"));
    assert!(!early.frames.iter().any(|frame| frame.kind == "completion"));
    let segment = fs::read(spool.join("seg-00000001.wal")).expect("segment");
    assert!(segment.len() > 32);
    assert!(!segment.windows(SQL.len()).any(|window| window == SQL.as_bytes()));
    let (code, text, err) = run(env!("CARGO_BIN_EXE_wal-inspect"), &["replay", "--spool", spool.to_str().unwrap()]);
    assert_eq!(code, 0, "{text} {err}");
    assert!(text.contains("state=admitted"));
    assert!(!text.contains("state=succeeded"));
    env.clean(&text, &err);
    notes.push("admission_before_row=ok");

    let mut rows = Vec::new();
    loop {
        match stream.next_row().await.expect("row") {
            Some(row) => rows.push(row),
            None => break,
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
    assert_eq!(done.requests.len(), 1);
    assert_eq!(done.requests[0].state, "succeeded");
    notes.push("query=ok");
    drop(stream);
    drop(session);

    let (code, text, err) = run(env!("CARGO_BIN_EXE_wal-inspect"), &["replay", "--spool", spool.to_str().unwrap()]);
    assert_eq!(code, 0, "{text} {err}");
    assert!(text.contains("state=succeeded"));
    assert!(text.contains("kind=completion"));
    env.clean(&text, &err);
    notes.push("reader_restart=ok");
    notes.push("completion_retained=ok");

    let mut wal = DiskWal::open(&spool, WalConfig::default()).unwrap();
    wal.seal_active_segment().unwrap();
    drop(wal);
    let send_cfg = env.send_config(&spool, None);
    let (code, out, err) = run(env!("CARGO_BIN_EXE_wal-send"), &["--config", send_cfg.to_str().unwrap()]);
    assert_eq!(code, 75, "{out} {err}");
    assert!(out.contains("retained"));
    env.clean(&out, &err);
    assert!(spool.join("seg-00000001.wal").is_file());
    let (code, out, err) = run(env!("CARGO_BIN_EXE_wal-inspect"), &["gc", "--spool", spool.to_str().unwrap()]);
    assert_eq!(code, 0, "{out} {err}");
    assert!(out.contains("removed=0"));
    env.clean(&out, &err);
    assert!(spool.join("seg-00000001.wal").is_file());
    notes.push("offline=75");
    notes.push("gc_before_ack=0");

    let collect_cfg = env.collector_config();
    let mut collector = Collector::start(&env.ingest_bin, &collect_cfg, &env);
    let (code, out, err) = run(env!("CARGO_BIN_EXE_wal-send"), &["--config", send_cfg.to_str().unwrap()]);
    assert_eq!(code, 0, "{out} {err}");
    assert!(out.contains("delivered=2"));
    env.clean(&out, &err);
    let first = receipts(&env, &done.spool_id).await;
    assert_eq!(first.len(), 2);
    assert!(first.iter().all(|row| row.project == "shop"));
    let counts = triple(&env, &done.spool_id).await;
    assert_eq!(counts, (2, 2, 0));
    notes.push("project=shop");
    notes.push("event_receipt_equal=ok");

    fs::remove_file(spool.join("frontier.chk")).unwrap();
    let (code, out, err) = run(env!("CARGO_BIN_EXE_wal-send"), &["--config", send_cfg.to_str().unwrap()]);
    assert_eq!(code, 0, "{out} {err}");
    assert!(out.contains("delivered=2"));
    env.clean(&out, &err);
    assert_eq!(receipts(&env, &done.spool_id).await, first);
    assert_eq!(triple(&env, &done.spool_id).await, (2, 2, 0));
    notes.push("dedup=ok");

    collector.stop(&env);
    collector = Collector::start(&env.ingest_bin, &collect_cfg, &env);
    fs::remove_file(spool.join("frontier.chk")).unwrap();
    let (code, out, err) = run(env!("CARGO_BIN_EXE_wal-send"), &["--config", send_cfg.to_str().unwrap()]);
    assert_eq!(code, 0, "{out} {err}");
    assert!(out.contains("delivered=2"));
    env.clean(&out, &err);
    assert_eq!(receipts(&env, &done.spool_id).await, first);
    notes.push("restart_collector=ok");

    let sample = &first[0];
    let payload = [0xaau8];
    let conflict_body = serde_json::json!({
        "protocol": "oo.audit.ingest/v1",
        "spool_id": sample.spool,
        "request_id": sample.request,
        "event_seq": sample.seq,
        "payload_hash": sha256_hex(&payload),
        "payload_hex": "aa",
        "project": "shop",
        "kind": "admission"
    });
    let config = load_send_config(&send_cfg).unwrap();
    let status = post_event_body(&config, &serde_json::to_vec(&conflict_body).unwrap())
        .await
        .expect("post");
    assert_eq!(status, 409);
    assert_eq!(receipts(&env, &done.spool_id).await, first);
    assert_eq!(triple(&env, &done.spool_id).await, (2, 2, 0));
    notes.push("conflict=409");

    let stranger = env.send_config(&spool, Some("stranger"));
    let status = post_event_body(
        &load_send_config(&stranger).unwrap(),
        &serde_json::to_vec(&conflict_body).unwrap(),
    )
    .await
    .expect("stranger");
    assert_eq!(status, 403);
    assert_eq!(receipts(&env, &done.spool_id).await, first);
    notes.push("stranger=403");

    let forced_spool = PathBuf::from("/tmp/oo-a380-force");
    reset(&forced_spool);
    let mut wal = DiskWal::open(&forced_spool, WalConfig::default()).unwrap();
    wal.record_not_admitted("shop", "oo_r_clerk").unwrap();
    wal.seal_active_segment().unwrap();
    drop(wal);
    let forced = env.send_config(&forced_spool, Some("force"));
    let (code, out, err) = run(env!("CARGO_BIN_EXE_wal-send"), &["--config", forced.to_str().unwrap()]);
    assert_eq!(code, 75, "{out} {err}");
    env.clean(&out, &err);
    assert!(forced_spool.join("seg-00000001.wal").is_file());
    let other: i64 = admin_count(
        &env,
        "SELECT count(*) FROM oo_audit_ingest.event WHERE project <> 'shop'",
    )
    .await;
    assert_eq!(other, 0);
    notes.push("force_project=retained");
    notes.push("header_ignored=ok");

    let saved = fs::read(spool.join("frontier.chk")).unwrap();
    corrupt_payload_hash(&spool.join("frontier.chk"));
    let (code, out, err) = run(env!("CARGO_BIN_EXE_wal-inspect"), &["gc", "--spool", spool.to_str().unwrap()]);
    assert_ne!(code, 0, "{out} {err}");
    env.clean(&out, &err);
    assert!(spool.join("seg-00000001.wal").is_file());
    notes.push("bad_hash_gc=closed");
    fs::write(spool.join("frontier.chk"), &saved).unwrap();
    let (code, out, err) = run(env!("CARGO_BIN_EXE_wal-inspect"), &["gc", "--spool", spool.to_str().unwrap()]);
    assert_eq!(code, 0, "{out} {err}");
    assert!(out.contains("removed=1"));
    env.clean(&out, &err);
    assert!(!spool.join("seg-00000001.wal").exists());
    notes.push("gc_after_ack=1");

    let mut wal = DiskWal::open(&spool, WalConfig::default()).unwrap();
    wal.record_not_admitted("shop", "oo_r_clerk").unwrap();
    wal.seal_active_segment().unwrap();
    drop(wal);
    fs::write(spool.join("ack.json"), b"{\"ack\":true}").unwrap();
    let (code, out, err) = run(env!("CARGO_BIN_EXE_wal-inspect"), &["gc", "--spool", spool.to_str().unwrap()]);
    assert_eq!(code, 0, "{out} {err}");
    assert!(out.contains("removed=0"));
    env.clean(&out, &err);
    assert!(spool.join("seg-00000002.wal").is_file() || spool.join("seg-00000003.wal").is_file());
    notes.push("caller_ack_ignored=ok");
    collector.stop(&env);

    guard_refusal(&env).await;
    notes.push("guard=ok");
    fsync_blocks_query(&env, &admin).await;
    notes.push("fsync=ok");
    quota_blocks_query(&env, &admin).await;
    notes.push("quota=ok");
    enospc_blocks_query(&env, &admin).await;
    notes.push("enospc=ok");

    let loose = PathBuf::from("/tmp/oo-a380-loose.conf");
    write_mode(&loose, &fs::read_to_string(&send_cfg).unwrap(), 0o644);
    let (code, out, err) = run(env!("CARGO_BIN_EXE_wal-send"), &["--config", loose.to_str().unwrap()]);
    assert_ne!(code, 0, "{out}");
    env.clean(&out, &err);
    notes.push("mode=ok");
    notes.push("boundary=ok");
    drop(admin);
}

struct Notes {
    path: PathBuf,
}

impl Notes {
    fn push(&mut self, line: &str) {
        eprintln!("{line}");
        let mut file = OpenOptions::new().create(true).append(true).open(&self.path).unwrap();
        writeln!(file, "{line}").unwrap();
    }
}

struct Env {
    repo: PathBuf,
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
        let login_password = secret("OO_A380_LOGIN_PASSWORD_FILE");
        let admin_password = secret("OO_A380_ADMIN_PASSWORD_FILE");
        let ingest_password = secret("OO_A380_INGEST_PASSWORD_FILE");
        let mtls = PathBuf::from(required("OO_A380_MTLS"));
        let mut secrets = vec![login_password.clone(), admin_password.clone(), ingest_password.clone()];
        for name in ["server.key", "client.key", "stranger.key"] {
            secrets.push(fs::read_to_string(mtls.join(name)).expect(name));
        }
        Self {
            repo: PathBuf::from(required("OO_A380_REPO")),
            host: required("OO_A380_PG_HOST"),
            port: required("OO_A380_PG_PORT").parse().expect("port"),
            query_db: required("OO_A380_QUERY_DB"),
            audit_db: required("OO_A380_AUDIT_DB"),
            login_user: required("OO_A380_LOGIN_USER"),
            login_password,
            admin_user: required("OO_A380_ADMIN_USER"),
            admin_password,
            ingest_user: required("OO_A380_INGEST_USER"),
            ingest_password,
            ca: fs::read(required("OO_A380_PG_CA")).expect("ca"),
            mtls,
            quota: PathBuf::from(required("OO_A380_QUOTA")),
            ingest_bin: PathBuf::from(required("OO_A380_INGEST_BIN")),
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
        self.try_open(user, password, db).await.expect("connect")
    }

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

    async fn session(&self) -> DedicatedSession {
        let endpoint = TlsEndpoint {
            host: self.host.clone(),
            port: self.port,
            database: self.query_db.clone(),
            user: self.login_user.clone(),
        };
        let tls = rustls_connector(&self.ca).expect("tls");
        let (client, connection) = connect_require_tls(&endpoint, &self.login_password, &self.ca)
            .await
            .expect("login");
        DedicatedSession::bind(client, connection, tls).await.expect("bind")
    }

    fn send_config(&self, spool: &Path, kind: Option<&str>) -> PathBuf {
        let path = match kind {
            Some("stranger") => PathBuf::from("/tmp/oo-a380-stranger.conf"),
            Some("force") => PathBuf::from("/tmp/oo-a380-force.conf"),
            _ => PathBuf::from("/tmp/oo-a380-send.conf"),
        };
        let cert = if kind == Some("stranger") { "stranger.crt" } else { "client.crt" };
        let key = if kind == Some("stranger") { "stranger.key" } else { "client.key" };
        let force = if kind == Some("force") { "\nforce_project=other" } else { "" };
        let text = format!(
            "spool={}\ncollector_host=127.0.0.1\ncollector_port=18444\nserver_name=audit.local\nca_file={}/ca.crt\ncert_file={}/{cert}\nkey_file={}/{key}{force}\n",
            spool.display(),
            self.mtls.display(),
            self.mtls.display(),
            self.mtls.display()
        );
        write_mode(&path, &text, 0o600);
        path
    }

    fn collector_config(&self) -> PathBuf {
        let path = PathBuf::from("/tmp/oo-a380-ingest.conf");
        let text = format!(
            "listen=127.0.0.1:18444\nca_file={mtls}/ca.crt\ncert_file={mtls}/server.crt\nkey_file={mtls}/server.key\nregistry_file={mtls}/registry.txt\npg_host={host}\npg_port={port}\npg_db={db}\npg_user={user}\npg_password_file={pass}\npg_ca_file={ca}\n",
            mtls = self.mtls.display(),
            host = self.host,
            port = self.port,
            db = self.audit_db,
            user = self.ingest_user,
            pass = required("OO_A380_INGEST_PASSWORD_FILE"),
            ca = required("OO_A380_PG_CA"),
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
        let stdout = child.stdout.take().expect("stdout");
        let text = wait_text(stdout);
        assert!(text.contains("listening"), "collector start: {text}");
        env.clean(&text, "");
        scan_proc(child.id(), env);
        Self { child: Some(child) }
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

fn wait_text(mut stdout: impl Read + Send + 'static) -> String {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 256];
        let count = stdout.read(&mut buf).unwrap_or(0);
        let _ = tx.send(String::from_utf8_lossy(&buf[..count]).into_owned());
    });
    rx.recv_timeout(Duration::from_secs(20)).unwrap_or_default()
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
        other => panic!("decimal {other:?}"),
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
             ORDER BY e.event_seq",
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

async fn triple(env: &Env, spool_id: &str) -> (i64, i64, i64) {
    let db = env.open(&env.admin_user, &env.admin_password, &env.audit_db).await;
    let row = db
        .client
        .query_one(
            "SELECT \
               (SELECT count(*) FROM oo_audit_ingest.event WHERE spool_id = ($1::text)::uuid), \
               (SELECT count(*) FROM oo_audit_ingest.receipt WHERE spool_id = ($1::text)::uuid), \
               (SELECT count(*) FROM oo_audit_ingest.event e \
                  LEFT JOIN oo_audit_ingest.receipt r USING (spool_id, request_id, event_seq) \
                  WHERE e.spool_id = ($1::text)::uuid AND r.receipt_id IS NULL)",
            &[&spool_id],
        )
        .await
        .expect("triple");
    (row.get(0), row.get(1), row.get(2))
}

async fn admin_count(env: &Env, sql: &str) -> i64 {
    let db = env.open(&env.admin_user, &env.admin_password, &env.audit_db).await;
    db.client.query_one(sql, &[]).await.expect("count").get(0)
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

async fn guard_refusal(env: &Env) {
    let dir = PathBuf::from("/tmp/oo-a380-guard");
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
    let wal = DiskWal::open(&dir, WalConfig::default()).unwrap();
    let err = must_err(
        session.execute_trusted_select(request(&owned, "SELECT hidden FROM shop.entry"), wal).await,
        "hidden",
    );
    assert!(matches!(err, ExecError::Guard(_)));
    assert_absent(&dir, TOKEN);
}

async fn fsync_blocks_query(env: &Env, admin: &Db) {
    let dir = PathBuf::from("/tmp/oo-a380-fsync");
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
    let dir = PathBuf::from("/tmp/oo-a380-quota");
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
    assert!(clear, "transaction open");
    assert_eq!(state, "idle");
}

fn assert_product_tree(repo: &Path) {
    let mut seen = 0u32;
    for line in PRODUCT_SHA.lines() {
        let (digest, rel) = line.split_once("  ").expect("sha line");
        let got = sha256_hex(&fs::read(repo.join(rel)).unwrap_or_else(|_| panic!("missing {rel}")));
        assert_eq!(got, digest, "{rel}");
        seen += 1;
    }
    assert_eq!(seen, 23);
}

fn assert_source_order(repo: &Path) {
    let exec = fs::read_to_string(repo.join("crates/oo-gateway/src/query_executor.rs")).unwrap();
    let start = exec.find("pub async fn execute_trusted_select").expect("execute");
    let end = start + exec[start..].find("pub async fn wait_idle").expect("wait");
    let body = &exec[start..end];
    let admit = body.find("audit.admit(&admission).await?;").expect("admit");
    let take = body.find("self.client.take()").expect("take");
    assert!(admit < take, "admit must return before the producer starts");
    assert!(!body.contains("build_transaction"), "BEGIN is not in the admission function");
    assert!(!body.contains("open_read_only"));
    let producer = exec.find("async fn run_producer").expect("producer");
    let drive = exec.find("async fn drive(").expect("drive");
    assert!(exec[producer..drive].contains("drive("));
    let open = exec.find("async fn open_read_only").expect("open");
    assert!(exec[open..open + 800].contains("build_transaction()"));
    let store = fs::read_to_string(repo.join("tools/gateway-audit-wal/src/store.rs")).unwrap();
    let append = store.find("fn append(").expect("append");
    let next = append + store[append..].find("fn sync_active(").expect("sync");
    let append_body = &store[append..next];
    let sync_at = append_body.find("self.sync_active()").expect("sync call");
    let index_at = append_body.find("self.index.insert").expect("index");
    assert!(sync_at < index_at, "frame is indexed only after fsync");
    let pg = fs::read_to_string(repo.join("tools/audit-ingest/src/pg.rs")).unwrap();
    let insert = pg.find("pub async fn insert_event").expect("insert");
    let end_fn = insert + pg[insert..].find("async fn existing_receipt").expect("existing");
    let fn_body = &pg[insert..end_fn];
    let begin = fn_body.find("self.client.transaction()").expect("begin");
    let event = fn_body.find("INSERT INTO oo_audit_ingest.event").expect("event");
    let receipt = fn_body.find("insert_receipt").expect("receipt");
    let commit = fn_body.find("transaction.commit()").expect("commit");
    assert!(begin < event && event < receipt && receipt < commit);
    assert_eq!(fn_body.matches("transaction.commit()").count(), 1);
    let ingest = fs::read_to_string(repo.join("tools/audit-ingest/src/lib.rs")).unwrap();
    let handle = ingest.find("async fn handle(").expect("handle");
    let handle_end = handle + ingest[handle..].find("fn status_parts").expect("status");
    let handle_body = &ingest[handle..handle_end];
    assert!(!handle_body.to_ascii_lowercase().contains("x-project"));
    assert!(handle_body.contains("certificate_uri"));
    assert!(handle_body.contains("prepare_event(&body"));
    let deliver = fs::read_to_string(repo.join("tools/gateway-audit-wal/src/deliver.rs")).unwrap();
    assert!(deliver.contains("x-project: not-the-identity"));
}

fn corrupt_payload_hash(path: &Path) {
    let mut bytes = fs::read(path).unwrap();
    assert!(bytes.len() > 48 + 32);
    bytes[48] ^= 0xff;
    let trailer = bytes.len() - 32;
    let sum = Sha256::digest(&bytes[..trailer]);
    bytes[trailer..].copy_from_slice(&sum);
    fs::write(path, bytes).unwrap();
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
    hex_of(&Sha256::digest(bytes))
}

fn hex_of(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(HEX[(byte >> 4) as usize]));
        out.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
    out
}
