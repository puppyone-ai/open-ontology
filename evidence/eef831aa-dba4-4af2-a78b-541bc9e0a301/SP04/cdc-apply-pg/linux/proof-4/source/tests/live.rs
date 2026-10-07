mod common;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use cdc_apply_pg::{
    binding_from, config_preimage, digest_text, plan_tables, schema_preimage, type_preimage,
    ColumnDecl, Endpoint, KeyDecl, PgType, Request, TablePlan, CONTRACT,
};
use oo_pipeline::physical_mapping::{
    plan_physical_mapping, quote_physical_name, Level, MappingRequest, REGISTER_SQL,
};
use oo_pipeline::target_installer::install;
use serde_json::Value;
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::PgConnection;
use sqlx::Connection;
use sqlx::Row;

const OWNER_A: &str = "11111111-1111-4111-8111-111111111111";
const OWNER_B: &str = "22222222-2222-4222-8222-222222222222";
const RUNTIME_USER: &str = "oo_a301_runtime";

struct Env {
    host: String,
    port: u16,
    admin_user: String,
    admin_pass: String,
    runtime_pass: String,
    runtime_pass_file: PathBuf,
    ca: Vec<u8>,
    ca_file: PathBuf,
}

struct Layout {
    orders_ns: String,
    orders_table: String,
    orders_id: String,
    orders_qty: String,
    orders_note: String,
    lines_ns: String,
    lines_table: String,
    lines_id: String,
    lines_amount: String,
}

