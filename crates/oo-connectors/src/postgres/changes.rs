//! Read committed pgoutput transactions without consuming or creating a slot.
//! The durable coordinator owns acknowledgement; replay is deliberately safe.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use arrow_array::RecordBatch;
use sha2::{Digest, Sha256};
use sqlx::Row;

use super::{PostgresEngine, duty, guards, lsn, position, sqltext, validate_budget, version};
use crate::engine::*;
use crate::session::{self, cell_bool, cell_i64, cell_opt_string, cell_string};
use crate::types::{MapDecision, RunBinding, assemble_batch, map_column};
use crate::{PgSession, PgSourceConfig, SourceError};

// Reuse the target's authoritative parser rather than introduce a third wire
// decoder. This module has no dependency on the pipeline crate or its runtime.
#[allow(dead_code)]
#[path = "../../../oo-pipeline/src/pg_current_capture.rs"]
mod wire;
use wire::{Cell, Message, Relation};

#[derive(Clone, Debug)]
pub struct CaptureConfig {
    pub slot: String,
    pub publication: String,
    /// Measured WAL production for cycle + outage, in bytes. Not a time claim.
    pub required_wal_headroom: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SlotStatus {
    pub protocol_version: u32,
    pub restart_lsn: u64,
    pub confirmed_flush_lsn: u64,
    pub safe_wal_size: Option<u64>,
    pub publish_via_partition_root: bool,
}

/// Only a consumer confirms (moves `confirmed_flush_lsn`). Decoding itself,
/// even a peek, may advance `restart_lsn` when it passes a running-xacts record
/// at or before the confirmed point (LogicalIncreaseRestartDecodingForSlot), so
/// a forward `restart_lsn` up to the unchanged confirmed point is not foreign.
fn slot_unchanged_by_others(before: &SlotStatus, after: &SlotStatus) -> bool {
    after.confirmed_flush_lsn == before.confirmed_flush_lsn
        && after.restart_lsn >= before.restart_lsn
        && after.restart_lsn <= after.confirmed_flush_lsn.max(before.restart_lsn)
}

impl SlotStatus {
    pub fn earliest(&self) -> u64 {
        self.restart_lsn.max(self.confirmed_flush_lsn)
    }

    pub fn require_position(&self, from: u64) -> Result<(), SourceError> {
        if from < self.earliest() {
            return Err(SourceError::protocol(
                "rebaseline_required_position_expired",
            ));
        }
        Ok(())
    }
}

/// A separate session owner, so capture can start before snapshot chunk reads.
pub struct PostgresChanges {
    engine: PostgresEngine,
    config: CaptureConfig,
    quarantined: BTreeSet<TableRef>,
    table_positions: BTreeMap<TableRef, u64>,
    projections: BTreeMap<TableRef, Vec<String>>,
}

impl PostgresChanges {
    pub fn new(source: PgSourceConfig, config: CaptureConfig) -> Result<Self, SourceError> {
        if !name(&config.slot) || !name(&config.publication) || config.required_wal_headroom == 0 {
            return Err(SourceError::config("capture_config"));
        }
        Ok(Self {
            engine: PostgresEngine::new(source),
            config,
            quarantined: BTreeSet::new(),
            table_positions: BTreeMap::new(),
            projections: BTreeMap::new(),
        })
    }

    /// Call only after persisting the table fence. The coordinator must retain
    /// source history at the frozen table position, not acknowledge this skip.
    pub fn quarantine(&mut self, tables: BTreeSet<TableRef>) {
        self.quarantined = tables;
    }

    /// Aligned table positions can lead the shared cursor after rebaseline.
    pub fn table_positions(&mut self, positions: BTreeMap<TableRef, u64>) {
        self.table_positions = positions;
    }
    /// Supplied only after native classification against the sealed catalog.
    pub fn projections(&mut self, projections: BTreeMap<TableRef, Vec<String>>) {
        self.projections = projections;
    }

    pub async fn preflight(&mut self) -> Result<SlotStatus, SourceError> {
        let mut s = self.engine.connect().await?;
        self.inspect(&mut s).await
    }

