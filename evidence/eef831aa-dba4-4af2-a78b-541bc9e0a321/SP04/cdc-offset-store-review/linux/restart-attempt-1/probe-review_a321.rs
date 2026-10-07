//! Independent review probe for cdc-offset-store.
//!
//! Receipts are written only by `_oo.commit_apply_receipt` after
//! `_oo.adopt_target_generation`. The mirror request has no receipt body.
//! A concurrent equal insert that hits unique_violation 23505 is reported
//! by the product as `receipt_hash_conflict`. That is the single-writer
//! limit. This probe does not claim multi-actor linearizability.

#![forbid(unsafe_code)]

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use cdc_offset_store::{
    hex, pos_ok, receipt_document, sha256, Receipt, BOOTSTRAP_SHA256, FENCE_SHA256,
    HISTORICAL_V2_SHA256, PREFLIGHT_SHA256, PROVENANCE,
};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;
use rustls::ClientConfig;
use tokio_postgres::types::ToSql;
use tokio_postgres::{Client, Config, Transaction};
use tokio_postgres_rustls::MakeRustlsConnect;

const OWNER: &str = "32132132-1321-4321-8321-321321321321";
const SOURCE: &str = "reviewsrc";
const CODEC: &str = "pg-commit-lsn/v1";
const LINEAGE: &str = "reviewline";
const SHARD: &str = "reviewsrc";

struct Env {
    host: String,
    port: u16,
    admin_pass: String,
    ca_file: String,
    ca: Vec<u8>,
    migrations: PathBuf,
    secrets: PathBuf,
    counts: PathBuf,
}

struct Files<'a> {
    env: &'a Env,
    reader: &'a Path,
    runtime: &'a Path,
}

struct Call {
    table: &'static str,
    txn: &'static str,
    pos: &'static str,
    tables: &'static [&'static str],
    expected: serde_json::Value,
    next_txn: &'static str,
    next_pos: &'static str,
    source_gap: bool,
    epoch_barrier: bool,
    extra: &'static [(&'static str, &'static str)],
}

struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
    value: serde_json::Value,
}

struct RowSpec {
    table: &'static str,
    txn: &'static str,
    pos: &'static str,
    epoch: i32,
    partial: bool,
    cross: bool,
    complete: bool,
    index: Option<i32>,
    count: Option<i32>,
    bitmap: &'static [u8],
    payload: &'static str,
}

struct Gold {
    table: String,
    txn: String,
    config_version: String,
    config_digest: String,
    type_policy_version: String,
    type_digest: String,
    hook_digest: String,
    schema_digest: String,
    payload_digest: String,
    protocol_version: String,
    codec: String,
    lineage: String,
    epoch: i32,
    pos: String,
    partial: bool,
    cross: bool,
    complete: bool,
    segment_index: Option<i32>,
    segment_count: Option<i32>,
    bitmap: Vec<u8>,
    fence_shard: String,
    fence_generation: i64,
    commit_at: String,
    hash: String,
}

