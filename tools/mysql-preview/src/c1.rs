//! Owned MySQL 8 ROW/GTID reader.
//!
//! Capture, decode, assembly, resume, and the durable spool use `mysql_async`
//! plus the public `cdc-transaction` and `cdc-spool` crates. Apply and ACK stay
//! with the shared transaction entry. This command does not apply and does not ACK.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::future::poll_fn;
use std::io::{Cursor, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cdc_spool::{recover, Limits, OutcomeKind, Recovery, Spool};
use cdc_transaction::{encode_position, parse_commit_lsn, Assembler, FaultCode};
use futures_util::stream::Stream;
use mysql_async::binlog::events::{EventData, RowsEventData, TableMapEvent};
use mysql_async::binlog::row::BinlogRow;
use mysql_async::binlog::value::BinlogValue;
use mysql_async::prelude::Queryable;
use mysql_async::{BinlogStream, BinlogStreamRequest, Conn, OptsBuilder, Row, Sid, SslOpts, Value as MyValue};
use oo_pipeline::target_installer::sha256_hex;
use serde_json::{json, Value};
use tokio::time::timeout;

use crate::config::{read_ca, read_secret};
use crate::error::PreviewError;

const FIXTURE: &str = "owned-mysql-8";
const SOURCE: &str = "mysql_c1";
const DATABASE: &str = "oo_c1";
const HDR: &str = "oo_c1.order_hdr";
const LINE: &str = "oo_c1.order_line";
const NATIVE_TABLES: [&str; 5] = [
    "native_mysql_20261012.hubs",
    "native_mysql_20261012.range_gap",
    "native_mysql_20261012.shipments",
    "native_mysql_20261012.tracking_events",
    "native_mysql_20261012.value_probe",
];
const RETENTION: u64 = 604_800;
const MAX_EVENTS: u64 = 20_000;
const PROVENANCE: &str = "caller_declared";

pub async fn configure_dba(config: &Path, out: &Path) -> Result<(), PreviewError> {
    let cfg = load_config(config)?;
    refuse_customer(&cfg)?;
    fresh_dir(out)?;
    let dba_password = read_secret(&cfg.dba_password_file)?;
    let mut conn = connect(&cfg, &cfg.dba_user, dba_password.as_str()).await?;
    let version = scalar(&mut conn, "SELECT @@GLOBAL.version").await?;
    if !version.starts_with("8.") {
        return Err(PreviewError::new("version", "mysql_major"));
    }
    enable_gtid(&mut conn).await?;
    exec_static(&mut conn, "SET PERSIST binlog_expire_logs_seconds = 604800").await?;
    exec_static(&mut conn, "CREATE DATABASE IF NOT EXISTS oo_c1").await?;
    exec_static(
        &mut conn,
        "CREATE TABLE IF NOT EXISTS oo_c1.order_hdr (id BIGINT NOT NULL PRIMARY KEY, note VARCHAR(64) NOT NULL) ENGINE=InnoDB",
    )
    .await?;
    exec_static(
        &mut conn,
        "CREATE TABLE IF NOT EXISTS oo_c1.order_line (id BIGINT NOT NULL PRIMARY KEY, hdr_id BIGINT NOT NULL, qty BIGINT NOT NULL) ENGINE=InnoDB",
    )
    .await?;
    let password = ensure_password(&cfg.password_file)?;
    ensure_user(&mut conn, password.as_str()).await?;
    let vars = preflight_values(&mut conn).await?;
    let grants: Vec<Row> = conn
        .query("SHOW GRANTS FOR 'oo_cdc'@'%'")
        .await
        .map_err(|_| PreviewError::new("privilege", "replication"))?;
    let grant_text = rows_first(&grants)?;
    grants_ok(&grant_text)?;
    write_json(
        &out.join("dba.json"),
        &json!({
            "protocol": "oo.mysql.c1-dba/v1",
            "fixture": FIXTURE,
            "version": version,
            "variables": vars,
            "grants": grant_text,
            "password_len": password.len(),
            "database": DATABASE,
            "customer_rds_touched": false
        }),
    )
}

pub async fn prove_c1(config: &Path, out: &Path) -> Result<(), PreviewError> {
    let cfg = load_config(config)?;
    refuse_customer(&cfg)?;
    if cfg.reconcile_every_ms <= cfg.health_every_ms {
        return Err(PreviewError::new("config", "period"));
    }
    fresh_dir(out)?;
    let config_hash = sha256_hex(&fs::read(config).map_err(|_| PreviewError::new("config", "path"))?);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_reader = Arc::clone(&stop);
    let cfg_reader = cfg.clone();
    let out_reader = out.to_path_buf();
    let reader = tokio::spawn(async move {
        stream(&cfg_reader, &out_reader, None, Until::CaughtUp, &stop_reader).await
    });
    if timeout(Duration::from_secs(30), wait_ready(out)).await.is_err() {
        stop.store(true, Ordering::Relaxed);
        return match join(reader).await {
            Err(err) => Err(err),
            Ok(_) => Err(PreviewError::new("c1", "ready_timeout")),
        };
    }
    let mut snap = connect_cdc(&cfg).await?;
    let snapshot_rows = count_tables(&mut snap).await?;
    drop(snap);
    if snapshot_rows != (0, 0) {
        stop.store(true, Ordering::Relaxed);
        let _ = join(reader).await;
        return Err(PreviewError::new("c1", "snapshot_not_empty"));
    }
    dml_script(&cfg, out).await?;
    stop.store(true, Ordering::Relaxed);
    let phase = join(reader).await?;
    if phase.health_ticks < 2 || phase.reconcile_ticks != 1 || phase.table_reads != 2 {
        return Err(PreviewError::new("c1", "period"));
    }
    if phase.reconcile_diff != 0 {
        return Err(PreviewError::new("c1", "reconcile_diff"));
    }
    check_script(&phase.rows)?;
    let mut dba = connect_dba(&cfg).await?;
    exec_static(&mut dba, "INSERT INTO oo_c1.order_hdr VALUES (810004,'resume')").await?;
    drop(dba);
    let cursor = fs::read_to_string(out.join("cursor.json")).map_err(|_| PreviewError::new("c1", "cursor"))?;
    let cursor: Value = serde_json::from_str(&cursor).map_err(|_| PreviewError::new("c1", "cursor"))?;
    let gtid = cursor["gtid"].as_str().ok_or_else(|| PreviewError::new("c1", "cursor"))?.to_string();
    let stop2 = Arc::new(AtomicBool::new(true));
    let resumed = stream(&cfg, out, Some(gtid), Until::Key(HDR, "810004"), &stop2).await?;
    let prior: BTreeSet<_> = phase.rows.iter().map(|row| row.txn_id.clone()).collect();
    if resumed.rows.iter().any(|row| prior.contains(&row.txn_id)) {
        return Err(PreviewError::new("c1", "duplicate"));
    }
    let mut rows = phase.rows.clone();
    rows.extend(resumed.rows.iter().cloned());
    let mut gold = connect_cdc(&cfg).await?;
    let diff = gold_diff(&mut gold, &rows, &cfg).await?;
    drop(gold);
    let mut audit = connect_dba(&cfg).await?;
    let writes = write_attempts(&mut audit).await?;
    drop(audit);
    if diff != 0 || writes != 0 {
        return Err(PreviewError::new("c1", "gold"));
    }
    write_json(
        &out.join("prove.json"),
        &json!({
            "protocol": "oo.mysql.c1-prove/v1",
            "complete": false,
            "formal_signoff": "0/10",
            "target_applied": false,
            "ack_allowed": false,
            "llm_calls": 0,
            "ontology_version_changed": false,
            "pack_rebuilt": false,
            "customer_rds_touched": false,
            "c5_database_touched": false,
            "config_sha256": config_hash,
            "position_codec": "mysql-binlog-pos/v1",
            "health_every_ms": cfg.health_every_ms,
            "reconcile_every_ms": cfg.reconcile_every_ms,
            "health_ticks": phase.health_ticks,
            "reconcile_ticks": phase.reconcile_ticks,
            "health_table_reads": 0,
            "snapshot_table_reads": 2,
            "reconcile_table_reads": phase.table_reads,
            "reconcile_diff": phase.reconcile_diff,
            "source_gold_diff": diff,
            "cdc_write_attempts": writes,
            "cross_table_one_commit": true,
            "rollback_absent": true,
            "late_commit_order": true,
            "same_commit_lsn_rows": true,
            "resume_row": "810004",
            "resume_gtid": resumed.cursor_gtid,
            "loader": loader_gap()
        }),
    )
}

pub async fn expire_c1(config: &Path, from: &Path, out: &Path) -> Result<(), PreviewError> {
    let cfg = load_config(config)?;
    refuse_customer(&cfg)?;
    fresh_dir(out)?;
    preserved(&cfg.preserve_dir)?;
    let cursor_bytes = fs::read(from.join("cursor.json")).map_err(|_| PreviewError::new("c1", "cursor"))?;
    let cursor: Value = serde_json::from_slice(&cursor_bytes).map_err(|_| PreviewError::new("c1", "cursor"))?;
    let old_gtid = cursor["gtid"].as_str().ok_or_else(|| PreviewError::new("c1", "cursor"))?.to_string();
    let mut dba = connect_dba(&cfg).await?;
    exec_static(&mut dba, "INSERT INTO oo_c1.order_hdr VALUES (810099,'gap-expired')").await?;
    exec_static(&mut dba, "FLUSH BINARY LOGS").await?;
    let (file, _) = binary_status(&mut dba).await?;
    if !binlog_name_ok(&file) {
        return Err(PreviewError::new("c1", "binlog_name"));
    }
    preserved(&cfg.preserve_dir)?;
    let purge = format!("PURGE BINARY LOGS TO '{file}'");
    dba.query_drop(purge).await.map_err(|err| mysql_err(&err))?;
    drop(dba);
    let expired = match stream(&cfg, out, Some(old_gtid), Until::Key(HDR, "810099"), &Arc::new(AtomicBool::new(true))).await {
        Err(err) if err.fact == "position_expired" => true,
        Ok(_) => return Err(PreviewError::new("c1", "expiry_not_observed")),
        Err(err) => return Err(err),
    };
    let recovered = recover_spool(&from.join("spool"))?;
    let mut cdc = connect_cdc(&cfg).await?;
    let source = source_rows(&mut cdc).await?;
    drop(cdc);
    let gap_on_source = source.contains_key(&(HDR.to_string(), "810099".to_string()));
    let gap_in_spool = recovered.contains_key(&(HDR.to_string(), "810099".to_string()));
    let mut spool_diff = 0u64;
    for (key, payload) in &recovered {
        if source.get(key) != Some(payload) {
            spool_diff += 1;
        }
    }
    for (key, _) in &source {
        if key.1 == "810099" {
            continue;
        }
        if !recovered.contains_key(key) {
            spool_diff += 1;
        }
    }
    if !expired || !gap_on_source || gap_in_spool || spool_diff != 0 {
        return Err(PreviewError::new("c1", "expire"));
    }
    write_json(
        &out.join("expire.json"),
        &json!({
            "protocol": "oo.mysql.c1-expire/v1",
            "complete": false,
            "target_applied": false,
            "ack_allowed": false,
            "position_expired": true,
            "server_code": 1236,
            "gap_row": "810099",
            "gap_on_source": true,
            "gap_in_spool": false,
            "spooled_source_diff": 0,
            "recovered_keys": recovered.len(),
            "preserve_dir_nonempty": true,
            "loader": loader_gap()
        }),
    )
}

pub async fn capture_business(config: &Path, out: &Path) -> Result<(), PreviewError> {
    let cfg = load_config(config)?;
    refuse_customer(&cfg)?;
    if cfg.full_table_gold || !native_columns_match(&cfg.columns) {
        return Err(PreviewError::new("config", "tables"));
    }
    if cfg.reconcile_every_ms <= cfg.health_every_ms {
        return Err(PreviewError::new("config", "period"));
    }
    fresh_dir(out)?;
    let config_hash = sha256_hex(&fs::read(config).map_err(|_| PreviewError::new("config", "path"))?);
    let mut dba = connect_dba(&cfg).await?;
    ensure_native_select(&mut dba).await?;
    if scalar(&mut dba, NATIVE_ID_CHECK).await? != "0" {
        return Err(PreviewError::new("c1", "already_present"));
    }
    drop(dba);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_reader = Arc::clone(&stop);
    let cfg_reader = cfg.clone();
    let out_reader = out.to_path_buf();
    let reader = tokio::spawn(async move {
        stream(&cfg_reader, &out_reader, None, Until::CaughtUp, &stop_reader).await
    });
    if timeout(Duration::from_secs(30), wait_ready(out)).await.is_err() {
        stop.store(true, Ordering::Relaxed);
        return match join(reader).await {
            Err(err) => Err(err),
            Ok(_) => Err(PreviewError::new("c1", "ready_timeout")),
        };
    }
    if let Err(err) = native_dml(&cfg).await {
        stop.store(true, Ordering::Relaxed);
        let _ = join(reader).await;
        return Err(err);
    }
    stop.store(true, Ordering::Relaxed);
    let phase = join(reader).await?;
    if phase.health_ticks < 2 || phase.reconcile_ticks != 1 || phase.table_reads != cfg.tables.len() as u64 || phase.reconcile_diff != 0 {
        return Err(PreviewError::new("c1", "period"));
    }
    check_native(&phase.rows)?;
    let mut gold = connect_cdc(&cfg).await?;
    let diff = gold_diff(&mut gold, &phase.rows, &cfg).await?;
    drop(gold);
    let mut audit = connect_dba(&cfg).await?;
    let writes = write_attempts(&mut audit).await?;
    drop(audit);
    if diff != 0 || writes != 0 {
        return Err(PreviewError::new("c1", "gold"));
    }
    write_json(
        &out.join("native.json"),
        &json!({
            "protocol": "oo.mysql.c1-native/v1",
            "complete": false,
            "formal_signoff": "0/10",
            "target_applied": false,
            "ack_allowed": false,
            "llm_calls": 0,
            "ontology_version_changed": false,
            "pack_rebuilt": false,
            "customer_rds_touched": false,
            "oo_c1_dml": false,
            "purge_repeated": false,
            "historical_rows_in_spool": false,
            "tables": cfg.tables,
            "visible_columns": 43,
            "role": "oo_r_mysql_conn",
            "config_sha256": config_hash,
            "position_codec": "mysql-binlog-pos/v1",
            "health_ticks": phase.health_ticks,
            "reconcile_ticks": phase.reconcile_ticks,
            "reconcile_table_reads": phase.table_reads,
            "source_gold_diff": diff,
            "cdc_write_attempts": writes,
            "cross_table_one_commit": true,
            "new_keys": ["910101", "910111", "910121", "910131", "910141"],
            "cursor_gtid": phase.cursor_gtid,
            "loader": loader_gap()
        }),
    )
}

#[derive(Clone)]
struct C1Config {
    host: String,
    port: u16,
    user: String,
    password_file: PathBuf,
    dba_user: String,
    dba_password_file: PathBuf,
    ca_file: PathBuf,
    server_id: u32,
    health_every_ms: u64,
    reconcile_every_ms: u64,
    preserve_dir: PathBuf,
    fixture: String,
    tables: Vec<String>,
    columns: BTreeMap<String, Vec<String>>,
    full_table_gold: bool,
}

#[derive(Clone)]
struct Span {
    uuid: String,
    start: u64,
    end: u64,
}

struct Phase {
    rows: Vec<Seen>,
    health_ticks: u64,
    reconcile_ticks: u64,
    table_reads: u64,
    reconcile_diff: u64,
    cursor_gtid: String,
}

#[derive(Clone)]
struct Seen {
    txn_id: String,
    commit_order: u64,
    commit_lsn: String,
    table: String,
    key: String,
    payload: Vec<String>,
    sequence: u64,
    deleted: bool,
}

enum Until {
    CaughtUp,
    Key(&'static str, &'static str),
}

struct Live {
    assembler: Assembler,
    span: Span,
    file: String,
    epoch: u32,
    bind: String,
    schema_hash: String,
    type_hash: String,
    pk: BTreeMap<String, Vec<usize>>,
    selected: BTreeSet<String>,
    visible: BTreeMap<String, Vec<usize>>,
    pending: Option<String>,
    buf: Vec<String>,
    counts: BTreeMap<String, u64>,
    globals: u64,
    rows: Vec<Seen>,
    events: u64,
    foreign: bool,
    out: PathBuf,
}

fn load_config(path: &Path) -> Result<C1Config, PreviewError> {
    let bytes = fs::read(path).map_err(|_| PreviewError::new("config", "path"))?;
    let text = std::str::from_utf8(&bytes).map_err(|_| PreviewError::new("config", "json"))?;
    let value: Value = serde_json::from_str(text).map_err(|_| PreviewError::new("config", "json"))?;
    let map = value.as_object().ok_or_else(|| PreviewError::new("config", "json"))?;
    let tables = map.get("tables").and_then(|item| item.as_array()).ok_or_else(|| PreviewError::new("config", "tables"))?;
    let mut names: Vec<&str> = tables.iter().filter_map(|item| item.as_str()).collect();
    names.sort_unstable();
    let columns = parse_columns(map)?;
    let full_table_gold = if names == [HDR, LINE] {
        if !columns.is_empty() {
            return Err(PreviewError::new("config", "columns"));
        }
        true
    } else if names == NATIVE_TABLES {
        if !native_columns_match(&columns) {
            return Err(PreviewError::new("config", "columns"));
        }
        false
    } else {
        return Err(PreviewError::new("config", "tables"));
    };
    Ok(C1Config {
        host: text_field(map, "host")?,
        port: map.get("port").and_then(|item| item.as_u64()).and_then(|item| u16::try_from(item).ok()).ok_or_else(|| PreviewError::new("config", "port"))?,
        user: text_field(map, "user")?,
        password_file: path_field(map, "password_file")?,
        dba_user: text_field(map, "dba_user")?,
        dba_password_file: path_field(map, "dba_password_file")?,
        ca_file: path_field(map, "ca_file")?,
        server_id: map.get("server_id").and_then(|item| item.as_u64()).and_then(|item| u32::try_from(item).ok()).filter(|item| *item > 1).ok_or_else(|| PreviewError::new("config", "server_id"))?,
        health_every_ms: map.get("health_every_ms").and_then(|item| item.as_u64()).filter(|item| *item >= 100).ok_or_else(|| PreviewError::new("config", "health_every_ms"))?,
        reconcile_every_ms: map.get("reconcile_every_ms").and_then(|item| item.as_u64()).filter(|item| *item >= 200).ok_or_else(|| PreviewError::new("config", "reconcile_every_ms"))?,
        preserve_dir: path_field(map, "preserve_dir")?,
        fixture: text_field(map, "fixture")?,
        tables: names.into_iter().map(str::to_string).collect(),
        columns,
        full_table_gold,
    })
}

fn text_field(map: &serde_json::Map<String, Value>, key: &str) -> Result<String, PreviewError> {
    map.get(key).and_then(|item| item.as_str()).filter(|item| !item.is_empty()).map(str::to_string).ok_or_else(|| PreviewError::new("config", "json"))
}

fn path_field(map: &serde_json::Map<String, Value>, key: &str) -> Result<PathBuf, PreviewError> {
    Ok(PathBuf::from(text_field(map, key)?))
}

fn refuse_customer(cfg: &C1Config) -> Result<(), PreviewError> {
    let host = cfg.host.to_ascii_lowercase();
    if cfg.fixture != FIXTURE || host.contains("rds") || host.contains("amazonaws") || cfg.user != "oo_cdc" {
        return Err(PreviewError::new("config", "fixture"));
    }
    Ok(())
}

fn fresh_dir(out: &Path) -> Result<(), PreviewError> {
    if out.exists() {
        return Err(PreviewError::new("config", "out_exists"));
    }
    fs::create_dir_all(out).map_err(|_| PreviewError::new("config", "out"))?;
    fs::create_dir(out.join("spool")).map_err(|_| PreviewError::new("config", "out"))?;
    Ok(())
}

fn preserved(dir: &Path) -> Result<(), PreviewError> {
    let entries = fs::read_dir(dir).map_err(|_| PreviewError::new("c1", "preserve_missing"))?;
    let mut found = false;
    for entry in entries {
        let entry = entry.map_err(|_| PreviewError::new("c1", "preserve_missing"))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("binlog.") && entry.metadata().map(|meta| meta.len() > 0).unwrap_or(false) {
            found = true;
        }
    }
    if found {
        Ok(())
    } else {
        Err(PreviewError::new("c1", "preserve_missing"))
    }
}

async fn connect(cfg: &C1Config, user: &str, password: &str) -> Result<Conn, PreviewError> {
    let _ca = read_ca(&cfg.ca_file)?;
    let ssl = SslOpts::default()
        .with_root_certs(vec![cfg.ca_file.clone().into()])
        .with_danger_accept_invalid_certs(false)
        .with_danger_skip_domain_validation(false)
        .with_disable_built_in_roots(true);
    let opts = OptsBuilder::default()
        .ip_or_hostname(cfg.host.clone())
        .tcp_port(cfg.port)
        .user(Some(user.to_string()))
        .pass(Some(password.to_string()))
        .db_name(Some(DATABASE.to_string()))
        .prefer_socket(false)
        .ssl_opts(ssl);
    let conn = timeout(Duration::from_secs(10), Conn::new(opts))
        .await
        .map_err(|_| PreviewError::new("connect", "connect_timeout"))?
        .map_err(|_| PreviewError::new("connect", "connect"))?;
    Ok(conn)
}

async fn connect_cdc(cfg: &C1Config) -> Result<Conn, PreviewError> {
    let password = read_secret(&cfg.password_file)?;
    connect(cfg, &cfg.user, password.as_str()).await
}

async fn connect_dba(cfg: &C1Config) -> Result<Conn, PreviewError> {
    let password = read_secret(&cfg.dba_password_file)?;
    connect(cfg, &cfg.dba_user, password.as_str()).await
}

async fn scalar(conn: &mut Conn, sql: &str) -> Result<String, PreviewError> {
    let row: Option<Row> = conn.query_first(sql).await.map_err(|_| PreviewError::new("protocol", "query"))?;
    let row = row.ok_or_else(|| PreviewError::new("protocol", "query"))?;
    value_string(&row_value(&row, 0)?)
}

fn row_value(row: &Row, index: usize) -> Result<MyValue, PreviewError> {
    row.get(index).ok_or_else(|| PreviewError::new("protocol", "cell"))
}

fn value_string(value: &MyValue) -> Result<String, PreviewError> {
    match value {
        MyValue::Bytes(bytes) => String::from_utf8(bytes.clone()).map_err(|_| PreviewError::new("value", "text")),
        MyValue::Int(item) => Ok(item.to_string()),
        MyValue::UInt(item) => Ok(item.to_string()),
        MyValue::NULL => Err(PreviewError::new("protocol", "cell")),
        _ => Err(PreviewError::new("value", "text")),
    }
}

fn rows_first(rows: &[Row]) -> Result<Vec<String>, PreviewError> {
    rows.iter().map(|row| value_string(&row_value(row, 0)?)).collect()
}

async fn exec_static(conn: &mut Conn, sql: &'static str) -> Result<(), PreviewError> {
    conn.query_drop(sql).await.map_err(|err| mysql_err(&err))
}

fn mysql_err(err: &mysql_async::Error) -> PreviewError {
    match err {
        mysql_async::Error::Server(server) if server.code == 1236 => PreviewError::new("c1", "position_expired"),
        _ => PreviewError::new("protocol", "query"),
    }
}

async fn enable_gtid(conn: &mut Conn) -> Result<(), PreviewError> {
    let mode = scalar(conn, "SELECT @@GLOBAL.gtid_mode").await?;
    if mode == "ON" {
        return Ok(());
    }
    if mode == "OFF" {
        exec_static(conn, "SET PERSIST enforce_gtid_consistency = ON").await?;
        exec_static(conn, "SET GLOBAL gtid_mode = OFF_PERMISSIVE").await?;
        exec_static(conn, "SET GLOBAL gtid_mode = ON_PERMISSIVE").await?;
    } else if mode == "OFF_PERMISSIVE" {
        exec_static(conn, "SET PERSIST enforce_gtid_consistency = ON").await?;
        exec_static(conn, "SET GLOBAL gtid_mode = ON_PERMISSIVE").await?;
    }
    for _ in 0..30 {
        let pending = scalar(conn, "SHOW STATUS LIKE 'Ongoing_anonymous_transaction_count'").await.unwrap_or_else(|_| "0".to_string());
        if pending == "0" || pending.ends_with('\t') {
            break;
        }
        let row: Option<Row> = conn
            .query_first("SHOW STATUS LIKE 'Ongoing_anonymous_transaction_count'")
            .await
            .map_err(|_| PreviewError::new("c1", "gtid_mode"))?;
        let count = row.as_ref().and_then(|item| value_string(&row_value(item, 1).ok()?).ok()).unwrap_or_else(|| "0".to_string());
        if count == "0" {
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    exec_static(conn, "SET GLOBAL gtid_mode = ON").await?;
    exec_static(conn, "SET PERSIST gtid_mode = ON").await?;
    Ok(())
}

fn ensure_password(path: &Path) -> Result<zeroize::Zeroizing<String>, PreviewError> {
    if path.exists() {
        return read_secret(path);
    }
    let mut bytes = [0u8; 24];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|_| PreviewError::new("config", "urandom"))?;
    let text: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut file| {
            use std::io::Write;
            file.write_all(text.as_bytes())
        })
        .map_err(|_| PreviewError::new("config", "password_file"))?;
    Ok(zeroize::Zeroizing::new(text))
}

async fn ensure_user(conn: &mut Conn, password: &str) -> Result<(), PreviewError> {
    if password.len() < 32 || !password.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(PreviewError::new("config", "password_file"));
    }
    let existing: Option<Row> = conn
        .query_first("SELECT user FROM mysql.user WHERE user = 'oo_cdc' AND host = '%'")
        .await
        .map_err(|_| PreviewError::new("privilege", "cdc_user"))?;
    if existing.is_none() {
        let sql = format!("CREATE USER 'oo_cdc'@'%' IDENTIFIED BY '{password}'");
        conn.query_drop(sql).await.map_err(|_| PreviewError::new("privilege", "cdc_user"))?;
    }
    exec_static(conn, "GRANT REPLICATION SLAVE, REPLICATION CLIENT ON *.* TO 'oo_cdc'@'%'").await?;
    exec_static(conn, "GRANT SELECT ON oo_c1.* TO 'oo_cdc'@'%'").await?;
    Ok(())
}