#[tokio::test]
#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
async fn fenced_writer_on_private_postgres() {
    let env = load_env();
    let mut admin = dial(
        &env,
        &env.admin_user,
        &env.admin_pass,
        "postgres",
    )
    .await;
    raw(
        &mut admin,
        "DROP DATABASE IF EXISTS oo_a301 WITH (FORCE)",
        "drop db",
    )
    .await;
    raw(
        &mut admin,
        "DROP DATABASE IF EXISTS oo_a301_bare WITH (FORCE)",
        "drop bare",
    )
    .await;
    raw(
        &mut admin,
        "DROP DATABASE IF EXISTS oo_a301_drift WITH (FORCE)",
        "drop drift",
    )
    .await;
    raw(&mut admin, "DROP ROLE IF EXISTS oo_a301_runtime", "drop runtime").await;
    raw(
        &mut admin,
        "DROP ROLE IF EXISTS oo_pipeline_runtime",
        "drop pipeline",
    )
    .await;
    raw(&mut admin, "DROP ROLE IF EXISTS oo_target_owner", "drop owner").await;
    let create_role = format!(
        "CREATE ROLE {RUNTIME_USER} LOGIN PASSWORD '{pass}' NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT NOBYPASSRLS NOREPLICATION",
        pass = env.runtime_pass
    );
    raw(&mut admin, &create_role, "create runtime").await;
    raw(&mut admin, "CREATE DATABASE oo_a301", "create db").await;
    raw(&mut admin, "CREATE DATABASE oo_a301_bare", "create bare").await;
    drop(admin);

    let mut db = dial(&env, &env.admin_user, &env.admin_pass, "oo_a301").await;
    install(&mut db).await.expect("install");
    raw(
        &mut db,
        "GRANT oo_pipeline_runtime TO oo_a301_runtime",
        "member",
    )
    .await;
    raw(
        &mut db,
        "GRANT CONNECT ON DATABASE oo_a301 TO oo_a301_runtime",
        "connect",
    )
    .await;
    raw(
        &mut db,
        "GRANT CONNECT ON DATABASE oo_a301_bare TO oo_a301_runtime",
        "connect bare",
    )
    .await;

    let selected = vec!["public.lines".to_owned(), "public.orders".to_owned()];
    let shape = base_request(PathBuf::from("/unused"));
    let tables = plan_tables(&shape, &selected).expect("tables");
    let (config_hash, schema_hash, type_hash) = hashes(&shape, &tables);
    register_tables(&mut db, &tables, &config_hash).await;
    let layout = create_logged_tables(&mut db, &tables).await;
    grant_dml(&mut db, &layout).await;
    snapshot(&mut db, &layout).await;
    assert_eq!(persistence(&mut db, &layout.orders_ns, &layout.orders_table).await, "p");
    assert_eq!(persistence(&mut db, &layout.lines_ns, &layout.lines_table).await, "p");
    assert_eq!(generation_count(&mut db).await, 0);

    let bare = seal_and_request(
        &env,
        "bare1",
        &shape,
        &selected,
        &config_hash,
        &schema_hash,
        &type_hash,
        &[common::data_line(
            "bare1",
            1,
            1,
            "public.orders",
            "1",
            "upsert",
            r#"[{"type":"int","ordinal":0,"value":"1"},{"type":"text","ordinal":1,"value":"z"}]"#,
            &[],
        )],
        &[("public.orders", 1)],
        false,
        "oo_a301_bare",
    );
    let bare_out = run_apply(&bare, &[]);
    assert_eq!(bare_out.0, 2, "{0:?}", bare_out.1);
    assert_eq!(bare_out.1["fault"], "capability_absent");
    assert_eq!(generation_count(&mut db).await, 0);

    // Installed triggers reject UPDATE and DELETE on schema_migrations.
    // Copy the installed database, then insert the historical v2 row with a
    // checksum the writer does not accept. The original rows stay immutable.
    drop(db);
    let mut admin = dial(&env, &env.admin_user, &env.admin_pass, "postgres").await;
    raw(
        &mut admin,
        "DO $$ BEGIN PERFORM pg_catalog.pg_terminate_backend(pid) FROM pg_catalog.pg_stat_activity WHERE datname = 'oo_a301' AND pid <> pg_catalog.pg_backend_pid(); END $$",
        "quiet template",
    )
    .await;
    raw(
        &mut admin,
        "CREATE DATABASE oo_a301_drift TEMPLATE oo_a301",
        "clone drift",
    )
    .await;
    drop(admin);
    let mut db = dial(&env, &env.admin_user, &env.admin_pass, "oo_a301").await;
    let mut drift_db = dial(&env, &env.admin_user, &env.admin_pass, "oo_a301_drift").await;
    raw(
        &mut drift_db,
        "GRANT CONNECT ON DATABASE oo_a301_drift TO oo_a301_runtime",
        "connect drift",
    )
    .await;
    raw(
        &mut drift_db,
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES ('target-fence-receipts/2', 'abababababababababababababababababababababababababababababababab', 'expand', '0002_target_segment_null_expand.sql')",
        "drift insert",
    )
    .await;
    let drifted = seal_and_request(
        &env,
        "drift1",
        &shape,
        &selected,
        &config_hash,
        &schema_hash,
        &type_hash,
        &[common::data_line(
            "drift1",
            1,
            1,
            "public.orders",
            "1",
            "upsert",
            r#"[{"type":"int","ordinal":0,"value":"1"},{"type":"text","ordinal":1,"value":"z"}]"#,
            &[],
        )],
        &[("public.orders", 1)],
        false,
        "oo_a301_drift",
    );
    let drift_out = run_apply(&drifted, &[]);
    assert_eq!(drift_out.0, 2, "{0:?}", drift_out.1);
    assert_eq!(drift_out.1["fault"], "migration_drift");
    assert_eq!(generation_count(&mut drift_db).await, 0);
    assert_eq!(generation_count(&mut db).await, 0);

    let mut flagged = shape.clone();
    flagged.columns.push(ColumnDecl {
        table: "public.orders".to_owned(),
        ordinal: 2,
        name: "flag".to_owned(),
        pg_type: "bool".to_owned(),
        nullable: true,
    });
    let flagged_tables = plan_tables(&flagged, &selected).expect("flagged");
    let (flag_config, flag_schema, flag_type) = hashes(&flagged, &flagged_tables);
    assert_eq!(flag_config, config_hash);
    let missing = seal_and_request(
        &env,
        "miss1",
        &flagged,
        &selected,
        &flag_config,
        &flag_schema,
        &flag_type,
        &[common::data_line(
            "miss1",
            1,
            1,
            "public.orders",
            "1",
            "upsert",
            r#"[{"type":"int","ordinal":0,"value":"1"},{"type":"text","ordinal":1,"value":"z"},{"type":"bool","ordinal":2,"value":true}]"#,
            &[],
        )],
        &[("public.orders", 1)],
        false,
        "oo_a301",
    );
    let missing_out = run_apply(&missing, &[]);
    assert_eq!(missing_out.0, 2, "{0:?}", missing_out.1);
    assert_eq!(missing_out.1["fault"], "mapping_absent");
    assert_eq!(generation_count(&mut db).await, 0);

    let happy_rows = vec![
        common::data_line(
            "inv17",
            1,
            1,
            "public.orders",
            "1",
            "upsert",
            r#"[{"type":"int","ordinal":0,"value":"9223372036854775807"},{"type":"unavailable","ordinal":1}]"#,
            &[1],
        ),
        common::data_line("inv17", 2, 1, "public.lines", "7", "delete", "[]", &[]),
        common::data_line(
            "inv17",
            3,
            2,
            "public.lines",
            "9",
            "upsert",
            r#"[{"type":"int","ordinal":0,"value":"-9223372036854775808"}]"#,
            &[],
        ),
        common::data_line(
            "inv17",
            4,
            2,
            "public.orders",
            "2",
            "upsert",
            r#"[{"type":"null","ordinal":0},{"type":"text","ordinal":1,"value":"new"}]"#,
            &[],
        ),
    ];
    let happy = seal_and_request(
        &env,
        "inv17",
        &shape,
        &selected,
        &config_hash,
        &schema_hash,
        &type_hash,
        &happy_rows,
        &[("public.lines", 2), ("public.orders", 2)],
        false,
        "oo_a301",
    );
    let applied = run_apply(&happy, &[]);
    assert_eq!(applied.0, 0, "{0:?}", applied.1);
    assert_eq!(applied.1["kind"], "applied");
    assert_eq!(applied.1["outcome"], "inserted");
    assert_eq!(applied.1["target_applied"], true);
    assert_eq!(applied.1["db_cross_table_consistent"], true);
    assert_eq!(applied.1["source_cross_table_consistent"], false);
    assert_eq!(applied.1["upstream_commit_lsn_verified"], false);
    assert_eq!(applied.1["debezium_normalizer"], false);
    assert_eq!(applied.1["p2_verified"], false);
    assert_eq!(applied.1["p11_verified"], false);
    assert_eq!(applied.1["ack_allowed"], false);
    assert_eq!(applied.1["control_lease_consulted"], false);
    assert_eq!(applied.1["commit_position_provenance"], "caller_declared");
    assert_eq!(applied.1["applied_pos"], common::POSITION);
    assert_eq!(orders(&mut db, &layout).await, vec![
        ("1".to_owned(), Some("9223372036854775807".to_owned()), Some("keep".to_owned())),
        ("2".to_owned(), None, Some("new".to_owned())),
    ]);
    assert_eq!(
        lines(&mut db, &layout).await,
        vec![("9".to_owned(), Some("-9223372036854775808".to_owned()))]
    );
    let receipt = receipt_row(&mut db, "inv17").await.expect("receipt");
    assert_eq!(receipt.cross, "true");
    assert_eq!(receipt.partial, "false");
    assert_eq!(receipt.complete, "true");
    assert!(receipt.segment_null);
    assert_eq!(receipt.bitmap_len, 0);
    assert_eq!(receipt.applied_pos, common::POSITION);
    assert_eq!(receipt.fence_generation, applied.1["generation"].as_i64().unwrap().to_string());

    let replay = run_apply(&happy, &[]);
    assert_eq!(replay.0, 0, "{0:?}", replay.1);
    assert_eq!(replay.1["outcome"], "replay");
    assert_eq!(replay.1["target_applied"], true);
    assert_eq!(replay.1["row_writes"], 0);
    assert_eq!(replay.1["db_cross_table_consistent"], true);
    assert_eq!(replay.1["source_cross_table_consistent"], false);
    let again = receipt_row(&mut db, "inv17").await.expect("receipt");
    assert_eq!(again.fence_generation, receipt.fence_generation);
    assert_eq!(again.commit_at, receipt.commit_at);
    assert_eq!(again.payload, receipt.payload);
    assert_eq!(orders(&mut db, &layout).await.len(), 2);

    let mut conflict_rows = happy_rows.clone();
    conflict_rows[0] = common::data_line(
        "inv17",
        1,
        1,
        "public.orders",
        "1",
        "upsert",
        r#"[{"type":"int","ordinal":0,"value":"3"},{"type":"unavailable","ordinal":1}]"#,
        &[1],
    );
    let conflict = seal_and_request(
        &env,
        "inv17b",
        &shape,
        &selected,
        &config_hash,
        &schema_hash,
        &type_hash,
        &conflict_rows,
        &[("public.lines", 2), ("public.orders", 2)],
        false,
        "oo_a301",
    );
    let conflicted = run_apply(&conflict, &[]);
    assert_eq!(conflicted.0, 2, "{0:?}", conflicted.1);
    assert_eq!(conflicted.1["fault"], "payload_conflict");
    assert_eq!(conflicted.1["sqlstate"], "OO010");
    assert_eq!(conflicted.1["target_applied"], false);
    let after_conflict = receipt_row(&mut db, "inv17").await.expect("receipt");
    assert_eq!(after_conflict.payload, receipt.payload);
    assert_eq!(after_conflict.commit_at, receipt.commit_at);
    assert_eq!(orders(&mut db, &layout).await[0].1.as_deref(), Some("9223372036854775807"));

    let unavailable = seal_and_request(
        &env,
        "inv18",
        &shape,
        &selected,
        &config_hash,
        &schema_hash,
        &type_hash,
        &[common::data_line(
            "inv18",
            1,
            1,
            "public.orders",
            "3",
            "upsert",
            r#"[{"type":"unavailable","ordinal":0},{"type":"text","ordinal":1,"value":"x"}]"#,
            &[0],
        )],
        &[("public.orders", 1)],
        false,
        "oo_a301",
    );
    let refused = run_apply(&unavailable, &[]);
    assert_eq!(refused.0, 2, "{0:?}", refused.1);
    assert_eq!(refused.1["fault"], "unavailable_insert");
    assert!(receipt_row(&mut db, "inv18").await.is_none());
    assert!(orders(&mut db, &layout).await.iter().all(|row| row.0 != "3"));

    let fence_path = seal_and_request(
        &env,
        "fence1",
        &shape,
        &selected,
        &config_hash,
        &schema_hash,
        &type_hash,
        &[common::data_line(
            "fence1",
            1,
            1,
            "public.orders",
            "1",
            "upsert",
            r#"[{"type":"int","ordinal":0,"value":"4"},{"type":"unavailable","ordinal":1}]"#,
            &[1],
        )],
        &[("public.orders", 1)],
        true,
        "oo_a301",
    );
    let mut held = dial(&env, RUNTIME_USER, &env.runtime_pass, "oo_a301").await;
    let held_gen = adopt_generation(&mut held).await;
    let blocked = run_apply(&fence_path, &[]);
    assert_eq!(blocked.0, 2, "{0:?}", blocked.1);
    assert_eq!(blocked.1["fault"], "fence_held");
    assert_eq!(blocked.1["sqlstate"], "55P03");
    assert_eq!(blocked.1["control_lease_consulted"], false);
    assert_eq!(blocked.1["target_applied"], false);
    assert!(receipt_row(&mut db, "fence1").await.is_none());
    let stale = if held_gen == 1 { 2 } else { 1 };
    let stale_err = commit_stale(&mut held, stale, &config_hash, &schema_hash, &type_hash).await;
    assert!(stale_err.contains("OO013"), "{stale_err}");
    drop(held);
    assert_eq!(orders(&mut db, &layout).await[0].1.as_deref(), Some("9223372036854775807"));

    let failure = seal_and_request(
        &env,
        "inv19",
        &shape,
        &selected,
        &config_hash,
        &schema_hash,
        &type_hash,
        &[common::data_line(
            "inv19",
            1,
            1,
            "public.orders",
            "1",
            "upsert",
            r#"[{"type":"int","ordinal":0,"value":"1"},{"type":"unavailable","ordinal":1}]"#,
            &[1],
        )],
        &[("public.orders", 1)],
        false,
        "oo_a301",
    );
    let failed = run_apply(&failure, &[("CDC_APPLY_INJECT", "row_failure")]);
    assert_eq!(failed.0, 2, "{0:?}", failed.1);
    assert_eq!(failed.1["fault"], "row_failure");
    assert!(receipt_row(&mut db, "inv19").await.is_none());
    assert_eq!(orders(&mut db, &layout).await[0].1.as_deref(), Some("9223372036854775807"));

    let before = seal_and_request(
        &env,
        "inv20",
        &shape,
        &selected,
        &config_hash,
        &schema_hash,
        &type_hash,
        &[common::data_line(
            "inv20",
            1,
            1,
            "public.orders",
            "4",
            "upsert",
            r#"[{"type":"int","ordinal":0,"value":"4"},{"type":"text","ordinal":1,"value":"pre"}]"#,
            &[],
        )],
        &[("public.orders", 1)],
        false,
        "oo_a301",
    );
    kill_apply(&before, "before_commit");
    assert!(receipt_row(&mut db, "inv20").await.is_none());
    assert!(orders(&mut db, &layout).await.iter().all(|row| row.0 != "4"));
    let resumed = run_apply(&before, &[]);
    assert_eq!(resumed.0, 0, "{0:?}", resumed.1);
    assert_eq!(resumed.1["outcome"], "inserted");
    assert_eq!(
        orders(&mut db, &layout).await.iter().find(|row| row.0 == "4").map(|row| row.1.clone()),
        Some(Some("4".to_owned()))
    );

    let after = seal_and_request(
        &env,
        "inv21",
        &shape,
        &selected,
        &config_hash,
        &schema_hash,
        &type_hash,
        &[common::data_line(
            "inv21",
            1,
            1,
            "public.orders",
            "5",
            "upsert",
            r#"[{"type":"int","ordinal":0,"value":"5"},{"type":"text","ordinal":1,"value":"post"}]"#,
            &[],
        )],
        &[("public.orders", 1)],
        false,
        "oo_a301",
    );
    kill_apply(&after, "after_commit_before_response");
    let committed = receipt_row(&mut db, "inv21").await.expect("committed before response");
    let replayed = run_apply(&after, &[]);
    assert_eq!(replayed.0, 0, "{0:?}", replayed.1);
    assert_eq!(replayed.1["outcome"], "replay");
    let replayed_receipt = receipt_row(&mut db, "inv21").await.expect("replay receipt");
    assert_eq!(replayed_receipt.commit_at, committed.commit_at);
    assert_eq!(replayed_receipt.fence_generation, committed.fence_generation);
    assert_eq!(
        orders(&mut db, &layout).await.iter().filter(|row| row.0 == "5").count(),
        1
    );

    let hidden = seal_and_request(
        &env,
        "inv22",
        &shape,
        &selected,
        &config_hash,
        &schema_hash,
        &type_hash,
        &[common::data_line(
            "inv22",
            1,
            1,
            "public.orders",
            "6",
            "upsert",
            r#"[{"type":"int","ordinal":0,"value":"6"},{"type":"text","ordinal":1,"value":"hide"}]"#,
            &[],
        )],
        &[("public.orders", 1)],
        false,
        "oo_a301",
    );
    let hidden_out = run_apply(&hidden, &[("CDC_APPLY_INJECT", "hide_commit")]);
    assert_eq!(hidden_out.0, 0, "{0:?}", hidden_out.1);
    assert_eq!(hidden_out.1["target_applied"], true);
    assert!(hidden_out.1["outcome"] == "inserted" || hidden_out.1["outcome"] == "replay");
    assert_eq!(
        orders(&mut db, &layout).await.iter().filter(|row| row.0 == "6").count(),
        1
    );

    let unknown = seal_and_request(
        &env,
        "inv23",
        &shape,
        &selected,
        &config_hash,
        &schema_hash,
        &type_hash,
        &[common::data_line(
            "inv23",
            1,
            1,
            "public.orders",
            "8",
            "upsert",
            r#"[{"type":"int","ordinal":0,"value":"8"},{"type":"text","ordinal":1,"value":"unk"}]"#,
            &[],
        )],
        &[("public.orders", 1)],
        false,
        "oo_a301",
    );
    let unknown_out = run_apply(
        &unknown,
        &[
            ("CDC_APPLY_INJECT", "hide_commit"),
            ("CDC_APPLY_RECONCILE", "down"),
        ],
    );
    assert_eq!(unknown_out.0, 3, "{0:?}", unknown_out.1);
    assert_eq!(unknown_out.1["kind"], "commit_unknown");
    assert_eq!(unknown_out.1["target_applied"], false);
    assert_eq!(unknown_out.1["db_cross_table_consistent"], false);
    let settled = run_apply(&unknown, &[]);
    assert_eq!(settled.0, 0, "{0:?}", settled.1);
    assert_eq!(settled.1["target_applied"], true);
    assert_eq!(
        orders(&mut db, &layout).await.iter().filter(|row| row.0 == "8").count(),
        1
    );
    let final_receipt = receipt_row(&mut db, "inv23").await.expect("settled receipt");
    assert_eq!(final_receipt.cross, "true");
}

