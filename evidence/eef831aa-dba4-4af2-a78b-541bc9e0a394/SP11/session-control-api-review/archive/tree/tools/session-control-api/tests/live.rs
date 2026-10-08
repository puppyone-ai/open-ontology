#![forbid(unsafe_code)]

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use session_control_api::{fixture_stage, load_config, CancelTrigger, OWNER_UUID};
use session_store::rustls_connector;
use tokio_postgres::config::SslMode;

const MARKER: &str = "alpha-transcript-marker";

struct ExitChild(Option<Child>);

impl Drop for ExitChild {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[tokio::test]
async fn localprocess_s1_through_s11_closes_and_restarts() {
    let fixture = PathBuf::from(std::env::var("OO_A387_FIXTURE").expect("OO_A387_FIXTURE"));
    let root = PathBuf::from("/tmp/oo-a387-live");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("grants")).unwrap();
    fs::create_dir_all(root.join("run")).unwrap();
    write_bindings(&root.join("bindings.json"), "sales");
    let config_path = root.join("config.json");
    write_config(&config_path, &fixture, &root, "localprocess", None);
    let bin = env!("CARGO_BIN_EXE_session-control-api");

    let (code, out, err) = run_to_exit(bin, "install", &config_path, 60);
    assert_eq!(code, Some(0), "install failed {err}");
    assert!(!out.contains("password"), "install stdout leaked a field name");
    assert!(out.contains("\"production_signed\":false"), "{out}");

    let mut grants = Vec::new();
    for principal in ["alice", "bob", "casey"] {
        write_config(
            &config_path,
            &fixture,
            &root,
            "localprocess",
            Some((principal, root.join("grants").join(format!("{principal}.grant")))),
        );
        let (code, out, err) = run_to_exit(bin, "issue-grant", &config_path, 30);
        assert_eq!(code, Some(0), "issue {principal} {err}");
        assert!(!out.contains(&fs::read_to_string(root.join("grants").join(format!("{principal}.grant"))).unwrap()));
        grants.push(fs::read_to_string(root.join("grants").join(format!("{principal}.grant"))).unwrap());
    }
    write_config(&config_path, &fixture, &root, "production", None);
    let (code, _out, err) = run_to_exit(bin, "serve", &config_path, 15);
    assert_eq!(code, Some(2), "production serve must refuse, stderr={err}");
    assert!(err.contains("runtime_unavailable") || err.contains("production_driver_absent"), "{err}");
    assert!(!root.join("run/port.txt").exists() || fs::read_to_string(root.join("run/port.txt")).unwrap().trim().is_empty() || code == Some(2));

