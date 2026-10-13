//! Spawn the pinned producers. This service does not reimplement their work.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio_postgres::config::SslMode;

use crate::config::{self, Config};
use crate::ids::{ident_ok, plain_token, token_detail};
use crate::jsonutil::{parse_exact, parse_producer_document};
use crate::sha256::{contains_secret, sha256_hex};
use crate::signal::signal_tree;
use crate::store::{ImportRow, Store};
use crate::tls::pg_tls;

pub struct Produced {
    pub success: bool,
    pub producer_code: Option<String>,
    pub producer_detail: Option<String>,
    pub rows: Option<i64>,
    pub child_started: bool,
    pub catalog: Option<String>,
    pub profile: Option<String>,
}

pub async fn execute(config: &Config, store: &Store, operation: &ImportRow) -> Produced {
    let parent = config.run_dir.join(&operation.operation_id);
    if fs::DirBuilder::new().recursive(true).mode(0o700).create(&parent).is_err() && !parent.is_dir() {
        return fail("io", "io", false);
    }
    let attempt = parent.join(format!("attempt-{}", operation.attempt));
    let cancel = parent.join("cancel");
    if cancel.exists() {
        return fail("cancel", "canceled", false);
    }
    match operation.kind.as_str() {
        "pg_inspect" => inspect(config, store, operation, &parent, &attempt, &cancel).await,
        "file_replace" => replace(config, store, operation, &parent, &cancel).await,
        _ => fail("kind", "kind", false),
    }
}