#[tokio::test]
#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
async fn independent_chain() {
    if std::env::var("CDC_OFFSET_RESTART").ok().as_deref() == Some("1") {
        return;
    }
    if std::env::var_os("CDC_OFFSET_HOST").is_none() {
        panic!("CDC_OFFSET_HOST is required");
    }
    fips_vectors();
    scan_sources();
    let env = load_env(true);
    hash_migrations(&env);
    let reader_pass = write_secret(&env.secrets, "reader.pass");
    let writer_pass = write_secret(&env.secrets, "writer.pass");
    let runtime_pass = write_secret(&env.secrets, "runtime.pass");
    let control_admin_pass = write_secret(&env.secrets, "control-admin.pass");
    let reader_file = env.secrets.join("reader.pass");
    let runtime_file = env.secrets.join("runtime.pass");
    let control_admin_file = env.secrets.join("control-admin.pass");
    let files = Files { env: &env, reader: &reader_file, runtime: &runtime_file };

    let admin = connect(&env, "postgres", "oo_fixture_admin", &env.admin_pass).await.expect("admin");
    create_role(&admin, "oo_cdc_offset_admin", true, false).await;
    create_role(&admin, "oo_cdc_offset_runtime", false, false).await;
    create_role(&admin, "oo_a321_runtime", true, true).await;
    create_role(&admin, "oo_a321_reader", true, false).await;
    create_role(&admin, "oo_a321_writer", true, true).await;
    set_password(&admin, "oo_cdc_offset_admin", &control_admin_pass).await;
    set_password(&admin, "oo_a321_runtime", &runtime_pass).await;
    set_password(&admin, "oo_a321_reader", &reader_pass).await;
    set_password(&admin, "oo_a321_writer", &writer_pass).await;
    admin
        .batch_execute("GRANT oo_cdc_offset_runtime TO oo_a321_runtime WITH INHERIT TRUE")
        .await
        .expect("runtime membership");
    admin.batch_execute("CREATE DATABASE oo_a321_target").await.expect("target database");
    admin
        .batch_execute("CREATE DATABASE oo_a321_control OWNER oo_cdc_offset_admin")
        .await
        .expect("control database");
    admin
        .batch_execute(
            "REVOKE ALL ON DATABASE oo_a321_control FROM PUBLIC; \
             GRANT CONNECT ON DATABASE oo_a321_control TO oo_cdc_offset_admin; \
             GRANT CONNECT ON DATABASE oo_a321_control TO oo_a321_runtime;",
        )
        .await
        .expect("control connect");

    let mut target = connect(&env, "oo_a321_target", "oo_fixture_admin", &env.admin_pass)
        .await
        .expect("target");
    eprintln!("phase=bootstrap");
    apply_file(&mut target, &env.migrations.join("0000_bootstrap_roles.sql"), false, "").await;
    target
        .batch_execute("GRANT oo_pipeline_runtime TO oo_a321_writer WITH INHERIT TRUE")
        .await
        .expect("writer membership");
    let bootstrap = file_hash(&env.migrations.join("0000_bootstrap_roles.sql"));
    let fence = file_hash(&env.migrations.join("0001_target_fence_expand.sql"));
    let markers = format!(
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES \
         ('target-bootstrap/1', '{bootstrap}', 'expand', '0000_bootstrap_roles.sql'), \
         ('target-fence-receipts/1', '{fence}', 'expand', '0001_target_fence_expand.sql')"
    );
    eprintln!("phase=fence");
    apply_file(&mut target, &env.migrations.join("0001_target_fence_expand.sql"), true, &markers).await;
    target
        .batch_execute(
            "GRANT CONNECT ON DATABASE oo_a321_target TO oo_a321_reader; \
             GRANT USAGE ON SCHEMA _oo TO oo_a321_reader; \
             GRANT SELECT ON _oo.apply_receipts TO oo_a321_reader; \
             GRANT SELECT ON _oo.schema_migrations TO oo_a321_reader;",
        )
        .await
        .expect("reader grants");

    eprintln!("phase=install");
    let installed = run_install(&env, &control_admin_file);
    expect_install(&installed, 0, "installed");
    let replay_install = run_install(&env, &control_admin_file);
    expect_install(&replay_install, 0, "replay");

    eprintln!("phase=migration-drift");
    let drifted = mirror(
        &files,
        call("orders", "txn-missing", "10", &["orders"], serde_json::Value::Null, "txn-next", "11", false, false),
    );
    expect_out(&drifted, 2, "refused", false, None);
    assert_eq!(drifted.value["fault"], "migration_drift", "{}", drifted.stdout);
    assert_eq!(drifted.value["durable_control_mirror"], false);

    eprintln!("phase=preflight");
    let preflight = file_hash(&env.migrations.join("0003_target_segment_preflight_expand.sql"));
    let v3 = format!(
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES \
         ('target-fence-receipts/3', '{preflight}', 'expand', '0003_target_segment_preflight_expand.sql')"
    );
    apply_file(
        &mut target,
        &env.migrations.join("0003_target_segment_preflight_expand.sql"),
        true,
        &v3,
    )
    .await;
    assert!(!env.migrations.join("0004_physical_mapping.sql").exists(), "0004 must stay out of this archive");
    let versions: Vec<String> = target
        .query("SELECT version FROM _oo.schema_migrations ORDER BY version", &[])
        .await
        .expect("versions")
        .iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(
        versions,
        vec![
            "target-bootstrap/1".to_owned(),
            "target-fence-receipts/1".to_owned(),
            "target-fence-receipts/3".to_owned(),
        ]
    );

    eprintln!("phase=least-privilege");
    let reader = connect(&env, "oo_a321_target", "oo_a321_reader", &reader_pass).await.expect("reader");
    assert_denied(&reader, "INSERT INTO _oo.apply_receipts (source) VALUES ('reviewsrc')").await;
    assert_denied(&reader, "SELECT count(*) FROM _oo.target_generations").await;
    assert_connect_denied(&env, "oo_a321_control", "oo_a321_reader", &reader_pass).await;
    assert_connect_denied(&env, "oo_a321_target", "oo_a321_runtime", &runtime_pass).await;
    let runtime = connect(&env, "oo_a321_control", "oo_a321_runtime", &runtime_pass).await.expect("runtime");
    let control = connect(&env, "oo_a321_control", "oo_cdc_offset_admin", &control_admin_pass)
        .await
        .expect("control admin");
    control
        .batch_execute(
            "CREATE SCHEMA probe_foreign; REVOKE ALL ON SCHEMA probe_foreign FROM PUBLIC; \
             CREATE TABLE probe_foreign.secret (id integer);",
        )
        .await
        .expect("foreign schema");
    assert_denied(&runtime, "SELECT count(*) FROM probe_foreign.secret").await;
    assert_denied(&runtime, "CREATE TABLE oo_cdc_offset.runtime_denied (id integer)").await;

    eprintln!("phase=absent");
    let before_target = count_target(&target).await;
    let absent = mirror(
        &files,
        call("orders", "txn-absent", "10", &["orders"], serde_json::Value::Null, "txn-absent-next", "11", false, false),
    );
    expect_out(&absent, 2, "refused", false, None);
    assert_eq!(absent.value["fault"], "target_receipt_absent", "{}", absent.stdout);
    assert_eq!(count_target(&target).await, before_target);
    assert_eq!(count_sql(&control, "SELECT count(*) FROM oo_cdc_offset.mirrored_receipt").await, 0);

    let mut writer = connect(&env, "oo_a321_target", "oo_a321_writer", &writer_pass).await.expect("writer");
    let generation: i64 = writer
        .query_one(
            "SELECT _oo.adopt_target_generation($1, ($2::text)::uuid)",
            &[&SHARD, &OWNER],
        )
        .await
        .expect("adopt")
        .get(0);
    assert!(generation >= 1, "generation={generation}");
    let unfenced = writer.batch_execute("INSERT INTO _oo.apply_receipts (source) VALUES ('reviewsrc')").await;
    match unfenced {
        Ok(_) => panic!("unfenced insert landed"),
        Err(err) => {
            let state = sqlstate(&err);
            assert!(state == "OO014" || state == "42501", "{}", show(&err));
        }
    }
    assert_eq!(count_target(&target).await, before_target);

    eprintln!("phase=txn-1");
    write_rows(
        &mut writer,
        generation,
        &[complete("orders", "txn-1", "10", 1), complete("lines", "txn-1", "10", 1)],
    )
    .await;
    let target_after_write = count_target(&target).await;
    let first = mirror(
        &files,
        call("orders", "txn-1", "10", &["lines", "orders"], serde_json::Value::Null, "txn-2", "20", false, false),
    );
    expect_out(&first, 0, "mirrored", true, None);
    assert_eq!(plain_pos(first.value["watermark_applied_pos"].as_str().unwrap()), "10");
    assert_eq!(count_target(&target).await, target_after_write, "mirror wrote the target");
    let gold = load_gold(&target, "orders", "txn-1").await;
    assert_eq!(first.value["receipt_hash"], gold.hash, "cli {}", first.stdout);
    let stored: String = control
        .query_one(
            "SELECT receipt_hash FROM oo_cdc_offset.mirrored_receipt \
             WHERE source = $1 AND table_generation = 'orders' AND txn_or_batch_id = 'txn-1'",
            &[&SOURCE],
        )
        .await
        .expect("stored hash")
        .get(0);
    assert_eq!(stored, gold.hash);
    let lines = mirror(
        &files,
        call("lines", "txn-1", "10", &["lines", "orders"], serde_json::Value::Null, "txn-2", "20", false, false),
    );
    expect_out(&lines, 0, "mirrored", false, None);
    assert_eq!(watermark_txn(&control, 1).await, "txn-1");

    eprintln!("phase=replay");
    let replay = mirror(
        &files,
        call("orders", "txn-1", "10", &["lines", "orders"], serde_json::Value::Null, "txn-2", "20", false, false),
    );
    expect_out(&replay, 0, "replay", false, None);
    assert_eq!(replay.value["receipt_hash"], gold.hash);
    assert_eq!(count_target(&target).await, target_after_write);

    eprintln!("phase=payload-conflict");
    let changed = conflict_payload(&mut writer, generation).await;
    assert_eq!(changed, "OO010");
    let still = mirror(
        &files,
        call("orders", "txn-1", "10", &["lines", "orders"], serde_json::Value::Null, "txn-2", "20", false, false),
    );
    expect_out(&still, 0, "replay", false, None);
    assert_eq!(still.value["receipt_hash"], gold.hash);
    let stored_again: String = control
        .query_one(
            "SELECT receipt_hash FROM oo_cdc_offset.mirrored_receipt \
             WHERE source = $1 AND table_generation = 'orders' AND txn_or_batch_id = 'txn-1'",
            &[&SOURCE],
        )
        .await
        .expect("stored again")
        .get(0);
    assert_eq!(stored_again, gold.hash);

    eprintln!("phase=partial");
    write_rows(&mut writer, generation, &[partial("orders", "txn-p", "15", 1)]).await;
    let partial_run = mirror(
        &files,
        call("orders", "txn-p", "15", &["orders"], serde_json::Value::Null, "txn-p2", "16", false, false),
    );
    expect_out(&partial_run, 0, "mirrored", false, Some("partial_segment"));
    assert_eq!(watermark_txn(&control, 1).await, "txn-1");
    assert_eq!(plain_pos(&watermark_pos(&control, 1).await), "10");

    eprintln!("phase=hole");
    write_rows(
        &mut writer,
        generation,
        &[complete("orders", "txn-3", "40", 1), complete("lines", "txn-3", "40", 1)],
    )
    .await;
    let gap = mirror(
        &files,
        call("orders", "txn-3", "40", &["lines", "orders"], previous("10", "txn-1", 1), "txn-3b", "41", false, false),
    );
    expect_out(&gap, 0, "mirrored", false, Some("source_gap"));
    assert_eq!(plain_pos(&watermark_pos(&control, 1).await), "10");
    assert_eq!(watermark_txn(&control, 1).await, "txn-1");

    eprintln!("phase=epoch-detected");
    write_rows(&mut writer, generation, &[complete("orders", "txn-e2", "100", 2)]).await;
    let barrier = mirror(
        &files,
        call("orders", "txn-e2", "100", &["orders"], previous("10", "txn-1", 1), "txn-e3", "110", false, false),
    );
    expect_out(&barrier, 0, "mirrored", false, Some("epoch_barrier"));
    assert_eq!(watermark_txn(&control, 1).await, "txn-1");
    assert_eq!(
        count_sql(&control, "SELECT count(*) FROM oo_cdc_offset.watermark WHERE source_epoch = 2").await,
        0
    );
    let epoch_open = mirror(
        &files,
        call("orders", "txn-e2", "100", &["orders"], serde_json::Value::Null, "txn-e3", "110", false, false),
    );
    expect_out(&epoch_open, 0, "replay", true, None);
    assert_eq!(watermark_txn(&control, 1).await, "txn-1");
    assert_eq!(watermark_txn(&control, 2).await, "txn-e2");
    assert_eq!(plain_pos(&watermark_pos(&control, 2).await), "100");

    eprintln!("phase=caller-source-gap");
    write_rows(
        &mut writer,
        generation,
        &[complete("orders", "txn-2", "20", 1), complete("lines", "txn-2", "20", 1)],
    )
    .await;
    let flagged = mirror(
        &files,
        call("orders", "txn-2", "20", &["lines", "orders"], previous("10", "txn-1", 1), "txn-flag", "30", true, false),
    );
    expect_out(&flagged, 0, "mirrored", false, Some("source_gap"));
    assert_eq!(flagged.value["source_applied"], false);
    assert_eq!(watermark_txn(&control, 1).await, "txn-1");
    let cleared = mirror(
        &files,
        call("orders", "txn-2", "20", &["lines", "orders"], previous("10", "txn-1", 1), "txn-flag", "30", false, false),
    );
    expect_out(&cleared, 0, "replay", true, None);
    assert_eq!(cleared.value["source_applied"], false, "cleared flag still does not certify the source");
    assert_eq!(watermark_txn(&control, 1).await, "txn-2");
    assert_eq!(plain_pos(&watermark_pos(&control, 1).await), "20");

    eprintln!("phase=caller-epoch-barrier");
    write_rows(
        &mut writer,
        generation,
        &[complete("orders", "txn-flag", "30", 1), complete("lines", "txn-flag", "30", 1)],
    )
    .await;
    let epoch_flag = mirror(
        &files,
        call(
            "orders",
            "txn-flag",
            "30",
            &["lines", "orders"],
            previous("20", "txn-2", 1),
            "txn-after",
            "40",
            false,
            true,
        ),
    );
    expect_out(&epoch_flag, 0, "mirrored", false, Some("epoch_barrier"));
    assert_eq!(epoch_flag.value["source_applied"], false);
    assert_eq!(epoch_flag.value["fence_proof"], false);
    assert_eq!(watermark_txn(&control, 1).await, "txn-2");
    let epoch_clear = mirror(
        &files,
        call(
            "orders",
            "txn-flag",
            "30",
            &["lines", "orders"],
            previous("20", "txn-2", 1),
            "txn-after",
            "40",
            false,
            false,
        ),
    );
    expect_out(&epoch_clear, 0, "replay", true, None);
    assert_eq!(watermark_txn(&control, 1).await, "txn-flag");
    assert_eq!(plain_pos(&watermark_pos(&control, 1).await), "30");

    eprintln!("phase=equal-insert-23505");
    write_rows(&mut writer, generation, &[complete("orders", "txn-race", "16", 1)]).await;
    let race_gold = load_gold(&target, "orders", "txn-race").await;
    let mark_before = watermark_txn(&control, 1).await;
    let mut holder = connect(&env, "oo_a321_control", "oo_a321_runtime", &runtime_pass)
        .await
        .expect("holder");
    let hold_tx = holder.transaction().await.expect("hold tx");
    hold_tx
        .batch_execute("SET LOCAL lock_timeout = '30s'; SET LOCAL statement_timeout = '30s'")
        .await
        .expect("hold limits");
    let inserted = insert_mirror(&hold_tx, &race_gold).await.expect("holder insert");
    assert_eq!(inserted, 1);
    let race_call = call(
        "orders",
        "txn-race",
        "16",
        &["orders"],
        previous("30", "txn-flag", 1),
        "txn-race-next",
        "17",
        false,
        false,
    );
    let mut child = spawn_mirror(&files, &race_call);
    let admin_control = connect(&env, "oo_a321_control", "oo_fixture_admin", &env.admin_pass)
        .await
        .expect("admin control");
    let started = Instant::now();
    let mut saw_lock = false;
    while started.elapsed() < Duration::from_secs(20) {
        if child.try_wait().expect("try_wait").is_some() {
            break;
        }
        let waiting: i64 = admin_control
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_stat_activity \
                 WHERE datname = 'oo_a321_control' AND usename = 'oo_a321_runtime' \
                   AND state = 'active' AND wait_event_type = 'Lock'",
                &[],
            )
            .await
            .expect("activity")
            .get(0);
        if waiting >= 1 {
            saw_lock = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(saw_lock, "product insert did not wait on the uncommitted equal row");
    hold_tx.commit().await.expect("holder commit");
    let race = wait_child(child).await;
    expect_out(&race, 2, "refused", false, None);
    assert_eq!(race.value["fault"], "receipt_hash_conflict", "{}", race.stdout);
    assert_eq!(race.value["durable_control_mirror"], false);
    assert_eq!(
        count_sql(
            &control,
            "SELECT count(*) FROM oo_cdc_offset.mirrored_receipt WHERE txn_or_batch_id = 'txn-race'",
        )
        .await,
        1
    );
    let race_stored: String = control
        .query_one(
            "SELECT receipt_hash FROM oo_cdc_offset.mirrored_receipt \
             WHERE source = $1 AND table_generation = 'orders' AND txn_or_batch_id = 'txn-race'",
            &[&SOURCE],
        )
        .await
        .expect("race hash")
        .get(0);
    assert_eq!(race_stored, race_gold.hash, "equal hash was stored by the other session");
    assert_eq!(watermark_txn(&control, 1).await, mark_before);
    let race_replay = mirror(&files, race_call);
    expect_out(&race_replay, 0, "replay", false, Some("source_gap"));
    assert_eq!(race_replay.value["receipt_hash"], race_gold.hash);
    assert_eq!(watermark_txn(&control, 1).await, "txn-flag");

    eprintln!("phase=hide-commit");
    write_rows(
        &mut writer,
        generation,
        &[complete("orders", "txn-after", "40", 1), complete("lines", "txn-after", "40", 1)],
    )
    .await;
    let hidden = mirror(
        &files,
        Call {
            extra: &[("CDC_OFFSET_INJECT", "hide_commit")],
            ..call(
                "orders",
                "txn-after",
                "40",
                &["lines", "orders"],
                previous("30", "txn-flag", 1),
                "txn-5",
                "50",
                false,
                false,
            )
        },
    );
    expect_out(&hidden, 0, "replay", true, None);
    assert_eq!(hidden.value["fault"], serde_json::Value::Null);
    assert_eq!(watermark_txn(&control, 1).await, "txn-after");
    assert_eq!(plain_pos(&watermark_pos(&control, 1).await), "40");
    assert_eq!(
        count_sql(
            &control,
            "SELECT count(*) FROM oo_cdc_offset.mirrored_receipt WHERE txn_or_batch_id = 'txn-after'",
        )
        .await,
        1
    );

    eprintln!("phase=commit-unknown");
    let before_unknown = count_sql(&control, "SELECT count(*) FROM oo_cdc_offset.mirrored_receipt").await;
    let epoch1_before = watermark_pos(&control, 1).await;
    write_rows(&mut writer, generation, &[partial("orders", "txn-u", "17", 1)]).await;
    let unknown = mirror(
        &files,
        Call {
            extra: &[("CDC_OFFSET_INJECT", "hide_commit"), ("CDC_OFFSET_RECONCILE", "down")],
            ..call("orders", "txn-u", "17", &["orders"], serde_json::Value::Null, "txn-u2", "18", false, false)
        },
    );
    expect_out(&unknown, 3, "commit_unknown", false, None);
    assert_eq!(unknown.value["durable_control_mirror"], false);
    assert_eq!(unknown.value["external_xack"], false);
    assert_eq!(unknown.value["external_xtrim"], false);
    let recovered = mirror(
        &files,
        call("orders", "txn-u", "17", &["orders"], serde_json::Value::Null, "txn-u2", "18", false, false),
    );
    expect_out(&recovered, 0, "replay", false, Some("partial_segment"));
    assert_eq!(
        count_sql(&control, "SELECT count(*) FROM oo_cdc_offset.mirrored_receipt").await,
        before_unknown + 1,
        "lookup outage must not insert a second row"
    );
    assert_eq!(watermark_pos(&control, 1).await, epoch1_before);

    eprintln!("phase=immutable");
    assert_denied(
        &runtime,
        "UPDATE oo_cdc_offset.mirrored_receipt SET receipt_hash = repeat('ab', 32) WHERE txn_or_batch_id = 'txn-1'",
    )
    .await;
    let admin_update = control
        .batch_execute(
            "UPDATE oo_cdc_offset.mirrored_receipt SET receipt_hash = repeat('ab', 32) WHERE txn_or_batch_id = 'txn-1'",
        )
        .await;
    match admin_update {
        Ok(_) => panic!("admin updated an immutable receipt"),
        Err(err) => assert_eq!(sqlstate(&err), "23514", "{}", show(&err)),
    }

    eprintln!("phase=install-drift");
    control
        .batch_execute("CREATE TABLE oo_cdc_offset.drift_probe (id integer)")
        .await
        .expect("drift table");
    let drift = run_install(&env, &control_admin_file);
    expect_install(&drift, 2, "refused");
    assert_eq!(drift.value["fault"], "install_drift", "{}", drift.stdout);
    control.batch_execute("DROP TABLE oo_cdc_offset.drift_probe").await.expect("drop drift");
    let repaired = run_install(&env, &control_admin_file);
    expect_install(&repaired, 0, "replay");

    let receipts = count_sql(&control, "SELECT count(*) FROM oo_cdc_offset.mirrored_receipt").await;
    let watermarks = count_sql(&control, "SELECT count(*) FROM oo_cdc_offset.watermark").await;
    assert_eq!(receipts, 10, "receipts={receipts}");
    assert_eq!(watermarks, 2);
    assert_eq!(plain_pos(&watermark_pos(&control, 1).await), "40");
    assert_eq!(watermark_txn(&control, 1).await, "txn-after");
    assert_eq!(plain_pos(&watermark_pos(&control, 2).await), "100");
    assert_eq!(watermark_txn(&control, 2).await, "txn-e2");
    let body = serde_json::json!({
        "receipts": receipts,
        "watermarks": watermarks,
        "epoch1_pos": plain_pos(&watermark_pos(&control, 1).await),
        "epoch1_txn": watermark_txn(&control, 1).await,
        "epoch2_pos": plain_pos(&watermark_pos(&control, 2).await),
        "epoch2_txn": watermark_txn(&control, 2).await,
        "txn1_hash": gold.hash,
        "race_fault": "receipt_hash_conflict",
        "linearizable_multi_actor": false,
        "source_applied": false,
        "ack_allowed": false,
        "external_xack": false,
        "external_xtrim": false,
    });
    fs::write(&env.counts, serde_json::to_vec(&body).unwrap()).unwrap();
    eprintln!("phase=counts {body}");
}

#[tokio::test]
async fn independent_restart() {
    if std::env::var("CDC_OFFSET_RESTART").ok().as_deref() != Some("1") {
        return;
    }
    let env = load_env(false);
    let recorded: serde_json::Value = serde_json::from_slice(&fs::read(&env.counts).expect("counts")).unwrap();
    let control = connect(&env, "oo_a321_control", "oo_fixture_admin", &env.admin_pass)
        .await
        .expect("control");
    let receipts = count_sql(&control, "SELECT count(*) FROM oo_cdc_offset.mirrored_receipt").await;
    let watermarks = count_sql(&control, "SELECT count(*) FROM oo_cdc_offset.watermark").await;
    assert_eq!(receipts, recorded["receipts"].as_i64().unwrap(), "{recorded}");
    assert_eq!(watermarks, recorded["watermarks"].as_i64().unwrap(), "{recorded}");
    assert_eq!(plain_pos(&watermark_pos(&control, 1).await), recorded["epoch1_pos"].as_str().unwrap());
    assert_eq!(watermark_txn(&control, 1).await, recorded["epoch1_txn"].as_str().unwrap());
    assert_eq!(plain_pos(&watermark_pos(&control, 2).await), recorded["epoch2_pos"].as_str().unwrap());
    assert_eq!(watermark_txn(&control, 2).await, recorded["epoch2_txn"].as_str().unwrap());
    let reader_file = env.secrets.join("reader.pass");
    let runtime_file = env.secrets.join("runtime.pass");
    let files = Files { env: &env, reader: &reader_file, runtime: &runtime_file };
    let replay = mirror(
        &files,
        call("orders", "txn-1", "10", &["lines", "orders"], serde_json::Value::Null, "txn-2", "20", false, false),
    );
    expect_out(&replay, 0, "replay", false, None);
    assert_eq!(replay.value["receipt_hash"], recorded["txn1_hash"]);
    assert_eq!(plain_pos(&watermark_pos(&control, 1).await), recorded["epoch1_pos"].as_str().unwrap());
    assert_eq!(watermark_txn(&control, 1).await, recorded["epoch1_txn"].as_str().unwrap());
    eprintln!("restart_counts_ok");
}

fn fips_vectors() {
    let empty = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    assert_eq!(hex(&sha256(b"").unwrap()), empty);
    assert_eq!(hex(&sha256(b"abc").unwrap()), abc);
}

fn scan_sources() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut blob = String::new();
    for name in ["lib.rs", "main.rs", "model.rs", "rules.rs", "sha256.rs", "sql.rs", "store.rs", "tls.rs"] {
        blob.push_str(&fs::read_to_string(root.join("src").join(name)).unwrap());
    }
    for needle in ["XACK", "XTRIM", "VALKEY", "valkey", "XADD", "XGROUP"] {
        assert!(!blob.contains(needle), "{needle}");
    }
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(!manifest.contains("cdc-apply-pg"), "{manifest}");
    assert!(!manifest.contains("path ="), "{manifest}");
}

