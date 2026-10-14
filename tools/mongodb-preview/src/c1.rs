//! Replica-set change stream capture.
//!
//! `clusterTime` is the comparable watermark (`mongo-cluster-time/v1`), with the
//! replica-set lineage and the election epoch. The resume token is opaque
//! (`mongo-resume-token/v1`) and is never ordered, compared, or used as a dedup key.
//! Same-`clusterTime` rows order and dedup by the server `txnOpIndex` carried inside
//! the token. Grade T is claimed only after an explicit `commitTransaction` event is
//! applied. A collection stream on this server expands a transaction into document
//! events and does not emit that commit event. Those events stay buffered until the
//! next event closes the group, then apply atomically at grade S0. This is not the
//! standalone C5 diff.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mongodb::Client;
use mongodb::bson::doc;
use mongodb::bson::{Bson, Document, Timestamp};
use mongodb::error::ErrorKind;
use mongodb::event::EventHandler;
use mongodb::event::command::{CommandEvent, CommandSucceededEvent};
use mongodb::options::{ClientOptions, Credential, ReadConcern, ServerMonitoringMode, TransactionOptions};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

use crate::arrow_utf8::{self, Utf8Cell, Utf8Column};
use crate::capture::APPROVED_FIELDS;
use crate::docseal;
use crate::frozen_columns;

pub const CLUSTER_CODEC: &str = "mongo-cluster-time/v1";
pub const RESUME_CODEC: &str = "mongo-resume-token/v1";
pub const QUEUE_CAP: usize = 2;

const WRITE_COMMANDS: &[&str] = &[
    "insert",
    "update",
    "delete",
    "findAndModify",
    "drop",
    "collMod",
    "create",
    "createIndexes",
    "dropIndexes",
    "dropDatabase",
    "renameCollection",
    "applyOps",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub op: String,
    pub key: Option<String>,
    pub doc: Option<Value>,
    pub pre: Option<Value>,
    pub cluster: u64,
    pub lsid: Option<String>,
    pub txn: Option<i64>,
    /// Server `txnOpIndex` for this `clusterTime`. Not a reader counter.
    pub op_index: u32,
}

#[derive(Clone, Debug)]
struct TxnBuf {
    lsid: String,
    txn: i64,
    events: Vec<Event>,
}

#[derive(Clone, Debug)]
pub struct Store {
    rows: BTreeMap<String, Value>,
    tombs: BTreeSet<String>,
    seen: BTreeSet<String>,
    applied: u64,
    buf: Option<TxnBuf>,
    pub multi_commits: u32,
    pub commits: u32,
    pub txn_groups: u32,
    pub aborts: u32,
    pub pre_images: u32,
    pub post_images: u32,
    pub hidden: BTreeSet<String>,
    pub null_memo: bool,
    pub missing_memo: bool,
    pub lineage: String,
    pub epoch: i64,
}

pub fn cluster_decimal(time: u32, increment: u32) -> u64 {
    ((time as u64) << 32) | u64::from(increment)
}

pub fn decimal_text(value: u64) -> String {
    value.to_string()
}

pub fn cmp_cluster(left: u64, right: u64) -> std::cmp::Ordering {
    left.cmp(&right)
}

pub fn resume_digest(raw: &[u8]) -> String {
    hex_bytes(&Sha256::digest(raw))
}

pub fn may_pull(unapplied: usize) -> bool {
    unapplied < QUEUE_CAP
}

/// History-lost resume. Invalid-token 280 is not expiry and must not continue.
pub fn position_expired(code: i32) -> bool {
    code == 286 || code == 136
}

/// Cluster time embedded in a resume token. The server uses it for history-lost.
/// It is not a watermark: callers must not order or dedup events by it.
pub fn token_cluster(data_hex: &str) -> Result<u64, &'static str> {
    let bytes = docseal::decode_hex(data_hex).map_err(|_| "token")?;
    if bytes.len() < 9 || bytes[0] != 0x82 {
        return Err("token");
    }
    let secs = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
    let inc = u32::from_be_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]);
    Ok(cluster_decimal(secs, inc))
}

/// Server operation index at one `clusterTime`. The token bytes stay opaque.
pub fn op_index(data_hex: &str) -> Result<u32, &'static str> {
    let bytes = docseal::decode_hex(data_hex).map_err(|_| "token")?;
    if bytes.len() < 9 || bytes[0] != 0x82 {
        return Err("token");
    }
    let mut at = 9usize;
    let version = read_ks_int(&bytes, &mut at)?;
    if version >= 1 {
        let _token_type = read_ks_int(&bytes, &mut at)?;
    }
    let index = read_ks_int(&bytes, &mut at)?;
    u32::try_from(index).map_err(|_| "token")
}

/// KeyString V1 integer. Zero is one byte. A positive integer is shifted left by one.
fn read_ks_int(bytes: &[u8], at: &mut usize) -> Result<i64, &'static str> {
    const NUMERIC: u8 = 30;
    const ZERO: u8 = NUMERIC + 11;
    const POS1: u8 = NUMERIC + 13;
    const POS8: u8 = NUMERIC + 20;
    let ctype = *bytes.get(*at).ok_or("token")?;
    *at += 1;
    if ctype == ZERO {
        return Ok(0);
    }
    if !(POS1..=POS8).contains(&ctype) {
        return Err("token");
    }
    let width = usize::from(ctype - POS1 + 1);
    if *at + width > bytes.len() {
        return Err("token");
    }
    let mut encoded = 0u64;
    for _ in 0..width {
        encoded = (encoded << 8) | u64::from(bytes[*at]);
        *at += 1;
    }
    if encoded & 1 == 1 {
        return Err("token");
    }
    i64::try_from(encoded >> 1).map_err(|_| "token")
}

/// Total order inside one lineage and epoch. A different lineage or epoch is not a gap to skip.
pub fn cmp_watermark(
    left_lineage: &str,
    left_epoch: i64,
    left_cluster: u64,
    right_lineage: &str,
    right_epoch: i64,
    right_cluster: u64,
) -> Result<std::cmp::Ordering, &'static str> {
    if left_lineage != right_lineage || left_epoch != right_epoch {
        return Err("incomparable");
    }
    Ok(cmp_cluster(left_cluster, right_cluster))
}

/// Code 286 or 136 stops. No ACK, no skip, no resume from the latest token.
pub fn expiry_record(code: i32, name: &str, message: &str, continued: bool) -> Value {
    let expired = !continued && position_expired(code);
    json!({
        "status": if expired { "POSITION_EXPIRED" } else { "not_expired" },
        "code": code,
        "code_name": name,
        "continued": continued,
        "resumed_from_latest": false,
        "ack": false,
        "skipped_gap": false,
        "recovery": if expired { "rebaseline_cdc_first" } else { "none" },
        "full_snapshot": false,
        "message": message.chars().take(240).collect::<String>()
    })
}

/// The approved recovery is a new change stream, a new L0, and a snapshot barrier.
pub fn rebaseline_barrier() -> Value {
    json!({
        "protocol": "oo.mongodb-preview.c1-rebaseline/v1",
        "recovery": "rebaseline_cdc_first",
        "steps": ["open_change_stream", "record_l0", "snapshot_read_concern", "apply_cluster_time_after_snapshot"],
        "full_snapshot": false,
        "periodic": false,
        "ack": false,
        "skip_gap": false,
        "resume_from_latest": false,
        "position_codec": CLUSTER_CODEC,
        "resume_codec": RESUME_CODEC,
        "resume_token_compared": false,
        "import_invoked": false
    })
}

