//! PostgreSQL C1 logical capture for this source.
//!
//! The platform reads a DBA-created publication and slot. It does not create them,
//! and it does not fall back to batch mode when they are missing.
//! Committed transactions are read with `pg_logical_slot_peek_binary_changes`
//! and acknowledged with `pg_replication_slot_advance` only after the target
//! transaction commits. Snapshot rows fill gaps and do not cover CDC writes.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use arrow_array::{Array, Int64Array, RecordBatch, StringArray};
use arrow_ipc::reader::StreamReader;
use arrow_ipc::writer::{IpcWriteOptions, StreamWriter};
use arrow_ipc::MetadataVersion;
use arrow_schema::{DataType, Field, Schema};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Cursor;
use std::sync::Arc;
use tokio_postgres::types::ToSql;
use tokio_postgres::GenericClient;

use crate::config::{Capture, Config};
use crate::error::Fail;
use crate::logistics::type_allowed;
use crate::pg::{connect, connect_read_only, endpoint_from_files, Endpoint, Session};

const CHECK_PROTOCOL: &str = "oo.postgres-preview.capture-check/v1";
const SYNC_PROTOCOL: &str = "oo.postgres-preview.capture-sync/v1";
const COMPARE_PROTOCOL: &str = "oo.postgres-preview.capture-compare/v1";
const SCHEDULE_PROTOCOL: &str = "oo.postgres-preview.capture-schedule/v1";