fn load_env() -> Env {
    let host = env::var("OO_A301_HOST").expect("OO_A301_HOST");
    let port = env::var("OO_A301_PORT").expect("OO_A301_PORT").parse().expect("port");
    let admin_user = env::var("OO_A301_ADMIN_USER").expect("OO_A301_ADMIN_USER");
    let admin_pass = password(&env::var("OO_A301_ADMIN_PASS_FILE").expect("admin pass file"));
    let runtime_pass_file = PathBuf::from(env::var("OO_A301_RUNTIME_PASS_FILE").expect("runtime pass file"));
    let runtime_pass = password(&runtime_pass_file.to_string_lossy());
    let ca_file = PathBuf::from(env::var("OO_A301_CA").expect("OO_A301_CA"));
    let ca = fs::read(&ca_file).expect("ca");
    checked_ident(&admin_user);
    Env {
        host,
        port,
        admin_user,
        admin_pass,
        runtime_pass,
        runtime_pass_file,
        ca,
        ca_file,
    }
}

fn password(path: &str) -> String {
    let text = fs::read_to_string(path).expect("password file").trim().to_owned();
    assert!(text.chars().all(|ch| ch.is_ascii_hexdigit()) && !text.is_empty());
    text
}

fn checked_ident(name: &str) {
    assert!(!name.is_empty() && name.len() <= 63);
    assert!(name.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'));
}

async fn dial(env: &Env, user: &str, pass: &str, database: &str) -> PgConnection {
    let options = PgConnectOptions::new()
        .host(&env.host)
        .port(env.port)
        .username(user)
        .password(pass)
        .database(database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(env.ca.clone());
    PgConnection::connect_with(&options).await.expect("dial")
}

async fn raw(conn: &mut PgConnection, sql: &str, label: &str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_owned()))
        .execute(&mut *conn)
        .await
        .unwrap_or_else(|err| panic!("{label} failed: {err}"));
}

