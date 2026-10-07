//! Private PostgreSQL proof. `OO_A308_LIVE=1` arms it. The library copies the bytes.

#![forbid(unsafe_code)]

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use file_import_pg::{
    error_document, exit_code, import_replace, parse_request, physical_plan, stage_replace, ImportError,
    ImportRequest, ImportResult, Stage,
};
use oo_pipeline::target_installer::{install, install_full_import};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};

const DATABASE: &str = "oo_a308_file";
const RUNTIME_USER: &str = "oo_a308_runtime";
const AGENT_USER: &str = "oo_a308_agent";

#[tokio::test]
#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
async fn live_replace_import() {
    if std::env::var("OO_A308_LIVE").ok().as_deref() != Some("1") {
        return;
    }
    let host = env("OO_A308_PG_HOST");
    let port: u16 = env("OO_A308_PG_PORT").parse().unwrap_or_else(|_| panic!("env port"));
    let admin_user = env("OO_A308_ADMIN_USER");
    let admin_password = secret(&env("OO_A308_ADMIN_PASS_FILE"));
    let runtime_password = secret(&env("OO_A308_RUNTIME_PASS_FILE"));
    let agent_password = secret(&env("OO_A308_AGENT_PASS_FILE"));
    let ca = std::fs::read(env("OO_A308_CA_FILE")).unwrap_or_else(|_| panic!("ca"));
    let worker = env("OO_A308_WORKER");
    let worker_sha = sha_file(Path::new(&worker));
    let dir = std::env::temp_dir().join("oo-a308-live");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_else(|_| panic!("dir"));

    let mut bootstrap = dial(&host, port, &admin_user, &admin_password, "postgres", &ca).await;
    ensure_role(&mut bootstrap, RUNTIME_USER, &runtime_password).await;
    ensure_role(&mut bootstrap, AGENT_USER, &agent_password).await;
    ensure_database(&mut bootstrap).await;
    drop(bootstrap);
    let mut admin = dial(&host, port, &admin_user, &admin_password, DATABASE, &ca).await;
    if install(&mut admin).await.is_err() {
        panic!("install failed");
    }
    if install_full_import(&mut admin).await.is_err() {
        panic!("install failed");
    }
    exec(
        &mut admin,
        "REVOKE ALL ON DATABASE oo_a308_file FROM PUBLIC",
    )
    .await;
    exec(&mut admin, "GRANT CONNECT ON DATABASE oo_a308_file TO oo_a308_runtime").await;
    exec(&mut admin, "GRANT CONNECT ON DATABASE oo_a308_file TO oo_a308_agent").await;
    exec(&mut admin, "GRANT oo_pipeline_runtime TO oo_a308_runtime").await;
    let roles_before = scalar(&mut admin, "SELECT count(*)::text FROM pg_catalog.pg_roles").await;

    let world = World {
        host,
        port,
        runtime_password,
        agent_password,
        ca,
        worker,
        worker_sha,
        dir,
    };

    let csv_body = "\
id,n,amount,flag,note,payload
a,-9223372036854775808,1.50,true,\"say, \"\"hi\"\"\",\"{\"\"a\"\":1}\"
b,9223372036854775807,,false,\"\",
c,,0.00,true,plain,null
";
    let csv_columns = json!([
        column("id", "utf8", "text", false, true, None, None),
        column("n", "int64", "int8", true, false, None, None),
        column("amount", "decimal", "numeric", true, false, Some(10), Some(2)),
        column("flag", "bool", "bool", false, false, None, None),
        column("note", "utf8", "text", true, false, None, None),
        column("payload", "utf8", "json", true, false, None, None)
    ]);
    let inserted = world
        .import(
            "rows.csv",
            csv_body.as_bytes(),
            "csv",
            "rows",
            "src_csv",
            "rows_csv",
            "filecsv",
            &csv_columns,
            json!({"policy": "empty"}),
            Some("refuse"),
            Some("exact"),
        )
        .await
        .expect("csv import");
    assert_result(&inserted, false, 3);
    let run_id = inserted.run_id.clone().unwrap_or_else(|| panic!("run"));
    assert_eq!(
        scalar_bind(&mut admin, "SELECT state FROM _oo.full_import_run WHERE run_id = $1::uuid", &run_id).await,
        "awaiting_publish"
    );
    assert_eq!(scalar(&mut admin, "SELECT count(*)::text FROM pg_catalog.pg_roles").await, roles_before);
    let generation = scalar_bind(
        &mut admin,
        "SELECT generation::text FROM _oo.target_generations WHERE shard_id = $1 ORDER BY generation DESC LIMIT 1",
        "filecsv",
    )
    .await;
    assert_eq!(generation, inserted.target_generation.unwrap_or(-1).to_string());
    assert_eq!(
        scalar_bind(
            &mut admin,
            "SELECT relpersistence::text FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = '_oo' AND c.relname = $1",
            &inserted.physical_table,
        )
        .await,
        "p"
    );
    assert_eq!(
        scalar_bind(
            &mut admin,
            "SELECT count(*)::text FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace CROSS JOIN LATERAL aclexplode(c.relacl) AS grant_row WHERE n.nspname = '_oo' AND c.relname = $1 AND grant_row.grantee = 0",
            &inserted.physical_table,
        )
        .await,
        "0"
    );
    let values = shadow_rows(&mut admin, &inserted.physical_table, &inserted.physical_columns).await;
    assert_eq!(
        values,
        vec![
            row(["a", "-9223372036854775808", "1.50", "t", "say, \"hi\"", "{\"a\":1}"]),
            vec![Some("b".to_string()), Some("9223372036854775807".to_string()), None, Some("f".to_string()), Some(String::new()), None],
            vec![Some("c".to_string()), None, Some("0.00".to_string()), Some("t".to_string()), Some("plain".to_string()), Some("null".to_string())],
        ]
    );
    denied(&world, RUNTIME_USER, &world.runtime_password, &inserted.physical_table).await;
    denied(&world, AGENT_USER, &world.agent_password, &inserted.physical_table).await;
    assert_worker_gone();

    let replayed = world
        .import(
            "rows.csv",
            csv_body.as_bytes(),
            "csv",
            "rows",
            "src_csv",
            "rows_csv",
            "filecsv",
            &csv_columns,
            json!({"policy": "empty"}),
            Some("refuse"),
            Some("exact"),
        )
        .await
        .expect("replay");
    assert_result(&replayed, true, 3);
    assert!(replayed.run_id.is_none());
    assert!(replayed.target_generation.is_none());
    assert_eq!(replayed.physical_table, inserted.physical_table);
    assert_eq!(
        scalar_bind(
            &mut admin,
            "SELECT generation::text FROM _oo.target_generations WHERE shard_id = $1 ORDER BY generation DESC LIMIT 1",
            "filecsv",
        )
        .await,
        generation
    );
    assert_eq!(count_shadow(&mut admin, &inserted.physical_table).await, "3");
    assert_worker_gone();

    let mut changed = csv_body.to_string();
    changed = changed.replace("plain", "other");
    let conflict = world
        .import(
            "rows-changed.csv",
            changed.as_bytes(),
            "csv",
            "rows",
            "src_csv",
            "rows_csv",
            "filecsv",
            &csv_columns,
            json!({"policy": "empty"}),
            Some("refuse"),
            Some("exact"),
        )
        .await
        .expect_err("conflict");
    assert_eq!((conflict.code, conflict.detail, conflict.child_started), ("hash_conflict", "receipt", true));
    assert_eq!(exit_code(&conflict), 3);
    assert!(error_document(&conflict).contains("\"published\":false"));
    assert_eq!(count_shadow(&mut admin, &inserted.physical_table).await, "3");
    assert_eq!(
        scalar_bind(
            &mut admin,
            "SELECT generation::text FROM _oo.target_generations WHERE shard_id = $1 ORDER BY generation DESC LIMIT 1",
            "filecsv",
        )
        .await,
        generation
    );
    assert_worker_gone();

    let tsv_columns = json!([
        column("id", "utf8", "text", false, true, None, None),
        column("note", "utf8", "text", true, false, None, None)
    ]);
    let tsv = world
        .import(
            "rows.tsv",
            b"id\tnote\nq\t\"hello\tworld\"\n",
            "tsv",
            "tsvrows",
            "src_tsv",
            "rows_tsv",
            "filetsv",
            &tsv_columns,
            json!({"policy": "empty"}),
            Some("refuse"),
            Some("exact"),
        )
        .await
        .expect("tsv");
    assert_result(&tsv, false, 1);
    assert_eq!(
        shadow_rows(&mut admin, &tsv.physical_table, &tsv.physical_columns).await,
        vec![vec![Some("q".to_string()), Some("hello\tworld".to_string())]]
    );
    assert_worker_gone();

    let token_columns = tsv_columns.clone();
    let token = world
        .import(
            "token.csv",
            b"id,note\n1,\\N\n2,\"\\N\"\n3,\n4,\"\"\n",
            "csv",
            "tokenrows",
            "src_token",
            "rows_token",
            "filetoken",
            &token_columns,
            json!({"policy": "token", "token": "\\N"}),
            Some("refuse"),
            Some("exact"),
        )
        .await
        .expect("token");
    assert_result(&token, false, 4);
    assert_eq!(
        shadow_rows(&mut admin, &token.physical_table, &token.physical_columns).await,
        vec![
            vec![Some("1".to_string()), None],
            vec![Some("2".to_string()), Some("\\N".to_string())],
            vec![Some("3".to_string()), Some(String::new())],
            vec![Some("4".to_string()), Some(String::new())],
        ]
    );
    assert_worker_gone();

    let json_columns = json!([
        column("id", "utf8", "text", false, true, None, None),
        column("payload", "json_text", "json", true, false, None, None)
    ]);
    let jsonl = world
        .import(
            "rows.jsonl",
            b"{\"id\":\"j1\",\"payload\":{\"a\":1}}\n{\"id\":\"j2\",\"payload\":null}\n",
            "jsonl",
            "jsonrows",
            "src_jsonl",
            "rows_jsonl",
            "filejsonl",
            &json_columns,
            json!({"policy": "json"}),
            None,
            None,
        )
        .await
        .expect("jsonl");
    assert_result(&jsonl, false, 2);
    assert_eq!(
        shadow_rows(&mut admin, &jsonl.physical_table, &jsonl.physical_columns).await,
        vec![
            vec![Some("j1".to_string()), Some("{\"a\":1}".to_string())],
            vec![Some("j2".to_string()), None],
        ]
    );
    assert_worker_gone();

    let runs_before = scalar(&mut admin, "SELECT count(*)::text FROM _oo.full_import_run").await;
    refuse_absent(
        &mut admin,
        &world,
        "missing.jsonl",
        b"{\"id\":\"m1\"}\n",
        "jsonl",
        "missingrows",
        "src_missing",
        "rows_missing",
        "filemissing",
        &json_columns,
        json!({"policy": "json"}),
        None,
        None,
        "missing_field",
        "missing_field",
        true,
        2,
    )
    .await;
    assert_eq!(scalar(&mut admin, "SELECT count(*)::text FROM _oo.full_import_run").await, runs_before);

    let id_note = tsv_columns.clone();
    refuse_absent(
        &mut admin,
        &world,
        "drift.csv",
        b"id\n1\n",
        "csv",
        "driftrows",
        "src_drift",
        "rows_drift",
        "filedrift",
        &json!([column("id", "utf8", "text", false, true, None, None)]),
        json!({"policy": "empty"}),
        Some("refuse"),
        Some("exact"),
        "hash_drift",
        "object",
        false,
        2,
    )
    .await;

    refuse_absent(
        &mut admin,
        &world,
        "bad.csv",
        b"id,note\na,b,c\n",
        "csv",
        "badrows",
        "src_bad",
        "rows_bad",
        "filebad",
        &id_note,
        json!({"policy": "empty"}),
        Some("refuse"),
        Some("exact"),
        "malformed",
        "protocol",
        true,
        4,
    )
    .await;
    refuse_absent(
        &mut admin,
        &world,
        "trunc.jsonl",
        b"{\"id\":\"t1\"}",
        "jsonl",
        "truncrows",
        "src_trunc",
        "rows_trunc",
        "filetrunc",
        &json!([column("id", "utf8", "text", false, true, None, None)]),
        json!({"policy": "json"}),
        None,
        None,
        "truncated",
        "truncated",
        true,
        4,
    )
    .await;
    refuse_absent(
        &mut admin,
        &world,
        "over.csv",
        b"id,n\na,9223372036854775808\n",
        "csv",
        "overrows",
        "src_over",
        "rows_over",
        "fileover",
        &json!([
            column("id", "utf8", "text", false, true, None, None),
            column("n", "int64", "int8", true, false, None, None)
        ]),
        json!({"policy": "empty"}),
        Some("refuse"),
        Some("exact"),
        "malformed",
        "protocol",
        true,
        4,
    )
    .await;

    // pg_terminate_backend asks the server to SIGTERM the backend. This does not
    // claim the signal arrived inside a COPY packet.
    let kill_columns = id_note;
    let kill_path = world.write("kill.csv", b"id,note\nk,kept\n");
    let kill_request = world.request(
        &kill_path,
        "csv",
        "killrows",
        "src_kill",
        "rows_kill",
        "filekill",
        &kill_columns,
        json!({"policy": "empty"}),
        Some("refuse"),
        Some("exact"),
    );
    let names = physical_plan(&kill_request).expect("kill names");
    let staged = match stage_replace(&kill_request, &world.runtime_password).await {
        Ok(stage) => stage,
        Err(error) => panic!("stage {} {}", error.code, error.detail),
    };
    let Stage::Ready(staged) = staged else { panic!("stage"); };
    let pid = staged.backend_pid();
    let table = staged.physical_table().to_string();
    assert_eq!(table, names.table);
    let terminated: bool = match sqlx::query_scalar("SELECT pg_catalog.pg_terminate_backend($1)")
        .bind(pid)
        .fetch_one(&mut admin)
        .await
    {
        Ok(value) => value,
        Err(error) => panic!("terminate {}", sqlstate(&error)),
    };
    if !terminated {
        panic!("terminate");
    }
    let mut gone = false;
    for _ in 0..20 {
        let alive: bool = match sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_stat_activity WHERE pid = $1)")
            .bind(pid)
            .fetch_one(&mut admin)
            .await
        {
            Ok(value) => value,
            Err(error) => panic!("pid {}", sqlstate(&error)),
        };
        if !alive {
            gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    if !gone {
        panic!("backend");
    }
    drop(staged);
    assert!(regclass(&mut admin, &table).await.is_none());
    let retried = import_replace(&kill_request, &world.runtime_password).await.expect("retry");
    assert_result(&retried, false, 1);
    assert_eq!(count_shadow(&mut admin, &retried.physical_table).await, "1");
    assert_eq!(
        shadow_rows(&mut admin, &retried.physical_table, &retried.physical_columns).await,
        vec![vec![Some("k".to_string()), Some("kept".to_string())]]
    );
    assert_worker_gone();
}

struct World {
    host: String,
    port: u16,
    runtime_password: String,
    agent_password: String,
    ca: Vec<u8>,
    worker: String,
    worker_sha: String,
    dir: PathBuf,
}

impl World {
    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.dir.join(name);
        std::fs::write(&path, bytes).unwrap_or_else(|_| panic!("fixture"));
        path
    }

    #[allow(clippy::too_many_arguments)]
    fn request(
        &self,
        path: &Path,
        format: &str,
        profile_id: &str,
        source: &str,
        table: &str,
        shard: &str,
        columns: &Value,
        null_policy: Value,
        bom: Option<&str>,
        header: Option<&str>,
    ) -> ImportRequest {
        let mut profile = json!({
            "format": format,
            "id": profile_id,
            "null": null_policy,
            "columns": columns,
        });
        if format != "jsonl" {
            profile["bom"] = json!(bom.unwrap_or("refuse"));
            profile["header"] = json!(header.unwrap_or("exact"));
        }
        let text = json!({
            "protocol": "oo.pipeline.file-import-pg/v1",
            "strategy": "replace",
            "parsing_mode": "trusted_public_fixture",
            "locale": "C",
            "type_policy": "preserve_row",
            "object": {"sha256": sha_file(path), "path": path_text(path)},
            "worker": {"sha256": self.worker_sha, "path": self.worker},
            "profile": profile,
            "budget": {
                "batch_rows": 32,
                "batch_bytes": 1_048_576,
                "max_record_bytes": 65_536,
                "max_field_bytes": 16_384,
                "max_total_bytes": 4_194_304,
                "max_rows": 1000,
                "max_columns": 16,
                "max_fields": 64,
                "worker_deadline_ms": 30_000,
                "parent_deadline_ms": 60_000
            },
            "target": {
                "host": self.host,
                "port": self.port,
                "database": DATABASE,
                "user": RUNTIME_USER,
                "server_name": self.host,
                "ca_path": env("OO_A308_CA_FILE")
            },
            "fence": {"shard": shard},
            "mapping": {"namespace": "fileimp", "table": table},
            "source": source
        })
        .to_string();
        parse_request(&text).expect("request")
    }

    #[allow(clippy::too_many_arguments)]
    async fn import(
        &self,
        name: &str,
        bytes: &[u8],
        format: &str,
        profile_id: &str,
        source: &str,
        table: &str,
        shard: &str,
        columns: &Value,
        null_policy: Value,
        bom: Option<&str>,
        header: Option<&str>,
    ) -> Result<ImportResult, ImportError> {
        let path = self.write(name, bytes);
        let request = self.request(&path, format, profile_id, source, table, shard, columns, null_policy, bom, header);
        import_replace(&request, &self.runtime_password).await
    }
}