pub fn apply_interface_gap() -> Value {
    json!({
        "protocol": "oo.mongodb-preview.c1-apply-gap/v1",
        "position_codec": CLUSTER_CODEC,
        "resume_codec": RESUME_CODEC,
        "import_command": "oo-pipeline import-native-update",
        "function": "oo_pipeline::native_update::import_native_update",
        "signature": "pub async fn import_native_update() -> Result<(), u8>",
        "classify": "fn classify(document: &Value) -> Result<UpdatePlan, &'static str>",
        "closed_allow": [
            "pg-commit-lsn/v1",
            "mssql-ct-version/v1",
            "mysql-binlog-pos/v1",
            "mariadb-gtid/v1",
            "batch-cursor-us/v1",
            "snapshot-zero/v1",
            "partition-signature/v1"
        ],
        "error": "closed",
        "policy_error": "c1_requires_t",
        "policy_site": "tools/source-capture-policy/src/gate.rs:161",
        "offset_codecs": [
            "pg-commit-lsn/v1",
            "mysql-binlog-pos/v1",
            "batch-cursor-us/v1",
            "snapshot-zero/v1",
            "mariadb-gtid/v1"
        ],
        "schedule_codecs": ["pg-commit-lsn/v1", "snapshot-zero/v1"],
        "loader_invoked": false,
        "cdc_offset_codec_ok": false,
        "policy_codec_known": true
    })
}

impl Store {
    pub fn new() -> Self {
        Self {
            rows: BTreeMap::new(),
            tombs: BTreeSet::new(),
            seen: BTreeSet::new(),
            applied: 0,
            buf: None,
            multi_commits: 0,
            commits: 0,
            txn_groups: 0,
            aborts: 0,
            pre_images: 0,
            post_images: 0,
            hidden: BTreeSet::new(),
            null_memo: false,
            missing_memo: false,
            lineage: String::new(),
            epoch: 0,
        }
    }

    pub fn grade(&self) -> &'static str {
        if self.multi_commits > 0 { "T" } else { "S0" }
    }

    pub fn load_snapshot(&mut self, rows: BTreeMap<String, Value>, cluster: u64) {
        for (key, doc) in rows {
            if self.tombs.contains(&key) {
                continue;
            }
            self.note_doc(&doc);
            self.rows.insert(key, doc);
        }
        if cmp_cluster(cluster, self.applied).is_gt() {
            self.applied = cluster;
        }
    }

    pub fn observe(&mut self, event: Event) -> Result<(), &'static str> {
        let id = dedup_id(&self.lineage, self.epoch, &event);
        if !self.seen.insert(id) {
            return Ok(());
        }
        // Collection streams publish transaction ops only after commit, with no commit event.
        // The next event from outside the group is the boundary. Do not call that grade T.
        let close_group = match &self.buf {
            Some(open) => {
                let same = event.lsid.as_deref() == Some(open.lsid.as_str()) && event.txn == Some(open.txn);
                !same && event.op != "commit" && event.op != "abort"
            }
            None => false,
        };
        if close_group {
            self.close_open_group()?;
        }
        if event.lsid.is_some() && event.txn.is_some() && event.op != "commit" && event.op != "abort" {
            return self.buffer(event);
        }
        if event.op == "commit" {
            return self.commit(event);
        }
        if event.op == "abort" {
            return self.abort(event);
        }
        if self.buf.is_some() {
            return Err("txn_open");
        }
        self.apply_one(&event)?;
        self.advance(event.cluster);
        Ok(())
    }

    pub fn replay(&mut self, events: &[Event]) -> Result<(), &'static str> {
        for event in events {
            self.observe(event.clone())?;
        }
        Ok(())
    }

    fn buffer(&mut self, event: Event) -> Result<(), &'static str> {
        let lsid = event.lsid.clone().ok_or("txn")?;
        let txn = event.txn.ok_or("txn")?;
        match &mut self.buf {
            None => {
                self.buf = Some(TxnBuf { lsid, txn, events: vec![event] });
                Ok(())
            }
            Some(open) if open.lsid == lsid && open.txn == txn => {
                open.events.push(event);
                Ok(())
            }
            Some(_) => Err("txn_mixed"),
        }
    }

    fn close_open_group(&mut self) -> Result<(), &'static str> {
        let mut open = self.buf.take().ok_or("txn_commit")?;
        order_group(&mut open.events)?;
        let mut cluster = 0u64;
        for item in &open.events {
            self.apply_one(item)?;
            if cmp_cluster(item.cluster, cluster).is_gt() {
                cluster = item.cluster;
            }
        }
        self.txn_groups += 1;
        self.advance(cluster);
        Ok(())
    }

    fn commit(&mut self, event: Event) -> Result<(), &'static str> {
        let lsid = event.lsid.clone().ok_or("txn")?;
        let txn = event.txn.ok_or("txn")?;
        let mut open = self.buf.take().ok_or("txn_commit")?;
        if open.lsid != lsid || open.txn != txn {
            return Err("txn_commit");
        }
        order_group(&mut open.events)?;
        let count = open.events.len();
        for item in &open.events {
            self.apply_one(item)?;
        }
        self.commits += 1;
        if count >= 2 {
            self.multi_commits += 1;
        }
        self.advance(event.cluster);
        Ok(())
    }

    fn abort(&mut self, event: Event) -> Result<(), &'static str> {
        let open = self.buf.take().ok_or("txn_abort")?;
        let lsid = event.lsid.ok_or("txn")?;
        let txn = event.txn.ok_or("txn")?;
        if open.lsid != lsid || open.txn != txn {
            return Err("txn_abort");
        }
        self.aborts += 1;
        self.advance(event.cluster);
        Ok(())
    }

    fn apply_one(&mut self, event: &Event) -> Result<(), &'static str> {
        if event.pre.is_some() {
            self.pre_images += 1;
        }
        match event.op.as_str() {
            "insert" | "update" | "replace" => {
                let key = event.key.clone().ok_or("key")?;
                let doc = event.doc.clone().ok_or("post_image")?;
                if event.op != "insert" {
                    self.post_images += 1;
                }
                self.note_doc(&doc);
                self.tombs.remove(&key);
                self.rows.insert(key, doc);
                Ok(())
            }
            "delete" => {
                let key = event.key.clone().ok_or("key")?;
                self.rows.remove(&key);
                self.tombs.insert(key);
                Ok(())
            }
            "ignore" => Ok(()),
            _ => Err("op"),
        }
    }

    fn note_doc(&mut self, doc: &Value) {
        let Some(map) = doc.as_object() else {
            return;
        };
        match map.get("memo") {
            None => self.missing_memo = true,
            Some(Value::Null) => self.null_memo = true,
            Some(_) => {}
        }
        for key in map.keys() {
            if !APPROVED_FIELDS.contains(&key.as_str()) {
                self.hidden.insert(key.clone());
            }
        }
    }

    fn advance(&mut self, cluster: u64) {
        if cmp_cluster(cluster, self.applied).is_gt() {
            self.applied = cluster;
        }
    }

    pub fn applied_json(&self, resume_sha: &str) -> Value {
        let rows: Vec<Value> = self
            .rows
            .iter()
            .map(|(key, doc)| json!({"key": key, "doc": doc}))
            .collect();
        json!({
            "protocol": "oo.mongodb-preview.c1-applied/v1",
            "capture_method": "C1",
            "position_codec": CLUSTER_CODEC,
            "token_order": "Total",
            "position_lineage": self.lineage,
            "source_epoch": self.epoch,
            "resume_codec": RESUME_CODEC,
            "resume_token_sha256": resume_sha,
            "resume_token_compared": false,
            "applied_pos": decimal_text(self.applied),
            "grade": self.grade(),
            "cross_table_consistent": false,
            "rows": rows,
            "tombstones": self.tombs.iter().cloned().collect::<Vec<_>>(),
            "hidden_fields": self.hidden.iter().cloned().collect::<Vec<_>>(),
            "multi_commits": self.multi_commits,
            "commits": self.commits,
            "txn_groups": self.txn_groups,
            "commit_event_seen": self.commits > 0,
            "pre_images": self.pre_images,
            "post_images": self.post_images,
            "null_memo": self.null_memo,
            "missing_memo": self.missing_memo,
            "model_called": false,
            "import_invoked": false,
            "source_acceptance": false
        })
    }
}

fn dedup_id(lineage: &str, epoch: i64, event: &Event) -> String {
    format!("{lineage}:{epoch}:{}:{}", event.cluster, event.op_index)
}