    async fn inspect(&self, s: &mut PgSession) -> Result<SlotStatus, SourceError> {
        if s.major < 13 {
            return Err(SourceError::version("capture_version"));
        }
        let role = session::fetch_control(
            s,
            sqlx::query(
                "SELECT rolreplication AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole, \
             current_setting('wal_level')='logical', current_setting('transaction_read_only')='on' \
             FROM pg_roles WHERE rolname=current_user",
            ),
        )
        .await?;
        if !cell_bool(&role, 0)? || !cell_bool(&role, 1)? || !cell_bool(&role, 2)? {
            return Err(SourceError::privilege("capture_role_or_wal_level"));
        }
        // JSON projection avoids mentioning catalog columns absent on old majors.
        let slot = session::fetch_control(s, sqlx::query(
            "SELECT count(*)::int8, min(plugin)::text, bool_and(database=current_database()), \
             bool_or(active OR temporary), min(restart_lsn::text), min(confirmed_flush_lsn::text), \
             min(wal_status), min(safe_wal_size)::int8, \
             min(to_jsonb(s)->>'invalidation_reason'), bool_or(COALESCE((to_jsonb(s)->>'two_phase')::bool,false)) \
             FROM pg_replication_slots s WHERE slot_name=$1"
        ).bind(&self.config.slot)).await?;
        if cell_i64(&slot, 0)? != 1 {
            return Err(SourceError::protocol("rebaseline_required_slot_missing"));
        }
        if cell_opt_string(&slot, 8)?.is_some()
            || !matches!(
                cell_opt_string(&slot, 6)?.as_deref(),
                Some("reserved" | "extended")
            )
        {
            return Err(SourceError::protocol(
                "rebaseline_required_slot_invalidated",
            ));
        }
        if cell_opt_string(&slot, 1)?.as_deref() != Some("pgoutput")
            || !cell_bool(&slot, 2)?
            || cell_bool(&slot, 3)?
            || cell_bool(&slot, 9)?
        {
            return Err(SourceError::protocol("slot_unavailable"));
        }
        let restart = cell_opt_string(&slot, 4)?
            .ok_or_else(|| SourceError::protocol("rebaseline_required_slot_position"))?;
        let confirmed = cell_opt_string(&slot, 5)?
            .ok_or_else(|| SourceError::protocol("rebaseline_required_slot_position"))?;
        let safe: Option<i64> = slot
            .try_get(7)
            .map_err(|_| SourceError::protocol("slot_headroom"))?;
        // Unlimited retention is not evidence of available source disk.
        if safe.is_none_or(|n| n < 0 || (n as u64) < self.config.required_wal_headroom) {
            return Err(SourceError::budget("wal_retention_headroom_unproven"));
        }
        let publication = session::fetch_control(
            s,
            sqlx::query(
                "SELECT count(*)::int8, bool_and(pubinsert AND pubupdate AND pubdelete), \
             bool_or(pubviaroot), bool_or(pg_get_userbyid(pubowner)=current_user), bool_or(puballtables) \
             FROM pg_publication WHERE pubname=$1",
            )
            .bind(&self.config.publication),
        )
        .await?;
        if cell_i64(&publication, 0)? != 1
            || !cell_bool(&publication, 1)?
            || cell_bool(&publication, 3)?
        {
            return Err(SourceError::privilege("publication_missing_or_incomplete"));
        }
        // pg_publication_tables opens every relation, including a quarantined
        // one under an exclusive lock. Explicit members can be proven from
        // catalogs without opening those relations.
        if !self.quarantined.is_empty() {
            if cell_bool(&publication, 4)? {
                return Err(SourceError::config(
                    "quarantine_requires_explicit_publication",
                ));
            }
            if s.major >= 15 {
                let schemas = session::fetch_control(
                    s,
                    sqlx::query(
                        "SELECT count(*)::int8 FROM pg_publication_namespace n
                     JOIN pg_publication p ON p.oid=n.pnpubid WHERE p.pubname=$1",
                    )
                    .bind(&self.config.publication),
                )
                .await?;
                if cell_i64(&schemas, 0)? != 0 {
                    return Err(SourceError::config(
                        "quarantine_requires_explicit_publication",
                    ));
                }
            }
        }
        let members = if self.quarantined.is_empty() {
            "SELECT schemaname::text, tablename::text, to_jsonb(p)->>'rowfilter' \
             FROM pg_publication_tables p WHERE pubname=$1"
        } else {
            "SELECT n.nspname::text,c.relname::text,
             CASE WHEN c.relkind='p' AND NOT p.pubviaroot THEN 'partition_leaf_scope'
                  ELSE to_jsonb(r)->>'prqual' END
             FROM pg_publication_rel r JOIN pg_publication p ON p.oid=r.prpubid
             JOIN pg_class c ON c.oid=r.prrelid JOIN pg_namespace n ON n.oid=c.relnamespace
             WHERE p.pubname=$1"
        };
        let rows =
            session::fetch_all(s, sqlx::query(members).bind(&self.config.publication)).await?;
        let mut actual = BTreeSet::new();
        for r in rows {
            if cell_opt_string(&r, 2)?.is_some() {
                return Err(SourceError::protocol("publication_row_filter"));
            }
            actual.insert((cell_string(&r, 0)?, cell_string(&r, 1)?));
        }
        let expected: BTreeSet<_> = self
            .engine
            .connector
            .source_config()
            .selection()
            .tables()
            .iter()
            .map(|t| (t.schema().to_owned(), t.table().to_owned()))
            .collect();
        // pubviaroot=true resolves to roots in this catalog; false resolves to
        // leaves. Never silently relabel leaf events as parent events.
        if actual != expected {
            return Err(SourceError::protocol(
                "publication_members_or_partition_roots",
            ));
        }
        for (schema, table) in expected {
            if self.quarantined.contains(&TableRef {
                schema: schema.clone(),
                name: table.clone(),
            }) {
                continue;
            }
            let facts = session::fetch_control(s, sqlx::query(
                "SELECT has_table_privilege(c.oid,'SELECT'), \
                 has_table_privilege(c.oid,'INSERT,UPDATE,DELETE,TRUNCATE,TRIGGER') \
                   OR pg_get_userbyid(c.relowner)=current_user \
                   OR EXISTS(SELECT 1 FROM pg_attribute a WHERE a.attrelid=c.oid AND a.attnum>0 \
                     AND NOT a.attisdropped AND has_column_privilege(c.oid,a.attnum,'INSERT,UPDATE')), \
                 c.relrowsecurity OR c.relforcerowsecurity, c.relreplident::text, \
                 EXISTS(SELECT 1 FROM pg_index i WHERE i.indrelid=c.oid AND i.indisprimary), \
                 EXISTS(SELECT 1 FROM pg_publication_rel r JOIN pg_publication p ON p.oid=r.prpubid \
                   WHERE r.prrelid=c.oid AND p.pubname=$3 AND to_jsonb(r)->>'prattrs' IS NOT NULL) \
                 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace \
                 WHERE n.nspname=$1 AND c.relname=$2"
            ).bind(schema).bind(table).bind(&self.config.publication)).await?;
            if !cell_bool(&facts, 0)? || cell_bool(&facts, 1)? || cell_bool(&facts, 2)? {
                return Err(SourceError::privilege("capture_table_privileges"));
            }
            if !cell_bool(&facts, 4)?
                || !matches!(cell_string(&facts, 3)?.as_str(), "d" | "f")
                || cell_bool(&facts, 5)?
            {
                return Err(SourceError::protocol(
                    "capture_requires_full_columns_and_primary_key",
                ));
            }
        }
        Ok(SlotStatus {
            protocol_version: version::pgoutput_proto_version(s.major as u32),
            restart_lsn: lsn::parse_lsn(&restart)?,
            confirmed_flush_lsn: lsn::parse_lsn(&confirmed)?,
            safe_wal_size: safe.map(|v| v as u64),
            publish_via_partition_root: cell_bool(&publication, 2)?,
        })
    }

