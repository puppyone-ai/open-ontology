//! Explicit C5 full-diff cycles. `sync: cdc` stays on the C1 admission path and is not executed here.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mysql_async::prelude::Queryable;
use mysql_async::{Conn, OptsBuilder, Row, Value};
use serde::Deserialize;
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row as PgRow};
use tokio::time::timeout;

use crate::capture::{self, CaptureRequest, Decision, ServerFacts, SyncMethod};
use crate::config::{self, ident_ok};

const SOURCE_SCHEMA: &str = "oo_c5";
const EXPECTED: &[(&str, &str)] = &[
    ("id", "bigint"),
    ("note", "varchar"),
    ("qty", "int"),
    ("money", "decimal"),
];

pub enum Kind {
    Cycle,
    Schedule,
}

pub enum RunError {
    Refuse(String),
    Input(String),
    Fail(String),
}

enum Stop {
    Refuse(String),
    Fail(String),
}

impl From<String> for Stop {
    fn from(value: String) -> Self {
        Self::Fail(value)
    }
}

#[derive(Debug)]
enum GateError {
    Admission(Box<Decision>),
    Code(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Repr {
    id: i64,
    note: Option<String>,
    qty: Option<i64>,
    money: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Live {
    generation: u64,
    snapshot_hash: String,
    rows: BTreeMap<i64, Repr>,
    tombstones: BTreeMap<i64, u64>,
    freshness: Option<String>,
}

struct Diff {
    inserts: Vec<Repr>,
    updates: Vec<Repr>,
    deletes: Vec<i64>,
}

struct Outcome {
    kind: &'static str,
    generation_before: u64,
    generation_after: u64,
    inserts: Vec<i64>,
    updates: Vec<i64>,
    deletes: Vec<i64>,
    snapshot_hash: String,
    grade: &'static str,
    warning: &'static str,
    published: bool,
}

#[derive(Deserialize)]
struct JobRequest {
    capture: CaptureRequest,
    #[serde(default)]
    facts: Option<ServerFacts>,
    label: String,
    schema: String,
    table: String,
    maria_host: String,
    maria_port: u16,
    maria_user_file: String,
    maria_password_file: String,
    pg_host: String,
    pg_port: u16,
    pg_user: String,
    pg_password_file: String,
    pg_database: String,
    #[serde(default)]
    interrupt_before_commit: bool,
    #[serde(default)]
    observe_ms: u64,
    #[serde(default)]
    ready_path: Option<String>,
}

fn gate(request: &CaptureRequest, facts: Option<&ServerFacts>) -> Result<Decision, GateError> {
    let facts = if request.sync == SyncMethod::Cdc {
        facts
    } else {
        None
    };
    let decision = capture::decide(request, facts).map_err(GateError::Admission)?;
    if decision.capture.as_deref() != Some("C5") || decision.method.as_deref() != Some("full") {
        return Err(GateError::Code("c5_not_selected"));
    }
    if decision
        .schedule
        .as_ref()
        .is_some_and(|plan| !plan.full_allowed)
    {
        return Err(GateError::Code("window_closed"));
    }
    Ok(decision)
}

pub fn deadlines(every_ms: u64, observe_ms: u64) -> Vec<u64> {
    if every_ms == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut at = every_ms;
    while at <= observe_ms {
        out.push(at);
        at = at.saturating_add(every_ms);
    }
    out
}

fn select_only(sql: &str) -> Result<(), String> {
    let trimmed = sql.trim_start();
    let Some(rest) = trimmed
        .get(..6)
        .filter(|word| word.eq_ignore_ascii_case("select"))
    else {
        return Err("refusing non-select on the source".to_string());
    };
    let boundary = trimmed.as_bytes().get(rest.len()).copied().unwrap_or(b' ');
    if boundary.is_ascii_whitespace() {
        Ok(())
    } else {
        Err("refusing non-select on the source".to_string())
    }
}

pub fn classify_columns(actual: &[(String, String)]) -> Result<(), SchemaFault> {
    let mut missing = Vec::new();
    let mut type_mismatch = Vec::new();
    for (name, expected) in EXPECTED {
        match actual
            .iter()
            .find(|(got, _)| got.eq_ignore_ascii_case(name))
        {
            None => missing.push((*name).to_string()),
            Some((_, got)) if !type_ok(name, got) => {
                type_mismatch.push(format!("{name}:{got}!={expected}"))
            }
            Some(_) => {}
        }
    }
    if !missing.is_empty() || !type_mismatch.is_empty() {
        return Err(SchemaFault::Broken {
            missing,
            type_mismatch,
        });
    }
    let extras: Vec<String> = actual
        .iter()
        .filter(|(name, _)| {
            !EXPECTED
                .iter()
                .any(|(expected, _)| name.eq_ignore_ascii_case(expected))
        })
        .map(|(name, _)| name.clone())
        .collect();
    if extras.is_empty() {
        Ok(())
    } else {
        Err(SchemaFault::NewColumn(extras))
    }
}

pub enum SchemaFault {
    Broken {
        missing: Vec<String>,
        type_mismatch: Vec<String>,
    },
    NewColumn(Vec<String>),
}

fn type_ok(name: &str, actual: &str) -> bool {
    let actual = actual.to_ascii_lowercase();
    match name {
        "id" => actual == "bigint",
        "note" => actual == "varchar",
        "qty" => actual == "int" || actual == "integer",
        "money" => actual == "decimal" || actual == "numeric",
        _ => false,
    }
}

fn canonical_money(text: &str) -> Result<String, String> {
    let text = text.trim();
    let (neg, rest) = if let Some(rest) = text.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = text.strip_prefix('+') {
        (false, rest)
    } else {
        (false, text)
    };
    if rest.is_empty() {
        return Err("money".to_string());
    }
    let (whole, frac) = rest.split_once('.').unwrap_or((rest, ""));
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !frac.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err("money".to_string());
    }
    if frac.len() > 2 && frac[2..].bytes().any(|byte| byte != b'0') {
        return Err("money scale".to_string());
    }
    let head = &frac[..frac.len().min(2)];
    let mut frac2 = head.to_string();
    while frac2.len() < 2 {
        frac2.push('0');
    }
    let trimmed = whole.trim_start_matches('0');
    let whole_norm = if trimmed.is_empty() { "0" } else { trimmed };
    if neg && !(whole_norm == "0" && frac2 == "00") {
        Ok(format!("-{whole_norm}.{frac2}"))
    } else {
        Ok(format!("{whole_norm}.{frac2}"))
    }
}

fn token(value: Option<&str>) -> String {
    match value {
        None => "\\N".to_string(),
        Some(text) => text
            .replace('\\', "\\\\")
            .replace('\t', "\\t")
            .replace('\n', "\\n"),
    }
}

impl Repr {
    fn line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}",
            self.id,
            token(self.note.as_deref()),
            token(self.qty.as_ref().map(i64::to_string).as_deref()),
            token(self.money.as_deref())
        )
    }

    fn hash(&self) -> String {
        hex32(Sha256::digest(self.line().as_bytes()).as_slice())
    }
}

fn index_rows(rows: &[Repr]) -> Result<BTreeMap<i64, Repr>, String> {
    let mut map = BTreeMap::new();
    for row in rows {
        if map.insert(row.id, row.clone()).is_some() {
            return Err(format!("duplicate key {}", row.id));
        }
    }
    Ok(map)
}

fn payload(map: &BTreeMap<i64, Repr>) -> String {
    let mut out = String::new();
    for row in map.values() {
        out.push_str(&row.line());
        out.push('\n');
    }
    out
}

fn hash_of(map: &BTreeMap<i64, Repr>) -> String {
    hex32(Sha256::digest(payload(map).as_bytes()).as_slice())
}