fn order_group(events: &mut [Event]) -> Result<(), &'static str> {
    events.sort_by(|left, right| {
        left.cluster
            .cmp(&right.cluster)
            .then(left.op_index.cmp(&right.op_index))
    });
    if events.windows(2).any(|pair| {
        pair[0].cluster == pair[1].cluster && pair[0].op_index == pair[1].op_index
    }) {
        return Err("op_identity");
    }
    Ok(())
}

struct Gate {
    names: Mutex<Vec<String>>,
    writes: Mutex<Vec<String>>,
    l0_token: Mutex<Option<Document>>,
    l0_time: Mutex<Option<Timestamp>>,
    find_time: Mutex<Option<Timestamp>>,
}

impl Gate {
    fn new() -> Self {
        Self {
            names: Mutex::new(Vec::new()),
            writes: Mutex::new(Vec::new()),
            l0_token: Mutex::new(None),
            l0_time: Mutex::new(None),
            find_time: Mutex::new(None),
        }
    }

    fn on_command(&self, event: CommandEvent) {
        match event {
            CommandEvent::Started(event) => {
                if WRITE_COMMANDS.contains(&event.command_name.as_str()) {
                    if let Ok(mut guard) = self.writes.lock() {
                        guard.push(event.command_name.clone());
                    }
                }
                if let Ok(mut guard) = self.names.lock() {
                    guard.push(event.command_name);
                }
            }
            CommandEvent::Succeeded(event) => self.on_success(&event),
            _ => {}
        }
    }

    fn on_success(&self, event: &CommandSucceededEvent) {
        if event.command_name == "aggregate" && self.l0_token.lock().ok().is_some_and(|guard| guard.is_none()) {
            if let Ok(cursor) = event.reply.get_document("cursor") {
                if let Ok(token) = cursor.get_document("postBatchResumeToken") {
                    if let Ok(mut guard) = self.l0_token.lock() {
                        *guard = Some(token.to_owned());
                    }
                }
            }
            if let Ok(ts) = event.reply.get_timestamp("operationTime") {
                if let Ok(mut guard) = self.l0_time.lock() {
                    *guard = Some(ts);
                }
            }
        }
        if event.command_name == "find" {
            if let Ok(ts) = event.reply.get_timestamp("operationTime") {
                if let Ok(mut guard) = self.find_time.lock() {
                    *guard = Some(ts);
                }
            }
        }
    }
}

struct Config {
    host: String,
    port: u16,
    database: String,
    collection: String,
    username: String,
    auth_source: String,
    password: String,
    replica_set: String,
    control: PathBuf,
    dba_file: PathBuf,
}

pub fn serve(path: &str) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| err.to_string())?;
    runtime.block_on(run(path))
}

async fn run(path: &str) -> Result<(), String> {
    let config = load_config(path)?;
    fs::create_dir_all(&config.control).map_err(|err| err.to_string())?;
    let gate = Arc::new(Gate::new());
    let client = connect(&config, &gate).await?;
    let hello = client
        .database("admin")
        .run_command(doc! { "hello": 1 })
        .await
        .map_err(|err| err.to_string())?;
    let set_name = hello.get_str("setName").map_err(|_| "standalone has no change stream")?;
    if set_name != config.replica_set {
        return Err("replica set name mismatch".to_string());
    }
    let epoch = hello_epoch(&hello)?;
    refuse_writes(&gate)?;
    let coll = client
        .database(&config.database)
        .collection::<Document>(&config.collection);
    let mut cursor = coll
        .aggregate(change_pipeline(None))
        .batch_size(1)
        .max_await_time(Duration::from_millis(400))
        .comment("oo-mongodb-c1-reader".to_string())
        .await
        .map_err(|err| err.to_string())?;
    let l0_token = gate
        .l0_token
        .lock()
        .map_err(|_| "gate")?
        .clone()
        .ok_or("l0 resume token missing")?;
    let l0_time = *gate.l0_time.lock().map_err(|_| "gate")?.as_ref().ok_or("l0 clusterTime missing")?;
    let l0_cluster = cluster_decimal(l0_time.time, l0_time.increment);
    let token_at = token_cluster(l0_token.get_str("_data").map_err(|_| "resume token has no _data")?)?;
    write_json(
        &config.control.join("l0.json"),
        &json!({
            "resume_codec": RESUME_CODEC,
            "resume_token_sha256": token_sha(&l0_token)?,
            "resume_token_compared": false,
            "position_codec": CLUSTER_CODEC,
            "position_lineage": set_name,
            "source_epoch": epoch,
            "l0_cluster": decimal_text(l0_cluster),
            "token_cluster": decimal_text(token_at),
            "set_name": set_name,
        }),
    )?;
    flag(&config.control, "opened")?;
    let mut session = client.start_session().await.map_err(|err| err.to_string())?;
    let tx = TransactionOptions::builder()
        .read_concern(ReadConcern::snapshot())
        .build();
    session
        .start_transaction()
        .with_options(tx)
        .await
        .map_err(|err| err.to_string())?;
    let mut snap_cursor = coll
        .clone()
        .find(doc! {})
        .session(&mut session)
        .await
        .map_err(|err| err.to_string())?;
    let mut snap_rows = BTreeMap::new();
    while let Some(item) = snap_cursor.next(&mut session).await {
        let doc = item.map_err(|err| err.to_string())?;
        let key = key_of(doc.get("_id").ok_or("snapshot key")?)?;
        snap_rows.insert(key, canon(&doc));
    }
    drop(snap_cursor);
    let snap_time = *gate.find_time.lock().map_err(|_| "gate")?.as_ref().ok_or("snapshot clusterTime missing")?;
    let snapshot_cluster = cluster_decimal(snap_time.time, snap_time.increment);
    if cmp_cluster(l0_cluster, snapshot_cluster).is_gt() {
        return Err("l0 clusterTime is after the snapshot".to_string());
    }
    flag(&config.control, "snapshot-open")?;
    wait_flag(&config.control, "race-written", 30).await?;
    let mut check_cursor = coll
        .clone()
        .find(doc! { "_id": "c1-race" })
        .session(&mut session)
        .await
        .map_err(|err| err.to_string())?;
    let mut race_in_snapshot = snap_rows.contains_key("c1-race");
    if let Some(item) = check_cursor.next(&mut session).await {
        let _ = item.map_err(|err| err.to_string())?;
        race_in_snapshot = true;
    }
    drop(check_cursor);
    session.commit_transaction().await.map_err(|err| err.to_string())?;
    drop(session);
    if race_in_snapshot {
        return Err("race document was inside the snapshot".to_string());
    }
    let mut store = Store::new();
    store.lineage = set_name.to_string();
    store.epoch = epoch;
    store.load_snapshot(snap_rows, snapshot_cluster);
    let mut resume = l0_token.clone();
    let (tx, mut rx) = mpsc::channel(QUEUE_CAP);
    let mut reader = Some(tokio::spawn(read_loop(cursor, tx)));
    let mut ready = false;
    let mut paused = false;
    let mut quiesced = false;
    let mut expired = false;
    loop {
        if exists(&config.control, "stop") {
            if let Some(task) = reader.take() {
                task.abort();
            }
            break;
        }
        if exists(&config.control, "quiesce") && !quiesced {
            if let Some(task) = reader.take() {
                task.abort();
                let _ = task.await;
            }
            quiesced = true;
            flag(&config.control, "quiesced")?;
        }
        if quiesced && exists(&config.control, "expire") && !expired {
            expired = true;
            expire_once(&coll, &l0_token, &config).await?;
        }
        if quiesced {
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        }
        if exists(&config.control, "pause") && !paused {
            if let Some(task) = reader.take() {
                task.abort();
            }
            paused = true;
            flag(&config.control, "paused")?;
        }
        if paused && exists(&config.control, "resume") {
            cursor = open_stream(&coll, Some(&resume)).await.map_err(|err| err.to_string())?;
            let (next_tx, next_rx) = mpsc::channel(QUEUE_CAP);
            rx = next_rx;
            reader = Some(tokio::spawn(read_loop(cursor, next_tx)));
            paused = false;
            flag(&config.control, "resumed")?;
        }
        if paused {
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        }
        let pulled = tokio::time::timeout(Duration::from_millis(400), rx.recv()).await;
        match pulled {
            Ok(Some(item)) => {
                let doc = item?;
                if let Some(token) = doc.get_document("_id").ok() {
                    resume = token.to_owned();
                }
                if let Some(event) = event_from(&doc)? {
                    spool(&config.control, &doc, event.op_index)?;
                    if event.op != "ignore" && cmp_cluster(event.cluster, snapshot_cluster).is_gt() {
                        store.observe(event).map_err(|err| err.to_string())?;
                    }
                }
                refuse_writes(&gate)?;
                write_json(&config.control.join("applied.json"), &store.applied_json(&token_sha(&resume)?))?;
            }
            Ok(None) => return Err("change stream ended".to_string()),
            Err(_) => {
                if !ready && store.rows.contains_key("c1-race") {
                    ready = true;
                    write_json(
                        &config.control.join("barrier.json"),
                        &json!({
                            "initial_load": "cdc_first_snapshot_fill",
                            "l0_cluster": decimal_text(l0_cluster),
                            "snapshot_cluster": decimal_text(snapshot_cluster),
                            "applied_pos": decimal_text(store.applied),
                            "position_lineage": store.lineage,
                            "source_epoch": store.epoch,
                            "race_in_snapshot": false,
                            "race_from_stream": true,
                            "position_codec": CLUSTER_CODEC,
                            "resume_token_compared": false
                        }),
                    )?;
                    flag(&config.control, "ready")?;
                }
            }
        }
    }
    if let Some(task) = reader.take() {
        task.abort();
    }
    refuse_writes(&gate)?;
    let dba = fs::read_to_string(&config.dba_file).unwrap_or_else(|_| "{}".to_string());
    let dba_json: Value = serde_json::from_str(&dba).unwrap_or(Value::Null);
    write_json(
        &config.control.join("receipt.json"),
        &json!({
            "protocol": "oo.mongodb-preview.c1-receipt/v1",
            "capture_method": "C1",
            "set_name": set_name,
            "storage_engine": dba_json.get("storage_engine").cloned().unwrap_or(Value::Null),
            "grade": store.grade(),
            "applied_pos": decimal_text(store.applied),
            "position_lineage": store.lineage,
            "source_epoch": store.epoch,
            "position_codec": CLUSTER_CODEC,
            "resume_codec": RESUME_CODEC,
            "resume_token_sha256": token_sha(&resume)?,
            "resume_token_compared": false,
            "reader_write_commands": gate.writes.lock().map_err(|_| "gate")?.clone(),
            "model_called": false,
            "import_invoked": false,
            "source_acceptance": false,
            "formal_acceptance": "0/10",
            "apply_gap": apply_interface_gap(),
            "dba": dba_json
        }),
    )?;
    write_json(&config.control.join("applied.json"), &store.applied_json(&token_sha(&resume)?))?;
    client.shutdown().await;
    Ok(())
}