fn hash_migrations(env: &Env) {
    assert_eq!(file_hash(&env.migrations.join("0000_bootstrap_roles.sql")), BOOTSTRAP_SHA256);
    assert_eq!(file_hash(&env.migrations.join("0001_target_fence_expand.sql")), FENCE_SHA256);
    assert_eq!(file_hash(&env.migrations.join("0002_target_segment_null_expand.sql")), HISTORICAL_V2_SHA256);
    assert_eq!(file_hash(&env.migrations.join("0003_target_segment_preflight_expand.sql")), PREFLIGHT_SHA256);
    assert!(!env.migrations.join("0004_physical_mapping.sql").exists());
}

fn file_hash(path: &Path) -> String {
    hex(&sha256(&fs::read(path).unwrap_or_else(|_| panic!("missing {}", path.display()))).unwrap())
}

fn load_env(reset_secrets: bool) -> Env {
    let host = std::env::var("CDC_OFFSET_HOST").unwrap();
    let port = std::env::var("CDC_OFFSET_PORT").unwrap().parse().unwrap();
    let admin_pass = fs::read_to_string(std::env::var("CDC_OFFSET_ADMIN_PASS_FILE").unwrap()).unwrap();
    let ca_file = std::env::var("CDC_OFFSET_CA_FILE").unwrap();
    let ca = fs::read(&ca_file).unwrap();
    let secrets = PathBuf::from(std::env::var("CDC_OFFSET_SECRET_DIR").unwrap());
    if reset_secrets {
        let _ = fs::remove_dir_all(&secrets);
        fs::create_dir_all(&secrets).unwrap();
    }
    Env {
        host,
        port,
        admin_pass: admin_pass.trim().to_owned(),
        ca_file,
        ca,
        migrations: PathBuf::from(std::env::var("CDC_OFFSET_MIGRATIONS").unwrap()),
        secrets,
        counts: PathBuf::from(std::env::var("CDC_OFFSET_COUNTS").unwrap()),
    }
}