    write_config(&config_path, &fixture, &root, "localprocess", None);
    let mut server = Command::new(bin)
        .arg("serve")
        .stdin(Stdio::from(File::open(&config_path).unwrap()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let port = wait_port(&root.join("run/port.txt"), &mut server);
    let _guard = ExitChild(Some(server));
    let cfg = load_config(&fs::read(&config_path).unwrap()).unwrap();

    let alice = consume(port, &grants[0]);
    let bob = consume(port, &grants[1]);
    let casey = consume(port, &grants[2]);
    let again = http(port, "POST", "/control/v1/entry/session/consume", "", "", &serde_json::json!({"grant": grants[0].trim()}).to_string());
    assert_eq!(again.0, 409, "{}", again.2);

    let forged = http(port, "POST", "/control/v1/conversations", &alice.cookie, &alice.csrf, &serde_json::json!({"principal_id":"bob"}).to_string());
    assert_eq!(forged.0, 422, "{}", forged.2);

    let created = http(port, "POST", "/control/v1/conversations", &alice.cookie, &alice.csrf, "{}");
    assert_eq!(created.0, 201, "{}", created.2);
    let created_body: Value = serde_json::from_str(&created.2).unwrap();
    let conversation = created_body["conversation_id"].as_str().unwrap().to_owned();
    let session = created_body["session_id"].as_str().unwrap().to_owned();
    assert_eq!(created_body["phase"], "ready");
    assert_eq!(created_body["production_signed"], false);

    let listed = http(port, "GET", "/control/v1/conversations", &alice.cookie, "", "");
    assert_eq!(listed.0, 200, "{}", listed.2);
    assert!(listed.2.contains(&conversation));
    let bob_list = http(port, "GET", "/control/v1/conversations", &bob.cookie, "", "");
    assert_eq!(bob_list.0, 200, "{}", bob_list.2);
    assert!(!bob_list.2.contains(&conversation), "cross-person list leaked a conversation");
    let bob_get = http(port, "GET", &format!("/control/v1/conversations/{conversation}"), &bob.cookie, "", "");
    assert_eq!(bob_get.0, 403, "{}", bob_get.2);
    assert!(!bob_get.2.contains(MARKER));

    let early = http(port, "POST", &format!("/control/v1/conversations/{conversation}/cancel"), &alice.cookie, &alice.csrf, "{}");
    assert_eq!(early.0, 409, "{}", early.2);
    let early_body: Value = serde_json::from_str(&early.2).unwrap();
    assert_eq!(early_body["disposition"], "incomplete");
    assert_eq!(early_body["reason"], "backend_absent");
    assert_eq!(early_body["retryable"], true);
    assert_ne!(early_body["phase"], "closed");
    assert_eq!(early_body["production_signed"], false);

    let turned = http(
        port,
        "POST",
        &format!("/control/v1/conversations/{conversation}/turns"),
        &alice.cookie,
        &alice.csrf,
        &serde_json::json!({"turn_id":"turn-1","text": MARKER}).to_string(),
    );
    assert_eq!(turned.0, 200, "{}", turned.2);
    assert!(Path::new(&format!("/tmp/oo-a387-live/run/{OWNER_UUID}/secrets/{session}.token")).is_file());
    let events = http(port, "GET", &format!("/control/v1/conversations/{conversation}/events?limit=1"), &alice.cookie, "", "");
    assert_eq!(events.0, 200, "{}", events.2);
    assert_eq!(serde_json::from_str::<Value>(&events.2).unwrap()["events"].as_array().unwrap().len(), 1);
    let too_many = http(port, "GET", &format!("/control/v1/conversations/{conversation}/events?limit=51"), &alice.cookie, "", "");
    assert_eq!(too_many.0, 422, "{}", too_many.2);
    let output = http(port, "GET", &format!("/control/v1/conversations/{conversation}/outputs/transcript"), &alice.cookie, "", "");
    assert_eq!(output.0, 200, "output headers {}", output.1);
    assert!(output.2.contains(MARKER));
    let bob_output = http(port, "GET", &format!("/control/v1/conversations/{conversation}/outputs/transcript"), &bob.cookie, "", "");
    assert_eq!(bob_output.0, 403, "{}", bob_output.2);
    assert!(!bob_output.2.contains(MARKER), "cross-person output leaked bytes");
    let bob_term = http(port, "POST", &format!("/control/v1/conversations/{conversation}/terminate"), &bob.cookie, &bob.csrf, "{}");
    assert_eq!(bob_term.0, 403, "{}", bob_term.2);

    let before_pool = pool_names();
    let closed = http(port, "POST", &format!("/control/v1/conversations/{conversation}/cancel"), &alice.cookie, &alice.csrf, "{}");
    assert_eq!(closed.0, 200, "{}", closed.2);
    assert_closed(&closed.2, "closed");
    assert!(!Path::new(&format!("/tmp/oo-a387-live/run/{OWNER_UUID}/secrets/{session}.token")).exists());
    assert_ne!(before_pool, pool_names());
    assert!(before_pool.difference(&pool_names()).next().is_some());
    assert_eq!(active_backends(&fixture, &format!("q{session}")).await, 0);
    let pid = harness_pid(&fixture, &session).await.expect("harness pid");
    match fs::symlink_metadata(format!("/proc/{pid}")) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        other => panic!("harness still present: {other:?}"),
    }
    let replay = http(port, "POST", &format!("/control/v1/conversations/{conversation}/cancel"), &alice.cookie, &alice.csrf, "{}");
    assert_eq!(replay.0, 200, "{}", replay.2);
    assert_closed(&replay.2, "replay");
    let still = http(port, "GET", &format!("/control/v1/conversations/{conversation}/outputs/transcript"), &alice.cookie, "", "");
    assert_eq!(still.0, 200, "{}", still.1);
    assert!(still.2.contains(MARKER));

    let second = http(port, "POST", "/control/v1/conversations", &alice.cookie, &alice.csrf, "{}");
    assert_eq!(second.0, 201, "{}", second.2);
    let second_body: Value = serde_json::from_str(&second.2).unwrap();
    let conversation2 = second_body["conversation_id"].as_str().unwrap();
    let session2 = second_body["session_id"].as_str().unwrap().to_owned();
    let turn2 = http(
        port,
        "POST",
        &format!("/control/v1/conversations/{conversation2}/turns"),
        &alice.cookie,
        &alice.csrf,
        &serde_json::json!({"turn_id":"turn-2","text":"restart-marker"}).to_string(),
    );
    assert_eq!(turn2.0, 200, "{}", turn2.2);
    let project = "proj-a";
    let fixture_receipt = fixture_stage(&cfg, project, &session2, session_transition_user()).await.expect("fixture");
    assert_eq!(fixture_receipt.disposition, "fixture_persisted");
    assert_eq!(fixture_receipt.phase, "releasing");
    assert!(fixture_receipt.retryable);
    assert!(!fixture_receipt.acceptable());
    let snap_dir = PathBuf::from(format!("/tmp/oo-a387-live/run/{OWNER_UUID}/snapshots/{session2}"));
    fs::create_dir_all(&snap_dir).unwrap();
    fs::write(snap_dir.join("snapshot.bin"), b"keep").unwrap();
    let blocked = http(port, "POST", &format!("/control/v1/conversations/{conversation2}/cancel"), &alice.cookie, &alice.csrf, "{}");
    assert_eq!(blocked.0, 409, "{}", blocked.2);
    assert!(blocked.2.contains("snapshot_present_uncollected"), "{}", blocked.2);
    assert!(!blocked.2.contains("\"disposition\":\"closed\""));
    fs::remove_file(snap_dir.join("snapshot.bin")).unwrap();
    let resumed = http(port, "POST", &format!("/control/v1/conversations/{conversation2}/cancel"), &alice.cookie, &alice.csrf, "{}");
    assert_eq!(resumed.0, 200, "{}", resumed.2);
    assert_closed(&resumed.2, "closed");
    assert_eq!(active_backends(&fixture, &format!("q{session2}")).await, 0);

    write_bindings(&root.join("bindings.json"), "other");
    let denied = http(port, "GET", &format!("/control/v1/conversations/{conversation}/outputs/transcript"), &alice.cookie, "", "");
    assert_eq!(denied.0, 403, "{}", denied.2);
    assert!(denied.2.contains("permission_changed"), "{}", denied.2);
    assert!(!denied.2.contains(MARKER));
    write_bindings(&root.join("bindings.json"), "sales");

    let third = http(port, "POST", "/control/v1/conversations", &alice.cookie, &alice.csrf, "{}");
    assert_eq!(third.0, 201, "{}", third.2);
    let third_body: Value = serde_json::from_str(&third.2).unwrap();
    let conversation3 = third_body["conversation_id"].as_str().unwrap();
    let session3 = third_body["session_id"].as_str().unwrap();
    let turn3 = http(
        port,
        "POST",
        &format!("/control/v1/conversations/{conversation3}/turns"),
        &alice.cookie,
        &alice.csrf,
        &serde_json::json!({"turn_id":"turn-3","text":"admin-marker"}).to_string(),
    );
    assert_eq!(turn3.0, 200, "{}", turn3.2);
    let terminated = http(port, "POST", &format!("/control/v1/conversations/{conversation3}/terminate"), &casey.cookie, &casey.csrf, "{}");
    assert_eq!(terminated.0, 200, "{}", terminated.2);
    assert_closed(&terminated.2, "closed");
    assert_eq!(active_backends(&fixture, &format!("q{session3}")).await, 0);
    let _ = session3;
}

fn session_transition_user() -> CancelTrigger {
    CancelTrigger::User
}

struct Session {
    cookie: String,
    csrf: String,
}

fn consume(port: u16, grant: &str) -> Session {
    let response = http(port, "POST", "/control/v1/entry/session/consume", "", "", &serde_json::json!({"grant": grant.trim()}).to_string());
    assert_eq!(response.0, 200, "{}", response.2);
    let body: Value = serde_json::from_str(&response.2).unwrap();
    let cookie = response
        .1
        .lines()
        .find_map(|line| line.to_ascii_lowercase().strip_prefix("set-cookie:").map(|value| value.trim().split(';').next().unwrap_or("").to_owned()))
        .expect("cookie");
    assert!(cookie.starts_with("oo-entry="));
    Session { cookie, csrf: body["csrf"].as_str().unwrap().to_owned() }
}

fn assert_closed(body: &str, disposition: &str) {
    let value: Value = serde_json::from_str(body).unwrap();
    assert_eq!(value["disposition"], disposition, "{body}");
    assert_eq!(value["phase"], "closed", "{body}");
    assert_eq!(value["production_signed"], false, "{body}");
    assert_eq!(value["deployment"], "localprocess", "{body}");
    assert_eq!(value["retryable"], false, "{body}");
    assert_eq!(value["reducer_physical_release_certified"], false, "{body}");
    let steps = value["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 11, "{body}");
    let step = |id: &str| steps.iter().find(|step| step["id"] == id).unwrap_or_else(|| panic!("missing {id}"));
    assert_eq!(step("s8")["status"], "confirmed");
    assert_eq!(step("s8")["actual_absence"], true);
    assert_eq!(step("s5")["status"], "skipped");
    assert_eq!(step("s5")["actual_absence"], false);
    for id in ["s1", "s2", "s3", "s4", "s6", "s7", "s9", "s10", "s11"] {
        assert_eq!(step(id)["status"], "confirmed", "{id}");
        assert_eq!(step(id)["actual_absence"], false, "{id}");
    }
}

fn pool_names() -> std::collections::BTreeSet<String> {
    let pool = PathBuf::from(format!("/tmp/oo-a387-live/run/{OWNER_UUID}/pool"));
    fs::read_dir(&pool)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

fn write_bindings(path: &Path, dept: &str) {
    let body = serde_json::json!({
        "bindings": [
            binding("alice", "analyst", dept, &["session.use"]),
            binding("bob", "analyst", "sales", &["session.use"]),
            binding("casey", "operator", "sales", &["session.use", "session.terminate"])
        ]
    });
    fs::write(path, serde_json::to_vec_pretty(&body).unwrap()).unwrap();
}

fn binding(principal: &str, role: &str, dept: &str, caps: &[&str]) -> Value {
    serde_json::json!({
        "principal_id": principal,
        "project_id": "proj-a",
        "role": role,
        "claims": [["dept", dept]],
        "policy_version": "pol-1",
        "knowledge_version": "know-1",
        "config_version": "cfg-1",
        "capabilities": caps,
    })
}

fn write_config(path: &Path, fixture: &Path, root: &Path, profile: &str, issue: Option<(&str, PathBuf)>) {
    let endpoint = fs::read_to_string(fixture.join("endpoint.txt")).unwrap();
    let mut lines = endpoint.lines();
    let host = lines.next().unwrap();
    let port: u16 = lines.next().unwrap().parse().unwrap();
    let database = lines.next().unwrap();
    let mut value = serde_json::json!({
        "contract": "session-control-api-v1",
        "profile": profile,
        "bind": "127.0.0.1:0",
        "owner_uuid": OWNER_UUID,
        "state_dir": root.join("run"),
        "bindings_path": root.join("bindings.json"),
        "audience_url": "https://gateway.example/v1",
        "pg": {
            "host": host,
            "port": port,
            "database": database,
            "ca_pem": fs::read_to_string(fixture.join("ca.crt")).unwrap(),
            "api_admin_password": read_secret(&fixture.join("api-admin.pass")),
            "api_runtime_password": read_secret(&fixture.join("api-runtime.pass")),
            "session_admin_password": read_secret(&fixture.join("admin.pass")),
            "session_runtime_password": read_secret(&fixture.join("runtime.pass")),
            "cancel_password": read_secret(&fixture.join("cancel.pass")),
            "query_password": read_secret(&fixture.join("query.pass")),
            "token_admin_password": read_secret(&fixture.join("token-admin.pass")),
            "token_runtime_password": read_secret(&fixture.join("token-runtime.pass")),
        }
    });
    if let Some((principal, grant_out)) = issue {
        value["issue"] = serde_json::json!({
            "principal_id": principal,
            "project_id": "proj-a",
            "grant_out": grant_out,
        });
    }
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    let mut file = OpenOptions::new().create_new(true).write(true).mode(0o600).open(path).unwrap();
    file.write_all(&serde_json::to_vec(&value).unwrap()).unwrap();
    file.sync_all().unwrap();
}

fn read_secret(path: &Path) -> String {
    fs::read_to_string(path).unwrap().trim().to_owned()
}

fn run_to_exit(bin: &str, cmd: &str, config: &Path, seconds: u64) -> (Option<i32>, String, String) {
    let mut child = Command::new(bin)
        .arg(cmd)
        .stdin(Stdio::from(File::open(config).unwrap()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    let code = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status.code().unwrap_or(99));
        }
        if start.elapsed() > Duration::from_secs(seconds) {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        thread::sleep(Duration::from_millis(50));
    };
    let mut out = String::new();
    let mut err = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut out);
    }
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut err);
    }
    (code, out, err)
}