fn hex32(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn empty_store() -> Live {
    Live {
        generation: 0,
        snapshot_hash: hash_of(&BTreeMap::new()),
        rows: BTreeMap::new(),
        tombstones: BTreeMap::new(),
        freshness: None,
    }
}

#[derive(Debug)]
struct CaptureDelta {
    inserts: Vec<i64>,
    updates: Vec<i64>,
    upserts: Vec<Repr>,
    deletes: Vec<i64>,
}

fn capture_delta(
    prior: &BTreeMap<i64, Repr>,
    current: &BTreeMap<i64, Repr>,
    permanent_absent: &[i64],
) -> Result<CaptureDelta, String> {
    for id in permanent_absent {
        if prior.contains_key(id) {
            return Err(format!(
                "permanent absence {id} is live in the prior generation"
            ));
        }
        if current.contains_key(id) {
            return Err(format!("resurrection {id}"));
        }
    }
    let diff = plan_diff(
        &Live {
            generation: 0,
            snapshot_hash: String::new(),
            rows: prior.clone(),
            tombstones: BTreeMap::new(),
            freshness: None,
        },
        current,
    );
    let inserts: Vec<i64> = diff.inserts.iter().map(|row| row.id).collect();
    let updates: Vec<i64> = diff.updates.iter().map(|row| row.id).collect();
    let mut upserts = diff.inserts;
    upserts.extend(diff.updates);
    upserts.sort_by_key(|row| row.id);
    Ok(CaptureDelta {
        inserts,
        updates,
        upserts,
        deletes: diff.deletes,
    })
}

fn plan_diff(store: &Live, next: &BTreeMap<i64, Repr>) -> Diff {
    let mut inserts = Vec::new();
    let mut updates = Vec::new();
    let mut deletes = Vec::new();
    for row in next.values() {
        match store.rows.get(&row.id) {
            None => inserts.push(row.clone()),
            Some(prev) if prev.line() != row.line() => updates.push(row.clone()),
            Some(_) => {}
        }
    }
    for id in store.rows.keys() {
        if !next.contains_key(id) {
            deletes.push(*id);
        }
    }
    Diff {
        inserts,
        updates,
        deletes,
    }
}

fn apply_snapshot(
    store: &mut Live,
    next_rows: &[Repr],
    interrupt: bool,
    now: &str,
) -> Result<Outcome, String> {
    let next = index_rows(next_rows)?;
    let snapshot_hash = hash_of(&next);
    let generation_before = store.generation;
    if snapshot_hash == store.snapshot_hash {
        return Ok(Outcome {
            kind: "cache",
            generation_before,
            generation_after: store.generation,
            inserts: Vec::new(),
            updates: Vec::new(),
            deletes: Vec::new(),
            snapshot_hash,
            grade: "B",
            warning: "DELETES_LAGGED",
            published: true,
        });
    }
    let diff = plan_diff(store, &next);
    let inserts: Vec<i64> = diff.inserts.iter().map(|row| row.id).collect();
    let updates: Vec<i64> = diff.updates.iter().map(|row| row.id).collect();
    let deletes = diff.deletes.clone();
    if interrupt {
        return Ok(Outcome {
            kind: "interrupted",
            generation_before,
            generation_after: store.generation,
            inserts,
            updates,
            deletes,
            snapshot_hash,
            grade: "B",
            warning: "DELETES_LAGGED",
            published: false,
        });
    }
    let generation = store.generation.saturating_add(1);
    for id in &diff.deletes {
        store.rows.remove(id);
        store.tombstones.insert(*id, generation);
    }
    for row in diff.updates {
        store.tombstones.remove(&row.id);
        store.rows.insert(row.id, row);
    }
    for row in diff.inserts {
        store.tombstones.remove(&row.id);
        store.rows.insert(row.id, row);
    }
    store.generation = generation;
    store.snapshot_hash.clone_from(&snapshot_hash);
    store.freshness = Some(now.to_string());
    Ok(Outcome {
        kind: "committed",
        generation_before,
        generation_after: generation,
        inserts,
        updates,
        deletes,
        snapshot_hash,
        grade: "B",
        warning: "DELETES_LAGGED",
        published: true,
    })
}

fn cells_json(rows: &BTreeMap<i64, Repr>) -> Json {
    Json::Array(
        rows.values()
            .map(|row| {
                json!({
                    "id": row.id,
                    "note": row.note,
                    "qty": row.qty,
                    "money": row.money,
                })
            })
            .collect(),
    )
}

fn outcome_json(
    outcome: &Outcome,
    store: &Live,
    admission: &Decision,
    label: &str,
    statements: &[String],
) -> Json {
    json!({
        "label": label,
        "outcome": outcome.kind,
        "published": outcome.published,
        "generation_before": outcome.generation_before,
        "generation_after": outcome.generation_after,
        "inserts": outcome.inserts,
        "updates": outcome.updates,
        "deletes": outcome.deletes,
        "snapshot_hash": outcome.snapshot_hash,
        "grade": outcome.grade,
        "capture": "C5",
        "delete_detection": "reconcile",
        "warning": outcome.warning,
        "token_order": "None",
        "freshness": store.freshness,
        "source_count": store.rows.len(),
        "tombstone_count": store.tombstones.len(),
        "tombstones": store.tombstones.iter().map(|(id, fence)| json!({"id": id, "fence": fence})).collect::<Vec<_>>(),
        "cells": cells_json(&store.rows),
        "statements": statements,
        "admission": admission,
        "customer_secret_reused": false,
        "production_authorization": false,
    })
}

fn pretty(value: &Json) -> Result<String, String> {
    serde_json::to_string_pretty(value).map_err(|err| err.to_string())
}

fn refuse_code(code: &str, extra: Json) -> Result<String, String> {
    let mut body = json!({
        "armed": false,
        "before_read": true,
        "code": code,
        "method": null,
        "grade": null,
        "capture": null,
    });
    if let (Some(map), Some(extra)) = (body.as_object_mut(), extra.as_object()) {
        for (key, value) in extra {
            map.insert(key.clone(), value.clone());
        }
    }
    pretty(&body)
}

pub async fn execute(path: &Path, kind: Kind) -> Result<String, RunError> {
    let request: JobRequest = read_job(path).map_err(RunError::Input)?;
    let facts = request
        .facts
        .as_ref()
        .filter(|_| request.capture.sync == SyncMethod::Cdc);
    let admission = match gate(&request.capture, facts) {
        Ok(decision) => decision,
        Err(GateError::Admission(decision)) => {
            let body = pretty(&json!(decision)).map_err(RunError::Input)?;
            return Err(RunError::Refuse(body));
        }
        Err(GateError::Code(code)) => {
            let body =
                refuse_code(code, json!({ "before_read": true })).map_err(RunError::Input)?;
            return Err(RunError::Refuse(body));
        }
    };
    check_idents(&request).map_err(RunError::Input)?;
    if request.label.is_empty()
        || request.label.len() > 80
        || !request.label.bytes().all(label_byte)
    {
        return Err(RunError::Input("label".to_string()));
    }
    let user = fs::read_to_string(&request.maria_user_file)
        .map_err(|err| RunError::Input(format!("reader user file: {err}")))?;
    let password = fs::read_to_string(&request.maria_password_file)
        .map_err(|err| RunError::Input(format!("reader password file: {err}")))?;
    let pg_password = fs::read_to_string(&request.pg_password_file)
        .map_err(|err| RunError::Input(format!("target password file: {err}")))?;
    let user = user.trim().to_string();
    let password = password.trim().to_string();
    let pg_password = pg_password.trim().to_string();
    if user.is_empty() || password.is_empty() || pg_password.is_empty() {
        return Err(RunError::Input("secret empty".to_string()));
    }
    let secrets = [password.as_str(), pg_password.as_str()];
    let result = match kind {
        Kind::Cycle => run_cycle(&request, &admission, &user, &password, &pg_password).await,
        Kind::Schedule => run_schedule(&request, &admission, &user, &password, &pg_password).await,
    };
    result.map_err(|err| match err {
        Stop::Refuse(body) => RunError::Refuse(body),
        Stop::Fail(err) => RunError::Fail(config::redact(&err, &secrets)),
    })
}

/// One read-only capture that writes a producer receipt for `import-native-update`.
///
/// The receipt bytes are built from this run's driver read, catalog, config, and
/// audit. Arrow bytes come from `mariadb_probe::encode::write_ipc` (arrow-ipc
/// 60.0.0, 8-byte alignment, metadata V5). `encode_fixture_arrow` stays limited
/// to int64 and utf8 and is not the product encoder.
/// This command does not open the C5 target and does not write MariaDB.
pub async fn produce(path: &Path) -> Result<String, RunError> {
    let job: ProduceJob = read_produce(path).map_err(RunError::Input)?;
    let facts = None;
    let admission = match gate(&job.capture, facts) {
        Ok(decision) => decision,
        Err(GateError::Admission(decision)) => {
            let body = pretty(&json!(decision)).map_err(RunError::Input)?;
            return Err(RunError::Refuse(body));
        }
        Err(GateError::Code(code)) => {
            let body =
                refuse_code(code, json!({ "before_read": true })).map_err(RunError::Input)?;
            return Err(RunError::Refuse(body));
        }
    };
    if job.schema != SOURCE_SCHEMA
        || job.table != "k_repr"
        || !ident_ok(&job.schema)
        || !ident_ok(&job.table)
    {
        return Err(RunError::Input(
            "object is outside the C5 representative fixture".to_string(),
        ));
    }
    if job.label.is_empty() || job.label.len() > 80 || !job.label.bytes().all(label_byte) {
        return Err(RunError::Input("label".to_string()));
    }
    if job.run_id.is_empty() || job.run_id.len() > 80 || !job.run_id.bytes().all(label_byte) {
        return Err(RunError::Input("run_id".to_string()));
    }
    if job.output_dir.is_empty() || job.output_dir.contains('\0') {
        return Err(RunError::Input("output_dir".to_string()));
    }
    let user = fs::read_to_string(&job.maria_user_file)
        .map_err(|err| RunError::Input(format!("reader user file: {err}")))?;
    let password = fs::read_to_string(&job.maria_password_file)
        .map_err(|err| RunError::Input(format!("reader password file: {err}")))?;
    let user = user.trim().to_string();
    let password = password.trim().to_string();
    if user.is_empty() || password.is_empty() {
        return Err(RunError::Input("secret empty".to_string()));
    }
    let request = JobRequest {
        capture: job.capture.clone(),
        facts: None,
        label: job.label.clone(),
        schema: job.schema.clone(),
        table: job.table.clone(),
        maria_host: job.maria_host.clone(),
        maria_port: job.maria_port,
        maria_user_file: job.maria_user_file.clone(),
        maria_password_file: job.maria_password_file.clone(),
        pg_host: String::new(),
        pg_port: 0,
        pg_user: String::new(),
        pg_password_file: String::new(),
        pg_database: String::new(),
        interrupt_before_commit: false,
        observe_ms: 0,
        ready_path: None,
    };
    let secrets = [password.as_str()];
    let result = write_produce(&job, &request, &admission, &user, &password).await;
    result.map_err(|err| match err {
        Stop::Refuse(body) => RunError::Refuse(body),
        Stop::Fail(err) => RunError::Fail(config::redact(&err, &secrets)),
    })
}

#[derive(Debug, Deserialize)]
struct PriorRow {
    id: i64,
    note: Option<String>,
    qty: Option<i64>,
    money: Option<String>,
}

#[derive(Deserialize)]
struct ProduceJob {
    capture: CaptureRequest,
    label: String,
    run_id: String,
    schema: String,
    table: String,
    maria_host: String,
    maria_port: u16,
    maria_user_file: String,
    maria_password_file: String,
    output_dir: String,
    #[serde(default)]
    source_generation: Option<i64>,
    #[serde(default)]
    parent_source_generation: Option<i64>,
    #[serde(default)]
    prior_rows: Option<Vec<PriorRow>>,
    #[serde(default)]
    permanent_absent: Vec<i64>,
}

struct GenerationPlan {
    generation: i64,
    parent: Option<i64>,
    prior: Option<BTreeMap<i64, Repr>>,
}

fn generation_plan(job: &ProduceJob) -> Result<GenerationPlan, String> {
    let generation = job.source_generation.unwrap_or(1);
    if generation < 1 {
        return Err("source_generation".to_string());
    }
    match job.parent_source_generation {
        None => {
            if generation != 1 {
                return Err("parent_source_generation required".to_string());
            }
            if job.prior_rows.is_some() {
                return Err("prior_rows without parent".to_string());
            }
            Ok(GenerationPlan {
                generation: 1,
                parent: None,
                prior: None,
            })
        }
        Some(parent) => {
            if parent.checked_add(1) != Some(generation) {
                return Err("stale_run".to_string());
            }
            let prior = job
                .prior_rows
                .as_ref()
                .ok_or_else(|| "prior_rows required".to_string())?;
            let rows: Vec<Repr> = prior
                .iter()
                .map(|row| Repr {
                    id: row.id,
                    note: row.note.clone(),
                    qty: row.qty,
                    money: row.money.clone(),
                })
                .collect();
            Ok(GenerationPlan {
                generation,
                parent: Some(parent),
                prior: Some(index_rows(&rows)?),
            })
        }
    }
}

struct ColumnFact {
    name: String,
    data_type: String,
    nullable: bool,
    precision: Option<i64>,
    scale: Option<i64>,
    char_length: Option<i64>,
    column_type: String,
}

fn read_produce(path: &Path) -> Result<ProduceJob, String> {
    let bytes = fs::read(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|err| format!("json {}: {err}", path.display()))
}

async fn write_produce(
    job: &ProduceJob,
    request: &JobRequest,
    admission: &Decision,
    user: &str,
    password: &str,
) -> Result<String, Stop> {
    let mut statements = Vec::new();
    let mut maria = connect_maria(request, user, password).await?;
    let version_sql = "SELECT @@version, @@log_bin, @@binlog_format, @@gtid_current_pos";
    statements.push(version_sql.to_string());
    let version_rows = maria_query(&mut maria, version_sql).await?;
    let columns = read_column_facts(&mut maria, &job.schema, &job.table, &mut statements).await?;
    let read = read_source(&mut maria, request, &mut statements).await;
    let _ = maria.disconnect().await;
    let (simple, rows) = read?;
    if let Err(fault) = classify_columns(&simple) {
        let code = match fault {
            SchemaFault::NewColumn(_) => "new_column",
            SchemaFault::Broken { .. } => "broken_schema",
        };
        return Err(Stop::Refuse(refuse_code(
            code,
            json!({ "before_read": false, "columns": simple }),
        )?));
    }
    let indexed = index_rows(&rows)?;
    for id in &job.permanent_absent {
        if indexed.contains_key(id) {
            return Err(Stop::Refuse(refuse_code(
                "resurrection",
                json!({ "id": id, "before_read": false }),
            )?));
        }
    }
    let plan = generation_plan(job)?;
    let generation = plan.generation;
    let parent = plan.parent;
    let delta = match &plan.prior {
        Some(prior_rows) => Some(capture_delta(prior_rows, &indexed, &job.permanent_absent)?),
        None => None,
    };
    let upserts: Vec<Repr> = match &delta {
        Some(item) => item.upserts.clone(),
        None => indexed.values().cloned().collect(),
    };
    let tombstones: Vec<i64> = delta
        .as_ref()
        .map(|item| item.deletes.clone())
        .unwrap_or_default();
    let live_rows: Vec<Repr> = indexed.values().cloned().collect();
    let delete_observed = !tombstones.is_empty();
    let snapshot = hash_of(&indexed);
    let server = version_rows
        .into_iter()
        .next()
        .ok_or_else(|| "version missing".to_string())?;
    let server = take_text(server, 4)?;
    let gaps = encoder_gap(&columns);
    let blocked = gaps
        .iter()
        .any(|item| item.get("reason").and_then(Json::as_str) != Some("exact"));
    let arrow = if blocked || upserts.is_empty() {
        None
    } else {
        Some(encode_exact(&columns, &upserts)?)
    };
    let out = Path::new(&job.output_dir);
    fs::create_dir_all(out).map_err(|err| format!("output dir: {err}"))?;
    let catalog = catalog_body(job, &columns, live_rows.len(), &snapshot, generation);
    let config = config_body(job);
    let input = input_body(job);
    let registry = registry_body(job, rows.len());
    let audit = json!({
        "statements": statements,
        "product_dml_ddl": 0,
        "reader": user
    });
    let cells = cells_body(&rows);
    let catalog_bytes = write_doc(out, "catalog.json", &catalog)?;
    let config_bytes = write_doc(out, "config.json", &config)?;
    let input_bytes = write_doc(out, "input.json", &input)?;
    let registry_bytes = write_doc(out, "registry.json", &registry)?;
    let audit_bytes = write_doc(out, "audit.json", &audit)?;
    let _cells_bytes = write_doc(out, "cells.json", &cells)?;
    let schema_digest = digest_bytes(&schema_preimage(&columns));
    let type_digest = digest_bytes(&type_preimage(&columns));
    let config_digest = digest_bytes(&config_bytes);
    let input_digest = digest_bytes(&input_bytes);
    let audit_digest = digest_bytes(&audit_bytes);
    let catalog_sha256 = digest_bytes(&catalog_bytes);
    let registry_sha256 = digest_bytes(&registry_bytes);
    let driver_digest = digest_bytes(
        &fs::read(std::env::current_exe().map_err(|err| format!("exe: {err}"))?)
            .map_err(|err| format!("exe read: {err}"))?,
    );
    let (arrow_sha, arrow_schema) = match &arrow {
        None => (Vec::new(), None),
        Some(bytes) => {
            let path = out.join("capture.arrow");
            fs::write(&path, bytes).map_err(|err| format!("arrow: {err}"))?;
            let schema = oo_pipeline::native_arrow::arrow_schema_sha256(bytes)
                .map_err(ToString::to_string)?;
            (
                vec![oo_pipeline::native_arrow::arrow_sha256(bytes)],
                Some(schema),
            )
        }
    };
    let receipt = receipt_body(
        job,
        generation,
        parent,
        &snapshot,
        &schema_digest,
        &type_digest,
        &config_digest,
        &input_digest,
        &audit_digest,
        &catalog_sha256,
        &registry_sha256,
        &driver_digest,
        &arrow_sha,
        arrow_schema.as_deref(),
    );
    let receipt_bytes = write_doc(out, "producer-receipt.json", &receipt)?;
    let receipt_sha = digest_bytes(&receipt_bytes);
    let grade = admission
        .grade
        .as_deref()
        .ok_or_else(|| "grade_unknown".to_string())?;
    let delete_detection = admission
        .delete_detection
        .as_deref()
        .ok_or_else(|| "delete_unknown".to_string())?;
    let load = load_body(
        job,
        &columns,
        &live_rows,
        generation,
        parent,
        &tombstones,
        &snapshot,
        grade,
        delete_detection,
        &schema_digest,
        &type_digest,
        &config_digest,
        &input_digest,
        &audit_digest,
        &receipt_sha,
        out,
        arrow.is_some(),
    );
    let _load_bytes = write_doc(out, "load-document.json", &load)?;
    let gap_doc = json!({
        "encoder": "mariadb_probe::encode::write_ipc",
        "arrow_crate": "arrow-ipc =60.0.0",
        "ipc_alignment": 8,
        "ipc_metadata_version": "V5",
        "fixture_encoder": "oo_pipeline::native_update::encode_fixture_arrow",
        "fixture_closed_kinds": ["int64", "utf8"],
        "blocked": blocked,
        "columns": gaps,
        "arrow_written": arrow.is_some(),
        "arrow_schema_sha256": arrow_schema,
        "note": "Product cells are an Arrow IPC stream from this source probe writer. encode_fixture_arrow is not used. The schema message hash is not the catalog preimage."
    });
    let _gap_bytes = write_doc(out, "encode-gap.json", &gap_doc)?;
    let delta_doc = json!({
        "source_generation": generation,
        "parent_source_generation": parent,
        "inserts": delta.as_ref().map(|item| item.inserts.clone()).unwrap_or_default(),
        "updates": delta.as_ref().map(|item| item.updates.clone()).unwrap_or_default(),
        "tombstones": tombstones,
        "upsert_count": upserts.len(),
        "live_count": live_rows.len(),
        "permanent_absent": job.permanent_absent,
        "delete_observed_this_run": delete_observed
    });
    let _delta_bytes = write_doc(out, "delta.json", &delta_doc)?;
    let profile = json!({
        "attachment": "not_attached",
        "measured": true,
        "engine": "mariadb",
        "version": server.first().cloned().flatten(),
        "log_bin": server.get(1).cloned().flatten(),
        "binlog_format": server.get(2).cloned().flatten(),
        "gtid_current_pos": server.get(3).cloned().flatten(),
        "source_id": "oo_c5_k_repr",
        "logical_schema": job.schema,
        "logical_table": job.table,
        "row_count": live_rows.len(),
        "snapshot_sha256": snapshot,
        "grade": admission.grade,
        "capture": admission.capture,
        "delete_detection": admission.delete_detection,
        "delete_observed_this_run": delete_observed,
        "warning": admission.warnings,
        "product_dml_ddl": 0
    });
    let _profile_bytes = write_doc(out, "profile.json", &profile)?;
    let native_probe = json!({
        "protocol": oo_pipeline::native_arrow::IMPORT_PROTOCOL,
        "source_engine": "mariadb",
        "source_id": "oo_c5_k_repr",
        "source_generation": generation,
        "parent_source_generation": parent,
        "target_storage": "postgresql"
    });
    let _probe_bytes = write_doc(out, "import-native-probe.json", &native_probe)?;
    let sealed_probe = json!({
        "protocol": oo_pipeline::native_catalog::REQUEST_PROTOCOL,
        "source_id": "oo_c5_k_repr",
        "source_generation": generation,
        "parent_source_generation": parent,
        "config_digest": config_digest,
        "schema_digest": schema_digest,
        "type_digest": type_digest,
        "input_digest": input_digest
    });
    let _sealed_bytes = write_doc(out, "catalog-native-probe.json", &sealed_probe)?;
    let summary = json!({
        "label": job.label,
        "run_id": job.run_id,
        "source_engine": "mariadb",
        "source_id": "oo_c5_k_repr",
        "source_generation": generation,
        "parent_source_generation": parent,
        "snapshot_sha256": snapshot,
        "row_count": live_rows.len(),
        "tombstones": tombstones,
        "receipt_sha256": receipt_sha,
        "schema_digest": schema_digest,
        "type_digest": type_digest,
        "config_digest": config_digest,
        "input_digest": input_digest,
        "audit_digest": audit_digest,
        "catalog_sha256": catalog_sha256,
        "registry_sha256": registry_sha256,
        "driver_digest": driver_digest,
        "arrow_sha256": arrow_sha,
        "arrow_schema_sha256": arrow_schema,
        "encoder_blocked": blocked,
        "identity_digests_match": identity_match(&receipt, &load),
        "grade": admission.grade,
        "delete_detection": admission.delete_detection,
        "delete_observed_this_run": delete_observed,
        "target_write": false,
        "customer_secret_reused": false,
        "production_authorization": false
    });
    Ok(pretty(&summary)?)
}

fn identity_match(receipt: &Json, load: &Json) -> bool {
    for name in [
        "source_engine",
        "source_id",
        "source_generation",
        "schema_digest",
        "type_digest",
        "config_digest",
        "input_digest",
        "audit_digest",
    ] {
        if receipt.get(name) != load.get(name) {
            return false;
        }
    }
    receipt.pointer("/snapshot/kind") == load.pointer("/snapshot/kind")
        && receipt.pointer("/snapshot/value") == load.pointer("/snapshot/value")
}

fn write_doc(dir: &Path, name: &str, value: &Json) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec(value).map_err(|err| format!("json {name}: {err}"))?;
    bytes.push(b'\n');
    fs::write(dir.join(name), &bytes).map_err(|err| format!("write {name}: {err}"))?;
    Ok(bytes)
}