fn write_secret(dir: &Path, name: &str) -> String {
    let left = rand_u128();
    let right = rand_u128();
    let value = format!("{left:032x}{right:032x}");
    fs::write(dir.join(name), &value).unwrap();
    value
}

fn rand_u128() -> u128 {
    let mut bytes = [0u8; 16];
    File::open("/dev/urandom").unwrap().read_exact(&mut bytes).unwrap();
    u128::from_ne_bytes(bytes)
}

fn complete(table: &'static str, txn: &'static str, pos: &'static str, epoch: i32) -> RowSpec {
    RowSpec {
        table,
        txn,
        pos,
        epoch,
        partial: false,
        cross: true,
        complete: true,
        index: None,
        count: None,
        bitmap: &[],
        payload: "payload-a",
    }
}

fn partial(table: &'static str, txn: &'static str, pos: &'static str, epoch: i32) -> RowSpec {
    RowSpec {
        table,
        txn,
        pos,
        epoch,
        partial: true,
        cross: false,
        complete: false,
        index: Some(0),
        count: Some(2),
        bitmap: &[0x01],
        payload: "payload-a",
    }
}

#[allow(clippy::too_many_arguments)]
fn call(
    table: &'static str,
    txn: &'static str,
    pos: &'static str,
    tables: &'static [&'static str],
    expected: serde_json::Value,
    next_txn: &'static str,
    next_pos: &'static str,
    source_gap: bool,
    epoch_barrier: bool,
) -> Call {
    Call {
        table,
        txn,
        pos,
        tables,
        expected,
        next_txn,
        next_pos,
        source_gap,
        epoch_barrier,
        extra: &[],
    }
}