fn event_from(doc: &Document) -> Result<Option<Event>, String> {
    let op_name = doc.get_str("operationType").map_err(|_| "operationType".to_string())?;
    let op = match op_name {
        "insert" => "insert",
        "update" | "replace" => "update",
        "delete" => "delete",
        "commitTransaction" => "commit",
        "abortTransaction" => "abort",
        "createIndexes" | "dropIndexes" | "modify" | "create" | "shardCollection" => "ignore",
        other => return Err(format!("operationType {other}")),
    };
    let cluster_ts = doc.get_timestamp("clusterTime").map_err(|_| "clusterTime".to_string())?;
    let key = doc
        .get_document("documentKey")
        .ok()
        .and_then(|item| item.get("_id"))
        .map(key_of)
        .transpose()?;
    let lsid = doc.get_document("lsid").ok().and_then(|item| item.get("id")).map(|id| {
        Bson::from(id.clone()).into_canonical_extjson().to_string()
    });
    let txn = match doc.get("txnNumber") {
        Some(Bson::Int64(value)) => Some(*value),
        Some(Bson::Int32(value)) => Some(i64::from(*value)),
        _ => None,
    };
    let data = doc
        .get_document("_id")
        .and_then(|token| token.get_str("_data"))
        .map_err(|_| "resume token".to_string())?;
    let index = op_index(data).map_err(|_| "op identity".to_string())?;
    Ok(Some(Event {
        op: op.to_string(),
        key,
        doc: embedded(doc, "fullDocument"),
        pre: embedded(doc, "fullDocumentBeforeChange"),
        cluster: cluster_decimal(cluster_ts.time, cluster_ts.increment),
        lsid,
        txn,
        op_index: index,
    }))
}

fn embedded(doc: &Document, name: &str) -> Option<Value> {
    match doc.get(name)? {
        Bson::Document(inner) => Some(canon(inner)),
        _ => None,
    }
}

fn canon(doc: &Document) -> Value {
    Bson::Document(doc.clone()).into_canonical_extjson()
}

fn key_of(id: &Bson) -> Result<String, String> {
    match id {
        Bson::ObjectId(oid) => Ok(oid.to_hex()),
        Bson::String(value) => Ok(value.clone()),
        other => Ok(Bson::from(other.clone()).into_canonical_extjson().to_string()),
    }
}

fn change_pipeline(resume: Option<&Document>) -> Vec<Document> {
    let mut body = doc! {
        "fullDocument": "whenAvailable",
        "fullDocumentBeforeChange": "whenAvailable",
        "showExpandedEvents": true,
    };
    if let Some(token) = resume {
        body.insert("resumeAfter", token.clone());
    }
    vec![doc! { "$changeStream": body }]
}

async fn open_stream(
    coll: &mongodb::Collection<Document>,
    resume: Option<&Document>,
) -> Result<mongodb::Cursor<Document>, mongodb::error::Error> {
    coll.aggregate(change_pipeline(resume))
        .batch_size(1)
        .max_await_time(Duration::from_millis(400))
        .comment("oo-mongodb-c1-reader".to_string())
        .await
}

async fn read_loop(mut cursor: mongodb::Cursor<Document>, tx: mpsc::Sender<Result<Document, String>>) {
    loop {
        match cursor.advance().await {
            Ok(true) => {
                let doc = match cursor.deserialize_current() {
                    Ok(doc) => doc,
                    Err(err) => {
                        let _ = tx.send(Err(err.to_string())).await;
                        break;
                    }
                };
                if tx.send(Ok(doc)).await.is_err() {
                    break;
                }
            }
            Ok(false) => break,
            Err(err) => {
                let _ = tx.send(Err(err.to_string())).await;
                break;
            }
        }
    }
}

async fn expire_once(
    coll: &mongodb::Collection<Document>,
    l0: &Document,
    config: &Config,
) -> Result<(), String> {
    // The stored token is resumed unchanged. A rewritten timestamp is not expiry.
    let (code, name, message, continued) = match open_stream(coll, Some(l0)).await {
        Ok(mut cursor) => match cursor.advance().await {
            Ok(_) => (0, "accepted".to_string(), "cursor continued".to_string(), true),
            Err(err) => command_failure(&err),
        },
        Err(err) => command_failure(&err),
    };
    let expired = !continued && position_expired(code);
    write_json(
        &config.control.join("expire.json"),
        &expiry_record(code, &name, &message, continued),
    )?;
    if !expired {
        return Err("stored resume token was not POSITION_EXPIRED".to_string());
    }
    write_json(&config.control.join("recovery.json"), &rebaseline_barrier())?;
    Ok(())
}

fn command_failure(err: &mongodb::error::Error) -> (i32, String, String, bool) {
    match err.kind.as_ref() {
        ErrorKind::Command(cmd) => (cmd.code, cmd.code_name.clone(), cmd.message.clone(), false),
        other => (0, "not_command".to_string(), other.to_string(), false),
    }
}

fn refuse_writes(gate: &Gate) -> Result<(), String> {
    let guard = gate.writes.lock().map_err(|_| "gate".to_string())?;
    if guard.is_empty() {
        Ok(())
    } else {
        Err("reader issued a write command".to_string())
    }
}

