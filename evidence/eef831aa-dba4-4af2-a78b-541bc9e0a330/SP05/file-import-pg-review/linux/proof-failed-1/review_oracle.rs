//! Independent PostgreSQL oracle for the file-import-pg review.
//! Armed only by `OO_A330_REVIEW=1`. It does not replace the author tests.

#![forbid(unsafe_code)]

use std::io::Write;
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

const DATABASE: &str = "oo_a330_file";
const RUNTIME_USER: &str = "oo_a330_runtime";
const AGENT_USER: &str = "oo_a330_agent";

struct Ctx {
    host: String,
    port: u16,
    runtime_password: String,
    agent_password: String,
    ca: Vec<u8>,
    ca_path: String,
    worker: String,
    worker_sha: String,
    dir: PathBuf,
    oracle: String,
}

struct Spec {
    file: &'static str,
    format: &'static str,
    profile: &'static str,
    source: &'static str,
    table: &'static str,
    shard: &'static str,
    columns: Value,
    nulls: Value,
}

#[tokio::test]
#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
async fn independent_typed_copy() {
    if std::env::var("OO_A330_REVIEW").ok().as_deref() != Some("1") {
        return;
    }
    let ctx = Ctx::load();
    note(&ctx, "oracle_start=1");
    let mut bootstrap = dial(&ctx, "oo_fixture_admin", &secret("OO_A330_ADMIN_PASS_FILE"), "postgres").await;
    ensure_role(&mut bootstrap, RUNTIME_USER, &ctx.runtime_password).await;
    ensure_role(&mut bootstrap, AGENT_USER, &ctx.agent_password).await;
    ensure_database(&mut bootstrap).await;
    drop(bootstrap);
    let mut admin = dial(&ctx, "oo_fixture_admin", &secret("OO_A330_ADMIN_PASS_FILE"), DATABASE).await;
    install(&mut admin).await.unwrap_or_else(|_| panic!("install"));
    install_full_import(&mut admin).await.unwrap_or_else(|_| panic!("install_full_import"));
    exec(&mut admin, "REVOKE ALL ON DATABASE oo_a330_file FROM PUBLIC").await;
    exec(&mut admin, "GRANT CONNECT ON DATABASE oo_a330_file TO oo_a330_runtime").await;
    exec(&mut admin, "GRANT CONNECT ON DATABASE oo_a330_file TO oo_a330_agent").await;
    exec(&mut admin, "GRANT oo_pipeline_runtime TO oo_a330_runtime").await;
    let roles_before = scalar(&mut admin, "SELECT count(*)::text FROM pg_catalog.pg_roles").await;
    note(&ctx, &format!("roles_before={roles_before}"));

    let csv_columns = json!([
        col("id", "utf8", "text", false, true, None, None),
        col("n", "int64", "int8", true, false, None, None),
        col("amount", "decimal", "numeric", true, false, Some(10), Some(2)),
        col("flag", "bool", "bool", false, false, None, None),
        col("note", "utf8", "text", true, false, None, None),
        col("payload", "utf8", "json", true, false, None, None)
    ]);
    let csv_body = "\
id,n,amount,flag,note,payload
lo,-9223372036854775808,-1.25,true,\"\",\"{\"\"k\"\":2}\"
hi,9223372036854775807,0.00,false,kept,null
mid,,1.50,true,,
";
    let csv = Spec {
        file: "rev.csv",
        format: "csv",
        profile: "revcsv",
        source: "src_rev_csv",
        table: "rev_csv",
        shard: "revcsv",
        columns: csv_columns,
        nulls: json!({"policy": "empty"}),
    };
    let inserted = ctx.import(&csv, csv_body.as_bytes()).await.expect("csv");
    assert_caller(&inserted, false, 3);
    let run_id = inserted.run_id.clone().expect("run");
    let stored = scalar_bind(
        &mut admin,
        "SELECT state FROM _oo.full_import_run WHERE run_id = $1::uuid",
        &run_id,
    )
    .await;
    note(&ctx, &format!("caller_state={}", inserted.state));
    note(&ctx, &format!("stored_run_state={stored}"));
    note(&ctx, &format!("result_stored={}", inserted.stored_run_state));
    assert_eq!(inserted.state, "awaiting_review");
    assert_eq!(stored, "awaiting_publish");
    assert_eq!(inserted.stored_run_state, "awaiting_publish");
    assert!(!inserted.published);
    assert!(!inserted.role_grant);
    let roles_after = scalar(&mut admin, "SELECT count(*)::text FROM pg_catalog.pg_roles").await;
    note(&ctx, &format!("roles_after_csv={roles_after}"));
    assert_eq!(roles_after, roles_before);
    let persistence = scalar_bind(
        &mut admin,
        "SELECT relpersistence::text FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = '_oo' AND c.relname = $1",
        &inserted.physical_table,
    )
    .await;
    note(&ctx, &format!("relpersistence={persistence}"));
    assert_eq!(persistence, "p");
    let public_grants = scalar_bind(
        &mut admin,
        "SELECT count(*)::text FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace CROSS JOIN LATERAL aclexplode(c.relacl) AS grant_row WHERE n.nspname = '_oo' AND c.relname = $1 AND grant_row.grantee = 0",
        &inserted.physical_table,
    )
    .await;
    note(&ctx, &format!("public_grantee0={public_grants}"));
    assert_eq!(public_grants, "0");
    let generation = scalar_bind(
        &mut admin,
        "SELECT generation::text FROM _oo.target_generations WHERE shard_id = $1 ORDER BY generation DESC LIMIT 1",
        csv.shard,
    )
    .await;
    note(&ctx, &format!("fence_generation={generation}"));
    assert_eq!(generation, inserted.target_generation.expect("generation").to_string());
    let cells = shadow_rows(&mut admin, &inserted.physical_table, &inserted.physical_columns).await;
    note(&ctx, &format!("csv_rows={}", show(&cells)));
    assert_eq!(
        cells,
        vec![
            vec![
                Some("lo".to_string()),
                Some("-9223372036854775808".to_string()),
                Some("-1.25".to_string()),
                Some("true".to_string()),
                Some(String::new()),
                Some("{\"k\":2}".to_string()),
            ],
            vec![
                Some("hi".to_string()),
                Some("9223372036854775807".to_string()),
                Some("0.00".to_string()),
                Some("false".to_string()),
                Some("kept".to_string()),
                Some("null".to_string()),
            ],
            vec![
                Some("mid".to_string()),
                None,
                Some("1.50".to_string()),
                Some("true".to_string()),
                None,
                None,
            ],
        ]
    );
    let runtime_state = denied(&ctx, RUNTIME_USER, &ctx.runtime_password, &inserted.physical_table).await;
    let agent_state = denied(&ctx, AGENT_USER, &ctx.agent_password, &inserted.physical_table).await;
    note(&ctx, &format!("runtime_select={runtime_state}"));
    note(&ctx, &format!("agent_select={agent_state}"));
    assert_eq!(runtime_state, "42501");
    assert_eq!(agent_state, "42501");
    let receipt = scalar_bind(
        &mut admin,
        "SELECT string_agg(DISTINCT position_codec || '|' || position_lineage, ',') FROM _oo.apply_receipts WHERE source = $1",
        csv.source,
    )
    .await;
    let batches = scalar_bind(
        &mut admin,
        "SELECT string_agg(txn_or_batch_id, ',' ORDER BY txn_or_batch_id) FROM _oo.apply_receipts WHERE source = $1",
        csv.source,
    )
    .await;
    note(&ctx, &format!("receipt={receipt}"));
    note(&ctx, &format!("batches={batches}"));
    assert_eq!(receipt, "snapshot-zero/v1|full-import.snapshot");
    assert_eq!(batches, "b0");
    observe_deferred(&ctx, &mut admin, &inserted.physical_table).await;
    assert_worker_gone(&ctx, "csv");

    let replayed = ctx.import(&csv, csv_body.as_bytes()).await.expect("replay");
    assert_caller(&replayed, true, 3);
    assert!(replayed.run_id.is_none());
    assert!(replayed.target_generation.is_none());
    assert_eq!(replayed.physical_table, inserted.physical_table);
    let runs = scalar_bind(
        &mut admin,
        "SELECT count(*)::text FROM _oo.full_import_run WHERE source_id = $1",
        csv.source,
    )
    .await;
    let replay_rows = count_shadow(&mut admin, &inserted.physical_table).await;
    note(&ctx, &format!("replay_runs={runs}"));
    note(&ctx, &format!("replay_rows={replay_rows}"));
    assert_eq!(runs, "1");
    assert_eq!(replay_rows, "3");
    assert_worker_gone(&ctx, "replay");

    let mut changed = csv_body.to_string();
    changed = changed.replace("kept", "moved");
    let conflict = ctx.import(&csv, changed.as_bytes()).await.expect_err("conflict");
    note(&ctx, &format!("hash_conflict={} {} {}", conflict.code, conflict.detail, conflict.child_started));
    assert_eq!((conflict.code, conflict.detail, conflict.child_started), ("hash_conflict", "receipt", true));
    assert_eq!(exit_code(&conflict), 3);
    assert!(error_document(&conflict).contains("\"published\":false"));
    assert_eq!(count_shadow(&mut admin, &inserted.physical_table).await, "3");
    assert_eq!(
        scalar_bind(&mut admin, "SELECT count(*)::text FROM _oo.full_import_run WHERE source_id = $1", csv.source).await,
        "1"
    );
    assert_worker_gone(&ctx, "hash_conflict");

    let tsv = Spec {
        file: "rev.tsv",
        format: "tsv",
        profile: "revtsv",
        source: "src_rev_tsv",
        table: "rev_tsv",
        shard: "revtsv",
        columns: json!([
            col("id", "utf8", "text", false, true, None, None),
            col("note", "utf8", "text", true, false, None, None)
        ]),
        nulls: json!({"policy": "empty"}),
    };
    let tsv_row = ctx.import(&tsv, b"id\tnote\nq\t\"left\tright\"\n").await.expect("tsv");
    assert_caller(&tsv_row, false, 1);
    let tsv_cells = shadow_rows(&mut admin, &tsv_row.physical_table, &tsv_row.physical_columns).await;
    note(&ctx, &format!("tsv_rows={}", show(&tsv_cells)));
    assert_eq!(tsv_cells, vec![vec![Some("q".to_string()), Some("left\tright".to_string())]]);
    assert_worker_gone(&ctx, "tsv");

    let token = Spec {
        file: "token.csv",
        format: "csv",
        profile: "revtok",
        source: "src_rev_tok",
        table: "rev_tok",
        shard: "revtok",
        columns: json!([
            col("id", "utf8", "text", false, true, None, None),
            col("note", "utf8", "text", true, false, None, None)
        ]),
        nulls: json!({"policy": "token", "token": "\\N"}),
    };
    let token_row = ctx
        .import(&token, b"id,note\n1,\\N\n2,\"\\N\"\n3,\n4,\"\"\n")
        .await
        .expect("token");
    assert_caller(&token_row, false, 4);
    let token_cells = shadow_rows(&mut admin, &token_row.physical_table, &token_row.physical_columns).await;
    note(&ctx, &format!("token_rows={}", show(&token_cells)));
    assert_eq!(
        token_cells,
        vec![
            vec![Some("1".to_string()), None],
            vec![Some("2".to_string()), Some("\\N".to_string())],
            vec![Some("3".to_string()), Some(String::new())],
            vec![Some("4".to_string()), Some(String::new())],
        ]
    );
    assert_worker_gone(&ctx, "token");

    let jsonl = Spec {
        file: "rev.jsonl",
        format: "jsonl",
        profile: "revjson",
        source: "src_rev_json",
        table: "rev_json",
        shard: "revjson",
        columns: json!([
            col("id", "utf8", "text", false, true, None, None),
            col("payload", "json_text", "json", true, false, None, None)
        ]),
        nulls: json!({"policy": "json"}),
    };
    let json_row = ctx
        .import(&jsonl, b"{\"id\":\"n1\",\"payload\":{\"k\":2}}\n{\"id\":\"n2\",\"payload\":null}\n")
        .await
        .expect("jsonl");
    assert_caller(&json_row, false, 2);
    let json_cells = shadow_rows(&mut admin, &json_row.physical_table, &json_row.physical_columns).await;
    note(&ctx, &format!("jsonl_rows={}", show(&json_cells)));
    assert_eq!(
        json_cells,
        vec![
            vec![Some("n1".to_string()), Some("{\"k\":2}".to_string())],
            vec![Some("n2".to_string()), None],
        ]
    );
    assert_worker_gone(&ctx, "jsonl");

    let missing = Spec {
        file: "miss.jsonl",
        format: "jsonl",
        profile: "revmiss",
        source: "src_rev_miss",
        table: "rev_miss",
        shard: "revmiss",
        columns: json!([
            col("id", "utf8", "text", false, true, None, None),
            col("payload", "json_text", "json", true, false, None, None)
        ]),
        nulls: json!({"policy": "json"}),
    };
    expect_absent(
        &ctx,
        &mut admin,
        &missing,
        b"{\"id\":\"m1\"}\n",
        None,
        "missing_field",
        "missing_field",
        true,
        2,
    )
    .await;
    expect_absent(
        &ctx,
        &mut admin,
        &Spec {
            file: "drift.csv",
            format: "csv",
            profile: "revdrift",
            source: "src_rev_drift",
            table: "rev_drift",
            shard: "revdrift",
            columns: json!([col("id", "utf8", "text", false, true, None, None)]),
            nulls: json!({"policy": "empty"}),
        },
        b"id\n1\n",
        Some(b"id\n9\n"),
        "hash_drift",
        "object",
        false,
        2,
    )
    .await;
    expect_absent(
        &ctx,
        &mut admin,
        &Spec {
            file: "cut.jsonl",
            format: "jsonl",
            profile: "revcut",
            source: "src_rev_cut",
            table: "rev_cut",
            shard: "revcut",
            columns: json!([col("id", "utf8", "text", false, true, None, None)]),
            nulls: json!({"policy": "json"}),
        },
        b"{\"id\":\"cut\"",
        None,
        "truncated",
        "truncated",
        true,
        4,
    )
    .await;

    let done = Spec {
        file: "done.jsonl",
        format: "jsonl",
        profile: "revdone",
        source: "src_rev_done",
        table: "rev_done",
        shard: "revdone",
        columns: json!([col("id", "utf8", "text", false, true, None, None)]),
        nulls: json!({"policy": "json"}),
    };
    let done_row = ctx.import(&done, b"{\"id\":\"done\"}").await.expect("complete_no_newline");
    assert_caller(&done_row, false, 1);
    let done_cells = shadow_rows(&mut admin, &done_row.physical_table, &done_row.physical_columns).await;
    note(&ctx, &format!("complete_no_newline={}", show(&done_cells)));
    assert_eq!(done_cells, vec![vec![Some("done".to_string())]]);
    let done_run = done_row.run_id.clone().expect("done run");
    assert_eq!(
        scalar_bind(&mut admin, "SELECT state FROM _oo.full_import_run WHERE run_id = $1::uuid", &done_run).await,
        "awaiting_publish"
    );
    assert_worker_gone(&ctx, "complete_no_newline");

    let pair = json!([
        col("id", "utf8", "text", false, true, None, None),
        col("note", "utf8", "text", true, false, None, None)
    ]);
    expect_absent(
        &ctx,
        &mut admin,
        &Spec {
            file: "bad.csv",
            format: "csv",
            profile: "revbad",
            source: "src_rev_bad",
            table: "rev_bad",
            shard: "revbad",
            columns: pair.clone(),
            nulls: json!({"policy": "empty"}),
        },
        b"id,note\na,b,c\n",
        None,
        "malformed",
        "protocol",
        true,
        4,
    )
    .await;
    expect_absent(
        &ctx,
        &mut admin,
        &Spec {
            file: "over.csv",
            format: "csv",
            profile: "revover",
            source: "src_rev_over",
            table: "rev_over",
            shard: "revover",
            columns: json!([
                col("id", "utf8", "text", false, true, None, None),
                col("n", "int64", "int8", true, false, None, None)
            ]),
            nulls: json!({"policy": "empty"}),
        },
        b"id,n\na,9223372036854775808\n",
        None,
        "malformed",
        "protocol",
        true,
        4,
    )
    .await;

    let kill = Spec {
        file: "kill.csv",
        format: "csv",
        profile: "revkill",
        source: "src_rev_kill",
        table: "rev_kill",
        shard: "revkill",
        columns: pair,
        nulls: json!({"policy": "empty"}),
    };
    let kill_path = ctx.write(kill.file, b"id,note\nk,kept\n");
    let kill_request = ctx.request(&kill, &kill_path);
    let names = physical_plan(&kill_request).expect("kill names");
    let runs_before = scalar(&mut admin, "SELECT count(*)::text FROM _oo.full_import_run").await;
    let staged = match stage_replace(&kill_request, &ctx.runtime_password).await {
        Ok(Stage::Ready(staged)) => staged,
        Ok(Stage::Replay(_)) => panic!("kill replay"),
        Err(error) => panic!("kill stage {} {}", error.code, error.detail),
    };
    let pid = staged.backend_pid();
    note(&ctx, &format!("terminate_pid={pid}"));
    let terminated: bool = sqlx::query_scalar("SELECT pg_catalog.pg_terminate_backend($1)")
        .bind(pid)
        .fetch_one(&mut admin)
        .await
        .unwrap_or_else(|_| panic!("terminate"));
    assert!(terminated);
    let mut gone = false;
    for _ in 0..40 {
        let alive: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_stat_activity WHERE pid = $1)")
            .bind(pid)
            .fetch_one(&mut admin)
            .await
            .unwrap_or_else(|_| panic!("pid"));
        if !alive {
            gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(gone, "backend");
    drop(staged);
    let shadow = regclass(&mut admin, &names.table).await;
    let runs_after = scalar(&mut admin, "SELECT count(*)::text FROM _oo.full_import_run").await;
    note(&ctx, &format!("terminated_shadow={}", shadow.unwrap_or_else(|| "absent".to_string())));
    note(&ctx, &format!("runs_before_terminate={runs_before}"));
    note(&ctx, &format!("runs_after_terminate={runs_after}"));
    assert!(shadow.is_none());
    assert_eq!(runs_after, runs_before);
    let retried = import_replace(&kill_request, &ctx.runtime_password).await.expect("retry");
    assert_caller(&retried, false, 1);
    let retry_cells = shadow_rows(&mut admin, &retried.physical_table, &retried.physical_columns).await;
    note(&ctx, &format!("retry_rows={}", show(&retry_cells)));
    assert_eq!(retry_cells, vec![vec![Some("k".to_string()), Some("kept".to_string())]]);
    assert_eq!(count_shadow(&mut admin, &retried.physical_table).await, "1");
    let retry_run = retried.run_id.clone().expect("retry run");
    let stored_retry = scalar_bind(
        &mut admin,
        "SELECT state FROM _oo.full_import_run WHERE run_id = $1::uuid",
        &retry_run,
    )
    .await;
    note(&ctx, &format!("retry_stored={stored_retry}"));
    assert_eq!(stored_retry, "awaiting_publish");
    assert_eq!(retried.state, "awaiting_review");
    assert_worker_gone(&ctx, "retry");
    note(&ctx, "oracle_end=1");
}

fn assert_caller(result: &ImportResult, replay: bool, rows: u64) {
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
}

async fn observe_deferred(ctx: &Ctx, admin: &mut PgConnection, table: &str) {
    let trigger = scalar_bind(
        admin,
        "SELECT pg_get_triggerdef(t.oid) FROM pg_catalog.pg_trigger t JOIN pg_catalog.pg_class c ON c.oid = t.tgrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = '_oo' AND c.relname = $1 AND t.tgname = 'full_import_shadow_commit'",
        table,
    )
    .await;
    let body = scalar(
        admin,
        "SELECT prosrc FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace WHERE n.nspname = '_oo' AND p.proname = 'full_import_guard_shadow'",
    )
    .await;
    let deferred = trigger.contains("DEFERRABLE INITIALLY DEFERRED");
    let requires_open = body.contains("state = 'open'");
    note(ctx, &format!("trigger_deferred={deferred}"));
    note(ctx, &format!("guard_requires_open={requires_open}"));
    note(ctx, &format!("triggerdef={trigger}"));
    assert!(deferred);
    assert!(requires_open);
}

#[allow(clippy::too_many_arguments)]
async fn expect_absent(
    ctx: &Ctx,
    admin: &mut PgConnection,
    spec: &Spec,
    bytes: &[u8],
    rewrite: Option<&[u8]>,
    code: &str,
    detail: &str,
    started: bool,
    status: i32,
) {
    let path = ctx.write(spec.file, bytes);
    let request = ctx.request(spec, &path);
    if let Some(next) = rewrite {
        std::fs::write(&path, next).expect("rewrite");
    }
    let names = physical_plan(&request).expect("names");
    let runs = scalar(admin, "SELECT count(*)::text FROM _oo.full_import_run").await;
    let error = import_replace(&request, &ctx.runtime_password).await.expect_err(spec.file);
    note(
        ctx,
        &format!(
            "refuse {}={} {} started={} exit={}",
            spec.file,
            error.code,
            error.detail,
            error.child_started,
            exit_code(&error)
        ),
    );
    assert_eq!((error.code, error.detail, error.child_started), (code, detail, started));
    assert_eq!(exit_code(&error), status);
    assert!(error_document(&error).contains("\"published\":false"));
    assert!(regclass(admin, &names.table).await.is_none());
    assert_eq!(scalar(admin, "SELECT count(*)::text FROM _oo.full_import_run").await, runs);
    assert_worker_gone(ctx, spec.file);
}

impl Ctx {
    fn load() -> Self {
        let host = env("OO_A330_PG_HOST");
        let port = env("OO_A330_PG_PORT").parse().unwrap_or_else(|_| panic!("port"));
        let dir = PathBuf::from("/work/tmp/a330-review");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let worker = env("OO_A330_WORKER");
        Self {
            host,
            port,
            runtime_password: secret("OO_A330_RUNTIME_PASS_FILE"),
            agent_password: secret("OO_A330_AGENT_PASS_FILE"),
            ca: std::fs::read(env("OO_A330_CA_FILE")).expect("ca"),
            ca_path: env("OO_A330_CA_FILE"),
            worker_sha: sha_file(Path::new(&worker)),
            worker,
            dir,
            oracle: env("OO_A330_ORACLE"),
        }
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.dir.join(name);
        std::fs::write(&path, bytes).expect("fixture");
        path
    }

    fn request(&self, spec: &Spec, path: &Path) -> ImportRequest {
        let mut profile = json!({
            "format": spec.format,
            "id": spec.profile,
            "null": spec.nulls,
            "columns": spec.columns,
        });
        if spec.format != "jsonl" {
            profile["bom"] = json!("refuse");
            profile["header"] = json!("exact");
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
                "ca_path": self.ca_path
            },
            "fence": {"shard": spec.shard},
            "mapping": {"namespace": "revimp", "table": spec.table},
            "source": spec.source
        })
        .to_string();
        parse_request(&text).expect("request")
    }

    async fn import(&self, spec: &Spec, bytes: &[u8]) -> Result<ImportResult, ImportError> {
        let path = self.write(spec.file, bytes);
        let request = self.request(spec, &path);
        import_replace(&request, &self.runtime_password).await
    }
}