fn previous(pos: &str, txn: &str, epoch: i32) -> serde_json::Value {
    serde_json::json!({
        "position_codec": CODEC,
        "position_lineage": LINEAGE,
        "source_epoch": epoch,
        "applied_pos": pos,
        "txn_or_batch_id": txn,
    })
}

fn mirror(files: &Files<'_>, call: Call) -> Run {
    let body = serde_json::json!({
        "contract": "cdc-offset-store-v1",
        "target": endpoint(files.env, "oo_a321_target", "oo_a321_reader", files.reader),
        "control": endpoint(files.env, "oo_a321_control", "oo_a321_runtime", files.runtime),
        "source": SOURCE,
        "table_generation": call.table,
        "txn_or_batch_id": call.txn,
        "order_binding": {
            "provenance": PROVENANCE,
            "expected_previous": call.expected,
            "declared_transaction": {
                "txn_or_batch_id": call.txn,
                "applied_pos": call.pos,
                "required_table_generations": call.tables,
            },
            "declared_next": {"txn_or_batch_id": call.next_txn, "applied_pos": call.next_pos},
            "source_gap": call.source_gap,
            "epoch_barrier": call.epoch_barrier,
        }
    });
    let text = body.to_string();
    assert!(!text.contains("payload_digest"), "{text}");
    assert!(!text.contains("receipt_hash"), "{text}");
    assert!(!text.contains("segment_bitmap"), "{text}");
    run_cli("mirror", &body, call.extra)
}