fn digest_bytes(bytes: &[u8]) -> String {
    hex32(Sha256::digest(bytes).as_slice())
}

fn schema_preimage(columns: &[ColumnFact]) -> Vec<u8> {
    let mut out = String::new();
    for column in columns {
        out.push_str(&column.name);
        out.push('\t');
        out.push_str(&column.data_type);
        out.push('\t');
        out.push_str(if column.nullable { "yes" } else { "no" });
        out.push('\t');
        out.push_str(&opt_text(column.precision));
        out.push('\t');
        out.push_str(&opt_text(column.scale));
        out.push('\t');
        out.push_str(&opt_text(column.char_length));
        out.push('\t');
        out.push_str(&column.column_type);
        out.push('\n');
    }
    out.into_bytes()
}

fn type_preimage(columns: &[ColumnFact]) -> Vec<u8> {
    let mut out = String::new();
    for column in columns {
        let arrow = match column.data_type.as_str() {
            "bigint" => "int64",
            "varchar" | "char" | "text" => "utf8",
            "int" | "integer" => "int32",
            "decimal" | "numeric" => "decimal128",
            _ => "encoder_kind_absent",
        };
        out.push_str(&column.name);
        out.push('\t');
        out.push_str(arrow);
        out.push('\t');
        out.push_str(&opt_text(column.precision));
        out.push('\t');
        out.push_str(&opt_text(column.scale));
        out.push('\n');
    }
    out.into_bytes()
}