fn assert_result(result: &ImportResult, replay: bool, rows: u64) {
    assert_eq!(result.protocol, "oo.pipeline.file-import-pg-result/v1");
    assert_eq!(result.state, "awaiting_review");
    assert!(!result.published);
    assert!(!result.role_grant);
    assert_eq!(result.stored_run_state, "awaiting_publish");
    assert_eq!(result.replay, replay);
    assert_eq!(result.rows, rows);
    assert_eq!(result.parsing_mode, "trusted_public_fixture");
    assert_eq!(result.kernel_restriction, "unsupported");
    assert!(result.child_reaped);
    assert_eq!(result.stored_position_codec, "snapshot-zero/v1");
    assert_eq!(result.stored_position_lineage, "full-import.snapshot");
    assert!(result.physical_table.starts_with("pt_") && result.physical_table.len() == 63);
}

fn column(
    name: &str,
    parse: &str,
    pg: &str,
    nullable: bool,
    primary_key: bool,
    precision: Option<u8>,
    scale: Option<i8>,
) -> Value {
    json!({
        "name": name,
        "parse": parse,
        "pg": pg,
        "nullable": nullable,
        "primary_key": primary_key,
        "precision": precision,
        "scale": scale,
    })
}

fn row(cells: [&str; 6]) -> Vec<Option<String>> {
    cells.into_iter().map(|cell| Some(cell.to_string())).collect()
}