async fn preflight_values(conn: &mut Conn) -> Result<Value, PreviewError> {
    let version = scalar(conn, "SELECT @@GLOBAL.version").await?;
    let gtid = scalar(conn, "SELECT @@GLOBAL.gtid_mode").await?;
    let consistent = scalar(conn, "SELECT @@GLOBAL.enforce_gtid_consistency").await?;
    let format = scalar(conn, "SELECT @@GLOBAL.binlog_format").await?;
    let image = scalar(conn, "SELECT @@GLOBAL.binlog_row_image").await?;
    let expire = scalar(conn, "SELECT @@GLOBAL.binlog_expire_logs_seconds").await?;
    let log_bin = scalar(conn, "SELECT @@GLOBAL.log_bin").await?;
    let server_id = scalar(conn, "SELECT @@GLOBAL.server_id").await?;
    if !version.starts_with("8.") || gtid != "ON" || consistent != "ON" || format != "ROW" || image != "FULL" || log_bin != "1" {
        return Err(PreviewError::new("c1", "dba_required"));
    }
    let expire_secs: u64 = expire.parse().map_err(|_| PreviewError::new("c1", "dba_required"))?;
    if expire_secs < RETENTION {
        return Err(PreviewError::new("c1", "dba_required"));
    }
    Ok(json!({
        "version": version,
        "gtid_mode": gtid,
        "enforce_gtid_consistency": consistent,
        "binlog_format": format,
        "binlog_row_image": image,
        "binlog_expire_logs_seconds": expire_secs,
        "log_bin": log_bin,
        "server_id": server_id
    }))
}