async fn connect(config: &Config, gate: &Arc<Gate>) -> Result<Client, String> {
    let mut options = ClientOptions::parse(format!(
        "mongodb://{}:{}/?directConnection=true&retryWrites=false&retryReads=false",
        config.host, config.port
    ))
    .await
    .map_err(|err| err.to_string())?;
    options.credential = Some(
        Credential::builder()
            .username(config.username.clone())
            .password(config.password.clone())
            .source(config.auth_source.clone())
            .build(),
    );
    options.direct_connection = Some(true);
    options.retry_writes = Some(false);
    options.retry_reads = Some(false);
    options.max_pool_size = Some(4);
    options.app_name = Some("oo-mongodb-c1-reader".to_string());
    options.server_selection_timeout = Some(Duration::from_secs(15));
    options.server_monitoring_mode = Some(ServerMonitoringMode::Poll);
    let watched = Arc::clone(gate);
    options.command_event_handler = Some(EventHandler::callback(move |event: CommandEvent| {
        watched.on_command(event);
    }));
    Client::with_options(options).map_err(|err| err.to_string())
}

fn load_config(path: &str) -> Result<Config, String> {
    let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
    let value: Value = serde_json::from_str(&text).map_err(|_| "c1 config is not JSON".to_string())?;
    let password_file = text_at(&value, "password_file")?;
    let password = fs::read_to_string(&password_file).map_err(|err| err.to_string())?;
    Ok(Config {
        host: text_at(&value, "host")?,
        port: value.get("port").and_then(Value::as_u64).ok_or("port")? as u16,
        database: text_at(&value, "database")?,
        collection: text_at(&value, "collection")?,
        username: text_at(&value, "username")?,
        auth_source: text_at(&value, "auth_source")?,
        password: password.trim().to_string(),
        replica_set: text_at(&value, "replica_set")?,
        control: PathBuf::from(text_at(&value, "control_dir")?),
        dba_file: PathBuf::from(text_at(&value, "dba_file")?),
    })
}

fn text_at(value: &Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .ok_or_else(|| key.to_string())
}

fn token_sha(token: &Document) -> Result<String, String> {
    let bytes = mongodb::bson::to_vec(token).map_err(|err| err.to_string())?;
    Ok(resume_digest(&bytes))
}

fn spool(control: &Path, doc: &Document, op_index: u32) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(control.join("spool.jsonl"))
        .map_err(|err| err.to_string())?;
    let line = json!({
        "op_index": op_index,
        "event": canon(doc),
        "resume_sha256": doc.get_document("_id").ok().and_then(|token| token_sha(token).ok())
    });
    writeln!(file, "{}", line).map_err(|err| err.to_string())?;
    file.sync_all().map_err(|err| err.to_string())
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|err| err.to_string())?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).map_err(|err| err.to_string())?;
    fs::rename(&tmp, path).map_err(|err| err.to_string())
}

fn flag(control: &Path, name: &str) -> Result<(), String> {
    fs::write(control.join(name), b"1\n").map_err(|err| err.to_string())
}

fn exists(control: &Path, name: &str) -> bool {
    control.join(name).is_file()
}

async fn wait_flag(control: &Path, name: &str, seconds: u64) -> Result<(), String> {
    for _ in 0..(seconds * 10) {
        if exists(control, name) {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(format!("{name} missing"))
}

fn hello_epoch(hello: &Document) -> Result<i64, String> {
    let op_time = hello
        .get_document("lastWrite")
        .and_then(|last| last.get_document("opTime"))
        .map_err(|_| "epoch".to_string())?;
    match op_time.get("t") {
        Some(Bson::Int64(value)) if *value >= 0 => Ok(*value),
        Some(Bson::Int32(value)) if *value >= 0 => Ok(i64::from(*value)),
        _ => Err("epoch".to_string()),
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

const SOURCE_ID: &str = "mongodb_connection_whole";
const REQUEST_PROTOCOL: &str = "oo.pipeline.native-update/v1";

pub fn field_state(doc: &Value, field: &str) -> &'static str {
    match doc.get(field) {
        None => "missing",
        Some(Value::Null) => "null",
        Some(_) => "value",
    }
}

pub fn public_doc(doc: &Value) -> Value {
    let Some(map) = doc.as_object() else {
        return Value::Object(serde_json::Map::new());
    };
    let mut out = serde_json::Map::new();
    for (key, value) in map {
        if APPROVED_FIELDS.contains(&key.as_str()) {
            out.insert(key.clone(), value.clone());
        }
    }
    Value::Object(out)
}

fn presence_text(doc: &Value) -> String {
    let mut out = serde_json::Map::new();
    for field in ["blob", "marks", "memo", "pack"] {
        out.insert(field.to_string(), json!(field_state(doc, field)));
    }
    Value::Object(out).to_string()
}

pub fn snapshot_before(
    current: &BTreeMap<String, Value>,
    events: &[Event],
) -> Result<BTreeMap<String, Value>, &'static str> {
    let mut rows = current.clone();
    let mut ordered = events.to_vec();
    order_group(&mut ordered)?;
    for event in ordered.iter().rev() {
        if matches!(event.op.as_str(), "ignore" | "commit" | "abort") {
            continue;
        }
        let key = event.key.as_deref().ok_or("key")?;
        match event.op.as_str() {
            "insert" => {
                rows.remove(key);
            }
            "update" | "delete" => {
                let pre = event.pre.clone().ok_or("pre_image")?;
                rows.insert(key.to_string(), pre);
            }
            _ => return Err("op"),
        }
    }
    Ok(rows)
}

pub fn seal_initial(rows: &BTreeMap<String, Value>) -> Result<arrow_utf8::ArrowFile, &'static str> {
    let columns = [
        Utf8Column { name: "_id".to_string(), nullable: false },
        Utf8Column { name: "doc".to_string(), nullable: false },
        Utf8Column { name: "presence".to_string(), nullable: false },
    ];
    let mut keys: Vec<&String> = rows.keys().collect();
    keys.sort();
    let body: Vec<Vec<Utf8Cell>> = keys
        .iter()
        .map(|key| {
            let doc = public_doc(&rows[*key]);
            vec![
                Utf8Cell::Text((*key).clone()),
                Utf8Cell::Text(doc.to_string()),
                Utf8Cell::Text(presence_text(&doc)),
            ]
        })
        .collect();
    arrow_utf8::write_utf8_file(&columns, &body)
}

pub fn seal_delta(events: &[Event], lineage: &str, epoch: i64) -> Result<arrow_utf8::ArrowFile, &'static str> {
    let columns = [
        Utf8Column { name: "op_identity".to_string(), nullable: false },
        Utf8Column { name: "op".to_string(), nullable: false },
        Utf8Column { name: "_id".to_string(), nullable: false },
        Utf8Column { name: "before".to_string(), nullable: true },
        Utf8Column { name: "after".to_string(), nullable: true },
        Utf8Column { name: "before_presence".to_string(), nullable: true },
        Utf8Column { name: "after_presence".to_string(), nullable: true },
        Utf8Column { name: "cluster".to_string(), nullable: false },
        Utf8Column { name: "lineage".to_string(), nullable: false },
        Utf8Column { name: "epoch".to_string(), nullable: false },
        Utf8Column { name: "txn_group".to_string(), nullable: false },
    ];
    let mut ordered = events.to_vec();
    order_group(&mut ordered)?;
    let body: Vec<Vec<Utf8Cell>> = ordered
        .iter()
        .filter(|event| !matches!(event.op.as_str(), "ignore" | "commit" | "abort"))
        .map(|event| {
            let key = event.key.clone().unwrap_or_default();
            let before = event.pre.as_ref().map(public_doc);
            let after = event.doc.as_ref().map(public_doc);
            let group = match (&event.lsid, event.txn) {
                (Some(lsid), Some(txn)) => format!("{lsid}:{txn}"),
                _ => String::new(),
            };
            vec![
                Utf8Cell::Text(format!("{}:{}", event.cluster, event.op_index)),
                Utf8Cell::Text(event.op.clone()),
                Utf8Cell::Text(key),
                image_cell(before.as_ref()),
                image_cell(after.as_ref()),
                presence_cell(before.as_ref()),
                presence_cell(after.as_ref()),
                Utf8Cell::Text(decimal_text(event.cluster)),
                Utf8Cell::Text(lineage.to_string()),
                Utf8Cell::Text(epoch.to_string()),
                Utf8Cell::Text(group),
            ]
        })
        .collect();
    arrow_utf8::write_utf8_file(&columns, &body)
}