#[allow(clippy::too_many_arguments)]
async fn refuse_absent(
    admin: &mut PgConnection,
    world: &World,
    name: &str,
    bytes: &[u8],
    format: &str,
    profile_id: &str,
    source: &str,
    table: &str,
    shard: &str,
    columns: &Value,
    null_policy: Value,
    bom: Option<&str>,
    header: Option<&str>,
    code: &str,
    detail: &str,
    child_started: bool,
    status: i32,
) {
    let path = world.write(name, bytes);
    let request = world.request(&path, format, profile_id, source, table, shard, columns, null_policy, bom, header);
    if code == "hash_drift" {
        std::fs::write(&path, b"id\n2\n").unwrap_or_else(|_| panic!("fixture"));
    }
    let names = physical_plan(&request).expect("names");
    let runs = scalar(admin, "SELECT count(*)::text FROM _oo.full_import_run").await;
    let error = import_replace(&request, &world.runtime_password).await.expect_err("refused");
    assert_eq!((error.code, error.detail, error.child_started), (code, detail, child_started));
    assert_eq!(exit_code(&error), status);
    assert!(error_document(&error).contains("\"published\":false"));
    assert!(regclass(admin, &names.table).await.is_none());
    assert_eq!(scalar(admin, "SELECT count(*)::text FROM _oo.full_import_run").await, runs);
    assert_worker_gone();
}