fn grants_ok(grants: &[String]) -> Result<(), PreviewError> {
    let text = grants.join("\n").to_ascii_uppercase();
    let needed = ["REPLICATION SLAVE", "REPLICATION CLIENT", "SELECT"];
    if needed.iter().any(|item| !text.contains(item)) || !text.contains("OO_C1") {
        return Err(PreviewError::new("privilege", "replication"));
    }
    for denied in ["INSERT", "UPDATE", "DELETE", "DROP", "ALTER", "CREATE", "SUPER", "ALL PRIVILEGES", "FILE"] {
        if text.contains(denied) {
            return Err(PreviewError::new("privilege", "write_grant"));
        }
    }
    Ok(())
}

async fn wait_ready(out: &Path) -> Result<(), PreviewError> {
    loop {
        if out.join("ready").exists() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn join(reader: tokio::task::JoinHandle<Result<Phase, PreviewError>>) -> Result<Phase, PreviewError> {
    match reader.await {
        Ok(result) => result,
        Err(_) => Err(PreviewError::new("c1", "join")),
    }
}

async fn dml_script(cfg: &C1Config, out: &Path) -> Result<(), PreviewError> {
    let mut a = connect_dba(cfg).await?;
    let mut b = connect_dba(cfg).await?;
    exec_static(&mut a, "START TRANSACTION").await?;
    exec_static(&mut a, "INSERT INTO oo_c1.order_hdr VALUES (810001,'cross')").await?;
    exec_static(&mut a, "INSERT INTO oo_c1.order_line VALUES (810011,810001,2)").await?;
    exec_static(&mut a, "COMMIT").await?;
    exec_static(&mut a, "START TRANSACTION").await?;
    exec_static(&mut a, "INSERT INTO oo_c1.order_hdr VALUES (810002,'rolled')").await?;
    exec_static(&mut a, "ROLLBACK").await?;
    exec_static(&mut a, "START TRANSACTION").await?;
    exec_static(&mut a, "INSERT INTO oo_c1.order_hdr VALUES (810003,'late')").await?;
    exec_static(&mut b, "START TRANSACTION").await?;
    exec_static(&mut b, "INSERT INTO oo_c1.order_line VALUES (810012,810003,1)").await?;
    exec_static(&mut b, "COMMIT").await?;
    exec_static(&mut a, "COMMIT").await?;
    exec_static(&mut a, "INSERT INTO oo_c1.order_line VALUES (810013,810001,1),(810014,810001,4)").await?;
    let _ = out;
    Ok(())
}

async fn stream(
    cfg: &C1Config,
    out: &Path,
    resume: Option<String>,
    until: Until,
    stop: &AtomicBool,
) -> Result<Phase, PreviewError> {
    let mut probe = connect_cdc(cfg).await?;
    let vars = preflight_values(&mut probe).await?;
    let source_server: u32 = vars["server_id"].as_str().and_then(|item| item.parse().ok()).ok_or_else(|| PreviewError::new("c1", "dba_required"))?;
    if source_server == cfg.server_id {
        return Err(PreviewError::new("config", "server_id"));
    }
    let grants = rows_first(
        &probe
            .query::<Row, _>("SHOW GRANTS FOR CURRENT_USER()")
            .await
            .map_err(|_| PreviewError::new("privilege", "replication"))?,
    )?;
    grants_ok(&grants)?;
    let (pk, names, catalog) = load_pk(&mut probe, &cfg.tables).await?;
    let visible = visible_ordinals(cfg, &names)?;
    let (file, _pos, executed) = full_status(&mut probe).await?;
    let span = match resume {
        Some(gtid) => parse_span(&gtid)?,
        None => parse_span(&executed)?,
    };
    let uuid = scalar(&mut probe, "SELECT @@GLOBAL.server_uuid").await?;
    if span.uuid != uuid && !span.uuid.is_empty() {
        return Err(PreviewError::new("c1", "gtid"));
    }
    let lineage = sha256_hex(uuid.as_bytes());
    let type_hash = sha256_hex(catalog.as_bytes());
    let schema_hash = sha256_hex(cfg.tables.join("\n").as_bytes());
    drop(probe);
    let mut live = Live {
        assembler: Assembler::new(),
        span: Span { uuid: if span.uuid.is_empty() { uuid.clone() } else { span.uuid.clone() }, start: span.start, end: span.end },
        file,
        epoch: 1,
        bind: String::new(),
        schema_hash,
        type_hash,
        pk,
        selected: cfg.tables.iter().cloned().collect(),
        visible,
        pending: None,
        buf: Vec::new(),
        counts: BTreeMap::new(),
        globals: 0,
        rows: Vec::new(),
        events: 0,
        foreign: false,
        out: out.to_path_buf(),
    };
    live.bind = bind_line(&lineage, &live.schema_hash, &live.type_hash, &schema_hash_of_config(&lineage), &cfg.tables);
    live.feed(&live.bind.clone())?;
    if !out.join("l0.json").exists() {
        write_json(
            &out.join("l0.json"),
            &json!({
                "protocol": "oo.mysql.c1-l0/v1",
                "gtid": display_span(&live.span),
                "file": live.file,
                "recorded_before_snapshot": true,
                "applied": false,
                "ack": false
            }),
        )?;
    }
    let gtid = display_span(&live.span);
    let sid = Sid::from_str(&gtid).map_err(|_| PreviewError::new("c1", "gtid"))?;
    let password = read_secret(&cfg.password_file)?;
    let conn = connect(cfg, &cfg.user, password.as_str()).await?;
    let request = BinlogStreamRequest::new(cfg.server_id).with_gtid().with_gtid_set([sid]);
    let stream = conn.get_binlog_stream(request).await.map_err(|err| mysql_err(&err))?;
    drop(password);
    fs::write(out.join("ready"), b"1").map_err(|_| PreviewError::new("config", "out"))?;
    let mut health = connect_cdc(cfg).await?;
    let mut reconcile = connect_cdc(cfg).await?;
    let mut stream: Pin<Box<BinlogStream>> = Box::pin(stream);
    let health_every = Duration::from_millis(cfg.health_every_ms);
    let reconcile_every = Duration::from_millis(cfg.reconcile_every_ms);
    let mut health_ticks = 0u64;
    let mut reconcile_ticks = 0u64;
    let mut table_reads = 0u64;
    let mut reconcile_diff = 0u64;
    let mut caught_up: Option<tokio::time::Instant> = None;
    let mut health_since = 0u64;
    let started = tokio::time::Instant::now();
    let mut next_health = started + health_every;
    loop {
        if started.elapsed() > Duration::from_secs(90) {
            return Err(PreviewError::new("budget", "statement_timeout"));
        }
        tokio::select! {
            biased;
            polled = poll_fn(|cx| stream.as_mut().poll_next(cx)) => {
                match polled {
                    Some(Ok(event)) => {
                        if live.on_event(event, stream.as_ref().get_ref())? {
                            caught_up = None;
                            health_since = 0;
                        }
                    }
                    Some(Err(err)) => return Err(mysql_err(&err)),
                    None => return Err(PreviewError::new("c1", "stream_end")),
                }
            }
            _ = tokio::time::sleep_until(next_health) => {
                next_health = tokio::time::Instant::now() + health_every;
                health_ticks += 1;
                let _ = scalar(&mut health, "SELECT @@GLOBAL.gtid_executed").await?;
                let idle = live.pending.is_none() && live.buf.is_empty();
                if stop.load(Ordering::Relaxed) && idle {
                    if caught_up.is_none() {
                        caught_up = Some(tokio::time::Instant::now());
                    }
                    health_since += 1;
                    let waited = caught_up.ok_or_else(|| PreviewError::new("c1", "period"))?.elapsed();
                    match until {
                        Until::CaughtUp if health_since >= 2 && waited >= reconcile_every && reconcile_ticks == 0 => {
                            reconcile_diff = gold_diff(&mut reconcile, &live.rows, cfg).await?;
                            table_reads += cfg.tables.len() as u64;
                            reconcile_ticks += 1;
                            break;
                        }
                        Until::Key(table, key) if health_since >= 1 && live.rows.iter().any(|row| row.table == table && row.key == key && !row.deleted) => {
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    Ok(Phase {
        rows: live.rows,
        health_ticks,
        reconcile_ticks,
        table_reads,
        reconcile_diff,
        cursor_gtid: display_span(&live.span),
    })
}

fn schema_hash_of_config(lineage: &str) -> String {
    sha256_hex(lineage.as_bytes())
}

fn bind_line(lineage: &str, schema_hash: &str, type_hash: &str, config_hash: &str, tables: &[String]) -> String {
    json!({
        "kind": "bind",
        "source": SOURCE,
        "lineage": lineage,
        "epoch": 1,
        "selected_tables": tables,
        "schema_hash": schema_hash,
        "config_hash": config_hash,
        "type_hash": type_hash,
        "hook_hashes": [],
        "commit_position_provenance": PROVENANCE
    })
    .to_string()
}

impl Live {
    fn feed(&mut self, line: &str) -> Result<Option<cdc_transaction::CompletedTxn>, PreviewError> {
        let out = self.assembler.apply_line(line).map_err(|fault| fault_error(fault.code))?;
        self.assembler.note_newline().map_err(|fault| fault_error(fault.code))?;
        Ok(out.completed)
    }

    fn on_event(&mut self, event: mysql_async::binlog::events::Event, stream: &BinlogStream) -> Result<bool, PreviewError> {
        self.events += 1;
        if self.events > MAX_EVENTS {
            return Err(PreviewError::new("budget", "max_rows"));
        }
        let header_pos = event.header().log_pos();
        let header_ts = event.header().timestamp();
        let data = event.read_data().map_err(|_| PreviewError::new("protocol", "event"))?;
        let Some(data) = data else { return Ok(false) };
        match data {
            EventData::RotateEvent(rotate) => {
                let name = rotate.name().to_string();
                if !name.is_empty() {
                    self.file = name;
                }
                Ok(false)
            }
            EventData::GtidEvent(gtid) => {
                self.on_gtid(gtid.sid(), gtid.gno())?;
                Ok(false)
            }
            EventData::QueryEvent(query) => {
                self.on_query(&query.query())?;
                Ok(false)
            }
            EventData::RowsEvent(rows) => {
                if matches!(rows, RowsEventData::PartialUpdateRowsEvent(_)) {
                    return Err(PreviewError::new("c1", "partial_image"));
                }
                let tme = stream.get_tme(rows.table_id()).ok_or_else(|| PreviewError::new("c1", "table_map"))?;
                self.on_rows(&rows, tme)
            }
            EventData::XidEvent(_) => self.on_xid(header_pos, header_ts),
            EventData::AnonymousGtidEvent(_) => Err(PreviewError::new("c1", "anonymous_gtid")),
            _ => Ok(false),
        }
    }

    fn on_gtid(&mut self, sid: [u8; 16], gno: u64) -> Result<(), PreviewError> {
        if self.pending.is_some() || !self.buf.is_empty() {
            return Err(PreviewError::new("c1", "unclosed_txn"));
        }
        let uuid = format_uuid(&sid);
        if uuid != self.span.uuid {
            return Err(PreviewError::new("c1", "lineage"));
        }
        if gno != self.span.end.saturating_add(1) {
            return Err(PreviewError::new("c1", "gtid_gap"));
        }
        self.pending = Some(format!("{uuid}:{gno}"));
        self.buf.clear();
        self.counts.clear();
        self.globals = 0;
        self.foreign = false;
        Ok(())
    }

    fn on_query(&mut self, query: &str) -> Result<(), PreviewError> {
        let verb = query.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
        if matches!(verb.as_str(), "BEGIN" | "START" | "COMMIT" | "XA") {
            return Ok(());
        }
        if verb == "ROLLBACK" {
            self.pending = None;
            self.buf.clear();
            self.foreign = false;
            self.rows.retain(|row| row.commit_order != 0);
            return Ok(());
        }
        if self.buf.is_empty() {
            return Ok(());
        }
        if matches!(verb.as_str(), "ALTER" | "CREATE" | "DROP" | "TRUNCATE" | "RENAME") {
            return Err(PreviewError::new("c1", "schema_pause"));
        }
        if matches!(verb.as_str(), "INSERT" | "UPDATE" | "DELETE") {
            return Err(PreviewError::new("c1", "statement_dml"));
        }
        Ok(())
    }

    fn on_rows(&mut self, rows: &RowsEventData<'_>, tme: &TableMapEvent<'_>) -> Result<bool, PreviewError> {
        let qualified = format!("{}.{}", tme.database_name(), tme.table_name());
        if !self.selected.contains(&qualified) {
            if self.pending.is_some() {
                self.foreign = true;
            }
            return Ok(false);
        }
        let Some(txn) = self.pending.clone() else {
            return Err(PreviewError::new("c1", "rows_outside_txn"));
        };
        let pk = self.pk.get(&qualified).cloned().ok_or_else(|| PreviewError::new("catalog", "primary_key"))?;
        let ordinals = self.visible.get(&qualified).cloned().ok_or_else(|| PreviewError::new("catalog", "columns"))?;
        let mut hit = false;
        for pair in rows.rows(tme) {
            let (before, after) = pair.map_err(|_| PreviewError::new("protocol", "event"))?;
            let (op, image, deleted) = match (before, after) {
                (None, Some(after)) => ("upsert", after, false),
                (Some(_before), Some(after)) => ("upsert", after, false),
                (Some(before), None) => ("delete", before, true),
                (None, None) => return Err(PreviewError::new("protocol", "event")),
            };
            let key_cells = row_cells(&image, &pk)?;
            let payload_cells = row_cells(&image, &ordinals)?;
            if key_cells.iter().chain(payload_cells.iter()).any(json_null) {
                return Err(PreviewError::new("value", "null"));
            }
            self.globals += 1;
            let collection = self.counts.entry(qualified.clone()).or_default();
            *collection += 1;
            let sequence = self.globals;
            let collection_sequence = *collection;
            let key = json_cell_text(key_cells.first().ok_or_else(|| PreviewError::new("value", "key"))?)?;
            let payload = payload_cells.iter().map(json_cell_text).collect::<Result<Vec<_>, _>>()?;
            self.buf.push(json!({
                "kind": "data",
                "txn_id": txn,
                "global_txn_sequence": sequence,
                "data_collection_sequence": collection_sequence,
                "table": qualified,
                "key": key_cells,
                "op": op,
                "payload": payload_cells,
                "unavailable_ordinals": []
            }).to_string());
            self.rows.push(Seen {
                txn_id: txn.clone(),
                commit_order: 0,
                commit_lsn: String::new(),
                table: qualified.clone(),
                key,
                payload,
                sequence,
                deleted,
            });
            hit = true;
        }
        Ok(hit)
    }

    fn on_xid(&mut self, pos: u32, timestamp: u32) -> Result<bool, PreviewError> {
        let Some(txn) = self.pending.clone() else { return Ok(false) };
        if self.foreign && !self.buf.is_empty() {
            return Err(PreviewError::new("c1", "mixed_txn"));
        }
        if self.buf.is_empty() {
            self.advance(&txn)?;
            self.pending = None;
            self.foreign = false;
            return Ok(false);
        }
        let lsn = commit_lsn(&self.file, pos)?;
        let position = encode_position(self.epoch, parse_commit_lsn(&lsn.0).map_err(|_| PreviewError::new("c1", "commit_lsn"))?).map_err(|_| PreviewError::new("c1", "commit_lsn"))?;
        let stamp = unix_text(timestamp).ok_or_else(|| PreviewError::new("c1", "timestamp"))?;
        let begin = json!({"kind": "begin", "txn_id": txn}).to_string();
        let end = json!({
            "kind": "end",
            "txn_id": txn,
            "count": self.globals,
            "per_collection_counts": self.counts,
            "commit_lsn": lsn.0,
            "source_commit_timestamp": stamp
        })
        .to_string();
        let mut lines = Vec::new();
        lines.push(self.bind.clone());
        lines.push(begin.clone());
        self.feed(&begin)?;
        let buffered = std::mem::take(&mut self.buf);
        for line in &buffered {
            lines.push(line.clone());
            self.feed(line)?;
        }
        let completed = self.feed(&end)?.ok_or_else(|| PreviewError::new("c1", "assembler"))?;
        lines.push(end);
        seal_spool(&self.out.join("spool").join(format!("{:06}", gno_of(&txn)?)), &lines)?;
        for row in &mut self.rows {
            if row.txn_id == txn {
                row.commit_order = completed.commit_order;
                row.commit_lsn = completed.commit_lsn.clone();
            }
        }
        let _ = position;
        self.advance(&txn)?;
        self.pending = None;
        self.buf.clear();
        self.foreign = false;
        Ok(true)
    }

    fn advance(&mut self, txn: &str) -> Result<(), PreviewError> {
        let gno = gno_of(txn)?;
        if self.span.start == 0 {
            self.span.start = gno;
        }
        self.span.end = gno;
        write_json(
            &self.out.join("cursor.json"),
            &json!({
                "protocol": "oo.mysql.c1-cursor/v1",
                "applied": false,
                "ack": false,
                "gtid": display_span(&self.span),
                "file": self.file,
                "position_codec": "mysql-binlog-pos/v1",
                "epoch": self.epoch
            }),
        )
    }
}

fn gno_of(txn: &str) -> Result<u64, PreviewError> {
    txn.rsplit_once(':').and_then(|(_, gno)| gno.parse().ok()).ok_or_else(|| PreviewError::new("c1", "gtid"))
}

fn format_uuid(sid: &[u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(36);
    for (index, byte) in sid.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn commit_lsn(file: &str, pos: u32) -> Result<(String, String), PreviewError> {
    let seq = file.rsplit_once('.').and_then(|(_, rest)| rest.parse::<u64>().ok()).ok_or_else(|| PreviewError::new("c1", "binlog_name"))?;
    if seq > u64::from(u32::MAX) || pos == 0 {
        return Err(PreviewError::new("c1", "commit_lsn"));
    }
    let lsn = format!("{seq:X}/{pos:X}");
    let value = parse_commit_lsn(&lsn).map_err(|_| PreviewError::new("c1", "commit_lsn"))?;
    let position = encode_position(1, value).map_err(|_| PreviewError::new("c1", "commit_lsn"))?;
    Ok((lsn, position))
}

fn unix_text(secs: u32) -> Option<String> {
    if secs == 0 {
        return None;
    }
    let (year, month, day, hour, minute, second) = civil_from_unix(secs);
    if !(1..=9999).contains(&year) {
        return None;
    }
    Some(format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"))
}

fn civil_from_unix(secs: u32) -> (i32, u32, u32, u32, u32, u32) {
    let days = i64::from(secs / 86_400);
    let rem = secs % 86_400;
    let hour = rem / 3_600;
    let minute = (rem % 3_600) / 60;
    let second = rem % 60;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + if month <= 2 { 1 } else { 0 };
    (year as i32, month as u32, day as u32, hour, minute, second)
}

fn parse_span(text: &str) -> Result<Span, PreviewError> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(Span { uuid: String::new(), start: 0, end: 0 });
    }
    if text.contains(',') || text.contains('\n') {
        return Err(PreviewError::new("c1", "gtid"));
    }
    let (uuid, rest) = text.split_once(':').ok_or_else(|| PreviewError::new("c1", "gtid"))?;
    let (start, end) = if let Some((left, right)) = rest.split_once('-') {
        (left.parse::<u64>().map_err(|_| PreviewError::new("c1", "gtid"))?, right.parse::<u64>().map_err(|_| PreviewError::new("c1", "gtid"))?)
    } else {
        let one = rest.parse::<u64>().map_err(|_| PreviewError::new("c1", "gtid"))?;
        (one, one)
    };
    if start == 0 || end < start {
        return Err(PreviewError::new("c1", "gtid"));
    }
    Ok(Span { uuid: uuid.to_string(), start, end })
}

fn display_span(span: &Span) -> String {
    if span.end == 0 {
        return String::new();
    }
    if span.start == span.end {
        format!("{}:{}", span.uuid, span.end)
    } else {
        format!("{}:{}-{}", span.uuid, span.start, span.end)
    }
}

fn fault_error(code: FaultCode) -> PreviewError {
    let fact = match code {
        FaultCode::BindInvalid => "bind_invalid",
        FaultCode::TimestampInvalid => "timestamp",
        FaultCode::CommitLsnInvalid => "commit_lsn",
        FaultCode::SequenceGap | FaultCode::SequenceOutOfRange | FaultCode::SequenceInvalid => "sequence_gap",
        FaultCode::CountMismatch | FaultCode::CollectionCountGap => "count_mismatch",
        FaultCode::KeyInvalid => "key",
        FaultCode::PayloadInvalid | FaultCode::OrdinalMismatch | FaultCode::OrdinalOrder => "payload",
        FaultCode::TxnInvalid => "gtid",
        FaultCode::Malformed => "malformed",
        _ => "assembler",
    };
    PreviewError::new("c1", fact)
}

fn seal_spool(dir: &Path, lines: &[String]) -> Result<(), PreviewError> {
    let mut body = Vec::new();
    for line in lines {
        body.extend_from_slice(line.as_bytes());
        body.push(b'\n');
    }
    let mut spool = Spool::create_new(dir, Limits::product()).map_err(|_| PreviewError::new("c1", "spool"))?;
    let outcome = spool.ingest_reader(Cursor::new(body)).map_err(|_| PreviewError::new("c1", "spool"))?;
    if !matches!(outcome.kind, OutcomeKind::Sealed(_) | OutcomeKind::RolledBack { .. }) {
        return Err(PreviewError::new("c1", "spool"));
    }
    if outcome.progress.ack_allowed || outcome.progress.target_applied {
        return Err(PreviewError::new("c1", "ack"));
    }
    Ok(())
}

async fn load_pk(conn: &mut Conn, tables: &[String]) -> Result<(BTreeMap<String, Vec<usize>>, BTreeMap<String, Vec<(usize, String)>>, String), PreviewError> {
    let (schema, names) = one_schema(tables)?;
    let sql = format!(
        "SELECT TABLE_NAME, ORDINAL_POSITION, COLUMN_NAME, COLUMN_KEY FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = '{schema}' AND TABLE_NAME IN ({}) ORDER BY TABLE_NAME, ORDINAL_POSITION",
        names.iter().map(|name| format!("'{name}'")).collect::<Vec<_>>().join(",")
    );
    let rows: Vec<Row> = conn.query(sql).await.map_err(|_| PreviewError::new("catalog", "columns"))?;
    let mut pk: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut found: BTreeMap<String, Vec<(usize, String)>> = BTreeMap::new();
    let mut catalog = String::new();
    for row in rows {
        let table = value_string(&row_value(&row, 0)?)?;
        let ordinal: u64 = value_string(&row_value(&row, 1)?)?.parse().map_err(|_| PreviewError::new("catalog", "columns"))?;
        let column = value_string(&row_value(&row, 2)?)?;
        let key = value_string(&row_value(&row, 3)?)?;
        catalog.push_str(&table);
        catalog.push('\t');
        catalog.push_str(&column);
        catalog.push('\t');
        catalog.push_str(&key);
        catalog.push('\n');
        let index = usize::try_from(ordinal - 1).map_err(|_| PreviewError::new("catalog", "columns"))?;
        let qualified = format!("{schema}.{table}");
        found.entry(qualified.clone()).or_default().push((index, column));
        if key == "PRI" {
            pk.entry(qualified).or_default().push(index);
        }
    }
    for table in tables {
        if pk.get(table).is_none_or(|cols| cols.is_empty()) {
            return Err(PreviewError::new("catalog", "primary_key"));
        }
    }
    Ok((pk, found, catalog))
}

async fn full_status(conn: &mut Conn) -> Result<(String, u64, String), PreviewError> {
    let (file, pos) = binary_status(conn).await?;
    let executed = scalar(conn, "SELECT @@GLOBAL.gtid_executed").await?;
    Ok((file, pos, executed))
}

async fn binary_status(conn: &mut Conn) -> Result<(String, u64), PreviewError> {
    let row: Option<Row> = conn.query_first("SHOW BINARY LOG STATUS").await.map_err(|_| PreviewError::new("protocol", "query"))?;
    let row = row.ok_or_else(|| PreviewError::new("protocol", "query"))?;
    let file = value_string(&row_value(&row, 0)?)?;
    let pos: u64 = value_string(&row_value(&row, 1)?)?.parse().map_err(|_| PreviewError::new("c1", "binlog_name"))?;
    Ok((file, pos))
}

async fn count_tables(conn: &mut Conn) -> Result<(u64, u64), PreviewError> {
    let hdr = scalar(conn, "SELECT COUNT(*) FROM oo_c1.order_hdr").await?;
    let line = scalar(conn, "SELECT COUNT(*) FROM oo_c1.order_line").await?;
    Ok((hdr.parse().map_err(|_| PreviewError::new("value", "count"))?, line.parse().map_err(|_| PreviewError::new("value", "count"))?))
}

async fn source_rows(conn: &mut Conn) -> Result<BTreeMap<(String, String), Vec<String>>, PreviewError> {
    let mut out = BTreeMap::new();
    let hdr: Vec<Row> = conn.query("SELECT id, note FROM oo_c1.order_hdr ORDER BY id").await.map_err(|_| PreviewError::new("protocol", "query"))?;
    for row in hdr {
        let id = value_string(&row_value(&row, 0)?)?;
        let note = value_string(&row_value(&row, 1)?)?;
        out.insert((HDR.to_string(), id.clone()), vec![id, note]);
    }
    let line: Vec<Row> = conn.query("SELECT id, hdr_id, qty FROM oo_c1.order_line ORDER BY id").await.map_err(|_| PreviewError::new("protocol", "query"))?;
    for row in line {
        let id = value_string(&row_value(&row, 0)?)?;
        let hdr = value_string(&row_value(&row, 1)?)?;
        let qty = value_string(&row_value(&row, 2)?)?;
        out.insert((LINE.to_string(), id.clone()), vec![id, hdr, qty]);
    }
    Ok(out)
}

fn fold(rows: &[Seen]) -> BTreeMap<(String, String), Vec<String>> {
    let mut ordered = rows.to_vec();
    ordered.sort_by(|left, right| left.commit_order.cmp(&right.commit_order).then(left.sequence.cmp(&right.sequence)));
    let mut out = BTreeMap::new();
    for row in ordered {
        if row.commit_order == 0 {
            continue;
        }
        let key = (row.table, row.key);
        if row.deleted {
            out.remove(&key);
        } else {
            out.insert(key, row.payload);
        }
    }
    out
}

async fn gold_diff(conn: &mut Conn, rows: &[Seen], cfg: &C1Config) -> Result<u64, PreviewError> {
    let decoded = fold(rows);
    if cfg.full_table_gold {
        let source = source_rows(conn).await?;
        let mut diff = 0u64;
        let keys: BTreeSet<_> = source.keys().chain(decoded.keys()).cloned().collect();
        for key in keys {
            if source.get(&key) != decoded.get(&key) {
                diff += 1;
            }
        }
        return Ok(diff);
    }
    let mut diff = 0u64;
    for ((table, key), payload) in &decoded {
        if native_key(conn, table, key).await?.as_ref() != Some(payload) {
            diff += 1;
        }
    }
    Ok(diff)
}

async fn write_attempts(conn: &mut Conn) -> Result<u64, PreviewError> {
    let sql = "SELECT CAST(IFNULL(SUM(COUNT_STAR),0) AS UNSIGNED) FROM performance_schema.events_statements_summary_by_user_by_event_name WHERE USER = 'oo_cdc' AND EVENT_NAME IN ('statement/sql/alter_table','statement/sql/create_table','statement/sql/delete','statement/sql/delete_multi','statement/sql/drop_table','statement/sql/insert','statement/sql/insert_select','statement/sql/replace','statement/sql/replace_select','statement/sql/truncate','statement/sql/update','statement/sql/update_multi')";
    let text = scalar(conn, sql).await?;
    text.parse().map_err(|_| PreviewError::new("privilege", "write_audit"))
}

fn check_script(rows: &[Seen]) -> Result<(), PreviewError> {
    let cross_hdr = rows.iter().find(|row| row.table == HDR && row.key == "810001").ok_or_else(|| PreviewError::new("c1", "cross_table"))?;
    let cross_line = rows.iter().find(|row| row.table == LINE && row.key == "810011").ok_or_else(|| PreviewError::new("c1", "cross_table"))?;
    if cross_hdr.txn_id != cross_line.txn_id || cross_hdr.commit_lsn != cross_line.commit_lsn {
        return Err(PreviewError::new("c1", "cross_table"));
    }
    if rows.iter().any(|row| row.key == "810002") {
        return Err(PreviewError::new("c1", "rollback"));
    }
    let late_hdr = rows.iter().find(|row| row.key == "810003").ok_or_else(|| PreviewError::new("c1", "late_commit"))?;
    let late_line = rows.iter().find(|row| row.key == "810012").ok_or_else(|| PreviewError::new("c1", "late_commit"))?;
    if late_line.commit_order >= late_hdr.commit_order || late_line.txn_id == late_hdr.txn_id {
        return Err(PreviewError::new("c1", "late_commit"));
    }
    let left = rows.iter().find(|row| row.key == "810013").ok_or_else(|| PreviewError::new("c1", "same_position"))?;
    let right = rows.iter().find(|row| row.key == "810014").ok_or_else(|| PreviewError::new("c1", "same_position"))?;
    if left.commit_lsn != right.commit_lsn || left.sequence == right.sequence || left.txn_id != right.txn_id {
        return Err(PreviewError::new("c1", "same_position"));
    }
    Ok(())
}

fn recover_spool(dir: &Path) -> Result<BTreeMap<(String, String), Vec<String>>, PreviewError> {
    let mut rows = Vec::new();
    let mut seen_txn = BTreeSet::new();
    let mut entries: Vec<_> = fs::read_dir(dir).map_err(|_| PreviewError::new("c1", "spool"))?.collect::<Result<_, _>>().map_err(|_| PreviewError::new("c1", "spool"))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            continue;
        }
        match recover(&entry.path()).map_err(|_| PreviewError::new("c1", "spool"))? {
            Recovery::Committed(committed) => {
                if !seen_txn.insert(committed.manifest.txn_id.clone()) {
                    return Err(PreviewError::new("c1", "duplicate"));
                }
                let iter = committed.iter().map_err(|_| PreviewError::new("c1", "spool"))?;
                for event in iter {
                    let event = event.map_err(|_| PreviewError::new("c1", "spool"))?;
                    let key = cell_text(event.key.first().ok_or_else(|| PreviewError::new("c1", "spool"))?)?;
                    let payload = event.payload.iter().map(cell_text).collect::<Result<Vec<_>, _>>()?;
                    let txn_id = committed.manifest.txn_id.clone();
                    rows.push(Seen {
                        commit_order: gno_of(&txn_id)?,
                        txn_id,
                        table: event.table,
                        key,
                        payload,
                        commit_lsn: committed.manifest.commit_lsn.clone(),
                        sequence: event.global_txn_sequence,
                        deleted: matches!(event.op, cdc_spool::Op::Delete),
                    });
                }
            }
            Recovery::RolledBack(_) | Recovery::Incomplete(_) | Recovery::Unknown { .. } => {}
        }
    }
    Ok(fold(&rows))
}

fn cell_text(cell: &cdc_spool::Cell) -> Result<String, PreviewError> {
    match cell {
        cdc_spool::Cell::Int { value, .. } | cdc_spool::Cell::Text { value, .. } | cdc_spool::Cell::Decimal { value, .. } => Ok(value.clone()),
        _ => Err(PreviewError::new("value", "cell")),
    }
}

fn binlog_name_ok(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("binlog.") else { return false };
    !rest.is_empty() && rest.len() <= 8 && rest.bytes().all(|byte| byte.is_ascii_digit())
}

fn loader_gap() -> Value {
    json!({
        "public_fn": "oo_pipeline::native_update::import_native_transaction",
        "protocol": "oo.pipeline.native-transaction/v1",
        "old_binary_sha256": "cb483866516dd783a2f75277ea9b0b2bc148418f2d6c5d9e27d305e4a9443e45",
        "old_binary_fact": "that host binary does not recognize import-native-transaction; the usage exit is the old artifact, not the current source API",
        "current_gap": "transaction_plans still requires postgres|postgresql and pg-commit-lsn/v1; mysql-binlog-pos/v1 is only allow-listed in classify",
        "shared_writer": "2b95896a-0f6d-442c-9f3d-51c074ba0138",
        "target_applied": false,
        "ack_allowed": false
    })
}

fn write_json(path: &Path, value: &Value) -> Result<(), PreviewError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|_| PreviewError::new("config", "out"))?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).map_err(|_| PreviewError::new("config", "out"))?;
    fs::rename(tmp, path).map_err(|_| PreviewError::new("config", "out"))?;
    Ok(())
}

fn row_cells(row: &BinlogRow, ordinals: &[usize]) -> Result<Vec<Value>, PreviewError> {
    let mut cells = Vec::new();
    for index in ordinals {
        cells.push(one_cell(u16::try_from(*index).map_err(|_| PreviewError::new("value", "ordinal"))?, row.as_ref(*index))?);
    }
    Ok(cells)
}

fn json_null(cell: &Value) -> bool {
    cell.get("type").and_then(|item| item.as_str()) == Some("null")
}

fn json_cell_text(cell: &Value) -> Result<String, PreviewError> {
    match cell.get("type").and_then(|item| item.as_str()) {
        Some("int" | "text" | "decimal" | "bytes") => cell
            .get("value")
            .and_then(|item| item.as_str())
            .map(str::to_string)
            .ok_or_else(|| PreviewError::new("value", "cell")),
        _ => Err(PreviewError::new("value", "cell")),
    }
}

fn one_cell(ordinal: u16, value: Option<&BinlogValue<'_>>) -> Result<Value, PreviewError> {
    let Some(value) = value else {
        return Ok(json!({"ordinal": ordinal, "type": "null"}));
    };
    let converted = MyValue::try_from(value.clone()).map_err(|_| PreviewError::new("c1", "partial_image"))?;
    match converted {
        MyValue::NULL => Ok(json!({"ordinal": ordinal, "type": "null"})),
        MyValue::Int(item) => Ok(json!({"ordinal": ordinal, "type": "int", "value": item.to_string()})),
        MyValue::UInt(item) => Ok(json!({"ordinal": ordinal, "type": "int", "value": item.to_string()})),
        MyValue::Bytes(bytes) => match String::from_utf8(bytes) {
            Ok(text) => Ok(json!({"ordinal": ordinal, "type": "text", "value": text})),
            Err(err) => {
                let hex: String = err.into_bytes().iter().map(|byte| format!("{byte:02x}")).collect();
                Ok(json!({"ordinal": ordinal, "type": "bytes", "value": hex}))
            }
        },
        MyValue::Float(_) | MyValue::Double(_) => Err(PreviewError::new("value", "inexact")),
        MyValue::Time(neg, days, hour, minute, second, micros) => {
            Ok(json!({"ordinal": ordinal, "type": "text", "value": time_text(neg, days, hour, minute, second, micros)?}))
        }
        MyValue::Date(year, month, day, hour, minute, second, _) => {
            if year == 0 {
                return Err(PreviewError::new("value", "zero_date"));
            }
            Ok(json!({"ordinal": ordinal, "type": "text", "value": format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")}))
        }
    }
}

const NATIVE_ID_CHECK: &str = "SELECT CAST((SELECT COUNT(*) FROM native_mysql_20261012.hubs WHERE hub_id=910101)+(SELECT COUNT(*) FROM native_mysql_20261012.shipments WHERE shipment_id=910111)+(SELECT COUNT(*) FROM native_mysql_20261012.tracking_events WHERE event_id=910121)+(SELECT COUNT(*) FROM native_mysql_20261012.value_probe WHERE probe_id=910131)+(SELECT COUNT(*) FROM native_mysql_20261012.range_gap WHERE gap_id=910141) AS CHAR)";

const NATIVE_DML: [&str; 7] = [
    "START TRANSACTION",
    "INSERT INTO native_mysql_20261012.hubs (hub_id, hub_code, hub_name, region_code, latitude, longitude, capacity_grams, opened_at, badge, note) VALUES (910101, 'C1-HUB', 'c1 native hub', 'C1', 12.5000, 100.2500, 7, '2026-10-14 08:15:00', 'c1b00001', 'c1-note')",
    "INSERT INTO native_mysql_20261012.shipments (shipment_id, origin_hub_id, dest_hub_id, status, transport_mode, weight_grams, cost_amount, created_at, promised_at, seal, comment) VALUES (910111, 910101, 910101, 'in_transit', 'road', 42, 3.2500, '2026-10-14 08:16:00', '2026-10-15 09:00:00', 'c1s00001', 'c1-ship')",
    "INSERT INTO native_mysql_20261012.tracking_events (event_id, shipment_id, hub_id, status, event_at, recorded_weight_grams, payload, comment) VALUES (910121, 910111, 910101, 'in_transit', '2026-10-14 08:17:00', 42, 'c1p00001', 'c1-evt')",
    "INSERT INTO native_mysql_20261012.value_probe (probe_id, unsigned_grams, exact_amount, payload, badge, day_on, clock_at, note) VALUES (910131, 9, 4.5000, '{\"tag\":\"c1\"}', 'c1v00001', '2026-10-14', '08:18:00', 'c1-probe')",
    "INSERT INTO native_mysql_20261012.range_gap (gap_id, table_name, column_name, row_key, reason, canonical_text) VALUES (910141, 'hubs', 'note', 910101, 'c1-visible', 'c1-gap')",
    "COMMIT",
];

fn ident_ok(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn one_schema(tables: &[String]) -> Result<(String, Vec<String>), PreviewError> {
    let mut schema = None;
    let mut names = Vec::new();
    for qualified in tables {
        let (left, right) = qualified.split_once('.').ok_or_else(|| PreviewError::new("config", "tables"))?;
        if !ident_ok(left) || !ident_ok(right) {
            return Err(PreviewError::new("config", "tables"));
        }
        if let Some(got) = &schema {
            if got != left {
                return Err(PreviewError::new("config", "tables"));
            }
        } else {
            schema = Some(left.to_string());
        }
        names.push(right.to_string());
    }
    Ok((schema.ok_or_else(|| PreviewError::new("config", "tables"))?, names))
}

fn parse_columns(map: &serde_json::Map<String, Value>) -> Result<BTreeMap<String, Vec<String>>, PreviewError> {
    let Some(raw) = map.get("columns") else {
        return Ok(BTreeMap::new());
    };
    let obj = raw.as_object().ok_or_else(|| PreviewError::new("config", "columns"))?;
    let mut out = BTreeMap::new();
    for (key, value) in obj {
        let (schema, table) = key.split_once('.').ok_or_else(|| PreviewError::new("config", "columns"))?;
        if !ident_ok(schema) || !ident_ok(table) {
            return Err(PreviewError::new("config", "columns"));
        }
        let list = value.as_array().ok_or_else(|| PreviewError::new("config", "columns"))?;
        let mut names = Vec::new();
        for item in list {
            let name = item.as_str().filter(|name| ident_ok(name)).ok_or_else(|| PreviewError::new("config", "columns"))?;
            names.push(name.to_string());
        }
        if names.is_empty() {
            return Err(PreviewError::new("config", "columns"));
        }
        out.insert(key.clone(), names);
    }
    Ok(out)
}

fn native_columns(table: &str) -> Option<&'static [&'static str]> {
    Some(match table {
        "native_mysql_20261012.hubs" => &["hub_id", "hub_code", "hub_name", "region_code", "latitude", "longitude", "capacity_grams", "opened_at", "badge", "note"],
        "native_mysql_20261012.range_gap" => &["gap_id", "table_name", "column_name", "row_key", "reason", "canonical_text"],
        "native_mysql_20261012.shipments" => &[
            "shipment_id",
            "origin_hub_id",
            "dest_hub_id",
            "status",
            "transport_mode",
            "weight_grams",
            "cost_amount",
            "created_at",
            "promised_at",
            "seal",
            "comment",
        ],
        "native_mysql_20261012.tracking_events" => &["event_id", "shipment_id", "hub_id", "status", "event_at", "recorded_weight_grams", "payload", "comment"],
        "native_mysql_20261012.value_probe" => &["probe_id", "unsigned_grams", "exact_amount", "payload", "badge", "day_on", "clock_at", "note"],
        _ => return None,
    })
}

fn native_columns_match(columns: &BTreeMap<String, Vec<String>>) -> bool {
    columns.len() == NATIVE_TABLES.len()
        && NATIVE_TABLES.iter().all(|table| {
            columns.get(*table).is_some_and(|got| native_columns(table).is_some_and(|want| got.iter().map(String::as_str).eq(want.iter().copied())))
        })
}

fn visible_ordinals(cfg: &C1Config, names: &BTreeMap<String, Vec<(usize, String)>>) -> Result<BTreeMap<String, Vec<usize>>, PreviewError> {
    let mut out = BTreeMap::new();
    for table in &cfg.tables {
        let have = names.get(table).ok_or_else(|| PreviewError::new("catalog", "columns"))?;
        let ords = if cfg.columns.is_empty() {
            have.iter().map(|(ord, _)| *ord).collect::<Vec<_>>()
        } else {
            let want = cfg.columns.get(table).ok_or_else(|| PreviewError::new("config", "columns"))?;
            let mut picked = Vec::new();
            for name in want {
                picked.push(have.iter().find(|(_, column)| column == name).map(|(ord, _)| *ord).ok_or_else(|| PreviewError::new("catalog", "columns"))?);
            }
            picked.sort_unstable();
            picked
        };
        if ords.is_empty() {
            return Err(PreviewError::new("catalog", "columns"));
        }
        out.insert(table.clone(), ords);
    }
    Ok(out)
}

async fn ensure_native_select(conn: &mut Conn) -> Result<(), PreviewError> {
    let mut grants = rows_first(
        &conn.query("SHOW GRANTS FOR 'oo_cdc'@'%'").await.map_err(|_| PreviewError::new("privilege", "replication"))?,
    )?;
    if !grants.join("\n").to_ascii_uppercase().contains("NATIVE_MYSQL_20261012") {
        exec_static(conn, "GRANT SELECT ON native_mysql_20261012.* TO 'oo_cdc'@'%'").await?;
        grants = rows_first(&conn.query("SHOW GRANTS FOR 'oo_cdc'@'%'").await.map_err(|_| PreviewError::new("privilege", "replication"))?)?;
    }
    grants_ok(&grants)?;
    if !grants.join("\n").to_ascii_uppercase().contains("NATIVE_MYSQL_20261012") {
        return Err(PreviewError::new("privilege", "replication"));
    }
    Ok(())
}

async fn native_dml(cfg: &C1Config) -> Result<(), PreviewError> {
    let mut conn = connect_dba(cfg).await?;
    for sql in NATIVE_DML {
        exec_static(&mut conn, sql).await?;
    }
    Ok(())
}

fn check_native(rows: &[Seen]) -> Result<(), PreviewError> {
    let keys = [
        ("native_mysql_20261012.hubs", "910101"),
        ("native_mysql_20261012.shipments", "910111"),
        ("native_mysql_20261012.tracking_events", "910121"),
        ("native_mysql_20261012.value_probe", "910131"),
        ("native_mysql_20261012.range_gap", "910141"),
    ];
    if rows.len() != keys.len() {
        return Err(PreviewError::new("c1", "cross_table"));
    }
    let first = rows.iter().find(|row| row.key == "910101").ok_or_else(|| PreviewError::new("c1", "cross_table"))?;
    for (table, key) in keys {
        let row = rows.iter().find(|row| row.table == table && row.key == key && !row.deleted).ok_or_else(|| PreviewError::new("c1", "cross_table"))?;
        if row.txn_id != first.txn_id || row.commit_lsn != first.commit_lsn || row.commit_order == 0 {
            return Err(PreviewError::new("c1", "cross_table"));
        }
    }
    Ok(())
}

struct NativeRead {
    sql: &'static str,
    width: usize,
    json_at: Option<usize>,
}

fn native_read(table: &str) -> Result<NativeRead, PreviewError> {
    Ok(match table {
        "native_mysql_20261012.hubs" => NativeRead {
            sql: "SELECT CAST(hub_id AS CHAR), hub_code, hub_name, region_code, CAST(latitude AS CHAR), CAST(longitude AS CHAR), CAST(capacity_grams AS CHAR), DATE_FORMAT(opened_at, '%Y-%m-%dT%H:%i:%sZ'), CONVERT(badge USING utf8mb4), note FROM native_mysql_20261012.hubs WHERE hub_id = ?",
            width: 10,
            json_at: None,
        },
        "native_mysql_20261012.range_gap" => NativeRead {
            sql: "SELECT CAST(gap_id AS CHAR), table_name, column_name, CAST(row_key AS CHAR), reason, canonical_text FROM native_mysql_20261012.range_gap WHERE gap_id = ?",
            width: 6,
            json_at: None,
        },
        "native_mysql_20261012.shipments" => NativeRead {
            sql: "SELECT CAST(shipment_id AS CHAR), CAST(origin_hub_id AS CHAR), CAST(dest_hub_id AS CHAR), status, transport_mode, CAST(weight_grams AS CHAR), CAST(cost_amount AS CHAR), DATE_FORMAT(created_at, '%Y-%m-%dT%H:%i:%sZ'), DATE_FORMAT(promised_at, '%Y-%m-%dT%H:%i:%sZ'), CONVERT(seal USING utf8mb4), comment FROM native_mysql_20261012.shipments WHERE shipment_id = ?",
            width: 11,
            json_at: None,
        },
        "native_mysql_20261012.tracking_events" => NativeRead {
            sql: "SELECT CAST(event_id AS CHAR), CAST(shipment_id AS CHAR), CAST(hub_id AS CHAR), status, DATE_FORMAT(event_at, '%Y-%m-%dT%H:%i:%sZ'), CAST(recorded_weight_grams AS CHAR), CONVERT(payload USING utf8mb4), comment FROM native_mysql_20261012.tracking_events WHERE event_id = ?",
            width: 8,
            json_at: None,
        },
        "native_mysql_20261012.value_probe" => NativeRead {
            sql: "SELECT CAST(probe_id AS CHAR), CAST(unsigned_grams AS CHAR), CAST(exact_amount AS CHAR), CAST(payload AS CHAR), CONVERT(badge USING utf8mb4), DATE_FORMAT(day_on, '%Y-%m-%dT00:00:00Z'), TIME_FORMAT(clock_at, '%H:%i:%s'), note FROM native_mysql_20261012.value_probe WHERE probe_id = ?",
            width: 8,
            json_at: Some(3),
        },
        _ => return Err(PreviewError::new("catalog", "columns")),
    })
}

fn compact_json(text: &str) -> Result<String, PreviewError> {
    let value: Value = serde_json::from_str(text).map_err(|_| PreviewError::new("value", "cell"))?;
    serde_json::to_string(&value).map_err(|_| PreviewError::new("value", "cell"))
}

async fn native_key(conn: &mut Conn, table: &str, key: &str) -> Result<Option<Vec<String>>, PreviewError> {
    let read = native_read(table)?;
    let id: u64 = key.parse().map_err(|_| PreviewError::new("value", "key"))?;
    let row: Option<Row> = conn.exec_first(read.sql, (id,)).await.map_err(|_| PreviewError::new("protocol", "query"))?;
    let Some(row) = row else {
        return Ok(None);
    };
    let mut cells = Vec::new();
    for index in 0..read.width {
        let mut text = value_string(&row_value(&row, index)?)?;
        if read.json_at == Some(index) {
            text = compact_json(&text)?;
        }
        cells.push(text);
    }
    Ok(Some(cells))
}

fn time_text(neg: bool, days: u32, hour: u8, minute: u8, second: u8, micros: u32) -> Result<String, PreviewError> {
    if neg || days != 0 || micros != 0 {
        return Err(PreviewError::new("value", "time"));
    }
    Ok(format!("{hour:02}:{minute:02}:{second:02}"))
}

#[cfg(test)]
mod tests {
    use super::{civil_from_unix, commit_lsn, compact_json, native_columns, parse_span, time_text, NATIVE_TABLES};

    #[test]
    fn commit_lsn_uses_the_docs04_split() {
        let (lsn, position) = commit_lsn("binlog.000007", 0x3ab).unwrap();
        assert_eq!(lsn, "7/3AB");
        let value = (7u128 << 32) | 0x3ab;
        assert_eq!(position, ((1u128 << 64) | value).to_string());
    }

    #[test]
    fn unix_epoch_and_a_known_second() {
        assert_eq!(civil_from_unix(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(civil_from_unix(1_700_000_000), (2023, 11, 14, 22, 13, 20));
    }

    #[test]
    fn gtid_interval_parses_one_uuid() {
        let span = parse_span("1bc0749a-c3f5-11f1-9a89-6e5583e808c4:1-3").unwrap();
        assert_eq!(span.start, 1);
        assert_eq!(span.end, 3);
    }

    #[test]
    fn whole_seconds_are_text_and_json_is_compact() {
        assert_eq!(time_text(false, 0, 8, 18, 0, 0).unwrap(), "08:18:00");
        assert!(time_text(true, 0, 1, 0, 0, 0).is_err());
        assert_eq!(compact_json("{\"tag\": \"c1\"}").unwrap(), "{\"tag\":\"c1\"}");
    }

    #[test]
    fn measured_role_columns_cover_the_five_tables() {
        let count: usize = NATIVE_TABLES.iter().map(|table| native_columns(table).unwrap().len()).sum();
        assert_eq!(count, 43);
    }
}