fn note(ctx: &Ctx, line: &str) {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&ctx.oracle)
        .expect("oracle");
    writeln!(file, "{line}").expect("oracle");
    file.flush().expect("oracle");
}

fn col(
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

fn show(rows: &[Vec<Option<String>>]) -> String {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|cell| match cell {
                    None => "NULL".to_string(),
                    Some(text) => text.replace('\\', "\\\\").replace('\t', "\\t"),
                })
                .collect::<Vec<_>>()
                .join("|")
        })
        .collect::<Vec<_>>()
        .join(" ;; ")
}

fn env(name: &str) -> String {
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => value,
        _ => panic!("env {name}"),
    }
}

fn secret(name: &str) -> String {
    let bytes = std::fs::read(env(name)).expect("pass");
    let mut text = String::from_utf8(bytes).expect("pass");
    if let Some(stripped) = text.strip_suffix('\n') {
        text = stripped.to_string();
    }
    if let Some(stripped) = text.strip_suffix('\r') {
        text = stripped.to_string();
    }
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        panic!("pass");
    }
    text
}

fn sha_file(path: &Path) -> String {
    let mut file = std::fs::File::open(path).expect("hash");
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let read = std::io::Read::read(&mut file, &mut buf).expect("hash");
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    hex(hasher.finalize().as_ref())
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
    let text = path.to_str().expect("path");
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

async fn dial(ctx: &Ctx, user: &str, password: &str, database: &str) -> PgConnection {
    let options = PgConnectOptions::new()
        .host(&ctx.host)
        .port(ctx.port)
        .username(user)
        .password(password)
        .database(database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ctx.ca.clone())
        .disable_statement_logging();
    PgConnection::connect_with(&options).await.unwrap_or_else(|_| panic!("connect"))
}

async fn exec(conn: &mut PgConnection, sql: &'static str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(&mut *conn)
        .await
        .unwrap_or_else(|_| panic!("sql"));
}

async fn scalar(conn: &mut PgConnection, sql: &str) -> String {
    sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&mut *conn)
        .await
        .unwrap_or_else(|error| panic!("sql {}", sqlstate(&error)))
}