fn spawn_mirror(files: &Files<'_>, call: &Call) -> std::process::Child {
    let body = serde_json::json!({
        "contract": "cdc-offset-store-v1",
        "target": endpoint(files.env, "oo_a321_target", "oo_a321_reader", files.reader),
        "control": endpoint(files.env, "oo_a321_control", "oo_a321_runtime", files.runtime),
        "source": SOURCE,
        "table_generation": call.table,
        "txn_or_batch_id": call.txn,
        "order_binding": {
            "provenance": PROVENANCE,
            "expected_previous": call.expected,
            "declared_transaction": {
                "txn_or_batch_id": call.txn,
                "applied_pos": call.pos,
                "required_table_generations": call.tables,
            },
            "declared_next": {"txn_or_batch_id": call.next_txn, "applied_pos": call.next_pos},
            "source_gap": call.source_gap,
            "epoch_barrier": call.epoch_barrier,
        }
    });
    let path = std::env::temp_dir().join(format!("cdc-offset-race-{}.json", std::process::id()));
    fs::write(&path, serde_json::to_vec(&body).unwrap()).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cdc-offset-store"));
    cmd.arg("mirror").arg(&path);
    for key in ["CDC_OFFSET_INJECT", "CDC_OFFSET_RECONCILE", "CDC_OFFSET_CHECKPOINT", "CDC_OFFSET_MARK"] {
        cmd.env_remove(key);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap()
}

async fn wait_child(mut child: std::process::Child) -> Run {
    let started = Instant::now();
    let status = loop {
        if started.elapsed() > Duration::from_secs(30) {
            let _ = child.kill();
            panic!("race child timed out");
        }
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let mut stdout = String::new();
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        pipe.read_to_string(&mut stdout).unwrap();
    }
    if let Some(mut pipe) = child.stderr.take() {
        pipe.read_to_string(&mut stderr).unwrap();
    }
    let stdout = stdout.trim().to_owned();
    let stderr = stderr.trim().to_owned();
    let value = serde_json::from_str(&stdout).unwrap_or(serde_json::Value::Null);
    Run { code: status.code(), stdout, stderr, value }
}

fn run_install(env: &Env, admin_file: &Path) -> Run {
    let body = serde_json::json!({
        "contract": "cdc-offset-store-v1",
        "control": endpoint(env, "oo_a321_control", "oo_cdc_offset_admin", admin_file),
    });
    run_cli("install", &body, &[])
}

fn endpoint(env: &Env, database: &str, user: &str, password_file: &Path) -> serde_json::Value {
    serde_json::json!({
        "host": env.host,
        "port": env.port,
        "database": database,
        "user": user,
        "password_file": password_file,
        "ca_file": env.ca_file,
        "server_name": env.host,
    })
}

fn run_cli(command: &str, body: &serde_json::Value, extra: &[(&str, &str)]) -> Run {
    let path = std::env::temp_dir().join(format!("cdc-offset-{command}-{}.json", std::process::id()));
    fs::write(&path, serde_json::to_vec(body).unwrap()).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cdc-offset-store"));
    cmd.arg(command).arg(&path);
    for key in ["CDC_OFFSET_INJECT", "CDC_OFFSET_RECONCILE", "CDC_OFFSET_CHECKPOINT", "CDC_OFFSET_MARK"] {
        cmd.env_remove(key);
    }
    for (key, value) in extra {
        cmd.env(key, value);
    }
    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let value = serde_json::from_str(&stdout).unwrap_or(serde_json::Value::Null);
    Run { code: output.status.code(), stdout, stderr, value }
}

fn expect_install(run: &Run, code: i32, kind: &str) {
    assert_eq!(run.code, Some(code), "{}\n{}", run.stdout, run.stderr);
    assert_eq!(run.value["kind"], kind, "{}", run.stdout);
    assert_eq!(run.value["contract"], "cdc-offset-store-v1");
    assert_eq!(run.value["target_touched"], false, "{}", run.stdout);
}

fn expect_out(run: &Run, code: i32, kind: &str, advanced: bool, hold: Option<&str>) {
    assert_eq!(run.code, Some(code), "{}\n{}", run.stdout, run.stderr);
    assert_eq!(run.value["kind"], kind, "{}", run.stdout);
    assert_eq!(run.value["contract"], "cdc-offset-store-v1");
    assert_eq!(run.value["checkpoint_provenance"], PROVENANCE, "{}", run.stdout);
    for key in [
        "source_applied",
        "fence_proof",
        "ack_allowed",
        "external_xack",
        "external_xtrim",
        "routing_verified",
        "real_source_provenance_verified",
        "debezium_normalizer",
    ] {
        assert_eq!(run.value[key], false, "{key} {}", run.stdout);
    }
    if kind == "mirrored" || kind == "replay" {
        assert_eq!(run.value["checkpoint_advanced"], advanced, "{}", run.stdout);
        assert_eq!(run.value["durable_control_mirror"], true, "{}", run.stdout);
    }
    match hold {
        Some(text) => assert_eq!(run.value["watermark_hold"], text, "{}", run.stdout),
        None => assert!(run.value["watermark_hold"].is_null(), "{}", run.stdout),
    }
}

async fn load_gold(client: &Client, table: &str, txn: &str) -> Gold {
    let row = client
        .query_one(
            "SELECT table_generation, config_version, config_digest, type_policy_version, type_digest, \
                    hook_digest, schema_digest, payload_digest, protocol_version, position_codec, \
                    position_lineage, source_epoch, applied_pos::text, partial_txn, cross_table_consistent, \
                    txn_complete, segment_index, segment_count, encode(segment_bitmap, 'hex'), \
                    fence_shard, fence_generation, \
                    to_char(target_commit_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
             FROM _oo.apply_receipts \
             WHERE source = $1 AND table_generation = $2 AND txn_or_batch_id = $3",
            &[&SOURCE, &table, &txn],
        )
        .await
        .unwrap_or_else(|err| panic!("gold {} {}", txn, show(&err)));
    let bitmap_hex: String = row.get(18);
    let bitmap = decode_hex(&bitmap_hex);
    let index: Option<i32> = row.get(16);
    let count: Option<i32> = row.get(17);
    let partial: bool = row.get(13);
    let cross: bool = row.get(14);
    let complete: bool = row.get(15);
    let epoch: i32 = row.get(11);
    let fence_generation: i64 = row.get(20);
    let pos = plain_pos(&row.get::<_, String>(12));
    let commit_at: String = row.get(21);
    let mut document = String::from("cdc-offset-receipt/v1\n");
    push(&mut document, "source", SOURCE);
    push(&mut document, "table_generation", &row.get::<_, String>(0));
    push(&mut document, "txn_or_batch_id", txn);
    push(&mut document, "config_version", &row.get::<_, String>(1));
    push(&mut document, "config_digest", &row.get::<_, String>(2));
    push(&mut document, "type_policy_version", &row.get::<_, String>(3));
    push(&mut document, "type_digest", &row.get::<_, String>(4));
    push(&mut document, "hook_digest", &row.get::<_, String>(5));
    push(&mut document, "schema_digest", &row.get::<_, String>(6));
    push(&mut document, "payload_digest", &row.get::<_, String>(7));
    push(&mut document, "protocol_version", &row.get::<_, String>(8));
    push(&mut document, "position_codec", &row.get::<_, String>(9));
    push(&mut document, "position_lineage", &row.get::<_, String>(10));
    push(&mut document, "source_epoch", &epoch.to_string());
    push(&mut document, "applied_pos", &pos);
    push(&mut document, "partial_txn", bit(partial));
    push(&mut document, "cross_table_consistent", bit(cross));
    push(&mut document, "txn_complete", bit(complete));
    push(&mut document, "segment_index", &opt_i32(index));
    push(&mut document, "segment_count", &opt_i32(count));
    push(&mut document, "segment_bitmap", &bitmap_text(&bitmap));
    push(&mut document, "fence_shard", &row.get::<_, String>(19));
    push(&mut document, "fence_generation", &fence_generation.to_string());
    push(&mut document, "target_commit_at", &commit_at);
    let receipt = Receipt {
        source: SOURCE.to_owned(),
        table_generation: row.get(0),
        txn_or_batch_id: txn.to_owned(),
        config_version: row.get(1),
        config_digest: row.get(2),
        type_policy_version: row.get(3),
        type_digest: row.get(4),
        hook_digest: row.get(5),
        schema_digest: row.get(6),
        payload_digest: row.get(7),
        protocol_version: row.get(8),
        position_codec: row.get(9),
        position_lineage: row.get(10),
        source_epoch: epoch,
        applied_pos: pos.clone(),
        partial_txn: partial,
        cross_table_consistent: cross,
        txn_complete: complete,
        segment_index: index,
        segment_count: count,
        segment_bitmap: bitmap.clone(),
        fence_shard: row.get(19),
        fence_generation,
        target_commit_at: commit_at.clone(),
    };
    let library = receipt_document(&receipt);
    assert_eq!(library, document, "gold document drifted from the library");
    let hash = hex(&sha256(document.as_bytes()).unwrap());
    Gold {
        table: row.get(0),
        txn: txn.to_owned(),
        config_version: row.get(1),
        config_digest: row.get(2),
        type_policy_version: row.get(3),
        type_digest: row.get(4),
        hook_digest: row.get(5),
        schema_digest: row.get(6),
        payload_digest: row.get(7),
        protocol_version: row.get(8),
        codec: row.get(9),
        lineage: row.get(10),
        epoch,
        pos,
        partial,
        cross,
        complete,
        segment_index: index,
        segment_count: count,
        bitmap,
        fence_shard: row.get(19),
        fence_generation,
        commit_at,
        hash,
    }
}

async fn insert_mirror(tx: &Transaction<'_>, gold: &Gold) -> Result<u64, tokio_postgres::Error> {
    let epoch = gold.epoch;
    let partial = gold.partial;
    let cross = gold.cross;
    let complete = gold.complete;
    let index = gold.segment_index;
    let count = gold.segment_count;
    let fence = gold.fence_generation;
    let params: [&(dyn ToSql + Sync); 25] = [
        &SOURCE,
        &gold.table,
        &gold.txn,
        &gold.config_version,
        &gold.config_digest,
        &gold.type_policy_version,
        &gold.type_digest,
        &gold.hook_digest,
        &gold.schema_digest,
        &gold.payload_digest,
        &gold.protocol_version,
        &gold.codec,
        &gold.lineage,
        &epoch,
        &gold.pos,
        &partial,
        &cross,
        &complete,
        &index,
        &count,
        &gold.bitmap,
        &gold.fence_shard,
        &fence,
        &gold.commit_at,
        &gold.hash,
    ];
    tx.execute(
        "INSERT INTO oo_cdc_offset.mirrored_receipt ( \
            source, table_generation, txn_or_batch_id, config_version, config_digest, type_policy_version, \
            type_digest, hook_digest, schema_digest, payload_digest, protocol_version, position_codec, \
            position_lineage, source_epoch, applied_pos, partial_txn, cross_table_consistent, txn_complete, \
            segment_index, segment_count, segment_bitmap, fence_shard, fence_generation, target_commit_at, \
            receipt_hash \
         ) VALUES ( \
            $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,($15::text)::numeric,$16,$17,$18,$19,$20,$21,$22,$23,($24::text)::timestamptz,$25 \
         )",
        &params,
    )
    .await
}

fn push(text: &mut String, key: &str, value: &str) {
    text.push_str(key);
    text.push('=');
    text.push_str(value);
    text.push('\n');
}

fn bit(value: bool) -> &'static str {
    if value { "1" } else { "0" }
}