pub struct Report {
    pub body: Value,
    pub error: Option<Fail>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct StoredCol {
    name: String,
    format_type: String,
    type_oid: i64,
    published: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Cell {
    pub(crate) kind: CellKind,
    pub(crate) text: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CellKind {
    Null,
    Unchanged,
    Value,
}

#[derive(Clone, Debug)]
pub(crate) struct RelCol {
    pub(crate) name: String,
    pub(crate) type_oid: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct Rel {
    pub(crate) oid: u32,
    pub(crate) namespace: String,
    pub(crate) name: String,
    pub(crate) columns: Vec<RelCol>,
}

#[derive(Clone, Debug)]
pub(crate) enum Msg {
    Begin { xid: i32, final_lsn: u64 },
    Commit { commit_lsn: u64, end_lsn: u64, commit_us: i64 },
    Relation(Rel),
    Insert { oid: u32, cells: Vec<Cell> },
    Update { oid: u32, cells: Vec<Cell> },
    Delete { oid: u32, cells: Vec<Cell> },
    Truncate { oids: Vec<u32> },
    Ignore,
}

#[derive(Clone, Debug)]
enum TxnEvent {
    Upsert { rel: Rel, cells: Vec<Cell>, seq: i64 },
    Delete { rel: Rel, cells: Vec<Cell>, seq: i64 },
    Truncate { rel: Rel, seq: i64 },
}

#[derive(Clone, Debug)]
struct ReadyTxn {
    commit_lsn: u64,
    end_lsn: u64,
    commit_us: i64,
    events: Vec<TxnEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tick {
    Fire,
    Wait,
    SkipOverlap,
}

struct ArrowRow {
    pk: String,
    op: i64,
    pos: String,
    seq: i64,
    src_ts: Option<i64>,
    unavail: String,
    values: Vec<Option<String>>,
    extras: Vec<(String, Option<String>)>,
}

#[derive(Clone)]
struct TablePlan {
    schema: String,
    table: String,
    pk: String,
    columns: Vec<StoredCol>,
    excluded: Vec<String>,
    replica: String,
}

struct Inspection {
    blocks: Vec<String>,
    warnings: Vec<String>,
    checklist: Vec<String>,
    wal_level: String,
    keep_size: String,
    server_version: String,
    slot_present: bool,
    slot_restart: String,
    slot_flush: String,
    tables: Vec<TablePlan>,
}

pub async fn check(config: &Config) -> Report {
    let mut body = base(CHECK_PROTOCOL);
    body["business_read"] = json!(false);
    body["connected"] = json!(false);
    body["fallback"] = json!("none");
    let Some(capture) = config.capture.clone() else {
        body["blocks"] = json!(["capture_absent"]);
        return Report { body, error: Some(Fail::new("capture.refused", 4)) };
    };
    if let Err(fail) = validate(config, &capture, false) {
        body["blocks"] = json!([fail.token]);
        return Report { body, error: Some(fail) };
    }
    body["method"] = json!(capture.method);
    body["publication"] = json!(capture.publication);
    body["slot"] = json!(capture.slot);
    body["generation"] = json!(capture.generation);
    let password_present = Path::new(&capture.password_file).is_file();
    body["cdc_password_present"] = json!(password_present);
    body["checklist_executed_by_platform"] = json!(false);
    body["inspect_role"] = json!(if password_present { "cdc" } else { "reader" });
    let endpoint = if password_present {
        cdc_endpoint(config, &capture)
    } else {
        reader_endpoint(config)
    };
    let endpoint = match endpoint {
        Ok(endpoint) => endpoint,
        Err(fail) => return Report { body, error: Some(fail) },
    };
    let session = match connect_read_only(&endpoint).await {
        Ok(session) => session,
        Err(fail) => return Report { body, error: Some(fail) },
    };
    body["connected"] = json!(true);
    match inspect(&session, config, &capture).await {
        Ok(found) => finish_inspection(body, found, None),
        Err(err) => db_report(body, err, "capture.blocked", 5),
    }
}

pub async fn sync(config: &Config) -> Report {
    let mut body = base(SYNC_PROTOCOL);
    body["business_read"] = json!(false);
    body["connected"] = json!(false);
    body["fallback"] = json!("none");
    body["loader_interface"] = loader_gap();
    let Some(capture) = config.capture.clone() else {
        body["blocks"] = json!(["capture_absent"]);
        return Report { body, error: Some(Fail::new("capture.refused", 4)) };
    };
    if let Err(fail) = validate(config, &capture, true) {
        body["blocks"] = json!([fail.token]);
        body["cdc_password_present"] = json!(Path::new(&capture.password_file).is_file());
        return Report { body, error: Some(fail) };
    }
    body["method"] = json!(capture.method);
    body["publication"] = json!(capture.publication);
    body["slot"] = json!(capture.slot);
    body["generation"] = json!(capture.generation);
    body["window_rows"] = json!(capture.window_rows);
    body["chunk_limit"] = match chunk_limit() {
        Ok(limit) => json!(limit),
        Err(fail) => return Report { body, error: Some(fail) },
    };
    let target_ep = match target_super(config) {
        Ok(endpoint) => endpoint,
        Err(fail) => return Report { body, error: Some(fail) },
    };
    let mut target = match connect(&target_ep).await {
        Ok(session) => session,
        Err(fail) => return Report { body, error: Some(fail) },
    };
    if let Err(err) = prepare_target(&mut target).await {
        return db_report(body, err, "capture.apply", 4);
    }
    let live = match read_live(&target, &config.source_id).await {
        Ok(live) => live,
        Err(err) => return db_report(body, err, "capture.apply", 4),
    };
    body["live_generation"] = json!(live);
    body["inspect_role"] = json!("cdc");
    let cdc = match cdc_endpoint(config, &capture) {
        Ok(endpoint) => endpoint,
        Err(fail) => return Report { body, error: Some(fail) },
    };
    let mut source = match connect_read_only(&cdc).await {
        Ok(session) => session,
        Err(fail) => return Report { body, error: Some(fail) },
    };
    body["connected"] = json!(true);
    let inspection = match inspect(&source, config, &capture).await {
        Ok(found) => found,
        Err(err) => return db_report(body, err, "capture.blocked", 5),
    };
    if !inspection.slot_present && live == Some(capture.generation) {
        body["phase"] = json!("position_expired");
        body["business_read"] = json!(false);
        body["slot_recreated"] = json!(false);
        body["blocks"] = json!(["slot_absent"]);
        body["warnings"] = json!(inspection.warnings);
        return Report { body, error: Some(Fail::new("capture.position_expired", 8)) };
    }
    if !inspection.blocks.is_empty() {
        return finish_inspection(body, inspection, Some(Fail::new("capture.blocked", 5)));
    }
    let mut advance = match connect(&cdc).await {
        Ok(session) => session,
        Err(fail) => return Report { body, error: Some(fail) },
    };
    if let Err(err) = prepare_source(&source, config.budget.statement_ms).await {
        return db_report(body, err, "capture.apply", 4);
    }
    if let Err(err) = prepare_source(&advance, config.budget.statement_ms).await {
        return db_report(body, err, "capture.apply", 4);
    }
    let mut ends = Ends { source: &mut source, advance: &mut advance, target: &mut target };
    run_sync(config, &capture, &inspection, &mut ends, body, live).await
}

pub async fn compare(config: &Config) -> Report {
    let mut body = base(COMPARE_PROTOCOL);
    body["business_read"] = json!(false);
    body["sampled"] = json!(false);
    let Some(capture) = config.capture.clone() else {
        return Report { body, error: Some(Fail::new("capture.refused", 4)) };
    };
    if let Err(fail) = validate(config, &capture, true) {
        return Report { body, error: Some(fail) };
    }
    let cdc = match cdc_endpoint(config, &capture) {
        Ok(endpoint) => endpoint,
        Err(fail) => return Report { body, error: Some(fail) },
    };
    let source = match connect_read_only(&cdc).await {
        Ok(session) => session,
        Err(fail) => return Report { body, error: Some(fail) },
    };
    let target_ep = match target_super(config) {
        Ok(endpoint) => endpoint,
        Err(fail) => return Report { body, error: Some(fail) },
    };
    let target = match connect_read_only(&target_ep).await {
        Ok(session) => session,
        Err(fail) => return Report { body, error: Some(fail) },
    };
    if let Err(err) = prepare_source(&source, config.budget.statement_ms).await {
        return db_report(body, err, "capture.diff", 7);
    }
    match compare_tables(config, &capture, &source, &target, &mut body).await {
        Ok(()) => {
            let mismatches = body["cell_mismatches"].as_u64().unwrap_or(0);
            let error = if mismatches == 0 { None } else { Some(Fail::new("capture.diff", 7)) };
            Report { body, error }
        }
        Err(err) => step_report(body, err, "capture.diff", 7),
    }
}

pub async fn schedule(config: &Config) -> Report {
    let mut body = base(SCHEDULE_PROTOCOL);
    body["business_read"] = json!(false);
    let Some(capture) = config.capture.clone() else {
        return Report { body, error: Some(Fail::new("capture.refused", 4)) };
    };
    if let Err(fail) = validate(config, &capture, true) {
        return Report { body, error: Some(fail) };
    }
    body["reconcile_ms"] = json!(capture.reconcile_ms);
    body["health_ms"] = json!(capture.health_ms);
    let cdc = match cdc_endpoint(config, &capture) {
        Ok(endpoint) => endpoint,
        Err(fail) => return Report { body, error: Some(fail) },
    };
    let source = match connect_read_only(&cdc).await {
        Ok(session) => session,
        Err(fail) => return Report { body, error: Some(fail) },
    };
    let target_ep = match target_super(config) {
        Ok(endpoint) => endpoint,
        Err(fail) => return Report { body, error: Some(fail) },
    };
    let target = match connect_read_only(&target_ep).await {
        Ok(session) => session,
        Err(fail) => return Report { body, error: Some(fail) },
    };
    if let Err(err) = prepare_source(&source, config.budget.statement_ms).await {
        return db_report(body, err, "capture.schedule", 4);
    }
    match run_schedule(config, &capture, &source, &target, &mut body).await {
        Ok(()) => Report { body, error: None },
        Err(err) => db_report(body, err, "capture.schedule", 4),
    }
}

fn validate(config: &Config, capture: &Capture, require_secret: bool) -> Result<(), Fail> {
    if capture.method != "c1-logical" {
        return Err(Fail::new("capture.refused", 4));
    }
    if !ident_ok(&capture.publication) || !ident_ok(&capture.slot) || !ident_ok(&capture.username) {
        return Err(Fail::new("capture.refused", 4));
    }
    if capture.username == config.source.username || capture.username == config.dba.username || capture.username == config.target.username {
        return Err(Fail::new("identity.separate", 6));
    }
    if capture.reconcile_ms == 0 || capture.health_ms == 0 || capture.reconcile_ms > 86_400_000 || capture.health_ms > 86_400_000 {
        return Err(Fail::new("capture.refused", 4));
    }
    if !(1..=5_000).contains(&capture.window_rows) || capture.generation == 0 {
        return Err(Fail::new("capture.refused", 4));
    }
    if !capture.password_file.starts_with('/') || capture.password_file.contains("..") || capture.password_file.contains(' ') {
        return Err(Fail::new("capture.refused", 4));
    }
    if require_secret && !Path::new(&capture.password_file).is_file() {
        return Err(Fail::new("capture.refused", 4));
    }
    let mut seen = BTreeSet::new();
    for object in &config.approved_objects {
        if !config.approved_fields.iter().any(|field| field.schema == object.schema && field.table == object.table) {
            return Err(Fail::new("capture.refused", 4));
        }
        seen.insert((object.schema.as_str(), object.table.as_str()));
    }
    if seen.is_empty() {
        return Err(Fail::new("capture.refused", 4));
    }
    Ok(())
}

fn ident_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

pub fn format_type_allowed(value: &str) -> bool {
    if matches!(
        value,
        "bigint"
            | "integer"
            | "smallint"
            | "boolean"
            | "real"
            | "double precision"
            | "text"
            | "bytea"
            | "uuid"
            | "date"
            | "json"
            | "numeric"
            | "character"
            | "character varying"
            | "time without time zone"
            | "timestamp without time zone"
            | "timestamp with time zone"
    ) {
        return true;
    }
    numeric_or_char(value) || timed(value)
}

fn numeric_or_char(value: &str) -> bool {
    for prefix in ["numeric", "character varying", "character"] {
        let Some(rest) = value.strip_prefix(prefix) else {
            continue;
        };
        let Some(inner) = rest.strip_prefix('(').and_then(|text| text.strip_suffix(')')) else {
            return false;
        };
        if prefix == "character" && value.starts_with("character varying") {
            continue;
        }
        return precision_ok(inner);
    }
    false
}

fn precision_ok(inner: &str) -> bool {
    let mut parts = inner.split(',');
    let Some(precision) = parts.next() else {
        return false;
    };
    let scale = parts.next();
    if parts.next().is_some() {
        return false;
    }
    let Ok(precision) = precision.parse::<u16>() else {
        return false;
    };
    if !(1..=1000).contains(&precision) {
        return false;
    }
    match scale {
        None => true,
        Some(scale) => scale.parse::<u16>().is_ok_and(|scale| scale <= precision),
    }
}

fn timed(value: &str) -> bool {
    for (name, suffix) in [
        ("time", "time without time zone"),
        ("timestamp", "timestamp without time zone"),
        ("timestamp", "timestamp with time zone"),
    ] {
        let Some(head) = value.strip_suffix(suffix) else {
            continue;
        };
        let Some(rest) = head.trim_end().strip_prefix(name) else {
            continue;
        };
        let Some(inner) = rest.strip_prefix('(').and_then(|text| text.strip_suffix(')')) else {
            return false;
        };
        let Ok(precision) = inner.parse::<u16>() else {
            return false;
        };
        return (0..=6).contains(&precision);
    }
    false
}

pub fn schedule_tick(now_ms: u64, last_ms: Option<u64>, period_ms: u64, in_flight: bool) -> Tick {
    if in_flight {
        return Tick::SkipOverlap;
    }
    match last_ms {
        Some(last) if now_ms.saturating_sub(last) < period_ms => Tick::Wait,
        _ => Tick::Fire,
    }
}

pub fn parse_lsn(text: &str) -> Result<u64, Fail> {
    let Some((hi, lo)) = text.split_once('/') else {
        return Err(Fail::new("capture.protocol", 4));
    };
    if hi.is_empty() || lo.is_empty() || !hi.bytes().all(|byte| byte.is_ascii_hexdigit()) || !lo.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Fail::new("capture.protocol", 4));
    }
    let hi = u64::from_str_radix(hi, 16).map_err(|_| Fail::new("capture.protocol", 4))?;
    let lo = u64::from_str_radix(lo, 16).map_err(|_| Fail::new("capture.protocol", 4))?;
    if hi > u64::from(u32::MAX) || lo > u64::from(u32::MAX) {
        return Err(Fail::new("capture.protocol", 4));
    }
    Ok((hi << 32) | lo)
}

pub fn format_lsn(value: u64) -> String {
    format!("{:X}/{:X}", value >> 32, value & 0xFFFF_FFFF)
}

pub fn lsn_advance(current: u64, next: u64) -> Option<u64> {
    if next > current { Some(next) } else { None }
}

fn selects_business_table(sql: &str) -> bool {
    let lower = sql.to_ascii_lowercase();
    for table in ["hubs", "shipments", "tracking_events", "delivery_notes", "payroll", "type_probe"] {
        if lower.contains(&format!("from {table}")) || lower.contains(&format!("from public.{table}")) || lower.contains(&format!("from customer_east.{table}")) {
            return true;
        }
    }
    false
}

fn chunk_limit() -> Result<Option<u32>, Fail> {
    match std::env::var("OO_CAPTURE_MAX_CHUNKS") {
        Err(_) => Ok(None),
        Ok(text) if text.is_empty() => Ok(None),
        Ok(text) => {
            let value: u32 = text.parse().map_err(|_| Fail::new("capture.refused", 4))?;
            if value == 0 {
                Err(Fail::new("capture.refused", 4))
            } else {
                Ok(Some(value))
            }
        }
    }
}

fn reader_endpoint(config: &Config) -> Result<Endpoint, Fail> {
    endpoint_from_files(
        &config.source.host,
        config.source.port,
        &config.source.database,
        &config.source.username,
        &config.source.password_file,
        &config.source.ca_file,
    )
}

fn cdc_endpoint(config: &Config, capture: &Capture) -> Result<Endpoint, Fail> {
    endpoint_from_files(
        &config.source.host,
        config.source.port,
        &config.source.database,
        &capture.username,
        &capture.password_file,
        &config.source.ca_file,
    )
}

fn target_super(config: &Config) -> Result<Endpoint, Fail> {
    let parent = Path::new(&config.target.password_file).parent().ok_or(Fail::new("capture.refused", 4))?;
    let path = parent.join("super");
    let path = path.to_str().ok_or(Fail::new("capture.refused", 4))?;
    endpoint_from_files(&config.target.host, config.target.port, &config.target.database, "postgres", path, &config.target.ca_file)
}

fn base(protocol: &str) -> Value {
    json!({
        "protocol": protocol,
        "route": "c1-logical",
        "platform_creates_publication": false,
        "platform_creates_slot": false
    })
}

fn loader_gap() -> Value {
    json!({
        "applied_schema": "oo_capture",
        "pipeline_loader_accepts_cdc_arrow": false,
        "direct_insert_into_oo_physical_rejected_by_trigger": true,
        "shared_owner": "06 pipeline/pg-builder waiting on 05 core/builder/source-semantics",
        "gateway_owner": "21 operator/Gateway fixed interface only",
        "self_build_owner": "public Self-build control",
        "request": "Accept bounded CDC Arrow batches with _oo_op, _oo_pos, _oo_src_ts, and _oo_unavailable, apply them idempotently in the loader, and publish only after the L_end barrier. This preview records that barrier in oo_capture and does not copy the shared loader, Self-build, or Gateway."
    })
}

fn db_report(mut body: Value, err: tokio_postgres::Error, token: &'static str, exit: u8) -> Report {
    let sqlstate = err.code().map(|code| code.code().to_owned()).unwrap_or_else(|| "none".to_owned());
    body["sqlstate"] = json!(sqlstate);
    body["pg_error"] = json!(pg_error_text(&err));
    Report { body, error: Some(Fail::new(token, exit)) }
}

fn pg_error_text(err: &tokio_postgres::Error) -> String {
    let mut text = err.to_string();
    let mut current = std::error::Error::source(err);
    for _ in 0..3 {
        let Some(item) = current else { break };
        text.push_str(": ");
        text.push_str(&item.to_string());
        current = std::error::Error::source(item);
    }
    text.truncate(300);
    text
}

enum StepErr {
    Db(tokio_postgres::Error),
    Rule(Fail),
}

impl From<tokio_postgres::Error> for StepErr {
    fn from(err: tokio_postgres::Error) -> Self {
        StepErr::Db(err)
    }
}

fn step_report(body: Value, err: StepErr, token: &'static str, exit: u8) -> Report {
    match err {
        StepErr::Db(err) => db_report(body, err, token, exit),
        StepErr::Rule(fail) => Report { body, error: Some(fail) },
    }
}

struct Ends<'a> {
    source: &'a mut Session,
    advance: &'a mut Session,
    target: &'a mut Session,
}

struct CaptureOut<'a> {
    applied: &'a mut BuildState,
    arrow: &'a mut Vec<Value>,
    paused: &'a mut Vec<String>,
}

fn finish_inspection(mut body: Value, found: Inspection, forced: Option<Fail>) -> Report {
    body["wal_level"] = json!(found.wal_level);
    body["max_slot_wal_keep_size"] = json!(found.keep_size);
    body["server_version"] = json!(found.server_version);
    body["slot_present"] = json!(found.slot_present);
    body["slot_flush_lsn"] = json!(found.slot_flush);
    body["blocks"] = json!(found.blocks);
    body["warnings"] = json!(found.warnings);
    body["dba_checklist"] = json!(found.checklist);
    body["business_read"] = json!(false);
    body["tables"] = json!(found.tables.iter().map(|table| {
        json!({
            "schema": table.schema,
            "table": table.table,
            "pk": table.pk,
            "replica": table.replica,
            "columns": table.columns.iter().map(|col| &col.name).collect::<Vec<_>>(),
            "excluded": table.excluded
        })
    }).collect::<Vec<_>>());
    let error = forced.or_else(|| {
        if found.blocks.is_empty() {
            None
        } else {
            Some(Fail::new("capture.blocked", 5))
        }
    });
    Report { body, error }
}

async fn prepare_source(session: &Session, statement_ms: u64) -> Result<(), tokio_postgres::Error> {
    let timeout = format!("SET statement_timeout = '{statement_ms}ms'");
    session.client.batch_execute("SET DateStyle = 'ISO, MDY'").await?;
    session.client.batch_execute("SET timezone = 'UTC'").await?;
    session.client.batch_execute("SET search_path = pg_catalog").await?;
    session.client.batch_execute(&timeout).await?;
    Ok(())
}

async fn prepare_target(session: &mut Session) -> Result<(), tokio_postgres::Error> {
    session.client.batch_execute("SET timezone = 'UTC'").await?;
    session.client.batch_execute("SET DateStyle = 'ISO, MDY'").await?;
    session.client.batch_execute("SET search_path = pg_catalog").await?;
    session.client.batch_execute(CONTROL_DDL).await?;
    Ok(())
}

const CONTROL_DDL: &str = "
CREATE SCHEMA IF NOT EXISTS oo_capture;
REVOKE ALL ON SCHEMA oo_capture FROM PUBLIC;
CREATE TABLE IF NOT EXISTS oo_capture.live (
  source_id text PRIMARY KEY,
  generation bigint NOT NULL,
  l0 text NOT NULL,
  l_end text NOT NULL,
  applied_lsn text NOT NULL,
  reason text NOT NULL,
  cross_table_consistent boolean NOT NULL
);
CREATE TABLE IF NOT EXISTS oo_capture.build (
  source_id text NOT NULL,
  generation bigint NOT NULL,
  phase text NOT NULL,
  l0 text,
  l_end text,
  applied_lsn text,
  reason text NOT NULL DEFAULT '',
  PRIMARY KEY (source_id, generation)
);
CREATE TABLE IF NOT EXISTS oo_capture.relmap (
  source_id text NOT NULL,
  generation bigint NOT NULL,
  rel_key text NOT NULL,
  physical text NOT NULL,
  pk_name text NOT NULL,
  columns_json text NOT NULL,
  excluded_json text NOT NULL,
  paused boolean NOT NULL DEFAULT false,
  pause_reason text NOT NULL DEFAULT '',
  PRIMARY KEY (source_id, generation, rel_key)
);
CREATE TABLE IF NOT EXISTS oo_capture.progress (
  source_id text NOT NULL,
  generation bigint NOT NULL,
  rel_key text NOT NULL,
  last_pk text,
  done boolean NOT NULL,
  rows bigint NOT NULL,
  PRIMARY KEY (source_id, generation, rel_key)
);
CREATE TABLE IF NOT EXISTS oo_capture.tombstone (
  source_id text NOT NULL,
  generation bigint NOT NULL,
  rel_key text NOT NULL,
  pk text NOT NULL,
  pos numeric(20,0) NOT NULL,
  PRIMARY KEY (source_id, generation, rel_key, pk)
);
CREATE TABLE IF NOT EXISTS oo_capture.truncated (
  source_id text NOT NULL,
  generation bigint NOT NULL,
  rel_key text NOT NULL,
  pos numeric(20,0) NOT NULL,
  PRIMARY KEY (source_id, generation, rel_key)
);
CREATE TABLE IF NOT EXISTS oo_capture.unavailable (
  source_id text NOT NULL,
  generation bigint NOT NULL,
  rel_key text NOT NULL,
  pk text NOT NULL,
  column_name text NOT NULL,
  PRIMARY KEY (source_id, generation, rel_key, pk, column_name)
);
CREATE TABLE IF NOT EXISTS oo_capture.unmapped (
  source_id text NOT NULL,
  generation bigint NOT NULL,
  rel_key text NOT NULL,
  column_name text NOT NULL,
  format_type text NOT NULL,
  type_oid bigint NOT NULL,
  first_pos numeric(20,0) NOT NULL,
  PRIMARY KEY (source_id, generation, rel_key, column_name)
);
CREATE TABLE IF NOT EXISTS oo_capture.quarantine (
  source_id text NOT NULL,
  generation bigint NOT NULL,
  rel_key text NOT NULL,
  pos numeric(20,0) NOT NULL,
  kind text NOT NULL,
  payload text NOT NULL
);
";

async fn inspect(session: &Session, config: &Config, capture: &Capture) -> Result<Inspection, tokio_postgres::Error> {
    let mut blocks = Vec::new();
    let mut warnings = Vec::new();
    if !Path::new(&capture.password_file).is_file() {
        blocks.push("cdc_password_absent".to_owned());
    }
    let wal = session.client.query_one(
        "SELECT current_setting('wal_level'), current_setting('max_slot_wal_keep_size'), current_setting('server_version'), current_setting('server_version_num'), pg_catalog.pg_is_in_recovery()",
        &[],
    ).await?;
    let wal_level: String = wal.get(0);
    let keep_size: String = wal.get(1);
    let server_version: String = wal.get(2);
    let version_num: String = wal.get(3);
    let recovery: bool = wal.get(4);
    if wal_level != "logical" {
        blocks.push("wal_level".to_owned());
    }
    if keep_size == "-1" {
        warnings.push("max_slot_wal_keep_size_unlimited".to_owned());
    }
    if recovery {
        warnings.push("source_in_recovery".to_owned());
    }
    if version_num.parse::<i64>().unwrap_or(0) < 130_000 {
        blocks.push("server_version".to_owned());
    }
    let publication = session.client.query_opt(
        "SELECT puballtables FROM pg_catalog.pg_publication WHERE pubname = $1",
        &[&capture.publication],
    ).await?;
    let mut slot_present = false;
    let mut slot_restart = String::new();
    let mut slot_flush = String::new();
    if let Some(row) = publication {
        let all_tables: bool = row.get(0);
        if all_tables {
            blocks.push("publication_all_tables".to_owned());
        }
        let members = session.client.query(
            "SELECT n.nspname, c.relname FROM pg_catalog.pg_publication_rel pr JOIN pg_catalog.pg_publication p ON p.oid = pr.prpubid JOIN pg_catalog.pg_class c ON c.oid = pr.prrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE p.pubname = $1",
            &[&capture.publication],
        ).await?;
        let mut got = BTreeSet::new();
        for member in &members {
            let schema: String = member.get(0);
            let table: String = member.get(1);
            got.insert((schema, table));
        }
        let mut want = BTreeSet::new();
        for object in &config.approved_objects {
            want.insert((object.schema.clone(), object.table.clone()));
        }
        if got != want {
            blocks.push("publication_members".to_owned());
        }
    } else {
        blocks.push("publication_absent".to_owned());
    }
    let slot = session.client.query_opt(
        "SELECT plugin, database, restart_lsn::text, confirmed_flush_lsn::text FROM pg_catalog.pg_replication_slots WHERE slot_name = $1",
        &[&capture.slot],
    ).await?;
    if let Some(row) = slot {
        slot_present = true;
        let plugin: String = row.get(0);
        let database: String = row.get(1);
        slot_restart = row.get::<_, Option<String>>(2).unwrap_or_default();
        slot_flush = row.get::<_, Option<String>>(3).unwrap_or_default();
        if plugin != "pgoutput" {
            blocks.push("slot_plugin".to_owned());
        }
        if database != config.source.database {
            blocks.push("slot_database".to_owned());
        }
        if slot_restart.is_empty() {
            blocks.push("slot_restart_lsn".to_owned());
        }
    } else {
        blocks.push("slot_absent".to_owned());
    }
    let role = session.client.query_opt(
        "SELECT rolreplication, rolsuper, rolcanlogin FROM pg_catalog.pg_roles WHERE rolname = $1",
        &[&capture.username],
    ).await?;
    if let Some(row) = role {
        let replication: bool = row.get(0);
        let superuser: bool = row.get(1);
        let login: bool = row.get(2);
        if !replication || !login {
            blocks.push("cdc_role".to_owned());
        }
        if superuser {
            blocks.push("cdc_superuser".to_owned());
        }
    } else {
        blocks.push("cdc_role_absent".to_owned());
    }
    let mut tables = Vec::new();
    let catalog_ready = !blocks.iter().any(|item| {
        matches!(item.as_str(), "wal_level" | "publication_absent" | "publication_all_tables" | "publication_members" | "slot_absent" | "slot_plugin" | "slot_database" | "server_version")
    });
    if catalog_ready {
        for object in &config.approved_objects {
            tables.push(inspect_table(&session.client, config, object.schema.as_str(), object.table.as_str(), &mut blocks, &mut warnings).await?);
        }
    }
    Ok(Inspection {
        blocks,
        warnings,
        checklist: checklist(config, capture),
        wal_level,
        keep_size,
        server_version,
        slot_present,
        slot_restart,
        slot_flush,
        tables,
    })
}

async fn inspect_table(
    client: &impl GenericClient,
    config: &Config,
    schema: &str,
    table: &str,
    blocks: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> Result<TablePlan, tokio_postgres::Error> {
    let replica_row = client.query_opt(
        "SELECT c.relreplident::text FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1 AND c.relname = $2",
        &[&schema, &table],
    ).await?;
    let replica = replica_row.map(|row| row.get::<_, String>(0)).unwrap_or_default();
    if replica == "n" {
        blocks.push(format!("replica_identity:{schema}.{table}"));
    } else if replica == "d" {
        warnings.push(format!("toast_default_identity:{schema}.{table}"));
    } else if replica != "f" && replica != "i" {
        blocks.push(format!("replica_identity:{schema}.{table}"));
    }
    let keys = client.query(
        "SELECT a.attname, t.typname FROM pg_catalog.pg_index i JOIN pg_catalog.pg_class c ON c.oid = i.indrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = ANY (i.indkey) JOIN pg_catalog.pg_type t ON t.oid = a.atttypid WHERE i.indisprimary AND n.nspname = $1 AND c.relname = $2 ORDER BY pg_catalog.array_position(i.indkey, a.attnum)",
        &[&schema, &table],
    ).await?;
    let mut pk = String::new();
    if keys.len() != 1 {
        blocks.push(format!("primary_key:{schema}.{table}"));
    } else {
        pk = keys[0].get(0);
        let typname: String = keys[0].get(1);
        if typname != "int8" {
            blocks.push(format!("primary_key_type:{schema}.{table}"));
        }
    }
    let rows = client.query(
        "SELECT a.attname, a.atttypid, pg_catalog.format_type(a.atttypid, a.atttypmod), t.typname FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid = a.attrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace JOIN pg_catalog.pg_type t ON t.oid = a.atttypid WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum",
        &[&schema, &table],
    ).await?;
    let mut columns = Vec::new();
    let mut excluded = Vec::new();
    for row in rows {
        let name: String = row.get(0);
        let oid = i64::from(row.get::<_, u32>(1));
        let format: String = row.get(2);
        let typname: String = row.get(3);
        let approved = config.approved_fields.iter().any(|field| field.schema == schema && field.table == table && field.column == name);
        if !approved {
            excluded.push(name);
            continue;
        }
        if !type_allowed(&typname) || !format_type_allowed(&format) || !ident_ok(&name) {
            blocks.push(format!("column_type:{schema}.{table}.{name}"));
            continue;
        }
        columns.push(StoredCol { name, format_type: format, type_oid: oid, published: true });
    }
    if !pk.is_empty() && !columns.iter().any(|col| col.name == pk) {
        blocks.push(format!("primary_key_unapproved:{schema}.{table}"));
    }
    Ok(TablePlan { schema: schema.to_owned(), table: table.to_owned(), pk, columns, excluded, replica })
}

fn checklist(config: &Config, capture: &Capture) -> Vec<String> {
    let tables = config.approved_objects.iter().map(|object| format!("{}.{}", object.schema, object.table)).collect::<Vec<_>>().join(", ");
    vec![
        "ALTER SYSTEM SET wal_level = logical".to_owned(),
        "ALTER SYSTEM SET max_slot_wal_keep_size = '1GB'".to_owned(),
        "Restart the existing source process. Do not recreate the data directory.".to_owned(),
        format!("CREATE ROLE {} WITH LOGIN REPLICATION NOSUPERUSER PASSWORD '<set by DBA>'", capture.username),
        format!("GRANT CONNECT ON DATABASE {} TO {}", config.source.database, capture.username),
        format!("GRANT USAGE ON SCHEMA public, customer_east TO {}", capture.username),
        format!("GRANT SELECT ON {tables} TO {}", capture.username),
        format!("GRANT EXECUTE ON FUNCTION pg_catalog.pg_current_wal_lsn() TO {}", capture.username),
        format!("CREATE PUBLICATION {} FOR TABLE {tables}", capture.publication),
        format!("SET ROLE {}; SELECT pg_catalog.pg_create_logical_replication_slot('{}', 'pgoutput')", capture.username, capture.slot),
    ]
}

async fn read_live(session: &Session, source_id: &str) -> Result<Option<u64>, tokio_postgres::Error> {
    let row = session.client.query_opt("SELECT generation FROM oo_capture.live WHERE source_id = $1", &[&source_id]).await?;
    Ok(row.map(|row| u64::try_from(row.get::<_, i64>(0)).unwrap_or(0)))
}

async fn run_sync(
    config: &Config,
    capture: &Capture,
    inspection: &Inspection,
    ends: &mut Ends<'_>,
    mut body: Value,
    live: Option<u64>,
) -> Report {
    let generation = i64::try_from(capture.generation).unwrap_or(0);
    if generation <= 0 {
        return Report { body, error: Some(Fail::new("capture.refused", 4)) };
    }
    if let Some(current) = live {
        if capture.generation + 1 < current || (capture.generation > current + 1) {
            body["blocks"] = json!(["generation_gap"]);
            return Report { body, error: Some(Fail::new("capture.refused", 4)) };
        }
        if capture.generation < current {
            body["blocks"] = json!(["stale_generation"]);
            return Report { body, error: Some(Fail::new("capture.refused", 4)) };
        }
    }
    let mut applied = match load_build(&ends.target.client, &config.source_id, generation).await {
        Ok(value) => value,
        Err(err) => return db_report(body, err, "capture.apply", 4),
    };
    let catch_up = live == Some(capture.generation) && applied.phase == "live";
    if !catch_up && live.is_some() && live != Some(capture.generation) && applied.phase.is_empty() {
        applied.phase = "snapshot".to_owned();
    }
    if applied.phase.is_empty() {
        applied.phase = if catch_up { "live".to_owned() } else { "snapshot".to_owned() };
    }
    if applied.l0.is_empty() {
        applied.l0 = inspection.slot_restart.clone();
        if let Err(fail) = parse_lsn(&applied.l0) {
            body["blocks"] = json!(["slot_restart_lsn"]);
            return Report { body, error: Some(fail) };
        }
    }
    if let Err(err) = save_build(&ends.target.client, &config.source_id, generation, &applied).await {
        return db_report(body, err, "capture.apply", 4);
    }
    body["l0"] = json!(applied.l0);
    body["phase"] = json!(applied.phase);
    let mut plans = inspection.tables.clone();
    if let Err(err) = ensure_relations(ends.target, config, capture, &mut plans).await {
        return step_report(body, err, "capture.apply", 4);
    }
    let mut rows_filled = 0u64;
    let mut arrow = Vec::new();
    let mut paused = Vec::new();
    if !catch_up {
        let mut total = 0u64;
        for plan in &plans {
            let sql = match count_sql(&plan.schema, &plan.table) {
                Ok(sql) => sql,
                Err(fail) => return Report { body, error: Some(fail) },
            };
            body["business_read"] = json!(true);
            let row = match ends.source.client.query_one(&sql, &[]).await {
                Ok(row) => row,
                Err(err) => return db_report(body, err, "capture.apply", 4),
            };
            let count: i64 = row.get(0);
            if count < 0 {
                return Report { body, error: Some(Fail::new("capture.apply", 4)) };
            }
            total += u64::try_from(count).unwrap_or(u64::MAX);
        }
        body["source_count"] = json!(total);
        if total > config.budget.max_rows {
            body["blocks"] = json!(["budget"]);
            return Report { body, error: Some(Fail::new("capture.budget", 5)) };
        }
        let limit = match chunk_limit() {
            Ok(limit) => limit,
            Err(fail) => return Report { body, error: Some(fail) },
        };
        let mut chunks = 0u32;
        let mut scratch = CaptureOut { applied: &mut applied, arrow: &mut arrow, paused: &mut paused };
        for plan in &plans {
            let outcome = match snapshot_table(config, capture, plan, ends, &mut scratch, &mut chunks, limit).await {
                Ok(outcome) => outcome,
                Err(err) => return step_report(body, err, "capture.apply", 4),
            };
            rows_filled += outcome.rows;
            if !outcome.done {
                body["phase"] = json!("interrupted");
                body["rows_filled"] = json!(rows_filled);
                body["arrow"] = json!(&*scratch.arrow);
                body["l0"] = json!(scratch.applied.l0);
                body["applied_lsn"] = json!(scratch.applied.applied);
                body["chunks_completed"] = json!(chunks);
                return Report { body, error: Some(Fail::new("capture.interrupted", 75)) };
            }
        }
    }
    let l_end = if catch_up && !applied.l_end.is_empty() {
        applied.l_end.clone()
    } else {
        match ends.source.client.query_one("SELECT pg_catalog.pg_current_wal_lsn()::text", &[]).await {
            Ok(row) => row.get(0),
            Err(err) => return db_report(body, err, "capture.apply", 4),
        }
    };
    applied.l_end = l_end.clone();
    let drained = {
        let mut scratch = CaptureOut { applied: &mut applied, arrow: &mut arrow, paused: &mut paused };
        match drain(config, capture, &plans, ends, &mut scratch, true).await {
            Ok(drained) => drained,
            Err(err) => return step_report(body, err, "capture.apply", 4),
        }
    };
    if drained.reason == "large_txn" {
        body["phase"] = json!("large_txn");
        body["pending_messages"] = json!(drained.pending);
        body["l_end"] = json!(l_end);
        body["applied_lsn"] = json!(applied.applied);
        return Report { body, error: Some(Fail::new("capture.large_txn", 4)) };
    }
    if drained.pending > 0 {
        body["phase"] = json!("open_transaction");
        body["pending_messages"] = json!(drained.pending);
        body["l_end"] = json!(l_end);
        body["applied_lsn"] = json!(applied.applied);
        return Report { body, error: Some(Fail::new("capture.blocked", 5)) };
    }
    let l_end_n = match parse_lsn(&l_end) {
        Ok(value) => value,
        Err(fail) => return Report { body, error: Some(fail) },
    };
    let applied_n = if applied.applied.is_empty() { 0 } else { match parse_lsn(&applied.applied) {
        Ok(value) => value,
        Err(fail) => return Report { body, error: Some(fail) },
    } };
    let mut reason = drained.reason;
    if lsn_advance(applied_n, l_end_n).is_some() && drained.txns == 0 {
        if let Err(err) = advance_slot(&ends.advance.client, &capture.slot, &l_end).await {
            return db_report(body, err, "capture.apply", 4);
        }
        applied.applied = l_end.clone();
        reason = "caught_up_empty".to_owned();
    }
    if !paused.is_empty() && live != Some(capture.generation) {
        applied.phase = "paused".to_owned();
        applied.reason = reason.clone();
        if let Err(err) = save_build(&ends.target.client, &config.source_id, generation, &applied).await {
            return db_report(body, err, "capture.apply", 4);
        }
        body["phase"] = json!("paused");
        body["paused"] = json!(paused);
        body["reason"] = json!(reason);
        return Report { body, error: Some(Fail::new("capture.schema_paused", 10)) };
    }
    if let Err(err) = publish(config, capture, ends.target, &plans, &paused, &applied, &reason).await {
        return step_report(body, err, "capture.apply", 4);
    }
    if !paused.is_empty() {
        body["phase"] = json!("paused");
        body["paused"] = json!(paused);
        body["cross_table_consistent"] = json!(false);
        body["reason"] = json!(reason);
        return Report { body, error: Some(Fail::new("capture.schema_paused", 10)) };
    }
    body["phase"] = json!("live");
    body["l0"] = json!(applied.l0);
    body["l_end"] = json!(applied.l_end);
    body["applied_lsn"] = json!(applied.applied);
    body["reason"] = json!(reason);
    body["rows_filled"] = json!(rows_filled);
    body["cdc_txns"] = json!(drained.txns);
    body["cdc_rows"] = json!(drained.rows);
    body["toast_unchanged"] = json!(drained.toast);
    body["slot_advances"] = json!(drained.advances);
    body["pending_messages"] = json!(0);
    body["arrow"] = json!(arrow);
    body["paused"] = json!(paused);
    body["cross_table_consistent"] = json!(true);
    body["business_read"] = json!(true);
    Report { body, error: None }
}

#[derive(Clone)]
struct BuildState {
    phase: String,
    l0: String,
    l_end: String,
    applied: String,
    reason: String,
}

async fn load_build(client: &impl GenericClient, source_id: &str, generation: i64) -> Result<BuildState, tokio_postgres::Error> {
    let row = client.query_opt(
        "SELECT phase, COALESCE(l0, ''), COALESCE(l_end, ''), COALESCE(applied_lsn, ''), reason FROM oo_capture.build WHERE source_id = $1 AND generation = $2",
        &[&source_id, &generation],
    ).await?;
    Ok(match row {
        None => BuildState { phase: String::new(), l0: String::new(), l_end: String::new(), applied: String::new(), reason: String::new() },
        Some(row) => BuildState { phase: row.get(0), l0: row.get(1), l_end: row.get(2), applied: row.get(3), reason: row.get(4) },
    })
}

async fn save_build(client: &impl GenericClient, source_id: &str, generation: i64, state: &BuildState) -> Result<(), tokio_postgres::Error> {
    client.execute(
        "INSERT INTO oo_capture.build (source_id, generation, phase, l0, l_end, applied_lsn, reason) VALUES ($1, $2, $3, NULLIF($4, ''), NULLIF($5, ''), NULLIF($6, ''), $7) ON CONFLICT (source_id, generation) DO UPDATE SET phase = EXCLUDED.phase, l0 = COALESCE(oo_capture.build.l0, EXCLUDED.l0), l_end = COALESCE(EXCLUDED.l_end, oo_capture.build.l_end), applied_lsn = COALESCE(EXCLUDED.applied_lsn, oo_capture.build.applied_lsn), reason = EXCLUDED.reason",
        &[&source_id, &generation, &state.phase, &state.l0, &state.l_end, &state.applied, &state.reason],
    ).await?;
    Ok(())
}

async fn ensure_relations(target: &mut Session, config: &Config, capture: &Capture, plans: &mut [TablePlan]) -> Result<(), StepErr> {
    let generation = i64::try_from(capture.generation).unwrap_or(0);
    for plan in plans.iter_mut() {
        let rel_key = format!("{}.{}", plan.schema, plan.table);
        let existing = target.client.query_opt(
            "SELECT physical, columns_json, excluded_json, paused, pause_reason FROM oo_capture.relmap WHERE source_id = $1 AND generation = $2 AND rel_key = $3",
            &[&config.source_id, &generation, &rel_key],
        ).await?;
        if let Some(row) = existing {
            let columns_json: String = row.get(1);
            let stored: Vec<StoredCol> = serde_json::from_str(&columns_json).unwrap_or_default();
            if !stored.is_empty() {
                plan.columns = stored;
            }
            let excluded_json: String = row.get(2);
            plan.excluded = serde_json::from_str(&excluded_json).unwrap_or_default();
            continue;
        }
        let physical = physical_name(&config.source_id, capture.generation, &plan.schema, &plan.table);
        let ddl = create_table_sql(&physical, &plan.pk, &plan.columns).map_err(StepErr::Rule)?;
        target.client.batch_execute(&ddl).await?;
        let columns_json = serde_json::to_string(&plan.columns).unwrap_or_else(|_| "[]".to_owned());
        let excluded_json = serde_json::to_string(&plan.excluded).unwrap_or_else(|_| "[]".to_owned());
        target.client.execute(
            "INSERT INTO oo_capture.relmap (source_id, generation, rel_key, physical, pk_name, columns_json, excluded_json) VALUES ($1, $2, $3, $4, $5, $6, $7)",
            &[&config.source_id, &generation, &rel_key, &physical, &plan.pk, &columns_json, &excluded_json],
        ).await?;
        target.client.execute(
            "INSERT INTO oo_capture.progress (source_id, generation, rel_key, last_pk, done, rows) VALUES ($1, $2, $3, NULL, false, 0) ON CONFLICT DO NOTHING",
            &[&config.source_id, &generation, &rel_key],
        ).await?;
    }
    Ok(())
}

fn physical_name(source: &str, generation: u64, schema: &str, table: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    hasher.update([0]);
    hasher.update(generation.to_string().as_bytes());
    hasher.update([0]);
    hasher.update(schema.as_bytes());
    hasher.update([0]);
    hasher.update(table.as_bytes());
    let digest = hasher.finalize();
    let mut name = String::from("c_");
    for byte in digest.iter().take(12) {
        name.push_str(&format!("{byte:02x}"));
    }
    name
}

fn create_table_sql(physical: &str, pk: &str, columns: &[StoredCol]) -> Result<String, Fail> {
    if !ident_ok(physical) || !ident_ok(pk) || columns.is_empty() {
        return Err(Fail::new("capture.refused", 4));
    }
    let mut defs = Vec::new();
    for col in columns {
        if !ident_ok(&col.name) || !format_type_allowed(&col.format_type) {
            return Err(Fail::new("capture.refused", 4));
        }
        defs.push(format!("\"{}\" {}", col.name, col.format_type));
    }
    defs.push(format!("PRIMARY KEY (\"{pk}\")"));
    defs.push("\"_oo_pos\" numeric(20,0) NOT NULL".to_owned());
    defs.push("\"_oo_seq\" bigint NOT NULL".to_owned());
    defs.push("\"_oo_op\" smallint NOT NULL".to_owned());
    defs.push("\"_oo_src_ts\" bigint".to_owned());
    Ok(format!("CREATE TABLE IF NOT EXISTS oo_capture.\"{physical}\" ({})", defs.join(", ")))
}

struct SnapOut {
    rows: u64,
    done: bool,
}

async fn snapshot_table(
    config: &Config,
    capture: &Capture,
    plan: &TablePlan,
    ends: &mut Ends<'_>,
    out: &mut CaptureOut<'_>,
    chunks_done: &mut u32,
    limit: Option<u32>,
) -> Result<SnapOut, StepErr> {
    let generation = i64::try_from(capture.generation).unwrap_or(0);
    let rel_key = format!("{}.{}", plan.schema, plan.table);
    let mut rows_total = 0u64;
    loop {
        let drained = drain(config, capture, std::slice::from_ref(plan), ends, out, false).await?;
        if drained.reason == "large_txn" {
            return Err(StepErr::Rule(Fail::new("capture.large_txn", 4)));
        }
        let progress = ends.target.client.query_one(
            "SELECT COALESCE(last_pk, ''), done, rows FROM oo_capture.progress WHERE source_id = $1 AND generation = $2 AND rel_key = $3",
            &[&config.source_id, &generation, &rel_key],
        ).await?;
        let last_pk: String = progress.get(0);
        let done: bool = progress.get(1);
        if done {
            return Ok(SnapOut { rows: rows_total, done: true });
        }
        let truncated = ends.target.client.query_opt(
            "SELECT 1 FROM oo_capture.truncated WHERE source_id = $1 AND generation = $2 AND rel_key = $3",
            &[&config.source_id, &generation, &rel_key],
        ).await?;
        if truncated.is_some() {
            ends.target.client.execute(
                "UPDATE oo_capture.progress SET done = true WHERE source_id = $1 AND generation = $2 AND rel_key = $3",
                &[&config.source_id, &generation, &rel_key],
            ).await?;
            return Ok(SnapOut { rows: rows_total, done: true });
        }
        let resumed = !last_pk.is_empty();
        let sql = read_sql(&plan.schema, &plan.table, &plan.columns, &plan.pk, capture.window_rows, resumed).map_err(StepErr::Rule)?;
        let last_value = if resumed {
            last_pk.parse::<i64>().map_err(|_| StepErr::Rule(Fail::new("capture.apply", 4)))?
        } else {
            0
        };
        let rows = if resumed {
            ends.source.client.query(&sql, &[&last_value]).await?
        } else {
            ends.source.client.query(&sql, &[]).await?
        };
        if rows.is_empty() {
            ends.target.client.execute(
                "UPDATE oo_capture.progress SET done = true WHERE source_id = $1 AND generation = $2 AND rel_key = $3",
                &[&config.source_id, &generation, &rel_key],
            ).await?;
            return Ok(SnapOut { rows: rows_total, done: true });
        }
        let pk_index = plan.columns.iter().position(|col| col.name == plan.pk).ok_or(StepErr::Rule(Fail::new("capture.apply", 4)))?;
        let mut page = Vec::new();
        for row in &rows {
            let mut values = Vec::new();
            for index in 0..plan.columns.len() {
                values.push(row.try_get(index)?);
            }
            let pk = values.get(pk_index).cloned().flatten().ok_or(StepErr::Rule(Fail::new("capture.apply", 4)))?;
            page.push(ArrowRow { pk, op: 3, pos: "0".to_owned(), seq: 0, src_ts: None, unavail: String::new(), values, extras: Vec::new() });
        }
        let bytes = write_arrow(&plan.schema, &plan.table, capture.generation, "snapshot", &plan.columns, &page).map_err(StepErr::Rule)?;
        let new_last = page.last().map(|row| row.pk.clone()).unwrap_or_default();
        let path = arrow_path(config, capture.generation, &rel_key, *chunks_done, &new_last);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|_| StepErr::Rule(Fail::new("capture.apply", 4)))?;
        }
        fs::write(&path, &bytes).map_err(|_| StepErr::Rule(Fail::new("capture.apply", 4)))?;
        let loaded = read_arrow(&bytes).map_err(StepErr::Rule)?;
        apply_fill(ends.target, config, capture, plan, &loaded).await?;
        let added = u64::try_from(page.len()).unwrap_or(0);
        rows_total += added;
        ends.target.client.execute(
            "UPDATE oo_capture.progress SET last_pk = $4, rows = rows + $5 WHERE source_id = $1 AND generation = $2 AND rel_key = $3",
            &[&config.source_id, &generation, &rel_key, &new_last, &i64::try_from(added).unwrap_or(0)],
        ).await?;
        out.arrow.push(json!({"path": path.display().to_string(), "sha256": sha256_bytes(&bytes), "rows": page.len(), "kind": "snapshot"}));
        *chunks_done += 1;
        let short = u64::try_from(rows.len()).unwrap_or(0) < capture.window_rows;
        if short {
            ends.target.client.execute(
                "UPDATE oo_capture.progress SET done = true WHERE source_id = $1 AND generation = $2 AND rel_key = $3",
                &[&config.source_id, &generation, &rel_key],
            ).await?;
            return Ok(SnapOut { rows: rows_total, done: true });
        }
        if limit.is_some_and(|limit| *chunks_done >= limit) {
            return Ok(SnapOut { rows: rows_total, done: false });
        }
    }
}