async fn scalar_bind(conn: &mut PgConnection, sql: &str, value: &str) -> String {
    sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(sql.to_string()))
        .bind(value)
        .fetch_one(&mut *conn)
        .await
        .unwrap_or_else(|error| panic!("sql {}", sqlstate(&error)))
}

async fn count_shadow(conn: &mut PgConnection, table: &str) -> String {
    let sql = format!("SELECT count(*)::text FROM _oo.{}", quote_physical(table));
    scalar(conn, &sql).await
}

async fn regclass(conn: &mut PgConnection, table: &str) -> Option<String> {
    sqlx::query_scalar("SELECT to_regclass(format('_oo.%I', $1::text))::text")
        .bind(table)
        .fetch_one(conn)
        .await
        .unwrap_or_else(|error| panic!("sql {}", sqlstate(&error)))
}

async fn shadow_rows(conn: &mut PgConnection, table: &str, columns: &[String]) -> Vec<Vec<Option<String>>> {
    let list = columns
        .iter()
        .map(|name| format!("{}::text", quote_physical(name)))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("SELECT {list} FROM _oo.{} ORDER BY \"_oo_pos\"", quote_physical(table));
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut *conn)
        .await
        .unwrap_or_else(|error| panic!("sql {}", sqlstate(&error)));
    rows.into_iter()
        .map(|row| {
            (0..columns.len())
                .map(|index| row.try_get::<Option<String>, _>(index).unwrap_or_else(|_| panic!("cell")))
                .collect()
        })
        .collect()
}

