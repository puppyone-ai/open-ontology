//! MongoDB replica-set snapshots and exact post-image change streams.
//! The reader needs read, changeStream, local.oplog.rs read and replSetGetConfig.
//! A DBA must enable changeStreamPreAndPostImages; the reader never does so.

use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{Duration, Instant},
};

use ::mongodb::{
    Client,
    bson::{self, Bson, Document, Timestamp, doc},
    options::{ClientOptions, Credential, ServerAddress, Tls, TlsOptions},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::{
    SourceError,
    document::{self, Result, ScalarColumn, Stats},
    engine::*,
};

pub struct MongoConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub auth_database: String,
    pub ca_file: Option<PathBuf>,
    /// Exact collection scope and explicit top-level scalar projections.
    pub collections: BTreeMap<String, Vec<ScalarColumn>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Position {
    pub source_epoch: String,
    pub seconds: u32,
    pub increment: u32,
    pub sequence: u64,
    pub resume_token: Option<Value>,
}

impl Position {
    pub fn watermark(&self) -> Result<Watermark> {
        Ok(Watermark::Opaque {
            value: serde_json::to_string(self)
                .map_err(|_| SourceError::protocol("mongodb_position"))?,
        })
    }
    pub fn decode(watermark: &Watermark) -> Result<Self> {
        let Watermark::Opaque { value } = watermark else {
            return Err(SourceError::config("mongodb_position"));
        };
        serde_json::from_str(value).map_err(|_| SourceError::config("mongodb_position"))
    }
    pub fn at_or_after(a: &Watermark, b: &Watermark) -> Result<bool> {
        let (a, b) = (Self::decode(a)?, Self::decode(b)?);
        if a.source_epoch != b.source_epoch {
            return Err(SourceError::protocol("mongodb_rebaseline_required_epoch"));
        }
        Ok((a.seconds, a.increment, a.sequence) >= (b.seconds, b.increment, b.sequence))
    }
    fn timestamp(&self) -> Timestamp {
        Timestamp {
            time: self.seconds,
            increment: self.increment,
        }
    }
}

pub struct MongoEngine {
    client: Client,
    config: MongoConfig,
    budget: ReadBudget,
    snapshot: Option<Position>,
    plans: BTreeMap<TableRef, TableChunkPlan>,
    pub stats: Stats,
}

fn error(e: ::mongodb::error::Error) -> SourceError {
    use ::mongodb::error::ErrorKind;
    match *e.kind {
        ErrorKind::Command(ref c) if matches!(c.code, 136 | 286 | 280) => {
            SourceError::protocol("mongodb_rebaseline_required_resume")
        }
        ErrorKind::Command(ref c) if matches!(c.code, 239 | 246) => {
            SourceError::protocol("mongodb_rebaseline_required_snapshot")
        }
        ErrorKind::Command(ref c) if matches!(c.code, 13 | 18) => {
            SourceError::privilege("mongodb_reader_grants")
        }
        ErrorKind::Command(ref c) if c.code == 50 => SourceError::budget("mongodb_timeout"),
        _ => SourceError::protocol("mongodb_command"),
    }
}

fn ext(doc: &Document) -> Value {
    Bson::Document(doc.clone()).into_canonical_extjson()
}
fn key(b: &Bson) -> KeyValue {
    KeyValue::Text(b.clone().into_canonical_extjson().to_string())
}
fn bound(v: &[KeyValue]) -> Result<Bson> {
    let [KeyValue::Text(text)] = v else {
        return Err(SourceError::config("mongodb_bound"));
    };
    let json: Value =
        serde_json::from_str(text).map_err(|_| SourceError::config("mongodb_bound"))?;
    Bson::try_from(json).map_err(|_| SourceError::config("mongodb_bound"))
}
fn cursor(reply: &Document, batch: &str) -> Result<(i64, Vec<Document>)> {
    let c = reply
        .get_document("cursor")
        .map_err(|_| SourceError::protocol("mongodb_cursor"))?;
    let id = c
        .get_i64("id")
        .map_err(|_| SourceError::protocol("mongodb_cursor"))?;
    let rows = c
        .get_array(batch)
        .map_err(|_| SourceError::protocol("mongodb_cursor"))?
        .iter()
        .map(|v| {
            v.as_document()
                .cloned()
                .ok_or_else(|| SourceError::protocol("mongodb_document"))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((id, rows))
}

impl MongoEngine {
    pub fn new(
        config: MongoConfig,
        password: Zeroizing<String>,
        budget: ReadBudget,
    ) -> Result<Self> {
        document::validate_budget(&budget)?;
        if config.database.is_empty()
            || config.collections.is_empty()
            || config.collections.len() > 10_000
        {
            return Err(SourceError::config("mongodb_scope"));
        }
        for (name, cols) in &config.collections {
            if name.is_empty() || name.contains('\0') || name.starts_with("system.") {
                return Err(SourceError::config("mongodb_scope"));
            }
            document::validate_columns(cols)?;
        }
        if config.ca_file.is_none() && !matches!(config.host.as_str(), "127.0.0.1" | "::1") {
            return Err(SourceError::config("mongodb_tls_required"));
        }
        let mut opts = ClientOptions::builder()
            .hosts(vec![ServerAddress::Tcp {
                host: config.host.clone(),
                port: Some(config.port),
            }])
            .direct_connection(true)
            .max_pool_size(1)
            .min_pool_size(0)
            .server_selection_timeout(Duration::from_millis(budget.statement_timeout_ms))
            .connect_timeout(Duration::from_millis(budget.statement_timeout_ms))
            .credential(
                Credential::builder()
                    .username(config.user.clone())
                    .password(password.to_string())
                    .source(config.auth_database.clone())
                    .build(),
            )
            .build();
        if let Some(ca) = &config.ca_file {
            opts.tls = Some(Tls::Enabled(
                TlsOptions::builder().ca_file_path(ca.clone()).build(),
            ));
        }
        let client = Client::with_options(opts).map_err(error)?;
        Ok(Self {
            client,
            config,
            budget,
            snapshot: None,
            plans: BTreeMap::new(),
            stats: Stats::default(),
        })
    }

    async fn command(&mut self, db: &str, command: Document) -> Result<Document> {
        let started = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_millis(self.budget.statement_timeout_ms),
            self.client.database(db).run_command(command),
        )
        .await
        .map_err(|_| SourceError::budget("mongodb_timeout"))
        .and_then(|r| r.map_err(error));
        self.stats.pace(started, &self.budget).await;
        let reply = result?;
        let bytes = bson::to_vec(&reply)
            .map_err(|_| SourceError::protocol("mongodb_bson"))?
            .len() as u64;
        self.stats.bytes_received += bytes;
        if bytes > self.budget.max_chunk_bytes {
            return Err(SourceError::budget("mongodb_response_bytes"));
        }
        Ok(reply)
    }

    pub async fn source_epoch(&mut self) -> Result<String> {
        let reply = self
            .command("admin", doc! { "replSetGetConfig": 1 })
            .await?;
        let id = reply
            .get_document("config")
            .ok()
            .and_then(|d| d.get_document("settings").ok())
            .and_then(|d| d.get_object_id("replicaSetId").ok())
            .ok_or_else(|| SourceError::protocol("mongodb_replica_set_id_required"))?;
        let mut identity = vec![id.to_hex()];
        let db = self.config.database.clone();
        for name in self.config.collections.keys().cloned().collect::<Vec<_>>() {
            let r = self
                .command(
                    &db,
                    doc! { "listCollections": 1, "filter": { "name": &name },
                    "cursor": { "batchSize": 2 } },
                )
                .await?;
            let (cursor, docs) = cursor(&r, "firstBatch")?;
            if cursor != 0 || docs.len() != 1 {
                return Err(SourceError::catalog("mongodb_exact_collection_required"));
            }
            let uuid = docs[0]
                .get_document("info")
                .ok()
                .and_then(|i| i.get("uuid"))
                .ok_or_else(|| SourceError::catalog("mongodb_collection_uuid"))?;
            identity.push(format!(
                "{db}.{name}:{}",
                uuid.clone().into_canonical_extjson()
            ));
        }
        identity.push(
            serde_json::to_string(&self.config.collections)
                .map_err(|_| SourceError::config("mongodb_columns"))?,
        );
        Ok(format!(
            "mongodb:v1:{:x}",
            Sha256::digest(identity.join("\n"))
        ))
    }

    async fn position_now(&mut self) -> Result<Position> {
        let epoch = self.source_epoch().await?;
        let hello = self.command("admin", doc! { "hello": 1 }).await?;
        if hello.get_str("setName").is_err() {
            return Err(SourceError::config("mongodb_replica_set_required"));
        }
        let ts = hello
            .get_document("lastWrite")
            .ok()
            .and_then(|d| d.get_document("majorityOpTime").ok())
            .and_then(|d| d.get_timestamp("ts").ok())
            .ok_or_else(|| SourceError::protocol("mongodb_majority_time"))?;
        Ok(Position {
            source_epoch: epoch,
            seconds: ts.time,
            increment: ts.increment,
            sequence: 0,
            resume_token: None,
        })
    }

    fn table(&self, table: &TableRef) -> Result<()> {
        if table.schema != self.config.database
            || !self.config.collections.contains_key(&table.name)
        {
            return Err(SourceError::config("mongodb_table_scope"));
        }
        Ok(())
    }

    async fn kill(&mut self, db: &str, collection: &str, id: i64) {
        if id != 0 {
            let _ = self
                .command(db, doc! { "killCursors": collection, "cursors": [id] })
                .await;
        }
    }

    async fn rows(
        &mut self,
        table: &TableRef,
        lo: Option<&[KeyValue]>,
        hi: Option<&[KeyValue]>,
        projection: bool,
    ) -> Result<Vec<Document>> {
        self.table(table)?;
        let at = self
            .snapshot
            .as_ref()
            .ok_or_else(|| SourceError::config("mongodb_plan_required"))?
            .timestamp();
        let mut range = Document::new();
        if let Some(v) = lo {
            range.insert("$gte", bound(v)?);
        }
        if let Some(v) = hi {
            range.insert("$lt", bound(v)?);
        }
        let mut command = doc! { "find": &table.name,
        "filter": if range.is_empty() { doc! {} } else { doc! { "_id": range } },
        "sort": { "_id": 1 }, "collation": { "locale": "simple" },
        "hint": { "_id": 1 }, "batchSize": (self.budget.max_chunk_rows + 1) as i64,
        "limit": (self.budget.max_chunk_rows + 1) as i64,
        "maxTimeMS": self.budget.statement_timeout_ms as i64,
        "readConcern": { "level": "snapshot", "atClusterTime": at } };
        if projection {
            command.insert("projection", doc! { "_id": 1 });
        }
        let reply = self.command(&table.schema, command).await?;
        let (mut id, mut rows) = cursor(&reply, "firstBatch")?;
        let result = async {
            loop {
                let bytes = rows
                    .iter()
                    .map(|d| bson::to_vec(d).map(|b| b.len()).unwrap_or(usize::MAX))
                    .sum::<usize>();
                if rows.len() as u64 > self.budget.max_chunk_rows
                    || bytes as u64 > self.budget.max_chunk_bytes
                {
                    return Err(SourceError::budget("mongodb_chunk"));
                }
                if id == 0 {
                    break;
                }
                let reply = self
                    .command(
                        &table.schema,
                        doc! { "getMore": id, "collection": &table.name,
                        "batchSize": (self.budget.max_chunk_rows + 1 - rows.len() as u64) as i64 },
                    )
                    .await?;
                let (next, page) = cursor(&reply, "nextBatch")?;
                id = next;
                rows.extend(page);
            }
            Ok(rows)
        }
        .await;
        self.kill(&table.schema, &table.name, id).await;
        result
    }

    pub async fn close(self) {
        self.client.shutdown().await;
    }
}

impl SourceEngine for MongoEngine {
    fn family(&self) -> EngineFamily {
        EngineFamily::Mongodb
    }
    async fn discover_tables(&mut self, _: &ReadBudget) -> Result<Vec<TableRef>> {
        Ok(self
            .config
            .collections
            .keys()
            .map(|name| TableRef {
                schema: self.config.database.clone(),
                name: name.clone(),
            })
            .collect())
    }
    async fn probe(&mut self) -> Result<ProbeReport> {
        let r = self.command("admin", doc! { "buildInfo": 1 }).await?;
        let version = EngineVersion::parse(
            self.family(),
            r.get_str("version")
                .map_err(|_| SourceError::version("mongodb_version"))?,
        )?;
        if version.major != 8 {
            return Err(SourceError::version("mongodb_supported_major_8"));
        }
        let epoch = self.source_epoch().await?;
        Ok(ProbeReport { protocol: ENGINE_PROTOCOL.into(), family: self.family(), version,
            capabilities: [Capability::Catalog, Capability::ChunkedSnapshot, Capability::Watermark,
                Capability::DigestDiff].into_iter().map(|c| (c, CapabilityState::Available))
                .chain([(Capability::NativeChanges, CapabilityState::NeedsSetup { requirements: vec![
                    Requirement { kind: RequirementKind::Feature, name: "exact_post_images".into(),
                        observed: None, required: "changeStreamPreAndPostImages.enabled".into(),
                        dba_statement: Some("db.runCommand({collMod: '<collection>', changeStreamPreAndPostImages: {enabled: true}})".into()) }
                ] })]).collect(),
            settings: BTreeMap::from([("source_epoch".into(), epoch),
                ("consistency".into(), "per_collection_snapshot".into()),
                ("monitor_connections".into(), "driver_monitoring_in_addition_to_pool_max_1".into())]) })
    }
    async fn plan_chunks(
        &mut self,
        table: &TableRef,
        budget: &ReadBudget,
    ) -> Result<TableChunkPlan> {
        document::validate_budget(budget)?;
        self.table(table)?;
        self.budget = budget.clone();
        if self.snapshot.is_none() {
            self.snapshot = Some(self.position_now().await?);
        }
        // Key-only pages establish bounded, source-ordered ranges without OFFSET.
        // Mongo range predicates are type-bracketed. Mixed _id types must not
        // silently disappear after the first keyset page.
        let at = self.snapshot.as_ref().unwrap().timestamp();
        let types = self
            .command(
                &table.schema,
                doc! { "aggregate": &table.name,
                "pipeline": [{ "$group": { "_id": { "$type": "$_id" } } }],
                "cursor": { "batchSize": 32 },
                "maxTimeMS": budget.statement_timeout_ms as i64,
                "readConcern": { "level": "snapshot", "atClusterTime": at } },
            )
            .await?;
        let (id, kinds) = cursor(&types, "firstBatch")?;
        if id != 0
            || kinds.len() > 1
            || kinds.first().is_some_and(|d| {
                !matches!(d.get_str("_id"), Ok("string" | "objectId" | "long" | "int"))
            })
        {
            self.kill(&table.schema, &table.name, id).await;
            return Err(SourceError::config(
                "mongodb_homogeneous_scalar_id_required",
            ));
        }
        let mut lo: Option<Vec<KeyValue>> = None;
        let mut chunks = Vec::new();
        loop {
            let mut filter = doc! {};
            if let Some(v) = &lo {
                filter = doc! { "_id": { "$gte": bound(v)? } };
            }
            let at = self.snapshot.as_ref().unwrap().timestamp();
            let reply = self.command(&table.schema, doc! { "find": &table.name, "filter": filter,
                "projection": { "_id": 1 }, "sort": { "_id": 1 }, "hint": { "_id": 1 },
                "collation": { "locale": "simple" }, "singleBatch": true,
                "limit": (budget.max_chunk_rows + 1) as i64, "batchSize": (budget.max_chunk_rows + 1) as i64,
                "maxTimeMS": budget.statement_timeout_ms as i64,
                "readConcern": { "level": "snapshot", "atClusterTime": at } }).await?;
            let (_, keys) = cursor(&reply, "firstBatch")?;
            // singleBatch can end at the wire byte cap, so only an empty page
            // proves EOF. Keep the last key for the next inclusive range.
            let hi = if keys.len() >= 2 {
                Some(vec![key(keys
                    .last()
                    .unwrap()
                    .get("_id")
                    .ok_or_else(|| SourceError::protocol("mongodb_id"))?)])
            } else if keys.is_empty() {
                None
            } else {
                // A one-row tail fits; the snapshot is immutable.
                let id = keys[0]
                    .get("_id")
                    .ok_or_else(|| SourceError::protocol("mongodb_id"))?;
                let check = self
                    .command(
                        &table.schema,
                        doc! { "find": &table.name,
                        "filter": { "_id": { "$gt": id.clone() } }, "projection": { "_id": 1 },
                        "sort": { "_id": 1 }, "limit": 1, "singleBatch": true,
                        "readConcern": { "level": "snapshot", "atClusterTime": at } },
                    )
                    .await?;
                if !cursor(&check, "firstBatch")?.1.is_empty() {
                    return Err(SourceError::budget("mongodb_key_page"));
                }
                None
            };
            chunks.push(ChunkSpec {
                table: table.clone(),
                plan_revision: 1,
                chunk_id: chunks.len() as u32,
                lo: lo.clone(),
                hi: hi.clone(),
                estimated_rows: None,
            });
            if hi.is_none() {
                break;
            }
            if chunks.len() >= 100_000 || lo == hi {
                return Err(SourceError::budget("mongodb_plan"));
            }
            lo = hi;
        }
        let plan = TableChunkPlan {
            protocol: ENGINE_PROTOCOL.into(),
            table: table.clone(),
            key: KeyShape::StringPrimaryKey {
                column: "_id".into(),
                collation: Some("BSON simple".into()),
            },
            strategy: ChunkStrategy::Keyset {
                columns: vec!["_id".into()],
            },
            estimated_rows: None,
            chunks,
        };
        self.plans.insert(table.clone(), plan.clone());
        Ok(plan)
    }
    async fn read_chunk(
        &mut self,
        chunk: &ChunkSpec,
        budget: &ReadBudget,
        sink: &mut dyn ChunkSink,
    ) -> Result<ChunkReceipt> {
        document::validate_budget(budget)?;
        self.budget = budget.clone();
        if !self
            .plans
            .get(&chunk.table)
            .is_some_and(|p| p.chunks.contains(chunk))
        {
            return Err(SourceError::config("mongodb_unknown_chunk"));
        }
        let docs = self
            .rows(
                &chunk.table,
                chunk.lo.as_deref(),
                chunk.hi.as_deref(),
                false,
            )
            .await?;
        let rows = docs
            .iter()
            .map(|d| {
                Ok((
                    key(d
                        .get("_id")
                        .ok_or_else(|| SourceError::protocol("mongodb_id"))?),
                    ext(d),
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let rows: Vec<_> = rows
            .into_iter()
            .map(|(k, v)| match k {
                KeyValue::Text(s) => (s, v),
                _ => unreachable!(),
            })
            .collect();
        let batch = document::batch(&self.config.collections[&chunk.table.name], &rows)?;
        let (bytes, sha256) = document::ipc(&batch, budget)?;
        sink.push(batch)?;
        let mark = self.snapshot.as_ref().unwrap().watermark()?;
        Ok(ChunkReceipt {
            table: chunk.table.clone(),
            plan_revision: chunk.plan_revision,
            chunk_id: chunk.chunk_id,
            rows: rows.len() as u64,
            bytes,
            sha256,
            low: Some(mark.clone()),
            high: Some(mark),
        })
    }
    async fn watermark(&mut self) -> Result<Option<Watermark>> {
        Ok(Some(self.position_now().await?.watermark()?))
    }
    async fn digest_range(
        &mut self,
        table: &TableRef,
        lo: Option<&[KeyValue]>,
        hi: Option<&[KeyValue]>,
    ) -> Result<RangeDigest> {
        let rows = self.rows(table, lo, hi, false).await?;
        let rows = rows
            .iter()
            .map(|d| {
                Ok((
                    d.get("_id")
                        .ok_or_else(|| SourceError::protocol("mongodb_id"))?
                        .clone()
                        .into_canonical_extjson()
                        .to_string(),
                    ext(d),
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(RangeDigest {
            table: table.clone(),
            lo: lo.map(<[_]>::to_vec),
            hi: hi.map(<[_]>::to_vec),
            rows: rows.len() as u64,
            digest: document::digest(&rows),
        })
    }
}

impl ChangeSource for MongoEngine {
    async fn earliest_available(&mut self) -> Result<Watermark> {
        let epoch = self.source_epoch().await?;
        let r = self
            .command(
                "local",
                doc! { "find": "oplog.rs", "sort": { "$natural": 1 },
                "projection": { "ts": 1 }, "limit": 1, "singleBatch": true },
            )
            .await?;
        let (_, docs) = cursor(&r, "firstBatch")?;
        let ts = docs
            .first()
            .and_then(|d| d.get_timestamp("ts").ok())
            .ok_or_else(|| SourceError::protocol("mongodb_retention_unknown"))?;
        Position {
            source_epoch: epoch,
            seconds: ts.time,
            increment: ts.increment,
            sequence: 0,
            resume_token: None,
        }
        .watermark()
    }
    async fn next_batch(
        &mut self,
        from: &Watermark,
        budget: &ReadBudget,
    ) -> Result<Option<ChangeBatch>> {
        document::validate_budget(budget)?;
        self.budget = budget.clone();
        let mut position = Position::decode(from)?;
        if position.source_epoch != self.source_epoch().await? {
            return Err(SourceError::protocol("mongodb_rebaseline_required_epoch"));
        }
        if !Position::at_or_after(from, &self.earliest_available().await?)? {
            return Err(SourceError::protocol("mongodb_rebaseline_required_resume"));
        }
        let mut options = doc! { "fullDocument": "required" };
        if let Some(token) = &position.resume_token {
            let token = Bson::try_from(token.clone())
                .map_err(|_| SourceError::config("mongodb_resume_token"))?;
            options.insert("resumeAfter", token);
        } else {
            options.insert("startAtOperationTime", position.timestamp());
        }
        let db = self.config.database.clone();
        let names: Vec<_> = self.config.collections.keys().cloned().collect();
        let mut filter = doc! { "$or": [
            { "ns.coll": { "$in": names } },
            { "operationType": { "$in": ["invalidate", "dropDatabase"] } }
        ] };
        // startAtOperationTime is inclusive. Filter the already snapshotted
        // timestamp on the server, or a full baseline batch looks falsely idle.
        if position.resume_token.is_none() {
            filter.insert("clusterTime", doc! { "$gt": position.timestamp() });
        }
        let r = self
            .command(
                &db,
                doc! { "aggregate": 1, "pipeline": [
                    { "$changeStream": options },
                    { "$match": filter }
                ], "cursor": { "batchSize": budget.max_chunk_rows as i64 },
                    "maxTimeMS": budget.statement_timeout_ms as i64 },
            )
            .await?;
        let (mut id, mut docs) = cursor(&r, "firstBatch")?;
        let result = async {
            for _ in 0..3 {
                if !docs.is_empty() || id == 0 {
                    break;
                }
                let r = self
                    .command(
                        &db,
                        doc! { "getMore": id, "collection": "$cmd.aggregate",
                        "batchSize": budget.max_chunk_rows as i64,
                        "maxTimeMS": budget.statement_timeout_ms.min(1000) as i64 },
                    )
                    .await?;
                let (next, page) = cursor(&r, "nextBatch")?;
                id = next;
                docs = page;
            }
            let mut changes = Vec::new();
            let mut images = Vec::new();
            let mut bytes = 0;
            for event in docs {
                let ts = event
                    .get_timestamp("clusterTime")
                    .map_err(|_| SourceError::protocol("mongodb_event_time"))?;
                if position.resume_token.is_none() && ts <= position.timestamp() {
                    continue;
                }
                if ts < position.timestamp() {
                    return Err(SourceError::protocol("mongodb_resume_position_mismatch"));
                }
                let op = event
                    .get_str("operationType")
                    .map_err(|_| SourceError::protocol("mongodb_event_op"))?;
                if !matches!(op, "insert" | "update" | "replace" | "delete") {
                    return Err(SourceError::protocol("mongodb_rebaseline_required_ddl"));
                }
                let name = event
                    .get_document("ns")
                    .ok()
                    .and_then(|d| d.get_str("coll").ok())
                    .ok_or_else(|| SourceError::protocol("mongodb_event_scope"))?;
                let table = TableRef {
                    schema: db.clone(),
                    name: name.into(),
                };
                self.table(&table)?;
                let id = event
                    .get_document("documentKey")
                    .ok()
                    .and_then(|d| d.get("_id"))
                    .ok_or_else(|| SourceError::protocol("mongodb_event_key"))?;
                let row_key = key(id);
                let cols = &self.config.collections[name];
                let image = if op == "delete" {
                    document::batch(cols, &[])?
                } else {
                    let full = event
                        .get_document("fullDocument")
                        .map_err(|_| SourceError::protocol("mongodb_exact_image_required"))?;
                    document::batch(
                        cols,
                        &[(id.clone().into_canonical_extjson().to_string(), ext(full))],
                    )?
                };
                bytes += document::ipc(&image, budget)?.0;
                if bytes > budget.max_chunk_bytes {
                    return Err(SourceError::budget("mongodb_change_bytes"));
                }
                changes.push(match op {
                    "insert" => RowChange::Insert {
                        table,
                        key: vec![row_key],
                    },
                    "delete" => RowChange::Delete {
                        table,
                        key: vec![row_key],
                    },
                    _ => RowChange::Update {
                        table,
                        key: vec![row_key],
                        old_key: None,
                    },
                });
                images.push(image);
                position.seconds = ts.time;
                position.increment = ts.increment;
                position.sequence = position
                    .sequence
                    .checked_add(1)
                    .ok_or_else(|| SourceError::protocol("mongodb_sequence"))?;
                position.resume_token = Some(ext(event
                    .get_document("_id")
                    .map_err(|_| SourceError::protocol("mongodb_resume_token"))?));
            }
            if changes.is_empty() {
                return Ok(None);
            }
            Ok(Some(ChangeBatch {
                position: position.watermark()?,
                changes,
                images,
            }))
        }
        .await;
        self.kill(&db, "$cmd.aggregate", id).await;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn position_roundtrip_epoch_and_same_cluster_time_order() {
        let mut p = Position {
            source_epoch: "replica-a".into(),
            seconds: 10,
            increment: 2,
            sequence: 1,
            resume_token: Some(serde_json::json!({"_data":"opaque"})),
        };
        let first = p.watermark().unwrap();
        assert_eq!(Position::decode(&first).unwrap(), p);
        p.sequence += 1;
        assert!(Position::at_or_after(&p.watermark().unwrap(), &first).unwrap());
        assert!(!Position::at_or_after(&first, &p.watermark().unwrap()).unwrap());
        p.source_epoch = "replica-b".into();
        assert!(Position::at_or_after(&first, &p.watermark().unwrap()).is_err());
    }
    #[test]
    fn bson_bounds_preserve_id_types() {
        for id in [
            Bson::Int64(i64::MAX),
            Bson::String("x".into()),
            Bson::ObjectId(bson::oid::ObjectId::new()),
        ] {
            assert_eq!(bound(&[key(&id)]).unwrap(), id);
        }
        assert!(bound(&[KeyValue::Int(1)]).is_err());
    }
}