fn read_sql(schema: &str, table: &str, columns: &[StoredCol], pk: &str, window: u64, resumed: bool) -> Result<String, Fail> {
    if !ident_ok(schema) || !ident_ok(table) || !ident_ok(pk) || !(1..=5_000).contains(&window) {
        return Err(Fail::new("capture.refused", 4));
    }
    let mut terms = Vec::new();
    for col in columns {
        if !ident_ok(&col.name) {
            return Err(Fail::new("capture.refused", 4));
        }
        terms.push(format!("\"{}\"::text", col.name));
    }
    let filter = if resumed {
        format!(" WHERE \"{schema}\".\"{table}\".\"{pk}\" > $1")
    } else {
        String::new()
    };
    Ok(format!(
        "SELECT {} FROM \"{schema}\".\"{table}\"{filter} ORDER BY \"{schema}\".\"{table}\".\"{pk}\" LIMIT {window}",
        terms.join(", ")
    ))
}

fn count_sql(schema: &str, table: &str) -> Result<String, Fail> {
    if !ident_ok(schema) || !ident_ok(table) {
        return Err(Fail::new("capture.refused", 4));
    }
    Ok(format!("SELECT count(*) FROM \"{schema}\".\"{table}\""))
}

fn arrow_path(config: &Config, generation: u64, rel_key: &str, chunk: u32, pk: &str) -> std::path::PathBuf {
    let safe_pk: String = pk.chars().filter(|ch| ch.is_ascii_digit()).collect();
    Path::new(&config.out_dir).join("capture-arrow").join(format!("g{generation}")).join(rel_key).join(format!("{chunk:06}-{safe_pk}.arrow"))
}