fn opt_text(value: Option<i64>) -> String {
    value
        .map(|item| item.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn catalog_body(
    job: &ProduceJob,
    columns: &[ColumnFact],
    rows: usize,
    snapshot: &str,
    generation: i64,
) -> Json {
    json!({
        "attachment": "not_attached",
        "measured": true,
        "sealed": false,
        "protocol": "oo.mariadb.c5-capture-catalog/v1",
        "engine": "mariadb",
        "source_id": "oo_c5_k_repr",
        "logical_schema": job.schema,
        "logical_table": job.table,
        "source_generation": generation,
        "row_count": rows,
        "snapshot_sha256": snapshot,
        "columns": columns.iter().map(column_json).collect::<Vec<_>>()
    })
}

fn column_json(column: &ColumnFact) -> Json {
    json!({
        "name": column.name,
        "data_type": column.data_type,
        "nullable": column.nullable,
        "precision": column.precision,
        "scale": column.scale,
        "char_length": column.char_length,
        "column_type": column.column_type
    })
}

fn config_body(job: &ProduceJob) -> Json {
    json!({
        "source_id": "oo_c5_k_repr",
        "source_engine": "mariadb",
        "requested_write_class": "R0",
        "r1w": null,
        "sync": "full",
        "capture": "C5",
        "logical_schema": job.schema,
        "logical_table": job.table,
        "customer_secret_reused": false,
        "production_authorization": false
    })
}

fn input_body(job: &ProduceJob) -> Json {
    json!({
        "source_id": "oo_c5_k_repr",
        "sync": "full",
        "consistency": "per_table",
        "detect_every_ms": job.capture.detect_every_ms,
        "reconcile_every_ms": job.capture.reconcile_every_ms,
        "window": job.capture.window,
        "stable_key": job.capture.stable_key,
        "cursor_safety": "commit_ordered",
        "read_from": "primary",
        "allow_write_capable_credential": false,
        "logical_schema": job.schema,
        "logical_table": job.table
    })
}

fn registry_body(job: &ProduceJob, rows: usize) -> Json {
    json!({
        "protocol": "oo.mariadb.c5-object-registry/v1",
        "source_id": "oo_c5_k_repr",
        "source_engine": "mariadb",
        "objects": [{
            "logical_schema": job.schema,
            "logical_table": job.table,
            "row_count": rows
        }]
    })
}

fn cells_body(rows: &[Repr]) -> Json {
    Json::Array(
        rows.iter()
            .map(|row| {
                json!({
                    "id": row.id,
                    "note": row.note,
                    "qty": row.qty,
                    "money": row.money
                })
            })
            .collect(),
    )
}

#[allow(clippy::too_many_arguments)]
fn receipt_body(
    job: &ProduceJob,
    generation: i64,
    parent: Option<i64>,
    snapshot: &str,
    schema_digest: &str,
    type_digest: &str,
    config_digest: &str,
    input_digest: &str,
    audit_digest: &str,
    catalog_sha256: &str,
    registry_sha256: &str,
    driver_digest: &str,
    arrow_sha: &[String],
    arrow_schema: Option<&str>,
) -> Json {
    let mut receipt = json!({
        "schema": oo_pipeline::native_arrow::CAPTURE_RECEIPT_PROTOCOL,
        "source_engine": "mariadb",
        "source_id": "oo_c5_k_repr",
        "source_generation": generation,
        "snapshot": {"kind": "snapshot-sha256", "value": snapshot},
        "run_id": job.run_id,
        "schema_digest": schema_digest,
        "type_digest": type_digest,
        "config_digest": config_digest,
        "input_digest": input_digest,
        "audit_digest": audit_digest,
        "catalog_sha256": catalog_sha256,
        "registry_sha256": registry_sha256,
        "driver_digest": driver_digest,
        "arrow_sha256": arrow_sha
    });
    if let Some(parent) = parent {
        receipt["parent_source_generation"] = json!(parent);
    }
    if let Some(schema) = arrow_schema {
        receipt["arrow_schema_sha256"] = json!(schema);
    }
    receipt
}

#[allow(clippy::too_many_arguments)]
fn load_body(
    job: &ProduceJob,
    columns: &[ColumnFact],
    rows: &[Repr],
    generation: i64,
    parent: Option<i64>,
    tombstones: &[i64],
    snapshot: &str,
    grade: &str,
    delete_detection: &str,
    schema_digest: &str,
    type_digest: &str,
    config_digest: &str,
    input_digest: &str,
    audit_digest: &str,
    receipt_sha: &str,
    out: &Path,
    arrow_written: bool,
) -> Json {
    let mut document = json!({
        "protocol": oo_pipeline::native_update::REQUEST_PROTOCOL,
        "source_engine": "mariadb",
        "source_id": "oo_c5_k_repr",
        "source_generation": generation,
        "logical_schema": job.schema,
        "logical_table": job.table,
        "batch_id": job.run_id,
        "capture_method": "c5_diff",
        "delete_detection": delete_detection,
        "consistency_grade": grade,
        "cross_table_consistent": false,
        "position_codec": "snapshot-zero/v1",
        "position_lineage": "mariadb-c5.snapshot",
        "applied_pos": "0",
        "config_digest": config_digest,
        "schema_digest": schema_digest,
        "type_digest": type_digest,
        "input_digest": input_digest,
        "audit_digest": audit_digest,
        "snapshot": {"kind": "snapshot-sha256", "value": snapshot},
        "columns": columns.iter().map(declared_column).collect::<Vec<_>>(),
        "tombstones": tombstones.iter().map(|id| id.to_string()).collect::<Vec<_>>(),
        "truth": truth_rows(rows),
        "row_identity": "unique",
        "key_column": "id",
        "mirror": false,
        "admission_path": out.join("producer-receipt.json").display().to_string(),
        "admission_expect": {"declared_receipt_sha256": receipt_sha}
    });
    if let Some(parent) = parent {
        document["parent_source_generation"] = json!(parent);
    }
    if arrow_written {
        document["arrow_path"] = json!(out.join("capture.arrow").display().to_string());
    }
    document
}

fn declared_column(column: &ColumnFact) -> Json {
    match column.data_type.as_str() {
        "bigint" => json!({
            "name": column.name,
            "logical_name": column.name,
            "arrow_type": "int64",
            "nullable": column.nullable,
            "precision": 64
        }),
        "varchar" | "char" | "text" => {
            let mut declared = json!({
                "name": column.name,
                "logical_name": column.name,
                "arrow_type": "utf8",
                "nullable": column.nullable
            });
            if let Some(len) = loader_char_length(column) {
                declared["char_length"] = json!(len);
            }
            declared
        }
        "int" | "integer" => json!({
            "name": column.name,
            "logical_name": column.name,
            "arrow_type": "int32",
            "nullable": column.nullable,
            "precision": 32
        }),
        "decimal" | "numeric" => json!({
            "name": column.name,
            "logical_name": column.name,
            "arrow_type": "decimal128",
            "nullable": column.nullable,
            "precision": column.precision,
            "scale": column.scale
        }),
        _ => json!({
            "name": column.name,
            "logical_name": column.name,
            "arrow_type": "encoder_kind_absent",
            "nullable": column.nullable
        }),
    }
}

fn truth_rows(rows: &[Repr]) -> Json {
    Json::Array(
        rows.iter()
            .map(|row| {
                json!([
                    row.id.to_string(),
                    row.note.clone(),
                    row.qty.map(|value| value.to_string()),
                    row.money.clone()
                ])
            })
            .collect(),
    )
}

fn loader_char_length(column: &ColumnFact) -> Option<i64> {
    match column.char_length {
        Some(len) if (1..=63).contains(&len) => Some(len),
        _ => None,
    }
}

fn encoder_gap(columns: &[ColumnFact]) -> Vec<Json> {
    columns.iter().map(|column| {
        let (arrow_type, reason, detail) = match column.data_type.as_str() {
            "bigint" => (
                "int64",
                "exact",
                "signed bigint is Arrow int64. The catalog keeps MariaDB numeric_precision. The loader key declaration is precision 64.".to_string(),
            ),
            "varchar" | "char" | "text" => {
                let detail = match column.char_length {
                    Some(len) if loader_char_length(column).is_none() => format!(
                        "character data is Arrow utf8. Source character_maximum_length {len} is outside the loader bpchar cap 1..=63, so the load declaration omits char_length and is varchar oid 1043. Cells are not truncated."
                    ),
                    _ => "character data is Arrow utf8".to_string(),
                };
                ("utf8", "exact", detail)
            }
            "int" | "integer" => (
                "int32",
                "exact",
                "signed int is Arrow int32. encode_fixture_arrow has no int32 kind and is not used. Encoding as int64 would change the PostgreSQL oid from 23 to 20.".to_string(),
            ),
            "decimal" | "numeric" => (
                "decimal128",
                "exact",
                "decimal and numeric are Arrow decimal128 at the measured precision and scale. encode_fixture_arrow has no decimal kind and is not used.".to_string(),
            ),
            _ => (
                "encoder_kind_absent",
                "encoder_kind_absent",
                "no closed kind for this source type".to_string(),
            ),
        };
        json!({
            "name": column.name,
            "data_type": column.data_type,
            "nullable": column.nullable,
            "source_char_length": column.char_length,
            "loader_char_length": loader_char_length(column),
            "loader_arrow_type": arrow_type,
            "reason": reason,
            "detail": detail
        })
    }).collect()
}

fn encode_exact(columns: &[ColumnFact], rows: &[Repr]) -> Result<Vec<u8>, String> {
    if columns.is_empty() || rows.is_empty() {
        return Err("empty".to_string());
    }
    let mut outs = Vec::with_capacity(columns.len());
    for column in columns {
        let plan = match column.data_type.as_str() {
            "bigint" => mariadb_probe::decide::Plan {
                kind: mariadb_probe::decide::Kind::Int {
                    bits: 64,
                    unsigned: false,
                },
                caveats: Vec::new(),
            },
            "varchar" | "char" | "text" => mariadb_probe::decide::Plan {
                kind: mariadb_probe::decide::Kind::Utf8 { provenance: None },
                caveats: Vec::new(),
            },
            "int" | "integer" => mariadb_probe::decide::Plan {
                kind: mariadb_probe::decide::Kind::Int {
                    bits: 32,
                    unsigned: false,
                },
                caveats: Vec::new(),
            },
            "decimal" | "numeric" => {
                let precision =
                    u8::try_from(column.precision.ok_or_else(|| "precision".to_string())?)
                        .map_err(|_| "precision".to_string())?;
                let scale =
                    u8::try_from(column.scale.unwrap_or(0)).map_err(|_| "precision".to_string())?;
                if precision == 0 || precision > 38 || scale > precision {
                    return Err("precision".to_string());
                }
                mariadb_probe::decide::Plan {
                    kind: mariadb_probe::decide::Kind::Decimal { precision, scale },
                    caveats: Vec::new(),
                }
            }
            _ => return Err("encoder_kind_absent".to_string()),
        };
        outs.push(mariadb_probe::encode::ColumnOut {
            name: column.name.clone(),
            nullable: column.nullable,
            plan,
            metadata: std::collections::HashMap::new(),
        });
    }
    let mut encoded_rows = Vec::with_capacity(rows.len());
    for row in rows {
        let mut cells = Vec::with_capacity(columns.len());
        for column in columns {
            cells.push(encode_cell(column, row)?);
        }
        encoded_rows.push(cells);
    }
    let table = mariadb_probe::encode::write_ipc(
        &outs,
        &encoded_rows,
        std::collections::HashMap::new(),
        rows.len(),
    )
    .map_err(|err| err.to_string())?;
    let decoded = oo_pipeline::native_arrow::decode_arrow_stream(&table.bytes)
        .map_err(|fault| fault.reason().to_string())?;
    if decoded.rows != rows.len() || decoded.fields.len() != columns.len() {
        return Err("bad_type".to_string());
    }
    for (field, column) in decoded.fields.iter().zip(columns) {
        let expect = match column.data_type.as_str() {
            "bigint" => "int64",
            "varchar" | "char" | "text" => "utf8",
            "int" | "integer" => "int32",
            "decimal" | "numeric" => "decimal128",
            _ => return Err("encoder_kind_absent".to_string()),
        };
        if field.name != column.name
            || field.nullable != column.nullable
            || field.arrow_type != expect
        {
            return Err("bad_type".to_string());
        }
        if expect == "decimal128"
            && (field.precision != narrow_i32(column.precision)
                || field.scale != narrow_i32(column.scale))
        {
            return Err("precision".to_string());
        }
        if expect == "int32" && field.precision != Some(32) {
            return Err("bad_type".to_string());
        }
        if expect == "int64" && field.precision != Some(64) {
            return Err("bad_type".to_string());
        }
    }
    Ok(table.bytes)
}

fn narrow_i32(value: Option<i64>) -> Option<i32> {
    value.and_then(|item| i32::try_from(item).ok())
}

pub fn dump_arrow(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|err| format!("arrow: {err}"))?;
    let decoded = oo_pipeline::native_arrow::decode_arrow_stream(&bytes)
        .map_err(|fault| fault.reason().to_string())?;
    let expect = [
        ("id", "int64", false),
        ("note", "utf8", true),
        ("qty", "int32", true),
        ("money", "decimal128", true),
    ];
    if decoded.fields.len() != expect.len() {
        return Err("bad_type".to_string());
    }
    for (field, (name, kind, nullable)) in decoded.fields.iter().zip(expect) {
        if field.name != name || field.arrow_type != kind || field.nullable != nullable {
            return Err(format!("bad_type {}", field.name));
        }
    }
    if decoded.fields[2].precision != Some(32)
        || decoded.fields[3].precision != Some(12)
        || decoded.fields[3].scale != Some(2)
    {
        return Err("precision".to_string());
    }
    let mut lines = Vec::with_capacity(decoded.rows);
    for index in 0..decoded.rows {
        let id = match &decoded.columns[0][index] {
            oo_pipeline::native_arrow::NativeCell::Int {
                bit_width: 64,
                value,
            } => value.to_string(),
            _ => return Err("bad_type".to_string()),
        };
        let note = match &decoded.columns[1][index] {
            oo_pipeline::native_arrow::NativeCell::Null => "\\N".to_string(),
            oo_pipeline::native_arrow::NativeCell::Utf8(text) => text.clone(),
            _ => return Err("bad_type".to_string()),
        };
        let qty = match &decoded.columns[2][index] {
            oo_pipeline::native_arrow::NativeCell::Null => "\\N".to_string(),
            oo_pipeline::native_arrow::NativeCell::Int {
                bit_width: 32,
                value,
            } => value.to_string(),
            _ => return Err("bad_type".to_string()),
        };
        let money = match &decoded.columns[3][index] {
            oo_pipeline::native_arrow::NativeCell::Null => "\\N".to_string(),
            oo_pipeline::native_arrow::NativeCell::Decimal {
                precision: 12,
                scale: 2,
                coefficient,
            } => format_money(*coefficient)?,
            _ => return Err("bad_type".to_string()),
        };
        lines.push(format!("{id}\t{note}\t{qty}\t{money}"));
    }
    if lines.is_empty() {
        return Err("empty".to_string());
    }
    Ok(lines.join("\n") + "\n")
}

fn format_money(coefficient: i128) -> Result<String, String> {
    let negative = coefficient < 0;
    let raw = coefficient.unsigned_abs().to_string();
    let digits = if raw.len() < 3 {
        format!("{raw:0>3}")
    } else {
        raw
    };
    let split = digits.len() - 2;
    let whole_raw = digits[..split].trim_start_matches('0');
    let whole = if whole_raw.is_empty() { "0" } else { whole_raw };
    let frac = &digits[split..];
    if negative && !(whole == "0" && frac == "00") {
        Ok(format!("-{whole}.{frac}"))
    } else {
        Ok(format!("{whole}.{frac}"))
    }
}

fn encode_cell(column: &ColumnFact, row: &Repr) -> Result<mariadb_probe::encode::Cell, String> {
    match column.name.as_str() {
        "id" => Ok(mariadb_probe::encode::Cell::I64(row.id)),
        "note" => match &row.note {
            Some(text) => Ok(mariadb_probe::encode::Cell::Utf8(text.clone())),
            None if column.nullable => Ok(mariadb_probe::encode::Cell::Null),
            None => Err("null_violation".to_string()),
        },
        "qty" => match row.qty {
            Some(value) => {
                i32::try_from(value).map_err(|_| format!("qty {value}"))?;
                Ok(mariadb_probe::encode::Cell::I64(value))
            }
            None if column.nullable => Ok(mariadb_probe::encode::Cell::Null),
            None => Err("null_violation".to_string()),
        },
        "money" => match &row.money {
            Some(text) => {
                let unscaled = mariadb_probe::decimal::parse_decimal(text, 12, 2)
                    .map_err(|err| err.to_string())?;
                Ok(mariadb_probe::encode::Cell::Decimal(unscaled))
            }
            None if column.nullable => Ok(mariadb_probe::encode::Cell::Null),
            None => Err("null_violation".to_string()),
        },
        _ => Err("encoder_kind_absent".to_string()),
    }
}

async fn read_column_facts(
    conn: &mut Conn,
    schema: &str,
    table: &str,
    statements: &mut Vec<String>,
) -> Result<Vec<ColumnFact>, String> {
    let sql = "SELECT column_name, data_type, is_nullable, numeric_precision, numeric_scale, character_maximum_length, column_type FROM information_schema.columns WHERE table_schema = ? AND table_name = ? ORDER BY ordinal_position";
    statements.push(sql.to_string());
    let rows = maria_exec(conn, sql, schema, table).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let values = take_text(row, 7)?;
        let name = values
            .first()
            .cloned()
            .flatten()
            .ok_or_else(|| "column name".to_string())?;
        let data_type = values
            .get(1)
            .cloned()
            .flatten()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let nullable = values.get(2).and_then(Option::as_deref) == Some("YES");
        out.push(ColumnFact {
            name,
            data_type,
            nullable,
            precision: opt_i64(values.get(3).cloned().flatten()),
            scale: opt_i64(values.get(4).cloned().flatten()),
            char_length: opt_i64(values.get(5).cloned().flatten()),
            column_type: values.get(6).cloned().flatten().unwrap_or_default(),
        });
    }
    Ok(out)
}

