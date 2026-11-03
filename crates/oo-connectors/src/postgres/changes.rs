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
}

impl PostgresChanges {
    pub fn new(source: PgSourceConfig, config: CaptureConfig) -> Result<Self, SourceError> {
        if !name(&config.slot) || !name(&config.publication) || config.required_wal_headroom == 0 {
            return Err(SourceError::config("capture_config"));
        }
        Ok(Self {
            engine: PostgresEngine::new(source),
            config,
        })
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
             bool_or(pubviaroot), bool_or(pg_get_userbyid(pubowner)=current_user) \
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
        let rows = session::fetch_all(
            s,
            sqlx::query(
                "SELECT schemaname::text, tablename::text, to_jsonb(p)->>'rowfilter' \
             FROM pg_publication_tables p WHERE pubname=$1",
            )
            .bind(&self.config.publication),
        )
        .await?;
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
        let mut stream = sqlx::query(
            "SELECT data FROM pg_logical_slot_peek_binary_changes($1,$2::text::pg_lsn,NULL, \
             'proto_version',$3,'publication_names',$4)",
        )
        .bind(&self.config.slot)
        .bind(lsn::format_lsn(upper))
        .bind(status.protocol_version.to_string())
        .bind(format!("\"{}\"", self.config.publication))
        .fetch(&mut s.conn);
        let mut relations = BTreeMap::new();
        let mut open = None;
        let mut events = Vec::new();
        let mut bytes = 0u64;
        let mut completed = None;
        while let Some(row) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            let row = row.map_err(crate::error::map_sqlx)?;
            let data: Vec<u8> = row
                .try_get(0)
                .map_err(|_| SourceError::protocol("pgoutput_data"))?;
            bytes = bytes.saturating_add(data.len() as u64);
            if bytes > budget.max_chunk_bytes {
                return Err(SourceError::budget("capture_replay_bytes"));
            }
            match wire::decode(&data).map_err(SourceError::protocol)? {
                Message::Begin { final_lsn, .. } => {
                    if open.replace(final_lsn).is_some() {
                        return Err(SourceError::protocol("pg_nested_transaction"));
                    }
                }
                Message::Relation(r) => {
                    relations.insert(r.oid, std::sync::Arc::new(r));
                }
                Message::Event {
                    kind,
                    oid,
                    old,
                    new,
                } => {
                    let final_lsn =
                        open.ok_or_else(|| SourceError::protocol("pg_event_boundary"))?;
                    if final_lsn >= from {
                        if events.len() as u64 >= budget.max_chunk_rows {
                            return Err(SourceError::budget("capture_transaction_rows"));
                        }
                        let relation = relations
                            .get(&oid)
                            .ok_or_else(|| SourceError::protocol("pg_relation"))?
                            .clone();
                        events.push((kind, relation, old, new));
                    }
                }
                Message::Commit { commit, end, .. } => {
                    if open.take() != Some(commit) || end <= commit {
                        return Err(SourceError::protocol("pg_commit_boundary"));
                    }
                    if end > from && !events.is_empty() {
                        completed = Some(end);
                        break;
                    }
                    events.clear();
                }
            }
        }
        drop(stream);
        if open.is_some() {
            return Err(SourceError::protocol("pg_incomplete_transaction"));
        }
        let Some(end) = completed else {
            return Ok(None);
        };
        let catalog = s.collect_catalog().await?;
        let mut changes = Vec::new();
        let mut images = Vec::new();
        let mut image_bytes = 0u64;
        let mut checked_relations = BTreeSet::new();
        for (kind, relation, old, new) in events {
            let table = TableRef {
                schema: relation.schema.clone(),
                name: relation.table.clone(),
            };
            self.engine.selected(&table)?;
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
            let shape = catalog
                .tables
                .iter()
                .find(|t| t.schema == table.schema && t.name == table.name)
                .ok_or_else(|| SourceError::catalog("pg_relation_drift"))?;
            if shape.columns.len() != relation.columns.len()
                || shape
                    .columns
                    .iter()
                    .zip(&relation.columns)
                    .any(|(a, b)| a.name != b.0 || a.type_oid != i64::from(b.1) || a.typmod != b.2)
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
            let image = image(
                s,
                shape,
                &relation,
                cells,
                &catalog.schema_hash,
                kind == b'D',
            )
            .await?;
            image_bytes = image_bytes.saturating_add(image.get_array_memory_size() as u64);
            if image_bytes > budget.max_chunk_bytes {
                return Err(SourceError::budget("capture_image_bytes"));
            }
            images.push(image);
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
        // A concurrent external consumer must not advance/invalidate our slot.
        let after = self.inspect(s).await?;
        if after.confirmed_flush_lsn != status.confirmed_flush_lsn
            || after.restart_lsn != status.restart_lsn
        {
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

async fn image(
    s: &mut PgSession,
    shape: &crate::SourceTable,
    r: &Relation,
    cells: &[Cell],
    schema_hash: &str,
    delete: bool,
) -> Result<RecordBatch, SourceError> {
    if cells.len() != shape.columns.len() {
        return Err(SourceError::protocol("pg_tuple_shape"));
    }
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
    let mut projections = Vec::new();
    for (i, c) in shape.columns.iter().enumerate() {
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
        projections.push(format!("CAST(CAST(${} AS text) AS {})", i + 1, ty));
    }
    let sql = format!("SELECT {}", projections.join(","));
    let mut q = sqlx::query(sqlx_core::sql_str::AssertSqlSafe(sql));
    for cell in cells {
        q = q.bind(match cell {
            Cell::Text(v) => Some(v.clone()),
            Cell::Null => None,
            Cell::Unchanged => {
                return Err(SourceError::protocol("rebaseline_required_unchanged_toast"));
            }
        });
    }
    // Convert the logged value, never re-read a later source row.
    let row = session::fetch_control(s, q).await?;
    let (decoded, _) = crate::bounded_read::decode_row(&plans, &row)?;
    // Deletes may omit non-key NOT NULL columns. Their image is not an upsert.
    let mut plans = plans;
    if delete {
        for p in &mut plans {
            p.mapping.nullable = true;
        }
    }
    let binding = RunBinding {
        source_id: s.source_id.clone(),
        schema: r.schema.clone(),
        table: r.table.clone(),
        run_generation: 0,
        input_digest: Sha256::digest(schema_hash.as_bytes()).into(),
    };
    let batch = assemble_batch(&binding, plans, &keys, vec![decoded])?;
    if !batch.rejects.is_empty() {
        return Err(SourceError::protocol("capture_rejected_cell"));
    }
    batch.record_batch()
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

    #[test]
    fn names_cannot_inject_replication_options() {
        assert!(name("oo_slot_1"));
        for s in ["", "x,y", "\"x\"", "x';", "a.b", "X"] {
            assert!(!name(s));
        }
    }
}