fn opt_i32(value: Option<i32>) -> String {
    value.map(|item| item.to_string()).unwrap_or_else(|| "-".to_owned())
}

fn bitmap_text(bitmap: &[u8]) -> String {
    if bitmap.is_empty() { "-".to_owned() } else { hex(bitmap) }
}

fn decode_hex(value: &str) -> Vec<u8> {
    if value.is_empty() {
        return Vec::new();
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).unwrap())
        .collect()
}

fn plain_pos(value: &str) -> String {
    let value = value.trim();
    let whole = match value.split_once('.') {
        Some((whole, fraction)) if fraction.chars().all(|ch| ch == '0') => whole,
        Some(_) => panic!("position fraction {value}"),
        None => value,
    };
    assert!(pos_ok(whole), "{whole}");
    whole.to_owned()
}

async fn write_rows(writer: &mut Client, generation: i64, rows: &[RowSpec]) {
    let tx = writer.transaction().await.unwrap();
    for row in rows {
        let status = insert_receipt(&tx, generation, row).await.unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(status, "inserted", "{}", row.txn);
    }
    tx.commit().await.unwrap();
}

async fn conflict_payload(writer: &mut Client, generation: i64) -> String {
    let tx = writer.transaction().await.unwrap();
    let row = complete("orders", "txn-1", "10", 1);
    let err = insert_receipt_payload(&tx, generation, &row, "payload-b").await.unwrap_err();
    let _ = tx.rollback().await;
    err
}

async fn insert_receipt(tx: &Transaction<'_>, generation: i64, row: &RowSpec) -> Result<String, String> {
    insert_receipt_payload(tx, generation, row, row.payload).await
}

async fn insert_receipt_payload(
    tx: &Transaction<'_>,
    generation: i64,
    row: &RowSpec,
    payload: &str,
) -> Result<String, String> {
    let config = digest("config");
    let type_digest = digest("type");
    let hook = digest("hook");
    let schema = digest("schema");
    let payload_digest = digest(payload);
    let index = row.index;
    let count = row.count;
    let bitmap = row.bitmap.to_vec();
    let partial = row.partial;
    let cross = row.cross;
    let complete = row.complete;
    let epoch = row.epoch;
    let params: [&(dyn ToSql + Sync); 21] = [
        &SHARD,
        &OWNER,
        &generation,
        &SOURCE,
        &row.table,
        &row.txn,
        &config,
        &type_digest,
        &hook,
        &schema,
        &payload_digest,
        &CODEC,
        &LINEAGE,
        &epoch,
        &row.pos,
        &partial,
        &cross,
        &complete,
        &index,
        &count,
        &bitmap,
    ];
    tx.query_one(
        "SELECT _oo.commit_apply_receipt($1, ($2::text)::uuid, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, ($15::text)::numeric, $16, $17, $18, $19, $20, $21)",
        &params,
    )
    .await
    .map(|row| row.get(0))
    .map_err(|err| sqlstate(&err))
}