fn opt_i64(value: Option<String>) -> Option<i64> {
    value.and_then(|text| text.parse().ok())
}

fn label_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
}

fn check_idents(request: &JobRequest) -> Result<(), String> {
    if !ident_ok(&request.schema) || !ident_ok(&request.table) {
        return Err("closed identifier".to_string());
    }
    if request.schema != SOURCE_SCHEMA || request.table != "k_repr" {
        return Err("object is outside the C5 representative fixture".to_string());
    }
    Ok(())
}

fn read_job(path: &Path) -> Result<JobRequest, String> {
    let bytes = fs::read(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|err| format!("json {}: {err}", path.display()))
}

async fn run_cycle(
    request: &JobRequest,
    admission: &Decision,
    user: &str,
    password: &str,
    pg_password: &str,
) -> Result<String, Stop> {
    let mut statements = Vec::new();
    let mut maria = connect_maria(request, user, password).await?;
    let read = read_source(&mut maria, request, &mut statements).await;
    let _ = maria.disconnect().await;
    let (columns, rows) = read?;
    if let Err(fault) = classify_columns(&columns) {
        return schema_refuse(request, pg_password, admission, &statements, fault).await;
    }
    let mut pg = connect_pg(request, pg_password).await?;
    ensure_target(&mut pg).await?;
    let mut store = load_store(&mut pg, &source_id(request)).await?;
    let now = unix_millis().to_string();
    let outcome = apply_snapshot(&mut store, &rows, request.interrupt_before_commit, &now)?;
    match outcome.kind {
        "committed" => write_commit(&mut pg, request, &store, &outcome).await?,
        "cache" | "interrupted" => write_log(&mut pg, request, &outcome).await?,
        other => return Err(format!("outcome {other}").into()),
    }
    let checked = load_store(&mut pg, &source_id(request)).await?;
    if checked.generation != outcome.generation_after
        || checked.snapshot_hash != store.snapshot_hash
        || checked.rows != store.rows
    {
        return Err("target reload differed from the committed snapshot"
            .to_string()
            .into());
    }
    let mut body = outcome_json(&outcome, &checked, admission, &request.label, &statements);
    let source_index = index_rows(&rows)?;
    body["source_cells"] = cells_json(&source_index);
    body["source_count"] = json!(rows.len());
    body["target_count"] = json!(checked.rows.len());
    body["cell_diff_prepared"] = json!(true);
    Ok(pretty(&body)?)
}

async fn schema_refuse(
    request: &JobRequest,
    pg_password: &str,
    admission: &Decision,
    statements: &[String],
    fault: SchemaFault,
) -> Result<String, Stop> {
    let (code, detail) = match fault {
        SchemaFault::Broken {
            missing,
            type_mismatch,
        } => (
            "broken_schema",
            json!({"missing": missing, "type_mismatch": type_mismatch}),
        ),
        SchemaFault::NewColumn(extras) => ("new_column", json!({"extras": extras})),
    };
    let mut pg_note = "not_recorded".to_string();
    if let Ok(mut pg) = connect_pg(request, pg_password).await
        && ensure_target(&mut pg).await.is_ok()
    {
        let store = load_store(&mut pg, &source_id(request))
            .await
            .unwrap_or_else(|_| empty_store());
        let outcome = Outcome {
            kind: "schema_refused",
            generation_before: store.generation,
            generation_after: store.generation,
            inserts: Vec::new(),
            updates: Vec::new(),
            deletes: Vec::new(),
            snapshot_hash: store.snapshot_hash.clone(),
            grade: "B",
            warning: "DELETES_LAGGED",
            published: false,
        };
        if write_log(&mut pg, request, &outcome).await.is_ok() {
            pg_note = format!("generation {}", store.generation);
        }
    }
    let body = json!({
        "armed": false,
        "before_read": false,
        "code": code,
        "method": "full",
        "grade": "B",
        "capture": "C5",
        "delete_detection": "reconcile",
        "warning": "DELETES_LAGGED",
        "detail": detail,
        "target_log": pg_note,
        "statements": statements,
        "admission": admission,
        "published": false,
        "production_authorization": false,
    });
    Err(Stop::Refuse(pretty(&body)?))
}

async fn run_schedule(
    request: &JobRequest,
    admission: &Decision,
    user: &str,
    password: &str,
    pg_password: &str,
) -> Result<String, Stop> {
    if request.observe_ms == 0 {
        return Err(Stop::Fail("observe_ms".to_string()));
    }
    let mut maria = connect_maria(request, user, password).await?;
    let mut pg = connect_pg(request, pg_password).await?;
    ensure_target(&mut pg).await?;
    if let Some(path) = &request.ready_path {
        if let Some(parent) = Path::new(path).parent() {
            fs::create_dir_all(parent).map_err(|err| format!("ready mkdir: {err}"))?;
        }
        fs::write(path, b"ready\n").map_err(|err| format!("ready write: {err}"))?;
    }
    eprintln!("c5 schedule ready {}", request.label);
    let start = Instant::now();
    let events = ticks(
        request.capture.detect_every_ms,
        request.capture.reconcile_every_ms,
        request.observe_ms,
    );
    let mut fires = Vec::new();
    for (planned, kind) in events {
        let goal = start + Duration::from_millis(planned);
        if let Some(wait) = goal.checked_duration_since(Instant::now()) {
            tokio::time::sleep(wait).await;
        }
        let actual = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        let fire = match kind {
            Tick::Detect => detect_fire(&mut maria, request, planned, actual).await?,
            Tick::Reconcile => {
                reconcile_fire(&mut maria, &mut pg, request, admission, planned, actual).await?
            }
        };
        fires.push(fire);
    }
    let _ = maria.disconnect().await;
    let store = load_store(&mut pg, &source_id(request)).await?;
    Ok(pretty(&json!({
        "label": request.label,
        "mode": "schedule",
        "observe_ms": request.observe_ms,
        "detect_every_ms": request.capture.detect_every_ms,
        "reconcile_every_ms": request.capture.reconcile_every_ms,
        "fires": fires,
        "generation": store.generation,
        "snapshot_hash": store.snapshot_hash,
        "freshness": store.freshness,
        "grade": "B",
        "warning": "DELETES_LAGGED",
        "delete_detection": "reconcile",
        "capture": "C5",
        "source_count": store.rows.len(),
        "cells": cells_json(&store.rows),
        "admission": admission,
        "production_authorization": false,
    }))?)
}

#[derive(Clone, Copy)]
enum Tick {
    Detect,
    Reconcile,
}

fn ticks(detect: u64, reconcile: u64, observe: u64) -> Vec<(u64, Tick)> {
    let mut events = Vec::new();
    for at in deadlines(detect, observe) {
        events.push((at, Tick::Detect));
    }
    for at in deadlines(reconcile, observe) {
        events.push((at, Tick::Reconcile));
    }
    events.sort_by_key(|event| {
        (
            event.0,
            match event.1 {
                Tick::Detect => 0u8,
                Tick::Reconcile => 1,
            },
        )
    });
    events
}

async fn detect_fire(
    maria: &mut Conn,
    request: &JobRequest,
    planned: u64,
    actual: u64,
) -> Result<Json, String> {
    let sql = format!(
        "SELECT CAST(COUNT(*) AS CHAR), CAST(COALESCE(SUM(CRC32(CONCAT_WS('\\t', CAST(`id` AS CHAR), IFNULL(`note`, ''), IFNULL(CAST(`qty` AS CHAR), ''), IFNULL(CAST(`money` AS CHAR), '')))), 0) AS CHAR) FROM `{}`.`{}`",
        request.schema, request.table
    );
    let rows = maria_query(maria, &sql).await?;
    let row = rows
        .into_iter()
        .next()
        .ok_or_else(|| "checksum missing".to_string())?;
    let values = take_text(row, 2)?;
    Ok(json!({
        "kind": "detect",
        "planned_ms": planned,
        "actual_ms": actual,
        "count": values.first().cloned().flatten(),
        "checksum": values.get(1).cloned().flatten(),
    }))
}

async fn reconcile_fire(
    maria: &mut Conn,
    pg: &mut sqlx::PgConnection,
    request: &JobRequest,
    _admission: &Decision,
    planned: u64,
    actual: u64,
) -> Result<Json, String> {
    let mut statements = Vec::new();
    let (columns, rows) = read_source(maria, request, &mut statements).await?;
    classify_columns(&columns).map_err(|fault| match fault {
        SchemaFault::Broken { .. } => "broken_schema during schedule".to_string(),
        SchemaFault::NewColumn(_) => "new_column during schedule".to_string(),
    })?;
    let mut store = load_store(pg, &source_id(request)).await?;
    let before_hash = store.snapshot_hash.clone();
    let before_gen = store.generation;
    let now = unix_millis().to_string();
    let outcome = apply_snapshot(&mut store, &rows, false, &now)?;
    match outcome.kind {
        "committed" => write_commit(pg, request, &store, &outcome).await?,
        "cache" => write_log(pg, request, &outcome).await?,
        other => return Err(format!("schedule outcome {other}")),
    }
    Ok(json!({
        "kind": "reconcile",
        "planned_ms": planned,
        "actual_ms": actual,
        "outcome": outcome.kind,
        "generation_before": before_gen,
        "generation_after": outcome.generation_after,
        "snapshot_hash_before": before_hash,
        "snapshot_hash": outcome.snapshot_hash,
        "inserts": outcome.inserts,
        "updates": outcome.updates,
        "deletes": outcome.deletes,
        "source_count": rows.len(),
        "statements": statements,
    }))
}