fn image_cell(doc: Option<&Value>) -> Utf8Cell {
    match doc {
        Some(doc) => Utf8Cell::Text(doc.to_string()),
        None => Utf8Cell::Null,
    }
}

fn presence_cell(doc: Option<&Value>) -> Utf8Cell {
    match doc {
        Some(doc) => Utf8Cell::Text(presence_text(doc)),
        None => Utf8Cell::Null,
    }
}

pub fn catalog_adapter(
    docs: &[Value],
    grade: &str,
    commit_event_seen: bool,
) -> Result<Value, &'static str> {
    if !matches!(grade, "S0" | "B" | "T") || (grade == "T" && !commit_event_seen) {
        return Err("grade");
    }
    let ontology: Vec<Value> = frozen_columns()
        .iter()
        .map(|column| {
            json!({
                "name": column.name,
                "arrow_type": column.arrow_type,
                "nullable": column.nullable,
                "precision": column.precision,
                "scale": column.scale
            })
        })
        .collect();
    if ontology.iter().any(|column| column["name"] == "secretNote") {
        return Err("secret");
    }
    let value = json!({
        "protocol": "oo.mongodb-preview.c1-catalog/v1",
        "collection": "oo_w141.shipments",
        "collections": 1,
        "engine": "mongodb",
        "source_id": SOURCE_ID,
        "capture_method": "C1",
        "consistency_grade": grade,
        "grade_t_claimed": grade == "T",
        "txn_grouping": if commit_event_seen { "T" } else { "S0" },
        "txn_note": "events that share lsid and txnNumber apply together; without commitTransaction the grade stays S0",
        "customer_writes": false,
        "hidden_fields": ["secretNote"],
        "public_extract": APPROVED_FIELDS,
        "ontology_columns": ontology,
        "sealed_arrow_type": "utf8",
        "null_versus_missing": "presence text, not a collapsed null",
        "variants": doc_variants(docs),
        "import_invoked": false,
        "model_called": false,
        "source_acceptance": false
    });
    if crate::carries_client_authority(&value) {
        return Err("authority");
    }
    Ok(value)
}

fn doc_variants(docs: &[Value]) -> Value {
    let mut seen: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut fields = BTreeSet::new();
    for doc in docs {
        let Some(map) = doc.as_object() else { continue };
        for key in map.keys() {
            fields.insert(key.clone());
        }
    }
    for doc in docs {
        let map = doc.as_object();
        for field in &fields {
            let tag = match map.and_then(|item| item.get(field)) {
                None => "missing",
                Some(value) => type_tag(value),
            };
            seen.entry(field.clone()).or_default().insert(tag.to_string());
        }
    }
    Value::Object(
        seen.into_iter()
            .map(|(key, tags)| (key, json!(tags.into_iter().collect::<Vec<_>>())))
            .collect(),
    )
}

fn type_tag(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(map) if map.contains_key("$oid") => "$oid",
        Value::Object(map) if map.contains_key("$numberLong") => "$numberLong",
        Value::Object(map) if map.contains_key("$numberInt") => "$numberInt",
        Value::Object(map) if map.contains_key("$numberDecimal") => "$numberDecimal",
        Value::Object(map) if map.contains_key("$numberDouble") => "$numberDouble",
        Value::Object(map) if map.contains_key("$date") => "$date",
        Value::Object(map) if map.contains_key("$binary") => "$binary",
        Value::Object(_) => "object",
    }
}

pub struct BatchRef<'a> {
    pub arrow_sha256: &'a str,
    pub schema_sha256: &'a str,
    pub rows: u64,
    pub arrow_path: &'a str,
}

pub fn producer_wire(
    lineage: &str,
    epoch: i64,
    snapshot_cluster: u64,
    applied: u64,
    grade: &str,
    commit_event_seen: bool,
    resume_sha: &str,
    initial: &BatchRef<'_>,
    delta: &BatchRef<'_>,
    tombstones: &[String],
    catalog_sha: &str,
) -> Result<Value, &'static str> {
    if lineage.is_empty() || epoch < 0 || snapshot_cluster == 0 || applied == 0 {
        return Err("watermark");
    }
    if !matches!(grade, "S0" | "B" | "T") || (grade == "T" && !commit_event_seen) {
        return Err("grade");
    }
    if resume_sha.len() != 64 {
        return Err("resume");
    }
    let initial_request = native_request(
        1,
        None,
        snapshot_cluster,
        lineage,
        epoch,
        grade,
        resume_sha,
        initial,
        &[],
        "c1-initial",
        catalog_sha,
    )?;
    let delta_request = native_request(
        2,
        Some(1),
        applied,
        lineage,
        epoch,
        grade,
        resume_sha,
        delta,
        tombstones,
        "c1-delta",
        catalog_sha,
    )?;
    let value = json!({
        "protocol": "oo.mongodb-preview.c1-producer-wire/v1",
        "submitted": false,
        "import_invoked": false,
        "ack": false,
        "loader_invoked": false,
        "generation_created": false,
        "model_called": false,
        "source_acceptance": false,
        "grade_t_claimed": grade == "T",
        "txn_grouping": if commit_event_seen { "T" } else { "S0" },
        "full_snapshot": false,
        "capture_method": "c1_cdc",
        "position_codec": CLUSTER_CODEC,
        "position_lineage": lineage,
        "source_epoch": epoch,
        "resume_token_compared": false,
        "gap": apply_interface_gap(),
        "on_position_expired": expiry_record(286, "ChangeStreamHistoryLost", "", false),
        "recovery": rebaseline_barrier(),
        "initial": initial_request,
        "delta": delta_request
    });
    if crate::carries_client_authority(&value) {
        return Err("authority");
    }
    Ok(value)
}

fn native_request(
    generation: i64,
    parent: Option<i64>,
    applied: u64,
    lineage: &str,
    epoch: i64,
    grade: &str,
    resume_sha: &str,
    batch: &BatchRef<'_>,
    tombstones: &[String],
    batch_id: &str,
    catalog_sha: &str,
) -> Result<Value, &'static str> {
    let columns: Vec<Value> = if batch_id.ends_with("initial") {
        vec![
            json!({"name": "_id", "logical_name": "_id", "arrow_type": "utf8", "nullable": false}),
            json!({"name": "doc", "logical_name": "doc", "arrow_type": "utf8", "nullable": false, "document": true}),
            json!({"name": "presence", "logical_name": "presence", "arrow_type": "utf8", "nullable": false}),
        ]
    } else {
        vec![
            json!({"name": "op_identity", "logical_name": "op_identity", "arrow_type": "utf8", "nullable": false}),
            json!({"name": "op", "logical_name": "op", "arrow_type": "utf8", "nullable": false}),
            json!({"name": "_id", "logical_name": "_id", "arrow_type": "utf8", "nullable": false}),
            json!({"name": "before", "logical_name": "before", "arrow_type": "utf8", "nullable": true, "document": true}),
            json!({"name": "after", "logical_name": "after", "arrow_type": "utf8", "nullable": true, "document": true}),
            json!({"name": "before_presence", "logical_name": "before_presence", "arrow_type": "utf8", "nullable": true}),
            json!({"name": "after_presence", "logical_name": "after_presence", "arrow_type": "utf8", "nullable": true}),
            json!({"name": "cluster", "logical_name": "cluster", "arrow_type": "utf8", "nullable": false}),
            json!({"name": "lineage", "logical_name": "lineage", "arrow_type": "utf8", "nullable": false}),
            json!({"name": "epoch", "logical_name": "epoch", "arrow_type": "utf8", "nullable": false}),
            json!({"name": "txn_group", "logical_name": "txn_group", "arrow_type": "utf8", "nullable": false}),
        ]
    };
    let mut request = json!({
        "protocol": REQUEST_PROTOCOL,
        "source_engine": "mongodb",
        "source_id": SOURCE_ID,
        "source_generation": generation,
        "logical_schema": "oo_w141",
        "logical_table": "shipments",
        "batch_id": batch_id,
        "capture_method": "c1_cdc",
        "delete_detection": "native",
        "consistency_grade": grade,
        "cross_table_consistent": false,
        "full_snapshot": false,
        "position_codec": CLUSTER_CODEC,
        "position_lineage": lineage,
        "source_epoch": epoch,
        "applied_pos": decimal_text(applied),
        "resume_codec": RESUME_CODEC,
        "resume_token_sha256": resume_sha,
        "resume_token_compared": false,
        "config_digest": digest_json(&json!({"batch": batch_id, "codec": CLUSTER_CODEC})),
        "schema_digest": catalog_sha,
        "type_digest": digest_json(&json!({"columns": frozen_columns().len(), "batch": batch_id})),
        "input_digest": digest_json(&json!({"rows": batch.rows, "applied": applied})),
        "audit_digest": digest_json(&json!({"resume": resume_sha, "batch": batch_id})),
        "columns": columns,
        "arrow_path": batch.arrow_path,
        "arrow_sha256": batch.arrow_sha256,
        "arrow_schema_sha256": batch.schema_sha256,
        "tombstones": tombstones,
        "row_identity": "unique",
        "key_column": "_id",
        "mirror": false
    });
    if let Some(parent) = parent {
        request["parent_source_generation"] = json!(parent);
    }
    Ok(request)
}