fn env(name: &str) -> String {
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => value,
        _ => panic!("env {name}"),
    }
}

fn secret(path: &str) -> String {
    let bytes = std::fs::read(path).unwrap_or_else(|_| panic!("pass"));
    let mut text = String::from_utf8(bytes).unwrap_or_else(|_| panic!("pass"));
    if let Some(stripped) = text.strip_suffix('\n') {
        text = stripped.to_string();
        if let Some(stripped) = text.strip_suffix('\r') {
            text = stripped.to_string();
        }
    }
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        panic!("pass");
    }
    text
}

fn sha_file(path: &Path) -> String {
    let mut file = std::fs::File::open(path).unwrap_or_else(|_| panic!("hash"));
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let read = file.read(&mut buf).unwrap_or_else(|_| panic!("hash"));
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    hex(hasher.finalize().as_slice())
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn path_text(path: &Path) -> String {
    let text = path.to_str().unwrap_or_else(|| panic!("path"));
    if !text.starts_with('/') {
        panic!("path");
    }
    text.to_string()
}

fn quote_physical(name: &str) -> String {
    let ok = name.len() == 63
        && (name.starts_with("pn_") || name.starts_with("pt_") || name.starts_with("pc_"))
        && name.bytes().skip(3).all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase());
    if !ok {
        panic!("physical");
    }
    format!("\"{name}\"")
}