fn source_id(request: &JobRequest) -> String {
    format!("{}.{}", request.schema, request.table)
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

async fn connect_maria(request: &JobRequest, user: &str, password: &str) -> Result<Conn, String> {
    let opts = OptsBuilder::default()
        .ip_or_hostname(request.maria_host.clone())
        .tcp_port(request.maria_port)
        .user(Some(user.to_string()))
        .pass(Some(password.to_string()))
        .db_name(Some(request.schema.clone()))
        .prefer_socket(false)
        .stmt_cache_size(0);
    let connected = timeout(Duration::from_secs(20), Conn::new(opts)).await;
    let mut conn = match connected {
        Ok(Ok(conn)) => conn,
        Ok(Err(err)) => return Err(format!("maria connect: {err}")),
        Err(_) => return Err("maria connect timeout".to_string()),
    };
    let rows = maria_query(&mut conn, "SELECT @@time_zone, @@version").await?;
    let row = rows
        .into_iter()
        .next()
        .ok_or_else(|| "time zone missing".to_string())?;
    let values = take_text(row, 2)?;
    if values.first().and_then(Option::as_deref) != Some("+00:00") {
        let _ = conn.disconnect().await;
        return Err(format!("time_zone {:?}", values.first()));
    }
    Ok(conn)
}

async fn maria_query(conn: &mut Conn, sql: &str) -> Result<Vec<Row>, String> {
    select_only(sql)?;
    match timeout(Duration::from_secs(20), conn.query(sql)).await {
        Ok(Ok(rows)) => Ok(rows),
        Ok(Err(err)) => Err(err.to_string()),
        Err(_) => Err("maria query timeout".to_string()),
    }
}

async fn maria_exec(
    conn: &mut Conn,
    sql: &str,
    schema: &str,
    table: &str,
) -> Result<Vec<Row>, String> {
    select_only(sql)?;
    match timeout(
        Duration::from_secs(20),
        conn.exec(sql, (schema.to_string(), table.to_string())),
    )
    .await
    {
        Ok(Ok(rows)) => Ok(rows),
        Ok(Err(err)) => Err(err.to_string()),
        Err(_) => Err("maria query timeout".to_string()),
    }
}

async fn read_source(
    conn: &mut Conn,
    request: &JobRequest,
    statements: &mut Vec<String>,
) -> Result<(Vec<(String, String)>, Vec<Repr>), String> {
    let catalog_sql = "SELECT column_name, data_type FROM information_schema.columns WHERE table_schema = ? AND table_name = ?";
    statements.push(catalog_sql.to_string());
    let catalog_rows = maria_exec(conn, catalog_sql, &request.schema, &request.table).await?;
    let mut columns = Vec::new();
    for row in catalog_rows {
        let values = take_text(row, 2)?;
        let name = values
            .first()
            .cloned()
            .flatten()
            .ok_or_else(|| "column name".to_string())?;
        let data_type = values.get(1).cloned().flatten().unwrap_or_default();
        columns.push((name, data_type));
    }
    if classify_columns(&columns).is_err() {
        return Ok((columns, Vec::new()));
    }
    let data_sql = format!(
        "SELECT CAST(`id` AS CHAR), CASE WHEN `note` IS NULL THEN NULL ELSE CAST(`note` AS CHAR) END, CASE WHEN `qty` IS NULL THEN NULL ELSE CAST(`qty` AS CHAR) END, CASE WHEN `money` IS NULL THEN NULL ELSE CAST(`money` AS CHAR) END FROM `{}`.`{}` ORDER BY `id`",
        request.schema, request.table
    );
    statements.push(data_sql.clone());
    let first = read_reprs(conn, &data_sql).await?;
    statements.push(data_sql.clone());
    let second = read_reprs(conn, &data_sql).await?;
    if first != second {
        return Err("source_changed_during_read".to_string());
    }
    Ok((columns, first))
}

async fn read_reprs(conn: &mut Conn, sql: &str) -> Result<Vec<Repr>, String> {
    let rows = maria_query(conn, sql).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let values = take_text(row, 4)?;
        let id = values
            .first()
            .cloned()
            .flatten()
            .ok_or_else(|| "id null".to_string())?;
        let id = id.parse::<i64>().map_err(|_| format!("id {id}"))?;
        let note = values.get(1).cloned().flatten();
        let qty = match values.get(2).cloned().flatten() {
            None => None,
            Some(text) => Some(text.parse::<i64>().map_err(|_| format!("qty {text}"))?),
        };
        let money = match values.get(3).cloned().flatten() {
            None => None,
            Some(text) => Some(canonical_money(&text)?),
        };
        out.push(Repr {
            id,
            note,
            qty,
            money,
        });
    }
    Ok(out)
}

fn take_text(mut row: Row, width: usize) -> Result<Vec<Option<String>>, String> {
    let mut values = Vec::with_capacity(width);
    for index in 0..width {
        let value = row.take(index).ok_or_else(|| format!("column {index}"))?;
        values.push(value_text(value)?);
    }
    Ok(values)
}

fn value_text(value: Value) -> Result<Option<String>, String> {
    match value {
        Value::NULL => Ok(None),
        Value::Bytes(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| "text utf8".to_string()),
        Value::Int(number) => Ok(Some(number.to_string())),
        Value::UInt(number) => Ok(Some(number.to_string())),
        _ => Err("text shape".to_string()),
    }
}

async fn connect_pg(request: &JobRequest, password: &str) -> Result<sqlx::PgConnection, String> {
    let options = PgConnectOptions::from_explicit()
        .host(&request.pg_host)
        .port(request.pg_port)
        .username(&request.pg_user)
        .password(password)
        .database(&request.pg_database)
        .ssl_mode(PgSslMode::Disable)
        .disable_statement_logging();
    let mut conn = timeout(Duration::from_secs(20), options.connect())
        .await
        .map_err(|_| "pg connect timeout".to_string())?
        .map_err(|err| format!("pg connect: {err}"))?;
    sqlx::query("SET statement_timeout = '60s'")
        .execute(&mut conn)
        .await
        .map_err(|err| err.to_string())?;
    Ok(conn)
}

async fn ensure_target(conn: &mut sqlx::PgConnection) -> Result<(), String> {
    for sql in TARGET_DDL {
        sqlx::query(*sql)
            .execute(&mut *conn)
            .await
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

const TARGET_DDL: &[&str] = &[
    "CREATE SCHEMA IF NOT EXISTS oo_c5",
    "CREATE TABLE IF NOT EXISTS oo_c5.generation (
        source_id text NOT NULL,
        generation bigint NOT NULL,
        snapshot_hash text NOT NULL,
        grade text NOT NULL,
        capture text NOT NULL,
        delete_detection text NOT NULL,
        warning text NOT NULL,
        freshness timestamptz NOT NULL,
        visible_count bigint NOT NULL,
        tombstone_count bigint NOT NULL,
        inserts integer NOT NULL,
        updates integer NOT NULL,
        deletes integer NOT NULL,
        PRIMARY KEY (source_id, generation)
    )",
    "CREATE TABLE IF NOT EXISTS oo_c5.live_row (
        source_id text NOT NULL,
        key_id bigint NOT NULL,
        note text,
        qty integer,
        money numeric(12,2),
        row_hash text NOT NULL,
        generation bigint NOT NULL,
        PRIMARY KEY (source_id, key_id)
    )",
    "CREATE TABLE IF NOT EXISTS oo_c5.tombstone (
        source_id text NOT NULL,
        key_id bigint NOT NULL,
        generation bigint NOT NULL,
        fence bigint NOT NULL,
        PRIMARY KEY (source_id, key_id)
    )",
    "CREATE TABLE IF NOT EXISTS oo_c5.cycle_log (
        cycle_id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
        source_id text NOT NULL,
        label text NOT NULL,
        outcome text NOT NULL,
        generation_before bigint NOT NULL,
        generation_after bigint NOT NULL,
        snapshot_hash text NOT NULL,
        detail jsonb NOT NULL
    )",
];

async fn load_store(conn: &mut sqlx::PgConnection, source_id: &str) -> Result<Live, String> {
    let generation: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(generation), 0) FROM oo_c5.generation WHERE source_id = $1",
    )
    .bind(source_id)
    .fetch_one(&mut *conn)
    .await
    .map_err(|err| err.to_string())?;
    let mut store = empty_store();
    if generation > 0 {
        let hash: String = sqlx::query_scalar(
            "SELECT snapshot_hash FROM oo_c5.generation WHERE source_id = $1 AND generation = $2",
        )
        .bind(source_id)
        .bind(generation)
        .fetch_one(&mut *conn)
        .await
        .map_err(|err| err.to_string())?;
        let freshness: String = sqlx::query_scalar(
            "SELECT to_char(freshness AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') FROM oo_c5.generation WHERE source_id = $1 AND generation = $2",
        )
        .bind(source_id)
        .bind(generation)
        .fetch_one(&mut *conn)
        .await
        .map_err(|err| err.to_string())?;
        store.generation = u64::try_from(generation).map_err(|_| "generation")?;
        store.snapshot_hash = hash;
        store.freshness = Some(freshness);
    }
    let rows = sqlx::query(
        "SELECT key_id, note, qty::text, CASE WHEN money IS NULL THEN NULL ELSE money::text END, row_hash FROM oo_c5.live_row WHERE source_id = $1 ORDER BY key_id",
    )
    .bind(source_id)
    .fetch_all(&mut *conn)
    .await
    .map_err(|err| err.to_string())?;
    for row in rows {
        let id: i64 = row.try_get(0).map_err(|err| err.to_string())?;
        let note: Option<String> = row.try_get(1).map_err(|err| err.to_string())?;
        let qty_text: Option<String> = row.try_get(2).map_err(|err| err.to_string())?;
        let money_text: Option<String> = row.try_get(3).map_err(|err| err.to_string())?;
        let stored_hash: String = row.try_get(4).map_err(|err| err.to_string())?;
        let qty = match qty_text {
            None => None,
            Some(text) => Some(
                text.parse::<i64>()
                    .map_err(|_| format!("stored qty {text}"))?,
            ),
        };
        let money = match money_text {
            None => None,
            Some(text) => Some(canonical_money(&text)?),
        };
        let repr = Repr {
            id,
            note,
            qty,
            money,
        };
        if repr.hash() != stored_hash {
            return Err(format!("stored row hash mismatch {id}"));
        }
        store.rows.insert(id, repr);
    }
    let tombs = sqlx::query("SELECT key_id, fence FROM oo_c5.tombstone WHERE source_id = $1")
        .bind(source_id)
        .fetch_all(&mut *conn)
        .await
        .map_err(|err| err.to_string())?;
    for row in tombs {
        let id: i64 = row.try_get(0).map_err(|err| err.to_string())?;
        let fence: i64 = row.try_get(1).map_err(|err| err.to_string())?;
        if store.rows.contains_key(&id) {
            return Err(format!("key {id} is both live and tombstoned"));
        }
        store
            .tombstones
            .insert(id, u64::try_from(fence).unwrap_or(0));
    }
    if generation > 0 {
        let visible = hash_of(&store.rows);
        if visible != store.snapshot_hash {
            return Err("generation snapshot_hash does not match live rows".to_string());
        }
    }
    Ok(store)
}