fn wait_port(path: &Path, child: &mut Child) -> u16 {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("server exited {} before listen", status.code().unwrap_or(99));
        }
        if let Ok(text) = fs::read_to_string(path)
            && let Ok(port) = text.trim().parse::<u16>()
            && port != 0
        {
            return port;
        }
        if start.elapsed() > Duration::from_secs(20) {
            panic!("server did not write a port");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn http(port: u16, method: &str, path: &str, cookie: &str, csrf: &str, body: &str) -> (u16, String, String) {
    use std::net::TcpStream;
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(90))).unwrap();
    stream.set_write_timeout(Some(Duration::from_secs(20))).unwrap();
    let mut head = format!("{method} {path} HTTP/1.1\r\nhost: 127.0.0.1\r\ncontent-length: {}\r\nconnection: close\r\n", body.len());
    if !cookie.is_empty() {
        head.push_str(&format!("cookie: {cookie}\r\n"));
    }
    if !csrf.is_empty() {
        head.push_str(&format!("x-oo-csrf: {csrf}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).unwrap();
    stream.write_all(body.as_bytes()).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8_lossy(&raw);
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_ref(), ""));
    let status = head.lines().next().unwrap_or("").split(' ').nth(1).unwrap_or("0").parse().unwrap_or(0);
    (status, head.to_owned(), body.to_owned())
}