async fn scalar(conn: &mut PgConnection, sql: &str) -> String {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_one(&mut *conn)
        .await
        .expect("scalar")
}

async fn generation_count(conn: &mut PgConnection) -> i64 {
    scalar(conn, "SELECT count(*)::text FROM _oo.target_generations")
        .await
        .parse()
        .expect("count")
}

fn base_request(spool: PathBuf) -> Request {
    Request {
        contract: CONTRACT.to_owned(),
        spool_dir: spool,
        endpoint: Endpoint {
            host: "pending".to_owned(),
            port: 1,
            database: "oo_a301".to_owned(),
            user: RUNTIME_USER.to_owned(),
            password_file: PathBuf::from("/secrets/runtime.pass"),
            ca_file: PathBuf::from("/secrets/ca.crt"),
            server_name: "pending".to_owned(),
        },
        shard: "billing".to_owned(),
        owner: OWNER_A.to_owned(),
        generation: 1,
        table_generation: "g1".to_owned(),
        run_generation: 1,
        lineage: common::LINEAGE.to_owned(),
        source: "billing".to_owned(),
        epoch: 1,
        control_lease_valid: false,
        segment: None,
        keys: vec![
            KeyDecl {
                table: "public.lines".to_owned(),
                ordinal: 0,
                name: "id".to_owned(),
                pg_type: "int8".to_owned(),
            },
            KeyDecl {
                table: "public.orders".to_owned(),
                ordinal: 0,
                name: "id".to_owned(),
                pg_type: "int8".to_owned(),
            },
        ],
        columns: vec![
            ColumnDecl {
                table: "public.lines".to_owned(),
                ordinal: 0,
                name: "amount".to_owned(),
                pg_type: "int8".to_owned(),
                nullable: true,
            },
            ColumnDecl {
                table: "public.orders".to_owned(),
                ordinal: 0,
                name: "qty".to_owned(),
                pg_type: "int8".to_owned(),
                nullable: true,
            },
            ColumnDecl {
                table: "public.orders".to_owned(),
                ordinal: 1,
                name: "note".to_owned(),
                pg_type: "text".to_owned(),
                nullable: true,
            },
        ],
    }
}