async fn write_commit(
    conn: &mut sqlx::PgConnection,
    request: &JobRequest,
    store: &Live,
    outcome: &Outcome,
) -> Result<(), String> {
    let source_id = source_id(request);
    let mut tx = conn.begin().await.map_err(|err| err.to_string())?;
    for id in &outcome.deletes {
        sqlx::query("DELETE FROM oo_c5.live_row WHERE source_id = $1 AND key_id = $2")
            .bind(&source_id)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|err| err.to_string())?;
        sqlx::query(
            "INSERT INTO oo_c5.tombstone (source_id, key_id, generation, fence) VALUES ($1, $2, $3, $3)
             ON CONFLICT (source_id, key_id) DO UPDATE SET generation = EXCLUDED.generation, fence = EXCLUDED.fence",
        )
        .bind(&source_id)
        .bind(id)
        .bind(i64::try_from(outcome.generation_after).map_err(|_| "generation")?)
        .execute(&mut *tx)
        .await
        .map_err(|err| err.to_string())?;
    }
    for id in outcome.updates.iter().chain(outcome.inserts.iter()) {
        let row = store
            .rows
            .get(id)
            .ok_or_else(|| format!("missing applied row {id}"))?;
        sqlx::query("DELETE FROM oo_c5.tombstone WHERE source_id = $1 AND key_id = $2")
            .bind(&source_id)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|err| err.to_string())?;
        let qty = match row.qty {
            Some(value) => Some(i32::try_from(value).map_err(|_| format!("qty {value}"))?),
            None => None,
        };
        sqlx::query(
            "INSERT INTO oo_c5.live_row (source_id, key_id, note, qty, money, row_hash, generation)
             VALUES ($1, $2, $3, $4, CAST($5 AS numeric(12,2)), $6, $7)
             ON CONFLICT (source_id, key_id) DO UPDATE
             SET note = EXCLUDED.note, qty = EXCLUDED.qty, money = EXCLUDED.money, row_hash = EXCLUDED.row_hash, generation = EXCLUDED.generation",
        )
        .bind(&source_id)
        .bind(row.id)
        .bind(&row.note)
        .bind(qty)
        .bind(&row.money)
        .bind(row.hash())
        .bind(i64::try_from(outcome.generation_after).map_err(|_| "generation")?)
        .execute(&mut *tx)
        .await
        .map_err(|err| err.to_string())?;
    }
    sqlx::query(
        "INSERT INTO oo_c5.generation (source_id, generation, snapshot_hash, grade, capture, delete_detection, warning, freshness, visible_count, tombstone_count, inserts, updates, deletes)
         VALUES ($1, $2, $3, 'B', 'C5', 'reconcile', 'DELETES_LAGGED', clock_timestamp(), $4, $5, $6, $7, $8)",
    )
    .bind(&source_id)
    .bind(i64::try_from(outcome.generation_after).map_err(|_| "generation")?)
    .bind(&outcome.snapshot_hash)
    .bind(i64::try_from(store.rows.len()).unwrap_or(0))
    .bind(i64::try_from(store.tombstones.len()).unwrap_or(0))
    .bind(i32::try_from(outcome.inserts.len()).unwrap_or(i32::MAX))
    .bind(i32::try_from(outcome.updates.len()).unwrap_or(i32::MAX))
    .bind(i32::try_from(outcome.deletes.len()).unwrap_or(i32::MAX))
    .execute(&mut *tx)
    .await
    .map_err(|err| err.to_string())?;
    insert_log(&mut tx, request, outcome).await?;
    tx.commit().await.map_err(|err| err.to_string())?;
    Ok(())
}

async fn write_log(
    conn: &mut sqlx::PgConnection,
    request: &JobRequest,
    outcome: &Outcome,
) -> Result<(), String> {
    let mut tx = conn.begin().await.map_err(|err| err.to_string())?;
    insert_log(&mut tx, request, outcome).await?;
    tx.commit().await.map_err(|err| err.to_string())?;
    Ok(())
}