async fn inspect(config: &Config, store: &Store, operation: &ImportRow, parent: &Path, attempt: &Path, cancel: &Path) -> Produced {
    let Some(source) = store.source(&operation.source_ref).await.ok().flatten() else {
        return fail("source", "source", false);
    };
    let Some(secret_ref) = source.secret_ref.as_deref() else {
        return fail("secret", "secret_ref", false);
    };
    let Some(secret_file) = config.secrets.get(secret_ref) else {
        return fail("secret", "secret_ref", false);
    };
    let tables = source.tables_json.as_deref().unwrap_or("[]");
    let Ok(table_value) = parse_exact(tables.as_bytes()) else {
        return fail("config", "tables", false);
    };
    let ready = parent.join("membership-ready");
    let body = json!({
        "protocol": "oo.onboarding.inspect/v1",
        "caller": "trusted_local_admin",
        "binaries": {
            "init_pg": pin_json(&config.producers.init_pg.path, &config.producers.init_pg.sha256),
            "import_pg": pin_json(&config.producers.pipeline.path, &config.producers.pipeline.sha256),
            "catalog_pg": pin_json(&config.producers.pipeline.path, &config.producers.pipeline.sha256),
            "captured_pg_draft": pin_json(&config.producers.builder.path, &config.producers.builder.sha256)
        },
        "source": {
            "source_id": source.source_ref,
            "host": source.host,
            "port": source.port,
            "database": source.database_name,
            "username": source.username,
            "secret_ref": secret_ref,
            "password_file": secret_file,
            "ca_file": &config.target.ca_file,
            "tables": table_value
        },
        "target": {
            "host": &config.target.host,
            "port": config.target.port,
            "database": &config.target.pg_database,
            "administrator": &config.target.administrator,
            "runtime_principal": &config.target.runtime_principal,
            "admin_password_file": &config.target.admin_password_file,
            "runtime_password_file": &config.target.runtime_password_file,
            "ca_file": &config.target.ca_file
        },
        "shard": format!("pg.{}", source.source_ref),
        "owner": &config.owner,
        "source_generation": 48,
        "connect_ms": 30000,
        "runtime_membership_ready_file": &ready,
        "membership_wait_ms": 60000,
        "comment_markers": &config.comment_markers,
        "deadlines_ms": {
            "import_not_ready_ms": 120000,
            "init_ms": 120000,
            "import_ms": 180000,
            "catalog_ms": 120000,
            "captured_ms": 60000
        }
    });
    let bytes = match serde_json::to_vec(&body) {
        Ok(bytes) => bytes,
        Err(_) => return fail("config", "json", false),
    };
    if contains_secret(&bytes, &config.secret_values) || !paths_are_plain(&body) {
        return fail("config", "secret", false);
    }
    let config_path = parent.join("inspect.json");
    if write_private(&config_path, &bytes).is_err() {
        return fail("config", "io", false);
    }
    let exit_path = attempt.join("steps").join("init-pg.exit");
    let database = config.target.pg_database.clone();
    let grant_config = config.clone_grant();
    let ready_grant = ready.clone();
    let grant = tokio::spawn(async move {
        grant_when_ready(&grant_config, &database, &exit_path, &ready_grant).await;
    });
    let bin = config.producers.pg_onboarding.path.clone();
    let secrets = config.secret_values.clone();
    let attempt_run = attempt.to_path_buf();
    let cancel_path = cancel.to_path_buf();
    let pid_file = parent.join("child.pid");
    let watcher = arm_pid(store, &operation.operation_id, &pid_file);
    let spawned = tokio::task::spawn_blocking(move || {
        run_child(
            &bin,
            &["inspect", "--config", &path_text(&config_path), "--run", &path_text(&attempt_run)],
            &cancel_path,
            &pid_file,
            Duration::from_secs(200),
            &secrets,
        )
    })
    .await;
    watcher.abort();
    grant.abort();
    let output = match spawned {
        Ok(output) => output,
        Err(_) => return fail("producer", "io", true),
    };
    if !output.started {
        return fail("producer", "io", false);
    }
    if output.overflow || contains_secret(&output.stdout, &config.secret_values) {
        return fail("producer", "redacted", true);
    }
    if output.code != Some(0) {
        let (code, detail) = stderr_token(&output.stderr);
        return Produced { success: false, producer_code: code, producer_detail: Some(detail), rows: None, child_started: true, catalog: None, profile: None };
    }
    let outcome = match read_json(&attempt.join("outcome.json"), &config.secret_values) {
        Ok(value) => value,
        Err(detail) => return fail("producer", detail, true),
    };
    let catalog = match read_text(&attempt.join("catalog.json"), &config.secret_values) {
        Ok(text) => text,
        Err(detail) => return fail("producer", detail, true),
    };
    if outcome.get("status").and_then(Value::as_str) != Some("awaiting_review") {
        let status = outcome.get("status").and_then(Value::as_str).unwrap_or("producer");
        let detail = if status == "ready" || status == "ready_for_trusted_query" { "ready_refused" } else { "status" };
        return fail("producer", detail, true);
    }
    let catalog_value = match parse_exact(catalog.as_bytes()) {
        Ok(value) => value,
        Err(_) => return fail("producer", "json", true),
    };
    if catalog_value.get("protocol").and_then(Value::as_str) != Some("oo.source.pgsql/v1") {
        return fail("producer", "protocol", true);
    }
    if config.comment_markers.iter().any(|marker| catalog.contains(marker)) {
        return fail("producer", "marker", true);
    }
    let profile = json!({
        "protocol": "oo.source-control.profile/v1",
        "kind": "pg_inspect",
        "phase": "awaiting_review",
        "source_ref": operation.source_ref,
        "tables": table_value,
        "published": false
    });
    let profile = match serde_json::to_string(&profile) {
        Ok(text) => text,
        Err(_) => return fail("producer", "json", true),
    };
    Produced { success: true, producer_code: None, producer_detail: None, rows: None, child_started: true, catalog: Some(catalog), profile: Some(profile) }
}