async fn apply_fill(target: &mut Session, config: &Config, capture: &Capture, plan: &TablePlan, rows: &[ArrowRow]) -> Result<(), StepErr> {
    let physical = physical_name(&config.source_id, capture.generation, &plan.schema, &plan.table);
    let rel_key = format!("{}.{}", plan.schema, plan.table);
    let generation = i64::try_from(capture.generation).unwrap_or(0);
    let mut offset = 0;
    while offset < rows.len() {
        let end = (offset + 40).min(rows.len());
        let batch = &rows[offset..end];
        let sql = fill_statement(&physical, &plan.columns, &plan.pk, batch.len()).map_err(StepErr::Rule)?;
        let mut owned = Vec::new();
        for row in batch {
            if row.values.len() != plan.columns.len() {
                return Err(StepErr::Rule(Fail::new("capture.apply", 4)));
            }
            for value in &row.values {
                owned.push(value.clone());
            }
        }
        let mut params: Vec<&(dyn ToSql + Sync)> = owned.iter().map(|value| value as &(dyn ToSql + Sync)).collect();
        let source_id = config.source_id.clone();
        params.push(&source_id);
        params.push(&generation);
        params.push(&rel_key);
        let tx = target.client.transaction().await?;
        tx.execute(&sql, &params).await?;
        tx.commit().await?;
        offset = end;
    }
    Ok(())
}

fn as_text_then(index: usize, ty: &str) -> String {
    format!("CAST(CAST(${index} AS text) AS {ty})")
}

fn fill_statement(physical: &str, columns: &[StoredCol], pk: &str, rows: usize) -> Result<String, Fail> {
    if rows == 0 || !ident_ok(physical) || !ident_ok(pk) {
        return Err(Fail::new("capture.apply", 4));
    }
    let mut names = Vec::new();
    for col in columns {
        if !ident_ok(&col.name) || !format_type_allowed(&col.format_type) {
            return Err(Fail::new("capture.apply", 4));
        }
        names.push(format!("\"{}\"", col.name));
    }
    let width = columns.len();
    let mut groups = Vec::new();
    for row in 0..rows {
        let mut cells = Vec::new();
        for (col, column) in columns.iter().enumerate() {
            let index = row * width + col + 1;
            cells.push(as_text_then(index, &column.format_type));
        }
        groups.push(format!("({})", cells.join(", ")));
    }
    let base = rows * width;
    let select_cols = names.iter().map(|name| format!("v.{name}")).collect::<Vec<_>>().join(", ");
    let col_list = names.join(", ");
    Ok(format!(
        "INSERT INTO oo_capture.\"{physical}\" ({col_list}, \"_oo_pos\", \"_oo_seq\", \"_oo_op\", \"_oo_src_ts\") SELECT {select_cols}, 0, 0, 3, NULL FROM (VALUES {}) AS v({col_list}) WHERE NOT EXISTS (SELECT 1 FROM oo_capture.tombstone t WHERE t.source_id = ${} AND t.generation = ${} AND t.rel_key = ${} AND t.pk = v.\"{pk}\"::text) AND NOT EXISTS (SELECT 1 FROM oo_capture.truncated tr WHERE tr.source_id = ${} AND tr.generation = ${} AND tr.rel_key = ${}) ON CONFLICT (\"{pk}\") DO NOTHING",
        groups.join(", "),
        base + 1,
        base + 2,
        base + 3,
        base + 1,
        base + 2,
        base + 3
    ))
}

