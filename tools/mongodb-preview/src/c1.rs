//! Replica-set change stream capture.
//!
//! `clusterTime` is the comparable watermark (`mongo-cluster-time/v1`).
//! The resume token is opaque (`mongo-resume-token/v1`) and is never ordered.
//! Grade T is claimed only after an explicit `commitTransaction` event is applied.
//! A collection stream on this server expands a transaction into document events and
//! does not emit that commit event. Those events stay buffered until the next event
//! closes the group, then apply atomically at grade S0. This is not the standalone C5 diff.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
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

use crate::capture::APPROVED_FIELDS;
use crate::docseal;

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
    pub ordinal: u32,
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

/// Cluster time embedded in a v1 resume token. The server uses it for history-lost.
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
            "batch-cursor-us/v1",
            "snapshot-zero/v1",
            "partition-signature/v1"
        ],
        "error": "closed",
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
        let id = dedup_id(&event);
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
        let open = self.buf.take().ok_or("txn_commit")?;
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
        let open = self.buf.take().ok_or("txn_commit")?;
        if open.lsid != lsid || open.txn != txn {
            return Err("txn_commit");
        }
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

fn dedup_id(event: &Event) -> String {
    format!(
        "{}:{}:{}:{}:{}:{}",
        event.cluster,
        event.lsid.as_deref().unwrap_or(""),
        event.txn.map(|value| value.to_string()).unwrap_or_default(),
        event.ordinal,
        event.op,
        event.key.as_deref().unwrap_or("")
    )
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
    store.load_snapshot(snap_rows, snapshot_cluster);
    let mut resume = l0_token.clone();
    let mut ordinal = 0u32;
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
                ordinal = ordinal.saturating_add(1);
                if let Some(token) = doc.get_document("_id").ok() {
                    resume = token.to_owned();
                }
                spool(&config.control, &doc, ordinal)?;
                if let Some(event) = event_from(&doc, ordinal)? {
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

fn event_from(doc: &Document, ordinal: u32) -> Result<Option<Event>, String> {
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
    Ok(Some(Event {
        op: op.to_string(),
        key,
        doc: embedded(doc, "fullDocument"),
        pre: embedded(doc, "fullDocumentBeforeChange"),
        cluster: cluster_decimal(cluster_ts.time, cluster_ts.increment),
        lsid,
        txn,
        ordinal,
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
        &json!({
            "status": if expired { "POSITION_EXPIRED" } else { "not_expired" },
            "code": code,
            "code_name": name,
            "continued": continued,
            "resumed_from_latest": false,
            "message": message.chars().take(240).collect::<String>()
        }),
    )?;
    if !expired {
        return Err("stored resume token was not POSITION_EXPIRED".to_string());
    }
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

fn spool(control: &Path, doc: &Document, ordinal: u32) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(control.join("spool.jsonl"))
        .map_err(|err| err.to_string())?;
    let line = json!({
        "ordinal": ordinal,
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

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