async fn replace(config: &Config, store: &Store, operation: &ImportRow, parent: &Path, cancel: &Path) -> Produced {
    let Ok(input) = parse_exact(operation.input_json.as_bytes()) else {
        return fail("config", "json", false);
    };
    let Some(sha) = input.get("object_sha256").and_then(Value::as_str) else {
        return fail("config", "sha256", false);
    };
    let Some(profile) = input.get("profile") else {
        return fail("config", "profile", false);
    };
    let Some(budget) = input.get("budget") else {
        return fail("config", "budget", false);
    };
    let Some(object) = store.object(&operation.source_ref, sha).await.ok().flatten() else {
        return fail("object", "object", false);
    };
    let object_path = config.object_dir.join(&object.stored_name);
    let bytes = match fs::read(&object_path) {
        Ok(bytes) => bytes,
        Err(_) => return fail("object", "io", false),
    };
    if sha256_hex(&bytes) != sha {
        return fail("object", "sha256", false);
    }
    let request = json!({
        "protocol": "oo.pipeline.file-import-pg/v1",
        "strategy": "replace",
        "parsing_mode": "trusted_public_fixture",
        "locale": "C",
        "type_policy": "preserve_row",
        "object": {"sha256": sha, "path": &object_path},
        "worker": {"sha256": &config.producers.worker.sha256, "path": &config.producers.worker.path},
        "profile": profile,
        "budget": budget,
        "target": {
            "host": &config.target.host,
            "port": config.target.port,
            "database": &config.target.file_database,
            "user": &config.target.runtime_principal,
            "server_name": &config.target.host,
            "ca_path": &config.target.ca_file
        },
        "fence": {"shard": format!("file.{}", operation.source_ref)},
        "mapping": {"namespace": "incoming", "table": operation.source_ref},
        "source": operation.source_ref
    });
    let request_bytes = match serde_json::to_vec(&request) {
        Ok(bytes) => bytes,
        Err(_) => return fail("config", "json", false),
    };
    if contains_secret(&request_bytes, &config.secret_values) {
        return fail("config", "secret", false);
    }
    let request_path = parent.join("request.json");
    if write_private(&request_path, &request_bytes).is_err() {
        return fail("config", "io", false);
    }
    let bin = config.producers.file_import.path.clone();
    let password = config.target.runtime_password_file.clone();
    let secrets = config.secret_values.clone();
    let cancel_path = cancel.to_path_buf();
    let pid_file = parent.join("child.pid");
    // A killed importer can still hold the session advisory lock. The producer
    // reports that race as database/sqlstate and does not wait. Retry only a
    // fast failure so a finished import that failed in the database is recorded once.
    let mut turn = 0u32;
    loop {
        let bin = bin.clone();
        let request_path = request_path.clone();
        let password = password.clone();
        let secrets = secrets.clone();
        let cancel_path = cancel_path.clone();
        let pid_file = pid_file.clone();
        let watcher = arm_pid(store, &operation.operation_id, &pid_file);
        let started = Instant::now();
        let spawned = tokio::task::spawn_blocking(move || {
            run_child(
                &bin,
                &[path_text(&request_path).as_str(), path_text(&password).as_str()],
                &cancel_path,
                &pid_file,
                Duration::from_secs(130),
                &secrets,
            )
        })
        .await;
        watcher.abort();
        let elapsed = started.elapsed();
        let output = match spawned {
            Ok(output) => output,
            Err(_) => return fail("producer", "io", true),
        };
        let produced = file_outcome(&output, profile, &config.secret_values);
        let lock_race = produced.producer_code.as_deref() == Some("database")
            && produced.producer_detail.as_deref() == Some("sqlstate")
            && elapsed < Duration::from_secs(4);
        if produced.success || !lock_race || turn == 7 {
            return produced;
        }
        turn += 1;
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn file_outcome(output: &ChildOutput, profile: &Value, secrets: &[Vec<u8>]) -> Produced {
    if !output.started {
        return fail("producer", "io", false);
    }
    if output.overflow || contains_secret(&output.stdout, secrets) || contains_secret(&output.stderr, secrets) {
        return fail("producer", "redacted", true);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let value = parse_exact(text.as_bytes()).unwrap_or(Value::Null);
    if output.code == Some(0) && value.get("protocol").and_then(Value::as_str) == Some("oo.pipeline.file-import-pg-result/v1") && value.get("state").and_then(Value::as_str) == Some("awaiting_review") && value.get("published") == Some(&Value::Bool(false))
    {
        let rows = value.get("rows").and_then(Value::as_u64).and_then(|rows| i64::try_from(rows).ok());
        let catalog = match serde_json::to_string(&value) {
            Ok(catalog) => catalog,
            Err(_) => return fail("producer", "json", true),
        };
        let profile = match serde_json::to_string(profile) {
            Ok(profile) => profile,
            Err(_) => return fail("producer", "json", true),
        };
        if contains_secret(catalog.as_bytes(), secrets) || contains_secret(profile.as_bytes(), secrets) {
            return fail("producer", "redacted", true);
        }
        return Produced { success: true, producer_code: None, producer_detail: None, rows, child_started: true, catalog: Some(catalog), profile: Some(profile) };
    }
    let state = value.get("state").and_then(Value::as_str).unwrap_or("");
    if state == "ready" || state == "ready_for_trusted_query" {
        return fail("producer", "ready_refused", true);
    }
    let code = value.get("code").and_then(Value::as_str).filter(|value| token_detail(value)).unwrap_or("producer");
    let detail = value.get("detail").and_then(Value::as_str).filter(|value| token_detail(value)).unwrap_or("redacted");
    Produced {
        success: false,
        producer_code: Some(code.to_string()),
        producer_detail: Some(detail.to_string()),
        rows: None,
        child_started: true,
        catalog: None,
        profile: None,
    }
}

struct GrantConfig {
    host: String,
    port: u16,
    administrator: String,
    runtime_principal: String,
    admin_password_file: PathBuf,
    ca_file: PathBuf,
}

impl Config {
    fn clone_grant(&self) -> GrantConfig {
        GrantConfig {
            host: self.target.host.clone(),
            port: self.target.port,
            administrator: self.target.administrator.clone(),
            runtime_principal: self.target.runtime_principal.clone(),
            admin_password_file: self.target.admin_password_file.clone(),
            ca_file: self.target.ca_file.clone(),
        }
    }
}

async fn grant_when_ready(config: &GrantConfig, database: &str, exit_path: &Path, ready: &Path) {
    if !ident_ok(database) || !ident_ok(&config.runtime_principal) || !ident_ok(&config.administrator) {
        return;
    }
    for _ in 0..900 {
        if let Ok(text) = fs::read_to_string(exit_path) {
            if text.trim() == "0" && grant(config, database).await.is_ok() {
                let _ = write_private(ready, b"ready\n");
            }
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn grant(config: &GrantConfig, database: &str) -> Result<(), ()> {
    let password = config::load_secret(&config.admin_password_file).map_err(|_| ())?;
    let password = String::from_utf8(password).map_err(|_| ())?;
    let mut pg = tokio_postgres::Config::new();
    pg.user(&config.administrator);
    pg.password(password);
    pg.host(&config.host);
    pg.port(config.port);
    pg.dbname("postgres");
    pg.application_name("oo-source-control-api");
    pg.ssl_mode(SslMode::Require);
    pg.connect_timeout(Duration::from_secs(10));
    let ca = fs::read(&config.ca_file).map_err(|_| ())?;
    let tls = pg_tls(&ca).map_err(|_| ())?;
    let (client, connection) = pg.connect(tls).await.map_err(|_| ())?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    for _ in 0..40 {
        let found = client
            .query_opt("SELECT 1 FROM pg_roles WHERE rolname = 'oo_pipeline_runtime'", &[])
            .await
            .map_err(|_| ())?;
        if found.is_some() {
            let role = &config.runtime_principal;
            client.batch_execute(&format!("GRANT CONNECT ON DATABASE {database} TO {role}")).await.map_err(|_| ())?;
            client
                .batch_execute(&format!("GRANT oo_pipeline_runtime TO {role} WITH INHERIT TRUE, SET FALSE"))
                .await
                .map_err(|_| ())?;
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err(())
}

struct ChildOutput {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    started: bool,
    overflow: bool,
}

fn arm_pid(store: &Store, operation_id: &str, pid_file: &Path) -> tokio::task::JoinHandle<()> {
    let store = store.clone();
    let operation_id = operation_id.to_string();
    let pid_file = pid_file.to_path_buf();
    tokio::spawn(async move {
        for _ in 0..600 {
            if let Ok(text) = fs::read_to_string(&pid_file)
                && let Ok(pid) = text.trim().parse::<i32>()
                && pid > 1
            {
                let _ = store.set_pid(&operation_id, pid).await;
                let _ = store.mark_started(&operation_id).await;
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
}

fn run_child(bin: &Path, args: &[&str], cancel: &Path, pid_file: &Path, deadline: Duration, secrets: &[Vec<u8>]) -> ChildOutput {
    let mut command = Command::new(bin);
    command
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/tmp")
        .env("LANG", "C.UTF-8")
        .current_dir("/tmp")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            return ChildOutput { code: None, stdout: Vec::new(), stderr: Vec::new(), started: false, overflow: false };
        }
    };
    let pid = child.id();
    if let Ok(pid) = i32::try_from(pid) {
        let _ = write_private(pid_file, format!("{pid}\n").as_bytes());
    }
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let out_thread = thread::spawn(move || read_capped(stdout, 8 * 1024 * 1024));
    let err_thread = thread::spawn(move || read_capped(stderr, 64 * 1024));
    let started_at = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if cancel.exists() || started_at.elapsed() >= deadline => {
                if let Ok(pid) = i32::try_from(pid) {
                    signal_tree(pid);
                }
                let _ = child.wait();
                break None;
            }
            Ok(None) => thread::sleep(Duration::from_millis(200)),
            Err(_) => break None,
        }
    };
    let (stdout, out_over) = out_thread.join().unwrap_or((Vec::new(), true));
    let (stderr, err_over) = err_thread.join().unwrap_or((Vec::new(), true));
    let overflow = out_over || err_over || contains_secret(&stdout, secrets) || contains_secret(&stderr, secrets);
    ChildOutput { code: status.and_then(|status| status.code()), stdout, stderr, started: true, overflow }
}

fn read_capped(pipe: Option<impl Read>, cap: usize) -> (Vec<u8>, bool) {
    let Some(mut pipe) = pipe else {
        return (Vec::new(), true);
    };
    let mut buf = Vec::new();
    let mut tmp = [0_u8; 8192];
    loop {
        match pipe.read(&mut tmp) {
            Ok(0) => return (buf, false),
            Ok(count) => {
                if buf.len().saturating_add(count) > cap {
                    return (Vec::new(), true);
                }
                buf.extend_from_slice(&tmp[..count]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return (buf, false),
        }
    }
}

fn stderr_token(stderr: &[u8]) -> (Option<String>, String) {
    let text = String::from_utf8_lossy(stderr);
    let line = text.lines().rev().find(|line| !line.is_empty()).unwrap_or("producer");
    let rest = line.strip_prefix("pg-onboarding: ").unwrap_or(line);
    let mut parts = rest.split_whitespace();
    let step = parts.next().unwrap_or("producer");
    let detail = parts.next().unwrap_or("producer");
    let step = if token_detail(step) { step.to_string() } else { "producer".to_string() };
    let detail = if token_detail(detail) { detail.to_string() } else { "redacted".to_string() };
    (Some(step), detail)
}

fn read_text(path: &Path, secrets: &[Vec<u8>]) -> Result<String, &'static str> {
    let bytes = fs::read(path).map_err(|_| "io")?;
    if bytes.len() > 8 * 1024 * 1024 || contains_secret(&bytes, secrets) {
        return Err("redacted");
    }
    String::from_utf8(bytes).map_err(|_| "json")
}

fn read_json(path: &Path, secrets: &[Vec<u8>]) -> Result<Value, &'static str> {
    let text = read_text(path, secrets)?;
    parse_producer_document(text.as_bytes()).map_err(|_| "json")
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), ()> {
    let mut file = OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path).map_err(|_| ())?;
    file.write_all(bytes).map_err(|_| ())?;
    Ok(())
}

fn pin_json(path: &Path, sha256: &str) -> Value {
    json!({"path": path, "sha256": sha256})
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn paths_are_plain(value: &Value) -> bool {
    match value {
        Value::String(text) if text.starts_with('/') || text.starts_with("secret://") => plain_token(text),
        Value::Object(map) => map.values().all(paths_are_plain),
        Value::Array(items) => items.iter().all(paths_are_plain),
        _ => true,
    }
}

fn fail(code: &'static str, detail: &'static str, started: bool) -> Produced {
    Produced {
        success: false,
        producer_code: Some(code.to_string()),
        producer_detail: Some(detail.to_string()),
        rows: None,
        child_started: started,
        catalog: None,
        profile: None,
    }
}