struct DrainOut {
    txns: u64,
    rows: u64,
    toast: u64,
    advances: u64,
    pending: u64,
    reason: String,
}

async fn drain(
    config: &Config,
    capture: &Capture,
    plans: &[TablePlan],
    ends: &mut Ends<'_>,
    scratch: &mut CaptureOut<'_>,
    until_quiet: bool,
) -> Result<DrainOut, StepErr> {
    let mut out = DrainOut { txns: 0, rows: 0, toast: 0, advances: 0, pending: 0, reason: "drained".to_owned() };
    let mut limit: i32 = 1024;
    loop {
        let sql = peek_sql(&capture.slot, &capture.publication, limit);
        if selects_business_table(&sql) {
            return Err(StepErr::Rule(Fail::new("capture.refused", 4)));
        }
        let rows = ends.source.client.query(&sql, &[]).await?;
        if rows.is_empty() {
            out.reason = if out.txns == 0 { "caught_up_empty".to_owned() } else { "drained".to_owned() };
            return Ok(out);
        }
        let mut messages = Vec::new();
        for row in &rows {
            let data: Vec<u8> = row.get(1);
            match decode_message(&data) {
                Ok(message) => messages.push(message),
                Err(fail) => return Err(StepErr::Rule(fail)),
            }
        }
        let assembled = assemble(&messages).map_err(StepErr::Rule)?;
        for txn in assembled.txns {
            let effect = apply_txn(config, capture, plans, ends.target, &txn, scratch.arrow, scratch.paused).await?;
            out.txns += 1;
            out.rows += effect.rows;
            out.toast += effect.toast;
            let current = if scratch.applied.applied.is_empty() { 0 } else { parse_lsn(&scratch.applied.applied).map_err(StepErr::Rule)? };
            if let Some(next) = lsn_advance(current, txn.end_lsn) {
                let text = format_lsn(next);
                advance_slot(&ends.advance.client, &capture.slot, &text).await?;
                scratch.applied.applied = text;
                out.advances += 1;
                let generation = i64::try_from(capture.generation).unwrap_or(0);
                save_build(&ends.target.client, &config.source_id, generation, scratch.applied).await?;
            }
        }
        if assembled.pending > 0 {
            if rows.len() < usize::try_from(limit).unwrap_or(usize::MAX) {
                out.pending = assembled.pending;
                out.reason = "open_transaction".to_owned();
                return Ok(out);
            }
            if limit >= 50_000 {
                out.pending = assembled.pending;
                out.reason = "large_txn".to_owned();
                return Ok(out);
            }
            limit = limit.saturating_mul(2).min(50_000);
            continue;
        }
        if !until_quiet || rows.len() < usize::try_from(limit).unwrap_or(usize::MAX) {
            out.reason = if out.txns == 0 { "caught_up_empty".to_owned() } else { "drained".to_owned() };
            return Ok(out);
        }
    }
}

pub(crate) fn peek_sql(slot: &str, publication: &str, limit: i32) -> String {
    format!(
        "SELECT lsn::text, data FROM pg_catalog.pg_logical_slot_peek_binary_changes('{slot}', NULL, {limit}, 'proto_version', '1', 'publication_names', '{publication}')"
    )
}

async fn advance_slot(client: &impl GenericClient, slot: &str, lsn: &str) -> Result<(), tokio_postgres::Error> {
    client.query_one(
        "SELECT pg_catalog.pg_replication_slot_advance($1, CAST(CAST($2 AS text) AS pg_lsn))::text",
        &[&slot, &lsn],
    ).await?;
    Ok(())
}

struct ApplyEffect {
    rows: u64,
    toast: u64,
}

async fn apply_txn(
    config: &Config,
    capture: &Capture,
    plans: &[TablePlan],
    target: &mut Session,
    txn: &ReadyTxn,
    arrow: &mut Vec<Value>,
    paused: &mut Vec<String>,
) -> Result<ApplyEffect, StepErr> {
    let mut effect = ApplyEffect { rows: 0, toast: 0 };
    let tx = target.client.transaction().await?;
    let mut rows_for_arrow: Vec<(String, Vec<StoredCol>, Vec<ArrowRow>)> = Vec::new();
    for event in &txn.events {
        let Some(plan) = plans.iter().find(|plan| plan.schema == event_schema(event) && plan.table == event_table(event)) else {
            continue;
        };
        let rel_key = format!("{}.{}", plan.schema, plan.table);
        if paused.contains(&rel_key) {
            continue;
        }
        match event {
            TxnEvent::Truncate { rel: _, seq } => {
                let physical = physical_name(&config.source_id, capture.generation, &plan.schema, &plan.table);
                let pos = txn.commit_lsn.to_string();
                let generation = i64::try_from(capture.generation).unwrap_or(0);
                let rel_key = format!("{}.{}", plan.schema, plan.table);
                tx.execute(
                    &format!("DELETE FROM oo_capture.\"{physical}\" WHERE \"_oo_pos\" < {} OR (\"_oo_pos\" = {} AND \"_oo_seq\" < $2)", as_text_then(1, "numeric"), as_text_then(1, "numeric")),
                    &[&pos, seq],
                ).await?;
                tx.execute(
                    "INSERT INTO oo_capture.truncated (source_id, generation, rel_key, pos) VALUES ($1, $2, $3, CAST(CAST($4 AS text) AS numeric)) ON CONFLICT (source_id, generation, rel_key) DO UPDATE SET pos = EXCLUDED.pos WHERE oo_capture.truncated.pos < EXCLUDED.pos",
                    &[&config.source_id, &generation, &rel_key, &pos],
                ).await?;
            }
            TxnEvent::Delete { rel, cells, seq } => {
                let Some(pk) = pk_text(plan, rel, cells) else {
                    mark_paused(&tx, config, capture, plan, "delete_key", txn.commit_lsn).await?;
                    paused.push(rel_key);
                    continue;
                };
                let physical = physical_name(&config.source_id, capture.generation, &plan.schema, &plan.table);
                let pos = txn.commit_lsn.to_string();
                let generation = i64::try_from(capture.generation).unwrap_or(0);
                let rel_key = format!("{}.{}", plan.schema, plan.table);
                if !ident_ok(&plan.pk) || !ident_ok(&physical) {
                    return Err(StepErr::Rule(Fail::new("capture.apply", 4)));
                }
                let pk_value = pk.parse::<i64>().map_err(|_| StepErr::Rule(Fail::new("capture.apply", 4)))?;
                tx.execute(
                    &format!("DELETE FROM oo_capture.\"{physical}\" WHERE \"{}\" = $1 AND (\"_oo_pos\" < {} OR (\"_oo_pos\" = {} AND \"_oo_seq\" < $3))", plan.pk, as_text_then(2, "numeric"), as_text_then(2, "numeric")),
                    &[&pk_value, &pos, seq],
                ).await?;
                tx.execute(
                    "INSERT INTO oo_capture.tombstone (source_id, generation, rel_key, pk, pos) VALUES ($1, $2, $3, $4, CAST(CAST($5 AS text) AS numeric)) ON CONFLICT DO NOTHING",
                    &[&config.source_id, &generation, &rel_key, &pk, &pos],
                ).await?;
                effect.rows += 1;
            }
            TxnEvent::Upsert { rel, cells, seq } => {
                let change = match align_upsert(plan, rel, cells) {
                    Align::Ready(change) => change,
                    Align::Pause(reason) => {
                        mark_paused(&tx, config, capture, plan, reason, txn.commit_lsn).await?;
                        paused.push(rel_key);
                        continue;
                    }
                    Align::NewColumn(change) => {
                        add_unmapped(&tx, config, capture, plan, rel, txn.commit_lsn).await?;
                        change
                    }
                };
                effect.toast += u64::try_from(change.unavail.split(',').filter(|item| !item.is_empty()).count()).unwrap_or(0);
                let mut row = change;
                row.op = 1;
                row.pos = txn.commit_lsn.to_string();
                row.seq = *seq;
                row.src_ts = Some(txn.commit_us);
                rows_for_arrow.push((format!("{}.{}", plan.schema, plan.table), plan.columns.clone(), vec![row]));
                effect.rows += 1;
            }
        }
    }
    for (rel_key, columns, rows) in &rows_for_arrow {
        let Some(plan) = plans.iter().find(|plan| format!("{}.{}", plan.schema, plan.table) == *rel_key) else {
            continue;
        };
        let bytes = write_arrow(&plan.schema, &plan.table, capture.generation, "cdc", columns, rows).map_err(StepErr::Rule)?;
        let loaded = read_arrow(&bytes).map_err(StepErr::Rule)?;
        for row in &loaded {
            apply_upsert(&tx, config, capture, plan, row).await?;
        }
        let pk = rows.first().map(|row| row.pk.as_str()).unwrap_or("0");
        let path = arrow_path(config, capture.generation, rel_key, u32::try_from(arrow.len()).unwrap_or(0), pk);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|_| StepErr::Rule(Fail::new("capture.apply", 4)))?;
        }
        fs::write(&path, &bytes).map_err(|_| StepErr::Rule(Fail::new("capture.apply", 4)))?;
        arrow.push(json!({"path": path.display().to_string(), "sha256": sha256_bytes(&bytes), "rows": rows.len(), "kind": "cdc"}));
    }
    tx.commit().await?;
    Ok(effect)
}

fn event_schema(event: &TxnEvent) -> &str {
    match event {
        TxnEvent::Upsert { rel, .. } | TxnEvent::Delete { rel, .. } | TxnEvent::Truncate { rel, .. } => rel.namespace.as_str(),
    }
}

fn event_table(event: &TxnEvent) -> &str {
    match event {
        TxnEvent::Upsert { rel, .. } | TxnEvent::Delete { rel, .. } | TxnEvent::Truncate { rel, .. } => rel.name.as_str(),
    }
}

fn pk_text(plan: &TablePlan, rel: &Rel, cells: &[Cell]) -> Option<String> {
    let index = rel.columns.iter().position(|col| col.name == plan.pk)?;
    let cell = cells.get(index)?;
    if cell.kind != CellKind::Value {
        return None;
    }
    cell.text.clone()
}