fn hashes(request: &Request, tables: &[TablePlan]) -> (String, String, String) {
    let binding = binding_from(request, tables);
    (
        digest_text(&config_preimage(&binding)),
        digest_text(&schema_preimage(tables)),
        digest_text(&type_preimage(tables)),
    )
}

async fn register_tables(conn: &mut PgConnection, tables: &[TablePlan], config_digest: &str) {
    for table in tables {
        register(conn, config_digest, Level::Namespace, &table.namespace, "", "").await;
        register(conn, config_digest, Level::Table, &table.namespace, &table.table, "").await;
        for field in table.keys.iter().chain(table.columns.iter()) {
            register(
                conn,
                config_digest,
                Level::Column,
                &table.namespace,
                &table.table,
                &field.name,
            )
            .await;
        }
    }
}

async fn register(
    conn: &mut PgConnection,
    config_digest: &str,
    level: Level,
    namespace: &str,
    table: &str,
    column: &str,
) {
    let plan = plan_physical_mapping(&MappingRequest {
        source: "billing".to_owned(),
        config_digest: config_digest.to_owned(),
        run_generation: 1,
        level,
        logical_namespace: namespace.to_owned(),
        logical_table: table.to_owned(),
        logical_column: column.to_owned(),
    })
    .expect("plan mapping");
    sqlx::query(REGISTER_SQL)
        .bind("billing")
        .bind(config_digest)
        .bind(1i64)
        .bind(level.as_str())
        .bind(namespace)
        .bind(table)
        .bind(column)
        .bind(plan.identity_digest.to_vec())
        .execute(&mut *conn)
        .await
        .expect("register");
}