pub fn signed_binding(
    catalog_sha: &str,
    initial: &BatchRef<'_>,
    delta: &BatchRef<'_>,
) -> Result<Value, &'static str> {
    let fields: Vec<Value> = frozen_columns()
        .iter()
        .map(|column| {
            json!({
                "name": column.name,
                "arrow_type": column.arrow_type,
                "nullable": column.nullable
            })
        })
        .collect();
    let mut binding = json!({
        "protocol": "oo.source-admission.capture-binding/v1",
        "source_engine": "mongodb",
        "source_id": SOURCE_ID,
        "submitted": false,
        "import_invoked": false,
        "decision_minted": false,
        "catalog_sha256": catalog_sha,
        "objects": [
            {"logical_schema": "oo_w141", "logical_table": "shipments", "batch": "initial", "row_count": initial.rows, "arrow_sha256": initial.arrow_sha256},
            {"logical_schema": "oo_w141", "logical_table": "shipments", "batch": "delta", "row_count": delta.rows, "arrow_sha256": delta.arrow_sha256}
        ],
        "fields": fields
    });
    binding["binding_sha256"] = json!(digest_json(&binding));
    if crate::carries_client_authority(&binding) {
        return Err("authority");
    }
    Ok(binding)
}

pub fn source_gold(rows: &BTreeMap<String, Value>, grade: &str, commit_event_seen: bool) -> Value {
    let public: BTreeMap<String, Value> = rows
        .iter()
        .map(|(key, doc)| (key.clone(), public_doc(doc)))
        .collect();
    json!({
        "public_rows": public.len(),
        "race_memo": memo_of(&public, "c1-race", "C1-RACE"),
        "miss_memo": memo_of(&public, "c1-miss", "C1-MISS"),
        "secret_note_public": public.values().any(|doc| doc.get("secretNote").is_some()),
        "consistency_grade": grade,
        "gap_present": public.contains_key("c1-gap"),
        "grade_t_claimed": commit_event_seen && grade == "T"
    })
}

fn memo_of(rows: &BTreeMap<String, Value>, key: &str, code: &str) -> Value {
    let found = rows.get(key).or_else(|| {
        rows.values()
            .find(|doc| doc.get("code").and_then(Value::as_str) == Some(code))
    });
    match found {
        None => json!("absent"),
        Some(doc) => json!(field_state(doc, "memo")),
    }
}

pub fn question_bank(gold: &Value) -> Result<Vec<Value>, &'static str> {
    let items = vec![
        candidate("row_count", "cardinality", "How many public shipment rows are in the current source read?", "public_rows"),
        candidate("race_memo", "presence", "What is the memo state of shipment c1-race?", "race_memo"),
        candidate("miss_memo", "presence", "What is the memo state of shipment c1-miss?", "miss_memo"),
        candidate("secret_hidden", "classification", "Is secretNote present on the public extract?", "secret_note_public"),
        candidate("capture_grade", "consistency", "Which consistency grade does this change-stream capture claim?", "consistency_grade"),
        candidate("gap_row", "deletion", "Is shipment c1-gap present in the current source rows?", "gap_present"),
    ];
    if !(4..=8).contains(&items.len()) {
        return Err("bank");
    }
    let mut payloads = BTreeSet::new();
    let mut grouped: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for item in &items {
        let key = item["truth_key"].as_str().ok_or("bank")?;
        let payload = gold.get(key).ok_or("bank")?.to_string();
        payloads.insert(payload.clone());
        grouped.entry(item["query_class"].as_str().ok_or("bank")?).or_default().insert(payload);
    }
    if payloads.len() < 4 {
        return Err("bank");
    }
    let classes: Vec<&BTreeSet<String>> = grouped.values().collect();
    let different = classes.iter().enumerate().any(|(index, left)| {
        classes.iter().skip(index + 1).any(|right| left != right)
    });
    if !different {
        return Err("bank");
    }
    Ok(items)
}

fn candidate(semantic_id: &str, query_class: &str, prompt: &str, truth_key: &str) -> Value {
    json!({
        "semantic_id": semantic_id,
        "query_class": query_class,
        "prompt": prompt,
        "truth_key": truth_key
    })
}

fn digest_json(value: &Value) -> String {
    arrow_utf8::hex_sha(&serde_json::to_vec(value).unwrap_or_default())
}

pub fn prepare(config_path: &str, spool_path: &str, out_dir: &str) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| err.to_string())?;
    runtime.block_on(prepare_run(config_path, spool_path, out_dir))
}