fn sqlstate(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .and_then(|database| database.code())
        .map(|code| code.to_string())
        .unwrap_or_default()
}

async fn dial(host: &str, port: u16, user: &str, password: &str, database: &str, ca: &[u8]) -> PgConnection {
    let options = PgConnectOptions::new()
        .host(host)
        .port(port)
        .username(user)
        .password(password)
        .database(database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec())
        .disable_statement_logging();
    match PgConnection::connect_with(&options).await {
        Ok(conn) => conn,
        Err(_) => panic!("connect"),
    }
}

async fn exec(conn: &mut PgConnection, sql: &'static str) {
    if sqlx::raw_sql(sqlx::AssertSqlSafe(sql)).execute(&mut *conn).await.is_err() {
        panic!("sql");
    }
}

async fn scalar(conn: &mut PgConnection, sql: &str) -> String {
    match sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&mut *conn)
        .await
    {
        Ok(value) => value,
        Err(error) => panic!("sql {}", sqlstate(&error)),
    }
}

async fn scalar_bind(conn: &mut PgConnection, sql: &str, value: &str) -> String {
    match sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(sql.to_string()))
        .bind(value)
        .fetch_one(&mut *conn)
        .await
    {
        Ok(text) => text,
        Err(error) => panic!("sql {}", sqlstate(&error)),
    }
}