fn digest(label: &str) -> String {
    hex(&sha256(label.as_bytes()).unwrap())
}

async fn create_role(client: &Client, name: &str, login: bool, inherit: bool) {
    let login_sql = if login { "LOGIN" } else { "NOLOGIN" };
    let inherit_sql = if inherit { "INHERIT" } else { "NOINHERIT" };
    let sql = format!(
        "CREATE ROLE {name} {login_sql} {inherit_sql} NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS"
    );
    client.batch_execute(&sql).await.unwrap_or_else(|err| panic!("{name} {}", show(&err)));
}

async fn set_password(client: &Client, role: &str, password: &str) {
    let template = format!("ALTER ROLE {role} PASSWORD %L");
    let sql: String = client
        .query_one("SELECT format($1::text, $2::text)", &[&template, &password])
        .await
        .unwrap_or_else(|err| panic!("{}", show(&err)))
        .get(0);
    client.batch_execute(&sql).await.unwrap_or_else(|err| panic!("{}", show(&err)));
}

async fn connect(env: &Env, database: &str, user: &str, password: &str) -> Result<Client, tokio_postgres::Error> {
    let certificate = CertificateDer::from_pem_slice(&env.ca).expect("ca");
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate).expect("ca");
    let config = ClientConfig::builder_with_provider(rustls::crypto::aws_lc_rs::default_provider().into())
        .with_safe_default_protocol_versions()
        .expect("tls")
        .with_root_certificates(roots)
        .with_no_client_auth();
    let tls = MakeRustlsConnect::new(config);
    let mut builder = Config::new();
    builder
        .host(&env.host)
        .port(env.port)
        .dbname(database)
        .user(user)
        .password(password)
        .ssl_mode(tokio_postgres::config::SslMode::Require)
        .connect_timeout(Duration::from_secs(20));
    let (client, task) = builder.connect(tls).await?;
    tokio::spawn(async move {
        let _ = task.await;
    });
    Ok(client)
}

async fn assert_connect_denied(env: &Env, database: &str, user: &str, password: &str) {
    match connect(env, database, user, password).await {
        Ok(_) => panic!("{user} connected to {database}"),
        Err(err) => assert_eq!(sqlstate(&err), "42501", "{}", show(&err)),
    }
}

async fn assert_denied(client: &Client, sql: &str) {
    match client.batch_execute(sql).await {
        Ok(_) => panic!("allowed: {sql}"),
        Err(err) => assert_eq!(sqlstate(&err), "42501", "{} {}", show(&err), sql),
    }
}

fn sqlstate(err: &tokio_postgres::Error) -> String {
    err.code().map(|code| code.code().to_owned()).unwrap_or_default()
}

fn show(err: &tokio_postgres::Error) -> String {
    match err.as_db_error() {
        Some(db) => format!("{} {}", db.code().code(), db.message()),
        None => err.to_string(),
    }
}

async fn count_target(client: &Client) -> i64 {
    client.query_one("SELECT count(*) FROM _oo.apply_receipts", &[]).await.unwrap().get(0)
}

async fn count_sql(client: &Client, sql: &str) -> i64 {
    client.query_one(sql, &[]).await.unwrap_or_else(|err| panic!("{} {}", sql, show(&err))).get(0)
}

async fn watermark_pos(client: &Client, epoch: i32) -> String {
    client
        .query_one(
            "SELECT applied_pos::text FROM oo_cdc_offset.watermark WHERE source = $1 AND source_epoch = $2",
            &[&SOURCE, &epoch],
        )
        .await
        .unwrap_or_else(|err| panic!("{}", show(&err)))
        .get(0)
}

async fn watermark_txn(client: &Client, epoch: i32) -> String {
    client
        .query_one(
            "SELECT applied_txn_id FROM oo_cdc_offset.watermark WHERE source = $1 AND source_epoch = $2",
            &[&SOURCE, &epoch],
        )
        .await
        .unwrap_or_else(|err| panic!("{}", show(&err)))
        .get(0)
}

fn statements(sql: &str) -> Vec<String> {
    let bytes = sql.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut index = 0;
    let mut dollar: Option<Vec<u8>> = None;
    while index < bytes.len() {
        if let Some(tag) = &dollar {
            if bytes[index..].starts_with(tag) {
                index += tag.len();
                dollar = None;
                continue;
            }
            index += 1;
            continue;
        }
        if bytes[index] == b'$'
            && let Some(end) = bytes[index + 1..].iter().position(|byte| *byte == b'$')
        {
            let end = index + 1 + end;
            let inner = &bytes[index + 1..end];
            if inner.iter().all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_') {
                dollar = Some(bytes[index..=end].to_vec());
                index = end + 1;
                continue;
            }
        }
        if bytes[index] == b';' {
            push_statement(&mut out, &bytes[start..index]);
            start = index + 1;
        }
        index += 1;
    }
    push_statement(&mut out, &bytes[start..]);
    out
}

fn push_statement(out: &mut Vec<String>, bytes: &[u8]) {
    let text = std::str::from_utf8(bytes).unwrap().trim();
    if text.lines().any(|line| {
        let line = line.trim();
        !line.is_empty() && !line.starts_with("--")
    }) {
        out.push(text.to_owned());
    }
}

async fn apply_file(client: &mut Client, path: &Path, locked: bool, markers: &str) {
    let sql = fs::read_to_string(path).unwrap_or_else(|_| panic!("missing {}", path.display()));
    if !locked {
        for statement in statements(&sql) {
            client
                .batch_execute(&statement)
                .await
                .unwrap_or_else(|err| panic!("{} {}", path.display(), show(&err)));
        }
        return;
    }
    let tx = client.transaction().await.unwrap();
    tx.batch_execute(
        "SET LOCAL lock_timeout = '15s'; SET LOCAL statement_timeout = '60s'; \
         SET LOCAL ROLE oo_target_owner; SELECT pg_catalog.pg_advisory_xact_lock(6404202605)",
    )
    .await
    .unwrap_or_else(|err| panic!("lock {}", show(&err)));
    tx.batch_execute(&sql).await.unwrap_or_else(|err| panic!("{} {}", path.display(), show(&err)));
    if !markers.is_empty() {
        tx.batch_execute(markers).await.unwrap_or_else(|err| panic!("marker {}", show(&err)));
    }
    tx.commit().await.unwrap_or_else(|err| panic!("commit {}", show(&err)));
}