async fn create_logged_tables(conn: &mut PgConnection, tables: &[TablePlan]) -> Layout {
    let mut schemas = std::collections::BTreeSet::new();
    let mut orders_ns = String::new();
    let mut orders_table = String::new();
    let mut orders_id = String::new();
    let mut orders_qty = String::new();
    let mut orders_note = String::new();
    let mut lines_ns = String::new();
    let mut lines_table = String::new();
    let mut lines_id = String::new();
    let mut lines_amount = String::new();
    for table in tables {
        let ns = quote_physical_name(
            &plan_physical_mapping(&mapping("billing", table, Level::Namespace, "", ""))
                .expect("ns")
                .physical_name,
        )
        .expect("quote ns");
        let relation = quote_physical_name(
            &plan_physical_mapping(&mapping("billing", table, Level::Table, &table.table, ""))
                .expect("table")
                .physical_name,
        )
        .expect("quote table");
        if schemas.insert(ns.clone()) {
            raw(conn, &format!("CREATE SCHEMA {ns}"), "schema").await;
        }
        let mut defs = Vec::new();
        let mut quoted_keys = Vec::new();
        for field in &table.keys {
            let name = quoted_column(table, &field.name);
            let ty = sql_type(field.pg_type);
            defs.push(format!("{name} {ty} NOT NULL"));
            quoted_keys.push(name);
        }
        for field in &table.columns {
            let name = quoted_column(table, &field.name);
            let nulls = if field.nullable { "NULL" } else { "NOT NULL" };
            let ty = sql_type(field.pg_type);
            defs.push(format!("{name} {ty} {nulls}"));
        }
        let keys = quoted_keys.join(", ");
        defs.push(format!("PRIMARY KEY ({keys})"));
        let body = defs.join(", ");
        raw(
            conn,
            &format!("CREATE TABLE {ns}.{relation} ({body})"),
            "table",
        )
        .await;
        if table.logical == "public.orders" {
            orders_ns = ns.clone();
            orders_table = relation.clone();
            orders_id = quoted_keys[0].clone();
            orders_qty = quoted_column(table, "qty");
            orders_note = quoted_column(table, "note");
        }
        if table.logical == "public.lines" {
            lines_ns = ns.clone();
            lines_table = relation.clone();
            lines_id = quoted_keys[0].clone();
            lines_amount = quoted_column(table, "amount");
        }
    }
    assert!(!orders_ns.is_empty() && !lines_ns.is_empty());
    Layout {
        orders_ns,
        orders_table,
        orders_id,
        orders_qty,
        orders_note,
        lines_ns,
        lines_table,
        lines_id,
        lines_amount,
    }
}

fn mapping(source: &str, table: &TablePlan, level: Level, logical_table: &str, column: &str) -> MappingRequest {
    let digest_request = Request {
        contract: CONTRACT.to_owned(),
        spool_dir: PathBuf::new(),
        endpoint: Endpoint {
            host: "oo-a301-pg".to_owned(),
            port: 5432,
            database: "oo_a301".to_owned(),
            user: RUNTIME_USER.to_owned(),
            password_file: PathBuf::from("/secrets/runtime.pass"),
            ca_file: PathBuf::from("/secrets/ca.crt"),
            server_name: "oo-a301-pg".to_owned(),
        },
        shard: "billing".to_owned(),
        owner: OWNER_A.to_owned(),
        generation: 1,
        table_generation: "g1".to_owned(),
        run_generation: 1,
        lineage: common::LINEAGE.to_owned(),
        source: source.to_owned(),
        epoch: 1,
        control_lease_valid: false,
        segment: None,
        keys: Vec::new(),
        columns: Vec::new(),
    };
    let tables = std::slice::from_ref(table);
    let binding = binding_from(&digest_request, tables);
    MappingRequest {
        source: source.to_owned(),
        config_digest: digest_text(&config_preimage(&binding)),
        run_generation: 1,
        level,
        logical_namespace: table.namespace.clone(),
        logical_table: if level == Level::Namespace {
            String::new()
        } else {
            logical_table.to_owned()
        },
        logical_column: column.to_owned(),
    }
}