    async fn read(
        &mut self,
        s: &mut PgSession,
        from: u64,
        budget: &ReadBudget,
    ) -> Result<Option<ChangeBatch>, SourceError> {
        let status = self.inspect(s).await?;
        status.require_position(from)?;
        let Some(Watermark::PgLsn { lsn: upper }) = position(s).await? else {
            return Err(SourceError::protocol("wal_position"));
        };
        if from > upper {
            return Err(SourceError::protocol("rebaseline_required_future_position"));
        }
        let cap = budget.max_chunk_bytes as u32;
        s.conn
            .set_incoming_bounds(Some(
                sqlx::postgres::PgIncomingBounds::try_new(cap.saturating_add(8192), cap, cap, 1600)
                    .map_err(|_| SourceError::budget("capture_incoming_bounds"))?,
            ))
            .map_err(crate::error::map_sqlx)?;
        // v2-v4 are negotiated, but streaming/two_phase stay off. Their ordinary
        // committed transaction messages retain the v1 framing used by the target.
        // A peek materializes everything it decodes before returning a row, so
        // `upto_nchanges` bounds the source work: the server stops after the
        // transaction that reaches the limit instead of decoding the whole
        // backlog up to `upper` on every batch.
        let target = budget.max_chunk_rows.clamp(1, BATCH_CHANGES);
        let mut limit = decode_limit(target);
        let collected = loop {
            let mut collect = Collect::new(from, target, budget);
            let mut stream = sqlx::query(
                "SELECT data FROM pg_logical_slot_peek_binary_changes($1,$2::text::pg_lsn,$3::int4, \
                 'proto_version',$4,'publication_names',$5)",
            )
            .bind(&self.config.slot)
            .bind(lsn::format_lsn(upper))
            .bind(i32::try_from(limit).map_err(|_| SourceError::budget("capture_decode_limit"))?)
            .bind(status.protocol_version.to_string())
            .bind(format!("\"{}\"", self.config.publication))
            .fetch(&mut s.conn);
            while let Some(row) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
                let row = row.map_err(crate::error::map_sqlx)?;
                let data: Vec<u8> = row
                    .try_get(0)
                    .map_err(|_| SourceError::protocol("pgoutput_data"))?;
                if collect.received(data.len() as u64)? == Flow::Stop {
                    break;
                }
                let flow = match wire::decode(&data).map_err(SourceError::protocol)? {
                    Message::Begin { final_lsn, .. } => collect.begin(final_lsn)?,
                    Message::Relation(r) => {
                        collect.relations.insert(r.oid, std::sync::Arc::new(r));
                        Flow::Continue
                    }
                    Message::Event {
                        kind,
                        oid,
                        old,
                        new,
                    } => {
                        let relation = collect.relations.get(&oid).cloned();
                        collect.event(|| {
                            Ok((
                                kind,
                                relation.ok_or_else(|| SourceError::protocol("pg_relation"))?,
                                old,
                                new,
                            ))
                        })?
                    }
                    Message::Commit { commit, end, .. } => collect.commit(commit, end)?,
                };
                if flow == Flow::Stop {
                    break;
                }
            }
            drop(stream);
            match collect.finish(limit)? {
                Collected::Done(done) => break done,
                // Only transactions this cursor skips filled the decode limit.
                Collected::Retry if limit < MAX_DECODE_LIMIT => {
                    limit = (limit * 4).min(MAX_DECODE_LIMIT);
                }
                Collected::Retry => return Err(SourceError::budget("capture_decode_limit")),
            }
        };
        let Some((end, events)) = collected else {
            return Ok(None);
        };
        let catalog = s.collect_catalog().await?;
        let mut changes = Vec::new();
        let mut pending: Vec<PendingImage> = Vec::new();
        let mut checked_relations = BTreeSet::new();
        for (kind, relation, mut old, mut new) in events {
            let table = TableRef {
                schema: relation.schema.clone(),
                name: relation.table.clone(),
            };
            self.engine.selected(&table)?;
            if self.quarantined.contains(&table)
                || self
                    .table_positions
                    .get(&table)
                    .is_some_and(|position| *position >= end)
            {
                continue;
            }
            if checked_relations.insert(relation.oid) {
                let current = session::fetch_control(s, sqlx::query(
                    "SELECT c.oid::int8 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace \
                     WHERE n.nspname=$1 AND c.relname=$2"
                ).bind(&table.schema).bind(&table.name)).await?;
                // Equal column shapes do not make a dropped/recreated table
                // the same replication object.
                if cell_i64(&current, 0)? != i64::from(relation.oid) {
                    return Err(SourceError::catalog(
                        "rebaseline_required_relation_identity",
                    ));
                }
            }
            let mut shape = catalog
                .tables
                .iter()
                .find(|t| t.schema == table.schema && t.name == table.name)
                .ok_or_else(|| SourceError::catalog("pg_relation_drift"))?
                .clone();
            let mut relation = (*relation).clone();
            if let Some(names) = self.projections.get(&table) {
                let indexes = names
                    .iter()
                    .map(|name| {
                        relation
                            .columns
                            .iter()
                            .position(|c| &c.0 == name)
                            .ok_or_else(|| {
                                SourceError::catalog("rebaseline_required_relation_drift")
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let project = |cells: Vec<Cell>| {
                    indexes
                        .iter()
                        .map(|i| {
                            cells
                                .get(*i)
                                .cloned()
                                .ok_or_else(|| SourceError::protocol("pg_tuple_shape"))
                        })
                        .collect()
                };
                old = old.map(project).transpose()?;
                new = new.map(project).transpose()?;
                relation.columns = indexes
                    .iter()
                    .map(|i| relation.columns[*i].clone())
                    .collect();
                shape.columns = names
                    .iter()
                    .map(|name| {
                        shape
                            .columns
                            .iter()
                            .find(|c| &c.name == name)
                            .cloned()
                            .ok_or_else(|| {
                                SourceError::catalog("rebaseline_required_relation_drift")
                            })
                    })
                    .collect::<Result<_, _>>()?;
            }
            if shape.columns.len() != relation.columns.len()
                || shape.columns.iter().zip(&relation.columns).any(|(a, b)| {
                    a.name != b.0
                        || a.type_oid != i64::from(b.1)
                        || (a.typmod != b.2
                            && !(self.projections.contains_key(&table) && a.type_oid == 1043))
                })
            {
                return Err(SourceError::catalog("rebaseline_required_relation_drift"));
            }
            let key_names: Vec<_> = shape.primary_key.iter().map(|c| c.name.as_str()).collect();
            let old_key = old
                .as_ref()
                .map(|v| keys(&relation, v, &key_names))
                .transpose()?;
            let key = match &new {
                Some(v) => keys(&relation, v, &key_names)?,
                None => old_key
                    .clone()
                    .ok_or_else(|| SourceError::protocol("pg_old_key"))?,
            };
            // One image per change, including a key-only image for a delete.
            let cells = new
                .as_ref()
                .or(old.as_ref())
                .ok_or_else(|| SourceError::protocol("pg_tuple"))?;
            if cells.len() != shape.columns.len() {
                return Err(SourceError::protocol("pg_tuple_shape"));
            }
            pending.push(PendingImage {
                cells: cells.clone(),
                delete: kind == b'D',
                shape,
                relation,
            });
            changes.push(match kind {
                b'I' => RowChange::Insert { table, key },
                b'U' => RowChange::Update {
                    table,
                    key,
                    old_key,
                },
                b'D' => RowChange::Delete { table, key },
                _ => return Err(SourceError::protocol("pg_event")),
            });
        }
        // Convert each run of one relation's logged values in few statements.
        let mut images = Vec::with_capacity(pending.len());
        let mut image_bytes = 0u64;
        let mut start = 0;
        while start < pending.len() {
            let first = &pending[start];
            let run = pending[start..]
                .iter()
                .take_while(|p| {
                    p.relation.oid == first.relation.oid
                        && p.relation.columns == first.relation.columns
                        && p.shape == first.shape
                })
                .count();
            for image in
                images_for_run(s, &pending[start..start + run], &catalog.schema_hash).await?
            {
                image_bytes = image_bytes.saturating_add(image.get_array_memory_size() as u64);
                if image_bytes > budget.max_chunk_bytes {
                    return Err(SourceError::budget("capture_image_bytes"));
                }
                images.push(image);
            }
            start += run;
        }
        if images.len() != changes.len() {
            return Err(SourceError::protocol("capture_image_count"));
        }
        // A concurrent external consumer must not advance/invalidate our slot.
        let after = self.inspect(s).await?;
        if !slot_unchanged_by_others(&status, &after) {
            return Err(SourceError::protocol("slot_changed_during_capture"));
        }
        Ok(Some(ChangeBatch {
            position: Watermark::PgLsn { lsn: end },
            changes,
            images,
        }))
    }
}

fn name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

fn keys(r: &Relation, cells: &[Cell], names: &[&str]) -> Result<Vec<KeyValue>, SourceError> {
    if cells.len() != r.columns.len() {
        return Err(SourceError::protocol("pg_tuple_shape"));
    }
    names
        .iter()
        .map(|name| {
            let i = r
                .columns
                .iter()
                .position(|c| c.0 == *name)
                .ok_or_else(|| SourceError::protocol("pg_key_column"))?;
            let Cell::Text(text) = &cells[i] else {
                return Err(SourceError::protocol("pg_key_missing"));
            };
            sqltext::KeyClass::from_oid(i64::from(r.columns[i].1)).value(Some(text.clone()))
        })
        .collect()
}

/// Changes per capture batch. Many small source transactions share one
/// bounded decode, a few image statements and one target commit; a single
/// transaction still has to fit `max_chunk_rows` on its own.
const BATCH_CHANGES: u64 = 4096;
/// pgoutput rows per peek beyond which a skipped prefix forces a retry.
const MAX_DECODE_LIMIT: u64 = 1 << 22;
/// Rows converted per image statement (parameters stay below 65,535).
const IMAGE_ROWS: usize = 512;

/// Each one-change transaction is Begin, the change and Commit (plus one
/// Relation per relation), so three rows per change bound a full batch.
fn decode_limit(target: u64) -> u64 {
    target
        .saturating_mul(3)
        .saturating_add(64)
        .min(MAX_DECODE_LIMIT)
}

#[derive(Debug, PartialEq, Eq)]
enum Flow {
    Continue,
    Stop,
}

enum Collected<T> {
    Done(Option<(u64, Vec<T>)>),
    Retry,
}

/// Groups decoded pgoutput messages into whole committed transactions after
/// `from`. It stops at a commit once the batch reaches its change or byte
/// target, and before a transaction that would overflow the batch; the
/// discarded transaction is decoded again by the next batch.
struct Collect<T> {
    from: u64,
    target: u64,
    max_rows: u64,
    max_bytes: u64,
    bytes: u64,
    rows: u64,
    open: Option<u64>,
    pending: Vec<T>,
    events: Vec<T>,
    completed: Option<u64>,
    stopped: bool,
    relations: BTreeMap<u32, std::sync::Arc<Relation>>,
}

impl<T> Collect<T> {
    fn new(from: u64, target: u64, budget: &ReadBudget) -> Self {
        Self {
            from,
            target,
            max_rows: budget.max_chunk_rows,
            max_bytes: budget.max_chunk_bytes,
            bytes: 0,
            rows: 0,
            open: None,
            pending: Vec::new(),
            events: Vec::new(),
            completed: None,
            stopped: false,
            relations: BTreeMap::new(),
        }
    }

    fn stop(&mut self) -> Flow {
        self.stopped = true;
        Flow::Stop
    }

    fn received(&mut self, bytes: u64) -> Result<Flow, SourceError> {
        self.rows += 1;
        self.bytes = self.bytes.saturating_add(bytes);
        if self.bytes > self.max_bytes {
            if self.completed.is_some() {
                return Ok(self.stop());
            }
            return Err(SourceError::budget("capture_replay_bytes"));
        }
        Ok(Flow::Continue)
    }

    fn begin(&mut self, final_lsn: u64) -> Result<Flow, SourceError> {
        if self.open.replace(final_lsn).is_some() {
            return Err(SourceError::protocol("pg_nested_transaction"));
        }
        self.pending.clear();
        Ok(Flow::Continue)
    }

    fn event(
        &mut self,
        item: impl FnOnce() -> Result<T, SourceError>,
    ) -> Result<Flow, SourceError> {
        let final_lsn = self
            .open
            .ok_or_else(|| SourceError::protocol("pg_event_boundary"))?;
        if final_lsn < self.from {
            return Ok(Flow::Continue);
        }
        if self.pending.len() as u64 >= self.max_rows {
            return Err(SourceError::budget("capture_transaction_rows"));
        }
        if self.completed.is_some()
            && (self.events.len() + self.pending.len()) as u64 >= self.max_rows
        {
            return Ok(self.stop());
        }
        self.pending.push(item()?);
        Ok(Flow::Continue)
    }

    fn commit(&mut self, commit: u64, end: u64) -> Result<Flow, SourceError> {
        if self.open.take() != Some(commit) || end <= commit {
            return Err(SourceError::protocol("pg_commit_boundary"));
        }
        if end > self.from && !self.pending.is_empty() {
            self.events.append(&mut self.pending);
            self.completed = Some(end);
            if self.events.len() as u64 >= self.target || self.bytes >= self.max_bytes / 2 {
                return Ok(self.stop());
            }
        }
        self.pending.clear();
        Ok(Flow::Continue)
    }

    fn finish(self, limit: u64) -> Result<Collected<T>, SourceError> {
        // The server emits whole transactions, so only a deliberate stop may
        // leave one open.
        if self.open.is_some() && !self.stopped {
            return Err(SourceError::protocol("pg_incomplete_transaction"));
        }
        match self.completed {
            Some(end) => Ok(Collected::Done(Some((end, self.events)))),
            // The server stops after the transaction that reaches the limit.
            None if self.rows >= limit && !self.stopped => Ok(Collected::Retry),
            None => Ok(Collected::Done(None)),
        }
    }
}

struct PendingImage {
    cells: Vec<Cell>,
    delete: bool,
    shape: crate::SourceTable,
    relation: Relation,
}

/// One statement converts up to IMAGE_ROWS logged tuples of one relation:
/// `VALUES` keeps an ordinal so the result order is the change order.
fn image_sql(types: &[String], rows: usize) -> String {
    let width = types.len() + 1;
    let values = (0..rows)
        .map(|row| {
            let cells = (0..width)
                .map(|col| {
                    let n = row * width + col + 1;
                    if col == 0 {
                        format!("${n}::int4")
                    } else {
                        format!("${n}::text")
                    }
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("({cells})")
        })
        .collect::<Vec<_>>()
        .join(",");
    let names = (0..types.len())
        .map(|i| format!("c{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let projections = types
        .iter()
        .enumerate()
        .map(|(i, ty)| format!("CAST(v.c{i} AS {ty})"))
        .collect::<Vec<_>>()
        .join(",");
    format!("SELECT {projections} FROM (VALUES {values}) AS v(o,{names}) ORDER BY v.o")
}

async fn images_for_run(
    s: &mut PgSession,
    run: &[PendingImage],
    schema_hash: &str,
) -> Result<Vec<RecordBatch>, SourceError> {
    let Some(first) = run.first() else {
        return Ok(Vec::new());
    };
    let (shape, r) = (&first.shape, &first.relation);
    let graph = s.load_type_graph(&shape.columns).await?;
    let keys: Vec<_> = shape.primary_key.iter().map(|c| c.attnum).collect();
    let plans: Vec<_> = shape
        .columns
        .iter()
        .map(|c| map_column(c, &graph, keys.contains(&c.attnum)))
        .collect();
    if plans
        .iter()
        .any(|p| p.mapping.decision != MapDecision::Lossless)
    {
        return Err(SourceError::protocol("capture_unsupported_column"));
    }
    let mut types = Vec::new();
    for c in &shape.columns {
        let ty = session::fetch_control(
            s,
            sqlx::query("SELECT format_type($1::bigint::oid,$2)")
                .bind(c.type_oid)
                .bind(c.typmod),
        )
        .await?;
        let ty = cell_string(&ty, 0)?;
        if !sqltext::valid_cast_type(&ty) {
            return Err(SourceError::protocol("pg_cast_type"));
        }
        types.push(ty);
    }
    let binding = RunBinding {
        source_id: s.source_id.clone(),
        schema: r.schema.clone(),
        table: r.table.clone(),
        run_generation: 0,
        input_digest: Sha256::digest(schema_hash.as_bytes()).into(),
    };
    let mut delete_plans = plans.clone();
    // Deletes may omit non-key NOT NULL columns. Their image is not an upsert.
    for p in &mut delete_plans {
        p.mapping.nullable = true;
    }
    let mut images = Vec::with_capacity(run.len());
    let per = IMAGE_ROWS.min(60_000 / (types.len() + 1)).max(1);
    for part in run.chunks(per) {
        let sql = image_sql(&types, part.len());
        let mut q = sqlx::query(sqlx_core::sql_str::AssertSqlSafe(sql));
        for (ordinal, pending) in part.iter().enumerate() {
            if pending.cells.len() != shape.columns.len() {
                return Err(SourceError::protocol("pg_tuple_shape"));
            }
            q = q.bind(ordinal as i32);
            for cell in &pending.cells {
                q = q.bind(match cell {
                    Cell::Text(v) => Some(v.clone()),
                    Cell::Null => None,
                    Cell::Unchanged => {
                        return Err(SourceError::protocol("rebaseline_required_unchanged_toast"));
                    }
                });
            }
        }
        // Convert the logged values, never re-read a later source row.
        let rows = session::fetch_all(s, q).await?;
        if rows.len() != part.len() {
            return Err(SourceError::protocol("capture_image_rows"));
        }
        for (row, pending) in rows.iter().zip(part) {
            let (decoded, _) = crate::bounded_read::decode_row(&plans, row)?;
            let plans = if pending.delete {
                delete_plans.clone()
            } else {
                plans.clone()
            };
            let batch = assemble_batch(&binding, plans, &keys, vec![decoded])?;
            if !batch.rejects.is_empty() {
                return Err(SourceError::protocol("capture_rejected_cell"));
            }
            images.push(batch.record_batch()?);
        }
    }
    Ok(images)
}

impl ChangeSource for PostgresChanges {
    async fn earliest_available(&mut self) -> Result<Watermark, SourceError> {
        Ok(Watermark::PgLsn {
            lsn: self.preflight().await?.earliest(),
        })
    }

    async fn next_batch(
        &mut self,
        from: &Watermark,
        budget: &ReadBudget,
    ) -> Result<Option<ChangeBatch>, SourceError> {
        validate_budget(budget)?;
        let Watermark::PgLsn { lsn } = from else {
            return Err(SourceError::protocol("pg_position_kind"));
        };
        let mut s = self.engine.connect().await?;
        guards(&mut s, budget).await?;
        self.engine.protect(&mut s).await?;
        let start = Instant::now();
        let result = tokio::time::timeout(
            s.budget.statement_timeout(),
            self.read(&mut s, *lsn, budget),
        )
        .await
        .map_err(|_| SourceError::budget("capture_timeout"))
        .and_then(|v| v);
        // Always discard the socket; a bounded/failed read may leave wire data.
        drop(s);
        duty(start, budget.duty_cycle_percent).await;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_restart_advance_during_peek_is_not_a_foreign_consumer() {
        let slot = |restart_lsn, confirmed_flush_lsn| SlotStatus {
            protocol_version: 1,
            restart_lsn,
            confirmed_flush_lsn,
            safe_wal_size: None,
            publish_via_partition_root: true,
        };
        let before = slot(100, 200);
        assert!(slot_unchanged_by_others(&before, &slot(100, 200)));
        // Peek decoded a running-xacts record before the confirmed point.
        assert!(slot_unchanged_by_others(&before, &slot(180, 200)));
        assert!(slot_unchanged_by_others(&before, &slot(200, 200)));
        // Another consumer confirmed, or the slot regressed/jumped past confirmation.
        assert!(!slot_unchanged_by_others(&before, &slot(100, 250)));
        assert!(!slot_unchanged_by_others(&before, &slot(90, 200)));
        assert!(!slot_unchanged_by_others(&before, &slot(260, 200)));
    }

    #[test]
    fn retained_wal_is_not_the_same_as_decodable_history() {
        let s = SlotStatus {
            protocol_version: 4,
            restart_lsn: 10,
            confirmed_flush_lsn: 20,
            safe_wal_size: Some(100),
            publish_via_partition_root: true,
        };
        assert_eq!(s.earliest(), 20);
        assert!(s.require_position(19).is_err());
        assert!(s.require_position(20).is_ok());
    }

    fn budget(rows: u64, bytes: u64) -> ReadBudget {
        ReadBudget {
            max_chunk_rows: rows,
            max_chunk_bytes: bytes,
            ..ReadBudget::default()
        }
    }

    /// One-change transactions at commit LSNs 10, 20, ... ending 5 later.
    fn feed(c: &mut Collect<u64>, txns: &[u64]) -> Flow {
        for &commit in txns {
            assert_eq!(c.received(10).unwrap(), Flow::Continue);
            assert_eq!(c.begin(commit).unwrap(), Flow::Continue);
            if c.received(10).unwrap() == Flow::Stop
                || c.event(|| Ok(commit)).unwrap() == Flow::Stop
            {
                return Flow::Stop;
            }
            c.received(10).unwrap();
            if c.commit(commit, commit + 5).unwrap() == Flow::Stop {
                return Flow::Stop;
            }
        }
        Flow::Continue
    }

    fn done(c: Collect<u64>, limit: u64) -> Option<(u64, Vec<u64>)> {
        match c.finish(limit).unwrap() {
            Collected::Done(done) => done,
            Collected::Retry => panic!("unexpected retry"),
        }
    }

    #[test]
    fn small_transactions_share_one_batch_up_to_the_target() {
        let mut c = Collect::new(0, 3, &budget(100, 1 << 20));
        assert_eq!(feed(&mut c, &[10, 20, 30, 40]), Flow::Stop);
        // Stops at the commit that reaches the target; 40 waits for the next batch.
        assert_eq!(done(c, 1000), Some((35, vec![10, 20, 30])));
        let mut c = Collect::new(0, 10, &budget(100, 1 << 20));
        assert_eq!(feed(&mut c, &[10, 20]), Flow::Continue);
        assert_eq!(done(c, 1000), Some((25, vec![10, 20])));
    }

    #[test]
    fn transactions_before_the_cursor_are_skipped_and_never_complete_a_batch() {
        let mut c = Collect::new(25, 10, &budget(100, 1 << 20));
        feed(&mut c, &[10, 20, 30]);
        assert_eq!(done(c, 1000), Some((35, vec![30])));
        // Only skipped transactions filled the decode limit: decode again, wider.
        let mut c = Collect::new(100, 10, &budget(100, 1 << 20));
        feed(&mut c, &[10, 20]);
        assert!(matches!(c.finish(6).unwrap(), Collected::Retry));
        let mut c = Collect::new(100, 10, &budget(100, 1 << 20));
        feed(&mut c, &[10, 20]);
        assert_eq!(done(c, 1000), None);
    }

    #[test]
    fn a_transaction_that_would_overflow_the_batch_is_left_for_the_next() {
        let mut c = Collect::new(0, 100, &budget(3, 1 << 20));
        assert_eq!(c.begin(10).unwrap(), Flow::Continue);
        for i in 0..2 {
            assert_eq!(c.event(|| Ok(i)).unwrap(), Flow::Continue);
        }
        c.commit(10, 15).unwrap();
        c.begin(20).unwrap();
        c.event(|| Ok(7)).unwrap();
        // The second transaction would make the batch exceed max_chunk_rows.
        assert_eq!(c.event(|| Ok(8)).unwrap(), Flow::Stop);
        assert_eq!(done(c, 1000), Some((15, vec![0, 1])));
        // A single transaction larger than the budget still refuses.
        let mut c = Collect::new(0, 100, &budget(2, 1 << 20));
        c.begin(10).unwrap();
        c.event(|| Ok(0)).unwrap();
        c.event(|| Ok(1)).unwrap();
        assert_eq!(
            c.event(|| Ok(2)).unwrap_err().to_string(),
            SourceError::budget("capture_transaction_rows").to_string()
        );
    }

    #[test]
    fn byte_budget_stops_after_a_completed_transaction_and_refuses_before_one() {
        let mut c = Collect::new(0, 100, &budget(100, 100));
        assert_eq!(feed(&mut c, &[10]), Flow::Continue);
        assert_eq!(c.received(80).unwrap(), Flow::Stop);
        assert_eq!(done(c, 1000), Some((15, vec![10])));
        let mut c = Collect::<u64>::new(0, 100, &budget(100, 50));
        c.received(40).unwrap();
        assert!(c.received(20).is_err());
        // Half the byte budget closes the batch at the next commit.
        let mut c = Collect::new(0, 100, &budget(100, 60));
        assert_eq!(feed(&mut c, &[10]), Flow::Stop);
    }

    #[test]
    fn protocol_boundaries_are_still_enforced() {
        let mut c = Collect::<u64>::new(0, 10, &budget(10, 1 << 20));
        assert!(c.event(|| Ok(1)).is_err());
        c.begin(10).unwrap();
        assert!(c.begin(20).is_err());
        let mut c = Collect::<u64>::new(0, 10, &budget(10, 1 << 20));
        c.begin(10).unwrap();
        assert!(c.commit(11, 15).is_err());
        let mut c = Collect::<u64>::new(0, 10, &budget(10, 1 << 20));
        c.begin(10).unwrap();
        c.event(|| Ok(1)).unwrap();
        assert!(c.finish(1000).is_err());
    }

    #[test]
    fn decode_limit_covers_a_full_batch_of_single_change_transactions() {
        assert!(decode_limit(BATCH_CHANGES) >= 3 * BATCH_CHANGES);
        assert_eq!(decode_limit(u64::MAX), MAX_DECODE_LIMIT);
        assert!(i32::try_from(MAX_DECODE_LIMIT).is_ok());
    }

    #[test]
    fn image_statement_keeps_change_order_and_binds_every_value() {
        let sql = image_sql(&["integer".into(), "text".into()], 2);
        assert_eq!(
            sql,
            "SELECT CAST(v.c0 AS integer),CAST(v.c1 AS text) FROM (VALUES \
             ($1::int4,$2::text,$3::text),($4::int4,$5::text,$6::text)) AS v(o,c0,c1) ORDER BY v.o"
        );
        assert!(crate::postgres::sqltext::read_only(&sql));
        let per = IMAGE_ROWS.min(60_000 / (1600 + 1)).max(1);
        assert!(per * 1601 < 65_535);
    }

    #[test]
    fn names_cannot_inject_replication_options() {
        assert!(name("oo_slot_1"));
        for s in ["", "x,y", "\"x\"", "x';", "a.b", "X"] {
            assert!(!name(s));
        }
    }
}