async fn denied(ctx: &Ctx, user: &str, password: &str, table: &str) -> String {
    let mut conn = dial(ctx, user, password, DATABASE).await;
    let sql = format!("SELECT 1 FROM _oo.{} LIMIT 1", quote_physical(table));
    match sqlx::query(sqlx::AssertSqlSafe(sql)).fetch_optional(&mut conn).await {
        Ok(_) => "visible".to_string(),
        Err(error) => sqlstate(&error),
    }
}

async fn ensure_role(conn: &mut PgConnection, name: &str, password: &str) {
    if name != RUNTIME_USER && name != AGENT_USER {
        panic!("role");
    }
    if !password.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        panic!("pass");
    }
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = $1)")
        .bind(name)
        .fetch_one(&mut *conn)
        .await
        .unwrap_or_else(|error| panic!("role {}", sqlstate(&error)));
    if exists {
        return;
    }
    let sql = format!("CREATE ROLE {name} LOGIN PASSWORD '{password}' NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT NOBYPASSRLS NOREPLICATION");
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(&mut *conn)
        .await
        .unwrap_or_else(|_| panic!("role"));
}

async fn ensure_database(conn: &mut PgConnection) {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_database WHERE datname = $1)")
        .bind(DATABASE)
        .fetch_one(&mut *conn)
        .await
        .unwrap_or_else(|error| panic!("database {}", sqlstate(&error)));
    if exists {
        return;
    }
    sqlx::raw_sql(sqlx::AssertSqlSafe(
        "CREATE DATABASE oo_a330_file TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'",
    ))
    .execute(&mut *conn)
    .await
    .unwrap_or_else(|error| panic!("database {}", sqlstate(&error)));
}

fn assert_worker_gone(ctx: &Ctx, case_name: &str) {
    let entries = std::fs::read_dir("/proc").expect("proc");
    for entry in entries.flatten() {
        let text = entry.file_name().to_string_lossy().to_string();
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let cmdline = std::fs::read(entry.path().join("cmdline")).unwrap_or_default();
        let comm = std::fs::read(entry.path().join("comm")).unwrap_or_default();
        let command = String::from_utf8_lossy(&cmdline);
        let comm_text = String::from_utf8_lossy(&comm);
        if command.contains("oo-file-parser-worker") || comm_text.starts_with("oo-file-parser") {
            panic!("worker {case_name}");
        }
    }
    note(ctx, &format!("worker_scan_absent={case_name}"));
}