fn quoted_column(table: &TablePlan, name: &str) -> String {
    let plan = plan_physical_mapping(&MappingRequest {
        source: "billing".to_owned(),
        config_digest: {
            let request = base_request(PathBuf::new());
            let selected = vec!["public.lines".to_owned(), "public.orders".to_owned()];
            let tables = plan_tables(&request, &selected).expect("replan");
            hashes(&request, &tables).0
        },
        run_generation: 1,
        level: Level::Column,
        logical_namespace: table.namespace.clone(),
        logical_table: table.table.clone(),
        logical_column: name.to_owned(),
    })
    .expect("column");
    quote_physical_name(&plan.physical_name).expect("quote")
}

fn sql_type(pg_type: PgType) -> &'static str {
    match pg_type {
        PgType::Bool => "bool",
        PgType::Int2 => "int2",
        PgType::Int4 => "int4",
        PgType::Int8 => "int8",
        PgType::Text => "text",
    }
}

async fn grant_dml(conn: &mut PgConnection, layout: &Layout) {
    checked_ident(RUNTIME_USER);
    for (ns, table) in [
        (&layout.orders_ns, &layout.orders_table),
        (&layout.lines_ns, &layout.lines_table),
    ] {
        raw(conn, &format!("GRANT USAGE ON SCHEMA {ns} TO {RUNTIME_USER}"), "usage").await;
        raw(
            conn,
            &format!("GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE {ns}.{table} TO {RUNTIME_USER}"),
            "dml",
        )
        .await;
    }
}

async fn snapshot(conn: &mut PgConnection, layout: &Layout) {
    let ns = &layout.orders_ns;
    let relation = &layout.orders_table;
    let id = &layout.orders_id;
    let qty = &layout.orders_qty;
    let note = &layout.orders_note;
    let orders = format!("INSERT INTO {ns}.{relation} ({id}, {qty}, {note}) VALUES ($1::int8, $2::int8, $3::text)");
    sqlx::query(sqlx::AssertSqlSafe(orders))
        .bind(1i64)
        .bind(10i64)
        .bind("keep")
        .execute(&mut *conn)
        .await
        .expect("seed orders");
    let ns = &layout.lines_ns;
    let relation = &layout.lines_table;
    let id = &layout.lines_id;
    let amount = &layout.lines_amount;
    let lines = format!("INSERT INTO {ns}.{relation} ({id}, {amount}) VALUES ($1::int8, $2::int8)");
    sqlx::query(sqlx::AssertSqlSafe(lines))
        .bind(7i64)
        .bind(5i64)
        .execute(&mut *conn)
        .await
        .expect("seed lines");
}

async fn persistence(conn: &mut PgConnection, namespace: &str, table: &str) -> String {
    let ns = namespace.trim_matches('"');
    let rel = table.trim_matches('"');
    let sql = "SELECT c.relpersistence::text FROM pg_catalog.pg_class AS c \
               INNER JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
               WHERE n.nspname = $1 AND c.relname = $2";
    sqlx::query_scalar(sql)
        .bind(ns)
        .bind(rel)
        .fetch_one(&mut *conn)
        .await
        .expect("persistence")
}

async fn orders(conn: &mut PgConnection, layout: &Layout) -> Vec<(String, Option<String>, Option<String>)> {
    let id = &layout.orders_id;
    let qty = &layout.orders_qty;
    let note = &layout.orders_note;
    let ns = &layout.orders_ns;
    let relation = &layout.orders_table;
    let sql = format!("SELECT {id}::text, {qty}::text, {note}::text FROM {ns}.{relation} ORDER BY {id}::int8");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut *conn)
        .await
        .expect("orders");
    rows.into_iter()
        .map(|row| {
            (
                row.try_get(0).expect("id"),
                row.try_get(1).expect("qty"),
                row.try_get(2).expect("note"),
            )
        })
        .collect()
}

async fn lines(conn: &mut PgConnection, layout: &Layout) -> Vec<(String, Option<String>)> {
    let id = &layout.lines_id;
    let amount = &layout.lines_amount;
    let ns = &layout.lines_ns;
    let relation = &layout.lines_table;
    let sql = format!("SELECT {id}::text, {amount}::text FROM {ns}.{relation} ORDER BY {id}::int8");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut *conn)
        .await
        .expect("lines");
    rows.into_iter()
        .map(|row| (row.try_get(0).expect("id"), row.try_get(1).expect("amount")))
        .collect()
}

struct ReceiptView {
    fence_generation: String,
    commit_at: String,
    cross: String,
    partial: String,
    complete: String,
    segment_null: bool,
    bitmap_len: i32,
    payload: String,
    applied_pos: String,
}

async fn receipt_row(conn: &mut PgConnection, txn: &str) -> Option<ReceiptView> {
    let sql = "SELECT fence_generation::text, target_commit_at::text, cross_table_consistent::text, \
               partial_txn::text, txn_complete::text, segment_index IS NULL, \
               octet_length(segment_bitmap)::int4, payload_digest, applied_pos::text \
               FROM _oo.apply_receipts \
               WHERE source = $1 AND table_generation = $2 AND txn_or_batch_id = $3";
    let row = sqlx::query(sql)
        .bind("billing")
        .bind("g1")
        .bind(txn)
        .fetch_optional(&mut *conn)
        .await
        .expect("receipt");
    row.map(|row| ReceiptView {
        fence_generation: row.try_get(0).expect("gen"),
        commit_at: row.try_get(1).expect("at"),
        cross: row.try_get(2).expect("cross"),
        partial: row.try_get(3).expect("partial"),
        complete: row.try_get(4).expect("complete"),
        segment_null: row.try_get(5).expect("segment"),
        bitmap_len: row.try_get(6).expect("bitmap"),
        payload: row.try_get(7).expect("payload"),
        applied_pos: row.try_get(8).expect("pos"),
    })
}