async fn active_backends(fixture: &Path, application: &str) -> i64 {
    let client = connect(fixture, "oo_fixture_query", "query.pass", "oo-a387-observe").await;
    let row = client
        .query_one(
            "SELECT count(*) FROM pg_catalog.pg_stat_activity WHERE application_name = $1 AND state = 'active'",
            &[&application],
        )
        .await
        .unwrap();
    row.get(0)
}

async fn harness_pid(fixture: &Path, session_id: &str) -> Option<i64> {
    let client = connect(fixture, "oo_session_api_runtime", "api-runtime.pass", "oo-a387-observe").await;
    let row = client
        .query_one("SELECT harness_pid FROM oo_session_api.resource WHERE session_id = $1", &[&session_id])
        .await
        .unwrap();
    row.get(0)
}

async fn connect(fixture: &Path, user: &str, pass_file: &str, application: &str) -> tokio_postgres::Client {
    let endpoint = fs::read_to_string(fixture.join("endpoint.txt")).unwrap();
    let mut lines = endpoint.lines();
    let host = lines.next().unwrap();
    let port: u16 = lines.next().unwrap().parse().unwrap();
    let database = lines.next().unwrap();
    let ca = fs::read(fixture.join("ca.crt")).unwrap();
    let password = read_secret(&fixture.join(pass_file));
    let tls = rustls_connector(&ca).unwrap();
    let mut config = tokio_postgres::Config::new();
    config.user(user);
    config.password(&password);
    config.host(host);
    config.port(port);
    config.dbname(database);
    config.ssl_mode(SslMode::Require);
    config.application_name(application);
    let (client, connection) = config.connect(tls).await.unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}