async fn prepare_run(config_path: &str, spool_path: &str, out_dir: &str) -> Result<(), String> {
    let config = load_config(config_path)?;
    let out = PathBuf::from(out_dir);
    fs::create_dir_all(&out).map_err(|err| err.to_string())?;
    let gate = Arc::new(Gate::new());
    let client = connect(&config, &gate).await?;
    let hello = client
        .database("admin")
        .run_command(doc! { "hello": 1 })
        .await
        .map_err(|err| err.to_string())?;
    let set_name = hello.get_str("setName").map_err(|_| "standalone has no change stream")?;
    if set_name != config.replica_set {
        return Err("replica set name mismatch".to_string());
    }
    let epoch = hello_epoch(&hello)?;
    let snapshot_cluster = read_decimal(&config.control.join("barrier.json"), "snapshot_cluster")?;
    let resume_sha = read_string(&config.control.join("applied.json"), "resume_token_sha256")?;
    let coll = client
        .database(&config.database)
        .collection::<Document>(&config.collection);
    let mut cursor = coll.find(doc! {}).await.map_err(|err| err.to_string())?;
    let mut current = BTreeMap::new();
    loop {
        if !cursor.advance().await.map_err(|err| err.to_string())? {
            break;
        }
        let doc = cursor.deserialize_current().map_err(|err| err.to_string())?;
        let key = key_of(doc.get("_id").ok_or("snapshot key")?)?;
        current.insert(key, canon(&doc));
    }
    drop(cursor);
    refuse_writes(&gate)?;
    let mut events = read_spool(Path::new(spool_path))?;
    events.retain(|event| event.op != "ignore" && cmp_cluster(event.cluster, snapshot_cluster).is_gt());
    dedup_events(&mut events);
    order_group(&mut events).map_err(|err| err.to_string())?;
    let initial_rows = snapshot_before(&current, &events).map_err(|err| err.to_string())?;
    let mut store = Store::new();
    store.lineage = set_name.to_string();
    store.epoch = epoch;
    store.load_snapshot(initial_rows.clone(), snapshot_cluster);
    store.replay(&events).map_err(|err| err.to_string())?;
    if store.rows != current {
        return Err(format!("source read differs from replayed stream: {} vs {}", store.rows.len(), current.len()));
    }
    let grade = store.grade();
    let commit_event_seen = store.commits > 0;
    if grade == "T" && !commit_event_seen {
        return Err("grade".to_string());
    }
    let initial_arrow = seal_initial(&initial_rows).map_err(|err| err.to_string())?;
    let delta_arrow = seal_delta(&events, set_name, epoch).map_err(|err| err.to_string())?;
    if initial_arrow.bytes.windows(10).any(|window| window == b"secretNote")
        || delta_arrow.bytes.windows(10).any(|window| window == b"secretNote")
    {
        return Err("secretNote was sealed".to_string());
    }
    let public_now: Vec<Value> = current.values().map(public_doc).collect();
    let mut catalog = catalog_adapter(&public_now, grade, commit_event_seen).map_err(|err| err.to_string())?;
    catalog["pre_images"] = json!(store.pre_images);
    catalog["post_images"] = json!(store.post_images);
    catalog["null_memo"] = json!(store.null_memo);
    catalog["missing_memo"] = json!(store.missing_memo);
    catalog["txn_groups"] = json!(store.txn_groups);
    let catalog_bytes = serde_json::to_vec(&catalog).map_err(|err| err.to_string())?;
    let catalog_sha = arrow_utf8::hex_sha(&catalog_bytes);
    let initial_path = out.join("initial.arrow");
    let delta_path = out.join("delta.arrow");
    fs::write(&initial_path, &initial_arrow.bytes).map_err(|err| err.to_string())?;
    fs::write(&delta_path, &delta_arrow.bytes).map_err(|err| err.to_string())?;
    let tombs: Vec<String> = store.tombs.iter().cloned().collect();
    let delta_rows = u64::try_from(
        events
            .iter()
            .filter(|event| !matches!(event.op.as_str(), "commit" | "abort" | "ignore"))
            .count(),
    )
    .map_err(|_| "rows")?;
    let initial_ref = BatchRef {
        arrow_sha256: &initial_arrow.file_sha256,
        schema_sha256: &initial_arrow.schema_sha256,
        rows: u64::try_from(initial_rows.len()).map_err(|_| "rows")?,
        arrow_path: initial_path.to_str().ok_or("path")?,
    };
    let delta_ref = BatchRef {
        arrow_sha256: &delta_arrow.file_sha256,
        schema_sha256: &delta_arrow.schema_sha256,
        rows: delta_rows,
        arrow_path: delta_path.to_str().ok_or("path")?,
    };
    let wire = producer_wire(
        set_name,
        epoch,
        snapshot_cluster,
        store.applied,
        grade,
        commit_event_seen,
        &resume_sha,
        &initial_ref,
        &delta_ref,
        &tombs,
        &catalog_sha,
    )
    .map_err(|err| err.to_string())?;
    let binding = signed_binding(&catalog_sha, &initial_ref, &delta_ref).map_err(|err| err.to_string())?;
    let gold = source_gold(&current, grade, commit_event_seen);
    let bank = question_bank(&gold).map_err(|err| err.to_string())?;
    let mut truth = Vec::new();
    let mut keys: Vec<&String> = current.keys().collect();
    keys.sort();
    for key in keys {
        truth.push(json!([key, public_doc(&current[key]).to_string()]));
    }
    write_json(&out.join("catalog.json"), &catalog)?;
    write_json(&out.join("binding.json"), &binding)?;
    write_json(&out.join("wire.json"), &wire)?;
    write_json(&out.join("gold.json"), &gold)?;
    write_json(&out.join("truth.json"), &json!(truth))?;
    write_json(&out.join("bank.json"), &json!(bank))?;
    write_json(
        &out.join("summary.json"),
        &json!({
            "protocol": "oo.mongodb-preview.c1-prepare/v1",
            "source_rows": current.len(),
            "initial_rows": initial_rows.len(),
            "delta_rows": delta_rows,
            "grade": grade,
            "commit_event_seen": commit_event_seen,
            "txn_groups": store.txn_groups,
            "position_lineage": set_name,
            "source_epoch": epoch,
            "applied_pos": decimal_text(store.applied),
            "snapshot_cluster": decimal_text(snapshot_cluster),
            "submitted": false,
            "import_invoked": false,
            "ack": false,
            "model_called": false,
            "source_acceptance": false,
            "formal_acceptance": "0/10",
            "customer_writes": false,
            "hidden_fields": ["secretNote"]
        }),
    )?;
    client.shutdown().await;
    Ok(())
}

fn dedup_events(events: &mut Vec<Event>) {
    let mut seen = BTreeSet::new();
    events.retain(|event| seen.insert(format!("{}:{}", event.cluster, event.op_index)));
}

fn read_spool(path: &Path) -> Result<Vec<Event>, String> {
    let file = fs::File::open(path).map_err(|err| err.to_string())?;
    let mut events = Vec::new();
    for line in std::io::BufReader::new(file).lines() {
        let line = line.map_err(|err| err.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(&line).map_err(|_| "spool".to_string())?;
        let event = value.get("event").ok_or("spool")?;
        events.push(event_from_extjson(event)?);
    }
    if events.is_empty() {
        return Err("spool is empty".to_string());
    }
    Ok(events)
}

fn event_from_extjson(value: &Value) -> Result<Event, String> {
    let op_name = value.get("operationType").and_then(Value::as_str).ok_or("operationType")?;
    let op = match op_name {
        "insert" => "insert",
        "update" | "replace" => "update",
        "delete" => "delete",
        "commitTransaction" => "commit",
        "abortTransaction" => "abort",
        "createIndexes" | "dropIndexes" | "modify" | "create" | "shardCollection" => "ignore",
        other => return Err(format!("operationType {other}")),
    };
    let secs = value
        .pointer("/clusterTime/$timestamp/t")
        .and_then(Value::as_u64)
        .ok_or("clusterTime")?;
    let inc = value
        .pointer("/clusterTime/$timestamp/i")
        .and_then(Value::as_u64)
        .ok_or("clusterTime")?;
    let data = value.pointer("/_id/_data").and_then(Value::as_str).ok_or("resume token")?;
    let index = op_index(data).map_err(|_| "op identity".to_string())?;
    let txn = value
        .pointer("/txnNumber/$numberLong")
        .and_then(Value::as_str)
        .and_then(|text| text.parse::<i64>().ok())
        .or_else(|| value.get("txnNumber").and_then(Value::as_i64));
    Ok(Event {
        op: op.to_string(),
        key: value.pointer("/documentKey/_id").map(ext_key).transpose()?,
        doc: value.get("fullDocument").filter(|item| item.is_object()).cloned(),
        pre: value.get("fullDocumentBeforeChange").filter(|item| item.is_object()).cloned(),
        cluster: cluster_decimal(u32::try_from(secs).map_err(|_| "clusterTime")?, u32::try_from(inc).map_err(|_| "clusterTime")?),
        lsid: value.pointer("/lsid/id").map(ToString::to_string),
        txn,
        op_index: index,
    })
}

fn ext_key(value: &Value) -> Result<String, String> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Object(map) => {
            if let Some(oid) = map.get("$oid").and_then(Value::as_str) {
                Ok(oid.to_string())
            } else {
                Ok(value.to_string())
            }
        }
        other => Ok(other.to_string()),
    }
}

fn read_decimal(path: &Path, key: &str) -> Result<u64, String> {
    let text = read_string(path, key)?;
    text.parse::<u64>().map_err(|_| format!("{key} is not a clusterTime"))
}

fn read_string(path: &Path, key: &str) -> Result<String, String> {
    let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
    let value: Value = serde_json::from_str(&text).map_err(|_| "json".to_string())?;
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .ok_or_else(|| key.to_string())
}