async fn insert_log(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    request: &JobRequest,
    outcome: &Outcome,
) -> Result<(), String> {
    let detail = json!({
        "inserts": outcome.inserts,
        "updates": outcome.updates,
        "deletes": outcome.deletes,
        "published": outcome.published,
        "warning": outcome.warning,
        "grade": outcome.grade,
    });
    sqlx::query(
        "INSERT INTO oo_c5.cycle_log (source_id, label, outcome, generation_before, generation_after, snapshot_hash, detail)
         VALUES ($1, $2, $3, $4, $5, $6, CAST($7 AS jsonb))",
    )
    .bind(source_id(request))
    .bind(&request.label)
    .bind(outcome.kind)
    .bind(i64::try_from(outcome.generation_before).unwrap_or(0))
    .bind(i64::try_from(outcome.generation_after).unwrap_or(0))
    .bind(&outcome.snapshot_hash)
    .bind(detail.to_string())
    .execute(&mut **tx)
    .await
    .map_err(|err| err.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{Consistency, CursorSafety, Privileges, ReadFrom, ReadNode};
    use serde_json::json;

    fn row(id: i64, note: Option<&str>, qty: Option<i64>, money: Option<&str>) -> Repr {
        Repr {
            id,
            note: note.map(str::to_string),
            qty,
            money: money.map(str::to_string),
        }
    }

    fn seed() -> Vec<Repr> {
        vec![
            row(1, Some("alpha"), Some(10), Some("12.50")),
            row(2, Some("beta"), None, Some("0.00")),
            row(3, None, Some(3), None),
            row(4, Some("delta"), Some(-7), Some("1234567890.12")),
            row(5, Some("epsilon"), Some(0), Some("-0.01")),
        ]
    }

    fn changed() -> Vec<Repr> {
        vec![
            row(1, Some("alpha-2"), Some(11), Some("12.75")),
            row(3, None, Some(3), None),
            row(4, Some("delta"), Some(-7), Some("1234567890.12")),
            row(5, Some("epsilon"), Some(0), Some("-0.01")),
            row(6, Some("zeta"), Some(1), Some("1.00")),
        ]
    }

    fn request() -> CaptureRequest {
        CaptureRequest {
            sync: SyncMethod::Full,
            secret_ref: "secret://oo-c5/reader".to_string(),
            secret_path_present: true,
            capability_known: true,
            license_known: true,
            consistency: Consistency::PerTable,
            detect_every_ms: 1000,
            reconcile_every_ms: 2000,
            window: None,
            now_minute: 720,
            stable_key: true,
            cursor_safety: CursorSafety::CommitOrdered,
            read_from: ReadFrom::Primary,
            allow_write_capable_credential: false,
        }
    }

    #[test]
    fn money_scale_and_sign_stay_canonical() {
        assert_eq!(canonical_money("12.5").unwrap(), "12.50");
        assert_eq!(canonical_money("12.50").unwrap(), "12.50");
        assert_eq!(canonical_money("-0.01").unwrap(), "-0.01");
        assert_eq!(canonical_money("0").unwrap(), "0.00");
        assert_eq!(canonical_money("1234567890.1200").unwrap(), "1234567890.12");
        assert!(canonical_money("12.501").is_err());
        assert!(canonical_money("12345678901234567890.1234567890").is_err());
    }

    #[test]
    fn explicit_cdc_does_not_run_as_c5() {
        let mut request = request();
        request.sync = SyncMethod::Cdc;
        let facts = ServerFacts {
            version: "11.4.13-MariaDB-ubu2404".to_string(),
            version_comment: "mariadb.org binary distribution".to_string(),
            log_bin: false,
            binlog_format: "MIXED".to_string(),
            binlog_row_image: "FULL".to_string(),
            gtid_current_pos: String::new(),
            gtid_strict_mode: false,
            binlog_expire_seconds: Some(864000),
            binlog_file_pos: None,
            read_node: ReadNode::Primary,
            privileges: Privileges {
                select: true,
                ..Privileges::none()
            },
        };
        match gate(&request, Some(&facts)) {
            Err(GateError::Admission(decision)) => {
                assert_eq!(decision.code, "binlog_off");
                assert!(decision.method.is_none());
                assert!(!decision.armed);
                assert_ne!(decision.capture.as_deref(), Some("C5"));
            }
            other => panic!("cdc fell through: {other:?}"),
        }
    }

    #[test]
    fn closed_window_refuses_before_a_full_pull() {
        let mut request = request();
        request.window = Some("20:00-08:00".to_string());
        request.now_minute = 720;
        match gate(&request, None) {
            Err(GateError::Code(code)) => assert_eq!(code, "window_closed"),
            other => panic!("window: {other:?}"),
        }
    }

    #[test]
    fn full_request_is_grade_b_reconcile() {
        let decision = gate(&request(), None).expect("full");
        assert_eq!(decision.capture.as_deref(), Some("C5"));
        assert_eq!(decision.grade.as_deref(), Some("B"));
        assert_eq!(decision.delete_detection.as_deref(), Some("reconcile"));
        assert_eq!(decision.token_order.as_deref(), Some("None"));
    }

    #[test]
    fn insert_update_delete_then_cache_and_interrupt() {
        let mut store = empty_store();
        let first = apply_snapshot(&mut store, &seed(), false, "t0").unwrap();
        assert_eq!(first.kind, "committed");
        assert_eq!(first.generation_after, 1);
        assert_eq!(first.inserts, vec![1, 2, 3, 4, 5]);
        assert_eq!(store.rows.len(), 5);
        let withheld = apply_snapshot(&mut store, &changed(), true, "t1").unwrap();
        assert_eq!(withheld.kind, "interrupted");
        assert_eq!(withheld.generation_after, 1);
        assert_eq!(withheld.inserts, vec![6]);
        assert_eq!(withheld.updates, vec![1]);
        assert_eq!(withheld.deletes, vec![2]);
        assert_eq!(store.rows.len(), 5);
        assert!(store.rows.contains_key(&2));
        assert!(!store.tombstones.contains_key(&2));
        let applied = apply_snapshot(&mut store, &changed(), false, "t2").unwrap();
        assert_eq!(applied.kind, "committed");
        assert_eq!(applied.generation_after, 2);
        assert_eq!(applied.inserts, vec![6]);
        assert_eq!(applied.updates, vec![1]);
        assert_eq!(applied.deletes, vec![2]);
        assert!(!store.rows.contains_key(&2));
        assert_eq!(store.tombstones.get(&2), Some(&2));
        assert_eq!(store.rows.len(), 5);
        let again = apply_snapshot(&mut store, &changed(), false, "t3").unwrap();
        assert_eq!(again.kind, "cache");
        assert_eq!(again.generation_after, 2);
        assert!(again.inserts.is_empty());
        assert!(!store.rows.contains_key(&2));
        let replay = apply_snapshot(&mut store, &changed(), true, "t4").unwrap();
        assert_eq!(replay.kind, "cache");
        assert_eq!(store.generation, 2);
    }

    #[test]
    fn snapshot_hash_ignores_input_order() {
        let mut rows = seed();
        let forward = hash_of(&index_rows(&rows).unwrap());
        rows.reverse();
        let backward = hash_of(&index_rows(&rows).unwrap());
        assert_eq!(forward, backward);
    }

    #[test]
    fn two_user_periods_have_different_fire_counts() {
        assert_eq!(deadlines(1000, 5000), vec![1000, 2000, 3000, 4000, 5000]);
        assert_eq!(deadlines(2000, 5000), vec![2000, 4000]);
        assert_eq!(deadlines(2000, 4500), vec![2000, 4000]);
        assert_eq!(deadlines(4000, 4500), vec![4000]);
    }

    #[test]
    fn schema_faults_do_not_drop_columns() {
        let ok = vec![
            ("id".to_string(), "bigint".to_string()),
            ("note".to_string(), "varchar".to_string()),
            ("qty".to_string(), "int".to_string()),
            ("money".to_string(), "decimal".to_string()),
        ];
        assert!(classify_columns(&ok).is_ok());
        let mut extra = ok.clone();
        extra.push(("extra".to_string(), "int".to_string()));
        assert!(matches!(
            classify_columns(&extra),
            Err(SchemaFault::NewColumn(_))
        ));
        let mut broken = ok;
        broken.retain(|(name, _)| name != "qty");
        assert!(matches!(
            classify_columns(&broken),
            Err(SchemaFault::Broken { .. })
        ));
    }

    #[test]
    fn source_statements_must_be_select() {
        assert!(select_only("SELECT 1").is_ok());
        assert!(select_only("  select `id` from `oo_c5`.`k_repr`").is_ok());
        assert!(select_only("INSERT INTO oo_c5.k_repr VALUES (1)").is_err());
        assert!(select_only("UPDATE oo_c5.k_repr SET qty = 1").is_err());
        assert!(select_only("DELETE FROM oo_c5.k_repr").is_err());
        assert!(select_only("CREATE TABLE oo_c5.k_repr (id bigint)").is_err());
        assert!(select_only("ALTER TABLE oo_c5.k_repr ADD extra int").is_err());
        assert!(select_only("DROP TABLE oo_c5.k_repr").is_err());
    }

    fn fact(
        name: &str,
        data_type: &str,
        nullable: bool,
        precision: Option<i64>,
        scale: Option<i64>,
    ) -> ColumnFact {
        ColumnFact {
            name: name.to_string(),
            data_type: data_type.to_string(),
            nullable,
            precision,
            scale,
            char_length: if data_type == "varchar" {
                Some(64)
            } else {
                None
            },
            column_type: data_type.to_string(),
        }
    }

    #[test]
    fn produce_encoder_writes_int32_and_decimal128() {
        let columns = vec![
            fact("id", "bigint", false, Some(19), Some(0)),
            fact("note", "varchar", true, None, None),
            fact("qty", "int", true, Some(10), Some(0)),
            fact("money", "decimal", true, Some(12), Some(2)),
        ];
        let gaps = encoder_gap(&columns);
        assert!(gaps.iter().all(|item| item["reason"] == json!("exact")));
        assert_eq!(gaps[2]["loader_arrow_type"], json!("int32"));
        assert_eq!(gaps[3]["loader_arrow_type"], json!("decimal128"));
        assert_eq!(gaps[1]["source_char_length"], json!(64));
        assert!(gaps[1]["loader_char_length"].is_null());
        let rows = vec![
            row(1, Some("alpha-2"), Some(11), Some("12.75")),
            row(3, None, Some(3), None),
            row(4, Some("delta"), Some(-7), Some("1234567890.12")),
            row(5, Some("epsilon-slow"), Some(0), Some("-0.01")),
            row(6, Some("zeta"), Some(1), Some("1.00")),
        ];
        let bytes = encode_exact(&columns, &rows).expect("encoder");
        let decoded = oo_pipeline::native_arrow::decode_arrow_stream(&bytes).expect("decode");
        assert_eq!(decoded.rows, 5);
        assert_eq!(decoded.fields[0].arrow_type, "int64");
        assert!(!decoded.fields[0].nullable);
        assert_eq!(decoded.fields[0].precision, Some(64));
        assert_eq!(decoded.fields[1].arrow_type, "utf8");
        assert!(decoded.fields[1].nullable);
        assert_eq!(decoded.fields[2].arrow_type, "int32");
        assert_eq!(decoded.fields[2].precision, Some(32));
        assert_eq!(decoded.fields[3].arrow_type, "decimal128");
        assert_eq!(decoded.fields[3].precision, Some(12));
        assert_eq!(decoded.fields[3].scale, Some(2));
        assert_eq!(
            decoded.columns[0][0],
            oo_pipeline::native_arrow::NativeCell::Int {
                bit_width: 64,
                value: 1
            }
        );
        assert_eq!(
            decoded.columns[1][1],
            oo_pipeline::native_arrow::NativeCell::Null
        );
        assert_eq!(
            decoded.columns[2][2],
            oo_pipeline::native_arrow::NativeCell::Int {
                bit_width: 32,
                value: -7
            }
        );
        assert_eq!(
            decoded.columns[3][2],
            oo_pipeline::native_arrow::NativeCell::Decimal {
                precision: 12,
                scale: 2,
                coefficient: 123_456_789_012
            }
        );
        assert_eq!(
            decoded.columns[3][3],
            oo_pipeline::native_arrow::NativeCell::Decimal {
                precision: 12,
                scale: 2,
                coefficient: -1
            }
        );
        assert_eq!(
            decoded.columns[3][1],
            oo_pipeline::native_arrow::NativeCell::Null
        );
        let dir = std::env::temp_dir().join("oo-c5-arrow-unit");
        fs::create_dir_all(&dir).expect("tmpdir");
        let arrow_path = dir.join("capture.arrow");
        fs::write(&arrow_path, &bytes).expect("arrow file");
        let dumped = dump_arrow(&arrow_path).expect("dump");
        assert!(dumped.contains("1\talpha-2\t11\t12.75\n"));
        assert!(dumped.contains("3\t\\N\t3\t\\N\n"));
        assert!(dumped.contains("4\tdelta\t-7\t1234567890.12\n"));
        assert!(dumped.contains("5\tepsilon-slow\t0\t-0.01\n"));
        assert!(dumped.contains("6\tzeta\t1\t1.00\n"));
        let lied = oo_pipeline::native_update::encode_fixture_arrow(
            &[oo_pipeline::native_update::FixtureColumn {
                name: "qty".to_string(),
                nullable: true,
                kind: oo_pipeline::native_update::FixtureKind::Int64,
            }],
            &[vec![oo_pipeline::native_update::FixtureCell::Int(11)]],
        )
        .expect("fixture");
        let lied_decoded =
            oo_pipeline::native_arrow::decode_arrow_stream(&lied).expect("fixture decode");
        assert_eq!(lied_decoded.fields[0].arrow_type, "int64");
        assert_ne!(
            lied_decoded.fields[0].arrow_type,
            decoded.fields[2].arrow_type
        );
        let schema = oo_pipeline::native_arrow::arrow_schema_sha256(&bytes).expect("schema");
        let batch = oo_pipeline::native_arrow::arrow_sha256(&bytes);
        let catalog = digest_bytes(&schema_preimage(&columns));
        assert_eq!(schema.len(), 64);
        assert_ne!(batch, schema);
        assert_ne!(schema, catalog);
        let digest = "ab".repeat(32);
        let job = ProduceJob {
            capture: request(),
            label: "produce-unit".to_string(),
            run_id: "produce-unit".to_string(),
            schema: "oo_c5".to_string(),
            table: "k_repr".to_string(),
            maria_host: "127.0.0.1".to_string(),
            maria_port: 1,
            maria_user_file: "user".to_string(),
            maria_password_file: "password".to_string(),
            output_dir: "/tmp/oo-c5-produce-unit".to_string(),
            source_generation: None,
            parent_source_generation: None,
            prior_rows: None,
            permanent_absent: Vec::new(),
        };
        let receipt = receipt_body(
            &job,
            1,
            None,
            &digest,
            &digest,
            &digest,
            &digest,
            &digest,
            &digest,
            &digest,
            &digest,
            &digest,
            &[],
            None,
        );
        let receipt_sha = "cd".repeat(32);
        let load = load_body(
            &job,
            &columns,
            &rows,
            1,
            None,
            &[],
            &digest,
            "B",
            "reconcile",
            &digest,
            &digest,
            &digest,
            &digest,
            &digest,
            &receipt_sha,
            Path::new("/tmp/oo-c5-produce-unit"),
            false,
        );
        assert_eq!(
            receipt["schema"],
            json!(oo_pipeline::native_arrow::CAPTURE_RECEIPT_PROTOCOL)
        );
        assert!(identity_match(&receipt, &load));
        assert_ne!(
            load["admission_expect"]["declared_receipt_sha256"],
            load["schema_digest"]
        );
        assert!(receipt.get("arrow_schema_sha256").is_none());
        assert!(load.get("arrow_path").is_none());
        let present = receipt_body(
            &job,
            1,
            None,
            &digest,
            &digest,
            &digest,
            &digest,
            &digest,
            &digest,
            &digest,
            &digest,
            &digest,
            std::slice::from_ref(&batch),
            Some(schema.as_str()),
        );
        assert_eq!(present["arrow_schema_sha256"], json!(schema));
        assert_ne!(present["arrow_schema_sha256"], json!(catalog));
        assert_eq!(present["arrow_sha256"], json!([batch]));
        let present_load = load_body(
            &job,
            &columns,
            &rows,
            1,
            None,
            &[],
            &digest,
            "B",
            "reconcile",
            &digest,
            &digest,
            &digest,
            &digest,
            &digest,
            &receipt_sha,
            Path::new("/tmp/oo-c5-produce-unit"),
            true,
        );
        assert!(present_load.get("arrow_path").is_some());
        assert!(identity_match(&present, &present_load));
        assert!(declared_column(&columns[1]).get("char_length").is_none());
        assert_eq!(declared_column(&columns[2])["arrow_type"], json!("int32"));
        assert_eq!(declared_column(&columns[2])["precision"], json!(32));
        assert_eq!(
            declared_column(&columns[3])["arrow_type"],
            json!("decimal128")
        );
        assert_eq!(declared_column(&columns[3])["precision"], json!(12));
        assert_eq!(declared_column(&columns[3])["scale"], json!(2));
        let rendered = format!("{receipt}{load}{present}{present_load}");
        assert!(!rendered.contains("secret://"));
        assert!(!rendered.contains("secret_ref"));
        assert_eq!(load["source_engine"], json!("mariadb"));
        assert_ne!(load["source_engine"], json!("mysql"));
        assert_eq!(load["tombstones"], json!([]));
        assert!(load.get("parent_source_generation").is_none());
    }

    #[test]
    fn capture_delta_records_update_insert_and_physical_delete() {
        let prior = index_rows(&[
            row(1, Some("alpha-2"), Some(11), Some("12.75")),
            row(3, None, Some(3), None),
            row(4, Some("delta"), Some(-7), Some("1234567890.12")),
            row(5, Some("epsilon-slow"), Some(0), Some("-0.01")),
            row(6, Some("zeta"), Some(1), Some("1.00")),
        ])
        .expect("prior");
        let current = index_rows(&[
            row(1, Some("alpha-3"), Some(12), Some("13.50")),
            row(3, None, Some(3), None),
            row(4, Some("delta"), Some(-7), Some("1234567890.12")),
            row(5, Some("epsilon-slow"), Some(0), Some("-0.01")),
            row(7, Some("eta"), Some(8), Some("2.50")),
        ])
        .expect("current");
        let delta = capture_delta(&prior, &current, &[2]).expect("delta");
        assert_eq!(delta.inserts, vec![7]);
        assert_eq!(delta.updates, vec![1]);
        assert_eq!(delta.deletes, vec![6]);
        assert_eq!(
            delta.upserts.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![1, 7]
        );
        let revived = index_rows(&[row(2, Some("beta"), None, Some("0.00"))]).expect("revived");
        let err = capture_delta(&prior, &revived, &[2]).expect_err("resurrection");
        assert!(err.contains("resurrection 2"));
        let columns = vec![
            fact("id", "bigint", false, Some(19), Some(0)),
            fact("note", "varchar", true, None, None),
            fact("qty", "int", true, Some(10), Some(0)),
            fact("money", "decimal", true, Some(12), Some(2)),
        ];
        let bytes = encode_exact(&columns, &delta.upserts).expect("arrow");
        let decoded = oo_pipeline::native_arrow::decode_arrow_stream(&bytes).expect("decode");
        assert_eq!(decoded.rows, 2);
        assert_eq!(
            decoded.columns[2][0],
            oo_pipeline::native_arrow::NativeCell::Int {
                bit_width: 32,
                value: 12
            }
        );
        assert_eq!(
            decoded.columns[3][0],
            oo_pipeline::native_arrow::NativeCell::Decimal {
                precision: 12,
                scale: 2,
                coefficient: 1_350
            }
        );
        let digest = "ab".repeat(32);
        let job = ProduceJob {
            capture: request(),
            label: "produce-delta".to_string(),
            run_id: "produce-delta".to_string(),
            schema: "oo_c5".to_string(),
            table: "k_repr".to_string(),
            maria_host: "127.0.0.1".to_string(),
            maria_port: 1,
            maria_user_file: "user".to_string(),
            maria_password_file: "password".to_string(),
            output_dir: "/tmp/oo-c5-produce-delta".to_string(),
            source_generation: Some(2),
            parent_source_generation: Some(1),
            prior_rows: None,
            permanent_absent: vec![2],
        };
        let live: Vec<Repr> = current.values().cloned().collect();
        let load = load_body(
            &job,
            &columns,
            &live,
            2,
            Some(1),
            &delta.deletes,
            &digest,
            "B",
            "reconcile",
            &digest,
            &digest,
            &digest,
            &digest,
            &digest,
            &digest,
            Path::new("/tmp/oo-c5-produce-delta"),
            true,
        );
        assert_eq!(load["source_generation"], json!(2));
        assert_eq!(load["parent_source_generation"], json!(1));
        assert_eq!(load["tombstones"], json!(["6"]));
        assert_eq!(load["source_engine"], json!("mariadb"));
        assert_eq!(load["truth"].as_array().expect("truth").len(), 5);
        let receipt = receipt_body(
            &job,
            2,
            Some(1),
            &digest,
            &digest,
            &digest,
            &digest,
            &digest,
            &digest,
            &digest,
            &digest,
            &digest,
            &[],
            None,
        );
        assert_eq!(receipt["parent_source_generation"], json!(1));
        assert!(identity_match(&receipt, &load));
    }
}