enum Align {
    Ready(ArrowRow),
    NewColumn(ArrowRow),
    Pause(&'static str),
}

fn align_upsert(plan: &TablePlan, rel: &Rel, cells: &[Cell]) -> Align {
    if cells.len() != rel.columns.len() {
        return Align::Pause("tuple_width");
    }
    let mut values = vec![None; plan.columns.len()];
    let mut unavail = Vec::new();
    let mut extras = Vec::new();
    let mut published = 0usize;
    for (index, rel_col) in rel.columns.iter().enumerate() {
        let cell = &cells[index];
        if plan.excluded.contains(&rel_col.name) {
            continue;
        }
        if let Some(column) = plan.columns.get(published)
            && column.name == rel_col.name
        {
            if column.type_oid != rel_col.type_oid {
                return Align::Pause("schema");
            }
            match cell.kind {
                CellKind::Unchanged => unavail.push(column.name.clone()),
                CellKind::Null => {}
                CellKind::Value => values[published] = cell.text.clone(),
            }
            published += 1;
            continue;
        }
        if plan.columns.iter().any(|column| column.name == rel_col.name) || !ident_ok(&rel_col.name) {
            return Align::Pause("schema");
        }
        match cell.kind {
            CellKind::Unchanged => {
                extras.push((rel_col.name.clone(), None));
                unavail.push(rel_col.name.clone());
            }
            CellKind::Null => extras.push((rel_col.name.clone(), None)),
            CellKind::Value => extras.push((rel_col.name.clone(), cell.text.clone())),
        }
    }
    if published != plan.columns.len() {
        return Align::Pause("schema");
    }
    let row = ArrowRow {
        pk: pk_text(plan, rel, cells).unwrap_or_default(),
        op: 1,
        pos: "0".to_owned(),
        seq: 0,
        src_ts: None,
        unavail: unavail.join(","),
        values,
        extras,
    };
    if row.pk.is_empty() {
        return Align::Pause("primary_key");
    }
    if row.extras.is_empty() { Align::Ready(row) } else { Align::NewColumn(row) }
}

async fn mark_paused(
    tx: &tokio_postgres::Transaction<'_>,
    config: &Config,
    capture: &Capture,
    plan: &TablePlan,
    reason: &str,
    pos: u64,
) -> Result<(), tokio_postgres::Error> {
    let generation = i64::try_from(capture.generation).unwrap_or(0);
    let rel_key = format!("{}.{}", plan.schema, plan.table);
    tx.execute(
        "UPDATE oo_capture.relmap SET paused = true, pause_reason = $4 WHERE source_id = $1 AND generation = $2 AND rel_key = $3",
        &[&config.source_id, &generation, &rel_key, &reason],
    ).await?;
    tx.execute(
        "INSERT INTO oo_capture.quarantine (source_id, generation, rel_key, pos, kind, payload) VALUES ($1, $2, $3, CAST(CAST($4 AS text) AS numeric), 'schema', $5)",
        &[&config.source_id, &generation, &rel_key, &pos.to_string(), &reason],
    ).await?;
    Ok(())
}

async fn add_unmapped(
    tx: &tokio_postgres::Transaction<'_>,
    config: &Config,
    capture: &Capture,
    plan: &TablePlan,
    rel: &Rel,
    pos: u64,
) -> Result<(), tokio_postgres::Error> {
    let generation = i64::try_from(capture.generation).unwrap_or(0);
    let rel_key = format!("{}.{}", plan.schema, plan.table);
    let physical = physical_name(&config.source_id, capture.generation, &plan.schema, &plan.table);
    for col in &rel.columns {
        if plan.columns.iter().any(|item| item.name == col.name) || plan.excluded.contains(&col.name) {
            continue;
        }
        if !ident_ok(&col.name) {
            continue;
        }
        tx.execute(
            &format!("ALTER TABLE oo_capture.\"{physical}\" ADD COLUMN IF NOT EXISTS \"{}\" text", col.name),
            &[],
        ).await?;
        tx.execute(
            "INSERT INTO oo_capture.unmapped (source_id, generation, rel_key, column_name, format_type, type_oid, first_pos) VALUES ($1, $2, $3, $4, 'text', $5, CAST(CAST($6 AS text) AS numeric)) ON CONFLICT DO NOTHING",
            &[&config.source_id, &generation, &rel_key, &col.name, &col.type_oid, &pos.to_string()],
        ).await?;
    }
    Ok(())
}

async fn apply_upsert(
    tx: &tokio_postgres::Transaction<'_>,
    config: &Config,
    capture: &Capture,
    plan: &TablePlan,
    row: &ArrowRow,
) -> Result<(), StepErr> {
    let physical = physical_name(&config.source_id, capture.generation, &plan.schema, &plan.table);
    let sql = upsert_sql(&physical, &plan.columns, &plan.pk).map_err(StepErr::Rule)?;
    let owned_values = row.values.clone();
    if owned_values.len() != plan.columns.len() || !ident_ok(&plan.pk) {
        return Err(StepErr::Rule(Fail::new("capture.apply", 4)));
    }
    let pos = row.pos.clone();
    let seq = row.seq;
    let src_ts = row.src_ts;
    let mut flags = Vec::new();
    let unavail: BTreeSet<&str> = row.unavail.split(',').filter(|item| !item.is_empty()).collect();
    for col in &plan.columns {
        flags.push(unavail.contains(col.name.as_str()));
    }
    let mut params: Vec<&(dyn ToSql + Sync)> = Vec::new();
    for value in &owned_values {
        params.push(value);
    }
    params.push(&pos);
    params.push(&seq);
    params.push(&src_ts);
    for flag in &flags {
        params.push(flag);
    }
    tx.execute(&sql, &params).await?;
    let pk_value = row.pk.parse::<i64>().map_err(|_| StepErr::Rule(Fail::new("capture.apply", 4)))?;
    for (name, value) in &row.extras {
        if !ident_ok(name) {
            return Err(StepErr::Rule(Fail::new("capture.apply", 4)));
        }
        let keep = unavail.contains(name.as_str());
        let extra_sql = format!(
            "UPDATE oo_capture.\"{physical}\" SET \"{name}\" = CASE WHEN $3::boolean THEN oo_capture.\"{physical}\".\"{name}\" ELSE $4 END WHERE \"{pk}\" = $1 AND \"_oo_pos\" = CAST(CAST($2 AS text) AS numeric) AND \"_oo_seq\" = $5",
            pk = plan.pk
        );
        tx.execute(&extra_sql, &[&pk_value, &pos, &keep, value, &seq]).await?;
    }
    if !row.unavail.is_empty() {
        let generation = i64::try_from(capture.generation).unwrap_or(0);
        let rel_key = format!("{}.{}", plan.schema, plan.table);
        for name in unavail {
            tx.execute(
                "INSERT INTO oo_capture.unavailable (source_id, generation, rel_key, pk, column_name) VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
                &[&config.source_id, &generation, &rel_key, &row.pk, &name],
            ).await?;
        }
    }
    Ok(())
}

fn upsert_sql(physical: &str, columns: &[StoredCol], pk: &str) -> Result<String, Fail> {
    if !ident_ok(physical) || !ident_ok(pk) || columns.is_empty() {
        return Err(Fail::new("capture.apply", 4));
    }
    let mut names = Vec::new();
    let mut inserts = Vec::new();
    let mut sets = Vec::new();
    for (index, col) in columns.iter().enumerate() {
        if !ident_ok(&col.name) || !format_type_allowed(&col.format_type) {
            return Err(Fail::new("capture.apply", 4));
        }
        let name = format!("\"{}\"", col.name);
        names.push(name.clone());
        inserts.push(as_text_then(index + 1, &col.format_type));
        let flag = columns.len() + 4 + index;
        sets.push(format!("{name} = CASE WHEN ${flag}::boolean THEN oo_capture.\"{physical}\".{name} ELSE EXCLUDED.{name} END"));
    }
    let n = columns.len();
    Ok(format!(
        "INSERT INTO oo_capture.\"{physical}\" ({}, \"_oo_pos\", \"_oo_seq\", \"_oo_op\", \"_oo_src_ts\") VALUES ({}, {}, ${}, 1, ${}) ON CONFLICT (\"{pk}\") DO UPDATE SET {}, \"_oo_pos\" = EXCLUDED.\"_oo_pos\", \"_oo_seq\" = EXCLUDED.\"_oo_seq\", \"_oo_op\" = EXCLUDED.\"_oo_op\", \"_oo_src_ts\" = EXCLUDED.\"_oo_src_ts\" WHERE oo_capture.\"{physical}\".\"_oo_pos\" < EXCLUDED.\"_oo_pos\" OR (oo_capture.\"{physical}\".\"_oo_pos\" = EXCLUDED.\"_oo_pos\" AND oo_capture.\"{physical}\".\"_oo_seq\" < EXCLUDED.\"_oo_seq\")",
        names.join(", "),
        inserts.join(", "),
        as_text_then(n + 1, "numeric"),
        n + 2,
        n + 3,
        sets.join(", ")
    ))
}

async fn publish(
    config: &Config,
    capture: &Capture,
    target: &mut Session,
    plans: &[TablePlan],
    paused: &[String],
    applied: &BuildState,
    reason: &str,
) -> Result<(), StepErr> {
    let generation = i64::try_from(capture.generation).unwrap_or(0);
    let tx = target.client.transaction().await?;
    for plan in plans {
        let rel_key = format!("{}.{}", plan.schema, plan.table);
        if paused.contains(&rel_key) {
            let view = view_name(&plan.schema, &plan.table).map_err(StepErr::Rule)?;
            tx.batch_execute(&format!("DROP VIEW IF EXISTS oo_capture.\"{view}\"")).await?;
            continue;
        }
        let physical = physical_name(&config.source_id, capture.generation, &plan.schema, &plan.table);
        let view = view_name(&plan.schema, &plan.table).map_err(StepErr::Rule)?;
        let cols = plan.columns.iter().filter(|col| col.published && ident_ok(&col.name)).map(|col| format!("\"{}\"", col.name)).collect::<Vec<_>>().join(", ");
        tx.batch_execute(&format!("CREATE OR REPLACE VIEW oo_capture.\"{view}\" AS SELECT {cols} FROM oo_capture.\"{physical}\"")).await?;
    }
    tx.execute(
        "INSERT INTO oo_capture.live (source_id, generation, l0, l_end, applied_lsn, reason, cross_table_consistent) VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (source_id) DO UPDATE SET generation = EXCLUDED.generation, l0 = EXCLUDED.l0, l_end = EXCLUDED.l_end, applied_lsn = EXCLUDED.applied_lsn, reason = EXCLUDED.reason, cross_table_consistent = EXCLUDED.cross_table_consistent",
        &[&config.source_id, &generation, &applied.l0, &applied.l_end, &applied.applied, &reason, &paused.is_empty()],
    ).await?;
    tx.execute(
        "INSERT INTO oo_capture.build (source_id, generation, phase, l0, l_end, applied_lsn, reason) VALUES ($1, $2, 'live', $3, $4, $5, $6) ON CONFLICT (source_id, generation) DO UPDATE SET phase = 'live', l_end = EXCLUDED.l_end, applied_lsn = EXCLUDED.applied_lsn, reason = EXCLUDED.reason",
        &[&config.source_id, &generation, &applied.l0, &applied.l_end, &applied.applied, &reason],
    ).await?;
    tx.batch_execute("REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA oo_capture FROM PUBLIC, oo_r_east, oo_r_reader").await?;
    tx.batch_execute("REVOKE ALL ON SCHEMA oo_capture FROM PUBLIC, oo_r_east, oo_r_reader").await?;
    tx.commit().await?;
    Ok(())
}

fn view_name(schema: &str, table: &str) -> Result<String, Fail> {
    let name = format!("v_{schema}_{table}");
    if ident_ok(&name) { Ok(name) } else { Err(Fail::new("capture.refused", 4)) }
}

async fn compare_tables(config: &Config, capture: &Capture, source: &Session, target: &Session, body: &mut Value) -> Result<(), StepErr> {
    let generation = i64::try_from(capture.generation).unwrap_or(0);
    body["business_read"] = json!(true);
    let mut cell_mismatches = 0u64;
    let mut source_rows = 0u64;
    let mut target_rows = 0u64;
    let mut samples = Vec::new();
    let mut per_table = Vec::new();
    for object in &config.approved_objects {
        let rel_key = format!("{}.{}", object.schema, object.table);
        let map = target.client.query_one(
            "SELECT physical, pk_name, columns_json FROM oo_capture.relmap WHERE source_id = $1 AND generation = $2 AND rel_key = $3",
            &[&config.source_id, &generation, &rel_key],
        ).await?;
        let physical: String = map.get(0);
        let pk: String = map.get(1);
        let columns: Vec<StoredCol> = serde_json::from_str::<Vec<StoredCol>>(&map.get::<_, String>(2)).unwrap_or_default();
        if !ident_ok(&physical) {
            return Err(StepErr::Rule(Fail::new("capture.diff", 7)));
        }
        let source_sql = read_sql(&object.schema, &object.table, &columns, &pk, capture.window_rows, true).map_err(StepErr::Rule)?;
        let source_first = read_sql(&object.schema, &object.table, &columns, &pk, capture.window_rows, false).map_err(StepErr::Rule)?;
        let target_sql = read_sql("oo_capture", &physical, &columns, &pk, capture.window_rows, true).map_err(StepErr::Rule)?;
        let target_first = read_sql("oo_capture", &physical, &columns, &pk, capture.window_rows, false).map_err(StepErr::Rule)?;
        let mut source_last: Option<i64> = None;
        let mut target_last: Option<i64> = None;
        let mut source_page = Vec::new();
        let mut target_page = Vec::new();
        let mut source_i = 0usize;
        let mut target_i = 0usize;
        let mut source_done = false;
        let mut target_done = false;
        let mut table_source = 0u64;
        let mut table_target = 0u64;
        let mut table_bad = 0u64;
        loop {
            if !source_done && source_i >= source_page.len() {
                source_page = fetch_page(&source.client, &source_first, &source_sql, source_last).await?;
                source_i = 0;
                source_done = source_page.is_empty();
            }
            if !target_done && target_i >= target_page.len() {
                target_page = fetch_page(&target.client, &target_first, &target_sql, target_last).await?;
                target_i = 0;
                target_done = target_page.is_empty();
            }
            if source_done && target_done {
                break;
            }
            let width = u64::try_from(columns.len()).unwrap_or(0);
            if source_done {
                let right_pk = pk_of(&columns, &pk, &target_page[target_i]).ok_or(StepErr::Rule(Fail::new("capture.diff", 7)))?;
                target_last = Some(pk_order_value(&right_pk).map_err(StepErr::Rule)?);
                table_bad += width;
                table_target += 1;
                target_i += 1;
                continue;
            }
            if target_done {
                let left_pk = pk_of(&columns, &pk, &source_page[source_i]).ok_or(StepErr::Rule(Fail::new("capture.diff", 7)))?;
                source_last = Some(pk_order_value(&left_pk).map_err(StepErr::Rule)?);
                table_bad += width;
                table_source += 1;
                source_i += 1;
                continue;
            }
            let left_pk = pk_of(&columns, &pk, &source_page[source_i]).ok_or(StepErr::Rule(Fail::new("capture.diff", 7)))?;
            let right_pk = pk_of(&columns, &pk, &target_page[target_i]).ok_or(StepErr::Rule(Fail::new("capture.diff", 7)))?;
            match pk_order(&left_pk, &right_pk).map_err(StepErr::Rule)? {
                std::cmp::Ordering::Equal => {
                    for (index, col) in columns.iter().enumerate() {
                        let left = source_page[source_i].get(index).cloned().flatten();
                        let right = target_page[target_i].get(index).cloned().flatten();
                        if left != right {
                            table_bad += 1;
                            if samples.len() < 20 {
                                samples.push(json!({"table": rel_key, "pk": left_pk, "column": col.name, "source": left.as_ref().map(|text| clip(text)), "target": right.as_ref().map(|text| clip(text))}));
                            }
                        }
                    }
                    table_source += 1;
                    table_target += 1;
                    let ordered = pk_order_value(&left_pk).map_err(StepErr::Rule)?;
                    source_last = Some(ordered);
                    target_last = Some(ordered);
                    source_i += 1;
                    target_i += 1;
                }
                std::cmp::Ordering::Less => {
                    table_bad += width;
                    table_source += 1;
                    source_last = Some(pk_order_value(&left_pk).map_err(StepErr::Rule)?);
                    source_i += 1;
                }
                std::cmp::Ordering::Greater => {
                    table_bad += width;
                    table_target += 1;
                    target_last = Some(pk_order_value(&right_pk).map_err(StepErr::Rule)?);
                    target_i += 1;
                }
            }
        }
        source_rows += table_source;
        target_rows += table_target;
        cell_mismatches += table_bad;
        per_table.push(json!({"table": rel_key, "source_rows": table_source, "target_rows": table_target, "cell_mismatches": table_bad}));
    }
    body["source_rows"] = json!(source_rows);
    body["target_rows"] = json!(target_rows);
    body["cell_mismatches"] = json!(cell_mismatches);
    body["rows_compared"] = json!(source_rows.max(target_rows));
    body["samples"] = json!(samples);
    body["tables"] = json!(per_table);
    body["sampled"] = json!(false);
    body["full_scan"] = json!(true);
    Ok(())
}

fn pk_order(left: &str, right: &str) -> Result<std::cmp::Ordering, Fail> {
    Ok(pk_order_value(left)?.cmp(&pk_order_value(right)?))
}

fn pk_order_value(text: &str) -> Result<i64, Fail> {
    text.parse::<i64>().map_err(|_| Fail::new("capture.diff", 7))
}

fn pk_of(columns: &[StoredCol], pk: &str, row: &[Option<String>]) -> Option<String> {
    let index = columns.iter().position(|col| col.name == pk)?;
    row.get(index).cloned().flatten()
}

fn clip(text: &str) -> String {
    text.chars().take(80).collect()
}

async fn fetch_page(client: &impl GenericClient, first_sql: &str, next_sql: &str, last: Option<i64>) -> Result<Vec<Vec<Option<String>>>, tokio_postgres::Error> {
    let rows = if let Some(last) = last {
        client.query(next_sql, &[&last]).await?
    } else {
        client.query(first_sql, &[]).await?
    };
    let mut page = Vec::new();
    for row in rows {
        let width = row.len();
        let mut values = Vec::new();
        for index in 0..width {
            values.push(row.try_get(index)?);
        }
        page.push(values);
    }
    Ok(page)
}

async fn run_schedule(config: &Config, capture: &Capture, source: &Session, target: &Session, body: &mut Value) -> Result<(), tokio_postgres::Error> {
    let started = epoch_ms();
    let mut reconcile_last = None;
    let mut health_last = None;
    let mut reconcile_fires = 0u64;
    let mut health_fires = 0u64;
    let mut events = Vec::new();
    while reconcile_fires < 2 || health_fires < 2 {
        if epoch_ms().saturating_sub(started) > 45_000 {
            break;
        }
        let now = epoch_ms();
        let reconcile_due = !matches!(schedule_tick(now, reconcile_last, capture.reconcile_ms, false), Tick::Wait);
        let health_due = !matches!(schedule_tick(now, health_last, capture.health_ms, false), Tick::Wait);
        if reconcile_due {
            let in_flight = true;
            if health_due {
                events.push(json!({"kind": "health", "action": "skip_overlap", "at_ms": now}));
            }
            let began = epoch_ms();
            let counts = reconcile_counts(config, capture, source, target).await?;
            let ended = epoch_ms();
            events.push(json!({"kind": "reconcile", "action": "fire", "began_ms": began, "ended_ms": ended, "counts": counts}));
            reconcile_last = Some(ended);
            reconcile_fires += 1;
            let _ = in_flight;
            if health_due {
                continue;
            }
        }
        let now = epoch_ms();
        if !matches!(schedule_tick(now, health_last, capture.health_ms, false), Tick::Wait) {
            let began = epoch_ms();
            let health = health_slot(&source.client, &capture.slot).await?;
            let ended = epoch_ms();
            events.push(json!({"kind": "health", "action": "fire", "began_ms": began, "ended_ms": ended, "retained_bytes": health}));
            health_last = Some(ended);
            health_fires += 1;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    body["events"] = json!(events);
    body["reconcile_fires"] = json!(reconcile_fires);
    body["health_fires"] = json!(health_fires);
    body["elapsed_ms"] = json!(epoch_ms().saturating_sub(started));
    body["business_read"] = json!(true);
    Ok(())
}

async fn reconcile_counts(config: &Config, capture: &Capture, source: &Session, target: &Session) -> Result<Vec<Value>, tokio_postgres::Error> {
    let generation = i64::try_from(capture.generation).unwrap_or(0);
    let mut counts = Vec::new();
    for object in &config.approved_objects {
        let rel_key = format!("{}.{}", object.schema, object.table);
        let sql = count_sql(&object.schema, &object.table).unwrap_or_else(|_| "SELECT 0".to_owned());
        let source_count: i64 = source.client.query_one(&sql, &[]).await?.get(0);
        let physical: String = target.client.query_one(
            "SELECT physical FROM oo_capture.relmap WHERE source_id = $1 AND generation = $2 AND rel_key = $3",
            &[&config.source_id, &generation, &rel_key],
        ).await?.get(0);
        let target_sql = count_sql("oo_capture", &physical).unwrap_or_else(|_| "SELECT 0".to_owned());
        let target_count: i64 = target.client.query_one(&target_sql, &[]).await?.get(0);
        counts.push(json!({"table": rel_key, "source": source_count, "target": target_count, "equal": source_count == target_count}));
    }
    Ok(counts)
}

async fn health_slot(client: &impl GenericClient, slot: &str) -> Result<String, tokio_postgres::Error> {
    let row = client.query_one(
        "SELECT COALESCE(pg_catalog.pg_wal_lsn_diff(pg_catalog.pg_current_wal_lsn(), restart_lsn)::text, '') FROM pg_catalog.pg_replication_slots WHERE slot_name = $1",
        &[&slot],
    ).await?;
    Ok(row.get(0))
}

fn epoch_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| u64::try_from(duration.as_millis()).unwrap_or(0)).unwrap_or(0)
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut text = String::with_capacity(64);
    for byte in digest {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

fn write_arrow(schema_name: &str, table: &str, generation: u64, path_kind: &str, columns: &[StoredCol], rows: &[ArrowRow]) -> Result<Vec<u8>, Fail> {
    if rows.is_empty() {
        return Err(Fail::new("arrow.empty", 8));
    }
    let mut fields = vec![
        Field::new("_oo_pk", DataType::Utf8, false),
        Field::new("_oo_op", DataType::Int64, false),
        Field::new("_oo_pos", DataType::Utf8, false),
        Field::new("_oo_seq", DataType::Int64, false),
        Field::new("_oo_src_ts", DataType::Int64, true),
        Field::new("_oo_unavail", DataType::Utf8, false),
    ];
    for col in columns {
        fields.push(Field::new(&col.name, DataType::Utf8, true));
    }
    let mut metadata = std::collections::HashMap::new();
    metadata.insert("oo.schema".to_owned(), schema_name.to_owned());
    metadata.insert("oo.table".to_owned(), table.to_owned());
    metadata.insert("oo.generation".to_owned(), generation.to_string());
    metadata.insert("oo.path".to_owned(), path_kind.to_owned());
    metadata.insert("oo.admitted_extract".to_owned(), "false".to_owned());
    metadata.insert("oo.published".to_owned(), columns.len().to_string());
    let extra_names = rows.first().map(|row| row.extras.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>()).unwrap_or_default();
    for name in &extra_names {
        fields.push(Field::new(name, DataType::Utf8, true));
    }
    let schema = Arc::new(Schema::new(fields).with_metadata(metadata));
    let mut arrays: Vec<Arc<dyn Array>> = vec![
        Arc::new(StringArray::from(rows.iter().map(|row| Some(row.pk.as_str())).collect::<Vec<_>>())),
        Arc::new(Int64Array::from(rows.iter().map(|row| row.op).collect::<Vec<_>>())),
        Arc::new(StringArray::from(rows.iter().map(|row| Some(row.pos.as_str())).collect::<Vec<_>>())),
        Arc::new(Int64Array::from(rows.iter().map(|row| row.seq).collect::<Vec<_>>())),
        Arc::new(Int64Array::from(rows.iter().map(|row| row.src_ts).collect::<Vec<_>>())),
        Arc::new(StringArray::from(rows.iter().map(|row| Some(row.unavail.as_str())).collect::<Vec<_>>())),
    ];
    for index in 0..columns.len() {
        let values = rows.iter().map(|row| row.values.get(index).and_then(|value| value.as_deref())).collect::<Vec<_>>();
        arrays.push(Arc::new(StringArray::from(values)));
    }
    for name in &extra_names {
        let values = rows.iter().map(|row| row.extras.iter().find(|(item, _)| item == name).and_then(|(_, value)| value.as_deref())).collect::<Vec<_>>();
        arrays.push(Arc::new(StringArray::from(values)));
    }
    let batch = RecordBatch::try_new(Arc::clone(&schema), arrays).map_err(|_| Fail::new("arrow.ipc", 1))?;
    let options = IpcWriteOptions::try_new(8, false, MetadataVersion::V5).map_err(|_| Fail::new("arrow.ipc", 1))?;
    let mut buffer = Vec::new();
    let mut writer = StreamWriter::try_new_with_options(&mut buffer, schema.as_ref(), options).map_err(|_| Fail::new("arrow.ipc", 1))?;
    writer.write(&batch).map_err(|_| Fail::new("arrow.ipc", 1))?;
    writer.finish().map_err(|_| Fail::new("arrow.ipc", 1))?;
    let _ = read_arrow(&buffer)?;
    Ok(buffer)
}

fn read_arrow(bytes: &[u8]) -> Result<Vec<ArrowRow>, Fail> {
    let reader = StreamReader::try_new(Cursor::new(bytes), None).map_err(|_| Fail::new("arrow.ipc", 1))?;
    let mut rows = Vec::new();
    for batch in reader {
        let batch = batch.map_err(|_| Fail::new("arrow.ipc", 1))?;
        let schema = batch.schema();
        if schema.metadata().get("oo.admitted_extract").map(String::as_str) != Some("false") {
            return Err(Fail::new("arrow.ipc", 1));
        }
        let pks = required_utf8(&batch, "_oo_pk")?;
        let ops = required_i64(&batch, "_oo_op")?;
        let pos = required_utf8(&batch, "_oo_pos")?;
        let seq = required_i64(&batch, "_oo_seq")?;
        let src = optional_i64(&batch, "_oo_src_ts")?;
        let unavail = required_utf8(&batch, "_oo_unavail")?;
        let published: usize = schema.metadata().get("oo.published").and_then(|text| text.parse().ok()).ok_or(Fail::new("arrow.ipc", 1))?;
        let business_names = schema.fields().iter().filter(|field| !field.name().starts_with("_oo_")).map(|field| field.name().clone()).collect::<Vec<_>>();
        let business: Vec<Vec<Option<String>>> = business_names.iter().map(|name| optional_utf8(&batch, name)).collect::<Result<_, _>>()?;
        if published > business.len() {
            return Err(Fail::new("arrow.ipc", 1));
        }
        for index in 0..pks.len() {
            let mut values = Vec::new();
            for column in business.iter().take(published) {
                values.push(column.get(index).cloned().flatten());
            }
            let mut extras = Vec::new();
            for (name, column) in business_names.iter().zip(business.iter()).skip(published) {
                extras.push((name.clone(), column.get(index).cloned().flatten()));
            }
            rows.push(ArrowRow {
                pk: pks[index].clone().ok_or(Fail::new("arrow.ipc", 1))?,
                op: ops[index].ok_or(Fail::new("arrow.ipc", 1))?,
                pos: pos[index].clone().ok_or(Fail::new("arrow.ipc", 1))?,
                seq: seq[index].ok_or(Fail::new("arrow.ipc", 1))?,
                src_ts: src.get(index).copied().flatten(),
                unavail: unavail[index].clone().unwrap_or_default(),
                values,
                extras,
            });
        }
    }
    if rows.is_empty() {
        return Err(Fail::new("arrow.empty", 8));
    }
    Ok(rows)
}

fn required_utf8(batch: &RecordBatch, name: &str) -> Result<Vec<Option<String>>, Fail> {
    optional_utf8(batch, name)
}

fn optional_utf8(batch: &RecordBatch, name: &str) -> Result<Vec<Option<String>>, Fail> {
    let index = batch.schema().index_of(name).map_err(|_| Fail::new("arrow.ipc", 1))?;
    let array = batch.column(index).as_any().downcast_ref::<StringArray>().ok_or(Fail::new("arrow.ipc", 1))?;
    let mut out = Vec::new();
    for row in 0..array.len() {
        if array.is_null(row) { out.push(None); } else { out.push(Some(array.value(row).to_owned())); }
    }
    Ok(out)
}

fn required_i64(batch: &RecordBatch, name: &str) -> Result<Vec<Option<i64>>, Fail> {
    optional_i64(batch, name)
}

fn optional_i64(batch: &RecordBatch, name: &str) -> Result<Vec<Option<i64>>, Fail> {
    let index = batch.schema().index_of(name).map_err(|_| Fail::new("arrow.ipc", 1))?;
    let array = batch.column(index).as_any().downcast_ref::<Int64Array>().ok_or(Fail::new("arrow.ipc", 1))?;
    let mut out = Vec::new();
    for row in 0..array.len() {
        if array.is_null(row) { out.push(None); } else { out.push(Some(array.value(row))); }
    }
    Ok(out)
}

struct Cur<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Cur<'a> {
    fn u8(&mut self) -> Result<u8, Fail> {
        let byte = *self.data.get(self.at).ok_or(Fail::new("capture.protocol", 4))?;
        self.at += 1;
        Ok(byte)
    }
    fn bytes(&mut self, len: usize) -> Result<&'a [u8], Fail> {
        let end = self.at.checked_add(len).ok_or(Fail::new("capture.protocol", 4))?;
        let slice = self.data.get(self.at..end).ok_or(Fail::new("capture.protocol", 4))?;
        self.at = end;
        Ok(slice)
    }
    fn i16(&mut self) -> Result<i16, Fail> {
        Ok(i16::from_be_bytes(self.bytes(2)?.try_into().map_err(|_| Fail::new("capture.protocol", 4))?))
    }
    fn u32(&mut self) -> Result<u32, Fail> {
        let bytes = self.bytes(4)?;
        Ok(u32::from_be_bytes(bytes.try_into().map_err(|_| Fail::new("capture.protocol", 4))?))
    }
    fn i32(&mut self) -> Result<i32, Fail> {
        Ok(i32::from_be_bytes(self.bytes(4)?.try_into().map_err(|_| Fail::new("capture.protocol", 4))?))
    }
    fn u64(&mut self) -> Result<u64, Fail> {
        let bytes = self.bytes(8)?;
        Ok(u64::from_be_bytes(bytes.try_into().map_err(|_| Fail::new("capture.protocol", 4))?))
    }
    fn i64(&mut self) -> Result<i64, Fail> {
        Ok(i64::from_be_bytes(self.bytes(8)?.try_into().map_err(|_| Fail::new("capture.protocol", 4))?))
    }
    fn cstr(&mut self) -> Result<String, Fail> {
        let rest = self.data.get(self.at..).ok_or(Fail::new("capture.protocol", 4))?;
        let end = rest.iter().position(|byte| *byte == 0).ok_or(Fail::new("capture.protocol", 4))?;
        if end > 256 {
            return Err(Fail::new("capture.protocol", 4));
        }
        let text = std::str::from_utf8(&rest[..end]).map_err(|_| Fail::new("capture.protocol", 4))?.to_owned();
        self.at += end + 1;
        Ok(text)
    }
}

pub(crate) fn decode_message(data: &[u8]) -> Result<Msg, Fail> {
    let mut cur = Cur { data, at: 0 };
    let kind = cur.u8()?;
    let message = match kind {
        b'B' => {
            let final_lsn = cur.u64()?;
            let _commit_us = cur.i64()?;
            let xid = cur.i32()?;
            Msg::Begin { xid, final_lsn }
        }
        b'C' => {
            let _flags = cur.u8()?;
            let commit_lsn = cur.u64()?;
            let end_lsn = cur.u64()?;
            let commit_us = cur.i64()?;
            Msg::Commit { commit_lsn, end_lsn, commit_us }
        }
        b'R' => {
            let oid = cur.u32()?;
            let namespace = cur.cstr()?;
            let name = cur.cstr()?;
            let _identity = cur.u8()?;
            let natts = cur.i16()?;
            if !(0..=256).contains(&natts) {
                return Err(Fail::new("capture.protocol", 4));
            }
            let mut columns = Vec::new();
            for _ in 0..natts {
                let _flags = cur.u8()?;
                let col_name = cur.cstr()?;
                let type_oid = i64::from(cur.u32()?);
                let _typmod = cur.i32()?;
                columns.push(RelCol { name: col_name, type_oid });
            }
            Msg::Relation(Rel { oid, namespace, name, columns })
        }
        b'I' => {
            let oid = cur.u32()?;
            if cur.u8()? != b'N' {
                return Err(Fail::new("capture.protocol", 4));
            }
            Msg::Insert { oid, cells: parse_tuple(&mut cur)? }
        }
        b'U' => {
            let oid = cur.u32()?;
            let marker = cur.u8()?;
            if marker == b'K' || marker == b'O' {
                let _old = parse_tuple(&mut cur)?;
                if cur.u8()? != b'N' {
                    return Err(Fail::new("capture.protocol", 4));
                }
            } else if marker != b'N' {
                return Err(Fail::new("capture.protocol", 4));
            }
            Msg::Update { oid, cells: parse_tuple(&mut cur)? }
        }
        b'D' => {
            let oid = cur.u32()?;
            let marker = cur.u8()?;
            if marker != b'K' && marker != b'O' {
                return Err(Fail::new("capture.protocol", 4));
            }
            Msg::Delete { oid, cells: parse_tuple(&mut cur)? }
        }
        b'T' => {
            let nrels = cur.i32()?;
            if !(0..=64).contains(&nrels) {
                return Err(Fail::new("capture.protocol", 4));
            }
            let _flags = cur.u8()?;
            let mut oids = Vec::new();
            for _ in 0..nrels {
                oids.push(cur.u32()?);
            }
            Msg::Truncate { oids }
        }
        b'O' => {
            let _lsn = cur.u64()?;
            let _name = cur.cstr()?;
            Msg::Ignore
        }
        b'Y' => {
            let _oid = cur.u32()?;
            let _namespace = cur.cstr()?;
            let _name = cur.cstr()?;
            Msg::Ignore
        }
        b'M' => {
            let _flags = cur.u8()?;
            let _lsn = cur.u64()?;
            let _prefix = cur.cstr()?;
            let len = cur.i32()?;
            if !(0..=1_048_576).contains(&len) {
                return Err(Fail::new("capture.protocol", 4));
            }
            let _body = cur.bytes(usize::try_from(len).map_err(|_| Fail::new("capture.protocol", 4))?)?;
            Msg::Ignore
        }
        _ => return Err(Fail::new("capture.protocol", 4)),
    };
    if cur.at != data.len() {
        return Err(Fail::new("capture.protocol", 4));
    }
    Ok(message)
}

fn parse_tuple(cur: &mut Cur<'_>) -> Result<Vec<Cell>, Fail> {
    let ncolumns = cur.i16()?;
    if !(0..=256).contains(&ncolumns) {
        return Err(Fail::new("capture.protocol", 4));
    }
    let mut cells = Vec::new();
    for _ in 0..ncolumns {
        match cur.u8()? {
            b'n' => cells.push(Cell { kind: CellKind::Null, text: None }),
            b'u' => cells.push(Cell { kind: CellKind::Unchanged, text: None }),
            b't' => {
                let len = cur.i32()?;
                if !(0..=16_777_216).contains(&len) {
                    return Err(Fail::new("capture.protocol", 4));
                }
                let bytes = cur.bytes(usize::try_from(len).map_err(|_| Fail::new("capture.protocol", 4))?)?;
                let text = std::str::from_utf8(bytes).map_err(|_| Fail::new("capture.protocol", 4))?.to_owned();
                cells.push(Cell { kind: CellKind::Value, text: Some(text) });
            }
            b'b' => {
                let len = cur.i32()?;
                if !(0..=16_777_216).contains(&len) {
                    return Err(Fail::new("capture.protocol", 4));
                }
                let bytes = cur.bytes(usize::try_from(len).map_err(|_| Fail::new("capture.protocol", 4))?)?;
                cells.push(Cell { kind: CellKind::Value, text: Some(bytea_text(bytes)) });
            }
            _ => return Err(Fail::new("capture.protocol", 4)),
        }
    }
    Ok(cells)
}

fn bytea_text(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(2 + bytes.len() * 2);
    out.push('\\');
    out.push('x');
    for byte in bytes {
        out.push(HEX[usize::from(byte >> 4)] as char);
        out.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    out
}

struct Assembled {
    txns: Vec<ReadyTxn>,
    pending: u64,
}

fn assemble(messages: &[Msg]) -> Result<Assembled, Fail> {
    let mut relations: BTreeMap<u32, Rel> = BTreeMap::new();
    let mut open: Option<(i32, Vec<TxnEvent>)> = None;
    let mut txns = Vec::new();
    let mut seq = 0i64;
    for message in messages {
        match message {
            Msg::Ignore => {}
            Msg::Relation(rel) => {
                relations.insert(rel.oid, rel.clone());
            }
            Msg::Begin { xid, .. } => {
                if open.is_some() {
                    return Err(Fail::new("capture.protocol", 4));
                }
                open = Some((*xid, Vec::new()));
                seq = 0;
            }
            Msg::Commit { commit_lsn, end_lsn, commit_us } => {
                let Some((_xid, events)) = open.take() else {
                    return Err(Fail::new("capture.protocol", 4));
                };
                txns.push(ReadyTxn { commit_lsn: *commit_lsn, end_lsn: *end_lsn, commit_us: *commit_us, events });
            }
            Msg::Insert { oid, cells } | Msg::Update { oid, cells } | Msg::Delete { oid, cells } => {
                let Some((_, events)) = open.as_mut() else {
                    return Err(Fail::new("capture.protocol", 4));
                };
                let rel = relations.get(oid).cloned().ok_or(Fail::new("capture.protocol", 4))?;
                seq += 1;
                let event = match message {
                    Msg::Delete { .. } => TxnEvent::Delete { rel, cells: cells.clone(), seq },
                    _ => TxnEvent::Upsert { rel, cells: cells.clone(), seq },
                };
                events.push(event);
            }
            Msg::Truncate { oids } => {
                let Some((_, events)) = open.as_mut() else {
                    return Err(Fail::new("capture.protocol", 4));
                };
                for oid in oids {
                    let rel = relations.get(oid).cloned().ok_or(Fail::new("capture.protocol", 4))?;
                    seq += 1;
                    events.push(TxnEvent::Truncate { rel, seq });
                }
            }
        }
    }
    let pending = u64::from(u8::from(open.is_some()));
    Ok(Assembled { txns, pending })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoder_keeps_commit_end_lsn_and_unchanged_toast() {
        let mut relation = Vec::new();
        relation.push(b'R');
        relation.extend_from_slice(&1u32.to_be_bytes());
        relation.extend_from_slice(b"public\0hubs\0");
        relation.push(b'd');
        relation.extend_from_slice(&2i16.to_be_bytes());
        relation.push(1);
        relation.extend_from_slice(b"hub_id\0");
        relation.extend_from_slice(&20u32.to_be_bytes());
        relation.extend_from_slice(&(-1i32).to_be_bytes());
        relation.push(0);
        relation.extend_from_slice(b"note\0");
        relation.extend_from_slice(&25u32.to_be_bytes());
        relation.extend_from_slice(&(-1i32).to_be_bytes());
        let mut begin = vec![b'B'];
        begin.extend_from_slice(&10u64.to_be_bytes());
        begin.extend_from_slice(&20i64.to_be_bytes());
        begin.extend_from_slice(&7i32.to_be_bytes());
        let mut update = vec![b'U'];
        update.extend_from_slice(&1u32.to_be_bytes());
        update.push(b'N');
        update.extend_from_slice(&2i16.to_be_bytes());
        update.push(b't');
        update.extend_from_slice(&1i32.to_be_bytes());
        update.push(b'9');
        update.push(b'u');
        let mut commit = vec![b'C'];
        commit.push(0);
        commit.extend_from_slice(&100u64.to_be_bytes());
        commit.extend_from_slice(&120u64.to_be_bytes());
        commit.extend_from_slice(&30i64.to_be_bytes());
        let messages = vec![
            decode_message(&relation).expect("relation"),
            decode_message(&begin).expect("begin"),
            decode_message(&update).expect("update"),
            decode_message(&commit).expect("commit"),
        ];
        assert!(matches!(messages[1], Msg::Begin { xid: 7, final_lsn: 10 }));
        let assembled = assemble(&messages).expect("assemble");
        assert_eq!(assembled.pending, 0);
        assert_eq!(assembled.txns.len(), 1);
        assert_eq!(assembled.txns[0].end_lsn, 120);
        assert_eq!(assembled.txns[0].commit_lsn, 100);
        assert!(lsn_advance(100, 120).is_some());
        assert!(lsn_advance(120, 120).is_none());
        let TxnEvent::Upsert { cells, .. } = &assembled.txns[0].events[0] else {
            panic!("upsert");
        };
        assert_eq!(cells[1].kind, CellKind::Unchanged);
    }

    #[test]
    fn open_begin_is_not_a_transaction() {
        let message = decode_message(&{
            let mut begin = vec![b'B'];
            begin.extend_from_slice(&1u64.to_be_bytes());
            begin.extend_from_slice(&2i64.to_be_bytes());
            begin.extend_from_slice(&3i32.to_be_bytes());
            begin
        }).expect("begin");
        let assembled = assemble(&[message]).expect("assemble");
        assert!(assembled.txns.is_empty());
        assert_eq!(assembled.pending, 1);
    }

    #[test]
    fn primary_key_order_is_numeric() {
        assert_eq!(pk_order("9", "10").expect("order"), std::cmp::Ordering::Less);
        assert_eq!(pk_order("10", "10").expect("order"), std::cmp::Ordering::Equal);
        assert!(pk_order("x", "1").is_err());
    }

    #[test]
    fn align_skips_excluded_and_keeps_a_new_column() {
        let plan = TablePlan {
            schema: "customer_east".to_owned(),
            table: "delivery_notes".to_owned(),
            pk: "note_id".to_owned(),
            columns: vec![
                StoredCol { name: "note_id".to_owned(), format_type: "bigint".to_owned(), type_oid: 20, published: true },
                StoredCol { name: "memo".to_owned(), format_type: "text".to_owned(), type_oid: 25, published: true },
            ],
            excluded: vec!["secret_note".to_owned()],
            replica: "d".to_owned(),
        };
        let rel = Rel {
            oid: 1,
            namespace: "customer_east".to_owned(),
            name: "delivery_notes".to_owned(),
            columns: vec![
                RelCol { name: "note_id".to_owned(), type_oid: 20 },
                RelCol { name: "secret_note".to_owned(), type_oid: 25 },
                RelCol { name: "memo".to_owned(), type_oid: 25 },
                RelCol { name: "extra_note".to_owned(), type_oid: 25 },
            ],
        };
        let cells = vec![
            Cell { kind: CellKind::Value, text: Some("7".to_owned()) },
            Cell { kind: CellKind::Value, text: Some("hidden".to_owned()) },
            Cell { kind: CellKind::Unchanged, text: None },
            Cell { kind: CellKind::Value, text: Some("added".to_owned()) },
        ];
        let Align::NewColumn(row) = align_upsert(&plan, &rel, &cells) else {
            panic!("new column");
        };
        assert_eq!(row.pk, "7");
        assert_eq!(row.values, vec![Some("7".to_owned()), None]);
        assert_eq!(row.extras, vec![("extra_note".to_owned(), Some("added".to_owned()))]);
        let flags: Vec<&str> = row.unavail.split(',').collect();
        assert!(flags.contains(&"memo"));
        assert!(!flags.contains(&"secret_note"));
    }

    #[test]
    fn lsn_text_round_trip_and_schedule_overlap() {
        assert_eq!(format_lsn(parse_lsn("16/B3748").expect("lsn")), "16/B3748");
        assert_eq!(schedule_tick(5_000, Some(0), 2_000, true), Tick::SkipOverlap);
        assert_eq!(schedule_tick(5_000, Some(4_000), 2_000, false), Tick::Wait);
        assert_eq!(schedule_tick(5_000, Some(1_000), 2_000, false), Tick::Fire);
        assert!(format_type_allowed("numeric(38,4)"));
        assert!(format_type_allowed("timestamp with time zone"));
        assert!(!format_type_allowed("jsonb"));
        assert!(!selects_business_table(&peek_sql("oo_slot", "oo_pub", 10)));
        assert!(selects_business_table("SELECT * FROM public.hubs"));
    }

    #[test]
    fn unknown_method_refuses_before_a_session() {
        let config = sample("c2-batch");
        let capture = config.capture.clone().expect("capture");
        assert_eq!(validate(&config, &capture, false).expect_err("method").token, "capture.refused");
    }

    fn sample(method: &str) -> Config {
        use crate::config::{Bins, Budget, EndpointFile, Model, ObjectName, FieldName};
        Config {
            protocol: "oo.postgres-preview.config/v1".to_owned(),
            source_id: "oo_dl01w1_46".to_owned(),
            secret_ref: "secret://oo-dl01w1-46/reader".to_owned(),
            role: "oo_dl01w1_46_reader".to_owned(),
            approved_objects: vec![ObjectName { schema: "public".to_owned(), table: "hubs".to_owned() }],
            approved_fields: vec![FieldName { schema: "public".to_owned(), table: "hubs".to_owned(), column: "hub_id".to_owned() }],
            pack_role: "oo_r_reader".to_owned(),
            budget: Budget { max_rows: 200_000, max_bytes: 134_217_728, statement_ms: 120_000 },
            model: Model { origin: "http://127.0.0.1:8317".to_owned(), path: "/v1/chat/completions".to_owned(), model_id: "grok-4.7".to_owned(), bearer_file: "/tmp/token".to_owned() },
            source: EndpointFile { host: "src".to_owned(), port: 5432, database: "src".to_owned(), username: "reader".to_owned(), password_file: "/run/secrets/reader".to_owned(), ca_file: "/run/secrets/ca.pem".to_owned() },
            dba: EndpointFile { host: "src".to_owned(), port: 5432, database: "src".to_owned(), username: "dba".to_owned(), password_file: "/run/secrets/dba".to_owned(), ca_file: "/run/secrets/ca.pem".to_owned() },
            target: EndpointFile { host: "tgt".to_owned(), port: 5432, database: "dst".to_owned(), username: "target".to_owned(), password_file: "/run/secrets/target".to_owned(), ca_file: "/run/secrets/ca.pem".to_owned() },
            bins: Bins { admission: "/opt/a".to_owned(), budget: "/opt/b".to_owned(), pipeline: "/opt/p".to_owned(), builder: "/opt/g".to_owned(), preview: "/opt/v".to_owned() },
            coordinator_state: "/ontology/execution/pg-budget-ready.json".to_owned(),
            out_dir: "/work-out/capture".to_owned(),
            denied: None,
            registry: None,
            source_generation: 1,
            capture: Some(Capture {
                method: method.to_owned(),
                publication: "oo_dl01w1_46_pub".to_owned(),
                slot: "oo_dl01w1_46_slot".to_owned(),
                username: "oo_dl01w1_46_cdc".to_owned(),
                password_file: "/run/secrets/cdc".to_owned(),
                reconcile_ms: 2_000,
                health_ms: 5_000,
                window_rows: 2_000,
                generation: 1,
            }),
        }
    }
}