async fn count_shadow(conn: &mut PgConnection, table: &str) -> String {
    let sql = format!("SELECT count(*)::text FROM _oo.{}", quote_physical(table));
    scalar(conn, &sql).await
}

async fn regclass(conn: &mut PgConnection, table: &str) -> Option<String> {
    match sqlx::query_scalar("SELECT to_regclass(format('_oo.%I', $1::text))::text")
        .bind(table)
        .fetch_one(&mut *conn)
        .await
    {
        Ok(value) => value,
        Err(error) => panic!("sql {}", sqlstate(&error)),
    }
}

async fn shadow_rows(conn: &mut PgConnection, table: &str, columns: &[String]) -> Vec<Vec<Option<String>>> {
    let list = columns
        .iter()
        .map(|name| format!("{}::text", quote_physical(name)))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("SELECT {list} FROM _oo.{} ORDER BY \"_oo_pos\"", quote_physical(table));
    let rows = match sqlx::query(sqlx::AssertSqlSafe(sql)).fetch_all(&mut *conn).await {
        Ok(rows) => rows,
        Err(error) => panic!("sql {}", sqlstate(&error)),
    };
    rows.into_iter()
        .map(|row| {
            (0..columns.len())
                .map(|index| match row.try_get(index) {
                    Ok(value) => value,
                    Err(_) => panic!("cell"),
                })
                .collect()
        })
        .collect()
}