async fn adopt_generation(conn: &mut PgConnection) -> i64 {
    let row = sqlx::query("SELECT _oo.adopt_target_generation($1::text, $2::text::uuid)")
        .bind("billing")
        .bind(OWNER_B)
        .fetch_one(&mut *conn)
        .await
        .expect("adopt");
    row.try_get(0).expect("generation")
}

async fn commit_stale(
    conn: &mut PgConnection,
    generation: i64,
    config_hash: &str,
    schema_hash: &str,
    type_hash: &str,
) -> String {
    let err = sqlx::query(
        "SELECT _oo.commit_apply_receipt( \
            $1::text, $2::text::uuid, $3::int8, $4::text, $5::text, $6::text, \
            $7::text, $8::text, $9::text, $10::text, $11::text, $12::text, $13::text, \
            $14::int4, $15::text::numeric, $16::bool, $17::bool, $18::bool, \
            $19::int4, $20::int4, $21::bytea)",
    )
    .bind("billing")
    .bind(OWNER_B)
    .bind(generation)
    .bind("billing")
    .bind("g1")
    .bind("stale1")
    .bind(config_hash)
    .bind(type_hash)
    .bind(digest_text("cdc-apply-hooks/v1\n"))
    .bind(schema_hash)
    .bind(common::LINEAGE)
    .bind("pg-commit-lsn/v1")
    .bind(common::LINEAGE)
    .bind(1i32)
    .bind(common::POSITION)
    .bind(false)
    .bind(true)
    .bind(true)
    .bind(None::<i32>)
    .bind(None::<i32>)
    .bind(Vec::<u8>::new())
    .fetch_one(&mut *conn)
    .await
    .expect_err("stale generation");
    err.to_string()
}

fn seal_and_request(
    env: &Env,
    name: &str,
    shape: &Request,
    selected: &[String],
    config_hash: &str,
    schema_hash: &str,
    type_hash: &str,
    rows: &[String],
    counts: &[(&str, u64)],
    lease: bool,
    database: &str,
) -> PathBuf {
    let root = common::work_dir(name);
    let spool = root.join("spool");
    let tables: Vec<&str> = selected.iter().map(String::as_str).collect();
    let txn = rows
        .first()
        .and_then(|line| line.split("\"txn_id\":\"").nth(1))
        .and_then(|rest| rest.split('"').next())
        .expect("txn");
    common::seal(
        &spool,
        txn,
        &tables,
        rows,
        counts,
        schema_hash,
        config_hash,
        type_hash,
    );
    let mut request = shape.clone();
    request.spool_dir = spool;
    request.control_lease_valid = lease;
    write_request(&request, env, database, lease, &request.spool_dir)
}

fn write_request(shape: &Request, env: &Env, database: &str, lease: bool, spool: &Path) -> PathBuf {
    let mut request = shape.clone();
    request.spool_dir = spool.to_owned();
    request.control_lease_valid = lease;
    request.endpoint = Endpoint {
        host: env.host.clone(),
        port: env.port,
        database: database.to_owned(),
        user: RUNTIME_USER.to_owned(),
        password_file: env.runtime_pass_file.clone(),
        ca_file: env.ca_file.clone(),
        server_name: env.host.clone(),
    };
    let path = spool.parent().expect("parent").join("request.json");
    fs::write(&path, serde_json::to_vec(&request).expect("json")).expect("request");
    path
}

fn run_apply(request: &Path, extra: &[(&str, &str)]) -> (i32, Value) {
    let bin = env!("CARGO_BIN_EXE_cdc-apply-pg");
    let mut command = Command::new(bin);
    command.arg("apply").arg(request);
    for (key, value) in extra {
        command.env(key, value);
    }
    let output = command.output().expect("run");
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code().unwrap_or(127);
    let value: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|err| {
        panic!("json {err} exit={code} stdout={stdout} stderr={stderr}")
    });
    (code, value)
}

fn kill_apply(request: &Path, checkpoint: &str) {
    let bin = env!("CARGO_BIN_EXE_cdc-apply-pg");
    let mark = request.parent().expect("parent").join(format!("{checkpoint}.mark"));
    let _ = fs::remove_file(&mark);
    let mut child = Command::new(bin)
        .arg("apply")
        .arg(request)
        .env("CDC_APPLY_CHECKPOINT", checkpoint)
        .env("CDC_APPLY_MARK", &mark)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let started = Instant::now();
    while !mark.exists() {
        if started.elapsed() > Duration::from_secs(120) {
            let _ = child.kill();
            let output = child.wait_with_output().expect("wait");
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            panic!("mark missing stdout={stdout} stderr={stderr}");
        }
        if let Some(status) = child.try_wait().expect("try") {
            let output = child.wait_with_output().expect("wait");
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            panic!("exited early {status} stdout={stdout} stderr={stderr}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    child.kill().expect("kill");
    let _ = child.wait();
}