async fn denied(world: &World, user: &str, password: &str, table: &str) {
    let mut conn = dial(&world.host, world.port, user, password, DATABASE, &world.ca).await;
    let sql = format!("SELECT 1 FROM _oo.{} LIMIT 1", quote_physical(table));
    match sqlx::query(sqlx::AssertSqlSafe(sql)).fetch_optional(&mut conn).await {
        Ok(_) => panic!("visible"),
        Err(error) => assert_eq!(sqlstate(&error), "42501"),
    }
}

async fn ensure_role(conn: &mut PgConnection, name: &str, password: &str) {
    if name != RUNTIME_USER && name != AGENT_USER {
        panic!("role");
    }
    if password.is_empty() || !password.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        panic!("pass");
    }
    let exists: bool = match sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = $1)")
        .bind(name)
        .fetch_one(&mut *conn)
        .await
    {
        Ok(value) => value,
        Err(error) => panic!("role {}", sqlstate(&error)),
    };
    if exists {
        return;
    }
    let sql = format!(
        "CREATE ROLE {name} LOGIN PASSWORD '{password}' NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT NOBYPASSRLS NOREPLICATION"
    );
    if sqlx::raw_sql(sqlx::AssertSqlSafe(sql)).execute(&mut *conn).await.is_err() {
        panic!("role");
    }
}

async fn ensure_database(conn: &mut PgConnection) {
    let exists: bool = match sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_database WHERE datname = $1)")
        .bind(DATABASE)
        .fetch_one(&mut *conn)
        .await
    {
        Ok(value) => value,
        Err(error) => panic!("database {}", sqlstate(&error)),
    };
    if exists {
        return;
    }
    if let Err(error) = sqlx::raw_sql(sqlx::AssertSqlSafe(
        "CREATE DATABASE oo_a308_file TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'",
    ))
    .execute(&mut *conn)
    .await
    {
        panic!("database {}", sqlstate(&error));
    }
}

fn assert_worker_gone() {
    let entries = std::fs::read_dir("/proc").unwrap_or_else(|_| panic!("proc"));
    for entry in entries.flatten() {
        let name = entry.file_name();
        let text = name.to_string_lossy();
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let cmdline = std::fs::read(entry.path().join("cmdline")).unwrap_or_default();
        let comm = std::fs::read(entry.path().join("comm")).unwrap_or_default();
        let command = String::from_utf8_lossy(&cmdline);
        let comm_text = String::from_utf8_lossy(&comm);
        if command.contains("oo-file-parser-worker") || comm_text.starts_with("oo-file-parser") {
            panic!("worker");
        }
    }
}
